//! Model catalogue, download queue, verification, reveal paths, and diagnostics support.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use double_love_engine::{
    DoctorEnvironment, DoctorReport, LegacyModelCleanupPreview, ModelCatalog, ModelDescriptor,
    ModelDescriptorWithInstallation, ModelDownloadSource, ModelError, ModelInstallState,
    ModelInstallation, ModelManager, ModelQueueEntry, ModelQueueSnapshot, ModelQueueState,
    ModelUiRole,
};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_RANGE, RANGE};

use crate::{DesktopEventSink, DesktopServiceError};

#[derive(Clone)]
pub struct ModelState {
    manager: Arc<Mutex<Option<ModelManager>>>,
    active: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    queue: Arc<Mutex<VecDeque<String>>>,
    current_request: Arc<Mutex<Option<String>>>,
    download_lock: Arc<Mutex<()>>,
    catalog: Option<ModelCatalog>,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            manager: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            current_request: Arc::new(Mutex::new(None)),
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

    pub fn queue_snapshot(&self) -> Result<ModelQueueSnapshot, String> {
        let active_model_id = self
            .current_request
            .lock()
            .map_err(|_| "model queue current lock is unavailable".to_string())?
            .clone();
        let queue = self
            .queue
            .lock()
            .map_err(|_| "model queue lock is unavailable".to_string())?;
        Ok(ModelQueueSnapshot {
            active_model_id: active_model_id.clone(),
            entries: queue
                .iter()
                .enumerate()
                .map(|(index, model_id)| ModelQueueEntry {
                    model_id: model_id.clone(),
                    position: (index + 1) as u32,
                    state: if active_model_id.as_deref() == Some(model_id.as_str()) {
                        ModelQueueState::Active
                    } else {
                        ModelQueueState::Queued
                    },
                })
                .collect(),
        })
    }

    fn emit_queue(&self, events: &dyn DesktopEventSink) {
        if let Ok(snapshot) = self.queue_snapshot() {
            emit(events, "dl://model-queue", &snapshot);
        }
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

    /// Returns a managed model directory only when the descriptor is a current MLX model.
    /// Legacy entries are deliberately readable for cleanup, but must never cross this boundary.
    pub fn installed_inference_dir(&self, root: &Path, model_id: &str) -> Result<PathBuf, String> {
        self.with_manager(root, |manager| {
            let descriptor = manager.descriptor(model_id)?;
            if descriptor.ui_role == ModelUiRole::Legacy
                || descriptor.download_source != ModelDownloadSource::Modelscope
            {
                return Err(ModelError::InvalidState(
                    "请选择当前 MLX 模型；旧版模型不能用于推理。".to_string(),
                ));
            }
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

    pub fn reveal_installed_dir(&self, root: &Path, model_id: &str) -> Result<PathBuf, String> {
        let directory = self.installed_dir(root, model_id)?;
        let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
        let canonical_directory = fs::canonicalize(&directory)
            .map_err(|_| format!("{} 的本地模型目录不存在。", model_id))?;
        if !canonical_directory.starts_with(&canonical_root) || !canonical_directory.is_dir() {
            return Err("模型目录不在受管位置内。".to_string());
        }
        Ok(canonical_directory)
    }

    pub fn import_from_folder(
        &self,
        root: &Path,
        model_id: &str,
        source: &Path,
        accepted_noncommercial_license: bool,
    ) -> Result<ModelInstallation, String> {
        let source_metadata =
            fs::symlink_metadata(source).map_err(|_| "选择的模型文件夹不存在。".to_string())?;
        if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
            return Err("请选择普通模型文件夹，不能使用符号链接。".to_string());
        }

        let descriptor = self
            .with_manager(root, |manager| Ok(manager.descriptor(model_id)?.clone()))
            .map_err(|error| error.to_string())?;
        if descriptor.ui_role == ModelUiRole::Legacy {
            return Err("旧版模型只能清理，不能再导入或用于推理。".to_string());
        }
        if descriptor.bundled {
            return Err("随应用提供的模型不需要导入。".to_string());
        }
        if descriptor.requires_noncommercial_confirmation() && !accepted_noncommercial_license {
            return Err("该模型仅限非商业使用，请先确认模型许可。".to_string());
        }
        if self
            .with_manager(root, |manager| manager.installation(model_id))
            .map_err(|error| error.to_string())?
            .state
            == ModelInstallState::Installed
        {
            return Err("该模型已经安装。".to_string());
        }

        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "active downloads lock is unavailable".to_string())?;
            if active.contains_key(model_id) {
                return Err("该模型当前有正在进行的任务，请先结束任务。".to_string());
            }
            active.insert(model_id.to_string(), flag);
        }

        let stage = format!("import-{}", staging_id());
        let staging_base = self
            .with_manager(root, |manager| manager.staging_root(&stage))
            .map_err(|error| error.to_string())?;
        let staging = staging_base.join(&descriptor.id).join(&descriptor.revision);
        let result = (|| {
            for file in descriptor.files.iter().filter(|file| file.allowed) {
                let relative = safe_relative_path(&file.path)?;
                let source_file = source.join(&relative);
                let metadata = fs::symlink_metadata(&source_file)
                    .map_err(|_| format!("缺少模型文件：{}", file.path))?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(format!("模型文件不是普通文件：{}", file.path));
                }
                if metadata.len() != file.size_bytes {
                    return Err(format!("模型文件大小不匹配：{}", file.path));
                }
                let destination = staging.join(&relative);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::copy(&source_file, &destination).map_err(|error| error.to_string())?;
            }
            self.with_manager(root, |manager| {
                manager.verify_directory(model_id, &staging)?;
                manager.atomically_install(model_id, &stage)
            })
            .map_err(|error| error.to_string())
        })();

        if result.is_err() {
            let _ = fs::remove_dir_all(&staging_base);
        }
        if let Ok(mut active) = self.active.lock() {
            active.remove(model_id);
        }
        result
    }

    /// 全量 MLX 迁移后，只允许新的 MLX WeSpeaker 权重进入推理路径。旧中文模型
    /// 可能仍留在安装记录中用于空间统计和清理，但绝不能作为回退偷偷启动。
    pub fn selected_speaker_model_dir(&self, root: &Path) -> Result<PathBuf, String> {
        self.with_manager(root, |manager| {
            let model_id = "wespeaker-voxceleb-resnet34-lm";
            if manager.catalog().get(model_id).is_some()
                && manager.installation(model_id)?.state == ModelInstallState::Installed
            {
                return manager.installation_dir(model_id);
            }
            Err(ModelError::InvalidState(
                "未安装 MLX 说话人识别模型。".to_string(),
            ))
        })
        .map_err(|error| error.to_string())
    }

    pub fn requires_noncommercial_confirmation(
        &self,
        root: &Path,
        model_id: &str,
    ) -> Result<bool, String> {
        self.with_manager(root, |manager| {
            Ok(manager
                .descriptor(model_id)?
                .requires_noncommercial_confirmation())
        })
        .map_err(|error| error.to_string())
    }

    pub fn installation_dir(&self, root: &Path, model_id: &str) -> Result<PathBuf, String> {
        self.with_manager(root, |manager| manager.installation_dir(model_id))
            .map_err(|error| error.to_string())
    }

    pub fn verify(&self, root: &Path, model_id: &str) -> Result<ModelInstallation, String> {
        self.with_manager(root, |manager| {
            if manager.descriptor(model_id)?.ui_role == ModelUiRole::Legacy {
                return Err(ModelError::InvalidState(
                    "旧版模型只能清理，不能重新校验或用于推理。".to_string(),
                ));
            }
            manager.verify_installed(model_id)
        })
        .map_err(|error| error.to_string())
    }

    pub fn remove(&self, root: &Path, model_id: &str) -> Result<ModelInstallation, ModelError> {
        self.with_manager(root, |manager| {
            if manager.descriptor(model_id)?.ui_role == ModelUiRole::Legacy {
                return Err(ModelError::InvalidState(
                    "旧版模型只能通过对应当前模型的“清理旧版本”操作移除。".to_string(),
                ));
            }
            manager.remove(model_id)
        })
    }

    pub fn legacy_cleanup_preview(
        &self,
        root: &Path,
        target_model_id: &str,
    ) -> Result<LegacyModelCleanupPreview, String> {
        self.with_manager(root, |manager| {
            manager.legacy_cleanup_preview(target_model_id)
        })
        .map_err(|error| error.to_string())
    }

    pub fn cleanup_legacy(
        &self,
        root: &Path,
        target_model_id: &str,
    ) -> Result<LegacyModelCleanupPreview, String> {
        self.with_manager(root, |manager| manager.cleanup_legacy(target_model_id))
            .map_err(|error| error.to_string())
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
                || snapshot.descriptor.ui_role == ModelUiRole::Legacy
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
            if snapshot.descriptor.ui_role != ModelUiRole::Legacy {
                next.verify_directory(&snapshot.descriptor.id, &destination)
                    .map_err(|error| error.to_string())?;
            }
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
        model_id: &str,
        accepted_noncommercial_license: bool,
        app_version: String,
        events: Arc<dyn DesktopEventSink>,
    ) -> Result<ModelInstallation, String> {
        self.begin_install_inner(
            root,
            model_id,
            accepted_noncommercial_license,
            app_version,
            events,
            None,
        )
    }

    #[cfg(test)]
    fn begin_install_fixture(
        &self,
        root: PathBuf,
        endpoint: String,
        model_id: &str,
        accepted_noncommercial_license: bool,
        app_version: String,
        events: Arc<dyn DesktopEventSink>,
    ) -> Result<ModelInstallation, String> {
        self.begin_install_inner(
            root,
            model_id,
            accepted_noncommercial_license,
            app_version,
            events,
            Some(endpoint),
        )
    }

    fn begin_install_inner(
        &self,
        root: PathBuf,
        model_id: &str,
        accepted_noncommercial_license: bool,
        app_version: String,
        events: Arc<dyn DesktopEventSink>,
        fixture_endpoint: Option<String>,
    ) -> Result<ModelInstallation, String> {
        self.with_manager(&root, |manager| {
            let descriptor = manager.descriptor(model_id)?;
            if descriptor.ui_role == ModelUiRole::Legacy {
                return Err(ModelError::InvalidState(
                    "旧版模型只能清理，不能重新安装或用于推理。".to_string(),
                ));
            }
            if descriptor.requires_noncommercial_confirmation()
                && !accepted_noncommercial_license
            {
                return Err(ModelError::InvalidState(
                    "该模型仅限非商业使用，请先确认 CC BY-NC-SA 4.0 许可。".to_string(),
                ));
            }
            manager.dependency_order(model_id)
        })
        .map_err(|error| error.to_string())?;
        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "active downloads lock is unavailable".to_string())?;
            if active.contains_key(model_id) {
                return Err("该模型已经在下载队列中。".to_string());
            }
            active.insert(model_id.to_string(), Arc::clone(&flag));
        }
        self.queue
            .lock()
            .map_err(|_| "model queue lock is unavailable".to_string())?
            .push_back(model_id.to_string());
        if let Err(error) = self.with_manager(&root, |manager| manager.queue_install(model_id)) {
            if let Ok(mut active) = self.active.lock() {
                active.remove(model_id);
            }
            if let Ok(mut queue) = self.queue.lock() {
                queue.retain(|queued| queued != model_id);
            }
            return Err(error.to_string());
        }
        let requested = self
            .with_manager(&root, |manager| manager.installation(model_id))
            .map_err(|error| error.to_string())?;
        let state = self.clone();
        let requested_id = model_id.to_string();
        let user_agent = format!("double-love-studio/{app_version}");
        let client = Client::builder()
            .user_agent(&user_agent)
            .build()
            .map_err(|_| "无法建立模型下载连接。".to_string())?;
        self.emit_queue(events.as_ref());
        std::thread::spawn(move || {
            state.run_install_batch(
                events,
                requested_id,
                root,
                client,
                fixture_endpoint,
            );
        });
        Ok(requested)
    }

    pub fn set_cancel(
        &self,
        root: &Path,
        model_id: &str,
        cancel: bool,
    ) -> Result<ModelInstallation, String> {
        self.set_cancel_internal(root, model_id, cancel, None)
    }

    pub fn set_cancel_with_events(
        &self,
        root: &Path,
        model_id: &str,
        cancel: bool,
        events: &dyn DesktopEventSink,
    ) -> Result<ModelInstallation, String> {
        self.set_cancel_internal(root, model_id, cancel, Some(events))
    }

    fn set_cancel_internal(
        &self,
        root: &Path,
        model_id: &str,
        cancel: bool,
        events: Option<&dyn DesktopEventSink>,
    ) -> Result<ModelInstallation, String> {
        let order = self
            .with_manager(root, |manager| manager.dependency_order(model_id))
            .map_err(|error| error.to_string())?;
        let is_current = self
            .current_request
            .lock()
            .map_err(|_| "model queue current lock is unavailable".to_string())?
            .as_deref()
            == Some(model_id);
        let flag = self
            .active
            .lock()
            .map_err(|_| "active downloads lock is unavailable".to_string())?
            .get(model_id)
            .cloned();
        if let Some(flag) = &flag {
            flag.store(true, Ordering::SeqCst);
        }
        if cancel && !is_current {
            if let Ok(mut active) = self.active.lock() {
                active.remove(model_id);
            }
            if let Ok(mut queue) = self.queue.lock() {
                queue.retain(|queued| queued != model_id);
            }
        }
        // A cancellation must not race the worker's final "Paused" cleanup. Wait
        // for the batch to leave the active table before publishing NotInstalled.
        if cancel && is_current && flag.is_some() {
            self.wait_for_batch_idle(&[model_id.to_string()])?;
        }
        let queued_roots = self
            .queue
            .lock()
            .map_err(|_| "model queue lock is unavailable".to_string())?
            .iter()
            .filter(|queued| queued.as_str() != model_id)
            .cloned()
            .collect::<Vec<_>>();
        let preserved_dependencies = self
            .with_manager(root, |manager| {
                let mut preserved = std::collections::BTreeSet::new();
                for queued in &queued_roots {
                    for dependency in manager.dependency_order(queued)? {
                        if dependency != *queued {
                            preserved.insert(dependency);
                        }
                    }
                }
                Ok(preserved)
            })
            .map_err(|error| error.to_string())?;
        let installations = self
            .with_manager(root, |manager| {
                let mut installations = Vec::with_capacity(order.len());
                for id in &order {
                    let current = manager.installation(id)?;
                    let next = if cancel {
                        if current.state == ModelInstallState::Installed
                            || (id != model_id && preserved_dependencies.contains(id))
                        {
                            current.state
                        } else {
                            ModelInstallState::NotInstalled
                        }
                    } else if id == model_id {
                        ModelInstallState::Paused
                    } else {
                        current.state
                    };
                    let installation = if next == current.state {
                        current
                    } else {
                        manager.transition(id, next)?
                    };
                    installations.push(installation);
                }
                Ok(installations)
            })
            .map_err(|error| error.to_string())?;
        if let Some(events) = events {
            for installation in &installations {
                emit_state(events, installation);
            }
        }
        installations
            .into_iter()
            .find(|installation| installation.model_id == model_id)
            .ok_or_else(|| "模型取消状态没有找到请求的模型。".to_string())
    }

    fn wait_for_batch_idle(&self, order: &[String]) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let active = self
                .active
                .lock()
                .map_err(|_| "active downloads lock is unavailable".to_string())?;
            if !order.iter().any(|id| active.contains_key(id)) {
                return Ok(());
            }
            drop(active);
            if Instant::now() >= deadline {
                return Err("下载任务仍在结束，请稍后重试。".to_string());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn run_install_batch(
        &self,
        events: Arc<dyn DesktopEventSink>,
        requested: String,
        root: PathBuf,
        client: Client,
        fixture_endpoint: Option<String>,
    ) {
        let flag = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.get(&requested).cloned())
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        loop {
            let is_front = self
                .queue
                .lock()
                .ok()
                .and_then(|queue| queue.front().cloned())
                .as_deref()
                == Some(requested.as_str());
            if is_front {
                break;
            }
            if flag.load(Ordering::SeqCst) {
                if let Ok(mut active) = self.active.lock() {
                    active.remove(&requested);
                }
                if let Ok(mut queue) = self.queue.lock() {
                    queue.retain(|model_id| model_id != &requested);
                }
                self.emit_queue(events.as_ref());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Ok(mut current) = self.current_request.lock() {
            *current = Some(requested.clone());
        }
        self.emit_queue(events.as_ref());
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
        for model_id in &order {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            if let Err(error) = self.download_model(
                events.as_ref(),
                &root,
                &client,
                fixture_endpoint.as_deref(),
                model_id,
                &flag,
            ) {
                if !flag.load(Ordering::SeqCst)
                    && let Ok(installation) = self.with_manager(&root, |manager| {
                        manager.mark_error(model_id, "MODEL_INSTALL_FAILED", error)
                    })
                {
                    emit_state(events.as_ref(), &installation);
                }
                if model_id != &requested {
                    let dependency_name = self
                        .with_manager(&root, |manager| {
                            Ok(manager.descriptor(model_id)?.display_name.clone())
                        })
                        .unwrap_or_else(|_| model_id.clone());
                    if let Ok(installation) = self.with_manager(&root, |manager| {
                        manager.mark_error(
                            &requested,
                            "MODEL_DEPENDENCY_FAILED",
                            format!("{dependency_name} 下载失败，请重试安装。"),
                        )
                    }) {
                        emit_state(events.as_ref(), &installation);
                    }
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
            active.remove(&requested);
        }
        if let Ok(mut queue) = self.queue.lock() {
            if queue.front().is_some_and(|model_id| model_id == &requested) {
                queue.pop_front();
            } else {
                queue.retain(|model_id| model_id != &requested);
            }
        }
        if let Ok(mut current) = self.current_request.lock() {
            if current.as_deref() == Some(requested.as_str()) {
                *current = None;
            }
        }
        self.emit_queue(events.as_ref());
    }

    fn download_model(
        &self,
        events: &dyn DesktopEventSink,
        root: &Path,
        client: &Client,
        fixture_endpoint: Option<&str>,
        model_id: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        #[cfg(not(test))]
        let _ = fixture_endpoint;
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
        let staging = self
            .with_manager(root, |manager| manager.staging_root(&stage))
            .map_err(|error| error.to_string())?
            .join(&descriptor.id)
            .join(&descriptor.revision);
        fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
        if descriptor.download_source == ModelDownloadSource::Modelscope {
            if !download_modelscope_snapshot(
                self,
                events,
                root,
                model_id,
                &descriptor,
                &staging,
                client,
                cancel,
            )? {
                return Ok(());
            }
        } else {
            // 旧的 URL 下载器只保留给 Rust fixture 测试。发布目录的所有可安装模型
            // 都是 ModelScope + MLX，因此绝不会进入这条路径。
            #[cfg(not(test))]
            {
                return Err("发布版本只支持受管的 ModelScope MLX 模型。".to_string());
            }
            #[cfg(test)]
            let endpoint = fixture_endpoint.unwrap_or_default();
            #[cfg(test)]
            if let Some(archive) = descriptor.archive.clone() {
                let archive_path = self
                    .with_manager(root, |manager| {
                        manager.archive_staging_path(model_id, &stage)
                    })
                    .map_err(|error| error.to_string())?;
                let part_path = PathBuf::from(format!("{}.part", archive_path.to_string_lossy()));
                if let Some(parent) = archive_path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                let archive_is_valid = archive_path.exists()
                    && self
                        .with_manager(root, |manager| {
                            manager.verify_archive(model_id, &archive_path)
                        })
                        .is_ok();
                if !archive_is_valid {
                    if archive_path.exists() {
                        fs::remove_file(&archive_path).map_err(|error| error.to_string())?;
                    }
                    if part_path
                        .metadata()
                        .is_ok_and(|metadata| metadata.len() == archive.size_bytes)
                    {
                        fs::rename(&part_path, &archive_path).map_err(|error| error.to_string())?;
                    } else {
                        let outcome = download_file_with_resume(
                            &client,
                            &archive.url,
                            &part_path,
                            archive.size_bytes,
                            cancel,
                            |written, bytes_per_second| {
                                let progress = self
                                    .with_manager(root, |manager| {
                                        manager.update_progress(
                                            model_id,
                                            Some("模型归档".to_string()),
                                            written,
                                            written,
                                            archive.size_bytes,
                                            Some(bytes_per_second),
                                        )
                                    })
                                    .map_err(|error| error.to_string())?;
                                emit(events, "dl://model-progress", &progress);
                                Ok(())
                            },
                        )
                        .map_err(|error| {
                            format!("{} · 模型归档：{error}", descriptor.display_name)
                        })?;
                        if outcome.is_none() {
                            return Ok(());
                        }
                        fs::rename(&part_path, &archive_path).map_err(|error| error.to_string())?;
                    }
                }
                self.with_manager(root, |manager| {
                    manager.verify_archive(model_id, &archive_path)?;
                    manager.extract_archive_to_staging(model_id, &stage, &archive_path)
                })
                .map_err(|error| error.to_string())?;
            } else {
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
                    )
                    .map_err(|error| {
                        format!("{} · {}：{error}", descriptor.display_name, file.path)
                    })?;
                    if outcome.is_none() {
                        return Ok(());
                    }
                    fs::rename(&part_path, &final_path).map_err(|error| error.to_string())?;
                    completed_before = completed_before.saturating_add(file.size_bytes);
                }
            }
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

#[cfg(test)]
fn model_url(
    descriptor: &ModelDescriptor,
    file: &str,
    legacy_endpoint: &str,
) -> Result<String, String> {
    let url = descriptor
        .source_url(file)
        .map_err(|error| error.to_string())?;
    #[cfg(test)]
    if legacy_endpoint.starts_with("http://127.0.0.1:")
        && let Some(suffix) = url.strip_prefix("https://huggingface.co")
    {
        return Ok(format!("{}{suffix}", legacy_endpoint.trim_end_matches('/')));
    }
    let _ = legacy_endpoint;
    Ok(url)
}

/// Download the immutable ModelScope file whitelist directly with the production reqwest
/// client. The URL comes only from the catalog descriptor; renderer preferences are not
/// consulted. Files are fetched in manifest order and remain in `.part` form until the
/// manager verifies and atomically installs the complete staging directory.
fn download_modelscope_snapshot(
    state: &ModelState,
    events: &dyn DesktopEventSink,
    root: &Path,
    model_id: &str,
    descriptor: &ModelDescriptor,
    staging: &Path,
    client: &Client,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    if descriptor.revision.len() != 40
        || !descriptor
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("模型清单包含无效的固定 revision。".to_string());
    }
    if descriptor.files.iter().all(|file| !file.allowed) {
        return Err("ModelScope 模型没有可下载的白名单文件。".to_string());
    }

    let mut completed_before = 0_u64;
    for file in descriptor.files.iter().filter(|file| file.allowed) {
        if cancel.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let relative = safe_relative_path(&file.path)?;
        let final_path = staging.join(&relative);
        if fs::symlink_metadata(&final_path).is_ok_and(|metadata| {
            metadata.file_type().is_file() && metadata.len() == file.size_bytes
        }) {
            completed_before = completed_before.saturating_add(file.size_bytes);
            continue;
        }
        let part_path = PathBuf::from(format!("{}.part", final_path.to_string_lossy()));
        if let Some(parent) = part_path.parent() {
            fs::create_dir_all(parent).map_err(|_| "模型下载目录不可用。".to_string())?;
        }
        let url = descriptor
            .source_url(&file.path)
            .map_err(|_| "ModelScope 模型下载地址无效。".to_string())?;
        if !url.starts_with("https://www.modelscope.cn/api/v1/models/") {
            return Err("ModelScope 模型下载地址无效。".to_string());
        }
        let outcome = download_file_with_resume(
            client,
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
                emit(events, "dl://model-progress", &progress);
                Ok(())
            },
        )
        .map_err(|error| format!("{} · {}：{error}", descriptor.display_name, file.path))?;
        if outcome.is_none() || cancel.load(Ordering::SeqCst) {
            return Ok(false);
        }
        fs::rename(&part_path, &final_path).map_err(|_| "无法完成模型文件保存。".to_string())?;
        completed_before = completed_before.saturating_add(file.size_bytes);
    }
    Ok(true)
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

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let range = value.trim().strip_prefix("bytes ")?;
    let (bounds, total) = range.split_once('/')?;
    let (start, end) = bounds.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    let total = total.parse().ok()?;
    (start <= end && end < total).then_some((start, end, total))
}

const MAX_DOWNLOAD_ATTEMPTS: usize = 5;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(100);

enum DownloadAttemptError {
    Transient,
    Fatal(String),
}

fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn is_transient_request_error(error: &reqwest::Error) -> bool {
    !error.is_builder()
        && !error.is_redirect()
        && (error.is_connect() || error.is_timeout() || error.is_request() || error.is_body())
}

fn retry_delay(retry_index: usize) -> Duration {
    RETRY_BASE_DELAY
        .checked_mul(2_u32.saturating_pow(retry_index as u32))
        .unwrap_or(Duration::MAX)
}

fn wait_for_retry(delay: Duration, cancel: &AtomicBool) -> bool {
    let deadline = Instant::now() + delay;
    while !cancel.load(Ordering::SeqCst) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
    false
}

fn download_file_with_resume(
    client: &Client,
    url: &str,
    part_path: &Path,
    expected_size: u64,
    cancel: &AtomicBool,
    on_progress: impl FnMut(u64, u64) -> Result<(), String>,
) -> Result<Option<u64>, String> {
    download_file_with_resume_and_wait(
        client,
        url,
        part_path,
        expected_size,
        cancel,
        wait_for_retry,
        on_progress,
    )
}

fn download_file_with_resume_and_wait(
    client: &Client,
    url: &str,
    part_path: &Path,
    expected_size: u64,
    cancel: &AtomicBool,
    mut retry_wait: impl FnMut(Duration, &AtomicBool) -> bool,
    mut on_progress: impl FnMut(u64, u64) -> Result<(), String>,
) -> Result<Option<u64>, String> {
    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        match download_file_attempt(
            client,
            url,
            part_path,
            expected_size,
            cancel,
            &mut on_progress,
        ) {
            Ok(result) => return Ok(result),
            Err(DownloadAttemptError::Fatal(error)) => return Err(error),
            Err(DownloadAttemptError::Transient) => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                if attempt + 1 == MAX_DOWNLOAD_ATTEMPTS || !retry_wait(retry_delay(attempt), cancel)
                {
                    return if cancel.load(Ordering::SeqCst) {
                        Ok(None)
                    } else {
                        Err("模型下载失败。".to_string())
                    };
                }
            }
        }
    }
    unreachable!("download attempts always return before the loop ends")
}

fn download_file_attempt(
    client: &Client,
    url: &str,
    part_path: &Path,
    expected_size: u64,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(u64, u64) -> Result<(), String>,
) -> Result<Option<u64>, DownloadAttemptError> {
    let metadata = match fs::symlink_metadata(part_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DownloadAttemptError::Fatal(
                "模型下载暂存文件不可用。".to_string(),
            ));
        }
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(DownloadAttemptError::Fatal(
                "无法读取模型下载状态。".to_string(),
            ));
        }
    };
    let mut offset = metadata.as_ref().map_or(0, fs::Metadata::len);
    if offset > expected_size {
        fs::remove_file(part_path)
            .map_err(|_| DownloadAttemptError::Fatal("无法重置模型下载暂存文件。".to_string()))?;
        offset = 0;
    }
    if metadata.is_some() && offset == expected_size {
        return Ok(Some(offset));
    }

    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = match request.send() {
        Ok(response) => response,
        Err(error) if is_transient_request_error(&error) => {
            return Err(DownloadAttemptError::Transient);
        }
        Err(_) => {
            return Err(DownloadAttemptError::Fatal(
                "模型下载地址无效。".to_string(),
            ));
        }
    };
    let status = response.status();
    if is_transient_status(status) {
        return Err(DownloadAttemptError::Transient);
    }
    if status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT {
        return Err(DownloadAttemptError::Fatal(format!(
            "模型服务器返回 HTTP {}",
            response.status()
        )));
    }
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range);
    let expected_range = (offset, expected_size.saturating_sub(1), expected_size);
    if status == StatusCode::PARTIAL_CONTENT
        && (expected_size == 0 || content_range != Some(expected_range))
    {
        return Err(DownloadAttemptError::Fatal(
            "模型服务器返回了缺失或不匹配的 Range。".to_string(),
        ));
    }
    if status == StatusCode::OK && offset > 0 && content_range != Some(expected_range) {
        return Err(DownloadAttemptError::Fatal(
            "模型服务器返回了缺失或不匹配的 Range。".to_string(),
        ));
    }
    if offset == 0
        && content_range.is_some()
        && (expected_size == 0 || content_range != Some(expected_range))
    {
        return Err(DownloadAttemptError::Fatal(
            "模型服务器返回了缺失或不匹配的 Range。".to_string(),
        ));
    }
    let expected_body_size = expected_size.saturating_sub(offset);
    if response
        .content_length()
        .is_some_and(|size| size != expected_body_size)
    {
        return Err(DownloadAttemptError::Fatal(
            "模型服务器返回的内容大小与清单不一致。".to_string(),
        ));
    }
    let append = offset > 0;
    let mut written = if append { offset } else { 0 };
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part_path)
        .map_err(|_| DownloadAttemptError::Fatal("无法打开模型下载暂存文件。".to_string()))?;
    let started = Instant::now();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            output
                .sync_all()
                .map_err(|_| DownloadAttemptError::Fatal("无法保存模型下载进度。".to_string()))?;
            return Ok(None);
        }
        let count = match response.read(&mut buffer) {
            Ok(count) => count,
            Err(_) => {
                let _ = output.sync_all();
                return Err(DownloadAttemptError::Transient);
            }
        };
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| DownloadAttemptError::Fatal("无法保存模型下载进度。".to_string()))?;
        written = written.saturating_add(count as u64);
        if written > expected_size {
            return Err(DownloadAttemptError::Fatal(
                "下载文件超过清单大小。".to_string(),
            ));
        }
        let elapsed = started.elapsed().as_secs().max(1);
        on_progress(written, written.saturating_sub(offset) / elapsed)
            .map_err(DownloadAttemptError::Fatal)?;
    }
    output
        .sync_all()
        .map_err(|_| DownloadAttemptError::Fatal("无法保存模型下载文件。".to_string()))?;
    if written != expected_size {
        return Err(DownloadAttemptError::Fatal(
            "下载文件大小与清单不一致。".to_string(),
        ));
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
                    },
                    {
                        "id": "asr-alt", "display_name": "ASR alternative", "component": "asr",
                        "repo_id": "fixture/asr-alt", "revision": "3333333333333333333333333333333333333333",
                        "files": [{"path": "model.bin", "size_bytes": expected.len(), "sha256": hash, "allowed": true}],
                        "license": "MIT", "license_url": "https://example.test/license",
                        "dependencies": [{"model_id": "dependency", "required": true, "reason": "fixture"}],
                        "min_memory_bytes": null, "source_url_template": "https://huggingface.co/fixture/asr-alt/resolve/{revision}/{path}", "bundled": false
                    }
                ]
            })
            .to_string(),
        )
        .expect("fixture catalog")
    }

    fn speaker_catalog() -> ModelCatalog {
        let hash = format!("{:x}", sha2::Sha256::digest(b"hello"));
        ModelCatalog::from_json(
            &json!({
                "schema_version": 1,
                "models": [
                    {
                        "id": "wespeaker-voxceleb-resnet34-lm", "display_name": "MLX speaker", "component": "speaker",
                        "ui_role": "primary", "repo_id": "mlx-community/wespeaker-voxceleb-resnet34-LM",
                        "download_source": "modelscope", "revision": "3333333333333333333333333333333333333333",
                        "files": [{"path": "config.json", "size_bytes": 5, "sha256": hash, "allowed": true}],
                        "license": "MIT", "license_url": "https://opensource.org/license/mit",
                        "dependencies": [], "min_memory_bytes": null,
                        "source_url_template": "https://www.modelscope.cn/api/v1/models/mlx-community/wespeaker-voxceleb-resnet34-LM/repo?Revision={revision}&FilePath={path}", "bundled": false
                    },
                    {
                        "id": "wespeaker-zh", "display_name": "Legacy Chinese speaker", "component": "speaker",
                        "ui_role": "legacy", "repo_id": "Wespeaker/wespeaker-cnceleb-resnet34",
                        "download_source": "bundled", "revision": "4444444444444444444444444444444444444444",
                        "files": [],
                        "license": "CNCeleb Research-Only", "license_url": "https://www.cnceleb.org/",
                        "dependencies": [], "min_memory_bytes": null,
                        "source_url_template": "legacy://wespeaker-zh/{path}", "bundled": false
                    },
                    {
                        "id": "research-model", "display_name": "Research fixture", "component": "speaker",
                        "ui_role": "primary", "repo_id": "fixture/research",
                        "download_source": "huggingface", "revision": "5555555555555555555555555555555555555555",
                        "files": [{"path": "config.yaml", "size_bytes": 5, "sha256": hash, "allowed": true}],
                        "license": "CNCeleb Research-Only", "license_url": "https://www.cnceleb.org/",
                        "dependencies": [], "min_memory_bytes": null,
                        "source_url_template": "https://huggingface.co/fixture/research/resolve/{revision}/{path}", "bundled": false
                    }
                ]
            })
            .to_string(),
        )
        .expect("speaker catalog")
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

    fn range_response_server(
        body: Vec<u8>,
        status: u16,
        content_range: Option<&str>,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let content_range = content_range.map(str::to_string);
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read request");
            let reason = if status == 206 {
                "Partial Content"
            } else {
                "OK"
            };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n",
                body.len()
            )
            .expect("headers");
            if let Some(content_range) = content_range {
                write!(stream, "Content-Range: {content_range}\r\n").expect("range");
            }
            write!(stream, "Connection: close\r\n\r\n").expect("headers end");
            stream.write_all(&body).expect("body");
        });
        (format!("http://{address}"), handle)
    }

    fn response_sequence_server(
        statuses: Vec<Option<u16>>,
        body: Vec<u8>,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            for status in statuses {
                let (mut stream, _) = listener.accept().expect("request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read request");
                let Some(status) = status else {
                    continue;
                };
                let reason = match status {
                    200 => "OK",
                    408 => "Request Timeout",
                    429 => "Too Many Requests",
                    403 => "Forbidden",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    502 => "Bad Gateway",
                    503 => "Service Unavailable",
                    504 => "Gateway Timeout",
                    _ => "Error",
                };
                let response_body: &[u8] = if status == 200 { body.as_slice() } else { &[] };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                )
                .expect("headers");
                let _ = stream.write_all(response_body);
            }
        });
        (format!("http://{address}"), handle)
    }

    fn user_agent_server(bytes: Vec<u8>, requests: usize) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().expect("request");
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).expect("read request");
                let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
                if !request.contains("user-agent: double-love-studio/0.2.0") {
                    write!(
                        stream,
                        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .expect("forbidden response");
                    continue;
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .expect("headers");
                stream.write_all(&bytes).expect("body");
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
            .begin_install_fixture(
                root.clone(),
                endpoint.clone(),
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
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
            .begin_install_fixture(
                root.clone(),
                endpoint.clone(),
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
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
        assert_eq!(report.model_checks.len(), 3);
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
    fn cancel_resets_uninstalled_batch_and_preserves_partial_staging() {
        let bytes = vec![b'x'; 4096];
        let (endpoint, fixture) = server(bytes.clone(), 1, true);
        let root = temp("cancel-batch");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));
        let events = Arc::new(Events::default());

        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("install queues");
        for _ in 0..200 {
            let dependency = state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "dependency")
                .expect("dependency")
                .installation;
            if dependency.bytes_downloaded > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let before = state
            .snapshot(&root)
            .expect("snapshot")
            .into_iter()
            .find(|item| item.descriptor.id == "dependency")
            .expect("dependency")
            .installation;
        let cancelled = state
            .set_cancel_with_events(&root, "asr", true, events.as_ref())
            .expect("cancel");
        assert_eq!(cancelled.state, ModelInstallState::NotInstalled);
        wait_state(&state, &root, "dependency", ModelInstallState::NotInstalled);
        let snapshot = state.snapshot(&root).expect("snapshot");
        let dependency = snapshot
            .iter()
            .find(|item| item.descriptor.id == "dependency")
            .expect("dependency")
            .installation
            .clone();
        let asr = snapshot
            .iter()
            .find(|item| item.descriptor.id == "asr")
            .expect("asr")
            .installation
            .clone();
        assert_eq!(dependency.state, ModelInstallState::NotInstalled);
        assert_eq!(dependency.staging_id, before.staging_id);
        assert!(dependency.bytes_downloaded > 0);
        assert_eq!(asr.state, ModelInstallState::NotInstalled);
        assert!(
            events
                .0
                .lock()
                .expect("events")
                .iter()
                .any(|(channel, payload)| channel == "dl://model-state"
                    && payload["model_id"] == "asr"
                    && payload["state"] == "not_installed")
        );

        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn cancel_waits_for_worker_then_reinstall_resumes_without_duplicate_task() {
        let bytes = vec![b'x'; 4096];
        let (endpoint, fixture) = server(bytes.clone(), 3, true);
        let root = temp("cancel-reinstall");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));
        let events = Arc::new(Events::default());

        state
            .begin_install_fixture(
                root.clone(),
                endpoint.clone(),
                "dependency",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("dependency install");
        wait_state(&state, &root, "dependency", ModelInstallState::Installed);
        state
            .begin_install_fixture(
                root.clone(),
                endpoint.clone(),
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("asr install");
        for _ in 0..200 {
            let asr = state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "asr")
                .expect("asr")
                .installation;
            if asr.bytes_downloaded > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let before = state
            .snapshot(&root)
            .expect("snapshot")
            .into_iter()
            .find(|item| item.descriptor.id == "asr")
            .expect("asr")
            .installation;
        state
            .set_cancel_with_events(&root, "asr", true, events.as_ref())
            .expect("cancel");
        assert_eq!(
            state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "asr")
                .expect("asr")
                .installation
                .state,
            ModelInstallState::NotInstalled
        );
        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("reinstall after cancel");
        wait_state(&state, &root, "asr", ModelInstallState::Installed);
        let after = state
            .snapshot(&root)
            .expect("snapshot")
            .into_iter()
            .find(|item| item.descriptor.id == "asr")
            .expect("asr")
            .installation;
        assert!(before.staging_id.is_some());
        assert!(after.staging_id.is_none());

        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn model_downloads_identify_the_desktop_client() {
        let bytes = b"model".to_vec();
        let (endpoint, fixture) = user_agent_server(bytes.clone(), 2);
        let root = temp("user-agent");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));

        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "asr",
                true,
                "0.2.0".to_string(),
                Arc::new(Events::default()),
            )
            .expect("install queues");
        wait_state(&state, &root, "asr", ModelInstallState::Installed);

        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn dependency_failure_makes_the_requested_model_retryable() {
        let bytes = b"model".to_vec();
        let (endpoint, fixture) = range_response_server(Vec::new(), 403, None);
        let root = temp("dependency-failure");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));

        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "asr",
                true,
                "0.2.0".to_string(),
                Arc::new(Events::default()),
            )
            .expect("install queues");
        wait_state(&state, &root, "dependency", ModelInstallState::Failed);
        wait_state(&state, &root, "asr", ModelInstallState::Failed);
        let requested = state
            .snapshot(&root)
            .expect("snapshot")
            .into_iter()
            .find(|item| item.descriptor.id == "asr")
            .expect("requested model")
            .installation;
        assert_eq!(
            requested.last_error_code.as_deref(),
            Some("MODEL_DEPENDENCY_FAILED")
        );
        assert!(
            requested
                .last_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Dependency"))
        );

        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn shared_dependency_does_not_leave_a_second_primary_orphaned() {
        let bytes = vec![b'x'; 4096];
        let (endpoint, fixture) = server(bytes.clone(), 1, true);
        let root = temp("shared-dependency");
        let state = ModelState::with_catalog(catalog(&endpoint, &bytes));
        let events = Arc::new(Events::default());

        state
            .begin_install_fixture(
                root.clone(),
                endpoint.clone(),
                "asr",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("first model queues");
        for _ in 0..200 {
            let dependency = state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "dependency")
                .expect("dependency")
                .installation;
            if dependency.bytes_downloaded > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "asr-alt",
                true,
                "0.2.0".to_string(),
                events.clone(),
            )
            .expect("second model joins queue");
        let queue = state.queue_snapshot().expect("queue snapshot");
        assert_eq!(queue.entries.len(), 2);
        assert_eq!(
            state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "asr-alt")
                .expect("alternative")
                .installation
                .state,
            ModelInstallState::Queued
        );
        state
            .set_cancel(&root, "asr-alt", true)
            .expect("cancel queued second");
        assert_eq!(state.queue_snapshot().expect("queue").entries.len(), 1);
        state.set_cancel(&root, "asr", true).expect("cancel first");
        wait_state(&state, &root, "dependency", ModelInstallState::NotInstalled);

        fixture.join().expect("fixture server");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn imports_only_manifest_files_and_reveals_only_verified_installations() {
        let expected = b"hello".to_vec();
        let root = temp("folder-import");
        let source = temp("folder-import-source");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("model.bin"), &expected).expect("model file");
        fs::write(source.join("README.md"), b"ignored").expect("extra file");
        let state = ModelState::with_catalog(catalog("https://example.invalid", &expected));

        let installation = state
            .import_from_folder(&root, "dependency", &source, true)
            .expect("valid folder import");
        assert_eq!(installation.state, ModelInstallState::Installed);
        let installed = state
            .reveal_installed_dir(&root, "dependency")
            .expect("verified reveal directory");
        assert_eq!(
            fs::read(installed.join("model.bin")).expect("model"),
            expected
        );
        assert!(!installed.join("README.md").exists());

        state
            .remove(&root, "dependency")
            .expect("remove imported model");
        fs::write(source.join("model.bin"), b"HELLO").expect("invalid model");
        let error = state
            .import_from_folder(&root, "dependency", &source, true)
            .expect_err("hash mismatch is rejected");
        assert!(error.contains("校验失败"));
        assert_eq!(
            state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "dependency")
                .expect("dependency")
                .installation
                .state,
            ModelInstallState::NotInstalled
        );

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(source).expect("cleanup source");
    }

    #[cfg(unix)]
    #[test]
    fn folder_import_rejects_symlinked_model_files() {
        use std::os::unix::fs::symlink;

        let expected = b"hello".to_vec();
        let root = temp("folder-import-symlink");
        let source = temp("folder-import-symlink-source");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("target.bin"), &expected).expect("target");
        symlink(source.join("target.bin"), source.join("model.bin")).expect("symlink");
        let state = ModelState::with_catalog(catalog("https://example.invalid", &expected));

        let error = state
            .import_from_folder(&root, "dependency", &source, true)
            .expect_err("symlink is rejected");
        assert!(error.contains("不是普通文件"));

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(source).expect("cleanup source");
    }

    #[test]
    fn zero_byte_cancel_returns_not_installed_and_hash_failure_becomes_corrupt() {
        let expected = vec![b'a'; 1024];
        let (endpoint, _cancel_fixture) = server(vec![b'b'; 1024], 1, true);
        let root = temp("cancel-corrupt");
        let state = ModelState::with_catalog(catalog(&endpoint, &expected));
        let events = Arc::new(Events::default());
        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "dependency",
                true,
                "0.2.0".to_string(),
                events,
            )
            .expect("queue");
        let cancelled = state.set_cancel(&root, "dependency", true).expect("cancel");
        assert_eq!(cancelled.state, ModelInstallState::NotInstalled);
        thread::sleep(Duration::from_millis(100));

        let (endpoint, fixture) = server(vec![b'b'; 1024], 1, false);
        // A new state reads the persisted not-installed snapshot and uses the same fixture catalog.
        let state = ModelState::with_catalog(catalog(&endpoint, &expected));
        state
            .begin_install_fixture(
                root.clone(),
                endpoint,
                "dependency",
                true,
                "0.2.0".to_string(),
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

        let catalog = ModelCatalog::builtin().expect("catalog");
        let modelscope = catalog
            .get("qwen3-asr-0.6b-4bit")
            .expect("ModelScope model");
        assert_eq!(modelscope.download_source, ModelDownloadSource::Modelscope);
        assert!(
            model_url(modelscope, "config.json", "https://mirror.invalid")
                .expect("model URL")
                .starts_with("https://www.modelscope.cn/api/v1/models/mlx-community/Qwen3-ASR-0.6B-4bit/repo?Revision=70ccd0ba0c24b0c78efc313ce81c1c78c64a3dd7")
        );

        let client = Client::builder().build().expect("client");
        let directory = temp("range");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");

        let (endpoint, fixture) = server(b"hello".to_vec(), 1, false);
        fs::write(&part, b"he").expect("partial");
        let outcome = download_file_with_resume(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .expect("206 resume");
        assert_eq!(outcome, Some(5));
        assert_eq!(fs::read(&part).expect("file"), b"hello");
        fixture.join().expect("fixture");

        let (endpoint, fixture) = range_response_server(b"llo".to_vec(), 200, Some("bytes 2-4/5"));
        fs::write(&part, b"he").expect("partial");
        download_file_with_resume(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .expect("ModelScope 200 + Content-Range resume");
        assert_eq!(fs::read(&part).expect("file"), b"hello");
        fixture.join().expect("fixture");

        for content_range in [
            None,
            Some("bytes 2-3/5"),
            Some("bytes 1-4/5"),
            Some("bytes 2-4/6"),
        ] {
            let (endpoint, fixture) = range_response_server(b"llo".to_vec(), 200, content_range);
            fs::write(&part, b"he").expect("partial");
            let error = download_file_with_resume(
                &client,
                &format!("{endpoint}/model.bin"),
                &part,
                5,
                &AtomicBool::new(false),
                |_, _| Ok(()),
            )
            .expect_err("missing or mismatched range must fail");
            assert!(error.contains("Range"));
            fixture.join().expect("fixture");
        }
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn downloader_fails_closed_for_sizes_and_preserves_cancelled_part() {
        let client = Client::builder()
            .user_agent("double-love-studio/0.2.0")
            .build()
            .expect("client");
        let directory = temp("downloader-fail-closed");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");

        let (endpoint, fixture) = range_response_server(b"hello".to_vec(), 200, None);
        let error = download_file_with_resume(
            &client,
            &format!("{endpoint}/short.bin"),
            &part,
            6,
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .expect_err("short response must fail");
        assert!(!error.contains(&endpoint));
        assert!(!error.contains(&directory.to_string_lossy().into_owned()));
        fixture.join().expect("short fixture");

        let (endpoint, fixture) = range_response_server(b"hello!".to_vec(), 200, None);
        let error = download_file_with_resume(
            &client,
            &format!("{endpoint}/oversize.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .expect_err("oversize response must fail");
        assert!(!error.contains(&endpoint));
        assert!(!error.contains(&directory.to_string_lossy().into_owned()));
        fixture.join().expect("oversize fixture");

        fs::write(&part, b"partial").expect("partial");
        let (endpoint, fixture) = server(vec![b'x'; 4096], 1, true);
        let cancel = AtomicBool::new(true);
        let outcome = download_file_with_resume(
            &client,
            &format!("{endpoint}/cancel.bin"),
            &part,
            4096,
            &cancel,
            |_, _| Ok(()),
        )
        .expect("cancelled download");
        assert_eq!(outcome, None);
        assert_eq!(fs::read(&part).expect("preserved part"), b"partial");
        fixture.join().expect("cancel fixture");

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn transient_http_failures_retry_five_attempts_with_incremental_wait() {
        let client = Client::builder().build().expect("client");
        let directory = temp("retry-http");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");
        let (endpoint, fixture) = response_sequence_server(
            vec![Some(503), Some(429), Some(408), Some(500), Some(200)],
            b"hello".to_vec(),
        );
        let mut delays = Vec::new();
        let outcome = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |delay, _| {
                delays.push(delay);
                true
            },
            |_, _| Ok(()),
        )
        .expect("transient HTTP failures should recover");
        assert_eq!(outcome, Some(5));
        assert_eq!(fs::read(&part).expect("downloaded file"), b"hello");
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
                Duration::from_millis(800),
            ]
        );
        fixture.join().expect("retry fixture");
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn network_error_retries_and_cancel_interrupts_retry_wait_without_deleting_part() {
        let client = Client::builder().build().expect("client");
        let directory = temp("retry-network");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");
        let (endpoint, fixture) =
            response_sequence_server(vec![None, Some(200)], b"hello".to_vec());
        let mut delays = Vec::new();
        let outcome = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |delay, _| {
                delays.push(delay);
                true
            },
            |_, _| Ok(()),
        )
        .expect("network error should recover");
        assert_eq!(outcome, Some(5));
        assert_eq!(fs::read(&part).expect("downloaded file"), b"hello");
        assert_eq!(delays, vec![Duration::from_millis(100)]);
        fixture.join().expect("network fixture");

        fs::write(&part, b"he").expect("partial file");
        let (endpoint, fixture) = response_sequence_server(vec![Some(503)], b"hello".to_vec());
        let cancel = AtomicBool::new(false);
        let mut waits = 0;
        let outcome = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &cancel,
            |_, cancel| {
                waits += 1;
                cancel.store(true, Ordering::SeqCst);
                false
            },
            |_, _| Ok(()),
        )
        .expect("cancelled retry should return cleanly");
        assert_eq!(outcome, None);
        assert_eq!(waits, 1);
        assert_eq!(fs::read(&part).expect("preserved partial file"), b"he");
        fixture.join().expect("cancel retry fixture");
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn non_transient_http_range_and_size_errors_are_not_retried() {
        let client = Client::builder().build().expect("client");
        let directory = temp("retry-fatal");
        fs::create_dir_all(&directory).expect("directory");
        let part = directory.join("model.bin.part");

        let (endpoint, fixture) = response_sequence_server(vec![Some(404)], Vec::new());
        let mut waits = 0;
        let error = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| {
                waits += 1;
                true
            },
            |_, _| Ok(()),
        )
        .expect_err("404 must fail without retry");
        assert!(error.contains("HTTP 404"));
        assert_eq!(waits, 0);
        fixture.join().expect("404 fixture");

        fs::write(&part, b"he").expect("partial file");
        let (endpoint, fixture) = range_response_server(b"llo".to_vec(), 200, Some("bytes 1-3/5"));
        let mut waits = 0;
        let error = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| {
                waits += 1;
                true
            },
            |_, _| Ok(()),
        )
        .expect_err("bad Range must fail without retry");
        assert!(error.contains("Range"));
        assert_eq!(waits, 0);
        fixture.join().expect("Range fixture");

        let (endpoint, fixture) = range_response_server(b"llo".to_vec(), 200, None);
        fs::write(&part, b"he").expect("partial file");
        let mut waits = 0;
        let error = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| {
                waits += 1;
                true
            },
            |_, _| Ok(()),
        )
        .expect_err("missing Range must fail without retry");
        assert!(error.contains("Range"));
        assert_eq!(waits, 0);
        fixture.join().expect("missing Range fixture");

        let (endpoint, fixture) = range_response_server(b"ll".to_vec(), 200, None);
        fs::remove_file(&part).expect("remove partial file");
        let mut waits = 0;
        let error = download_file_with_resume_and_wait(
            &client,
            &format!("{endpoint}/model.bin"),
            &part,
            5,
            &AtomicBool::new(false),
            |_, _| {
                waits += 1;
                true
            },
            |_, _| Ok(()),
        )
        .expect_err("short body must fail without retry");
        assert!(error.contains("大小"));
        assert_eq!(waits, 0);
        fixture.join().expect("size fixture");

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn speaker_runtime_selects_only_the_current_mlx_model() {
        let root = temp("speaker-selection");
        let state = ModelState::with_catalog(speaker_catalog());
        state
            .with_manager(&root, |manager| {
                manager.transition(
                    "wespeaker-voxceleb-resnet34-lm",
                    ModelInstallState::Installed,
                )?;
                manager.transition("wespeaker-zh", ModelInstallState::Installed)?;
                Ok(())
            })
            .expect("mark installed");
        assert!(
            state
                .selected_speaker_model_dir(&root)
                .expect("MLX speaker model")
                .to_string_lossy()
                .contains("wespeaker-voxceleb-resnet34-lm")
        );
        state
            .with_manager(&root, |manager| {
                manager.remove("wespeaker-voxceleb-resnet34-lm")
            })
            .expect("remove MLX speaker");
        assert!(
            state
                .selected_speaker_model_dir(&root)
                .expect_err("legacy speaker must not be selected")
                .contains("MLX")
        );
        assert!(
            state
                .requires_noncommercial_confirmation(&root, "research-model")
                .expect("license lookup")
        );
        let error = state
            .begin_install_fixture(
                root.clone(),
                "https://example.invalid".to_string(),
                "wespeaker-zh",
                true,
                "0.2.0".to_string(),
                Arc::new(Events::default()),
            )
            .expect_err("legacy speaker cannot be installed");
        assert!(error.contains("旧版模型"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn legacy_cleanup_is_scoped_to_a_current_model_and_preserves_shared_aligner() {
        let root = temp("legacy-cleanup");
        let state = ModelState::default();
        state
            .with_manager(&root, |manager| {
                for model_id in [
                    "qwen3-asr-0.6b-4bit",
                    "qwen3-asr-1.7b-8bit",
                    "qwen3-asr-0.6b",
                    "qwen3-asr-1.7b",
                    "qwen3-forced-aligner-0.6b",
                ] {
                    let directory = manager.installation_dir(model_id)?;
                    fs::create_dir_all(&directory)?;
                    fs::write(directory.join("legacy.bin"), model_id.as_bytes())?;
                    manager.transition(model_id, ModelInstallState::Installed)?;
                }
                Ok(())
            })
            .expect("seed legacy models");
        let first = state
            .legacy_cleanup_preview(&root, "qwen3-asr-0.6b-4bit")
            .expect("preview");
        assert_eq!(
            first
                .removable
                .iter()
                .map(|item| item.model_id.as_str())
                .collect::<Vec<_>>(),
            ["qwen3-asr-0.6b"]
        );
        assert_eq!(first.retained[0].model_id, "qwen3-forced-aligner-0.6b");
        state
            .cleanup_legacy(&root, "qwen3-asr-0.6b-4bit")
            .expect("cleanup first legacy ASR");
        assert_eq!(
            state
                .snapshot(&root)
                .expect("snapshot")
                .into_iter()
                .find(|item| item.descriptor.id == "qwen3-forced-aligner-0.6b")
                .expect("legacy aligner")
                .installation
                .state,
            ModelInstallState::Installed
        );
        let second = state
            .legacy_cleanup_preview(&root, "qwen3-asr-1.7b-8bit")
            .expect("second preview");
        assert_eq!(
            second
                .removable
                .iter()
                .map(|item| item.model_id.as_str())
                .collect::<Vec<_>>(),
            ["qwen3-asr-1.7b", "qwen3-forced-aligner-0.6b"]
        );
        assert!(
            state
                .legacy_cleanup_preview(&root, "qwen3-asr-0.6b")
                .is_err()
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn noncommercial_folder_import_requires_explicit_confirmation() {
        let root = temp("speaker-import-license");
        let source = temp("speaker-import-license-source");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("config.yaml"), b"hello").expect("model file");
        let state = ModelState::with_catalog(speaker_catalog());

        let error = state
            .import_from_folder(&root, "research-model", &source, false)
            .expect_err("license confirmation required");
        assert!(error.contains("非商业"));
        assert_eq!(
            state
                .import_from_folder(&root, "research-model", &source, true)
                .expect("confirmed import")
                .state,
            ModelInstallState::Installed
        );

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(source).expect("cleanup source");
    }
}
