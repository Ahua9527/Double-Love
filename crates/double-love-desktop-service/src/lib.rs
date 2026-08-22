pub mod models;
pub mod preferences;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use double_love_engine::{
    CanvasSpec, DEFAULT_HANDLES_MS, Diagnostic, DiagnosticLevel, DiarizeConfig, DoctorEnvironment,
    FfmpegTools, FrameRate, MediaAssetSummary, ModelError, OperationResult, ProgressEvent,
    ProgressSink, ProjectExportPreview, ProjectStore, ProjectSummary, SharedSink, SpeakerIdentity,
    SpeakerNameAgentPayload, SubtitleStyle, TaskRegistry, TaskState, TranscribeConfig,
    agent_name_payload_preview, append_full_main_track_asset, append_main_track_clip,
    compile_project_timeline, create_project, export_project_ass_to, export_project_xmeml_to,
    export_rough_cut, export_rough_cut_to, ffmpeg_supports_ass_filter,
    import_media as engine_import_media, list_media_assets, local_name_proposals,
    move_main_track_clip, omit_words, open_project, preview_project_export, remove_main_track_clip,
    render_project_mp4_to, restore_words, speaker_diarization_result, split_main_track_clip,
    start_speaker_diarization, start_transcription, transcript_view, trim_main_track_clip,
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

#[derive(Debug, Clone, Default)]
pub struct DesktopRuntimeConfig {
    pub resource_dir: Option<PathBuf>,
    /// Enabled only by explicit host integration tests; production commands keep mock=false.
    pub test_transcribe_mock: bool,
    /// Enabled only by explicit host integration tests; production commands keep mock=false.
    pub test_speaker_mock: bool,
}

pub struct DesktopState {
    app_data_dir: PathBuf,
    runtime: DesktopRuntimeConfig,
    open_project: OpenProjectSlot,
    task_registry: TaskRegistry,
    preferences: preferences::PreferencesState,
    models: models::ModelState,
    history_navigation: Mutex<HistoryNavigation>,
}

impl DesktopState {
    fn new(app_data_dir: PathBuf, runtime: DesktopRuntimeConfig) -> Self {
        Self {
            app_data_dir,
            runtime,
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

type CommandHandler = dyn Fn(&DesktopState, Arc<dyn DesktopEventSink>, Value) -> Result<Value, DesktopServiceError>
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
            Arc<dyn DesktopEventSink>,
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
        event_sink: Arc<dyn DesktopEventSink>,
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
        if let Some(object_id) = &mut diagnostic.object_id {
            sanitize_renderer_text(object_id, replacements);
        }
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

const MAX_PROGRESS_TEXT_BYTES: usize = 4096;
const MIN_REDACTED_NUMERIC_ARRAY_VALUES: usize = 8;
const REDACTED_PROGRESS_TEXT: &str = "<REDACTED>";
const TRUNCATED_PROGRESS_TEXT: &str = "<TRUNCATED>";

fn numeric_array_redaction_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    debug_assert_eq!(bytes.get(open), Some(&b'['));
    let mut cursor = open + 1;
    let mut value_count = 0;

    loop {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if cursor == bytes.len() {
            return (value_count >= MIN_REDACTED_NUMERIC_ARRAY_VALUES).then_some(cursor);
        }

        let token_start = cursor;
        while let Some(byte) = bytes.get(cursor) {
            if byte.is_ascii_whitespace() || matches!(byte, b'[' | b',' | b']') {
                break;
            }
            cursor += 1;
        }
        let numeric = token_start != cursor && text[token_start..cursor].parse::<f64>().is_ok();
        if !numeric {
            return (value_count >= MIN_REDACTED_NUMERIC_ARRAY_VALUES).then_some(token_start);
        }
        value_count += 1;

        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        match bytes.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b']') if value_count >= MIN_REDACTED_NUMERIC_ARRAY_VALUES => {
                return Some(cursor + 1);
            }
            Some(b']') => return None,
            Some(_) => {
                return (value_count >= MIN_REDACTED_NUMERIC_ARRAY_VALUES).then_some(cursor);
            }
            None => {
                return (value_count >= MIN_REDACTED_NUMERIC_ARRAY_VALUES).then_some(cursor);
            }
        }
    }
}

fn redact_large_numeric_arrays(text: &mut String) {
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find('[') {
        let start = search_from + relative;
        if let Some(end) = numeric_array_redaction_end(text, start) {
            ranges.push((start, end));
            search_from = end;
        } else {
            search_from = start + 1;
        }
    }
    if ranges.is_empty() {
        return;
    }

    let mut redacted = String::with_capacity(text.len());
    let mut copy_from = 0;
    for (start, end) in ranges {
        redacted.push_str(&text[copy_from..start]);
        redacted.push_str(REDACTED_PROGRESS_TEXT);
        copy_from = end;
    }
    redacted.push_str(&text[copy_from..]);
    *text = redacted;
}

fn cap_progress_text(text: &mut String) {
    if text.len() <= MAX_PROGRESS_TEXT_BYTES {
        return;
    }
    let mut boundary = MAX_PROGRESS_TEXT_BYTES - TRUNCATED_PROGRESS_TEXT.len();
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text.push_str(TRUNCATED_PROGRESS_TEXT);
}

fn sanitize_progress_text(text: &mut String, replacements: &[(String, &str)]) {
    sanitize_renderer_text(text, replacements);
    redact_large_numeric_arrays(text);
    cap_progress_text(text);
}

fn sanitize_speaker_agent_payload(
    mut payload: SpeakerNameAgentPayload,
    replacements: &[(String, &str)],
) -> SpeakerNameAgentPayload {
    sanitize_renderer_text(&mut payload.speaker_id, replacements);
    for utterance in &mut payload.utterances {
        sanitize_renderer_text(utterance, replacements);
    }
    sanitize_renderer_text(&mut payload.instruction, replacements);
    payload
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
struct MainTrackAppendParams {
    #[serde(alias = "assetId")]
    asset_id: String,
    #[serde(alias = "sourceInFrame")]
    source_in_frame: i64,
    #[serde(alias = "sourceOutFrame")]
    source_out_frame: i64,
}

#[derive(Deserialize)]
struct MainTrackAppendFullParams {
    #[serde(alias = "assetId")]
    asset_id: String,
}

#[derive(Deserialize)]
struct MainTrackMoveParams {
    #[serde(alias = "clipId")]
    clip_id: String,
    #[serde(alias = "beforeClipId")]
    before_clip_id: Option<String>,
}

#[derive(Deserialize)]
struct MainTrackTrimParams {
    #[serde(alias = "clipId")]
    clip_id: String,
    #[serde(alias = "sourceInFrame")]
    source_in_frame: i64,
    #[serde(alias = "sourceOutFrame")]
    source_out_frame: i64,
}

#[derive(Deserialize)]
struct MainTrackSplitParams {
    #[serde(alias = "clipId")]
    clip_id: String,
    #[serde(alias = "sourceAtFrame")]
    source_at_frame: i64,
}

#[derive(Deserialize)]
struct MainTrackRemoveParams {
    #[serde(alias = "clipId")]
    clip_id: String,
}

#[derive(Deserialize)]
struct OutputRateSetParams {
    rate: Option<FrameRate>,
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

#[derive(Deserialize)]
struct ModelIdParams {
    #[serde(alias = "modelId")]
    model_id: String,
}

#[derive(Default, Deserialize)]
struct ModelRevealParams {
    #[serde(default, alias = "modelId")]
    model_id: Option<String>,
}

#[derive(Deserialize)]
struct TranscribeStartParams {
    #[serde(alias = "assetId")]
    asset_id: String,
    model: String,
    language: String,
}

#[derive(Deserialize)]
struct TaskCancelParams {
    #[serde(alias = "taskId")]
    task_id: String,
}

#[derive(Deserialize)]
struct AssetIdParams {
    #[serde(alias = "assetId")]
    asset_id: String,
}

#[derive(Deserialize)]
struct SpeakerAgentPayloadParams {
    #[serde(alias = "assetId")]
    asset_id: String,
    #[serde(alias = "speakerId")]
    speaker_id: String,
}

#[derive(Deserialize)]
struct SpeakerNameConfirmParams {
    #[serde(alias = "speakerId")]
    speaker_id: String,
    #[serde(alias = "displayName")]
    display_name: String,
    confirmed: bool,
}

#[derive(Deserialize)]
struct SpeakerMergeConfirmParams {
    #[serde(alias = "keepSpeakerId")]
    keep_speaker_id: String,
    #[serde(alias = "mergeSpeakerId")]
    merge_speaker_id: String,
    confirmed: bool,
}

#[derive(Deserialize)]
struct EditOmitParams {
    #[serde(alias = "assetId")]
    asset_id: String,
    #[serde(alias = "startOrdinal")]
    start_ordinal: i64,
    #[serde(alias = "endOrdinal")]
    end_ordinal: i64,
    #[serde(default, alias = "handlesBeforeMs")]
    handles_before_ms: Option<i64>,
    #[serde(default, alias = "handlesAfterMs")]
    handles_after_ms: Option<i64>,
}

#[derive(Deserialize)]
struct EditRestoreParams {
    #[serde(alias = "operationId")]
    operation_id: String,
    #[serde(alias = "startOrdinal")]
    start_ordinal: i64,
    #[serde(alias = "endOrdinal")]
    end_ordinal: i64,
}

#[derive(Deserialize)]
struct RoughcutApplyParams {
    #[serde(alias = "assetId")]
    asset_id: String,
    #[serde(alias = "targetPath")]
    target_path: String,
}

#[derive(Deserialize)]
struct ProjectExportApplyParams {
    #[serde(alias = "targetPath")]
    target_path: String,
}

#[derive(Serialize)]
struct ResolvedPath {
    path: String,
}

#[derive(Clone)]
struct ServiceProgressSink {
    events: Arc<dyn DesktopEventSink>,
    renderer_path_replacements: Vec<(String, &'static str)>,
}

impl ServiceProgressSink {
    fn new(
        events: Arc<dyn DesktopEventSink>,
        renderer_path_replacements: Vec<(String, &'static str)>,
    ) -> Self {
        Self {
            events,
            renderer_path_replacements,
        }
    }
}

#[derive(Serialize)]
struct TaskStateEvent {
    task_id: String,
    state: TaskState,
}

impl ProgressSink for ServiceProgressSink {
    fn progress(&self, mut event: ProgressEvent) {
        sanitize_progress_text(&mut event.phase, &self.renderer_path_replacements);
        sanitize_progress_text(&mut event.message, &self.renderer_path_replacements);
        if let Ok(payload) = serde_json::to_value(event) {
            let _ = self.events.emit("dl://progress", payload);
        }
    }

    fn task_state(&self, task_id: &str, state: TaskState) {
        if let Ok(payload) = serde_json::to_value(TaskStateEvent {
            task_id: task_id.to_string(),
            state,
        }) {
            let _ = self.events.emit("dl://task-state", payload);
        }
    }
}

fn resolve_asr_sidecar_dir(state: &DesktopState) -> PathBuf {
    if let Some(dir) = std::env::var_os("DOUBLELOVE_ASR_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(resource_dir) = state.runtime.resource_dir.as_deref() {
        for candidate in [
            resource_dir.join("model-runtime/asr"),
            resource_dir.join("resources/model-runtime/asr"),
        ] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for candidate in [
        manifest_root.join("sidecars/asr"),
        PathBuf::from("sidecars/asr"),
        PathBuf::from("../sidecars/asr"),
    ] {
        if candidate.is_dir() {
            return candidate;
        }
    }
    manifest_root.join("sidecars/asr")
}

fn resolve_speaker_sidecar_dir(state: &DesktopState) -> PathBuf {
    if let Some(dir) = std::env::var_os("DOUBLELOVE_SPEAKER_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(resource_dir) = state.runtime.resource_dir.as_deref() {
        for candidate in [
            resource_dir.join("model-runtime/speaker"),
            resource_dir.join("resources/model-runtime/speaker"),
        ] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    manifest_root.join("sidecars/speaker")
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

fn project_timeline_name(summary: &ProjectSummary) -> String {
    Path::new(&summary.root)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name} Rough Cut"))
        .unwrap_or_else(|| "Double Love Rough Cut".to_string())
}

fn sanitize_project_result<T>(
    result: OperationResult<T>,
    summary: &ProjectSummary,
) -> OperationResult<T> {
    let replacements = renderer_path_replacements(&[(&summary.root, "<PROJECT>")]);
    sanitize_renderer_diagnostics(result, &replacements)
}

fn sanitize_project_export_result<T>(
    store: &ProjectStore,
    summary: &ProjectSummary,
    result: OperationResult<T>,
) -> OperationResult<T> {
    let assets = store.media_assets().unwrap_or_default();
    let mut sensitive_paths = assets
        .iter()
        .map(|asset| (asset.original_path.as_str(), "<MEDIA>"))
        .collect::<Vec<_>>();
    sensitive_paths.push((&summary.root, "<PROJECT>"));
    let mut replacements = renderer_path_replacements(&sensitive_paths);
    replacements.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
    sanitize_renderer_diagnostics(result, &replacements)
}

fn set_project_subtitle_style(
    store: &ProjectStore,
    summary: &ProjectSummary,
    style: SubtitleStyle,
) -> OperationResult<SubtitleStyle> {
    let result = match store.set_subtitle_style(&style) {
        Ok(revision) => {
            let mut result = OperationResult::success(style);
            result.revision = Some(revision);
            result
        }
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    sanitize_project_result(result, summary)
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
                        record_recent_project_warning(
                            state,
                            events.as_ref(),
                            &summary,
                            &mut result,
                            true,
                        );
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
                    record_recent_project_warning(
                        state,
                        events.as_ref(),
                        &summary,
                        &mut result,
                        false,
                    );
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
    registry.register("timeline_get", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                compile_project_timeline(store, &project_timeline_name(summary)),
                summary,
            )
        }))
    });
    registry.register("main_track_append", |state, _events, payload| {
        let params: MainTrackAppendParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                append_main_track_clip(
                    store,
                    &params.asset_id,
                    params.source_in_frame,
                    params.source_out_frame,
                ),
                summary,
            )
        }))
    });
    registry.register("main_track_append_full", |state, _events, payload| {
        let params: MainTrackAppendFullParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                append_full_main_track_asset(store, &params.asset_id),
                summary,
            )
        }))
    });
    registry.register("main_track_list", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let result = match store.main_track_clips() {
                Ok(clips) => OperationResult::success(clips),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("main_track_move", |state, _events, payload| {
        let params: MainTrackMoveParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                move_main_track_clip(store, &params.clip_id, params.before_clip_id.as_deref()),
                summary,
            )
        }))
    });
    registry.register("main_track_trim", |state, _events, payload| {
        let params: MainTrackTrimParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                trim_main_track_clip(
                    store,
                    &params.clip_id,
                    params.source_in_frame,
                    params.source_out_frame,
                ),
                summary,
            )
        }))
    });
    registry.register("main_track_split", |state, _events, payload| {
        let params: MainTrackSplitParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                split_main_track_clip(store, &params.clip_id, params.source_at_frame),
                summary,
            )
        }))
    });
    registry.register("main_track_remove", |state, _events, payload| {
        let params: MainTrackRemoveParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(remove_main_track_clip(store, &params.clip_id), summary)
        }))
    });
    registry.register("canvas_get", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let result = match store.canvas_spec() {
                Ok(canvas) => OperationResult::success(canvas),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("canvas_set", |state, _events, payload| {
        let params: CanvasSetParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let result = match store.set_canvas_spec(&params.canvas) {
                Ok(revision) => {
                    let mut result = OperationResult::success(params.canvas);
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("output_rate_get", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let result = match store.output_rate() {
                Ok(rate) => OperationResult::success(rate),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("output_rate_set", |state, _events, payload| {
        let params: OutputRateSetParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let revision = match params.rate {
                Some(rate) => store.set_output_rate(rate),
                None => store.clear_output_rate(),
            };
            let result = match revision {
                Ok(revision) => {
                    let mut result = OperationResult::success(params.rate);
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("subtitle_style_get", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let result = match store.subtitle_style() {
                Ok(style) => OperationResult::success(style),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            };
            sanitize_project_result(result, summary)
        }))
    });
    registry.register("subtitle_style_set", |state, _events, payload| {
        let params: SubtitleStyleSetParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            set_project_subtitle_style(store, summary, params.style)
        }))
    });
    registry.register(
        "apply_default_subtitle_style",
        |state, _events, _payload| {
            let preferences =
                match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                    Ok(preferences) => preferences,
                    Err(error) => {
                        return result_value(preferences::command_error::<SubtitleStyle>(
                            error, true,
                        ));
                    }
                };
            result_value(with_store(state, |store, summary| {
                set_project_subtitle_style(store, summary, preferences.default_subtitle_style)
            }))
        },
    );
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
            events.as_ref(),
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
            events.as_ref(),
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
            events.as_ref(),
            params.default_asr_model,
            params.step,
        ))
    });
    registry.register("onboarding_reset", |state, events, _payload| {
        result_value(preferences::onboarding_reset(
            state.app_data_dir(),
            state.preferences(),
            events.as_ref(),
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
    registry.register("model_install", |state, events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state.models().begin_install(
            PathBuf::from(preferences.model_root),
            preferences.model_endpoint,
            &params.model_id,
            events,
        ) {
            Ok(installation) => OperationResult::success(installation),
            Err(error) => OperationResult::failed("MODEL_INSTALL_FAILED", error),
        };
        result_value(result)
    });
    for (name, cancel, error_code) in [
        ("model_pause", false, "MODEL_PAUSE_FAILED"),
        ("model_cancel", true, "MODEL_CANCEL_FAILED"),
    ] {
        registry.register(name, move |state, _events, payload| {
            let params: ModelIdParams = params(payload)?;
            let preferences =
                match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                    Ok(preferences) => preferences,
                    Err(error) => {
                        return result_value(preferences::command_error::<
                            double_love_engine::ModelInstallation,
                        >(error, true));
                    }
                };
            let result = match state.models().set_cancel(
                Path::new(&preferences.model_root),
                &params.model_id,
                cancel,
            ) {
                Ok(installation) => OperationResult::success(installation),
                Err(error) => OperationResult::failed(error_code, error),
            };
            result_value(result)
        });
    }
    registry.register("model_resume", |state, events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state.models().begin_install(
            PathBuf::from(preferences.model_root),
            preferences.model_endpoint,
            &params.model_id,
            events,
        ) {
            Ok(installation) => OperationResult::success(installation),
            Err(error) => OperationResult::failed("MODEL_INSTALL_FAILED", error),
        };
        result_value(result)
    });
    registry.register("model_verify", |state, _events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state
            .models()
            .verify(Path::new(&preferences.model_root), &params.model_id)
        {
            Ok(installation) => OperationResult::success(installation),
            Err(error) => OperationResult::failed("MODEL_VERIFY_FAILED", error),
        };
        result_value(result)
    });
    registry.register("model_remove", |state, events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state
            .models()
            .remove(Path::new(&preferences.model_root), &params.model_id)
        {
            Ok(installation) => {
                if let Ok(payload) = serde_json::to_value(&installation) {
                    let _ = events.emit("dl://model-state", payload);
                }
                OperationResult::success(installation)
            }
            Err(error @ ModelError::DependencyInUse { .. }) => {
                OperationResult::failed("MODEL_DEPENDENCY_IN_USE", error.to_string())
            }
            Err(error) => OperationResult::failed("MODEL_REMOVE_FAILED", error.to_string()),
        };
        result_value(result)
    });
    registry.register("model_reveal", |state, _events, payload| {
        let params: ModelRevealParams = if payload.is_null() {
            ModelRevealParams::default()
        } else {
            params(payload)?
        };
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<ResolvedPath>(error, true));
                }
            };
        let path = if let Some(model_id) = params.model_id {
            match state
                .models()
                .installation_dir(Path::new(&preferences.model_root), &model_id)
            {
                Ok(path) => path,
                Err(error) => {
                    return result_value(OperationResult::<ResolvedPath>::failed(
                        "MODEL_REVEAL_FAILED",
                        error,
                    ));
                }
            }
        } else {
            PathBuf::from(preferences.model_root)
        };
        let _ = std::fs::create_dir_all(&path);
        result_value(OperationResult::success(ResolvedPath {
            path: path.to_string_lossy().into_owned(),
        }))
    });
    registry.register("doctor_run", |state, events, _payload| {
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::DoctorReport,
                    >(error, true));
                }
            };
        let profile =
            match preferences::system_profile_for(state.app_data_dir(), state.preferences()) {
                Ok(profile) => profile,
                Err(error) => {
                    return result_value(
                        OperationResult::<double_love_engine::DoctorReport>::failed(
                            "SYSTEM_PROFILE_FAILED",
                            error.to_string(),
                        ),
                    );
                }
            };
        let tools = FfmpegTools::discover().ok();
        let environment = DoctorEnvironment {
            architecture: profile.architecture,
            os_version: profile.os_version,
            memory_bytes: profile.memory_bytes,
            free_model_bytes: profile.free_model_bytes,
            ffmpeg_available: tools.is_some(),
            libass_available: tools.as_ref().is_some_and(ffmpeg_supports_ass_filter),
            asr_runtime_ready: resolve_asr_sidecar_dir(state)
                .join(".venv/bin/python")
                .is_file(),
            speaker_runtime_ready: resolve_speaker_sidecar_dir(state)
                .join(".venv/bin/python")
                .is_file(),
        };
        let result = match state
            .models()
            .doctor_report(Path::new(&preferences.model_root), environment)
        {
            Ok(report) => {
                models::emit_doctor(events.as_ref(), &report);
                OperationResult::success(report)
            }
            Err(error) => OperationResult::failed("DOCTOR_FAILED", error),
        };
        result_value(result)
    });
    registry.register("diagnostics_reveal_logs", |state, _events, _payload| {
        let path = state.app_data_dir().join("logs");
        let _ = std::fs::create_dir_all(&path);
        result_value(OperationResult::success(ResolvedPath {
            path: path.to_string_lossy().into_owned(),
        }))
    });
    registry.register("transcribe_start", |state, events, payload| {
        let params: TranscribeStartParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<Value>(error, true));
                }
            };
        let model_root = Path::new(&preferences.model_root);
        let model_dir = match state.models().installed_dir(model_root, &params.model) {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let aligner_dir = match state
            .models()
            .installed_dir(model_root, "qwen3-forced-aligner-0.6b")
        {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let result = match state.open_project().with_current(|open| {
            let package_dir = resolve_asr_sidecar_dir(state);
            let model_root_path = model_root.to_string_lossy().into_owned();
            let model_dir_path = model_dir.to_string_lossy().into_owned();
            let aligner_dir_path = aligner_dir.to_string_lossy().into_owned();
            let package_dir_path = package_dir.to_string_lossy().into_owned();
            let replacements = renderer_path_replacements(&[
                (&open.summary.root, "<PROJECT>"),
                (&model_dir_path, "<MODEL>"),
                (&aligner_dir_path, "<MODEL>"),
                (&package_dir_path, "<MODEL>"),
                (&model_root_path, "<MODEL>"),
            ]);
            let config = TranscribeConfig {
                asset_id: params.asset_id,
                model: params.model,
                model_dir,
                aligner_dir,
                language: params.language,
                mock: state.runtime.test_transcribe_mock,
                python: None,
                package_dir,
                log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
                chunk_seconds: 30,
            };
            let sink: SharedSink = Arc::new(ServiceProgressSink::new(events, replacements));
            start_transcription(Arc::clone(&open.store), state.task_registry(), sink, config)
        }) {
            Ok(Ok(task_id)) => OperationResult::success(serde_json::json!({"task_id": task_id})),
            Ok(Err(error)) => OperationResult::failed("TRANSCRIBE_START_FAILED", error),
            Err(error) if error.code == PROJECT_NOT_OPEN => {
                OperationResult::failed(PROJECT_NOT_OPEN, "请先打开或创建一个项目。")
            }
            Err(error) => OperationResult::failed(error.code, error.message),
        };
        result_value(result)
    });
    registry.register("task_cancel", |state, _events, payload| {
        let params: TaskCancelParams = params(payload)?;
        let result = if state.task_registry().cancel(&params.task_id) {
            OperationResult::success(serde_json::json!({"task_id": params.task_id}))
        } else {
            OperationResult::failed(
                "TASK_NOT_RUNNING",
                format!("任务 {} 当前没有可取消的运行实例。", params.task_id),
            )
        };
        result_value(result)
    });
    registry.register("transcript_get", |state, _events, payload| {
        let params: AssetIdParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            transcript_view(store, &params.asset_id)
        }))
    });
    registry.register("speaker_list", |state, _events, _payload| {
        result_value(with_store(state, |store, _| {
            match store.speaker_identities() {
                Ok(speakers) => OperationResult::success(speakers),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register("speaker_name_proposals", |state, _events, payload| {
        let params: AssetIdParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.transcript_words(&params.asset_id) {
                Ok(words) => OperationResult::success(local_name_proposals(&words)),
                Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
            }
        }))
    });
    registry.register(
        "speaker_agent_payload_preview",
        |state, _events, payload| {
            let params: SpeakerAgentPayloadParams = params(payload)?;
            result_value(with_store(state, |store, summary| {
                let words = match store.transcript_words(&params.asset_id) {
                    Ok(words) => words,
                    Err(error) => {
                        return OperationResult::failed("STORAGE_ERROR", error.to_string());
                    }
                };
                let assets = match store.media_assets() {
                    Ok(assets) => assets,
                    Err(error) => {
                        return OperationResult::failed("STORAGE_ERROR", error.to_string());
                    }
                };
                let mut sensitive_paths = assets
                    .iter()
                    .map(|asset| (asset.original_path.as_str(), "<MEDIA>"))
                    .collect::<Vec<_>>();
                sensitive_paths.push((&summary.root, "<PROJECT>"));
                let mut replacements = renderer_path_replacements(&sensitive_paths);
                replacements.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
                OperationResult::success(sanitize_speaker_agent_payload(
                    agent_name_payload_preview(&words, &params.speaker_id),
                    &replacements,
                ))
            }))
        },
    );
    registry.register("speaker_name_confirm", |state, _events, payload| {
        let params: SpeakerNameConfirmParams = params(payload)?;
        if !params.confirmed {
            return result_value(OperationResult::<SpeakerIdentity>::failed(
                "SPEAKER_CONFIRM_REQUIRED",
                "请确认后再应用说话人姓名。",
            ));
        }
        result_value(with_store(state, |store, _| {
            match store.confirm_speaker_name(&params.speaker_id, &params.display_name) {
                Ok(identity) => {
                    let mut result = OperationResult::success(identity);
                    result.revision = store.revision().ok();
                    result
                }
                Err(error) => {
                    OperationResult::failed("SPEAKER_NAME_CONFIRM_FAILED", error.to_string())
                }
            }
        }))
    });
    registry.register("speaker_merge_confirm", |state, _events, payload| {
        let params: SpeakerMergeConfirmParams = params(payload)?;
        if !params.confirmed {
            return result_value(OperationResult::<SpeakerIdentity>::failed(
                "SPEAKER_CONFIRM_REQUIRED",
                "请确认后再合并说话人。",
            ));
        }
        result_value(with_store(state, |store, _| {
            match store.merge_speaker_identities(&params.keep_speaker_id, &params.merge_speaker_id)
            {
                Ok(identity) => {
                    let mut result = OperationResult::success(identity);
                    result.revision = store.revision().ok();
                    result
                }
                Err(error) => OperationResult::failed("SPEAKER_MERGE_FAILED", error.to_string()),
            }
        }))
    });
    registry.register("speaker_diarize_start", |state, events, payload| {
        let params: AssetIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(OperationResult::<Value>::failed(
                        "PREFERENCES_READ_FAILED",
                        error.to_string(),
                    ));
                }
            };
        let model_root = Path::new(&preferences.model_root);
        let speaker_model_dir = match state.models().installed_dir(model_root, "wespeaker-zh") {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let result = match state.open_project().with_current(|open| {
            let package_dir = resolve_speaker_sidecar_dir(state);
            let model_root_path = model_root.to_string_lossy().into_owned();
            let speaker_model_dir_path = speaker_model_dir.to_string_lossy().into_owned();
            let package_dir_path = package_dir.to_string_lossy().into_owned();
            let replacements = renderer_path_replacements(&[
                (&open.summary.root, "<PROJECT>"),
                (&speaker_model_dir_path, "<MODEL>"),
                (&package_dir_path, "<MODEL>"),
                (&model_root_path, "<MODEL>"),
            ]);
            let config = DiarizeConfig {
                asset_id: params.asset_id,
                mock: state.runtime.test_speaker_mock,
                python: None,
                package_dir,
                log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
                vad_model_dir: PathBuf::from("bundled"),
                speaker_model_dir,
            };
            let sink: SharedSink = Arc::new(ServiceProgressSink::new(events, replacements));
            start_speaker_diarization(Arc::clone(&open.store), state.task_registry(), sink, config)
        }) {
            Ok(Ok(task_id)) => OperationResult::success(serde_json::json!({"task_id": task_id})),
            Ok(Err(error)) => OperationResult::failed("SPEAKER_START_FAILED", error),
            Err(error) if error.code == PROJECT_NOT_OPEN => {
                OperationResult::failed(PROJECT_NOT_OPEN, "请先打开或创建一个项目。")
            }
            Err(error) => OperationResult::failed(error.code, error.message),
        };
        result_value(result)
    });
    registry.register("speaker_diarization_get", |state, _events, payload| {
        let params: AssetIdParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            speaker_diarization_result(store, &params.asset_id)
        }))
    });
    registry.register("edit_omit", |state, _events, payload| {
        let params: EditOmitParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            omit_words(
                store,
                &params.asset_id,
                params.start_ordinal,
                params.end_ordinal,
                params.handles_before_ms.unwrap_or(DEFAULT_HANDLES_MS),
                params.handles_after_ms.unwrap_or(DEFAULT_HANDLES_MS),
            )
        }))
    });
    registry.register("edit_restore", |state, _events, payload| {
        let params: EditRestoreParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            restore_words(
                store,
                &params.operation_id,
                params.start_ordinal,
                params.end_ordinal,
            )
        }))
    });
    registry.register("roughcut_preview", |state, _events, payload| {
        let params: AssetIdParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let exports_dir = Path::new(&summary.root).join(".doublelove/exports");
            export_rough_cut(store, &params.asset_id, &exports_dir, false)
        }))
    });
    registry.register("export_roughcut_apply", |state, _events, payload| {
        let params: RoughcutApplyParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            export_rough_cut_to(
                store,
                &params.asset_id,
                Path::new(&params.target_path),
                true,
            )
        }))
    });
    registry.register("project_export_preview", |state, _events, _payload| {
        result_value(with_store(state, |store, summary| {
            let result = preview_project_export(store, &project_timeline_name(summary));
            sanitize_project_export_result(store, summary, result)
        }))
    });
    registry.register("project_export_xmeml_apply", |state, _events, payload| {
        let params: ProjectExportApplyParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let result = export_project_xmeml_to(
                store,
                &project_timeline_name(summary),
                Path::new(&params.target_path),
            );
            sanitize_project_export_result(store, summary, result)
        }))
    });
    registry.register("project_export_ass_apply", |state, _events, payload| {
        let params: ProjectExportApplyParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let result = export_project_ass_to(
                store,
                &project_timeline_name(summary),
                Path::new(&params.target_path),
            );
            sanitize_project_export_result(store, summary, result)
        }))
    });
    registry.register("project_render_mp4_apply", |state, _events, payload| {
        let params: ProjectExportApplyParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let result = match FfmpegTools::discover() {
                Ok(tools) => render_project_mp4_to(
                    store,
                    &project_timeline_name(summary),
                    &tools,
                    &Path::new(&summary.root).join(".doublelove/cache"),
                    Path::new(&params.target_path),
                ),
                Err(diagnostic) => {
                    let mut result: OperationResult<ProjectExportPreview> =
                        OperationResult::failed(&diagnostic.code, &diagnostic.cause);
                    result.diagnostics[0].suggested_action = diagnostic.suggested_action.clone();
                    result
                }
            };
            sanitize_project_export_result(store, summary, result)
        }))
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
        Self::with_registry_and_runtime(
            app_data_dir,
            event_sink,
            commands,
            DesktopRuntimeConfig::default(),
        )
    }

    pub fn with_registry_and_runtime(
        app_data_dir: Option<PathBuf>,
        event_sink: Arc<dyn DesktopEventSink>,
        commands: CommandRegistry,
        runtime: DesktopRuntimeConfig,
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
            state: DesktopState::new(app_data_dir, runtime),
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
            .invoke(&self.state, Arc::clone(&self.event_sink), name, payload)
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

    #[derive(Default)]
    struct RecordingEventSink {
        events: Mutex<Vec<(String, Value)>>,
    }

    impl DesktopEventSink for RecordingEventSink {
        fn emit(&self, channel: &str, payload: Value) -> Result<(), DesktopServiceError> {
            self.events
                .lock()
                .map_err(|_| DesktopServiceError::internal("recording event sink lock"))?
                .push((channel.to_string(), payload));
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
        let state = Arc::new(DesktopState::new(
            temp_directory("atomic-app-data"),
            DesktopRuntimeConfig::default(),
        ));
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
        original.diagnostics[0].object_id = Some(selected.to_string());
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
            sanitized.diagnostics[0].object_id.as_deref(),
            Some("<SELECTED_MEDIA>")
        );
        assert_eq!(
            sanitized.diagnostics[0].suggested_action.as_deref(),
            Some("retry <SELECTED_MEDIA> from <PROJECT>")
        );
    }

    #[test]
    fn progress_text_redacts_large_numeric_arrays_but_keeps_short_lists() {
        let mut text = concat!(
            "speaker protocol error: {\"values\":",
            "[0.101001, -0.202002, 3.03e-1, 4, 5.05, 6.06, 7.07, 8.08]}",
            " short=[1.0, 0.0]"
        )
        .to_string();

        sanitize_progress_text(&mut text, &[]);

        assert_eq!(
            text,
            "speaker protocol error: {\"values\":<REDACTED>} short=[1.0, 0.0]"
        );
        for leaked in ["0.101001", "-0.202002", "3.03e-1", "8.08"] {
            assert!(!text.contains(leaked), "numeric value leaked: {text}");
        }
    }

    #[test]
    fn progress_text_cap_preserves_utf8_and_the_byte_limit() {
        let mut text = format!(
            "{}[0.11,0.22,0.33,0.44,0.55,0.66,0.77,0.88]",
            "隐私文本".repeat(2_000)
        );

        sanitize_progress_text(&mut text, &[]);

        assert!(text.len() <= MAX_PROGRESS_TEXT_BYTES);
        assert!(text.ends_with(TRUNCATED_PROGRESS_TEXT));
        assert!(!text.contains("0.11"));
        assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    }

    #[test]
    fn service_progress_sink_sanitizes_free_text_without_changing_task_or_counts() {
        let project = temp_directory("progress-project");
        let model = temp_directory("progress-model");
        let aligner = temp_directory("progress-aligner");
        let project_text = project.to_string_lossy().into_owned();
        let model_text = model.to_string_lossy().into_owned();
        let aligner_text = aligner.to_string_lossy().into_owned();
        let recorded = Arc::new(RecordingEventSink::default());
        let events: Arc<dyn DesktopEventSink> = recorded.clone();
        let sink = ServiceProgressSink::new(
            events,
            renderer_path_replacements(&[
                (&project_text, "<PROJECT>"),
                (&model_text, "<MODEL>"),
                (&aligner_text, "<MODEL>"),
            ]),
        );

        sink.progress(ProgressEvent {
            task: "task-123".to_string(),
            phase: format!("load {model_text}"),
            completed: Some(4),
            total: Some(9),
            message: format!(
                "read {project_text}/prepared.wav with {aligner_text}: [0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8]"
            ),
        });
        sink.task_state("task-123", TaskState::Failed);

        let events = recorded.events.lock().expect("recorded events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "dl://progress");
        assert_eq!(events[0].1["task"], "task-123");
        assert_eq!(events[0].1["phase"], "load <MODEL>");
        assert_eq!(events[0].1["completed"], 4);
        assert_eq!(events[0].1["total"], 9);
        assert_eq!(
            events[0].1["message"],
            "read <PROJECT>/prepared.wav with <MODEL>: <REDACTED>"
        );
        assert_eq!(events[1].0, "dl://task-state");
        assert_eq!(events[1].1["task_id"], "task-123");
        assert_eq!(events[1].1["state"], "failed");
        let serialized = serde_json::to_string(&*events).expect("events JSON");
        assert!(!serialized.contains(&project_text));
        assert!(!serialized.contains(&model_text));
        assert!(!serialized.contains(&aligner_text));
        assert!(!serialized.contains("0.1,0.2"));
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
