pub mod models;
pub mod preferences;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use double_love_engine::{OperationResult, ProjectStore, ProjectSummary, TaskRegistry};
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

#[derive(Clone)]
pub struct OpenProject {
    pub summary: ProjectSummary,
    pub store: Arc<Mutex<ProjectStore>>,
}

#[derive(Default)]
pub struct OpenProjectSlot {
    project: Mutex<Option<OpenProject>>,
}

impl OpenProjectSlot {
    pub fn install(
        &self,
        project: OpenProject,
    ) -> Result<Option<OpenProject>, DesktopServiceError> {
        let mut current = self
            .project
            .lock()
            .map_err(|_| DesktopServiceError::internal("open-project state lock is unavailable"))?;
        Ok(current.replace(project))
    }

    pub fn current(&self) -> Result<OpenProject, DesktopServiceError> {
        self.project
            .lock()
            .map_err(|_| DesktopServiceError::internal("open-project state lock is unavailable"))?
            .clone()
            .ok_or_else(|| {
                DesktopServiceError::new(PROJECT_NOT_OPEN, "no desktop project is currently open")
            })
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
    ) -> Result<Option<OpenProject>, DesktopServiceError> {
        let navigation = HistoryNavigation {
            project_id: Some(summary.project_id.clone()),
            last_actual_revision: summary.revision,
            current_snapshot_revision: summary.revision,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        let replaced = self.open_project.install(OpenProject { summary, store })?;
        *self.history_navigation.lock().map_err(|_| {
            DesktopServiceError::internal("history navigation state lock is unavailable")
        })? = navigation;
        Ok(replaced)
    }
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

/// Registers the Phase 4 Slice 1 commands on the host-neutral service registry.
pub fn register_commands(registry: &mut CommandRegistry) {
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

        let error = match service.state().open_project().current() {
            Ok(_) => panic!("project should start closed"),
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
            service.state().open_project().current()?.summary.project_id,
            first_summary.project_id
        );

        let (second_root, second_summary, second_store) = open_project("second-project")?;
        let replaced = service
            .state()
            .install_project(second_summary.clone(), Arc::new(Mutex::new(second_store)))?
            .expect("second install should replace the first project");
        assert_eq!(replaced.summary.project_id, first_summary.project_id);
        assert_eq!(
            service.state().open_project().current()?.summary.project_id,
            second_summary.project_id
        );
        assert_eq!(
            service
                .state()
                .history_navigation()
                .lock()
                .expect("history navigation")
                .project_id
                .as_deref(),
            Some(second_summary.project_id.as_str())
        );

        drop(replaced);
        drop(service);
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
    fn crate_manifest_has_no_desktop_framework_dependency() {
        // Grep-level boundary guard: the service cannot depend on Tauri or Electron.
        let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("read service manifest")
            .to_lowercase();
        assert!(!manifest.contains("tauri"));
        assert!(!manifest.contains("electron"));
    }
}
