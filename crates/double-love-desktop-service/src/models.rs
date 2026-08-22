//! Model catalogue, download queue, verification, reveal paths, and diagnostics support.

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
    DoctorEnvironment, DoctorReport, ModelCatalog, ModelDescriptor,
    ModelDescriptorWithInstallation, ModelError, ModelInstallState, ModelInstallation,
    ModelManager,
};
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_RANGE, RANGE};

use crate::{DesktopEventSink, DesktopServiceError};

#[derive(Clone)]
pub struct ModelState {
    manager: Arc<Mutex<Option<ModelManager>>>,
    active: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    download_lock: Arc<Mutex<()>>,
    catalog: Option<ModelCatalog>,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            manager: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(HashMap::new())),
            download_lock: Arc::new(Mutex::new(())),
            catalog: None,
        }
    }
}

impl std::fmt::Debug for ModelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelState").finish_non_exhaustive()
    }
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

impl ModelState {
    #[cfg(test)]
    fn with_catalog(catalog: ModelCatalog) -> Self {
        Self {
            catalog: Some(catalog),
            ..Self::default()
        }
    }

    fn with_manager<T>(
        &self,
        root: &Path,
        operation: impl FnOnce(&mut ModelManager) -> Result<T, ModelError>,
    ) -> Result<T, ModelError> {
        let mut slot = self.manager.lock().map_err(|_| {
            ModelError::InvalidState("model manager lock is unavailable".to_string())
        })?;
        if slot.as_ref().is_none_or(|manager| manager.root() != root) {
            *slot = Some(match &self.catalog {
                Some(catalog) => ModelManager::new(root, catalog.clone())?,
                None => ModelManager::with_builtin_catalog(root)?,
            });
        }
        let manager = slot.as_mut().expect("model manager initialized");
        mark_bundled_installed(manager)?;
        operation(manager)
    }

    pub fn snapshot(&self, root: &Path) -> Result<Vec<ModelDescriptorWithInstallation>, String> {
        self.with_manager(root, |manager| Ok(manager.snapshot()))
            .map_err(|error| error.to_string())
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

    pub fn installation_dir(&self, root: &Path, model_id: &str) -> Result<PathBuf, String> {
        self.with_manager(root, |manager| manager.installation_dir(model_id))
            .map_err(|error| error.to_string())
    }

    pub fn verify(&self, root: &Path, model_id: &str) -> Result<ModelInstallation, String> {
        self.with_manager(root, |manager| manager.verify_installed(model_id))
            .map_err(|error| error.to_string())
    }

    pub fn remove(&self, root: &Path, model_id: &str) -> Result<ModelInstallation, ModelError> {
        self.with_manager(root, |manager| manager.remove(model_id))
    }

    pub fn doctor_report(
        &self,
        root: &Path,
        environment: DoctorEnvironment,
    ) -> Result<DoctorReport, String> {
        self.with_manager(root, |manager| Ok(manager.doctor_report(environment)))
            .map_err(|error| error.to_string())
    }

    pub fn migrate_root(&self, old_root: &Path, new_root: &Path) -> Result<(), String> {
        if old_root == new_root {
            return Ok(());
        }
        if !new_root.is_absolute() {
            return Err("新的模型目录必须是绝对路径。".to_string());
        }
        let old = match &self.catalog {
            Some(catalog) => ModelManager::new(old_root, catalog.clone()),
            None => ModelManager::with_builtin_catalog(old_root),
        }
        .map_err(|error| error.to_string())?;
        let mut next = match &self.catalog {
            Some(catalog) => ModelManager::new(new_root, catalog.clone()),
            None => ModelManager::with_builtin_catalog(new_root),
        }
        .map_err(|error| error.to_string())?;
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
        mark_bundled_installed(&mut next).map_err(|error| error.to_string())?;
        let mut slot = self
            .manager
            .lock()
            .map_err(|_| "model manager lock is unavailable".to_string())?;
        *slot = Some(next);
        Ok(())
    }

    pub fn begin_install(
        &self,
        root: PathBuf,
        endpoint: String,
        model_id: &str,
        events: Arc<dyn DesktopEventSink>,
    ) -> Result<ModelInstallation, String> {
        let order = self
            .with_manager(&root, |manager| {
                manager.queue_install(model_id)?;
                manager.dependency_order(model_id)
            })
            .map_err(|error| error.to_string())?;
        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "active downloads lock is unavailable".to_string())?;
            if order.iter().any(|id| active.contains_key(id)) {
                return self
                    .with_manager(&root, |manager| manager.installation(model_id))
                    .map_err(|error| error.to_string());
            }
            for id in &order {
                active.insert(id.clone(), Arc::clone(&flag));
            }
            active.insert(model_id.to_string(), Arc::clone(&flag));
        }
        let requested = self
            .with_manager(&root, |manager| manager.installation(model_id))
            .map_err(|error| error.to_string())?;
        let state = self.clone();
        let requested_id = model_id.to_string();
        std::thread::spawn(move || {
            state.run_install_batch(events, requested_id, endpoint, root);
        });
        Ok(requested)
    }

    pub fn set_cancel(
        &self,
        root: &Path,
        model_id: &str,
        cancel: bool,
    ) -> Result<ModelInstallation, String> {
        if let Some(flag) = self
            .active
            .lock()
            .map_err(|_| "active downloads lock is unavailable".to_string())?
            .get(model_id)
            .cloned()
        {
            flag.store(true, Ordering::SeqCst);
        }
        self.with_manager(root, |manager| {
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

    fn run_install_batch(
        &self,
        events: Arc<dyn DesktopEventSink>,
        requested: String,
        endpoint: String,
        root: PathBuf,
    ) {
        let _queue_guard = match self.download_lock.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let order = match self.with_manager(&root, |manager| manager.dependency_order(&requested)) {
            Ok(order) => order,
            Err(error) => {
                let _ = self.with_manager(&root, |manager| {
                    manager.mark_error(&requested, "MODEL_DEPENDENCY_FAILED", error.to_string())
                });
                return;
            }
        };
        let flag = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(&requested).cloned())
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        for model_id in &order {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            if let Err(error) =
                self.download_model(events.as_ref(), &root, &endpoint, model_id, &flag)
            {
                if !flag.load(Ordering::SeqCst)
                    && let Ok(installation) = self.with_manager(&root, |manager| {
                        manager.mark_error(model_id, "MODEL_INSTALL_FAILED", error)
                    })
                {
                    emit_state(events.as_ref(), &installation);
                }
                break;
            }
        }
        if flag.load(Ordering::SeqCst) {
            for model_id in &order {
                if let Ok(installation) = self.with_manager(&root, |manager| {
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
                    emit_state(events.as_ref(), &installation);
                }
            }
        }
        if let Ok(mut active) = self.active.lock() {
            for model_id in order {
                active.remove(&model_id);
            }
            active.remove(&requested);
        }
    }

    fn download_model(
        &self,
        events: &dyn DesktopEventSink,
        root: &Path,
        endpoint: &str,
        model_id: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        let (descriptor, current) = self
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
            let installation = self
                .with_manager(root, |manager| {
                    manager.transition(model_id, ModelInstallState::Installed)
                })
                .map_err(|error| error.to_string())?;
            emit_state(events, &installation);
            return Ok(());
        }
        if descriptor.files.is_empty() {
            return Err("模型清单尚未包含可校验的权重文件。".to_string());
        }

        let stage = current.staging_id.unwrap_or_else(staging_id);
        let installation = self
            .with_manager(root, |manager| {
                manager.set_staging_id(model_id, Some(stage.clone()))?;
                manager.transition(model_id, ModelInstallState::Downloading)
            })
            .map_err(|error| error.to_string())?;
        emit_state(events, &installation);
        let client = Client::builder()
            .build()
            .map_err(|error| format!("无法建立下载连接：{error}"))?;
        let staging = self
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
                    let progress = self
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
                    emit(events, "dl://model-progress", &progress);
                    Ok(())
                },
            )?;
            if outcome.is_none() {
                return Ok(());
            }
            fs::rename(&part_path, &final_path).map_err(|error| error.to_string())?;
            completed_before = completed_before.saturating_add(file.size_bytes);
        }

        let verifying = self
            .with_manager(root, |manager| {
                manager.transition(model_id, ModelInstallState::Verifying)
            })
            .map_err(|error| error.to_string())?;
        emit_state(events, &verifying);
        let installed = self
            .with_manager(root, |manager| manager.atomically_install(model_id, &stage))
            .map_err(|error| error.to_string())?;
        emit_state(events, &installed);
        Ok(())
    }
}

fn mark_bundled_installed(manager: &mut ModelManager) -> Result<(), ModelError> {
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
    Ok(())
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

fn emit<T: serde::Serialize>(events: &dyn DesktopEventSink, channel: &str, payload: &T) {
    if let Ok(value) = serde_json::to_value(payload) {
        let _ = events.emit(channel, value);
    }
}

fn emit_state(events: &dyn DesktopEventSink, installation: &ModelInstallation) {
    emit(events, "dl://model-state", installation);
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

pub fn emit_doctor<T: serde::Serialize>(events: &dyn DesktopEventSink, report: &T) {
    emit(events, "dl://doctor-result", report);
}

pub fn service_error(error: impl ToString) -> DesktopServiceError {
    DesktopServiceError::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use sha2::Digest;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    #[derive(Default)]
    struct Events(Mutex<Vec<(String, Value)>>);

    impl DesktopEventSink for Events {
        fn emit(&self, channel: &str, payload: Value) -> Result<(), DesktopServiceError> {
            self.0
                .lock()
                .expect("events")
                .push((channel.to_string(), payload));
            Ok(())
        }
    }

    fn temp(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "double-love-model-service-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ))
    }

    fn catalog(_endpoint: &str, expected: &[u8]) -> ModelCatalog {
        let hash = format!("{:x}", sha2::Sha256::digest(expected));
        ModelCatalog::from_json(
            &json!({
                "schema_version": 1,
                "models": [
                    {
                        "id": "dependency", "display_name": "Dependency", "component": "forced_aligner",
                        "repo_id": "fixture/dependency", "revision": "1111111111111111111111111111111111111111",
                        "files": [{"path": "model.bin", "size_bytes": expected.len(), "sha256": hash, "allowed": true}],
                        "license": "MIT", "license_url": "https://example.test/license", "dependencies": [],
                        "min_memory_bytes": null, "source_url_template": "https://huggingface.co/fixture/dependency/resolve/{revision}/{path}", "bundled": false
                    },
                    {
                        "id": "asr", "display_name": "ASR", "component": "asr",
                        "repo_id": "fixture/asr", "revision": "2222222222222222222222222222222222222222",
                        "files": [{"path": "model.bin", "size_bytes": expected.len(), "sha256": hash, "allowed": true}],
                        "license": "MIT", "license_url": "https://example.test/license",
                        "dependencies": [{"model_id": "dependency", "required": true, "reason": "fixture"}],
                        "min_memory_bytes": null, "source_url_template": "https://huggingface.co/fixture/asr/resolve/{revision}/{path}", "bundled": false
                    }
                ]
            })
            .to_string(),
        )
        .expect("fixture catalog")
    }

    fn server(bytes: Vec<u8>, requests: usize, slow: bool) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().expect("request");
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).expect("read request");
                let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
                let offset = request
                    .lines()
                    .find_map(|line| line.strip_prefix("range: bytes="))
                    .and_then(|range| range.strip_suffix('-'))
                    .and_then(|offset| offset.parse::<usize>().ok())
                    .unwrap_or(0);
                let body = &bytes[offset..];
                if offset > 0 {
                    write!(
                        stream,
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(), offset, bytes.len() - 1, bytes.len()
                    )
                    .expect("headers");
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .expect("headers");
                }
                if slow {
                    for chunk in body.chunks(64) {
                        if stream.write_all(chunk).is_err() {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                } else {
                    let _ = stream.write_all(body);
                }
            }
        });
        (format!("http://{address}"), handle)
    }

    fn wait_state(state: &ModelState, root: &Path, id: &str, expected: ModelInstallState) {
        for _ in 0..400 {
            let snapshot = state.snapshot(root).expect("snapshot");
            let current = snapshot
                .iter()
                .find(|item| item.descriptor.id == id)
                .expect("model")
                .installation
                .state;
            if current == expected {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("model {id} did not reach {expected:?}");
    }

    #[test]
    fn install_pause_resume_verify_remove_and_doctor_use_synthetic_http() {
        let bytes = vec![b'x'; 4096];
        let (endpoint, fixture) = server(bytes.clone(), 3, true);
        let root = temp("lifecycle");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));
        let events = Arc::new(Events::default());

        state
            .begin_install(root.clone(), endpoint.clone(), "asr", events.clone())
            .expect("install queues");
        for _ in 0..200 {
            let snapshot = state.snapshot(&root).expect("snapshot");
            let dependency = &snapshot
                .iter()
                .find(|item| item.descriptor.id == "dependency")
                .expect("dependency")
                .installation;
            if dependency.bytes_downloaded > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let paused = state
            .set_cancel(&root, "asr", false)
            .expect("pause requested model");
        assert_eq!(paused.state, ModelInstallState::Paused);
        wait_state(&state, &root, "dependency", ModelInstallState::Paused);
        let staging_id = state
            .snapshot(&root)
            .expect("snapshot")
            .into_iter()
            .find(|item| item.descriptor.id == "dependency")
            .expect("dependency")
            .installation
            .staging_id
            .expect("staging preserved");
        assert!(root.join(".staging").join(staging_id).exists());

        // The first worker must release the single queue before resume starts a new worker.
        thread::sleep(Duration::from_millis(100));
        state
            .begin_install(root.clone(), endpoint.clone(), "asr", events.clone())
            .expect("resume");
        wait_state(&state, &root, "asr", ModelInstallState::Installed);
        assert_eq!(
            state.verify(&root, "asr").expect("verify").state,
            ModelInstallState::Installed
        );
        assert!(matches!(
            state.remove(&root, "dependency"),
            Err(ModelError::DependencyInUse { .. })
        ));
        assert_eq!(
            state.remove(&root, "asr").expect("remove asr").state,
            ModelInstallState::NotInstalled
        );
        assert_eq!(
            state
                .remove(&root, "dependency")
                .expect("remove dependency")
                .state,
            ModelInstallState::NotInstalled
        );
        let report = state
            .doctor_report(
                &root,
                DoctorEnvironment {
                    architecture: "arm64".to_string(),
                    os_version: "fixture".to_string(),
                    memory_bytes: 16,
                    free_model_bytes: 8,
                    ffmpeg_available: true,
                    libass_available: true,
                    asr_runtime_ready: true,
                    speaker_runtime_ready: false,
                },
            )
            .expect("doctor");
        assert_eq!(report.architecture, "arm64");
        assert_eq!(report.model_checks.len(), 2);
        let recorded = events.0.lock().expect("events");
        assert!(
            recorded
                .iter()
                .any(|(name, _)| name == "dl://model-progress")
        );
        assert!(recorded.iter().any(|(name, payload)| {
            name == "dl://model-state" && payload["state"] == "installed"
        }));
        assert!(
            !serde_json::to_string(&*recorded)
                .expect("events JSON")
                .contains(&root.to_string_lossy().into_owned())
        );
        drop(recorded);
        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn zero_byte_cancel_returns_not_installed_and_hash_failure_becomes_corrupt() {
        let expected = vec![b'a'; 1024];
        let (endpoint, _cancel_fixture) = server(vec![b'b'; 1024], 1, true);
        let root = temp("cancel-corrupt");
        let state = ModelState::with_catalog(catalog(&endpoint, &expected));
        let events = Arc::new(Events::default());
        state
            .begin_install(root.clone(), endpoint, "dependency", events)
            .expect("queue");
        let cancelled = state.set_cancel(&root, "dependency", true).expect("cancel");
        assert_eq!(cancelled.state, ModelInstallState::NotInstalled);
        thread::sleep(Duration::from_millis(100));

        let (endpoint, fixture) = server(vec![b'b'; 1024], 1, false);
        // A new state reads the persisted not-installed snapshot and uses the same fixture catalog.
        let state = ModelState::with_catalog(catalog(&endpoint, &expected));
        state
            .begin_install(
                root.clone(),
                endpoint,
                "dependency",
                Arc::new(Events::default()),
            )
            .expect("install corrupt bytes");
        wait_state(&state, &root, "dependency", ModelInstallState::Corrupt);
        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn download_paths_and_range_resume_are_strict() {
        assert!(safe_relative_path("config.json").is_ok());
        assert!(safe_relative_path("weights/model.safetensors").is_ok());
        assert!(safe_relative_path("../model.bin").is_err());
        assert!(safe_relative_path("/tmp/model.bin").is_err());

        let (endpoint, fixture) = server(b"hello".to_vec(), 1, false);
        let directory = temp("range");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");
        fs::write(&part, b"he").expect("partial");
        let outcome = download_file_with_resume(
            &Client::builder().build().expect("client"),
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .expect("resume");
        assert_eq!(outcome, Some(5));
        assert_eq!(fs::read(&part).expect("file"), b"hello");
        fixture.join().expect("fixture");
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
