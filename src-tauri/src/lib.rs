//! Tauri 命令面：把 engine 能力暴露给 studio 前端。
//! 业务写入全部走 engine；这一层只做参数装配、项目状态持有与进度事件转发。

mod media_protocol;
mod models;
mod preferences;
mod settings_window;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use double_love_engine::{
    CanvasSpec, DEFAULT_HANDLES_MS, Diagnostic, DiagnosticLevel, DiarizeConfig, EditOperation,
    ExportOutcome, FfmpegTools, FrameRate, MainTrackClip, MediaAssetSummary, OperationResult,
    ProgressEvent, ProgressSink, ProjectExportPreview, ProjectStore, ProjectSummary,
    RevisionHistoryEntry, SharedSink, SpeakerDiarizationResult, SpeakerIdentity,
    SpeakerNameAgentPayload, SpeakerNameProposal, SubtitleStyle, TaskRegistry, TaskState,
    TimelineIRv2, TranscribeConfig, TranscriptViewData, agent_name_payload_preview,
    append_full_main_track_asset, append_main_track_clip, compile_project_timeline, create_project,
    export_project_ass_to, export_project_xmeml_to, export_rough_cut, export_rough_cut_to,
    import_media as engine_import_media, list_media_assets, local_name_proposals,
    move_main_track_clip, omit_words, open_project, preview_project_export, remove_main_track_clip,
    render_project_mp4_to, restore_words, speaker_diarization_result, split_main_track_clip,
    start_speaker_diarization, start_transcription, transcript_view, trim_main_track_clip,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// 当前打开的项目：summary + 共享库连接（转录 worker 持有同一份 Arc）。
struct OpenProject {
    summary: ProjectSummary,
    store: Arc<Mutex<ProjectStore>>,
}

pub(crate) struct AppState {
    project: Mutex<Option<OpenProject>>,
    registry: TaskRegistry,
    pub(crate) preferences: preferences::PreferencesState,
    pub(crate) models: models::ModelRuntimeState,
    history_navigation: Mutex<HistoryNavigation>,
}

#[derive(Default)]
struct HistoryNavigation {
    project_id: Option<String>,
    last_actual_revision: u64,
    current_snapshot_revision: u64,
    undo: Vec<u64>,
    redo: Vec<u64>,
}

impl AppState {
    fn new() -> Self {
        Self {
            project: Mutex::new(None),
            registry: TaskRegistry::new(),
            preferences: preferences::PreferencesState::new(),
            models: models::ModelRuntimeState::default(),
            history_navigation: Mutex::new(HistoryNavigation::default()),
        }
    }
}

/// 进度汇：engine 进度/终态 → 前端事件（dl://progress、dl://task-state）。
struct TauriSink {
    app: AppHandle,
}

#[derive(Clone, Serialize)]
struct TaskStateEvent {
    task_id: String,
    state: TaskState,
}

impl ProgressSink for TauriSink {
    fn progress(&self, event: ProgressEvent) {
        let _ = self.app.emit("dl://progress", event);
    }

    fn task_state(&self, task_id: &str, state: TaskState) {
        let _ = self.app.emit(
            "dl://task-state",
            TaskStateEvent {
                task_id: task_id.to_string(),
                state,
            },
        );
    }
}

/// 打开/创建成功后把项目装进 AppState（后续命令不再传项目路径）。
fn install_project(state: &AppState, summary: &ProjectSummary) -> Result<(), String> {
    let store =
        ProjectStore::open(Path::new(&summary.database)).map_err(|error| error.to_string())?;
    let revision = store.revision().map_err(|error| error.to_string())?;
    let undo = store
        .revision_history(10_000)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| entry.restorable && entry.revision < revision)
        .map(|entry| entry.revision)
        .rev()
        .collect();
    *state.project.lock().expect("project lock") = Some(OpenProject {
        summary: summary.clone(),
        store: Arc::new(Mutex::new(store)),
    });
    *state.history_navigation.lock().expect("history navigation") = HistoryNavigation {
        project_id: Some(summary.project_id.clone()),
        last_actual_revision: revision,
        current_snapshot_revision: revision,
        undo,
        redo: Vec::new(),
    };
    Ok(())
}

/// 取当前项目的库连接；未打开项目 → 统一 PROJECT_NOT_OPEN。
fn with_store<T>(
    state: &AppState,
    operation: impl FnOnce(&ProjectStore, &ProjectSummary) -> OperationResult<T>,
) -> OperationResult<T> {
    let guard = state.project.lock().expect("project lock");
    let Some(open) = guard.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let store = open.store.lock().expect("store lock");
    operation(&store, &open.summary)
}

/// 定位打包或开发期 ASR sidecar：环境覆盖 → App 资源 → 开发期相对目录。
fn resolve_asr_sidecar_dir(app: &AppHandle) -> PathBuf {
    if let Some(dir) = std::env::var_os("DOUBLELOVE_ASR_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in [
            resource_dir.join("model-runtime/asr"),
            resource_dir.join("resources/model-runtime/asr"),
        ] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    for candidate in ["sidecars/asr", "../sidecars/asr"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            return path;
        }
    }
    PathBuf::from("sidecars/asr") // 不存在时让启动失败并给出清晰错误
}

fn resolve_speaker_sidecar_dir(app: &AppHandle) -> PathBuf {
    if let Some(dir) = std::env::var_os("DOUBLELOVE_SPEAKER_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in [
            resource_dir.join("model-runtime/speaker"),
            resource_dir.join("resources/model-runtime/speaker"),
        ] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    for candidate in ["sidecars/speaker", "../sidecars/speaker"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            return path;
        }
    }
    PathBuf::from("sidecars/speaker")
}

/// 发布包优先使用随 App 分发的媒体运行时；开发环境可继续使用 PATH/Homebrew。
fn resolve_media_tools(
    app: &AppHandle,
) -> Result<FfmpegTools, Box<double_love_engine::Diagnostic>> {
    if let Ok(resource_dir) = app.path().resource_dir() {
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
    }
    FfmpegTools::discover()
}

#[tauri::command]
fn project_create(
    app: AppHandle,
    path: String,
    state: State<AppState>,
) -> OperationResult<ProjectSummary> {
    match create_project(&PathBuf::from(path)) {
        Ok(mut summary) => {
            let mut style_warning = None;
            match preferences::current_preferences(&app, &state.preferences) {
                Ok(preferences) => match ProjectStore::open(Path::new(&summary.database))
                    .and_then(|store| store.set_subtitle_style(&preferences.default_subtitle_style))
                {
                    Ok(revision) => summary.revision = revision,
                    Err(error) => style_warning = Some(error.to_string()),
                },
                Err(error) => style_warning = Some(error.to_string()),
            }
            match install_project(&state, &summary) {
                Ok(()) => {
                    let mut result = OperationResult::success(summary.clone());
                    if let Some(error) = style_warning {
                        result.diagnostics.push(Diagnostic {
                            level: DiagnosticLevel::Warning,
                            code: "DEFAULT_SUBTITLE_STYLE_FAILED".to_string(),
                            cause: error,
                            object_id: None,
                            impact: "项目已创建，但使用了内置字幕默认值。".to_string(),
                            blocks_export: false,
                            suggested_action: Some("可在编辑器中重新设置字幕样式。".to_string()),
                        });
                    }
                    if let Err(error) =
                        preferences::record_recent_project(&app, &state.preferences, &summary)
                    {
                        result.diagnostics.push(Diagnostic {
                            level: DiagnosticLevel::Warning,
                            code: "RECENT_PROJECT_WRITE_FAILED".to_string(),
                            cause: error.to_string(),
                            object_id: None,
                            impact: "项目已创建，但最近项目列表未更新。".to_string(),
                            blocks_export: false,
                            suggested_action: Some("稍后在设置中重新打开项目列表。".to_string()),
                        });
                    }
                    result
                }
                Err(error) => OperationResult::failed("STORAGE_ERROR", error),
            }
        }
        Err(error) => OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string()),
    }
}

#[tauri::command]
fn project_open(
    app: AppHandle,
    path: String,
    state: State<AppState>,
) -> OperationResult<ProjectSummary> {
    match open_project(&PathBuf::from(path)) {
        Ok(summary) => match install_project(&state, &summary) {
            Ok(()) => {
                let mut result = OperationResult::success(summary.clone());
                if let Err(error) =
                    preferences::record_recent_project(&app, &state.preferences, &summary)
                {
                    result.diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        code: "RECENT_PROJECT_WRITE_FAILED".to_string(),
                        cause: error.to_string(),
                        object_id: None,
                        impact: "项目已打开，但最近项目列表未更新。".to_string(),
                        blocks_export: false,
                        suggested_action: Some("稍后在设置中重新打开项目列表。".to_string()),
                    });
                }
                result
            }
            Err(error) => OperationResult::failed("STORAGE_ERROR", error),
        },
        Err(error) => OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string()),
    }
}

/// 导入本地媒体：ffprobe 探测 + 校验 + 抽取 16kHz 准备音频。
#[tauri::command]
fn import_media(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> OperationResult<MediaAssetSummary> {
    with_store(&state, |store, summary| {
        let tools = match resolve_media_tools(&app) {
            Ok(tools) => tools,
            Err(diagnostic) => {
                let mut result: OperationResult<MediaAssetSummary> =
                    OperationResult::failed(&diagnostic.code, &diagnostic.cause);
                result.diagnostics[0].suggested_action = diagnostic.suggested_action.clone();
                return result;
            }
        };
        let prepared_dir = Path::new(&summary.root).join(".doublelove/prepared");
        engine_import_media(store, &prepared_dir, &tools, Path::new(&path))
    })
}

/// 列出全部已导入资产（资产列表首屏与刷新）。
#[tauri::command]
fn assets_list(state: State<AppState>) -> OperationResult<Vec<MediaAssetSummary>> {
    with_store(&state, |store, _| list_media_assets(store))
}

/// 启动转录（异步）：立即返回 task_id，进度走 dl://progress，终态走 dl://task-state。
#[tauri::command]
fn transcribe_start(
    app: AppHandle,
    state: State<AppState>,
    asset_id: String,
    model: String,
    language: String,
) -> OperationResult<serde_json::Value> {
    let preferences = match preferences::current_preferences(&app, &state.preferences) {
        Ok(preferences) => preferences,
        Err(error) => return OperationResult::failed("PREFERENCES_READ_FAILED", error.to_string()),
    };
    let model_root = Path::new(&preferences.model_root);
    let model_dir = match state.models.installed_dir(model_root, &model) {
        Ok(path) => path,
        Err(error) => return OperationResult::failed("MODEL_NOT_READY", error),
    };
    let aligner_dir = match state
        .models
        .installed_dir(model_root, "qwen3-forced-aligner-0.6b")
    {
        Ok(path) => path,
        Err(error) => return OperationResult::failed("MODEL_NOT_READY", error),
    };
    let guard = state.project.lock().expect("project lock");
    let Some(open) = guard.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let config = TranscribeConfig {
        asset_id,
        model,
        model_dir,
        aligner_dir,
        language,
        mock: false,
        python: None,
        package_dir: resolve_asr_sidecar_dir(&app),
        log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
        chunk_seconds: 30,
    };
    let sink: SharedSink = Arc::new(TauriSink { app });
    match start_transcription(Arc::clone(&open.store), &state.registry, sink, config) {
        Ok(task_id) => OperationResult::success(serde_json::json!({ "task_id": task_id })),
        Err(error) => OperationResult::failed("TRANSCRIBE_START_FAILED", error),
    }
}

#[tauri::command]
fn task_cancel(state: State<AppState>, task_id: String) -> OperationResult<serde_json::Value> {
    if state.registry.cancel(&task_id) {
        OperationResult::success(serde_json::json!({ "task_id": task_id }))
    } else {
        OperationResult::failed(
            "TASK_NOT_RUNNING",
            format!("任务 {task_id} 当前没有可取消的运行实例。"),
        )
    }
}

/// 当前项目的最新 revision。UI 在写入前可用它发现外部 CLI 的更新并刷新，而不是覆盖新状态。
#[tauri::command]
fn project_revision(state: State<AppState>) -> OperationResult<u64> {
    with_store(&state, |store, _| match store.revision() {
        Ok(revision) => OperationResult::success(revision),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn project_history(
    state: State<AppState>,
    limit: Option<usize>,
) -> OperationResult<Vec<RevisionHistoryEntry>> {
    with_store(&state, |store, _| {
        match store.revision_history(limit.unwrap_or(80)) {
            Ok(history) => OperationResult::success(history),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }
    })
}

#[tauri::command]
fn project_restore_revision(
    state: State<AppState>,
    revision: u64,
) -> OperationResult<serde_json::Value> {
    with_store(&state, |store, _| match store.restore_revision(revision) {
        Ok(new_revision) => {
            let mut result = OperationResult::success(serde_json::json!({
                "restored_revision": revision,
                "revision": new_revision,
            }));
            result.revision = Some(new_revision);
            result
        }
        Err(error) => OperationResult::failed("HISTORY_RESTORE_FAILED", error.to_string()),
    })
}

fn reset_history_navigation(
    navigation: &mut HistoryNavigation,
    store: &ProjectStore,
    project_id: &str,
) -> Result<(), String> {
    let revision = store.revision().map_err(|error| error.to_string())?;
    navigation.project_id = Some(project_id.to_string());
    navigation.last_actual_revision = revision;
    navigation.current_snapshot_revision = revision;
    navigation.undo = store
        .revision_history(10_000)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| entry.restorable && entry.revision < revision)
        .map(|entry| entry.revision)
        .rev()
        .collect();
    navigation.redo.clear();
    Ok(())
}

#[tauri::command]
fn edit_undo(state: State<AppState>) -> OperationResult<()> {
    let project = state.project.lock().expect("project lock");
    let Some(open) = project.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let store = open.store.lock().expect("store lock");
    let actual_revision = match store.revision() {
        Ok(revision) => revision,
        Err(error) => return OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let mut navigation = state.history_navigation.lock().expect("history navigation");
    let navigation_is_stale = navigation.project_id.as_deref()
        != Some(open.summary.project_id.as_str())
        || navigation.last_actual_revision != actual_revision;
    if navigation_is_stale
        && let Err(error) =
            reset_history_navigation(&mut navigation, &store, &open.summary.project_id)
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
}

#[tauri::command]
fn edit_redo(state: State<AppState>) -> OperationResult<()> {
    let project = state.project.lock().expect("project lock");
    let Some(open) = project.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let store = open.store.lock().expect("store lock");
    let actual_revision = match store.revision() {
        Ok(revision) => revision,
        Err(error) => return OperationResult::failed("STORAGE_ERROR", error.to_string()),
    };
    let mut navigation = state.history_navigation.lock().expect("history navigation");
    if navigation.project_id.as_deref() != Some(open.summary.project_id.as_str())
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
}

/// TranscriptView 首屏数据：词 + 分段 + 活跃 omit。
#[tauri::command]
fn transcript_get(state: State<AppState>, asset_id: String) -> OperationResult<TranscriptViewData> {
    with_store(&state, |store, _| transcript_view(store, &asset_id))
}

/// 删除词区间（不传 handles 用引擎默认 120ms）。
#[tauri::command]
fn edit_omit(
    state: State<AppState>,
    asset_id: String,
    start_ordinal: i64,
    end_ordinal: i64,
    handles_before_ms: Option<i64>,
    handles_after_ms: Option<i64>,
) -> OperationResult<EditOperation> {
    with_store(&state, |store, _| {
        omit_words(
            store,
            &asset_id,
            start_ordinal,
            end_ordinal,
            handles_before_ms.unwrap_or(DEFAULT_HANDLES_MS),
            handles_after_ms.unwrap_or(DEFAULT_HANDLES_MS),
        )
    })
}

/// 恢复某个 omit 的词区间（完全或部分覆盖，部分覆盖自动拆段）。
#[tauri::command]
fn edit_restore(
    state: State<AppState>,
    operation_id: String,
    start_ordinal: i64,
    end_ordinal: i64,
) -> OperationResult<EditOperation> {
    with_store(&state, |store, _| {
        restore_words(store, &operation_id, start_ordinal, end_ordinal)
    })
}

/// 粗剪预览：编译 IR + 诊断，不落盘、不写库（Preview 先于 Apply）。
#[tauri::command]
fn roughcut_preview(state: State<AppState>, asset_id: String) -> OperationResult<ExportOutcome> {
    with_store(&state, |store, summary| {
        let exports_dir = Path::new(&summary.root).join(".doublelove/exports");
        export_rough_cut(store, &asset_id, &exports_dir, false)
    })
}

/// 粗剪导出：写到保存对话框选中的精确路径 + sha256 + export_artifact 落账。
#[tauri::command]
fn export_roughcut_apply(
    state: State<AppState>,
    asset_id: String,
    target_path: String,
) -> OperationResult<ExportOutcome> {
    with_store(&state, |store, _| {
        export_rough_cut_to(store, &asset_id, Path::new(&target_path), true)
    })
}

fn project_timeline_name(summary: &ProjectSummary) -> String {
    Path::new(&summary.root)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name} Rough Cut"))
        .unwrap_or_else(|| "Double Love Rough Cut".to_string())
}

/// 多素材项目的统一导出预览：TimelineIR v2、字幕 Cue 与双端兼容性报告。
#[tauri::command]
fn project_export_preview(state: State<AppState>) -> OperationResult<ProjectExportPreview> {
    with_store(&state, |store, summary| {
        preview_project_export(store, &project_timeline_name(summary))
    })
}

#[tauri::command]
fn project_export_xmeml_apply(
    state: State<AppState>,
    target_path: String,
) -> OperationResult<ProjectExportPreview> {
    with_store(&state, |store, summary| {
        export_project_xmeml_to(
            store,
            &project_timeline_name(summary),
            Path::new(&target_path),
        )
    })
}

#[tauri::command]
fn project_export_ass_apply(
    state: State<AppState>,
    target_path: String,
) -> OperationResult<ProjectExportPreview> {
    with_store(&state, |store, summary| {
        export_project_ass_to(
            store,
            &project_timeline_name(summary),
            Path::new(&target_path),
        )
    })
}

#[tauri::command]
fn project_render_mp4_apply(
    app: AppHandle,
    state: State<AppState>,
    target_path: String,
) -> OperationResult<ProjectExportPreview> {
    with_store(&state, |store, summary| {
        let tools = match resolve_media_tools(&app) {
            Ok(tools) => tools,
            Err(diagnostic) => {
                let mut result = OperationResult::failed(&diagnostic.code, &diagnostic.cause);
                result.diagnostics[0].suggested_action = diagnostic.suggested_action.clone();
                return result;
            }
        };
        render_project_mp4_to(
            store,
            &project_timeline_name(summary),
            &tools,
            &Path::new(&summary.root).join(".doublelove/cache"),
            Path::new(&target_path),
        )
    })
}

/// 当前项目的多素材主轨预览。所有播放、字幕和 NLE 输出都应从同一 TimelineIR v2 读取。
#[tauri::command]
fn timeline_get(state: State<AppState>) -> OperationResult<TimelineIRv2> {
    with_store(&state, |store, summary| {
        compile_project_timeline(store, &project_timeline_name(summary))
    })
}

#[tauri::command]
fn main_track_append(
    state: State<AppState>,
    asset_id: String,
    source_in_frame: i64,
    source_out_frame: i64,
) -> OperationResult<MainTrackClip> {
    with_store(&state, |store, _| {
        append_main_track_clip(store, &asset_id, source_in_frame, source_out_frame)
    })
}

#[tauri::command]
fn main_track_append_full(
    state: State<AppState>,
    asset_id: String,
) -> OperationResult<MainTrackClip> {
    with_store(&state, |store, _| {
        append_full_main_track_asset(store, &asset_id)
    })
}

#[tauri::command]
fn main_track_list(state: State<AppState>) -> OperationResult<Vec<MainTrackClip>> {
    with_store(&state, |store, _| match store.main_track_clips() {
        Ok(clips) => OperationResult::success(clips),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn main_track_move(
    state: State<AppState>,
    clip_id: String,
    before_clip_id: Option<String>,
) -> OperationResult<()> {
    with_store(&state, |store, _| {
        move_main_track_clip(store, &clip_id, before_clip_id.as_deref())
    })
}

#[tauri::command]
fn main_track_trim(
    state: State<AppState>,
    clip_id: String,
    source_in_frame: i64,
    source_out_frame: i64,
) -> OperationResult<MainTrackClip> {
    with_store(&state, |store, _| {
        trim_main_track_clip(store, &clip_id, source_in_frame, source_out_frame)
    })
}

#[tauri::command]
fn main_track_split(
    state: State<AppState>,
    clip_id: String,
    source_at_frame: i64,
) -> OperationResult<Vec<MainTrackClip>> {
    with_store(&state, |store, _| {
        split_main_track_clip(store, &clip_id, source_at_frame)
    })
}

#[tauri::command]
fn main_track_remove(state: State<AppState>, clip_id: String) -> OperationResult<()> {
    with_store(&state, |store, _| remove_main_track_clip(store, &clip_id))
}

#[tauri::command]
fn canvas_get(state: State<AppState>) -> OperationResult<CanvasSpec> {
    with_store(&state, |store, _| match store.canvas_spec() {
        Ok(canvas) => OperationResult::success(canvas),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn canvas_set(state: State<AppState>, canvas: CanvasSpec) -> OperationResult<CanvasSpec> {
    with_store(&state, |store, _| match store.set_canvas_spec(&canvas) {
        Ok(revision) => {
            let mut result = OperationResult::success(canvas);
            result.revision = Some(revision);
            result
        }
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn output_rate_get(state: State<AppState>) -> OperationResult<Option<FrameRate>> {
    with_store(&state, |store, _| match store.output_rate() {
        Ok(rate) => OperationResult::success(rate),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

/// `None` 清除显式选择，输出帧率重新跟随主轨第一段素材。
#[tauri::command]
fn output_rate_set(
    state: State<AppState>,
    rate: Option<FrameRate>,
) -> OperationResult<Option<FrameRate>> {
    with_store(&state, |store, _| {
        let revision = match rate {
            Some(rate) => store.set_output_rate(rate),
            None => store.clear_output_rate(),
        };
        match revision {
            Ok(revision) => {
                let mut result = OperationResult::success(rate);
                result.revision = Some(revision);
                result
            }
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }
    })
}

#[tauri::command]
fn subtitle_style_get(state: State<AppState>) -> OperationResult<SubtitleStyle> {
    with_store(&state, |store, _| match store.subtitle_style() {
        Ok(style) => OperationResult::success(style),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn subtitle_style_set(
    state: State<AppState>,
    style: SubtitleStyle,
) -> OperationResult<SubtitleStyle> {
    with_store(&state, |store, _| match store.set_subtitle_style(&style) {
        Ok(revision) => {
            let mut result = OperationResult::success(style);
            result.revision = Some(revision);
            result
        }
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn apply_default_subtitle_style(
    app: AppHandle,
    state: State<AppState>,
) -> OperationResult<SubtitleStyle> {
    let preferences = match preferences::current_preferences(&app, &state.preferences) {
        Ok(preferences) => preferences,
        Err(error) => return OperationResult::failed("PREFERENCES_READ_FAILED", error.to_string()),
    };
    subtitle_style_set(state, preferences.default_subtitle_style)
}

#[tauri::command]
fn speaker_list(state: State<AppState>) -> OperationResult<Vec<SpeakerIdentity>> {
    with_store(&state, |store, _| match store.speaker_identities() {
        Ok(speakers) => OperationResult::success(speakers),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

#[tauri::command]
fn speaker_save(
    state: State<AppState>,
    speaker: SpeakerIdentity,
) -> OperationResult<SpeakerIdentity> {
    with_store(&state, |store, _| {
        match store.upsert_speaker_identity(&speaker) {
            Ok(()) => OperationResult::success(speaker),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
        }
    })
}

#[tauri::command]
fn speaker_name_proposals(
    state: State<AppState>,
    asset_id: String,
) -> OperationResult<Vec<SpeakerNameProposal>> {
    with_store(&state, |store, _| match store.transcript_words(&asset_id) {
        Ok(words) => OperationResult::success(local_name_proposals(&words)),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

/// 给用户展示可发送给 Agent 的最小文本包。应用不在这里调用任何外部网络服务。
#[tauri::command]
fn speaker_agent_payload_preview(
    state: State<AppState>,
    asset_id: String,
    speaker_id: String,
) -> OperationResult<SpeakerNameAgentPayload> {
    with_store(&state, |store, _| match store.transcript_words(&asset_id) {
        Ok(words) => OperationResult::success(agent_name_payload_preview(&words, &speaker_id)),
        Err(error) => OperationResult::failed("STORAGE_ERROR", error.to_string()),
    })
}

/// 本地/Agent 姓名候选都需要用户明确确认后才会写入项目身份。
#[tauri::command]
fn speaker_name_confirm(
    state: State<AppState>,
    speaker_id: String,
    display_name: String,
    confirmed: bool,
) -> OperationResult<SpeakerIdentity> {
    if !confirmed {
        return OperationResult::failed("SPEAKER_CONFIRM_REQUIRED", "请确认后再应用说话人姓名。");
    }
    with_store(&state, |store, _| {
        match store.confirm_speaker_name(&speaker_id, &display_name) {
            Ok(identity) => {
                let mut result = OperationResult::success(identity);
                result.revision = store.revision().ok();
                result
            }
            Err(error) => OperationResult::failed("SPEAKER_NAME_CONFIRM_FAILED", error.to_string()),
        }
    })
}

/// 合并候选永远需要明确确认；未确认时不改变任何 cluster 或逐词归属。
#[tauri::command]
fn speaker_merge_confirm(
    state: State<AppState>,
    keep_speaker_id: String,
    merge_speaker_id: String,
    confirmed: bool,
) -> OperationResult<SpeakerIdentity> {
    if !confirmed {
        return OperationResult::failed("SPEAKER_CONFIRM_REQUIRED", "请确认后再合并说话人。");
    }
    with_store(&state, |store, _| {
        match store.merge_speaker_identities(&keep_speaker_id, &merge_speaker_id) {
            Ok(identity) => {
                let mut result = OperationResult::success(identity);
                result.revision = store.revision().ok();
                result
            }
            Err(error) => OperationResult::failed("SPEAKER_MERGE_FAILED", error.to_string()),
        }
    })
}

/// 启动本地说话人分离。声纹向量不走 Tauri 返回值，只保留在本地项目库。
#[tauri::command]
fn speaker_diarize_start(
    app: AppHandle,
    state: State<AppState>,
    asset_id: String,
) -> OperationResult<serde_json::Value> {
    let preferences = match preferences::current_preferences(&app, &state.preferences) {
        Ok(preferences) => preferences,
        Err(error) => return OperationResult::failed("PREFERENCES_READ_FAILED", error.to_string()),
    };
    let speaker_model_dir = match state
        .models
        .installed_dir(Path::new(&preferences.model_root), "wespeaker-zh")
    {
        Ok(path) => path,
        Err(error) => return OperationResult::failed("MODEL_NOT_READY", error),
    };
    let guard = state.project.lock().expect("project lock");
    let Some(open) = guard.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let config = DiarizeConfig {
        asset_id,
        mock: false,
        python: None,
        package_dir: resolve_speaker_sidecar_dir(&app),
        log_dir: Path::new(&open.summary.root).join(".doublelove/logs"),
        vad_model_dir: PathBuf::from("bundled"),
        speaker_model_dir,
    };
    let sink: SharedSink = Arc::new(TauriSink { app });
    match start_speaker_diarization(Arc::clone(&open.store), &state.registry, sink, config) {
        Ok(task_id) => OperationResult::success(serde_json::json!({ "task_id": task_id })),
        Err(error) => OperationResult::failed("SPEAKER_START_FAILED", error),
    }
}

#[tauri::command]
fn speaker_diarization_get(
    state: State<AppState>,
    asset_id: String,
) -> OperationResult<SpeakerDiarizationResult> {
    with_store(&state, |store, _| {
        speaker_diarization_result(store, &asset_id)
    })
}

fn metadata_mvp_pending(operation: &str) -> OperationResult<String> {
    OperationResult::failed(
        "METADATA_MVP_PENDING",
        format!("{operation} 已登记为稳定命令契约，等待 Metadata MVP 阶段接入。"),
    )
}

#[tauri::command]
fn import_silverstack_preview() -> OperationResult<String> {
    metadata_mvp_pending("import_silverstack_preview")
}

#[tauri::command]
fn operation_apply() -> OperationResult<String> {
    metadata_mvp_pending("operation_apply")
}

#[tauri::command]
fn export_premiere_preview() -> OperationResult<String> {
    metadata_mvp_pending("export_premiere_preview")
}

#[tauri::command]
fn export_premiere_apply() -> OperationResult<String> {
    metadata_mvp_pending("export_premiere_apply")
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::new())
        .setup(|app| {
            settings_window::install_native_menu(app.handle())?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if settings_window::is_settings_menu_item(event.id().as_ref()) {
                let _ = settings_window::open_settings_window(app);
            }
        })
        .on_window_event(|window, event| {
            if window.label() == settings_window::SETTINGS_WINDOW_LABEL
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        // media://localhost/<asset_id>：只服务当前项目内已导入资产的原始媒体（只读引用）。
        .register_uri_scheme_protocol("media", |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            let asset_id = request.uri().path().trim_start_matches('/');
            let method = request.method().as_str().to_string();
            let range = request
                .headers()
                .get(tauri::http::header::RANGE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let path = {
                let guard = state.project.lock().expect("project lock");
                guard.as_ref().and_then(|open| {
                    open.store
                        .lock()
                        .expect("store lock")
                        .media_asset(asset_id)
                        .ok()
                        .flatten()
                        .map(|asset| asset.original_path)
                })
            };
            match path {
                Some(path) => {
                    media_protocol::media_response(&method, range.as_deref(), Path::new(&path))
                }
                None => media_protocol::not_found(),
            }
        })
        .invoke_handler(tauri::generate_handler![
            settings_window::settings_open,
            preferences::preferences_get,
            preferences::preferences_update,
            preferences::recent_projects_list,
            preferences::recent_project_forget,
            preferences::system_profile,
            preferences::onboarding_get,
            preferences::onboarding_complete,
            preferences::onboarding_reset,
            models::model_catalog,
            models::model_install,
            models::model_pause,
            models::model_resume,
            models::model_cancel,
            models::model_verify,
            models::model_remove,
            models::model_reveal,
            models::doctor_run,
            models::diagnostics_reveal_logs,
            project_create,
            project_open,
            import_media,
            assets_list,
            transcribe_start,
            task_cancel,
            project_revision,
            project_history,
            project_restore_revision,
            transcript_get,
            edit_omit,
            edit_restore,
            edit_undo,
            edit_redo,
            roughcut_preview,
            export_roughcut_apply,
            project_export_preview,
            project_export_xmeml_apply,
            project_export_ass_apply,
            project_render_mp4_apply,
            timeline_get,
            main_track_append,
            main_track_append_full,
            main_track_list,
            main_track_move,
            main_track_trim,
            main_track_split,
            main_track_remove,
            canvas_get,
            canvas_set,
            output_rate_get,
            output_rate_set,
            subtitle_style_get,
            subtitle_style_set,
            apply_default_subtitle_style,
            speaker_list,
            speaker_save,
            speaker_name_proposals,
            speaker_agent_payload_preview,
            speaker_name_confirm,
            speaker_merge_confirm,
            speaker_diarize_start,
            speaker_diarization_get,
            import_silverstack_preview,
            operation_apply,
            export_premiere_preview,
            export_premiere_apply
        ])
        .run(tauri::generate_context!())
        .expect("error while running Double Love Studio");
}
