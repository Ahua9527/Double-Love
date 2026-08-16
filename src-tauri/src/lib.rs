use std::path::PathBuf;

use double_love_engine::{OperationResult, ProjectSummary, create_project, open_project};

#[tauri::command]
fn project_create(path: String) -> OperationResult<ProjectSummary> {
    create_project(&PathBuf::from(path))
        .map(OperationResult::success)
        .unwrap_or_else(|error| OperationResult::failed("PROJECT_CREATE_FAILED", error.to_string()))
}

#[tauri::command]
fn project_open(path: String) -> OperationResult<ProjectSummary> {
    open_project(&PathBuf::from(path))
        .map(OperationResult::success)
        .unwrap_or_else(|error| OperationResult::failed("PROJECT_OPEN_FAILED", error.to_string()))
}

#[tauri::command]
fn task_cancel(task: String) -> OperationResult<String> {
    OperationResult::failed(
        "TASK_NOT_RUNNING",
        format!("任务 {task} 当前没有可取消的运行实例。"),
    )
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
        .invoke_handler(tauri::generate_handler![
            project_create,
            project_open,
            import_silverstack_preview,
            operation_apply,
            export_premiere_preview,
            export_premiere_apply,
            task_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running Double Love Studio");
}
