pub mod models;
pub mod preferences;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use double_love_engine::{
    CanvasSpec, Diagnostic, DiagnosticLevel, FfmpegTools, MediaAssetSummary, OperationResult,
    ProjectStore, ProjectSummary, SubtitleStyle, TaskRegistry, create_project,
    import_media as engine_import_media, list_media_assets, open_project,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const ENGINE_VERSION: &str = double_love_engine::ENGINE_VERSION;
pub const UNKNOWN_COMMAND: &str = "UNKNOWN_COMMAND";
pub const INVALID_PARAMS: &str = "INVALID_PARAMS";
pub const HOST_UNAVAILABLE: &str = "HOST_UNAVAILABLE";
pub const INTERNAL: &str = "INTERNAL";
pub const PROJECT_NOT_OPEN: &str = "PROJECT_NOT_OPEN";
pub const APP_DATA_DIR_REQUIRED: &str = "APP_DATA_DIR_REQUIRED";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct DesktopServiceError {
    pub code: &'static str,
    pub message: String,
}

impl DesktopServiceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn host_unavailable(message: impl Into<String>) -> Self {
        Self::new(HOST_UNAVAILABLE, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(INTERNAL, message)
    }
}

/// Host-neutral event output used by future progress, model, and preference slices.
pub trait DesktopEventSink: Send + Sync {
    fn emit(&self, channel: &str, payload: Value) -> Result<(), DesktopServiceError>;
}

pub struct OpenProject {
    summary: ProjectSummary,
    store: Arc<Mutex<ProjectStore>>,
}

impl OpenProject {
    pub fn summary(&self) -> &ProjectSummary {
        &self.summary
    }
}

#[derive(Default)]
pub struct OpenProjectSlot {
    project: Mutex<Option<OpenProject>>,
}

impl OpenProjectSlot {
    pub fn with_current<T>(
        &self,
        operation: impl FnOnce(&OpenProject) -> T,
    ) -> Result<T, DesktopServiceError> {
        let current = self
            .project
            .lock()
            .map_err(|_| DesktopServiceError::internal("open-project state lock is unavailable"))?;
        let open = current.as_ref().ok_or_else(|| {
            DesktopServiceError::new(PROJECT_NOT_OPEN, "no desktop project is currently open")
        })?;
        Ok(operation(open))
    }

    pub fn with_store<T>(
        &self,
        operation: impl FnOnce(&ProjectStore, &ProjectSummary) -> T,
    ) -> Result<T, DesktopServiceError> {
        self.with_current(|open| {
            let store = open
                .store
                .lock()
                .map_err(|_| DesktopServiceError::internal("project store lock is unavailable"))?;
            Ok(operation(&store, &open.summary))
        })?
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryNavigation {
    pub project_id: Option<String>,
    pub last_actual_revision: u64,
    pub current_snapshot_revision: u64,
    pub undo: Vec<u64>,
    pub redo: Vec<u64>,
}

pub struct DesktopState {
    app_data_dir: PathBuf,
    open_project: OpenProjectSlot,
    task_registry: TaskRegistry,
    preferences: preferences::PreferencesState,
    models: models::ModelState,
    history_navigation: Mutex<HistoryNavigation>,
}

impl DesktopState {
    fn new(app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            open_project: OpenProjectSlot::default(),
            task_registry: TaskRegistry::new(),
            preferences: preferences::PreferencesState::default(),
            models: models::ModelState::default(),
            history_navigation: Mutex::new(HistoryNavigation::default()),
        }
    }

    pub fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    pub fn open_project(&self) -> &OpenProjectSlot {
        &self.open_project
    }

    pub fn task_registry(&self) -> &TaskRegistry {
        &self.task_registry
    }

    pub fn preferences(&self) -> &preferences::PreferencesState {
        &self.preferences
    }

    pub fn models(&self) -> &models::ModelState {
        &self.models
    }

    pub fn history_navigation(&self) -> &Mutex<HistoryNavigation> {
        &self.history_navigation
    }

    pub fn install_project(
        &self,
        summary: ProjectSummary,
        store: Arc<Mutex<ProjectStore>>,
    ) -> Result<Option<ProjectSummary>, DesktopServiceError> {
        let mut project =
            self.open_project.project.lock().map_err(|_| {
                DesktopServiceError::internal("open-project state lock is unavailable")
            })?;
        let store_guard = store
            .lock()
            .map_err(|_| DesktopServiceError::internal("project store lock is unavailable"))?;
        let navigation = history_navigation_for_store(&store_guard, &summary.project_id)
            .map_err(|error| DesktopServiceError::new("STORAGE_ERROR", error))?;
        let mut history = self.history_navigation.lock().map_err(|_| {
            DesktopServiceError::internal("history navigation state lock is unavailable")
        })?;
        drop(store_guard);
        let replaced = project
            .replace(OpenProject { summary, store })
            .map(|open| open.summary);
        *history = navigation;
        Ok(replaced)
    }
}

fn history_navigation_for_store(
    store: &ProjectStore,
    project_id: &str,
) -> Result<HistoryNavigation, String> {
    let revision = store.revision().map_err(|error| error.to_string())?;
    let undo = store
        .revision_history(10_000)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| entry.restorable && entry.revision < revision)
        .map(|entry| entry.revision)
        .rev()
        .collect();
    Ok(HistoryNavigation {
        project_id: Some(project_id.to_string()),
        last_actual_revision: revision,
        current_snapshot_revision: revision,
        undo,
        redo: Vec::new(),
    })
}

type CommandHandler = dyn Fn(&DesktopState, &dyn DesktopEventSink, Value) -> Result<Value, DesktopServiceError>
    + Send
    + Sync;

#[derive(Default)]
pub struct CommandRegistry {
    handlers: HashMap<String, Box<CommandHandler>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        handler: impl Fn(
            &DesktopState,
            &dyn DesktopEventSink,
            Value,
        ) -> Result<Value, DesktopServiceError>
        + Send
        + Sync
        + 'static,
    ) {
        self.handlers.insert(name.into(), Box::new(handler));
    }

    fn invoke(
        &self,
        state: &DesktopState,
        event_sink: &dyn DesktopEventSink,
        name: &str,
        payload: Value,
    ) -> Result<Value, DesktopServiceError> {
        if name.trim().is_empty() {
            return Err(DesktopServiceError::invalid_params(
                "invoke command name must be non-blank",
            ));
        }
        let handler = self.handlers.get(name).ok_or_else(|| {
            DesktopServiceError::new(UNKNOWN_COMMAND, format!("unknown command: {name}"))
        })?;
        handler(state, event_sink, payload)
    }
}

fn result_value<T: Serialize>(result: OperationResult<T>) -> Result<Value, DesktopServiceError> {
    serde_json::to_value(result).map_err(|error| DesktopServiceError::internal(error.to_string()))
}

fn params<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, DesktopServiceError> {
    serde_json::from_value(payload)
        .map_err(|error| DesktopServiceError::invalid_params(error.to_string()))
}

fn renderer_path_replacements(paths: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    let mut replacements = Vec::with_capacity(paths.len() * 2);
    for &(path, placeholder) in paths {
        if path.is_empty() {
            continue;
        }
        if let Ok(canonical) = Path::new(path).canonicalize() {
            replacements.push((canonical.to_string_lossy().into_owned(), placeholder));
        }
        replacements.push((path.to_string(), placeholder));
    }
    replacements
}

fn sanitize_renderer_diagnostics<T>(
    mut result: OperationResult<T>,
    replacements: &[(String, &str)],
) -> OperationResult<T> {
    for diagnostic in &mut result.diagnostics {
        sanitize_renderer_text(&mut diagnostic.cause, replacements);
        sanitize_renderer_text(&mut diagnostic.impact, replacements);
        if let Some(action) = &mut diagnostic.suggested_action {
            sanitize_renderer_text(action, replacements);
        }
    }
    result
}

fn sanitize_renderer_text(text: &mut String, replacements: &[(String, &str)]) {
    for (sensitive, placeholder) in replacements {
        if !sensitive.is_empty() && text.contains(sensitive) {
            *text = text.replace(sensitive, placeholder);
        }
    }
}

#[derive(Deserialize)]
struct ProjectPathParams {
    path: String,
}

#[derive(Deserialize)]
struct ResolveMediaAssetParams {
    asset_id: String,
}

#[derive(Serialize)]
struct ResolvedMediaAsset {
    path: String,
}

#[derive(Default, Deserialize)]
struct ProjectHistoryParams {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ProjectRevisionParams {
    revision: u64,
}

#[derive(Deserialize)]
struct CanvasSetParams {
    canvas: CanvasSpec,
}

#[derive(Deserialize)]
struct SubtitleStyleSetParams {
    style: SubtitleStyle,
}

#[derive(Deserialize)]
struct PreferencesUpdateParams {
    patch: preferences::PreferencesPatch,
}

#[derive(Deserialize)]
struct RecentProjectForgetParams {
    root: String,
}

#[derive(Default, Deserialize)]
struct OnboardingCompleteParams {
    #[serde(default, alias = "defaultAsrModel")]
    default_asr_model: Option<String>,
    #[serde(default)]
    step: Option<u8>,
}

fn with_store<T>(
    state: &DesktopState,
    operation: impl FnOnce(&ProjectStore, &ProjectSummary) -> OperationResult<T>,
) -> OperationResult<T> {
    match state.open_project().with_store(operation) {
        Ok(result) => result,
        Err(error) if error.code == PROJECT_NOT_OPEN => {
            OperationResult::failed(PROJECT_NOT_OPEN, "请先打开或创建一个项目。")
        }
        Err(error) => OperationResult::failed(error.code, error.message),
    }
}

fn warning(
    result: &mut OperationResult<ProjectSummary>,
    code: &str,
    cause: impl Into<String>,
    impact: &str,
    suggested_action: &str,
) {
    result.diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: code.to_string(),
        cause: cause.into(),
        object_id: None,
        impact: impact.to_string(),
        blocks_export: false,
        suggested_action: Some(suggested_action.to_string()),
    });
}

fn install_open_project(
    state: &DesktopState,
    summary: &ProjectSummary,
) -> Result<(), DesktopServiceError> {
    let store = ProjectStore::open(Path::new(&summary.database))
        .map_err(|error| DesktopServiceError::new("STORAGE_ERROR", error.to_string()))?;
    state
        .install_project(summary.clone(), Arc::new(Mutex::new(store)))
        .map(|_| ())
}

fn record_recent_project_warning(
    state: &DesktopState,
    events: &dyn DesktopEventSink,
    summary: &ProjectSummary,
    result: &mut OperationResult<ProjectSummary>,
    created: bool,
) {
    if let Err(error) = preferences::record_recent_project(
        state.app_data_dir(),
        state.preferences(),
        events,
        summary,
    ) {
        warning(
            result,
            "RECENT_PROJECT_WRITE_FAILED",
            error.to_string(),
            if created {
                "项目已创建，但最近项目列表未更新。"
            } else {
                "项目已打开，但最近项目列表未更新。"
            },
            "稍后在设置中重新打开项目列表。",
        );
    }
}

fn reset_history_navigation(
    navigation: &mut HistoryNavigation,
    store: &ProjectStore,
    project_id: &str,
) -> Result<(), String> {
    *navigation = history_navigation_for_store(store, project_id)?;
    Ok(())
}

/// Registers the migrated host-neutral desktop commands.
pub fn register_commands(registry: &mut CommandRegistry) {
    registry.register("project_create", |state, events, payload| {
        let params: ProjectPathParams = params(payload)?;
        let result = match create_project(Path::new(&params.path)) {
            Ok(mut summary) => {
                let style_warning = match preferences::current_preferences(
                    state.app_data_dir(),
                    state.preferences(),
                ) {
                    Ok(preferences) => match ProjectStore::open(Path::new(&summary.database))
                        .and_then(|store| {
                            store.set_subtitle_style(&preferences.default_subtitle_style)
                        }) {
                        Ok(revision) => {
                            summary.revision = revision;
                            None
                        }
                        Err(error) => Some(error.to_string()),
                    },
                    Err(error) => Some(error.to_string()),
                };
                match install_open_project(state, &summary) {
                    Ok(()) => {
                        let mut result = OperationResult::success(summary.clone());
                        if let Some(error) = style_warning {
                            warning(
                                &mut result,
                                "DEFAULT_SUBTITLE_STYLE_FAILED",
                                error,
                                "项目已创建，但使用了内置字幕默认值。",
                                "可在编辑器中重新设置字幕样式。",
                            );
                        }
                        record_recent_project_warning(state, events, &summary, &mut result, true);
                        result
                    }
                    Err(error) => OperationResult::failed("STORAGE_ERROR", error.message),
                }
            }
            Err(error) => OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string()),
        };
        result_value(result)
    });
    registry.register("project_open", |state, events, payload| {
        let params: ProjectPathParams = params(payload)?;
        let result = match open_project(Path::new(&params.path)) {
            Ok(summary) => match install_open_project(state, &summary) {
                Ok(()) => {
                    let mut result = OperationResult::success(summary.clone());
                    record_recent_project_warning(state, events, &summary, &mut result, false);
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.message),
            },
            Err(error) => OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string()),
        };
        result_value(result)
    });
    registry.register("import_media", |state, _events, payload| {
        let params: ProjectPathParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let replacements = renderer_path_replacements(&[
                (&params.path, "<SELECTED_MEDIA>"),
                (&summary.root, "<PROJECT>"),
            ]);
            let result = match FfmpegTools::discover() {
                Ok(tools) => {
                    let prepared_dir = Path::new(&summary.root).join(".doublelove/prepared");
                    engine_import_media(store, &prepared_dir, &tools, Path::new(&params.path))
                }
                Err(diagnostic) => {
                    let mut result: OperationResult<MediaAssetSummary> =
                        OperationResult::failed(&diagnostic.code, &diagnostic.cause);
                    result.diagnostics[0].suggested_action = diagnostic.suggested_action.clone();
                    result
                }
            };
            sanitize_renderer_diagnostics(result, &replacements)
        }))
    });
    registry.register("assets_list", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let replacements = renderer_path_replacements(&[(&summary.root, "<PROJECT>")]);
            sanitize_renderer_diagnostics(list_media_assets(store), &replacements)
        }))
    });
    registry.register("resolve_media_asset", |state, _events, payload| {
        let params: ResolveMediaAssetParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.media_asset(&params.asset_id) {
                Ok(Some(asset)) if Path::new(&asset.original_path).is_file() => {
                    OperationResult::success(ResolvedMediaAsset {
                        path: asset.original_path,
                    })
                }
                Ok(Some(_)) | Ok(None) => OperationResult::failed(
                    "MEDIA_ASSET_NOT_FOUND",
                    "媒体资产不存在或源文件已不可用。",
                ),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register("project_revision", |state, _events, _payload| {
        result_value(with_store(state, |store, _| match store.revision() {
            Ok(revision) => OperationResult::success(revision),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }))
    });
    registry.register("project_history", |state, _events, payload| {
        let params: ProjectHistoryParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.revision_history(params.limit.unwrap_or(80)) {
                Ok(history) => OperationResult::success(history),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register("project_restore_revision", |state, _events, payload| {
        let params: ProjectRevisionParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.restore_revision(params.revision) {
                Ok(new_revision) => {
                    let mut result = OperationResult::success(serde_json::json!({
                        "restored_revision": params.revision,
                        "revision": new_revision,
                    }));
                    result.revision = Some(new_revision);
                    result
                }
                Err(error) => OperationResult::failed("HISTORY_RESTORE_FAILED", error.to_string()),
            }
        }))
    });
    registry.register("edit_undo", |state, _events, _payload| {
        let result = with_store(state, |store, summary| {
            let actual_revision = match store.revision() {
                Ok(revision) => revision,
                Err(error) => {
                    return OperationResult::failed("STORAGE_ERROR", error.to_string());
                }
            };
            let mut navigation = match state.history_navigation().lock() {
                Ok(navigation) => navigation,
                Err(_) => {
                    return OperationResult::failed(
                        INTERNAL,
                        "history navigation state lock is unavailable",
                    );
                }
            };
            let navigation_is_stale = navigation.project_id.as_deref()
                != Some(summary.project_id.as_str())
                || navigation.last_actual_revision != actual_revision;
            if navigation_is_stale
                && let Err(error) =
                    reset_history_navigation(&mut navigation, store, &summary.project_id)
            {
                return OperationResult::failed("HISTORY_READ_FAILED", error);
            }
            let Some(target) = navigation.undo.pop() else {
                return OperationResult::failed("HISTORY_UNDO_EMPTY", "没有更早的编辑版本。");
            };
            let previous = navigation.current_snapshot_revision;
            match store.restore_revision(target) {
                Ok(revision) => {
                    navigation.redo.push(previous);
                    navigation.current_snapshot_revision = target;
                    navigation.last_actual_revision = revision;
                    let mut result = OperationResult::success(());
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("HISTORY_UNDO_FAILED", error.to_string()),
            }
        });
        result_value(result)
    });
    registry.register("edit_redo", |state, _events, _payload| {
        let result = with_store(state, |store, summary| {
            let actual_revision = match store.revision() {
                Ok(revision) => revision,
                Err(error) => {
                    return OperationResult::failed("STORAGE_ERROR", error.to_string());
                }
            };
            let mut navigation = match state.history_navigation().lock() {
                Ok(navigation) => navigation,
                Err(_) => {
                    return OperationResult::failed(
                        INTERNAL,
                        "history navigation state lock is unavailable",
                    );
                }
            };
            if navigation.project_id.as_deref() != Some(summary.project_id.as_str())
                || navigation.last_actual_revision != actual_revision
            {
                return OperationResult::failed("HISTORY_REDO_EMPTY", "没有可以重做的编辑版本。");
            }
            let Some(target) = navigation.redo.pop() else {
                return OperationResult::failed("HISTORY_REDO_EMPTY", "没有可以重做的编辑版本。");
            };
            let previous = navigation.current_snapshot_revision;
            match store.restore_revision(target) {
                Ok(revision) => {
                    navigation.undo.push(previous);
                    navigation.current_snapshot_revision = target;
                    navigation.last_actual_revision = revision;
                    let mut result = OperationResult::success(());
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("HISTORY_REDO_FAILED", error.to_string()),
            }
        });
        result_value(result)
    });
    registry.register("canvas_get", |state, _events, _payload| {
        result_value(with_store(state, |store, _| match store.canvas_spec() {
            Ok(canvas) => OperationResult::success(canvas),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }))
    });
    registry.register("canvas_set", |state, _events, payload| {
        let params: CanvasSetParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.set_canvas_spec(&params.canvas) {
                Ok(revision) => {
                    let mut result = OperationResult::success(params.canvas);
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register("subtitle_style_get", |state, _events, _payload| {
        result_value(with_store(state, |store, _| match store.subtitle_style() {
            Ok(style) => OperationResult::success(style),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }))
    });
    registry.register("subtitle_style_set", |state, _events, payload| {
        let params: SubtitleStyleSetParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.set_subtitle_style(&params.style) {
                Ok(revision) => {
                    let mut result = OperationResult::success(params.style);
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register("preferences_get", |state, _events, _payload| {
        result_value(preferences::preferences_get(
            state.app_data_dir(),
            state.preferences(),
        ))
    });
    registry.register("preferences_update", |state, events, payload| {
        let params: PreferencesUpdateParams = params(payload)?;
        if let Some(model_root) = params.patch.model_root.as_deref() {
            let current =
                match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                    Ok(current) => current,
                    Err(error) => {
                        return result_value(preferences::command_error::<
                            preferences::AppPreferencesV1,
                        >(error, true));
                    }
                };
            if let Err(error) = state
                .models()
                .migrate_root(Path::new(&current.model_root), Path::new(model_root))
            {
                return result_value(OperationResult::<preferences::AppPreferencesV1>::failed(
                    "MODEL_ROOT_MIGRATION_FAILED",
                    error,
                ));
            }
        }
        result_value(preferences::preferences_update(
            state.app_data_dir(),
            state.preferences(),
            events,
            params.patch,
        ))
    });
    registry.register("recent_projects_list", |state, _events, _payload| {
        result_value(preferences::recent_projects_list(
            state.app_data_dir(),
            state.preferences(),
        ))
    });
    registry.register("recent_project_forget", |state, events, payload| {
        let params: RecentProjectForgetParams = params(payload)?;
        result_value(preferences::recent_project_forget(
            state.app_data_dir(),
            state.preferences(),
            events,
            params.root,
        ))
    });
    registry.register("system_profile", |state, _events, _payload| {
        result_value(preferences::system_profile(
            state.app_data_dir(),
            state.preferences(),
        ))
    });
    registry.register("onboarding_get", |state, _events, _payload| {
        result_value(preferences::onboarding_get(
            state.app_data_dir(),
            state.preferences(),
        ))
    });
    registry.register("onboarding_complete", |state, events, payload| {
        let params = if payload.is_null() {
            OnboardingCompleteParams::default()
        } else {
            params(payload)?
        };
        result_value(preferences::onboarding_complete(
            state.app_data_dir(),
            state.preferences(),
            events,
            params.default_asr_model,
            params.step,
        ))
    });
    registry.register("onboarding_reset", |state, events, _payload| {
        result_value(preferences::onboarding_reset(
            state.app_data_dir(),
            state.preferences(),
            events,
        ))
    });
    registry.register("model_catalog", |state, _events, _payload| {
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        Vec<double_love_engine::ModelDescriptorWithInstallation>,
                    >(error, true));
                }
            };
        let result = match state.models().snapshot(Path::new(&preferences.model_root)) {
            Ok(snapshot) => OperationResult::success(snapshot),
            Err(error) => OperationResult::failed("MODEL_CATALOG_FAILED", error),
        };
        result_value(result)
    });
}

pub struct DesktopService {
    state: DesktopState,
    commands: CommandRegistry,
    event_sink: Arc<dyn DesktopEventSink>,
}

impl DesktopService {
    pub fn new(
        app_data_dir: Option<PathBuf>,
        event_sink: Arc<dyn DesktopEventSink>,
    ) -> Result<Self, DesktopServiceError> {
        Self::with_registry(app_data_dir, event_sink, CommandRegistry::new())
    }

    pub fn with_registry(
        app_data_dir: Option<PathBuf>,
        event_sink: Arc<dyn DesktopEventSink>,
        commands: CommandRegistry,
    ) -> Result<Self, DesktopServiceError> {
        let app_data_dir = app_data_dir
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| {
                DesktopServiceError::new(
                    APP_DATA_DIR_REQUIRED,
                    "desktop host requires an explicit --app-data-dir; no default is inferred",
                )
            })?;
        Ok(Self {
            state: DesktopState::new(app_data_dir),
            commands,
            event_sink,
        })
    }

    pub fn state(&self) -> &DesktopState {
        &self.state
    }

    pub fn event_sink(&self) -> &dyn DesktopEventSink {
        self.event_sink.as_ref()
    }

    pub fn invoke(&self, name: &str, payload: Value) -> Result<Value, DesktopServiceError> {
        self.commands
            .invoke(&self.state, self.event_sink.as_ref(), name, payload)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use double_love_engine::{ProjectStore, create_project};

    use super::*;

    #[derive(Default)]
    struct TestEventSink;

    impl DesktopEventSink for TestEventSink {
        fn emit(&self, _channel: &str, _payload: Value) -> Result<(), DesktopServiceError> {
            Ok(())
        }
    }

    fn temp_directory(label: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "double-love-desktop-service-{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn open_project(
        label: &str,
    ) -> Result<(PathBuf, ProjectSummary, ProjectStore), Box<dyn Error>> {
        let root = temp_directory(label);
        let summary = create_project(&root)?;
        let store = ProjectStore::open(Path::new(&summary.database))?;
        Ok((root, summary, store))
    }

    #[test]
    fn open_project_slot_installs_replaces_and_reports_not_open() -> Result<(), Box<dyn Error>> {
        let app_data_dir = temp_directory("app-data");
        let service = DesktopService::new(Some(app_data_dir.clone()), Arc::new(TestEventSink))?;
        assert_eq!(service.state().app_data_dir(), app_data_dir);

        let error = match service.state().open_project().with_current(|_| ()) {
            Ok(()) => panic!("project should start closed"),
            Err(error) => error,
        };
        assert_eq!(error.code, PROJECT_NOT_OPEN);

        let (first_root, first_summary, first_store) = open_project("first-project")?;
        assert!(
            service
                .state()
                .install_project(first_summary.clone(), Arc::new(Mutex::new(first_store)))?
                .is_none()
        );
        assert_eq!(
            service
                .state()
                .open_project()
                .with_current(|open| open.summary().project_id.clone())?,
            first_summary.project_id
        );

        let (second_root, second_summary, second_store) = open_project("second-project")?;
        second_store.set_canvas_spec(&CanvasSpec::default())?;
        second_store.set_subtitle_style(&SubtitleStyle::default())?;
        let replaced = service
            .state()
            .install_project(second_summary.clone(), Arc::new(Mutex::new(second_store)))?
            .expect("second install should replace the first project");
        assert_eq!(replaced.project_id, first_summary.project_id);
        assert_eq!(
            service
                .state()
                .open_project()
                .with_current(|open| open.summary().project_id.clone())?,
            second_summary.project_id
        );
        let navigation = service
            .state()
            .history_navigation()
            .lock()
            .expect("history navigation");
        assert_eq!(
            navigation.project_id.as_deref(),
            Some(second_summary.project_id.as_str())
        );
        assert_eq!(navigation.last_actual_revision, 2);
        assert_eq!(navigation.current_snapshot_revision, 2);
        assert_eq!(navigation.undo, vec![1]);
        assert!(navigation.redo.is_empty());
        drop(navigation);

        drop(replaced);
        drop(service);
        fs::remove_dir_all(first_root)?;
        fs::remove_dir_all(second_root)?;
        Ok(())
    }

    #[test]
    fn project_replacement_waits_for_in_flight_operation_and_resets_history_atomically()
    -> Result<(), Box<dyn Error>> {
        let state = Arc::new(DesktopState::new(temp_directory("atomic-app-data")));
        let (first_root, first_summary, first_store) = open_project("atomic-first")?;
        state.install_project(first_summary.clone(), Arc::new(Mutex::new(first_store)))?;

        let (second_root, second_summary, second_store) = open_project("atomic-second")?;
        second_store.set_canvas_spec(&CanvasSpec::default())?;
        second_store.set_subtitle_style(&SubtitleStyle::default())?;
        let second_store = Arc::new(Mutex::new(second_store));

        let (operation_entered_tx, operation_entered_rx) = mpsc::channel();
        let (release_operation_tx, release_operation_rx) = mpsc::channel();
        let operation_state = Arc::clone(&state);
        let first_project_id = first_summary.project_id.clone();
        let operation = thread::spawn(move || {
            operation_state
                .open_project()
                .with_store(|store, summary| {
                    assert_eq!(summary.project_id, first_project_id);
                    operation_entered_tx
                        .send(())
                        .expect("signal operation entry");
                    release_operation_rx.recv().expect("release operation");
                    store
                        .set_canvas_spec(&CanvasSpec::default())
                        .expect("mutate first project");
                    let mut navigation = operation_state
                        .history_navigation()
                        .lock()
                        .expect("history navigation");
                    reset_history_navigation(&mut navigation, store, &summary.project_id)
                        .expect("reset first-project history");
                })
                .expect("in-flight project operation");
        });
        operation_entered_rx.recv().expect("operation entered");

        let (replacement_started_tx, replacement_started_rx) = mpsc::channel();
        let (replacement_done_tx, replacement_done_rx) = mpsc::channel();
        let replacement_state = Arc::clone(&state);
        let replacement_summary = second_summary.clone();
        let replacement = thread::spawn(move || {
            replacement_started_tx
                .send(())
                .expect("signal replacement start");
            let replaced = replacement_state
                .install_project(replacement_summary, second_store)
                .expect("install replacement project");
            replacement_done_tx
                .send(replaced)
                .expect("signal replacement completion");
        });
        replacement_started_rx
            .recv()
            .expect("replacement thread started");
        assert!(
            replacement_done_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "replacement must wait while a project operation holds the slot lock"
        );

        release_operation_tx.send(()).expect("release operation");
        operation.join().expect("join operation thread");
        let replaced = replacement_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("replacement completed after operation")
            .expect("first project was replaced");
        replacement.join().expect("join replacement thread");
        assert_eq!(replaced.project_id, first_summary.project_id);

        let final_revision = state.open_project().with_store(|store, summary| {
            assert_eq!(summary.project_id, second_summary.project_id);
            store.revision().expect("second-project revision")
        })?;
        assert_eq!(final_revision, 2);
        let navigation = state
            .history_navigation()
            .lock()
            .expect("history navigation");
        assert_eq!(
            navigation.project_id.as_deref(),
            Some(second_summary.project_id.as_str())
        );
        assert_eq!(navigation.last_actual_revision, 2);
        assert_eq!(navigation.current_snapshot_revision, 2);
        assert_eq!(navigation.undo, vec![1]);
        assert!(navigation.redo.is_empty());
        drop(navigation);

        drop(state);
        fs::remove_dir_all(first_root)?;
        fs::remove_dir_all(second_root)?;
        Ok(())
    }

    #[test]
    fn app_data_directory_must_be_injected() {
        let error = DesktopService::new(None, Arc::new(TestEventSink))
            .err()
            .expect("missing app data directory should fail");
        assert_eq!(error.code, APP_DATA_DIR_REQUIRED);
        assert!(error.message.contains("--app-data-dir"));
    }

    #[test]
    fn renderer_diagnostic_sanitization_preserves_the_operation_contract() {
        let selected = "/private/source/selected.mp4";
        let project = "/private/project";
        let mut original: OperationResult<Value> =
            OperationResult::failed("MEDIA_PROBE_FAILED", format!("probe {selected}"));
        original.revision = Some(17);
        original.data = Some(serde_json::json!({"unchanged": selected}));
        original.diagnostics[0].impact = format!("impact {project}");
        original.diagnostics[0].suggested_action = Some(format!("retry {selected} from {project}"));
        let original_counts = original.counts.clone();
        let original_data = original.data.clone();

        let sanitized = sanitize_renderer_diagnostics(
            original,
            &renderer_path_replacements(&[(selected, "<SELECTED_MEDIA>"), (project, "<PROJECT>")]),
        );

        assert_eq!(
            sanitized.status,
            double_love_engine::OperationStatus::Failed
        );
        assert_eq!(sanitized.revision, Some(17));
        assert_eq!(sanitized.data, original_data);
        assert_eq!(sanitized.counts, original_counts);
        assert_eq!(sanitized.diagnostics[0].code, "MEDIA_PROBE_FAILED");
        assert_eq!(sanitized.diagnostics[0].cause, "probe <SELECTED_MEDIA>");
        assert_eq!(sanitized.diagnostics[0].impact, "impact <PROJECT>");
        assert_eq!(
            sanitized.diagnostics[0].suggested_action.as_deref(),
            Some("retry <SELECTED_MEDIA> from <PROJECT>")
        );
    }

    #[test]
    fn crate_manifest_has_no_desktop_framework_dependency() {
        // Grep-level boundary guard: the service cannot depend on Tauri or Electron.
        let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("read service manifest")
            .to_lowercase();
        assert!(!manifest.contains("tauri"));
        assert!(!manifest.contains("electron"));
    }
}
