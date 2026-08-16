//! Tauri 命令面：把 engine 能力暴露给 studio 前端。
//! 业务写入全部走 engine；这一层只做参数装配、项目状态持有与进度事件转发。

mod media_protocol;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use double_love_engine::{
    DEFAULT_HANDLES_MS, EditOperation, ExportOutcome, FfmpegTools, MediaAssetSummary,
    OperationResult, ProgressEvent, ProgressSink, ProjectStore, ProjectSummary, SharedSink,
    TaskRegistry, TaskState, TranscribeConfig, TranscriptViewData, create_project,
    export_rough_cut, export_rough_cut_to, import_media as engine_import_media, list_media_assets,
    omit_words, open_project, restore_words, start_transcription, transcript_view,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// 当前打开的项目：summary + 共享库连接（转录 worker 持有同一份 Arc）。
struct OpenProject {
    summary: ProjectSummary,
    store: Arc<Mutex<ProjectStore>>,
}

struct AppState {
    project: Mutex<Option<OpenProject>>,
    registry: TaskRegistry,
}

impl AppState {
    fn new() -> Self {
        Self {
            project: Mutex::new(None),
            registry: TaskRegistry::new(),
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
    *state.project.lock().expect("project lock") = Some(OpenProject {
        summary: summary.clone(),
        store: Arc::new(Mutex::new(store)),
    });
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

/// 定位 sidecars/asr：env 覆盖 → 开发期常见相对位置（tauri dev 的 cwd 是 src-tauri）。
fn resolve_sidecar_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DOUBLELOVE_ASR_DIR") {
        return PathBuf::from(dir);
    }
    for candidate in ["sidecars/asr", "../sidecars/asr"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            return path;
        }
    }
    PathBuf::from("sidecars/asr") // 不存在时让启动失败并给出清晰错误
}

#[tauri::command]
fn project_create(path: String, state: State<AppState>) -> OperationResult<ProjectSummary> {
    match create_project(&PathBuf::from(path)) {
        Ok(summary) => match install_project(&state, &summary) {
            Ok(()) => OperationResult::success(summary),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error),
        },
        Err(error) => OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string()),
    }
}

#[tauri::command]
fn project_open(path: String, state: State<AppState>) -> OperationResult<ProjectSummary> {
    match open_project(&PathBuf::from(path)) {
        Ok(summary) => match install_project(&state, &summary) {
            Ok(()) => OperationResult::success(summary),
            Err(error) => OperationResult::failed("STORAGE_ERROR", error),
        },
        Err(error) => OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string()),
    }
}

/// 导入本地媒体：ffprobe 探测 + 校验 + 抽取 16kHz 准备音频。
#[tauri::command]
fn import_media(state: State<AppState>, path: String) -> OperationResult<MediaAssetSummary> {
    with_store(&state, |store, summary| {
        let tools = match FfmpegTools::discover() {
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
    let guard = state.project.lock().expect("project lock");
    let Some(open) = guard.as_ref() else {
        return OperationResult::failed("PROJECT_NOT_OPEN", "请先打开或创建一个项目。");
    };
    let config = TranscribeConfig {
        asset_id,
        model,
        language,
        mock: false,
        python: None,
        package_dir: resolve_sidecar_dir(),
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
        .manage(AppState::new())
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
            project_create,
            project_open,
            import_media,
            assets_list,
            transcribe_start,
            task_cancel,
            transcript_get,
            edit_omit,
            edit_restore,
            roughcut_preview,
            export_roughcut_apply,
            import_silverstack_preview,
            operation_apply,
            export_premiere_preview,
            export_premiere_apply
        ])
        .run(tauri::generate_context!())
        .expect("error while running Double Love Studio");
}
