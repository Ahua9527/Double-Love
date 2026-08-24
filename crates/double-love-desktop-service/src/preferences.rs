//! Tauri-free application preferences, recent-project persistence, and system profile support.

use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use double_love_engine::{
    CanvasSpec, Diagnostic, DiagnosticLevel, FrameRate, OperationResult, ProjectStore,
    ProjectSummary, SubtitleStyle,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

use crate::{DesktopEventSink, DesktopServiceError};

const STORE_FILE: &str = "preferences.json";
const PRE_ELECTRON_BACKUP_FILE: &str = "preferences.json.pre-electron-backup";
const STORE_KEY: &str = "app_preferences";
pub const CURRENT_PREFERENCES_SCHEMA: u32 = 1;
pub const CURRENT_ONBOARDING_VERSION: u32 = 1;
const LOW_MEMORY_MODEL: &str = "qwen3-asr-0.6b-4bit";
const HIGH_MEMORY_MODEL: &str = "qwen3-asr-1.7b-8bit";
const LEGACY_LOW_MEMORY_MODEL: &str = "qwen3-asr-0.6b";
const LEGACY_HIGH_MEMORY_MODEL: &str = "qwen3-asr-1.7b";
const MODEL_ENDPOINT: &str = "https://huggingface.co";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimecodePrecision {
    Frame,
    Millisecond,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectLibraryView {
    Grid,
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProjectRecord {
    pub project_id: Option<String>,
    pub root: String,
    pub display_name: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentProject {
    pub project_id: Option<String>,
    pub root: String,
    pub display_name: String,
    pub last_opened_at: String,
    pub exists: bool,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub canvas: Option<CanvasSpec>,
    pub output_rate: Option<FrameRate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppPreferencesV1 {
    pub schema_version: u32,
    pub theme: ThemeMode,
    pub restore_last_project: bool,
    pub timecode_precision: TimecodePrecision,
    pub project_library_view: ProjectLibraryView,
    pub history_limit: Option<u32>,
    pub transcript_section_tint: bool,
    pub cjk_spacing: bool,
    pub default_subtitle_style: SubtitleStyle,
    pub model_root: String,
    pub model_endpoint: String,
    pub default_asr_model: String,
    pub onboarding_version: u32,
    pub onboarding_completed: bool,
    pub recent_projects: Vec<RecentProjectRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreferencesPatch {
    pub theme: Option<ThemeMode>,
    pub restore_last_project: Option<bool>,
    pub project_library_view: Option<ProjectLibraryView>,
    #[serde(default, deserialize_with = "deserialize_nullable_history_limit")]
    pub history_limit: Option<Option<u32>>,
    pub transcript_section_tint: Option<bool>,
    pub cjk_spacing: Option<bool>,
    pub default_subtitle_style: Option<SubtitleStyle>,
    pub default_asr_model: Option<String>,
    pub model_endpoint: Option<String>,
    pub model_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemProfile {
    pub memory_bytes: u64,
    pub architecture: String,
    pub os_version: String,
    pub free_model_bytes: u64,
    pub recommended_asr_model: String,
}

fn deserialize_nullable_history_limit<'de, D>(
    deserializer: D,
) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u32>::deserialize(deserializer)?))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingState {
    pub version: u32,
    pub completed: bool,
    pub step: u8,
}

#[derive(Debug, Clone)]
pub enum PreferencesError {
    Path(String),
    Store(String),
    Io(String),
    Decode(String),
    Invalid(String),
    Endpoint,
}

impl std::fmt::Display for PreferencesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(message)
            | Self::Store(message)
            | Self::Io(message)
            | Self::Decode(message)
            | Self::Invalid(message) => f.write_str(message),
            Self::Endpoint => {
                f.write_str("模型下载源必须是没有账号、密码或查询参数的 HTTPS 地址。")
            }
        }
    }
}

impl std::error::Error for PreferencesError {}

#[derive(Debug, Default)]
pub struct PreferencesState {
    lock: Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
struct PreferencesChanged<'a> {
    changed_keys: &'a [String],
}

pub fn default_preferences_for_root(model_root: impl Into<String>) -> AppPreferencesV1 {
    AppPreferencesV1 {
        schema_version: CURRENT_PREFERENCES_SCHEMA,
        theme: ThemeMode::Light,
        restore_last_project: true,
        timecode_precision: TimecodePrecision::Frame,
        project_library_view: ProjectLibraryView::Grid,
        history_limit: Some(200),
        transcript_section_tint: true,
        cjk_spacing: true,
        default_subtitle_style: SubtitleStyle::default(),
        model_root: model_root.into(),
        model_endpoint: MODEL_ENDPOINT.to_string(),
        default_asr_model: LOW_MEMORY_MODEL.to_string(),
        onboarding_version: CURRENT_ONBOARDING_VERSION,
        onboarding_completed: false,
        recent_projects: Vec::new(),
    }
}

fn default_preferences(app_data_dir: &Path) -> AppPreferencesV1 {
    let mut preferences =
        default_preferences_for_root(app_data_dir.join("models").to_string_lossy().into_owned());
    preferences.default_asr_model = if read_memory_bytes() < 16 * 1024 * 1024 * 1024 {
        LOW_MEMORY_MODEL.to_string()
    } else {
        HIGH_MEMORY_MODEL.to_string()
    };
    preferences
}

fn store_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(STORE_FILE)
}

fn corrupt_store_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("preferences");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("json");
    let timestamp = OffsetDateTime::now_utc().unix_timestamp();
    let mut candidate = parent.join(format!("{stem}.corrupt.{timestamp}.{extension}"));
    let mut suffix = 1_u32;
    while candidate.exists() {
        candidate = parent.join(format!("{stem}.corrupt.{timestamp}.{suffix}.{extension}"));
        suffix += 1;
    }
    candidate
}

fn move_corrupt_store(path: &Path) -> Result<(), PreferencesError> {
    if path.exists() {
        fs::rename(path, corrupt_store_path(path))
            .map_err(|error| PreferencesError::Io(error.to_string()))?;
    }
    Ok(())
}

fn endpoint_is_allowed(endpoint: &str) -> bool {
    let Ok(url) = Url::parse(endpoint) else {
        return false;
    };
    let local_fixture = cfg!(test)
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some();
    if url.scheme() != "https" && !local_fixture {
        return false;
    }
    url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn validate_preferences(value: &AppPreferencesV1) -> Result<(), PreferencesError> {
    if value.schema_version != CURRENT_PREFERENCES_SCHEMA {
        return Err(PreferencesError::Invalid(format!(
            "不支持的偏好版本 {}。",
            value.schema_version
        )));
    }
    if value.model_root.trim().is_empty() || !Path::new(&value.model_root).is_absolute() {
        return Err(PreferencesError::Invalid(
            "模型目录必须是非空的绝对路径。".to_string(),
        ));
    }
    if !endpoint_is_allowed(&value.model_endpoint) {
        return Err(PreferencesError::Endpoint);
    }
    if !matches!(
        value.default_asr_model.as_str(),
        LOW_MEMORY_MODEL | HIGH_MEMORY_MODEL
    ) {
        return Err(PreferencesError::Invalid(
            "默认 ASR 模型必须是 Qwen3 ASR 的当前 MLX 版本。".to_string(),
        ));
    }
    if value
        .history_limit
        .is_some_and(|limit| !matches!(limit, 50 | 100 | 200 | 500 | 1000))
    {
        return Err(PreferencesError::Invalid(
            "回滚上限必须是 50、100、200、500、1000 或不设上限。".to_string(),
        ));
    }
    Ok(())
}

fn migrate_default_asr_model(value: &str) -> &str {
    match value {
        LEGACY_LOW_MEMORY_MODEL => LOW_MEMORY_MODEL,
        LEGACY_HIGH_MEMORY_MODEL => HIGH_MEMORY_MODEL,
        _ => value,
    }
}

fn decode_preferences(
    raw: &Value,
    defaults: &AppPreferencesV1,
) -> Result<AppPreferencesV1, PreferencesError> {
    let Some(object) = raw.as_object() else {
        return Err(PreferencesError::Decode(
            "偏好值必须是 JSON 对象。".to_string(),
        ));
    };
    if let Some(version) = object.get("schema_version") {
        let Some(version) = version.as_u64() else {
            return Err(PreferencesError::Decode(
                "schema_version 必须是整数。".to_string(),
            ));
        };
        if version > CURRENT_PREFERENCES_SCHEMA as u64 {
            return Err(PreferencesError::Invalid(format!(
                "不支持的偏好版本 {version}。"
            )));
        }
    }
    let mut merged = serde_json::to_value(defaults)
        .map_err(|error| PreferencesError::Decode(error.to_string()))?;
    let Some(merged_object) = merged.as_object_mut() else {
        return Err(PreferencesError::Decode("默认偏好值不是对象。".to_string()));
    };
    for (key, value) in object {
        if merged_object.contains_key(key) {
            merged_object.insert(key.clone(), value.clone());
        }
    }
    merged_object.insert(
        "schema_version".to_string(),
        Value::from(CURRENT_PREFERENCES_SCHEMA),
    );
    let mut value: AppPreferencesV1 = serde_json::from_value(merged)
        .map_err(|error| PreferencesError::Decode(error.to_string()))?;
    if !object.contains_key("history_limit") {
        value.history_limit = None;
    }
    value.default_asr_model = migrate_default_asr_model(&value.default_asr_model).to_string();
    value.timecode_precision = TimecodePrecision::Frame;
    value.recent_projects = normalize_recent_records(value.recent_projects);
    validate_preferences(&value)?;
    Ok(value)
}

fn decode_store_bytes(
    bytes: &[u8],
    defaults: &AppPreferencesV1,
) -> Result<Option<AppPreferencesV1>, PreferencesError> {
    let values: HashMap<String, Value> = serde_json::from_slice(bytes)
        .map_err(|error| PreferencesError::Decode(error.to_string()))?;
    values
        .get(STORE_KEY)
        .map(|raw| decode_preferences(raw, defaults))
        .transpose()
}

fn save_preferences_unlocked(
    app_data_dir: &Path,
    value: &AppPreferencesV1,
) -> Result<(), PreferencesError> {
    fs::create_dir_all(app_data_dir).map_err(|error| PreferencesError::Io(error.to_string()))?;
    let path = store_path(app_data_dir);
    let temporary = app_data_dir.join(format!(".{STORE_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({ STORE_KEY: value }))
        .map_err(|error| PreferencesError::Decode(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options
        .open(&temporary)
        .map_err(|error| PreferencesError::Io(error.to_string()))?;
    output
        .write_all(&bytes)
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.sync_all())
        .map_err(|error| PreferencesError::Io(error.to_string()))?;
    drop(output);
    fs::rename(&temporary, &path).map_err(|error| PreferencesError::Io(error.to_string()))?;
    #[cfg(unix)]
    fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|error| PreferencesError::Io(error.to_string()))?;
    Ok(())
}

fn backup_existing_preferences_unlocked(path: &Path) -> Result<(), PreferencesError> {
    let backup = path.with_file_name(PRE_ELECTRON_BACKUP_FILE);
    match fs::symlink_metadata(&backup) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(PreferencesError::Io(error.to_string())),
    }

    let mut input =
        fs::File::open(path).map_err(|error| PreferencesError::Io(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = match options.open(&backup) {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(PreferencesError::Io(error.to_string())),
    };
    let copy_result = (|| -> io::Result<()> {
        io::copy(&mut input, &mut output)?;
        #[cfg(unix)]
        fs::set_permissions(&backup, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        output.sync_all()
    })();
    if let Err(error) = copy_result {
        drop(output);
        let _ = fs::remove_file(&backup);
        return Err(PreferencesError::Io(error.to_string()));
    }
    Ok(())
}

fn with_preferences<T>(
    app_data_dir: &Path,
    state: &PreferencesState,
    operation: impl FnOnce(&AppPreferencesV1, bool) -> Result<T, PreferencesError>,
) -> Result<(T, bool), PreferencesError> {
    let _guard = state
        .lock
        .lock()
        .map_err(|_| PreferencesError::Store("preferences lock is unavailable".to_string()))?;
    let defaults = default_preferences(app_data_dir);
    let path = store_path(app_data_dir);
    let store_exists = path
        .try_exists()
        .map_err(|error| PreferencesError::Io(error.to_string()))?;
    let (preferences, recovered, has_preferences) = if store_exists {
        backup_existing_preferences_unlocked(&path)?;
        let bytes = fs::read(&path).map_err(|error| PreferencesError::Io(error.to_string()))?;
        match decode_store_bytes(&bytes, &defaults) {
            Ok(Some(preferences)) => (preferences, false, true),
            Ok(None) => (defaults, false, false),
            Err(_) => {
                move_corrupt_store(&path)?;
                (defaults, true, false)
            }
        }
    } else {
        (defaults, false, false)
    };
    if recovered {
        save_preferences_unlocked(app_data_dir, &preferences)?;
    }
    let result = operation(&preferences, has_preferences)?;
    Ok((result, recovered))
}

pub fn current_preferences(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> Result<AppPreferencesV1, PreferencesError> {
    with_preferences(app_data_dir, state, |preferences, existed| {
        if !existed {
            save_preferences_unlocked(app_data_dir, preferences)?;
        }
        Ok(preferences.clone())
    })
    .map(|(preferences, _)| preferences)
}

fn changed_keys_from_patch(patch: &PreferencesPatch) -> Vec<String> {
    let mut changed = Vec::new();
    if patch.theme.is_some() {
        changed.push("theme".to_string());
    }
    if patch.restore_last_project.is_some() {
        changed.push("restore_last_project".to_string());
    }
    if patch.project_library_view.is_some() {
        changed.push("project_library_view".to_string());
    }
    if patch.history_limit.is_some() {
        changed.push("history_limit".to_string());
    }
    if patch.transcript_section_tint.is_some() {
        changed.push("transcript_section_tint".to_string());
    }
    if patch.cjk_spacing.is_some() {
        changed.push("cjk_spacing".to_string());
    }
    if patch.default_subtitle_style.is_some() {
        changed.push("default_subtitle_style".to_string());
    }
    if patch.default_asr_model.is_some() {
        changed.push("default_asr_model".to_string());
    }
    if patch.model_endpoint.is_some() {
        changed.push("model_endpoint".to_string());
    }
    if patch.model_root.is_some() {
        changed.push("model_root".to_string());
    }
    changed
}

fn apply_patch(
    mut value: AppPreferencesV1,
    patch: &PreferencesPatch,
) -> Result<AppPreferencesV1, PreferencesError> {
    if let Some(theme) = patch.theme {
        value.theme = theme;
    }
    if let Some(restore) = patch.restore_last_project {
        value.restore_last_project = restore;
    }
    if let Some(view) = patch.project_library_view {
        value.project_library_view = view;
    }
    if let Some(limit) = patch.history_limit {
        value.history_limit = limit;
    }
    if let Some(tint) = patch.transcript_section_tint {
        value.transcript_section_tint = tint;
    }
    if let Some(spacing) = patch.cjk_spacing {
        value.cjk_spacing = spacing;
        value.default_subtitle_style.cjk_spacing = spacing;
    }
    if let Some(style) = &patch.default_subtitle_style {
        value.default_subtitle_style = style.clone();
    }
    if let Some(model) = &patch.default_asr_model {
        value.default_asr_model = model.clone();
    }
    if let Some(endpoint) = &patch.model_endpoint {
        if !endpoint_is_allowed(endpoint) {
            return Err(PreferencesError::Endpoint);
        }
        value.model_endpoint = endpoint.clone();
    }
    if let Some(root) = &patch.model_root {
        if root.trim().is_empty() || !Path::new(root).is_absolute() {
            return Err(PreferencesError::Invalid(
                "模型目录必须是非空的绝对路径。".to_string(),
            ));
        }
        value.model_root = root.clone();
    }
    value.schema_version = CURRENT_PREFERENCES_SCHEMA;
    validate_preferences(&value)?;
    Ok(value)
}

fn warning<T>(
    mut result: OperationResult<T>,
    code: &str,
    cause: impl Into<String>,
) -> OperationResult<T> {
    result.diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: code.to_string(),
        cause: cause.into(),
        object_id: None,
        impact: "已使用完整默认值恢复，原文件已保留为备份。".to_string(),
        blocks_export: false,
        suggested_action: None,
    });
    result
}

pub(crate) fn command_error<T>(error: PreferencesError, read: bool) -> OperationResult<T> {
    let code = match &error {
        PreferencesError::Endpoint => "MODEL_ENDPOINT_INVALID",
        PreferencesError::Invalid(_) => "PREFERENCES_INVALID_FIELD",
        PreferencesError::Path(_)
        | PreferencesError::Store(_)
        | PreferencesError::Io(_)
        | PreferencesError::Decode(_) => {
            if read {
                "PREFERENCES_READ_FAILED"
            } else {
                "PREFERENCES_WRITE_FAILED"
            }
        }
    };
    OperationResult::failed(code, error.to_string())
}

fn emit_preferences_changed(
    event_sink: &dyn DesktopEventSink,
    changed_keys: &[String],
) -> Result<(), DesktopServiceError> {
    let payload = serde_json::to_value(PreferencesChanged { changed_keys })
        .map_err(|error| DesktopServiceError::internal(error.to_string()))?;
    event_sink.emit("dl://preferences-changed", payload)
}

pub fn preferences_get(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> OperationResult<AppPreferencesV1> {
    match with_preferences(app_data_dir, state, |preferences, existed| {
        if !existed {
            save_preferences_unlocked(app_data_dir, preferences)?;
        }
        Ok(preferences.clone())
    }) {
        Ok((preferences, recovered)) => {
            let result = OperationResult::success(preferences);
            if recovered {
                warning(
                    result,
                    "PREFERENCES_RECOVERED",
                    "偏好文件损坏，已恢复为默认值。",
                )
            } else {
                result
            }
        }
        Err(error) => command_error(error, true),
    }
}

pub fn preferences_update(
    app_data_dir: &Path,
    state: &PreferencesState,
    event_sink: &dyn DesktopEventSink,
    patch: PreferencesPatch,
) -> OperationResult<AppPreferencesV1> {
    let changed_keys = changed_keys_from_patch(&patch);
    match with_preferences(app_data_dir, state, |preferences, _| {
        let next = apply_patch(preferences.clone(), &patch)?;
        save_preferences_unlocked(app_data_dir, &next)?;
        Ok(next)
    }) {
        Ok((preferences, recovered)) => {
            let _ = emit_preferences_changed(event_sink, &changed_keys);
            let result = OperationResult::success(preferences);
            if recovered {
                warning(
                    result,
                    "PREFERENCES_RECOVERED",
                    "偏好文件损坏，已恢复默认值后应用更新。",
                )
            } else {
                result
            }
        }
        Err(error) => command_error(error, false),
    }
}

fn canonical_project_root(root: &str) -> Result<String, PreferencesError> {
    let path = Path::new(root);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| PreferencesError::Io(error.to_string()))?
            .join(path)
    };
    if absolute.exists() {
        absolute
            .canonicalize()
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|error| PreferencesError::Io(error.to_string()))
    } else {
        let mut normalized = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                _ => normalized.push(component.as_os_str()),
            }
        }
        Ok(normalized.to_string_lossy().into_owned())
    }
}

fn normalize_recent_records(mut records: Vec<RecentProjectRecord>) -> Vec<RecentProjectRecord> {
    records.sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
    let mut project_ids = HashSet::new();
    let mut roots = HashSet::new();
    records
        .into_iter()
        .filter_map(|mut record| {
            record.root = canonical_project_root(&record.root).unwrap_or(record.root);
            let duplicate_id = record
                .project_id
                .as_ref()
                .is_some_and(|project_id| !project_ids.insert(project_id.clone()));
            if duplicate_id || !roots.insert(record.root.clone()) {
                None
            } else {
                Some(record)
            }
        })
        .collect()
}

fn display_name_for_root(root: &str) -> String {
    Path::new(root)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名项目")
        .to_string()
}

fn utc_now() -> Result<String, PreferencesError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| PreferencesError::Io(error.to_string()))
}

fn file_created_at(path: &Path) -> Option<String> {
    let created = path.metadata().ok()?.created().ok()?;
    OffsetDateTime::from(created).format(&Rfc3339).ok()
}

fn upsert_recent(
    mut preferences: AppPreferencesV1,
    summary: &ProjectSummary,
) -> Result<AppPreferencesV1, PreferencesError> {
    let root = canonical_project_root(&summary.root)?;
    let record = RecentProjectRecord {
        project_id: Some(summary.project_id.clone()),
        root: root.clone(),
        display_name: display_name_for_root(&root),
        last_opened_at: utc_now()?,
    };
    preferences.recent_projects.retain(|item| {
        item.root != root && item.project_id.as_deref() != Some(summary.project_id.as_str())
    });
    preferences.recent_projects.insert(0, record);
    // The freshly opened project is authoritative even when multiple opens share the same
    // wall-clock tick (or the system clock moves backwards). Existing records were normalized
    // when read, so sorting again here can incorrectly move the newest record behind an older one.
    Ok(preferences)
}

pub fn record_recent_project(
    app_data_dir: &Path,
    state: &PreferencesState,
    event_sink: &dyn DesktopEventSink,
    summary: &ProjectSummary,
) -> Result<(), PreferencesError> {
    let result = with_preferences(app_data_dir, state, |preferences, _| {
        let next = upsert_recent(preferences.clone(), summary)?;
        save_preferences_unlocked(app_data_dir, &next)
    });
    if result.is_ok() {
        let _ = emit_preferences_changed(event_sink, &["recent_projects".to_string()]);
    }
    result.map(|_| ())
}

fn recent_projects(preferences: &AppPreferencesV1) -> Vec<RecentProject> {
    normalize_recent_records(preferences.recent_projects.clone())
        .into_iter()
        .map(|record| {
            let exists = Path::new(&record.root).exists();
            let database = Path::new(&record.root).join(".doublelove/project.sqlite");
            let metadata = database
                .is_file()
                .then(|| ProjectStore::open(&database))
                .and_then(Result::ok)
                .and_then(|store| store.project_library_metadata().ok());
            let created_at = metadata
                .as_ref()
                .and_then(|value| value.created_at.clone())
                .or_else(|| file_created_at(&database));
            RecentProject {
                exists,
                project_id: record.project_id,
                root: record.root,
                display_name: record.display_name,
                last_opened_at: record.last_opened_at,
                created_at,
                modified_at: metadata
                    .as_ref()
                    .and_then(|value| value.modified_at.clone()),
                canvas: metadata.as_ref().map(|value| value.canvas.clone()),
                output_rate: metadata.and_then(|value| value.output_rate),
            }
        })
        .collect()
}

pub fn recent_project_path(
    app_data_dir: &Path,
    state: &PreferencesState,
    project_id: &str,
) -> Result<PathBuf, PreferencesError> {
    if project_id.trim().is_empty() {
        return Err(PreferencesError::Invalid("项目标识不能为空。".to_string()));
    }
    with_preferences(app_data_dir, state, |preferences, _| {
        let record = normalize_recent_records(preferences.recent_projects.clone())
            .into_iter()
            .find(|record| record.project_id.as_deref() == Some(project_id))
            .ok_or_else(|| PreferencesError::Invalid("项目不在我的项目列表中。".to_string()))?;
        let path = PathBuf::from(record.root);
        if !path.exists() {
            return Err(PreferencesError::Invalid("项目位置已经丢失。".to_string()));
        }
        Ok(path)
    })
    .map(|(path, _)| path)
}

pub fn registered_project_paths(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> Result<Vec<PathBuf>, PreferencesError> {
    with_preferences(app_data_dir, state, |preferences, _| {
        Ok(
            normalize_recent_records(preferences.recent_projects.clone())
                .into_iter()
                .map(|record| PathBuf::from(record.root))
                .collect(),
        )
    })
    .map(|(paths, _)| paths)
}

pub fn recent_projects_list(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> OperationResult<Vec<RecentProject>> {
    match with_preferences(app_data_dir, state, |preferences, existed| {
        if !existed {
            save_preferences_unlocked(app_data_dir, preferences)?;
        }
        Ok(recent_projects(preferences))
    }) {
        Ok((projects, _)) => OperationResult::success(projects),
        Err(error) => command_error(error, true),
    }
}

pub fn recent_project_forget(
    app_data_dir: &Path,
    state: &PreferencesState,
    event_sink: &dyn DesktopEventSink,
    root: String,
) -> OperationResult<()> {
    let canonical = match canonical_project_root(&root) {
        Ok(root) => root,
        Err(error) => return command_error(error, false),
    };
    match with_preferences(app_data_dir, state, |preferences, _| {
        if !preferences
            .recent_projects
            .iter()
            .any(|item| item.root == canonical)
        {
            return Err(PreferencesError::Invalid(
                "最近项目记录不存在。".to_string(),
            ));
        }
        let mut next = preferences.clone();
        next.recent_projects.retain(|item| item.root != canonical);
        save_preferences_unlocked(app_data_dir, &next)
    }) {
        Ok(((), _)) => {
            let _ = emit_preferences_changed(event_sink, &["recent_projects".to_string()]);
            OperationResult::success(())
        }
        Err(PreferencesError::Invalid(message)) if message == "最近项目记录不存在。" => {
            OperationResult::failed("RECENT_PROJECT_NOT_FOUND", message)
        }
        Err(error) => command_error(error, false),
    }
}

fn read_memory_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("sysctl").args(["-n", "hw.memsize"]).output()
            && let Ok(value) = String::from_utf8(output.stdout)
            && let Ok(bytes) = value.trim().parse()
        {
            return bytes;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = fs::read_to_string("/proc/meminfo")
            && let Some(line) = contents.lines().find(|line| line.starts_with("MemTotal:"))
            && let Some(kib) = line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse::<u64>().ok())
        {
            return kib.saturating_mul(1024);
        }
    }
    0
}

fn read_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("sw_vers").arg("-productVersion").output()
            && let Ok(value) = String::from_utf8(output.stdout)
            && !value.trim().is_empty()
        {
            return value.trim().to_string();
        }
    }
    std::env::consts::OS.to_string()
}

fn architecture() -> String {
    match std::env::consts::ARCH {
        "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    }
}

fn free_bytes(path: &Path) -> u64 {
    let mut existing = path.to_path_buf();
    while !existing.exists() {
        if !existing.pop() {
            return 0;
        }
    }
    #[cfg(unix)]
    {
        let Ok(path) = CString::new(existing.to_string_lossy().as_bytes()) else {
            return 0;
        };
        // SAFETY: path is a NUL-free C string and statvfs initializes the POD.
        unsafe {
            let mut stats: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(path.as_ptr(), &mut stats) == 0 {
                return (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64);
            }
        }
    }
    0
}

pub fn system_profile_for(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> Result<SystemProfile, PreferencesError> {
    let preferences = current_preferences(app_data_dir, state)?;
    let model_root = PathBuf::from(preferences.model_root);
    let memory_bytes = read_memory_bytes();
    let recommended_asr_model = if memory_bytes < 16 * 1024 * 1024 * 1024 {
        LOW_MEMORY_MODEL
    } else {
        HIGH_MEMORY_MODEL
    };
    Ok(SystemProfile {
        memory_bytes,
        architecture: architecture(),
        os_version: read_os_version(),
        free_model_bytes: free_bytes(&model_root),
        recommended_asr_model: recommended_asr_model.to_string(),
    })
}

pub fn system_profile(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> OperationResult<SystemProfile> {
    match system_profile_for(app_data_dir, state) {
        Ok(profile) => OperationResult::success(profile),
        Err(error) => command_error(error, true),
    }
}

fn onboarding_from_preferences(preferences: &AppPreferencesV1) -> OnboardingState {
    let completed = preferences.onboarding_completed
        && preferences.onboarding_version >= CURRENT_ONBOARDING_VERSION;
    OnboardingState {
        version: CURRENT_ONBOARDING_VERSION,
        completed,
        step: if completed { 3 } else { 1 },
    }
}

pub fn onboarding_get(
    app_data_dir: &Path,
    state: &PreferencesState,
) -> OperationResult<OnboardingState> {
    match with_preferences(app_data_dir, state, |preferences, existed| {
        if !existed {
            save_preferences_unlocked(app_data_dir, preferences)?;
        }
        Ok(onboarding_from_preferences(preferences))
    }) {
        Ok((onboarding, _)) => OperationResult::success(onboarding),
        Err(error) => command_error(error, true),
    }
}

pub fn onboarding_complete(
    app_data_dir: &Path,
    state: &PreferencesState,
    event_sink: &dyn DesktopEventSink,
    default_asr_model: Option<String>,
    step: Option<u8>,
) -> OperationResult<OnboardingState> {
    if let Some(step) = step
        && !(1..=3).contains(&step)
    {
        return OperationResult::failed("ONBOARDING_STEP_INVALID", "引导步骤必须是 1、2 或 3。");
    }
    match with_preferences(app_data_dir, state, |preferences, _| {
        let mut next = preferences.clone();
        if let Some(model) = default_asr_model.as_deref() {
            if !matches!(model, LOW_MEMORY_MODEL | HIGH_MEMORY_MODEL) {
                return Err(PreferencesError::Invalid(
                    "默认 ASR 模型必须是 Qwen3 ASR 的当前 MLX 版本。".to_string(),
                ));
            }
            next.default_asr_model = model.to_string();
        }
        next.onboarding_version = CURRENT_ONBOARDING_VERSION;
        next.onboarding_completed = true;
        save_preferences_unlocked(app_data_dir, &next)?;
        Ok(onboarding_from_preferences(&next))
    }) {
        Ok((onboarding, _)) => {
            let changed = if default_asr_model.is_some() {
                vec![
                    "onboarding_completed".to_string(),
                    "default_asr_model".to_string(),
                ]
            } else {
                vec!["onboarding_completed".to_string()]
            };
            let _ = emit_preferences_changed(event_sink, &changed);
            OperationResult::success(onboarding)
        }
        Err(error) => command_error(error, false),
    }
}

pub fn onboarding_reset(
    app_data_dir: &Path,
    state: &PreferencesState,
    event_sink: &dyn DesktopEventSink,
) -> OperationResult<OnboardingState> {
    match with_preferences(app_data_dir, state, |preferences, _| {
        let mut next = preferences.clone();
        next.onboarding_version = CURRENT_ONBOARDING_VERSION;
        next.onboarding_completed = false;
        save_preferences_unlocked(app_data_dir, &next)?;
        Ok(onboarding_from_preferences(&next))
    }) {
        Ok((onboarding, _)) => {
            let _ = emit_preferences_changed(event_sink, &["onboarding_completed".to_string()]);
            OperationResult::success(onboarding)
        }
        Err(error) => command_error(error, false),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temp_directory(label: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "double-love-preferences-{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn summary(root: &str, id: &str) -> ProjectSummary {
        ProjectSummary {
            project_id: id.to_string(),
            root: root.to_string(),
            database: format!("{root}/.doublelove/project.sqlite"),
            revision: 0,
        }
    }

    #[test]
    fn defaults_are_stable_and_local_first() {
        let preferences = default_preferences_for_root("/tmp/double-love/models");
        assert_eq!(preferences.schema_version, 1);
        assert_eq!(preferences.theme, ThemeMode::Light);
        assert!(preferences.restore_last_project);
        assert_eq!(preferences.default_asr_model, LOW_MEMORY_MODEL);
        assert_eq!(preferences.history_limit, Some(200));
        assert!(preferences.recent_projects.is_empty());
    }

    #[test]
    fn legacy_asr_preference_maps_to_its_mlx_successor() {
        let defaults = default_preferences_for_root("/tmp/double-love/models");
        for (legacy, current) in [
            (LEGACY_LOW_MEMORY_MODEL, LOW_MEMORY_MODEL),
            (LEGACY_HIGH_MEMORY_MODEL, HIGH_MEMORY_MODEL),
        ] {
            let value = serde_json::json!({
                "app_preferences": {
                    "schema_version": 1,
                    "default_asr_model": legacy
                }
            });
            let decoded = decode_store_bytes(
                &serde_json::to_vec(&value).expect("preferences JSON"),
                &defaults,
            )
            .expect("legacy preference decodes")
            .expect("preferences");
            assert_eq!(decoded.default_asr_model, current);
        }
    }

    #[test]
    fn patch_updates_only_declared_fields_and_validates_endpoint() {
        let defaults = default_preferences_for_root("/tmp/double-love/models");
        let patch = PreferencesPatch {
            theme: Some(ThemeMode::Dark),
            project_library_view: Some(ProjectLibraryView::List),
            model_endpoint: Some("https://mirror.example.test/hub".to_string()),
            ..PreferencesPatch::default()
        };
        let updated = apply_patch(defaults.clone(), &patch).expect("valid patch");
        assert_eq!(updated.theme, ThemeMode::Dark);
        assert_eq!(updated.project_library_view, ProjectLibraryView::List);
        assert_eq!(updated.model_endpoint, "https://mirror.example.test/hub");
        assert_eq!(updated.restore_last_project, defaults.restore_last_project);

        let invalid = PreferencesPatch {
            model_endpoint: Some("https://user:token@example.test".to_string()),
            ..PreferencesPatch::default()
        };
        assert!(matches!(
            apply_patch(defaults, &invalid),
            Err(PreferencesError::Endpoint)
        ));

        let unlimited: PreferencesPatch = serde_json::from_value(serde_json::json!({
            "history_limit": null
        }))
        .expect("nullable history limit patch");
        assert_eq!(unlimited.history_limit, Some(None));
        assert_eq!(
            apply_patch(
                default_preferences_for_root("/tmp/double-love/models"),
                &unlimited,
            )
            .expect("unlimited applies")
            .history_limit,
            None
        );
    }

    #[test]
    fn local_http_fixture_endpoint_is_test_only() {
        assert!(endpoint_is_allowed("http://127.0.0.1:3123/models"));
        assert!(!endpoint_is_allowed("http://localhost:3123/models"));
    }

    #[test]
    fn recent_projects_are_complete_and_deduplicated_by_id_or_path() {
        let root_base = temp_directory("recent-projects");
        let mut preferences = default_preferences_for_root("/tmp/double-love/models");
        for index in 0..25 {
            let root = root_base.join(format!("project-{index}"));
            let root = root.to_string_lossy().into_owned();
            preferences = upsert_recent(preferences, &summary(&root, &index.to_string())).unwrap();
        }
        assert_eq!(preferences.recent_projects.len(), 25);
        assert_eq!(
            preferences.recent_projects[0].root,
            root_base.join("project-24").to_string_lossy()
        );

        preferences = upsert_recent(
            preferences,
            &summary(
                &root_base.join("project-10").to_string_lossy(),
                "replacement",
            ),
        )
        .unwrap();
        assert_eq!(preferences.recent_projects.len(), 25);
        assert_eq!(
            preferences.recent_projects[0].project_id.as_deref(),
            Some("replacement")
        );

        preferences = upsert_recent(
            preferences,
            &summary(
                &root_base.join("project-10-relocated").to_string_lossy(),
                "replacement",
            ),
        )
        .unwrap();
        assert_eq!(preferences.recent_projects.len(), 25);
        assert_eq!(
            preferences.recent_projects[0].root,
            root_base.join("project-10-relocated").to_string_lossy()
        );
        assert!(
            preferences
                .recent_projects
                .iter()
                .all(|record| record.root != root_base.join("project-10").to_string_lossy())
        );
    }

    #[test]
    fn tauri_v1_fixture_decodes_to_the_same_complete_values() {
        let bytes = include_bytes!("../tests/fixtures/preferences/v1.json");
        let fixture: HashMap<String, Value> =
            serde_json::from_slice(bytes).expect("v1 fixture json");
        let mut expected: AppPreferencesV1 =
            serde_json::from_value(fixture.get(STORE_KEY).expect("preferences fixture").clone())
                .expect("complete v1 preferences fixture");
        expected.timecode_precision = TimecodePrecision::Frame;
        expected.default_asr_model = LOW_MEMORY_MODEL.to_string();
        let defaults = default_preferences_for_root("/tmp/double-love/default-models");
        let decoded = decode_store_bytes(bytes, &defaults)
            .expect("v1 store is valid")
            .expect("preferences key exists");

        assert_eq!(decoded, expected);
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.theme, ThemeMode::Dark);
        assert!(!decoded.restore_last_project);
        assert_eq!(decoded.timecode_precision, TimecodePrecision::Frame);
        assert!(decoded.onboarding_completed);
    }

    #[test]
    fn tauri_partial_v0_fixture_matches_the_frozen_migration_result() {
        let bytes = include_bytes!("../tests/fixtures/preferences/partial-v0.json");
        let fixture: HashMap<String, Value> =
            serde_json::from_slice(bytes).expect("partial fixture json");
        let expected: AppPreferencesV1 = serde_json::from_value(
            fixture
                .get("expected_after_migration")
                .expect("complete migration expectation")
                .clone(),
        )
        .expect("complete migrated preferences fixture");
        let defaults = default_preferences_for_root("/tmp/double-love/default-models");
        let migrated = decode_store_bytes(bytes, &defaults)
            .expect("partial store migrates")
            .expect("preferences key exists");

        assert_eq!(migrated, expected);
    }

    #[test]
    fn tauri_corrupt_fixture_is_classified_as_decode_failure() {
        let defaults = default_preferences_for_root("/tmp/double-love/default-models");
        assert!(matches!(
            decode_store_bytes(
                include_bytes!("../tests/fixtures/preferences/corrupt.json"),
                &defaults
            ),
            Err(PreferencesError::Decode(_))
        ));
    }

    #[test]
    fn preexisting_store_gets_one_unchanging_pre_electron_backup() {
        let app_data = temp_directory("pre-electron-backup");
        fs::create_dir_all(&app_data).expect("app data");
        let fixture = include_bytes!("../tests/fixtures/preferences/v1.json");
        fs::write(app_data.join(STORE_FILE), fixture).expect("preferences fixture");
        let state = PreferencesState::default();

        let first = preferences_get(&app_data, &state);
        assert_eq!(
            first.data.as_ref().expect("preferences").theme,
            ThemeMode::Dark
        );
        let backup = app_data.join(PRE_ELECTRON_BACKUP_FILE);
        assert_eq!(fs::read(&backup).expect("preferences backup"), fixture);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&backup)
                    .expect("backup metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        let mut changed = first.data.expect("preferences data");
        changed.theme = ThemeMode::Light;
        save_preferences_unlocked(&app_data, &changed).expect("store changes after backup");
        let second = preferences_get(&app_data, &state);
        assert_eq!(
            second.data.expect("changed preferences").theme,
            ThemeMode::Light
        );
        assert_eq!(fs::read(&backup).expect("unchanged backup"), fixture);

        fs::remove_dir_all(app_data).expect("remove test directory");
    }

    #[test]
    fn corrupt_store_is_backed_up_and_recovered_on_disk() {
        let app_data = temp_directory("corrupt-recovery");
        fs::create_dir_all(&app_data).expect("app data");
        let corrupt = include_bytes!("../tests/fixtures/preferences/corrupt.json");
        fs::write(app_data.join(STORE_FILE), corrupt).expect("corrupt fixture");

        let result = preferences_get(&app_data, &PreferencesState::default());
        assert!(result.data.is_some());
        assert_eq!(result.diagnostics[0].code, "PREFERENCES_RECOVERED");
        let recovered = fs::read(app_data.join(STORE_FILE)).expect("recovered store");
        assert!(decode_store_bytes(&recovered, &default_preferences(&app_data)).is_ok());
        assert!(
            fs::read_dir(&app_data)
                .expect("app data entries")
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("preferences.corrupt."))
        );
        assert_eq!(
            fs::read(app_data.join(PRE_ELECTRON_BACKUP_FILE)).expect("pre-Electron corrupt backup"),
            corrupt
        );

        fs::remove_dir_all(app_data).expect("remove test directory");
    }
}
