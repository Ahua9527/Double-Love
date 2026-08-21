//! Application-wide preferences and recent-project persistence.

use std::{
    collections::HashMap,
    ffi::CString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use double_love_engine::{
    Diagnostic, DiagnosticLevel, OperationResult, ProjectSummary, SubtitleStyle,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::{Store as TauriStore, StoreExt};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

const STORE_FILE: &str = "preferences.json";
const STORE_KEY: &str = "app_preferences";
pub const CURRENT_PREFERENCES_SCHEMA: u32 = 1;
pub const CURRENT_ONBOARDING_VERSION: u32 = 1;
const MAX_RECENT_PROJECTS: usize = 20;
const LOW_MEMORY_MODEL: &str = "qwen3-asr-0.6b";
const HIGH_MEMORY_MODEL: &str = "qwen3-asr-1.7b";
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProjectRecord {
    pub project_id: Option<String>,
    pub root: String,
    pub display_name: String,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProject {
    pub project_id: Option<String>,
    pub root: String,
    pub display_name: String,
    pub last_opened_at: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppPreferencesV1 {
    pub schema_version: u32,
    pub theme: ThemeMode,
    pub restore_last_project: bool,
    pub timecode_precision: TimecodePrecision,
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
    pub timecode_precision: Option<TimecodePrecision>,
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

pub struct PreferencesState {
    lock: Mutex<()>,
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self::new()
    }
}

impl PreferencesState {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct PreferencesChanged<'a> {
    changed_keys: &'a [String],
}

fn default_model_root(app: &AppHandle) -> Result<PathBuf, PreferencesError> {
    app.path()
        .app_data_dir()
        .map(|root| root.join("models"))
        .map_err(|error| PreferencesError::Path(error.to_string()))
}

pub fn default_preferences_for_root(model_root: impl Into<String>) -> AppPreferencesV1 {
    AppPreferencesV1 {
        schema_version: CURRENT_PREFERENCES_SCHEMA,
        theme: ThemeMode::Light,
        restore_last_project: true,
        timecode_precision: TimecodePrecision::Frame,
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

pub fn current_preferences(
    app: &AppHandle,
    state: &PreferencesState,
) -> Result<AppPreferencesV1, PreferencesError> {
    with_store(app, state, |store, preferences| {
        if store.get(STORE_KEY).is_none() {
            save_preferences(store, preferences)?;
        }
        Ok(preferences.clone())
    })
    .map(|(preferences, _)| preferences)
}

fn default_preferences(app: &AppHandle) -> Result<AppPreferencesV1, PreferencesError> {
    let mut preferences =
        default_preferences_for_root(default_model_root(app)?.to_string_lossy().into_owned());
    preferences.default_asr_model = if read_memory_bytes() < 16 * 1024 * 1024 * 1024 {
        LOW_MEMORY_MODEL.to_string()
    } else {
        HIGH_MEMORY_MODEL.to_string()
    };
    Ok(preferences)
}

fn store_path(app: &AppHandle) -> Result<PathBuf, PreferencesError> {
    app.path()
        .app_data_dir()
        .map(|root| root.join(STORE_FILE))
        .map_err(|error| PreferencesError::Path(error.to_string()))
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
            "默认 ASR 模型必须是 qwen3-asr-0.6b 或 qwen3-asr-1.7b。".to_string(),
        ));
    }
    if value.recent_projects.len() > MAX_RECENT_PROJECTS {
        return Err(PreferencesError::Invalid(
            "最近项目数量超过 20 条。".to_string(),
        ));
    }
    Ok(())
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
    value.recent_projects.truncate(MAX_RECENT_PROJECTS);
    validate_preferences(&value)?;
    Ok(value)
}

fn inspect_store_file(
    app: &AppHandle,
    defaults: &AppPreferencesV1,
) -> Result<bool, PreferencesError> {
    let path = store_path(app)?;
    if !path.exists() {
        return Ok(false);
    }
    let bytes = fs::read(&path).map_err(|error| PreferencesError::Io(error.to_string()))?;
    let values: HashMap<String, Value> = match serde_json::from_slice(&bytes) {
        Ok(values) => values,
        Err(_) => {
            move_corrupt_store(&path)?;
            return Ok(true);
        }
    };
    if let Some(raw) = values.get(STORE_KEY)
        && decode_preferences(raw, defaults).is_err()
    {
        move_corrupt_store(&path)?;
        return Ok(true);
    }
    Ok(false)
}

fn with_store<T>(
    app: &AppHandle,
    state: &PreferencesState,
    operation: impl FnOnce(&TauriStore<tauri::Wry>, &AppPreferencesV1) -> Result<T, PreferencesError>,
) -> Result<(T, bool), PreferencesError> {
    let _guard = state.lock.lock().expect("preferences lock");
    let defaults = default_preferences(app)?;
    let recovered = inspect_store_file(app, &defaults)?;
    let store = app
        .store(STORE_FILE)
        .map_err(|error| PreferencesError::Store(error.to_string()))?;
    let preferences = if recovered {
        defaults.clone()
    } else if let Some(raw) = store.get(STORE_KEY) {
        decode_preferences(&raw, &defaults)?
    } else {
        defaults.clone()
    };
    if recovered {
        save_preferences(&store, &preferences)?;
    }
    let result = operation(&store, &preferences)?;
    Ok((result, recovered))
}

fn save_preferences(
    store: &TauriStore<tauri::Wry>,
    value: &AppPreferencesV1,
) -> Result<(), PreferencesError> {
    let raw =
        serde_json::to_value(value).map_err(|error| PreferencesError::Decode(error.to_string()))?;
    store.set(STORE_KEY, raw);
    store
        .save()
        .map_err(|error| PreferencesError::Store(error.to_string()))
}

fn changed_keys_from_patch(patch: &PreferencesPatch) -> Vec<String> {
    let mut changed = Vec::new();
    if patch.theme.is_some() {
        changed.push("theme".to_string());
    }
    if patch.restore_last_project.is_some() {
        changed.push("restore_last_project".to_string());
    }
    if patch.timecode_precision.is_some() {
        changed.push("timecode_precision".to_string());
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
    if let Some(precision) = patch.timecode_precision {
        value.timecode_precision = precision;
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
        // `preferences_update` 在进入这里前已经完成复制和完整性校验。
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

fn command_error<T>(error: PreferencesError, read: bool) -> OperationResult<T> {
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

fn emit_preferences_changed(app: &AppHandle, changed_keys: &[String]) {
    let _ = app.emit(
        "dl://preferences-changed",
        PreferencesChanged { changed_keys },
    );
}

#[tauri::command]
pub fn preferences_get(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<AppPreferencesV1> {
    match with_store(&app, &state.preferences, |store, preferences| {
        if store.get(STORE_KEY).is_none() {
            save_preferences(store, preferences)?;
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

#[tauri::command]
pub fn preferences_update(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    patch: PreferencesPatch,
) -> OperationResult<AppPreferencesV1> {
    if let Some(model_root) = patch.model_root.as_deref() {
        let current = match current_preferences(&app, &state.preferences) {
            Ok(current) => current,
            Err(error) => return command_error(error, true),
        };
        if let Err(error) = state
            .models
            .migrate_root(Path::new(&current.model_root), Path::new(model_root))
        {
            return OperationResult::failed("MODEL_ROOT_MIGRATION_FAILED", error);
        }
    }
    let changed_keys = changed_keys_from_patch(&patch);
    match with_store(&app, &state.preferences, |store, preferences| {
        let next = apply_patch(preferences.clone(), &patch)?;
        save_preferences(store, &next)?;
        Ok(next)
    }) {
        Ok((preferences, recovered)) => {
            emit_preferences_changed(&app, &changed_keys);
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
        Ok(absolute.to_string_lossy().into_owned())
    }
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
    preferences.recent_projects.retain(|item| item.root != root);
    preferences.recent_projects.insert(0, record);
    preferences.recent_projects.truncate(MAX_RECENT_PROJECTS);
    Ok(preferences)
}

pub fn record_recent_project(
    app: &AppHandle,
    state: &PreferencesState,
    summary: &ProjectSummary,
) -> Result<(), PreferencesError> {
    let result = with_store(app, state, |store, preferences| {
        let next = upsert_recent(preferences.clone(), summary)?;
        save_preferences(store, &next)
    });
    if result.is_ok() {
        emit_preferences_changed(app, &["recent_projects".to_string()]);
    }
    result.map(|_| ())
}

fn recent_projects(preferences: &AppPreferencesV1) -> Vec<RecentProject> {
    let mut records = preferences.recent_projects.clone();
    records.sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
    records
        .into_iter()
        .take(MAX_RECENT_PROJECTS)
        .map(|record| RecentProject {
            exists: Path::new(&record.root).exists(),
            project_id: record.project_id,
            root: record.root,
            display_name: record.display_name,
            last_opened_at: record.last_opened_at,
        })
        .collect()
}

#[tauri::command]
pub fn recent_projects_list(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<Vec<RecentProject>> {
    match with_store(&app, &state.preferences, |store, preferences| {
        if store.get(STORE_KEY).is_none() {
            save_preferences(store, preferences)?;
        }
        Ok(recent_projects(preferences))
    }) {
        Ok((projects, _)) => OperationResult::success(projects),
        Err(error) => command_error(error, true),
    }
}

#[tauri::command]
pub fn recent_project_forget(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    root: String,
) -> OperationResult<()> {
    let canonical = match canonical_project_root(&root) {
        Ok(root) => root,
        Err(error) => return command_error(error, false),
    };
    match with_store(&app, &state.preferences, |store, preferences| {
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
        save_preferences(store, &next)
    }) {
        Ok(((), _)) => {
            emit_preferences_changed(&app, &["recent_projects".to_string()]);
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
        if let Ok(contents) = fs::read_to_string("/proc/meminfo") {
            if let Some(line) = contents.lines().find(|line| line.starts_with("MemTotal:")) {
                if let Some(kib) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    return kib.saturating_mul(1024);
                }
            }
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
    app: &AppHandle,
    state: &PreferencesState,
) -> Result<SystemProfile, PreferencesError> {
    let preferences = current_preferences(app, state)?;
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

#[tauri::command]
pub fn system_profile(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<SystemProfile> {
    match system_profile_for(&app, &state.preferences) {
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

#[tauri::command]
pub fn onboarding_get(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<OnboardingState> {
    match with_store(&app, &state.preferences, |store, preferences| {
        if store.get(STORE_KEY).is_none() {
            save_preferences(store, preferences)?;
        }
        Ok(onboarding_from_preferences(preferences))
    }) {
        Ok((onboarding, _)) => OperationResult::success(onboarding),
        Err(error) => command_error(error, true),
    }
}

#[tauri::command]
pub fn onboarding_complete(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    default_asr_model: Option<String>,
    step: Option<u8>,
) -> OperationResult<OnboardingState> {
    if let Some(step) = step
        && !(1..=3).contains(&step)
    {
        return OperationResult::failed("ONBOARDING_STEP_INVALID", "引导步骤必须是 1、2 或 3。");
    }
    match with_store(&app, &state.preferences, |store, preferences| {
        let mut next = preferences.clone();
        if let Some(model) = default_asr_model.as_deref() {
            if !matches!(model, LOW_MEMORY_MODEL | HIGH_MEMORY_MODEL) {
                return Err(PreferencesError::Invalid(
                    "默认 ASR 模型必须是 qwen3-asr-0.6b 或 qwen3-asr-1.7b。".to_string(),
                ));
            }
            next.default_asr_model = model.to_string();
        }
        next.onboarding_version = CURRENT_ONBOARDING_VERSION;
        next.onboarding_completed = true;
        save_preferences(store, &next)?;
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
            emit_preferences_changed(&app, &changed);
            OperationResult::success(onboarding)
        }
        Err(error) => command_error(error, false),
    }
}

#[tauri::command]
pub fn onboarding_reset(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<OnboardingState> {
    match with_store(&app, &state.preferences, |store, preferences| {
        let mut next = preferences.clone();
        next.onboarding_version = CURRENT_ONBOARDING_VERSION;
        next.onboarding_completed = false;
        save_preferences(store, &next)?;
        Ok(onboarding_from_preferences(&next))
    }) {
        Ok((onboarding, _)) => {
            emit_preferences_changed(&app, &["onboarding_completed".to_string()]);
            OperationResult::success(onboarding)
        }
        Err(error) => command_error(error, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(preferences.recent_projects.is_empty());
    }

    #[test]
    fn patch_updates_only_declared_fields_and_validates_endpoint() {
        let defaults = default_preferences_for_root("/tmp/double-love/models");
        let patch = PreferencesPatch {
            theme: Some(ThemeMode::Dark),
            model_endpoint: Some("https://mirror.example.test/hub".to_string()),
            ..PreferencesPatch::default()
        };
        let updated = apply_patch(defaults.clone(), &patch).expect("valid patch");
        assert_eq!(updated.theme, ThemeMode::Dark);
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
    }

    #[test]
    fn recent_projects_are_deduplicated_and_capped_at_twenty() {
        let mut preferences = default_preferences_for_root("/tmp/double-love/models");
        for index in 0..25 {
            let root = format!("/tmp/double-love/project-{index}");
            preferences = upsert_recent(preferences, &summary(&root, &index.to_string())).unwrap();
        }
        assert_eq!(preferences.recent_projects.len(), MAX_RECENT_PROJECTS);
        assert_eq!(
            preferences.recent_projects[0].root,
            "/tmp/double-love/project-24"
        );

        preferences = upsert_recent(
            preferences,
            &summary("/tmp/double-love/project-10", "replacement"),
        )
        .unwrap();
        assert_eq!(preferences.recent_projects.len(), MAX_RECENT_PROJECTS);
        assert_eq!(
            preferences.recent_projects[0].root,
            "/tmp/double-love/project-10"
        );
        assert_eq!(
            preferences.recent_projects[0].project_id.as_deref(),
            Some("replacement")
        );
    }

    #[test]
    fn migration_fills_missing_v1_fields() {
        let defaults = default_preferences_for_root("/tmp/double-love/models");
        let raw = serde_json::json!({ "theme": "dark", "schema_version": 0 });
        let migrated = decode_preferences(&raw, &defaults).expect("migration succeeds");
        assert_eq!(migrated.schema_version, CURRENT_PREFERENCES_SCHEMA);
        assert_eq!(migrated.theme, ThemeMode::Dark);
        assert_eq!(migrated.model_endpoint, MODEL_ENDPOINT);
    }
}
