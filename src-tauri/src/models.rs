//! Application-wide model catalogue, download queue, verification and diagnostics commands.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use double_love_engine::{
    DoctorReport, ModelDescriptor, ModelDescriptorWithInstallation, ModelError, ModelInstallState,
    ModelInstallation, ModelManager, OperationResult, ffmpeg_supports_ass_filter,
};
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_RANGE, RANGE};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct ModelRuntimeState {
    manager: Mutex<Option<ModelManager>>,
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
    download_lock: Mutex<()>,
}

impl Default for ModelRuntimeState {
    fn default() -> Self {
        Self {
            manager: Mutex::new(None),
            active: Mutex::new(HashMap::new()),
            download_lock: Mutex::new(()),
        }
    }
}

fn failed<T>(code: &str, error: impl ToString) -> OperationResult<T> {
    OperationResult::failed(code, error.to_string())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        return Err("模型清单包含不安全路径。".to_string());
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("模型清单包含不安全路径。".to_string());
        }
    }
    Ok(path.to_path_buf())
}

impl ModelRuntimeState {
    fn with_manager<T>(
        &self,
        root: &Path,
        operation: impl FnOnce(&mut ModelManager) -> Result<T, ModelError>,
    ) -> Result<T, ModelError> {
        let mut slot = self.manager.lock().expect("model manager lock");
        if slot.as_ref().is_none_or(|manager| manager.root() != root) {
            *slot = Some(ModelManager::with_builtin_catalog(root)?);
        }
        let manager = slot.as_mut().expect("model manager initialized");
        let bundled = manager
            .catalog()
            .iter()
            .filter(|descriptor| descriptor.bundled)
            .map(|descriptor| descriptor.id.clone())
            .collect::<Vec<_>>();
        for model_id in bundled {
            if manager.installation(&model_id)?.state != ModelInstallState::Installed {
                manager.transition(&model_id, ModelInstallState::Installed)?;
            }
        }
        operation(manager)
    }

    pub fn installed_dir(&self, root: &Path, model_id: &str) -> Result<PathBuf, String> {
        self.with_manager(root, |manager| {
            let installation = manager.installation(model_id)?;
            if installation.state != ModelInstallState::Installed {
                return Err(ModelError::InvalidState(format!(
                    "{model_id} 尚未安装或未通过校验"
                )));
            }
            manager.installation_dir(model_id)
        })
        .map_err(|error| error.to_string())
    }

    pub fn migrate_root(&self, old_root: &Path, new_root: &Path) -> Result<(), String> {
        if old_root == new_root {
            return Ok(());
        }
        if !new_root.is_absolute() {
            return Err("新的模型目录必须是绝对路径。".to_string());
        }
        let mut old_slot = self.manager.lock().expect("model manager lock");
        if old_slot
            .as_ref()
            .is_none_or(|manager| manager.root() != old_root)
        {
            *old_slot = Some(
                ModelManager::with_builtin_catalog(old_root).map_err(|error| error.to_string())?,
            );
        }
        let old = old_slot.as_mut().expect("old model manager");
        let mut next =
            ModelManager::with_builtin_catalog(new_root).map_err(|error| error.to_string())?;
        for snapshot in old.snapshot() {
            if snapshot.installation.state != ModelInstallState::Installed
                || snapshot.descriptor.bundled
            {
                continue;
            }
            let source = old
                .installation_dir(&snapshot.descriptor.id)
                .map_err(|error| error.to_string())?;
            let destination = next
                .installation_dir(&snapshot.descriptor.id)
                .map_err(|error| error.to_string())?;
            copy_directory(&source, &destination)?;
            next.verify_directory(&snapshot.descriptor.id, &destination)
                .map_err(|error| error.to_string())?;
            next.transition(&snapshot.descriptor.id, ModelInstallState::Installed)
                .map_err(|error| error.to_string())?;
        }
        *old_slot = Some(next);
        Ok(())
    }
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err("旧模型目录不可用。".to_string());
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err("模型目录不能包含符号链接。".to_string());
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn preferences(
    app: &AppHandle,
    state: &crate::AppState,
) -> Result<crate::preferences::AppPreferencesV1, String> {
    crate::preferences::current_preferences(app, &state.preferences)
        .map_err(|error| error.to_string())
}

fn model_url(descriptor: &ModelDescriptor, file: &str, endpoint: &str) -> Result<String, String> {
    let url = descriptor
        .source_url(file)
        .map_err(|error| error.to_string())?;
    if let Some(suffix) = url.strip_prefix("https://huggingface.co") {
        Ok(format!("{}{suffix}", endpoint.trim_end_matches('/')))
    } else {
        Ok(url)
    }
}

fn staging_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("install-{}-{millis}", std::process::id())
}

fn emit_state(app: &AppHandle, installation: &ModelInstallation) {
    let _ = app.emit("dl://model-state", installation);
}

fn download_file_with_resume(
    client: &Client,
    url: &str,
    part_path: &Path,
    expected_size: u64,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64) -> Result<(), String>,
) -> Result<Option<u64>, String> {
    if part_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() > expected_size)
    {
        fs::remove_file(part_path).map_err(|error| error.to_string())?;
    }
    let offset = part_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .map_err(|error| format!("模型下载失败：{error}"))?;
    let partial = response.status().as_u16() == 206;
    if !partial && !response.status().is_success() {
        return Err(format!("模型服务器返回 HTTP {}", response.status()));
    }
    if partial {
        let expected_prefix = format!("bytes {offset}-");
        let expected_suffix = format!("/{expected_size}");
        let valid_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.starts_with(&expected_prefix) && value.ends_with(&expected_suffix)
            });
        if !valid_range {
            return Err("模型服务器返回了不匹配的 Range。".to_string());
        }
    }
    let (mut written, append) = if partial { (offset, true) } else { (0, false) };
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part_path)
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            output.sync_all().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let count = response
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        written = written.saturating_add(count as u64);
        if written > expected_size {
            return Err("下载文件超过清单大小。".to_string());
        }
        let elapsed = started.elapsed().as_secs().max(1);
        on_progress(written, written.saturating_sub(offset) / elapsed)?;
    }
    output.sync_all().map_err(|error| error.to_string())?;
    if written != expected_size {
        return Err("下载文件大小与清单不一致。".to_string());
    }
    Ok(Some(written))
}

fn run_install_batch(app: AppHandle, requested: String, endpoint: String, root: PathBuf) {
    let state = app.state::<crate::AppState>();
    let _queue_guard = state
        .models
        .download_lock
        .lock()
        .expect("download queue lock");
    let order = match state
        .models
        .with_manager(&root, |manager| manager.dependency_order(&requested))
    {
        Ok(order) => order,
        Err(error) => {
            let _ = state.models.with_manager(&root, |manager| {
                manager.mark_error(&requested, "MODEL_DEPENDENCY_FAILED", error.to_string())
            });
            return;
        }
    };

    let flag = state
        .models
        .active
        .lock()
        .expect("active downloads")
        .get(&requested)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    for model_id in &order {
        if flag.load(Ordering::SeqCst) {
            break;
        }
        if let Err(error) = download_model(&app, &state.models, &root, &endpoint, model_id, &flag) {
            if !flag.load(Ordering::SeqCst) {
                if let Ok(installation) = state.models.with_manager(&root, |manager| {
                    manager.mark_error(model_id, "MODEL_INSTALL_FAILED", error)
                }) {
                    emit_state(&app, &installation);
                }
            }
            break;
        }
    }
    if flag.load(Ordering::SeqCst) {
        for model_id in &order {
            if let Ok(installation) = state.models.with_manager(&root, |manager| {
                let current = manager.installation(model_id)?;
                if matches!(
                    current.state,
                    ModelInstallState::Queued | ModelInstallState::Downloading
                ) {
                    manager.transition(model_id, ModelInstallState::Paused)
                } else {
                    Ok(current)
                }
            }) {
                emit_state(&app, &installation);
            }
        }
    }
    let mut active = state.models.active.lock().expect("active downloads");
    for model_id in order {
        active.remove(&model_id);
    }
    active.remove(&requested);
}

fn download_model(
    app: &AppHandle,
    state: &ModelRuntimeState,
    root: &Path,
    endpoint: &str,
    model_id: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let (descriptor, current) = state
        .with_manager(root, |manager| {
            Ok((
                manager.descriptor(model_id)?.clone(),
                manager.installation(model_id)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    if current.state == ModelInstallState::Installed {
        return Ok(());
    }
    if descriptor.bundled {
        let installation = state
            .with_manager(root, |manager| {
                manager.transition(model_id, ModelInstallState::Installed)
            })
            .map_err(|error| error.to_string())?;
        emit_state(app, &installation);
        return Ok(());
    }
    if descriptor.files.is_empty() {
        return Err("模型清单尚未包含可校验的权重文件。".to_string());
    }

    let stage = current.staging_id.unwrap_or_else(staging_id);
    let installation = state
        .with_manager(root, |manager| {
            manager.set_staging_id(model_id, Some(stage.clone()))?;
            manager.transition(model_id, ModelInstallState::Downloading)
        })
        .map_err(|error| error.to_string())?;
    emit_state(app, &installation);
    let client = Client::builder()
        .build()
        .map_err(|error| format!("无法建立下载连接：{error}"))?;
    let staging = state
        .with_manager(root, |manager| manager.staging_root(&stage))
        .map_err(|error| error.to_string())?
        .join(&descriptor.id)
        .join(&descriptor.revision);
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let mut completed_before = 0_u64;

    for file in &descriptor.files {
        if !file.allowed {
            continue;
        }
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let relative = safe_relative_path(&file.path)?;
        let final_path = staging.join(&relative);
        if final_path
            .metadata()
            .is_ok_and(|metadata| metadata.len() == file.size_bytes)
        {
            completed_before = completed_before.saturating_add(file.size_bytes);
            continue;
        }
        let part_path = PathBuf::from(format!("{}.part", final_path.to_string_lossy()));
        if let Some(parent) = part_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let url = model_url(&descriptor, &file.path, endpoint)?;
        let outcome = download_file_with_resume(
            &client,
            &url,
            &part_path,
            file.size_bytes,
            cancel,
            |written, bytes_per_second| {
                let progress = state
                    .with_manager(root, |manager| {
                        manager.update_progress(
                            model_id,
                            Some(file.path.clone()),
                            completed_before.saturating_add(written),
                            written,
                            file.size_bytes,
                            Some(bytes_per_second),
                        )
                    })
                    .map_err(|error| error.to_string())?;
                let _ = app.emit("dl://model-progress", progress);
                Ok(())
            },
        )?;
        if outcome.is_none() {
            return Ok(());
        }
        fs::rename(&part_path, &final_path).map_err(|error| error.to_string())?;
        completed_before = completed_before.saturating_add(file.size_bytes);
    }

    let verifying = state
        .with_manager(root, |manager| {
            manager.transition(model_id, ModelInstallState::Verifying)
        })
        .map_err(|error| error.to_string())?;
    emit_state(app, &verifying);
    let installed = state
        .with_manager(root, |manager| manager.atomically_install(model_id, &stage))
        .map_err(|error| error.to_string())?;
    emit_state(app, &installed);
    Ok(())
}

fn begin_install(
    app: &AppHandle,
    state: &crate::AppState,
    model_id: &str,
) -> Result<ModelInstallation, String> {
    let prefs = preferences(app, state)?;
    let root = PathBuf::from(&prefs.model_root);
    let order = state
        .models
        .with_manager(&root, |manager| {
            manager.queue_install(model_id)?;
            manager.dependency_order(model_id)
        })
        .map_err(|error| error.to_string())?;
    let flag = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.models.active.lock().expect("active downloads");
        if order.iter().any(|id| active.contains_key(id)) {
            return state
                .models
                .with_manager(&root, |manager| manager.installation(model_id))
                .map_err(|error| error.to_string());
        }
        for id in &order {
            active.insert(id.clone(), Arc::clone(&flag));
        }
        active.insert(model_id.to_string(), Arc::clone(&flag));
    }
    let requested = state
        .models
        .with_manager(&root, |manager| manager.installation(model_id))
        .map_err(|error| error.to_string())?;
    let app_handle = app.clone();
    let requested_id = model_id.to_string();
    std::thread::spawn(move || {
        run_install_batch(app_handle, requested_id, prefs.model_endpoint, root)
    });
    Ok(requested)
}

#[tauri::command]
pub fn model_catalog(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<Vec<ModelDescriptorWithInstallation>> {
    let prefs = match preferences(&app, &state) {
        Ok(value) => value,
        Err(error) => return failed("PREFERENCES_READ_FAILED", error),
    };
    match state
        .models
        .with_manager(Path::new(&prefs.model_root), |manager| {
            Ok(manager.snapshot())
        }) {
        Ok(snapshot) => OperationResult::success(snapshot),
        Err(error) => failed("MODEL_CATALOG_FAILED", error),
    }
}

#[tauri::command]
pub fn model_install(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    match begin_install(&app, &state, &model_id) {
        Ok(installation) => OperationResult::success(installation),
        Err(error) => failed("MODEL_INSTALL_FAILED", error),
    }
}

fn set_cancel(
    app: &AppHandle,
    state: &crate::AppState,
    model_id: &str,
    cancel: bool,
) -> Result<ModelInstallation, String> {
    let prefs = preferences(app, state)?;
    if let Some(flag) = state
        .models
        .active
        .lock()
        .expect("active downloads")
        .get(model_id)
        .cloned()
    {
        flag.store(true, Ordering::SeqCst);
    }
    state
        .models
        .with_manager(Path::new(&prefs.model_root), |manager| {
            let current = manager.installation(model_id)?;
            let next = if cancel && current.bytes_downloaded == 0 {
                ModelInstallState::NotInstalled
            } else {
                ModelInstallState::Paused
            };
            manager.transition(model_id, next)
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn model_pause(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    match set_cancel(&app, &state, &model_id, false) {
        Ok(installation) => OperationResult::success(installation),
        Err(error) => failed("MODEL_PAUSE_FAILED", error),
    }
}

#[tauri::command]
pub fn model_cancel(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    match set_cancel(&app, &state, &model_id, true) {
        Ok(installation) => OperationResult::success(installation),
        Err(error) => failed("MODEL_CANCEL_FAILED", error),
    }
}

#[tauri::command]
pub fn model_resume(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    model_install(app, state, model_id)
}

#[tauri::command]
pub fn model_verify(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    let prefs = match preferences(&app, &state) {
        Ok(value) => value,
        Err(error) => return failed("PREFERENCES_READ_FAILED", error),
    };
    match state
        .models
        .with_manager(Path::new(&prefs.model_root), |manager| {
            manager.verify_installed(&model_id)
        }) {
        Ok(installation) => OperationResult::success(installation),
        Err(error) => failed("MODEL_VERIFY_FAILED", error),
    }
}

#[tauri::command]
pub fn model_remove(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: String,
) -> OperationResult<ModelInstallation> {
    let prefs = match preferences(&app, &state) {
        Ok(value) => value,
        Err(error) => return failed("PREFERENCES_READ_FAILED", error),
    };
    match state
        .models
        .with_manager(Path::new(&prefs.model_root), |manager| {
            manager.remove(&model_id)
        }) {
        Ok(installation) => {
            emit_state(&app, &installation);
            OperationResult::success(installation)
        }
        Err(error) => failed("MODEL_REMOVE_FAILED", error),
    }
}

#[tauri::command]
pub fn model_reveal(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    model_id: Option<String>,
) -> OperationResult<()> {
    let prefs = match preferences(&app, &state) {
        Ok(value) => value,
        Err(error) => return failed("PREFERENCES_READ_FAILED", error),
    };
    let path = if let Some(model_id) = model_id {
        match state
            .models
            .with_manager(Path::new(&prefs.model_root), |manager| {
                manager.installation_dir(&model_id)
            }) {
            Ok(path) => path,
            Err(error) => return failed("MODEL_REVEAL_FAILED", error),
        }
    } else {
        PathBuf::from(&prefs.model_root)
    };
    let _ = fs::create_dir_all(&path);
    match std::process::Command::new("open").arg(path).spawn() {
        Ok(_) => OperationResult::success(()),
        Err(error) => failed("MODEL_REVEAL_FAILED", error),
    }
}

#[tauri::command]
pub fn doctor_run(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> OperationResult<DoctorReport> {
    let prefs = match preferences(&app, &state) {
        Ok(value) => value,
        Err(error) => return failed("PREFERENCES_READ_FAILED", error),
    };
    let profile = match crate::preferences::system_profile_for(&app, &state.preferences) {
        Ok(value) => value,
        Err(error) => return failed("SYSTEM_PROFILE_FAILED", error),
    };
    let tools = super::resolve_media_tools(&app).ok();
    let ffmpeg_available = tools.is_some();
    let libass_available = tools.as_ref().is_some_and(ffmpeg_supports_ass_filter);
    let asr_runtime_ready = super::resolve_asr_sidecar_dir(&app)
        .join(".venv/bin/python")
        .is_file();
    let speaker_runtime_ready = super::resolve_speaker_sidecar_dir(&app)
        .join(".venv/bin/python")
        .is_file();
    match state
        .models
        .with_manager(Path::new(&prefs.model_root), |manager| {
            Ok(manager.doctor_report(
                profile.architecture,
                profile.os_version,
                profile.memory_bytes,
                profile.free_model_bytes,
                ffmpeg_available,
                libass_available,
                asr_runtime_ready,
                speaker_runtime_ready,
            ))
        }) {
        Ok(report) => {
            let _ = app.emit("dl://doctor-result", &report);
            OperationResult::success(report)
        }
        Err(error) => failed("DOCTOR_FAILED", error),
    }
}

#[tauri::command]
pub fn diagnostics_reveal_logs(app: AppHandle) -> OperationResult<()> {
    let path = match app.path().app_log_dir() {
        Ok(path) => path,
        Err(error) => return failed("LOG_PATH_FAILED", error),
    };
    let _ = fs::create_dir_all(&path);
    match std::process::Command::new("open").arg(path).spawn() {
        Ok(_) => OperationResult::success(()),
        Err(error) => failed("LOG_REVEAL_FAILED", error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use double_love_engine::ModelCatalog;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn mirror_endpoint_keeps_the_pinned_model_path() {
        let catalog = ModelCatalog::builtin().expect("catalog");
        let descriptor = catalog.get("qwen3-asr-0.6b").expect("model");
        let url = model_url(descriptor, "config.json", "https://models.example.test").expect("url");
        assert!(url.starts_with("https://models.example.test/Qwen/Qwen3-ASR-0.6B/resolve/"));
        assert!(url.contains(&descriptor.revision));
    }

    #[test]
    fn download_paths_reject_parent_and_absolute_components() {
        assert!(safe_relative_path("config.json").is_ok());
        assert!(safe_relative_path("weights/model.safetensors").is_ok());
        assert!(safe_relative_path("../model.bin").is_err());
        assert!(safe_relative_path("/tmp/model.bin").is_err());
    }

    #[test]
    fn local_http_fixture_resumes_a_partial_download_with_range() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("fixture request");
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).expect("read fixture request");
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.to_ascii_lowercase().contains("range: bytes=2-"));
            stream
                .write_all(
                    b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\nConnection: close\r\n\r\nllo",
                )
                .expect("write fixture response");
        });

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("double-love-http-fixture-{stamp}"));
        fs::create_dir_all(&directory).expect("fixture directory");
        let part = directory.join("model.bin.part");
        fs::write(&part, b"he").expect("partial file");
        let client = Client::builder().build().expect("client");
        let cancel = AtomicBool::new(false);
        let mut updates = Vec::new();
        let outcome = download_file_with_resume(
            &client,
            &format!("http://{address}/model.bin"),
            &part,
            5,
            &cancel,
            |written, _| {
                updates.push(written);
                Ok(())
            },
        )
        .expect("resumed download");

        assert_eq!(outcome, Some(5));
        assert_eq!(fs::read(&part).expect("downloaded file"), b"hello");
        assert_eq!(updates.last(), Some(&5));
        server.join().expect("fixture server");
        fs::remove_dir_all(directory).expect("remove fixture directory");
    }
}
