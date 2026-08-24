pub mod models;
pub mod preferences;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use double_love_engine::{
    CanvasSpec, DEFAULT_HANDLES_MS, Diagnostic, DiagnosticLevel, DiarizeConfig,
    DoctorCapabilityCheck, DoctorCapabilityStatus, DoctorEnvironment, FfmpegTools, FrameRate,
    MainTrackClip, MediaAssetSummary, ModelError, OperationResult, ProgressEvent, ProgressSink,
    ProjectExportPreview, ProjectStore, ProjectSummary, SharedSink, Sidecar, SidecarCommand,
    SidecarEvent, SidecarPoll, SpeakerIdentity, SpeakerNameAgentPayload, SubtitleStyle,
    TaskRegistry, TaskState, TranscribeConfig, agent_name_payload_preview,
    append_full_main_track_asset, append_main_track_clip, backup_sqlite_database,
    compile_project_timeline, create_project, export_project_ass_to, export_project_xmeml_to,
    export_rough_cut, export_rough_cut_to, ffmpeg_supports_ass_filter,
    import_media as engine_import_media, insert_full_main_track_assets, list_media_assets,
    local_name_proposals, move_main_track_clip, omit_words, open_project, preview_project_export,
    probe_media, remove_main_track_clip, render_project_mp4_to, restore_words,
    speaker_diarization_result, split_main_track_clip, start_speaker_diarization,
    start_transcription, transcript_view, trim_main_track_clip,
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
const ELECTRON_APP_VERSION_FALLBACK: &str = "0.2.0";

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

    pub fn close(&self) -> Result<Option<ProjectSummary>, DesktopServiceError> {
        let mut current = self
            .project
            .lock()
            .map_err(|_| DesktopServiceError::internal("open-project state lock is unavailable"))?;
        Ok(current.take().map(|open| open.summary))
    }

    pub fn current_project_id(&self) -> Result<Option<String>, DesktopServiceError> {
        let current = self
            .project
            .lock()
            .map_err(|_| DesktopServiceError::internal("open-project state lock is unavailable"))?;
        Ok(current.as_ref().map(|open| open.summary.project_id.clone()))
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
    active_asset_tasks: Arc<Mutex<HashMap<String, String>>>,
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
            active_asset_tasks: Arc::new(Mutex::new(HashMap::new())),
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

    fn track_asset_task(&self, task_id: &str, asset_id: &str) {
        if let Ok(mut tasks) = self.active_asset_tasks.lock() {
            tasks.insert(task_id.to_string(), asset_id.to_string());
            if self
                .task_registry
                .state(task_id)
                .is_some_and(|state| !matches!(state, TaskState::Pending | TaskState::Running))
            {
                tasks.remove(task_id);
            }
        }
    }

    fn asset_has_active_task(&self, asset_id: &str) -> bool {
        self.active_asset_tasks
            .lock()
            .is_ok_and(|tasks| tasks.values().any(|candidate| candidate == asset_id))
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

fn configured_history_limit(state: &DesktopState) -> Option<usize> {
    preferences::current_preferences(state.app_data_dir(), state.preferences())
        .ok()
        .and_then(|value| value.history_limit)
        .map(|value| value as usize)
}

fn prune_current_project_history(state: &DesktopState) -> Result<u64, String> {
    state
        .open_project()
        .with_store(|store, _| store.prune_project_snapshots(configured_history_limit(state)))
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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
        let mut value = handler(state, event_sink, payload)?;
        if value.get("revision").is_some_and(Value::is_number)
            && let Err(error) = prune_current_project_history(state)
            && let Some(diagnostics) = value.get_mut("diagnostics").and_then(Value::as_array_mut)
        {
            diagnostics.push(serde_json::json!({
                "level": "warning",
                "code": "HISTORY_LIMIT_APPLY_FAILED",
                "cause": error,
                "object_id": null,
                "impact": "本次编辑已保存，但旧恢复快照尚未按上限清理。",
                "blocks_export": false,
                "suggested_action": "重新打开项目后再试。"
            }));
        }
        Ok(value)
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

#[derive(Serialize)]
struct PreparedProjectTrash {
    path: String,
    was_current: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ProjectThumbnailFingerprint {
    asset_id: String,
    source_frame: i64,
    source_size: u64,
    source_modified_ms: u128,
}

#[derive(Default, Deserialize)]
struct ProjectHistoryParams {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct HistoryLimitParams {
    limit: Option<u32>,
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
struct MainTrackInsertAssetsParams {
    #[serde(alias = "assetIds")]
    asset_ids: Vec<String>,
    #[serde(alias = "beforeClipId")]
    before_clip_id: Option<String>,
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

#[derive(Deserialize)]
struct RecentProjectOpenParams {
    #[serde(alias = "projectId")]
    project_id: String,
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

#[derive(Deserialize)]
struct ModelLegacyCleanupApplyParams {
    #[serde(alias = "modelId")]
    model_id: String,
    confirmed: bool,
}

#[derive(Deserialize)]
struct ModelInstallParams {
    #[serde(alias = "modelId")]
    model_id: String,
    #[serde(default, alias = "acceptNoncommercialLicense")]
    accept_noncommercial_license: bool,
    #[serde(default, alias = "appVersion")]
    app_version: Option<String>,
}

#[derive(Deserialize)]
struct ModelResumeParams {
    #[serde(alias = "modelId")]
    model_id: String,
    #[serde(default, alias = "appVersion")]
    app_version: Option<String>,
}

#[derive(Deserialize)]
struct ModelImportFolderParams {
    #[serde(alias = "modelId")]
    model_id: String,
    source_path: PathBuf,
    #[serde(default, alias = "acceptNoncommercialLicense")]
    accept_noncommercial_license: bool,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorRunDepth {
    #[default]
    Quick,
    Deep,
}

#[derive(Default, Deserialize)]
struct DoctorRunParams {
    #[serde(default, alias = "appVersion")]
    app_version: Option<String>,
    #[serde(default)]
    depth: DoctorRunDepth,
}

#[derive(Serialize)]
struct RendererDoctorReport {
    #[serde(flatten)]
    report: double_love_engine::DoctorReport,
    app_version: String,
}

fn normalize_app_version(value: Option<String>) -> String {
    value
        .map(|version| version.trim().to_string())
        .filter(|version| {
            !version.is_empty()
                && version.len() <= 64
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-+_".contains(&byte))
        })
        .unwrap_or_else(|| ELECTRON_APP_VERSION_FALLBACK.to_string())
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
    active_asset_tasks: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl ServiceProgressSink {
    fn new(
        events: Arc<dyn DesktopEventSink>,
        renderer_path_replacements: Vec<(String, &'static str)>,
    ) -> Self {
        Self {
            events,
            renderer_path_replacements,
            active_asset_tasks: None,
        }
    }

    fn with_active_asset_tasks(mut self, tasks: Arc<Mutex<HashMap<String, String>>>) -> Self {
        self.active_asset_tasks = Some(tasks);
        self
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
        if !matches!(state, TaskState::Pending | TaskState::Running)
            && let Some(tasks) = &self.active_asset_tasks
            && let Ok(mut tasks) = tasks.lock()
        {
            tasks.remove(task_id);
        }
        if let Ok(payload) = serde_json::to_value(TaskStateEvent {
            task_id: task_id.to_string(),
            state,
        }) {
            let _ = self.events.emit("dl://task-state", payload);
        }
    }
}

fn resolve_bundled_model_runtime_dir(
    state: &DesktopState,
    component: &str,
) -> Result<PathBuf, String> {
    let resource_dir = state.runtime.resource_dir.as_deref().ok_or_else(|| {
        "App 内置模型运行时不可用，请重新安装完整的 Double Love Studio。".to_string()
    })?;
    for candidate in [
        resource_dir.join(format!("model-runtime/{component}")),
        resource_dir.join(format!("resources/model-runtime/{component}")),
    ] {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "App 内置 {component} 运行时不可用，请重新安装完整的 Double Love Studio。"
    ))
}

fn resolve_model_download_runtime(state: &DesktopState) -> Option<models::ModelDownloadRuntime> {
    let package_dir = resolve_bundled_model_runtime_dir(state, "asr").ok()?;
    let python = package_dir.join(".venv/bin/python");
    (python.is_file() && package_dir.is_dir()).then_some(models::ModelDownloadRuntime {
        python,
        package_dir,
    })
}

fn mlx_supported() -> bool {
    mlx_supported_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn mlx_supported_for(os: &str, architecture: &str) -> bool {
    os == "macos" && matches!(architecture, "aarch64" | "arm64")
}

fn mlx_unsupported_message() -> &'static str {
    "本机模型仅支持 Apple Silicon（M 系列芯片）的 macOS。"
}

fn resolve_asr_sidecar_dir(state: &DesktopState) -> Result<PathBuf, String> {
    resolve_bundled_model_runtime_dir(state, "asr")
}

fn resolve_speaker_sidecar_dir(state: &DesktopState) -> Result<PathBuf, String> {
    resolve_bundled_model_runtime_dir(state, "speaker")
}

/// The desktop product accepts only the media runtime shipped inside the App.
fn resolve_media_tools(
    state: &DesktopState,
) -> Result<FfmpegTools, Box<double_love_engine::Diagnostic>> {
    let resource_dir = state.runtime.resource_dir.as_deref().ok_or_else(|| {
        Box::new(Diagnostic {
            level: DiagnosticLevel::Error,
            code: "MEDIA_RUNTIME_MISSING".to_string(),
            cause: "App 内置媒体运行时不可用。".to_string(),
            object_id: None,
            impact: "无法导入、生成缩略图或渲染 MP4".to_string(),
            blocks_export: true,
            suggested_action: Some("请重新安装完整的 Double Love Studio。".to_string()),
        })
    })?;
    for runtime_dir in [
        resource_dir.join("runtime"),
        resource_dir.join("resources/runtime"),
    ] {
        let ffmpeg = runtime_dir.join("ffmpeg");
        let ffprobe = runtime_dir.join("ffprobe");
        if ffmpeg.is_file() || ffprobe.is_file() {
            return FfmpegTools::from_paths(ffprobe, ffmpeg);
        }
    }
    Err(Box::new(Diagnostic {
        level: DiagnosticLevel::Error,
        code: "MEDIA_RUNTIME_MISSING".to_string(),
        cause: "App 内置媒体运行时不可用。".to_string(),
        object_id: None,
        impact: "无法导入、生成缩略图或渲染 MP4".to_string(),
        blocks_export: true,
        suggested_action: Some("请重新安装完整的 Double Love Studio。".to_string()),
    }))
}

struct DoctorRuntimeProbe {
    checks: Vec<DoctorCapabilityCheck>,
    ffmpeg_available: bool,
    libass_available: bool,
    asr_runtime_ready: bool,
    speaker_runtime_ready: bool,
}

fn doctor_check(
    id: &str,
    status: DoctorCapabilityStatus,
    detail: impl Into<String>,
    suggested_action: Option<&str>,
) -> DoctorCapabilityCheck {
    DoctorCapabilityCheck {
        id: id.to_string(),
        status,
        detail: detail.into(),
        suggested_action: suggested_action.map(str::to_string),
    }
}

fn command_succeeds(program: &Path, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn command_output_contains(program: &Path, args: &[&str], needle: &str) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            stdout
                .lines()
                .chain(stderr.lines())
                .any(|line| line.split_whitespace().any(|token| token == needle))
        })
        .unwrap_or(false)
}

fn python_runtime_matches(python: &Path, code: &str) -> bool {
    Command::new(python)
        .args(["-c", code])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn probe_python_runtime(
    package_dir: Result<PathBuf, String>,
    package_name: &str,
    code: &str,
    id: &str,
) -> (DoctorCapabilityCheck, bool) {
    let Ok(package_dir) = package_dir else {
        return (
            doctor_check(
                id,
                DoctorCapabilityStatus::Blocked,
                format!("App 内置 {package_name} 运行时不可用。"),
                Some("请重新安装完整的 Double Love Studio。"),
            ),
            false,
        );
    };
    let python = package_dir.join(".venv/bin/python");
    if !python.is_file() {
        return (
            doctor_check(
                id,
                DoctorCapabilityStatus::Blocked,
                format!("App 内置 {package_name} Python 运行时不可用。"),
                Some("请重新安装完整的 Double Love Studio。"),
            ),
            false,
        );
    }
    let ready = python_runtime_matches(&python, code);
    (
        doctor_check(
            id,
            if ready {
                DoctorCapabilityStatus::Ready
            } else {
                DoctorCapabilityStatus::Blocked
            },
            if ready {
                format!("App 内置 {package_name} 运行时可用。")
            } else {
                format!("App 内置 {package_name} Python 依赖不完整或版本不匹配。")
            },
            (!ready).then_some("请重新安装完整的 Double Love Studio。"),
        ),
        ready,
    )
}

fn probe_bundled_runtime(state: &DesktopState) -> DoctorRuntimeProbe {
    let mut checks = Vec::new();
    let tools = resolve_media_tools(state).ok();
    let ffmpeg_available = tools
        .as_ref()
        .is_some_and(|tools| command_succeeds(&tools.ffmpeg, &["-hide_banner", "-version"]));
    let ffprobe_available = tools
        .as_ref()
        .is_some_and(|tools| command_succeeds(&tools.ffprobe, &["-hide_banner", "-version"]));
    let libass_available = tools.as_ref().is_some_and(ffmpeg_supports_ass_filter);
    let h264_available = tools.as_ref().is_some_and(|tools| {
        command_output_contains(&tools.ffmpeg, &["-hide_banner", "-encoders"], "libx264")
    });
    let aac_available = tools.as_ref().is_some_and(|tools| {
        command_output_contains(&tools.ffmpeg, &["-hide_banner", "-encoders"], "aac")
    });
    checks.push(doctor_check(
        "media.ffmpeg_runtime",
        if ffmpeg_available {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if ffmpeg_available {
            "App 内置 ffmpeg 可用。"
        } else {
            "App 内置 ffmpeg 不可用。"
        },
        (!ffmpeg_available).then_some("请重新安装完整的 Double Love Studio。"),
    ));
    checks.push(doctor_check(
        "media.ffprobe_runtime",
        if ffprobe_available {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if ffprobe_available {
            "App 内置 ffprobe 可用。"
        } else {
            "App 内置 ffprobe 不可用。"
        },
        (!ffprobe_available).then_some("请重新安装完整的 Double Love Studio。"),
    ));
    for (id, ready, detail) in [
        (
            "media.ass_filter",
            libass_available,
            "App 内置 ffmpeg 的 ASS/libass 字幕滤镜",
        ),
        (
            "media.h264_encoder",
            h264_available,
            "App 内置 ffmpeg 的 H.264 编码器",
        ),
        (
            "media.aac_encoder",
            aac_available,
            "App 内置 ffmpeg 的 AAC 编码器",
        ),
    ] {
        checks.push(doctor_check(
            id,
            if ready {
                DoctorCapabilityStatus::Ready
            } else if tools.is_some() {
                DoctorCapabilityStatus::Blocked
            } else {
                DoctorCapabilityStatus::NotRun
            },
            if ready {
                format!("{detail}可用。")
            } else {
                format!("{detail}不可用。")
            },
            (!ready).then_some("请重新安装完整的 Double Love Studio。"),
        ));
    }

    let (asr_check, asr_runtime_ready) = probe_python_runtime(
        resolve_asr_sidecar_dir(state),
        "ASR",
        "import importlib.metadata as m\nimport double_love_asr, modelscope, modelscope_hub\nassert m.version('mlx-qwen3-asr') == '0.3.5'\nassert m.version('modelscope') == '1.39.1'\nassert m.version('modelscope-hub') == '0.2.0'\nfor name in ('torch', 'torchaudio', 'wespeaker', 'silero-vad', 'onnxruntime'):\n    try: m.version(name)\n    except m.PackageNotFoundError: continue\n    raise SystemExit(name)",
        "runtime.asr",
    );
    let (speaker_check, speaker_runtime_ready) = probe_python_runtime(
        resolve_speaker_sidecar_dir(state),
        "Speaker",
        "import importlib.metadata as m\nimport mlx, mlx_audio, numpy, double_love_speaker.engine, double_love_speaker.mlx_resnet\nassert m.version('mlx') == '0.31.1'\nassert m.version('mlx-audio') == '0.5.0'\nassert m.version('numpy') == '2.3.5'\nfor name in ('torch', 'torchaudio', 'wespeaker', 'silero-vad', 'onnxruntime'):\n    try: m.version(name)\n    except m.PackageNotFoundError: continue\n    raise SystemExit(name)",
        "runtime.speaker",
    );
    checks.push(asr_check);
    checks.push(speaker_check);
    checks.push(doctor_check(
        "system.mlx_platform",
        if mlx_supported() {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if mlx_supported() {
            "本机满足 Apple Silicon MLX 运行条件。"
        } else {
            mlx_unsupported_message()
        },
        (!mlx_supported()).then_some("请在 Apple Silicon macOS 上运行 Double Love Studio。"),
    ));

    DoctorRuntimeProbe {
        checks,
        ffmpeg_available,
        libass_available,
        asr_runtime_ready,
        speaker_runtime_ready,
    }
}

fn doctor_temp_stem(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("double-love-doctor-{label}-{stamp}"))
}

fn write_doctor_wav(path: &Path) -> Result<(), String> {
    let sample_rate = 16_000_u32;
    let sample_count = sample_rate;
    let data_bytes = sample_count * 2;
    let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
    file.write_all(b"RIFF").map_err(|error| error.to_string())?;
    file.write_all(&(36 + data_bytes).to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(b"WAVEfmt ")
        .map_err(|error| error.to_string())?;
    file.write_all(&16_u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&1_u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&1_u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&sample_rate.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&(sample_rate * 2).to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&2_u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&16_u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(b"data").map_err(|error| error.to_string())?;
    file.write_all(&data_bytes.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&vec![0_u8; data_bytes as usize])
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn deep_media_smoke(state: &DesktopState) -> DoctorCapabilityCheck {
    let Some(tools) = resolve_media_tools(state).ok() else {
        return doctor_check(
            "deep.media_render",
            DoctorCapabilityStatus::NotRun,
            "App 内置媒体运行时不可用，未执行编码试跑。",
            Some("请重新安装完整的 Double Love Studio。"),
        );
    };
    let output_path = doctor_temp_stem("render").with_extension("mp4");
    let result = Command::new(&tools.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=16x16:r=25:d=1",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=mono",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ar",
            "48000",
        ])
        .arg(&output_path)
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && command_succeeds(
                    &tools.ffprobe,
                    &[
                        "-hide_banner",
                        "-v",
                        "error",
                        "-show_entries",
                        "format=format_name",
                        "-of",
                        "default=nw=1",
                        output_path.to_string_lossy().as_ref(),
                    ],
                )
        });
    let _ = fs::remove_file(&output_path);
    doctor_check(
        "deep.media_render",
        if result {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if result {
            "App 内置 ffmpeg/ffprobe 已完成 H.264/AAC 短时编码验证。"
        } else {
            "App 内置 ffmpeg/ffprobe 编码试跑失败。"
        },
        (!result).then_some("请重新安装完整的 Double Love Studio。"),
    )
}

fn deep_asr_smoke(
    state: &DesktopState,
    preferences: &preferences::AppPreferencesV1,
    report: &double_love_engine::DoctorReport,
) -> DoctorCapabilityCheck {
    let model_id = preferences.default_asr_model.as_str();
    let ready = |id: &str| {
        report
            .model_checks
            .iter()
            .find(|check| check.model_id == id)
            .is_some_and(|check| check.state == double_love_engine::ModelInstallState::Installed)
    };
    if !ready(model_id) || !ready("qwen3-forced-aligner-0.6b-8bit") {
        return doctor_check(
            "deep.asr",
            DoctorCapabilityStatus::NotRun,
            "默认转录模型或 ForcedAligner 未安装，未执行模型试跑。",
            Some("请先在设置 → 本地模型中完成安装和校验。"),
        );
    }
    let Ok(package_dir) = resolve_asr_sidecar_dir(state) else {
        return doctor_check(
            "deep.asr",
            DoctorCapabilityStatus::NotRun,
            "App 内置 ASR 运行时不可用，未执行模型试跑。",
            Some("请重新安装完整的 Double Love Studio。"),
        );
    };
    let python = package_dir.join(".venv/bin/python");
    let root = Path::new(&preferences.model_root);
    let Ok(model_dir) = state.models().installed_inference_dir(root, model_id) else {
        return doctor_check(
            "deep.asr",
            DoctorCapabilityStatus::NotRun,
            "默认转录模型未通过本地完整性检查，未执行模型试跑。",
            Some("请在设置 → 本地模型中重新校验模型。"),
        );
    };
    let Ok(aligner_dir) = state
        .models()
        .installed_inference_dir(root, "qwen3-forced-aligner-0.6b-8bit")
    else {
        return doctor_check(
            "deep.asr",
            DoctorCapabilityStatus::NotRun,
            "ForcedAligner 未通过本地完整性检查，未执行模型试跑。",
            Some("请在设置 → 本地模型中重新校验模型。"),
        );
    };
    let base = doctor_temp_stem("asr");
    let wav_path = base.with_extension("wav");
    let log_path = base.with_extension("log");
    let task_id = "doctor-asr".to_string();
    let success = (|| -> Result<bool, String> {
        write_doctor_wav(&wav_path)?;
        let mut sidecar = Sidecar::spawn(&python, &package_dir, false, &log_path)
            .map_err(|error| error.to_string())?;
        sidecar
            .send(&SidecarCommand::Transcribe {
                task_id: task_id.clone(),
                wav_path: wav_path.to_string_lossy().into_owned(),
                model: model_id.to_string(),
                model_dir: model_dir.to_string_lossy().into_owned(),
                aligner_dir: aligner_dir.to_string_lossy().into_owned(),
                language: "zh".to_string(),
                source_sample_rate: 16_000,
                chunk_seconds: 1,
            })
            .map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if std::time::Instant::now() >= deadline {
                return Err("ASR 模型试跑超时。".to_string());
            }
            match sidecar.next_event(Duration::from_secs(1)) {
                SidecarPoll::Event(Ok(SidecarEvent::Done { .. })) => return Ok(true),
                SidecarPoll::Event(Ok(SidecarEvent::Error { message, .. })) => {
                    return Err(message);
                }
                SidecarPoll::Event(Err(error)) => return Err(error),
                SidecarPoll::Closed => return Err("ASR sidecar 已关闭。".to_string()),
                SidecarPoll::TimedOut | SidecarPoll::Event(Ok(_)) => {}
            }
        }
    })()
    .unwrap_or(false);
    let _ = fs::remove_file(&wav_path);
    let _ = fs::remove_file(&log_path);
    doctor_check(
        "deep.asr",
        if success {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if success {
            "App 内置 ASR 与 ForcedAligner 已完成短时运行验证。"
        } else {
            "App 内置 ASR 或 ForcedAligner 模型试跑失败。"
        },
        (!success)
            .then_some("请在设置 → 本地模型中重新校验模型，并重新安装完整的 Double Love Studio。"),
    )
}

fn deep_speaker_smoke(
    state: &DesktopState,
    preferences: &preferences::AppPreferencesV1,
    report: &double_love_engine::DoctorReport,
) -> DoctorCapabilityCheck {
    let ready = |id: &str| {
        report
            .model_checks
            .iter()
            .find(|check| check.model_id == id)
            .is_some_and(|check| check.state == double_love_engine::ModelInstallState::Installed)
    };
    if !ready("wespeaker-voxceleb-resnet34-lm") || !ready("silero-vad-v6") {
        return doctor_check(
            "deep.speaker",
            DoctorCapabilityStatus::NotRun,
            "说话人模型或 Silero VAD 未安装，未执行模型试跑。",
            Some("请先在设置 → 本地模型中完成安装和校验。"),
        );
    }
    let Ok(package_dir) = resolve_speaker_sidecar_dir(state) else {
        return doctor_check(
            "deep.speaker",
            DoctorCapabilityStatus::NotRun,
            "App 内置 Speaker 运行时不可用，未执行模型试跑。",
            Some("请重新安装完整的 Double Love Studio。"),
        );
    };
    let python = package_dir.join(".venv/bin/python");
    let root = Path::new(&preferences.model_root);
    let Ok(speaker_model_dir) = state.models().selected_speaker_model_dir(root) else {
        return doctor_check(
            "deep.speaker",
            DoctorCapabilityStatus::NotRun,
            "说话人模型未通过本地完整性检查，未执行模型试跑。",
            Some("请在设置 → 本地模型中重新校验模型。"),
        );
    };
    let Ok(vad_model_dir) = state
        .models()
        .installed_inference_dir(root, "silero-vad-v6")
    else {
        return doctor_check(
            "deep.speaker",
            DoctorCapabilityStatus::NotRun,
            "Silero VAD 未通过本地完整性检查，未执行模型试跑。",
            Some("请在设置 → 本地模型中重新校验模型。"),
        );
    };
    let base = doctor_temp_stem("speaker");
    let wav_path = base.with_extension("wav");
    let log_path = base.with_extension("log");
    let success = (|| -> Result<bool, String> {
        write_doctor_wav(&wav_path)?;
        let mut sidecar = Sidecar::spawn_module(
            &python,
            &package_dir,
            "double_love_speaker",
            false,
            &log_path,
        )
        .map_err(|error| error.to_string())?;
        sidecar
            .send(&SidecarCommand::Diarize {
                task_id: "doctor-speaker".to_string(),
                wav_path: wav_path.to_string_lossy().into_owned(),
                vad_model_dir: vad_model_dir.to_string_lossy().into_owned(),
                speaker_model_dir: speaker_model_dir.to_string_lossy().into_owned(),
                source_sample_rate: 16_000,
            })
            .map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if std::time::Instant::now() >= deadline {
                return Err("Speaker 模型试跑超时。".to_string());
            }
            match sidecar.next_event(Duration::from_secs(1)) {
                SidecarPoll::Event(Ok(SidecarEvent::DiarizationDone { .. })) => return Ok(true),
                SidecarPoll::Event(Ok(SidecarEvent::Error { message, .. })) => {
                    return Err(message);
                }
                SidecarPoll::Event(Err(error)) => return Err(error),
                SidecarPoll::Closed => return Err("Speaker sidecar 已关闭。".to_string()),
                SidecarPoll::TimedOut | SidecarPoll::Event(Ok(_)) => {}
            }
        }
    })()
    .unwrap_or(false);
    let _ = fs::remove_file(&wav_path);
    let _ = fs::remove_file(&log_path);
    doctor_check(
        "deep.speaker",
        if success {
            DoctorCapabilityStatus::Ready
        } else {
            DoctorCapabilityStatus::Blocked
        },
        if success {
            "App 内置 Speaker 与 Silero VAD 已完成短时运行验证。"
        } else {
            "App 内置 Speaker 或 Silero VAD 模型试跑失败。"
        },
        (!success)
            .then_some("请在设置 → 本地模型中重新校验模型，并重新安装完整的 Double Love Studio。"),
    )
}

fn deep_diagnostics(
    state: &DesktopState,
    preferences: &preferences::AppPreferencesV1,
    report: &double_love_engine::DoctorReport,
) -> Vec<DoctorCapabilityCheck> {
    vec![
        deep_media_smoke(state),
        deep_asr_smoke(state, preferences, report),
        deep_speaker_smoke(state, preferences, report),
    ]
}

fn resolve_project_thumbnail(
    state: &DesktopState,
    project_id: &str,
) -> OperationResult<ResolvedMediaAsset> {
    let root = match preferences::recent_project_path(
        state.app_data_dir(),
        state.preferences(),
        project_id,
    ) {
        Ok(root) => root,
        Err(error) => {
            return OperationResult::failed("PROJECT_THUMBNAIL_FORBIDDEN", error.to_string());
        }
    };
    let database = root.join(".doublelove/project.sqlite");
    let store = match ProjectStore::open(&database) {
        Ok(store) => store,
        Err(error) => {
            return OperationResult::failed("PROJECT_THUMBNAIL_UNAVAILABLE", error.to_string());
        }
    };
    let source = match store.project_thumbnail_source() {
        Ok(Some(source)) => source,
        Ok(None) => {
            return OperationResult::failed(
                "PROJECT_THUMBNAIL_EMPTY",
                "项目还没有可以生成缩略图的视频。",
            );
        }
        Err(error) => {
            return OperationResult::failed("PROJECT_THUMBNAIL_UNAVAILABLE", error.to_string());
        }
    };
    let source_path = Path::new(&source.original_path);
    let source_metadata = match source_path.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => {
            return OperationResult::failed(
                "PROJECT_THUMBNAIL_SOURCE_MISSING",
                "项目缩略图引用的源视频已经不可用。",
            );
        }
    };
    let fingerprint = ProjectThumbnailFingerprint {
        asset_id: source.asset_id,
        source_frame: source.source_frame,
        source_size: source_metadata.len(),
        source_modified_ms: source_metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_millis())
            .unwrap_or_default(),
    };
    let cache = root.join(".doublelove/cache");
    let thumbnail = cache.join("project-library-thumbnail.jpg");
    let fingerprint_path = cache.join("project-library-thumbnail.json");
    let cached = fs::read(&fingerprint_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ProjectThumbnailFingerprint>(&bytes).ok())
        .is_some_and(|value| value == fingerprint);
    if cached && thumbnail.is_file() {
        return OperationResult::success(ResolvedMediaAsset {
            path: thumbnail.to_string_lossy().into_owned(),
        });
    }
    let tools = match resolve_media_tools(state) {
        Ok(tools) => tools,
        Err(diagnostic) => {
            return OperationResult::failed(&diagnostic.code, &diagnostic.cause);
        }
    };
    if let Err(error) = fs::create_dir_all(&cache) {
        return OperationResult::failed("PROJECT_THUMBNAIL_WRITE_FAILED", error.to_string());
    }
    let temporary = cache.join("project-library-thumbnail.tmp.jpg");
    let rational = source.rate.rational();
    let seek_seconds =
        source.source_frame.max(0) as f64 * rational.den as f64 / rational.num as f64;
    let output = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss"])
        .arg(format!("{seek_seconds:.6}"))
        .arg("-i")
        .arg(source_path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black",
            "-q:v",
            "3",
        ])
        .arg(&temporary)
        .output();
    match output {
        Ok(output) if output.status.success() && temporary.is_file() => {}
        Ok(output) => {
            let _ = fs::remove_file(&temporary);
            return OperationResult::failed(
                "PROJECT_THUMBNAIL_GENERATE_FAILED",
                String::from_utf8_lossy(&output.stderr).into_owned(),
            );
        }
        Err(error) => {
            return OperationResult::failed("PROJECT_THUMBNAIL_GENERATE_FAILED", error.to_string());
        }
    }
    if let Err(error) = fs::rename(&temporary, &thumbnail) {
        let _ = fs::remove_file(&temporary);
        return OperationResult::failed("PROJECT_THUMBNAIL_WRITE_FAILED", error.to_string());
    }
    if let Ok(bytes) = serde_json::to_vec(&fingerprint) {
        let _ = fs::write(fingerprint_path, bytes);
    }
    OperationResult::success(ResolvedMediaAsset {
        path: thumbnail.to_string_lossy().into_owned(),
    })
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

fn warning<T>(
    result: &mut OperationResult<T>,
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

fn backup_existing_project_database(root: &Path) -> Result<(), String> {
    let project_data = root.join(".doublelove");
    let source = project_data.join("project.sqlite");
    if source
        .try_exists()
        .map_err(|error| format!("pre-Electron project backup check failed: {error}"))?
    {
        backup_sqlite_database(
            &source,
            &project_data.join("project.pre-electron-backup.sqlite"),
        )
        .map_err(|error| format!("pre-Electron project backup failed: {error}"))?;
    }
    Ok(())
}

fn install_open_project(
    state: &DesktopState,
    summary: &ProjectSummary,
) -> Result<(), DesktopServiceError> {
    let store = ProjectStore::open(Path::new(&summary.database))
        .map_err(|error| DesktopServiceError::new("STORAGE_ERROR", error.to_string()))?;
    store
        .prune_project_snapshots(configured_history_limit(state))
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

fn open_project_at_path(
    state: &DesktopState,
    events: &dyn DesktopEventSink,
    project_root: &Path,
) -> OperationResult<ProjectSummary> {
    match backup_existing_project_database(project_root)
        .and_then(|()| open_project(project_root).map_err(|error| error.to_string()))
    {
        Ok(summary) => match install_open_project(state, &summary) {
            Ok(()) => {
                let mut result = OperationResult::success(summary.clone());
                record_recent_project_warning(state, events, &summary, &mut result, false);
                result
            }
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.message),
        },
        Err(error) => OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string()),
    }
}

/// Registers the migrated host-neutral desktop commands.
pub fn register_commands(registry: &mut CommandRegistry) {
    registry.register("project_create", |state, events, payload| {
        let params: ProjectPathParams = params(payload)?;
        let project_root = Path::new(&params.path);
        let result = match backup_existing_project_database(project_root)
            .and_then(|()| create_project(project_root).map_err(|error| error.to_string()))
        {
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
        result_value(open_project_at_path(
            state,
            events.as_ref(),
            Path::new(&params.path),
        ))
    });
    registry.register("recent_project_open", |state, events, payload| {
        let params: RecentProjectOpenParams = params(payload)?;
        let project_root = match preferences::recent_project_path(
            state.app_data_dir(),
            state.preferences(),
            &params.project_id,
        ) {
            Ok(path) => path,
            Err(error) => {
                let message = error.to_string();
                let code = if message == "项目位置已经丢失。" {
                    "RECENT_PROJECT_MISSING"
                } else {
                    "RECENT_PROJECT_NOT_FOUND"
                };
                return result_value(OperationResult::<ProjectSummary>::failed(code, message));
            }
        };
        result_value(open_project_at_path(state, events.as_ref(), &project_root))
    });
    registry.register("project_checkpoint", |state, _events, _payload| {
        let result = match state
            .open_project()
            .with_store(|store, _| store.checkpoint().and_then(|()| store.revision()))
        {
            Ok(Ok(revision)) => OperationResult::success(Some(revision)),
            Ok(Err(error)) => {
                OperationResult::failed("PROJECT_CHECKPOINT_FAILED", error.to_string())
            }
            Err(error) if error.code == PROJECT_NOT_OPEN => OperationResult::success(None),
            Err(error) => OperationResult::failed(error.code, error.message),
        };
        result_value(result)
    });
    registry.register("project_close", |state, _events, _payload| {
        let checkpoint = match state
            .open_project()
            .with_store(|store, _| store.checkpoint())
        {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(error) if error.code == PROJECT_NOT_OPEN => None,
            Err(error) => Some(error.message),
        };
        if let Some(error) = checkpoint {
            return result_value(OperationResult::<()>::failed("PROJECT_CLOSE_FAILED", error));
        }
        state.open_project().close()?;
        if let Ok(mut navigation) = state.history_navigation().lock() {
            *navigation = HistoryNavigation::default();
        }
        result_value(OperationResult::success(()))
    });
    registry.register("prepare_project_trash", |state, _events, payload| {
        let params: RecentProjectOpenParams = params(payload)?;
        if state.task_registry().has_active() {
            return result_value(OperationResult::<PreparedProjectTrash>::failed(
                "PROJECT_TRASH_TASK_ACTIVE",
                "后台任务运行时不能移动项目到废纸篓。",
            ));
        }
        let root = match preferences::recent_project_path(
            state.app_data_dir(),
            state.preferences(),
            &params.project_id,
        ) {
            Ok(root) => root,
            Err(error) => {
                return result_value(OperationResult::<PreparedProjectTrash>::failed(
                    "PROJECT_TRASH_FORBIDDEN",
                    error.to_string(),
                ));
            }
        };
        let database = root.join(".doublelove/project.sqlite");
        let store = match ProjectStore::open(&database) {
            Ok(store) => store,
            Err(error) => {
                return result_value(OperationResult::<PreparedProjectTrash>::failed(
                    "PROJECT_TRASH_INVALID",
                    error.to_string(),
                ));
            }
        };
        if store.project_id().ok().flatten().as_deref() != Some(params.project_id.as_str()) {
            return result_value(OperationResult::<PreparedProjectTrash>::failed(
                "PROJECT_TRASH_INVALID",
                "项目标识与项目文件夹不匹配。",
            ));
        }
        let was_current = state.open_project().current_project_id()?.as_deref()
            == Some(params.project_id.as_str());
        if was_current {
            let checkpoint = state
                .open_project()
                .with_store(|current, _| current.checkpoint());
            match checkpoint {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    return result_value(OperationResult::<PreparedProjectTrash>::failed(
                        "PROJECT_CHECKPOINT_FAILED",
                        error.to_string(),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        result_value(OperationResult::success(PreparedProjectTrash {
            path: root.to_string_lossy().into_owned(),
            was_current,
        }))
    });
    registry.register("import_media", |state, _events, payload| {
        let params: ProjectPathParams = params(payload)?;
        result_value(with_store(state, |store, summary| {
            let replacements = renderer_path_replacements(&[
                (&params.path, "<SELECTED_MEDIA>"),
                (&summary.root, "<PROJECT>"),
            ]);
            let result = match resolve_media_tools(state) {
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
    registry.register("media_preflight", |state, _events, payload| {
        let params: ProjectPathParams = params(payload)?;
        result_value(with_store(state, |_store, summary| {
            let replacements = renderer_path_replacements(&[
                (&params.path, "<SELECTED_MEDIA>"),
                (&summary.root, "<PROJECT>"),
            ]);
            let result = match resolve_media_tools(state) {
                Ok(tools) => probe_media(&tools, Path::new(&params.path)),
                Err(diagnostic) => {
                    let mut result = OperationResult::failed(&diagnostic.code, &diagnostic.cause);
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
    registry.register("media_asset_remove", |state, _events, payload| {
        let params: AssetIdParams = params(payload)?;
        if state.asset_has_active_task(&params.asset_id) {
            return result_value(OperationResult::<Value>::failed(
                "MEDIA_ASSET_BUSY",
                "这个素材正在执行转录或说话人任务，请先结束任务。",
            ));
        }
        result_value(with_store(state, |store, summary| {
            match store.remove_media_asset(&params.asset_id) {
                Ok((revision, removed_clips, prepared_wav_path)) => {
                    if let Some(path) = prepared_wav_path {
                        let prepared_root = Path::new(&summary.root).join(".doublelove/prepared");
                        if let (Ok(canonical_path), Ok(canonical_root)) = (
                            Path::new(&path).canonicalize(),
                            prepared_root.canonicalize(),
                        ) && canonical_path.starts_with(canonical_root)
                            && canonical_path.is_file()
                        {
                            let _ = std::fs::remove_file(canonical_path);
                        }
                    }
                    let mut result =
                        OperationResult::success(double_love_engine::MediaAssetRemoval {
                            asset_id: params.asset_id,
                            removed_clips,
                        });
                    result.revision = Some(revision);
                    result
                }
                Err(error) => OperationResult::failed("MEDIA_REMOVE_FAILED", error.to_string()),
            }
        }))
    });
    registry.register("resolve_media_asset", |state, _events, payload| {
        let params: ResolveMediaAssetParams = params(payload)?;
        result_value(with_store(state, |store, _| {
            match store.active_media_asset(&params.asset_id) {
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
    registry.register("history_limit_preview", |state, _events, payload| {
        let params: HistoryLimitParams = params(payload)?;
        let Some(limit) = params.limit else {
            return result_value(OperationResult::success(0_u64));
        };
        let mut removed = 0_u64;
        if let Ok(paths) =
            preferences::registered_project_paths(state.app_data_dir(), state.preferences())
        {
            for root in paths {
                let database = root.join(".doublelove/project.sqlite");
                if !database.is_file() {
                    continue;
                }
                if let Ok(count) = ProjectStore::open(&database)
                    .and_then(|store| store.restorable_snapshot_count())
                {
                    removed = removed.saturating_add(count.saturating_sub(limit as u64));
                }
            }
        }
        result_value(OperationResult::success(removed))
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
            let mut result = sanitize_project_result(
                compile_project_timeline(store, &project_timeline_name(summary)),
                summary,
            );
            if let Some(timeline) = result.data.as_mut() {
                for source in &mut timeline.sources {
                    source.original_path.clear();
                }
            }
            result
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
    registry.register("main_track_insert_assets", |state, _events, payload| {
        let params: MainTrackInsertAssetsParams = params(payload)?;
        if params.asset_ids.is_empty() || params.asset_ids.len() > 64 {
            return result_value(OperationResult::<Vec<MainTrackClip>>::failed(
                "MAIN_TRACK_INSERT_INVALID",
                "请选择 1 到 64 个素材。",
            ));
        }
        result_value(with_store(state, |store, summary| {
            sanitize_project_result(
                insert_full_main_track_assets(
                    store,
                    &params.asset_ids,
                    params.before_clip_id.as_deref(),
                ),
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
        let history_limit_changed = params.patch.history_limit.is_some();
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
        let mut result = preferences::preferences_update(
            state.app_data_dir(),
            state.preferences(),
            events.as_ref(),
            params.patch,
        );
        if history_limit_changed && result.status == double_love_engine::OperationStatus::Success {
            let limit = result
                .data
                .as_ref()
                .and_then(|preferences| preferences.history_limit)
                .map(|value| value as usize);
            let mut failed = 0_u64;
            if let Ok(paths) =
                preferences::registered_project_paths(state.app_data_dir(), state.preferences())
            {
                for root in paths {
                    let database = root.join(".doublelove/project.sqlite");
                    if !database.is_file() {
                        continue;
                    }
                    if ProjectStore::open(&database)
                        .and_then(|store| store.prune_project_snapshots(limit))
                        .is_err()
                    {
                        failed += 1;
                    }
                }
            }
            if failed > 0 {
                warning(
                    &mut result,
                    "HISTORY_LIMIT_PARTIAL",
                    format!("有 {failed} 个项目暂时无法更新回滚上限。"),
                    "偏好已保存，可访问项目已更新。",
                    "下次打开这些项目时会再次应用。",
                );
            }
            let _ = state.open_project().with_store(|store, summary| {
                if let Ok(mut navigation) = state.history_navigation().lock() {
                    let _ = reset_history_navigation(&mut navigation, store, &summary.project_id);
                }
            });
        }
        result_value(result)
    });
    registry.register("recent_projects_list", |state, _events, _payload| {
        result_value(preferences::recent_projects_list(
            state.app_data_dir(),
            state.preferences(),
        ))
    });
    registry.register("resolve_project_thumbnail", |state, _events, payload| {
        let params: RecentProjectOpenParams = params(payload)?;
        result_value(resolve_project_thumbnail(state, &params.project_id))
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
    registry.register("model_queue_get", |state, _events, _payload| {
        result_value(match state.models().queue_snapshot() {
            Ok(snapshot) => OperationResult::success(snapshot),
            Err(error) => OperationResult::failed("MODEL_QUEUE_FAILED", error),
        })
    });
    registry.register("model_install", |state, events, payload| {
        let params: ModelInstallParams = params(payload)?;
        if !mlx_supported() {
            return result_value(
                OperationResult::<double_love_engine::ModelInstallation>::failed(
                    "MLX_APPLE_SILICON_REQUIRED",
                    mlx_unsupported_message(),
                ),
            );
        }
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state.models().begin_install_with_runtime(
            PathBuf::from(preferences.model_root),
            preferences.model_endpoint,
            resolve_model_download_runtime(state),
            &params.model_id,
            params.accept_noncommercial_license,
            normalize_app_version(params.app_version),
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
        registry.register(name, move |state, events, payload| {
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
            let result = match state.models().set_cancel_with_events(
                Path::new(&preferences.model_root),
                &params.model_id,
                cancel,
                events.as_ref(),
            ) {
                Ok(installation) => OperationResult::success(installation),
                Err(error) => OperationResult::failed(error_code, error),
            };
            result_value(result)
        });
    }
    registry.register("model_resume", |state, events, payload| {
        let params: ModelResumeParams = params(payload)?;
        if !mlx_supported() {
            return result_value(
                OperationResult::<double_love_engine::ModelInstallation>::failed(
                    "MLX_APPLE_SILICON_REQUIRED",
                    mlx_unsupported_message(),
                ),
            );
        }
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state.models().begin_install_with_runtime(
            PathBuf::from(preferences.model_root),
            preferences.model_endpoint,
            resolve_model_download_runtime(state),
            &params.model_id,
            true,
            normalize_app_version(params.app_version),
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
    registry.register("model_legacy_cleanup_preview", |state, _events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::LegacyModelCleanupPreview,
                    >(error, true));
                }
            };
        let result = match state
            .models()
            .legacy_cleanup_preview(Path::new(&preferences.model_root), &params.model_id)
        {
            Ok(preview) => OperationResult::success(preview),
            Err(error) => OperationResult::failed("MODEL_LEGACY_CLEANUP_PREVIEW_FAILED", error),
        };
        result_value(result)
    });
    registry.register("model_legacy_cleanup_apply", |state, events, payload| {
        let params: ModelLegacyCleanupApplyParams = params(payload)?;
        if !params.confirmed {
            return result_value(OperationResult::<
                double_love_engine::LegacyModelCleanupPreview,
            >::failed(
                "MODEL_LEGACY_CLEANUP_CONFIRM_REQUIRED",
                "请确认后再清理旧模型版本。",
            ));
        }
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::LegacyModelCleanupPreview,
                    >(error, true));
                }
            };
        let result = match state
            .models()
            .cleanup_legacy(Path::new(&preferences.model_root), &params.model_id)
        {
            Ok(preview) => {
                if let Ok(snapshot) = state.models().snapshot(Path::new(&preferences.model_root)) {
                    for item in &preview.removable {
                        if let Some(installation) = snapshot
                            .iter()
                            .find(|entry| entry.descriptor.id == item.model_id)
                            .map(|entry| &entry.installation)
                            && let Ok(payload) = serde_json::to_value(installation)
                        {
                            let _ = events.emit("dl://model-state", payload);
                        }
                    }
                }
                OperationResult::success(preview)
            }
            Err(error) => OperationResult::failed("MODEL_LEGACY_CLEANUP_FAILED", error),
        };
        result_value(result)
    });
    registry.register("model_import_folder", |state, events, payload| {
        let params: ModelImportFolderParams = params(payload)?;
        if !mlx_supported() {
            return result_value(
                OperationResult::<double_love_engine::ModelInstallation>::failed(
                    "MLX_APPLE_SILICON_REQUIRED",
                    mlx_unsupported_message(),
                ),
            );
        }
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<
                        double_love_engine::ModelInstallation,
                    >(error, true));
                }
            };
        let result = match state.models().import_from_folder(
            Path::new(&preferences.model_root),
            &params.model_id,
            &params.source_path,
            params.accept_noncommercial_license,
        ) {
            Ok(installation) => {
                if let Ok(payload) = serde_json::to_value(&installation) {
                    let _ = events.emit("dl://model-state", payload);
                }
                OperationResult::success(installation)
            }
            Err(error) => OperationResult::failed("MODEL_IMPORT_FAILED", error),
        };
        result_value(result)
    });
    registry.register("model_reveal", |state, _events, payload| {
        let params: ModelIdParams = params(payload)?;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<ResolvedPath>(error, true));
                }
            };
        let path = match state
            .models()
            .reveal_installed_dir(Path::new(&preferences.model_root), &params.model_id)
        {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<ResolvedPath>::failed(
                    "MODEL_REVEAL_FAILED",
                    error,
                ));
            }
        };
        result_value(OperationResult::success(ResolvedPath {
            path: path.to_string_lossy().into_owned(),
        }))
    });
    registry.register("doctor_run", |state, events, payload| {
        let doctor_params = params::<DoctorRunParams>(payload)?;
        let app_version = normalize_app_version(doctor_params.app_version);
        let depth = doctor_params.depth;
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<RendererDoctorReport>(
                        error, true,
                    ));
                }
            };
        let profile =
            match preferences::system_profile_for(state.app_data_dir(), state.preferences()) {
                Ok(profile) => profile,
                Err(error) => {
                    return result_value(OperationResult::<RendererDoctorReport>::failed(
                        "SYSTEM_PROFILE_FAILED",
                        error.to_string(),
                    ));
                }
            };
        let runtime_probe = probe_bundled_runtime(state);
        let environment = DoctorEnvironment {
            architecture: profile.architecture,
            os_version: profile.os_version,
            memory_bytes: profile.memory_bytes,
            free_model_bytes: profile.free_model_bytes,
            ffmpeg_available: runtime_probe.ffmpeg_available,
            libass_available: runtime_probe.libass_available,
            asr_runtime_ready: runtime_probe.asr_runtime_ready,
            speaker_runtime_ready: runtime_probe.speaker_runtime_ready,
        };
        let result = match state
            .models()
            .doctor_report(Path::new(&preferences.model_root), environment)
        {
            Ok(mut report) => {
                report.capability_checks = runtime_probe.checks;
                let installed = |model_id: &str| {
                    report
                        .model_checks
                        .iter()
                        .find(|check| check.model_id == model_id)
                        .is_some_and(|check| {
                            check.state == double_love_engine::ModelInstallState::Installed
                        })
                };
                let model_root_ready = report.model_root_available;
                let asr_chain_ready = installed(&preferences.default_asr_model)
                    && installed("qwen3-forced-aligner-0.6b-8bit");
                let speaker_chain_ready =
                    installed("wespeaker-voxceleb-resnet34-lm") && installed("silero-vad-v6");
                report.capability_checks.push(doctor_check(
                    "storage.model_root",
                    if model_root_ready {
                        DoctorCapabilityStatus::Ready
                    } else {
                        DoctorCapabilityStatus::Blocked
                    },
                    if model_root_ready {
                        "模型目录可用。"
                    } else {
                        "模型目录不可用。"
                    },
                    (!model_root_ready).then_some("请在设置中重新选择可用的模型目录。"),
                ));
                report.capability_checks.push(doctor_check(
                    "models.asr_chain",
                    if asr_chain_ready {
                        DoctorCapabilityStatus::Ready
                    } else {
                        DoctorCapabilityStatus::Warning
                    },
                    if asr_chain_ready {
                        "默认转录模型与 ForcedAligner 均已校验。"
                    } else {
                        "默认转录模型或 ForcedAligner 尚未准备完成。"
                    },
                    (!asr_chain_ready).then_some("请在设置 → 本地模型中完成安装和校验。"),
                ));
                report.capability_checks.push(doctor_check(
                    "models.speaker_chain",
                    if speaker_chain_ready {
                        DoctorCapabilityStatus::Ready
                    } else {
                        DoctorCapabilityStatus::Warning
                    },
                    if speaker_chain_ready {
                        "说话人模型与 Silero VAD 均已校验。"
                    } else {
                        "说话人模型或 Silero VAD 尚未准备完成。"
                    },
                    (!speaker_chain_ready).then_some("请在设置 → 本地模型中完成安装和校验。"),
                ));
                if depth == DoctorRunDepth::Deep {
                    let deep_checks = deep_diagnostics(state, &preferences, &report);
                    report.capability_checks.extend(deep_checks);
                }
                let renderer_report = RendererDoctorReport {
                    report,
                    app_version,
                };
                models::emit_doctor(events.as_ref(), &renderer_report);
                OperationResult::success(renderer_report)
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
        if !mlx_supported() {
            return result_value(OperationResult::<Value>::failed(
                "MLX_APPLE_SILICON_REQUIRED",
                mlx_unsupported_message(),
            ));
        }
        let preferences =
            match preferences::current_preferences(state.app_data_dir(), state.preferences()) {
                Ok(preferences) => preferences,
                Err(error) => {
                    return result_value(preferences::command_error::<Value>(error, true));
                }
            };
        let model_root = Path::new(&preferences.model_root);
        let model_dir = match state
            .models()
            .installed_inference_dir(model_root, &params.model)
        {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let aligner_dir = match state
            .models()
            .installed_inference_dir(model_root, "qwen3-forced-aligner-0.6b-8bit")
        {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let media_tools = match resolve_media_tools(state) {
            Ok(tools) => tools,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed(&error.code, &error.cause));
            }
        };
        let package_dir = match resolve_asr_sidecar_dir(state) {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed(
                    "ASR_RUNTIME_MISSING",
                    error,
                ));
            }
        };
        let result = match state.open_project().with_current(|open| {
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
                asset_id: params.asset_id.clone(),
                model: params.model,
                model_dir,
                aligner_dir,
                language: params.language,
                mock: state.runtime.test_transcribe_mock,
                python: None,
                package_dir,
                log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
                prepared_dir: Path::new(&open.summary.root).join(".doublelove/prepared"),
                media_tools,
                chunk_seconds: 30,
            };
            let sink: SharedSink = Arc::new(
                ServiceProgressSink::new(events, replacements)
                    .with_active_asset_tasks(Arc::clone(&state.active_asset_tasks)),
            );
            start_transcription(Arc::clone(&open.store), state.task_registry(), sink, config)
        }) {
            Ok(Ok(task_id)) => {
                state.track_asset_task(&task_id, &params.asset_id);
                OperationResult::success(serde_json::json!({"task_id": task_id}))
            }
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
        if !mlx_supported() {
            return result_value(OperationResult::<Value>::failed(
                "MLX_APPLE_SILICON_REQUIRED",
                mlx_unsupported_message(),
            ));
        }
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
        let speaker_model_dir = match state.models().selected_speaker_model_dir(model_root) {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let vad_model_dir = match state
            .models()
            .installed_inference_dir(model_root, "silero-vad-v6")
        {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed("MODEL_NOT_READY", error));
            }
        };
        let package_dir = match resolve_speaker_sidecar_dir(state) {
            Ok(path) => path,
            Err(error) => {
                return result_value(OperationResult::<Value>::failed(
                    "SPEAKER_RUNTIME_MISSING",
                    error,
                ));
            }
        };
        let result = match state.open_project().with_current(|open| {
            let model_root_path = model_root.to_string_lossy().into_owned();
            let speaker_model_dir_path = speaker_model_dir.to_string_lossy().into_owned();
            let vad_model_dir_path = vad_model_dir.to_string_lossy().into_owned();
            let package_dir_path = package_dir.to_string_lossy().into_owned();
            let replacements = renderer_path_replacements(&[
                (&open.summary.root, "<PROJECT>"),
                (&speaker_model_dir_path, "<MODEL>"),
                (&vad_model_dir_path, "<MODEL>"),
                (&package_dir_path, "<MODEL>"),
                (&model_root_path, "<MODEL>"),
            ]);
            let config = DiarizeConfig {
                asset_id: params.asset_id.clone(),
                mock: state.runtime.test_speaker_mock,
                python: None,
                package_dir,
                log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
                vad_model_dir,
                speaker_model_dir,
            };
            let sink: SharedSink = Arc::new(
                ServiceProgressSink::new(events, replacements)
                    .with_active_asset_tasks(Arc::clone(&state.active_asset_tasks)),
            );
            start_speaker_diarization(Arc::clone(&open.store), state.task_registry(), sink, config)
        }) {
            Ok(Ok(task_id)) => {
                state.track_asset_task(&task_id, &params.asset_id);
                OperationResult::success(serde_json::json!({"task_id": task_id}))
            }
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
            let result = match resolve_media_tools(state) {
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
    fn doctor_app_version_uses_main_value_with_a_020_fallback() {
        assert_eq!(
            normalize_app_version(Some(" 0.2.1-feed ".to_string())),
            "0.2.1-feed"
        );
        assert_eq!(normalize_app_version(None), "0.2.0");
        assert_eq!(
            normalize_app_version(Some("../private".to_string())),
            "0.2.0"
        );
    }

    #[test]
    fn mlx_model_runtime_is_gated_to_apple_silicon() {
        assert!(mlx_supported_for("macos", "aarch64"));
        assert!(mlx_supported_for("macos", "arm64"));
        assert!(!mlx_supported_for("macos", "x86_64"));
        assert!(!mlx_supported_for("linux", "aarch64"));
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
    fn bundled_media_runtime_is_preferred_over_path_discovery() -> Result<(), Box<dyn Error>> {
        let root = temp_directory("bundled-media-runtime");
        let runtime_dir = root.join("runtime");
        fs::create_dir_all(&runtime_dir)?;
        let ffmpeg = runtime_dir.join("ffmpeg");
        let ffprobe = runtime_dir.join("ffprobe");
        fs::write(&ffmpeg, "#!/bin/sh\nexit 0\n")?;
        fs::write(&ffprobe, "#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755))?;
            fs::set_permissions(&ffprobe, fs::Permissions::from_mode(0o755))?;
        }

        let state = DesktopState::new(
            root.join("app-data"),
            DesktopRuntimeConfig {
                resource_dir: Some(root.clone()),
                ..DesktopRuntimeConfig::default()
            },
        );
        let tools = resolve_media_tools(&state).expect("resolve bundled media runtime");

        assert_eq!(tools.ffmpeg, ffmpeg);
        assert_eq!(tools.ffprobe, ffprobe);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn product_runtime_resolution_never_falls_back_to_external_tools() {
        let state = DesktopState::new(
            temp_directory("runtime-no-fallback"),
            DesktopRuntimeConfig::default(),
        );

        assert!(resolve_media_tools(&state).is_err());
        assert!(resolve_asr_sidecar_dir(&state).is_err());
        assert!(resolve_speaker_sidecar_dir(&state).is_err());
        assert!(resolve_model_download_runtime(&state).is_none());

        let probe = probe_bundled_runtime(&state);
        assert!(!probe.ffmpeg_available);
        assert!(!probe.asr_runtime_ready);
        assert!(probe.checks.iter().any(|check| {
            check.id == "media.ffmpeg_runtime" && check.status == DoctorCapabilityStatus::Blocked
        }));
        assert!(probe.checks.iter().any(|check| {
            check.id == "runtime.asr" && check.status == DoctorCapabilityStatus::Blocked
        }));
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
    fn terminal_task_state_releases_the_asset_delete_guard() {
        let tasks = Arc::new(Mutex::new(HashMap::from([(
            "task-1".to_string(),
            "asset-1".to_string(),
        )])));
        let events: Arc<dyn DesktopEventSink> = Arc::new(RecordingEventSink::default());
        let sink = ServiceProgressSink::new(events, Vec::new())
            .with_active_asset_tasks(Arc::clone(&tasks));

        sink.task_state("task-1", TaskState::Running);
        assert_eq!(tasks.lock().expect("tasks").len(), 1);
        sink.task_state("task-1", TaskState::Cancelled);
        assert!(tasks.lock().expect("tasks").is_empty());
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
