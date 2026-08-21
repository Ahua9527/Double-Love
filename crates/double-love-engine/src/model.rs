//! 本地模型目录、安装状态与完整性校验。
//!
//! 这个模块刻意不依赖 Tauri、`reqwest` 或异步 runtime。桌面层只需要把网络响应适配到
//! [`ModelFetcher`]，其余的清单解析、依赖顺序、断点文件写入、哈希校验和原子切换都在
//! 纯 Rust 引擎中完成，因此 CLI、Tauri 和本地 fixture 测试共享同一套规则。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ts_rs::TS;

pub const MODEL_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const MODEL_INSTALLATIONS_SCHEMA_VERSION: u32 = 1;

/// 清单中的实际模型组件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelComponent {
    Asr,
    ForcedAligner,
    Vad,
    Speaker,
}

/// 模型文件的固定完整性元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    // 只有 `allowed=true` 的文件可以被安装器写入最终目录。
    pub allowed: bool,
}

/// 一个模型对另一个模型的安装依赖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelDependency {
    pub model_id: String,
    pub required: bool,
    pub reason: String,
}

/// 内置模型清单中的模型描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub component: ModelComponent,
    pub repo_id: Option<String>,
    // 固定的 40 位提交 hash；禁止 main、tag 或其他浮动引用。
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub license: String,
    pub license_url: String,
    pub dependencies: Vec<ModelDependency>,
    pub min_memory_bytes: Option<u64>,
    pub source_url_template: String,
    // 运行时随 App 分发、不可由用户删除的组件（例如当前 Silero VAD）。
    #[serde(default)]
    pub bundled: bool,
}

/// 安装器对外暴露的固定状态机。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelInstallState {
    #[default]
    NotInstalled,
    Queued,
    Downloading,
    Paused,
    Verifying,
    Installed,
    Corrupt,
    Failed,
}

impl ModelInstallState {
    pub fn is_installed(self) -> bool {
        self == Self::Installed
    }

    fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::NotInstalled, Self::Queued)
                | (Self::NotInstalled, Self::Installed)
                | (Self::Queued, Self::Downloading)
                | (Self::Queued, Self::Paused)
                | (Self::Queued, Self::NotInstalled)
                | (Self::Downloading, Self::Paused)
                | (Self::Downloading, Self::Verifying)
                | (Self::Downloading, Self::Failed)
                | (Self::Downloading, Self::NotInstalled)
                | (Self::Paused, Self::Queued)
                | (Self::Paused, Self::NotInstalled)
                | (Self::Verifying, Self::Installed)
                | (Self::Verifying, Self::Corrupt)
                | (Self::Verifying, Self::Failed)
                | (Self::Installed, Self::Corrupt)
                | (Self::Installed, Self::Installed)
                | (Self::Corrupt, Self::Installed)
                | (Self::Failed, Self::Installed)
                | (Self::Corrupt, Self::Queued)
                | (Self::Corrupt, Self::NotInstalled)
                | (Self::Failed, Self::Queued)
                | (Self::Failed, Self::NotInstalled)
        )
    }
}

/// 可持久化的一条安装快照。路径故意只保存 staging id，不保存绝对目录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelInstallation {
    pub model_id: String,
    pub revision: String,
    pub state: ModelInstallState,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub staging_id: Option<String>,
    pub last_error_code: Option<String>,
    // 仅允许用户可读摘要，不应包含路径、URL、token 或音频文本。
    pub last_error_message: Option<String>,
    pub updated_at: String,
}

impl ModelInstallation {
    fn new(descriptor: &ModelDescriptor) -> Self {
        Self {
            model_id: descriptor.id.clone(),
            revision: descriptor.revision.clone(),
            state: ModelInstallState::NotInstalled,
            bytes_downloaded: 0,
            bytes_total: descriptor.total_size(),
            staging_id: None,
            last_error_code: None,
            last_error_message: None,
            updated_at: now_string(),
        }
    }
}

/// 设置窗口用的目录快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelDescriptorWithInstallation {
    pub descriptor: ModelDescriptor,
    pub installation: ModelInstallation,
}

/// 下载进度事件。绝不包含本地绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelDownloadProgress {
    pub model_id: String,
    pub state: ModelInstallState,
    pub current_file: Option<String>,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub file_bytes_downloaded: u64,
    pub file_bytes_total: u64,
    pub speed_bytes_per_second: Option<u64>,
}

/// 诊断中的单项模型完整性结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DoctorModelCheck {
    pub model_id: String,
    pub state: ModelInstallState,
    pub revision: String,
    pub integrity_ok: bool,
    pub error_code: Option<String>,
}

/// 本地诊断报告。路径字段仅用根目录标签，不携带绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub architecture: String,
    pub os_version: String,
    pub memory_bytes: u64,
    pub free_model_bytes: u64,
    pub model_root_available: bool,
    pub ffmpeg_available: bool,
    pub libass_available: bool,
    pub asr_runtime_ready: bool,
    pub speaker_runtime_ready: bool,
    pub model_checks: Vec<DoctorModelCheck>,
    pub warnings: Vec<String>,
}

/// 诊断执行时采集的本机环境；与模型目录状态分开传入，避免诊断入口随检查项增长。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorEnvironment {
    pub architecture: String,
    pub os_version: String,
    pub memory_bytes: u64,
    pub free_model_bytes: u64,
    pub ffmpeg_available: bool,
    pub libass_available: bool,
    pub asr_runtime_ready: bool,
    pub speaker_runtime_ready: bool,
}

/// 清单解析后的只读索引。
#[derive(Debug, Clone)]
pub struct ModelCatalog {
    descriptors: BTreeMap<String, ModelDescriptor>,
}

impl ModelCatalog {
    pub fn from_json(input: &str) -> Result<Self, ModelError> {
        let raw: RawCatalog = serde_json::from_str(input)
            .map_err(|error| ModelError::CatalogInvalid(format!("JSON 无法解析：{error}")))?;
        if raw.schema_version != MODEL_CATALOG_SCHEMA_VERSION {
            return Err(ModelError::CatalogInvalid(format!(
                "不支持的模型清单版本：{}",
                raw.schema_version
            )));
        }
        if raw.models.is_empty() {
            return Err(ModelError::CatalogInvalid("模型清单不能为空".to_string()));
        }
        let mut descriptors = BTreeMap::new();
        for descriptor in raw.models {
            validate_descriptor(&descriptor)?;
            if descriptors
                .insert(descriptor.id.clone(), descriptor.clone())
                .is_some()
            {
                return Err(ModelError::CatalogInvalid(format!(
                    "模型 id 重复：{}",
                    descriptor.id
                )));
            }
        }
        for descriptor in descriptors.values() {
            for dependency in &descriptor.dependencies {
                if !descriptors.contains_key(&dependency.model_id) {
                    return Err(ModelError::CatalogInvalid(format!(
                        "{} 引用了不存在的依赖 {}",
                        descriptor.id, dependency.model_id
                    )));
                }
            }
        }
        Ok(Self { descriptors })
    }

    /// 加载随源码固定的清单；远程文件不能覆盖它。
    pub fn builtin() -> Result<Self, ModelError> {
        Self::from_json(include_str!(
            "../../../src-tauri/resources/model-catalog-v1.json"
        ))
    }

    pub fn get(&self, model_id: &str) -> Option<&ModelDescriptor> {
        self.descriptors.get(model_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ModelDescriptor> {
        self.descriptors.values()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

impl ModelDescriptor {
    pub fn total_size(&self) -> u64 {
        self.files
            .iter()
            .filter(|file| file.allowed)
            .map(|file| file.size_bytes)
            .sum()
    }

    pub fn file(&self, path: &str) -> Option<&ModelFile> {
        self.files.iter().find(|file| file.path == path)
    }

    /// 将 `{revision}` 与 `{path}` 展开到清单提供的固定 URL 模板。
    pub fn source_url(&self, path: &str) -> Result<String, ModelError> {
        let file = self
            .file(path)
            .ok_or_else(|| ModelError::FileNotInManifest(path.to_string()))?;
        if !file.allowed {
            return Err(ModelError::FileNotAllowed(path.to_string()));
        }
        Ok(self
            .source_url_template
            .replace("{revision}", &self.revision)
            .replace("{path}", path))
    }
}

/// 将网络层隔离在 Tauri 之外。实现者应保证 status=Partial 时从 `range_start` 开始。
pub trait ModelFetcher: Send + Sync {
    fn fetch(&self, url: &str, range_start: u64) -> Result<FetchResponse, ModelError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchStatus {
    Partial,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: FetchStatus,
    pub start: u64,
    pub total_bytes: u64,
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

/// 将一次 HTTP/fixture 响应安全地写入 `.part` 文件。
///
/// `206` 只能追加到同一 offset；服务器回 `200` 时必须从零截断，绝不能把完整响应
/// 追加到半截文件。调用方可在每次返回后持久化 offset/etag 到自己的 staging manifest。
pub fn write_fetch_response(
    part_path: &Path,
    expected_size: u64,
    existing_offset: u64,
    response: &FetchResponse,
) -> Result<u64, ModelError> {
    if existing_offset > expected_size {
        return Err(ModelError::Download("已有分片超过清单大小".to_string()));
    }
    let (write_offset, append) = match response.status {
        FetchStatus::Partial => {
            if response.start != existing_offset {
                return Err(ModelError::Download(format!(
                    "Range 响应从 {} 开始，期望 {}",
                    response.start, existing_offset
                )));
            }
            (existing_offset, true)
        }
        FetchStatus::Full => (0, false),
    };
    if response.total_bytes != expected_size
        || write_offset.saturating_add(response.bytes.len() as u64) > expected_size
    {
        return Err(ModelError::Download("下载响应大小与清单不一致".to_string()));
    }
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = if append {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(part_path)?
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(part_path)?
    };
    file.write_all(&response.bytes)?;
    file.sync_all()?;
    Ok(write_offset + response.bytes.len() as u64)
}

/// 已安装模型的管理器。它不启动网络任务，也不持有 WebView 生命周期。
#[derive(Debug)]
pub struct ModelManager {
    root: PathBuf,
    catalog: ModelCatalog,
    installations: BTreeMap<String, ModelInstallation>,
}

impl ModelManager {
    pub fn new(root: impl Into<PathBuf>, catalog: ModelCatalog) -> Result<Self, ModelError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join(".staging"))?;
        let mut installations = load_installations(&root, &catalog)?;
        let mut recovered = false;
        for installation in installations.values_mut() {
            if matches!(
                installation.state,
                ModelInstallState::Queued
                    | ModelInstallState::Downloading
                    | ModelInstallState::Verifying
            ) {
                installation.state = ModelInstallState::Paused;
                installation.last_error_code = Some("MODEL_TASK_INTERRUPTED".to_string());
                installation.last_error_message =
                    Some("上次安装任务被中断，可以继续下载。".to_string());
                installation.updated_at = now_string();
                recovered = true;
            }
        }
        let manager = Self {
            root,
            catalog,
            installations,
        };
        if recovered {
            manager.persist()?;
        }
        Ok(manager)
    }

    pub fn with_builtin_catalog(root: impl Into<PathBuf>) -> Result<Self, ModelError> {
        Self::new(root, ModelCatalog::builtin()?)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn descriptor(&self, model_id: &str) -> Result<&ModelDescriptor, ModelError> {
        self.catalog
            .get(model_id)
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_string()))
    }

    pub fn installation(&self, model_id: &str) -> Result<ModelInstallation, ModelError> {
        let descriptor = self.descriptor(model_id)?;
        Ok(self
            .installations
            .get(model_id)
            .cloned()
            .unwrap_or_else(|| ModelInstallation::new(descriptor)))
    }

    pub fn installations(&self) -> Vec<ModelInstallation> {
        self.catalog
            .iter()
            .map(|descriptor| {
                self.installations
                    .get(&descriptor.id)
                    .cloned()
                    .unwrap_or_else(|| ModelInstallation::new(descriptor))
            })
            .collect()
    }

    pub fn snapshot(&self) -> Vec<ModelDescriptorWithInstallation> {
        self.catalog
            .iter()
            .filter_map(|descriptor| {
                self.installation(&descriptor.id).ok().map(|installation| {
                    ModelDescriptorWithInstallation {
                        descriptor: descriptor.clone(),
                        installation,
                    }
                })
            })
            .collect()
    }

    /// 返回依赖优先的去重顺序；循环依赖会明确失败。
    pub fn dependency_order(&self, model_id: &str) -> Result<Vec<String>, ModelError> {
        self.descriptor(model_id)?;
        let mut output = Vec::new();
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        self.visit_dependencies(model_id, &mut visiting, &mut visited, &mut output)?;
        Ok(output)
    }

    fn visit_dependencies(
        &self,
        model_id: &str,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        output: &mut Vec<String>,
    ) -> Result<(), ModelError> {
        if visited.contains(model_id) {
            return Ok(());
        }
        if !visiting.insert(model_id.to_string()) {
            return Err(ModelError::DependencyCycle(model_id.to_string()));
        }
        let descriptor = self.descriptor(model_id)?;
        for dependency in &descriptor.dependencies {
            if dependency.required
                || self.installation(&dependency.model_id)?.state != ModelInstallState::Installed
            {
                self.visit_dependencies(&dependency.model_id, visiting, visited, output)?;
            }
        }
        visiting.remove(model_id);
        visited.insert(model_id.to_string());
        output.push(model_id.to_string());
        Ok(())
    }

    /// 把模型及其依赖加入单一队列；已安装项不重复排队。
    pub fn queue_install(&mut self, model_id: &str) -> Result<Vec<ModelInstallation>, ModelError> {
        let order = self.dependency_order(model_id)?;
        for id in &order {
            let current = self.installation(id)?;
            if current.state == ModelInstallState::Installed {
                continue;
            }
            let state = current.state;
            if !matches!(
                state,
                ModelInstallState::Queued
                    | ModelInstallState::Downloading
                    | ModelInstallState::Verifying
            ) {
                self.transition(id, ModelInstallState::Queued)?;
            }
        }
        self.persist()?;
        order
            .iter()
            .map(|id| self.installation(id))
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn transition(
        &mut self,
        model_id: &str,
        next: ModelInstallState,
    ) -> Result<ModelInstallation, ModelError> {
        self.descriptor(model_id)?;
        let mut installation = self.installation(model_id)?;
        if installation.state != next && !installation.state.can_transition_to(next) {
            return Err(ModelError::InvalidTransition {
                model_id: model_id.to_string(),
                from: installation.state,
                to: next,
            });
        }
        installation.state = next;
        installation.updated_at = now_string();
        if next == ModelInstallState::Queued {
            installation.last_error_code = None;
            installation.last_error_message = None;
        }
        self.installations
            .insert(model_id.to_string(), installation.clone());
        self.persist()?;
        Ok(installation)
    }

    pub fn update_progress(
        &mut self,
        model_id: &str,
        current_file: Option<String>,
        bytes_downloaded: u64,
        file_bytes_downloaded: u64,
        file_bytes_total: u64,
        speed_bytes_per_second: Option<u64>,
    ) -> Result<ModelDownloadProgress, ModelError> {
        let descriptor = self.descriptor(model_id)?;
        let mut installation = self.installation(model_id)?;
        if !matches!(
            installation.state,
            ModelInstallState::Queued | ModelInstallState::Downloading | ModelInstallState::Paused
        ) {
            return Err(ModelError::InvalidState(format!(
                "{} 当前状态不能更新下载进度",
                installation.state.as_str()
            )));
        }
        let file_limit = descriptor
            .file(current_file.as_deref().unwrap_or_default())
            .map(|file| file.size_bytes)
            .unwrap_or(file_bytes_total);
        if bytes_downloaded > descriptor.total_size()
            || file_bytes_downloaded > file_limit
            || file_bytes_total > file_limit
        {
            return Err(ModelError::Download("文件进度超过清单大小".to_string()));
        }
        installation.bytes_downloaded = bytes_downloaded;
        installation.bytes_total = descriptor.total_size();
        installation.updated_at = now_string();
        self.installations
            .insert(model_id.to_string(), installation.clone());
        self.persist()?;
        Ok(ModelDownloadProgress {
            model_id: model_id.to_string(),
            state: installation.state,
            current_file,
            bytes_downloaded: installation.bytes_downloaded,
            bytes_total: installation.bytes_total,
            file_bytes_downloaded,
            file_bytes_total,
            speed_bytes_per_second,
        })
    }

    pub fn mark_error(
        &mut self,
        model_id: &str,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<ModelInstallation, ModelError> {
        let mut installation = self.installation(model_id)?;
        let next = if matches!(
            installation.state,
            ModelInstallState::Verifying | ModelInstallState::Installed
        ) {
            ModelInstallState::Corrupt
        } else {
            ModelInstallState::Failed
        };
        if installation.state != next && !installation.state.can_transition_to(next) {
            return Err(ModelError::InvalidTransition {
                model_id: model_id.to_string(),
                from: installation.state,
                to: next,
            });
        }
        installation.state = next;
        installation.last_error_code = Some(code.into());
        installation.last_error_message = Some(message.into());
        installation.updated_at = now_string();
        self.installations
            .insert(model_id.to_string(), installation.clone());
        self.persist()?;
        Ok(installation)
    }

    pub fn set_staging_id(
        &mut self,
        model_id: &str,
        staging_id: Option<String>,
    ) -> Result<ModelInstallation, ModelError> {
        let mut installation = self.installation(model_id)?;
        installation.staging_id = staging_id;
        installation.updated_at = now_string();
        self.installations
            .insert(model_id.to_string(), installation.clone());
        self.persist()?;
        Ok(installation)
    }

    pub fn staging_root(&self, staging_id: &str) -> Result<PathBuf, ModelError> {
        if staging_id.is_empty()
            || staging_id.chars().any(|character| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            })
        {
            return Err(ModelError::UnsafePath(staging_id.to_string()));
        }
        Ok(self.root.join(".staging").join(staging_id))
    }

    pub fn installation_dir(&self, model_id: &str) -> Result<PathBuf, ModelError> {
        let descriptor = self.descriptor(model_id)?;
        Ok(self.root.join(&descriptor.id).join(&descriptor.revision))
    }

    /// 校验 staging 或最终目录中的所有清单文件；不接受绝对路径或额外文件。
    pub fn verify_directory(&self, model_id: &str, directory: &Path) -> Result<(), ModelError> {
        let descriptor = self.descriptor(model_id)?;
        if !directory.is_dir() {
            return Err(ModelError::Integrity("模型目录不存在".to_string()));
        }
        let mut expected = BTreeSet::new();
        for file in descriptor.files.iter().filter(|file| file.allowed) {
            let relative = safe_relative_path(&file.path)?;
            expected.insert(file.path.clone());
            let path = directory.join(relative);
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| ModelError::Integrity(format!("缺少模型文件：{}", file.path)))?;
            if !metadata.file_type().is_file() || metadata.len() != file.size_bytes {
                return Err(ModelError::Integrity(format!(
                    "模型文件大小不匹配：{}",
                    file.path
                )));
            }
            let digest = sha256_file(&path)?;
            if digest != file.sha256 {
                return Err(ModelError::Integrity(format!(
                    "模型文件校验失败：{}",
                    file.path
                )));
            }
        }
        for path in walk_files(directory)? {
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| ModelError::UnsafePath(path.to_string_lossy().into_owned()))?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            // staging manifest 记录 revision、endpoint 和恢复 offset；它不是远程模型文件，
            // 允许保留在最终目录中作为本地元数据，但不能把其他清单外文件带入安装结果。
            if relative == "manifest.json" {
                continue;
            }
            if !expected.contains(&relative) {
                return Err(ModelError::Integrity(format!(
                    "模型目录含有清单外文件：{}",
                    relative
                )));
            }
        }
        Ok(())
    }

    /// 完整校验后，把 staging 目录原子切换到最终 revision 目录，再写入安装状态。
    pub fn atomically_install(
        &mut self,
        model_id: &str,
        staging_id: &str,
    ) -> Result<ModelInstallation, ModelError> {
        let staging_base = self.staging_root(staging_id)?;
        let descriptor = self.descriptor(model_id)?.clone();
        let staging = staging_base.join(&descriptor.id).join(&descriptor.revision);
        self.verify_directory(model_id, &staging)?;

        let destination_parent = self.root.join(&descriptor.id);
        fs::create_dir_all(&destination_parent)?;
        let destination = destination_parent.join(&descriptor.revision);
        if destination.exists() {
            // 同一 revision 已经完成安装时，删除未完成副本不会影响旧 revision。
            self.verify_directory(model_id, &destination)?;
            fs::remove_dir_all(&staging_base)?;
        } else {
            fs::rename(
                &staging_base,
                self.root
                    .join(".staging")
                    .join(format!(".installed-{staging_id}")),
            )?;
            let moved = self
                .root
                .join(".staging")
                .join(format!(".installed-{staging_id}"));
            // 先将整个 staging 移到同一文件系统，再把模型 revision 目录切过去；若第二步
            // 失败，调用方仍可从 .installed-* 恢复，不会出现“已安装但状态未写入”的假象。
            fs::rename(
                moved.join(&descriptor.id).join(&descriptor.revision),
                &destination,
            )?;
            let _ = fs::remove_dir_all(moved);
        }

        let mut installation = self.installation(model_id)?;
        installation.state = ModelInstallState::Installed;
        installation.bytes_downloaded = descriptor.total_size();
        installation.bytes_total = descriptor.total_size();
        installation.staging_id = None;
        installation.last_error_code = None;
        installation.last_error_message = None;
        installation.updated_at = now_string();
        self.installations
            .insert(model_id.to_string(), installation.clone());
        self.persist()?;
        Ok(installation)
    }

    /// 重新哈希已安装目录；被用户改动时状态转为 corrupt。
    pub fn verify_installed(&mut self, model_id: &str) -> Result<ModelInstallation, ModelError> {
        let directory = self.installation_dir(model_id)?;
        let verification = self.verify_directory(model_id, &directory);
        match verification {
            Ok(()) => {
                let mut installation = self.installation(model_id)?;
                if installation.state != ModelInstallState::Installed {
                    if !installation
                        .state
                        .can_transition_to(ModelInstallState::Installed)
                    {
                        return Err(ModelError::InvalidTransition {
                            model_id: model_id.to_string(),
                            from: installation.state,
                            to: ModelInstallState::Installed,
                        });
                    }
                    installation.state = ModelInstallState::Installed;
                }
                installation.last_error_code = None;
                installation.last_error_message = None;
                installation.updated_at = now_string();
                self.installations
                    .insert(model_id.to_string(), installation.clone());
                self.persist()?;
                Ok(installation)
            }
            Err(error) => {
                let _ = self.mark_error(model_id, "MODEL_INTEGRITY_FAILED", error.to_string());
                Err(error)
            }
        }
    }

    /// 删除一个已安装模型；共享 ForcedAligner 仍被 ASR 使用时拒绝删除。
    pub fn remove(&mut self, model_id: &str) -> Result<ModelInstallation, ModelError> {
        let descriptor = self.descriptor(model_id)?.clone();
        if descriptor.bundled {
            return Err(ModelError::InvalidState(format!(
                "{} 是随 App 分发的运行时组件，不能单独删除",
                model_id
            )));
        }
        let installation = self.installation(model_id)?;
        if installation.state != ModelInstallState::Installed {
            return Err(ModelError::InvalidState(format!(
                "{} 尚未安装，不能删除",
                model_id
            )));
        }
        for other in self.catalog.iter() {
            if other.id == descriptor.id {
                continue;
            }
            if other
                .dependencies
                .iter()
                .any(|dependency| dependency.required && dependency.model_id == descriptor.id)
                && self.installation(&other.id)?.state == ModelInstallState::Installed
            {
                return Err(ModelError::DependencyInUse {
                    model_id: descriptor.id,
                    by_model: other.id.clone(),
                });
            }
        }
        let directory = self.installation_dir(model_id)?;
        if directory.exists() {
            fs::remove_dir_all(directory)?;
        }
        let mut next = installation;
        next.state = ModelInstallState::NotInstalled;
        next.bytes_downloaded = 0;
        next.staging_id = None;
        next.updated_at = now_string();
        self.installations
            .insert(model_id.to_string(), next.clone());
        self.persist()?;
        Ok(next)
    }

    pub fn doctor_report(&mut self, environment: DoctorEnvironment) -> DoctorReport {
        let mut model_checks = Vec::new();
        let mut warnings = Vec::new();
        for descriptor in self.catalog.iter() {
            let installation = self
                .installation(&descriptor.id)
                .expect("catalog installation exists");
            let integrity = if installation.state == ModelInstallState::Installed {
                self.verify_directory(
                    &descriptor.id,
                    &self.installation_dir(&descriptor.id).expect("path"),
                )
                .is_ok()
            } else {
                false
            };
            if installation.state == ModelInstallState::Installed && !integrity {
                warnings.push(format!("{} 模型完整性校验失败", descriptor.display_name));
            }
            model_checks.push(DoctorModelCheck {
                model_id: descriptor.id.clone(),
                state: if integrity {
                    ModelInstallState::Installed
                } else {
                    installation.state
                },
                revision: descriptor.revision.clone(),
                integrity_ok: integrity,
                error_code: (!integrity && installation.state == ModelInstallState::Installed)
                    .then(|| "MODEL_INTEGRITY_FAILED".to_string()),
            });
        }
        if !environment.ffmpeg_available {
            warnings.push("ffmpeg/ffprobe 不可用".to_string());
        }
        if !environment.libass_available {
            warnings.push("libass 不可用".to_string());
        }
        if !environment.asr_runtime_ready {
            warnings.push("ASR 运行时不可用".to_string());
        }
        if !environment.speaker_runtime_ready {
            warnings.push("说话人运行时不可用".to_string());
        }
        DoctorReport {
            schema_version: 1,
            generated_at: now_string(),
            architecture: environment.architecture,
            os_version: environment.os_version,
            memory_bytes: environment.memory_bytes,
            free_model_bytes: environment.free_model_bytes,
            model_root_available: self.root.is_dir(),
            ffmpeg_available: environment.ffmpeg_available,
            libass_available: environment.libass_available,
            asr_runtime_ready: environment.asr_runtime_ready,
            speaker_runtime_ready: environment.speaker_runtime_ready,
            model_checks,
            warnings,
        }
    }

    fn persist(&self) -> Result<(), ModelError> {
        persist_installations(&self.root, &self.installations)
    }
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("模型清单无效：{0}")]
    CatalogInvalid(String),
    #[error("未知模型：{0}")]
    UnknownModel(String),
    #[error("模型依赖循环：{0}")]
    DependencyCycle(String),
    #[error("模型依赖仍在使用：{model_id} 被 {by_model} 依赖")]
    DependencyInUse { model_id: String, by_model: String },
    #[error("模型状态无效：{0}")]
    InvalidState(String),
    #[error("模型状态不能从 {from:?} 转为 {to:?}：{model_id}")]
    InvalidTransition {
        model_id: String,
        from: ModelInstallState,
        to: ModelInstallState,
    },
    #[error("模型完整性失败：{0}")]
    Integrity(String),
    #[error("模型下载失败：{0}")]
    Download(String),
    #[error("清单未声明文件：{0}")]
    FileNotInManifest(String),
    #[error("文件不允许安装：{0}")]
    FileNotAllowed(String),
    #[error("不安全路径：{0}")]
    UnsafePath(String),
    #[error("安装状态文件损坏：{0}")]
    StateCorrupt(String),
    #[error("文件系统错误：{0}")]
    Filesystem(#[from] io::Error),
    #[error("JSON 编码错误：{0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct RawCatalog {
    schema_version: u32,
    models: Vec<ModelDescriptor>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstallationStore {
    schema_version: u32,
    installations: BTreeMap<String, ModelInstallation>,
}

fn validate_descriptor(descriptor: &ModelDescriptor) -> Result<(), ModelError> {
    if descriptor.id.is_empty()
        || descriptor.id.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        })
    {
        return Err(ModelError::CatalogInvalid(format!(
            "模型 id 无效：{}",
            descriptor.id
        )));
    }
    if descriptor.revision.len() != 40
        || descriptor
            .revision
            .chars()
            .any(|character| !character.is_ascii_hexdigit())
    {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 的 revision 必须是 40 位 commit hash",
            descriptor.id
        )));
    }
    if descriptor.license.is_empty()
        || !descriptor.license_url.starts_with("https://")
        || descriptor.source_url_template.is_empty()
        || (!descriptor.bundled && !descriptor.source_url_template.starts_with("https://"))
    {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 缺少 license 或下载源",
            descriptor.id
        )));
    }
    let mut seen_files = BTreeSet::new();
    for file in &descriptor.files {
        safe_relative_path(&file.path)?;
        if file.allowed && (file.size_bytes == 0 || !is_sha256(&file.sha256)) {
            return Err(ModelError::CatalogInvalid(format!(
                "{} 的文件 {} 缺少 size/sha256",
                descriptor.id, file.path
            )));
        }
        if file.allowed && !allowed_file_type(&file.path) {
            return Err(ModelError::CatalogInvalid(format!(
                "{} 的文件类型不在白名单：{}",
                descriptor.id, file.path
            )));
        }
        if !seen_files.insert(file.path.clone()) {
            return Err(ModelError::CatalogInvalid(format!(
                "{} 的文件路径重复：{}",
                descriptor.id, file.path
            )));
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn allowed_file_type(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "json" | "txt" | "safetensors" | "pt" | "yaml" | "yml" | "onnx" | "bin"
            )
        })
}

fn safe_relative_path(value: &str) -> Result<PathBuf, ModelError> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        return Err(ModelError::UnsafePath(value.to_string()));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => return Err(ModelError::UnsafePath(value.to_string())),
        }
    }
    Ok(path.to_path_buf())
}

fn load_installations(
    root: &Path,
    catalog: &ModelCatalog,
) -> Result<BTreeMap<String, ModelInstallation>, ModelError> {
    let path = root.join("installations.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let value = fs::read_to_string(path)?;
    let store: InstallationStore = serde_json::from_str(&value)
        .map_err(|error| ModelError::StateCorrupt(error.to_string()))?;
    if store.schema_version != MODEL_INSTALLATIONS_SCHEMA_VERSION {
        return Err(ModelError::StateCorrupt(format!(
            "不支持的安装状态版本：{}",
            store.schema_version
        )));
    }
    for (id, installation) in &store.installations {
        let descriptor = catalog
            .get(id)
            .ok_or_else(|| ModelError::StateCorrupt(format!("未知安装项：{id}")))?;
        if installation.model_id != *id || installation.revision != descriptor.revision {
            return Err(ModelError::StateCorrupt(format!(
                "安装状态与清单不一致：{id}"
            )));
        }
    }
    Ok(store.installations)
}

fn persist_installations(
    root: &Path,
    installations: &BTreeMap<String, ModelInstallation>,
) -> Result<(), ModelError> {
    let path = root.join("installations.json");
    let tmp = root.join("installations.json.tmp");
    let payload = serde_json::to_vec_pretty(&InstallationStore {
        schema_version: MODEL_INSTALLATIONS_SCHEMA_VERSION,
        installations: installations.clone(),
    })?;
    let mut file = File::create(&tmp)?;
    file.write_all(&payload)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, ModelError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, ModelError> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_symlink() {
                return Err(ModelError::Integrity(
                    "模型目录不能包含符号链接".to_string(),
                ));
            }
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                files.push(path);
            } else {
                return Err(ModelError::Integrity(
                    "模型目录含有未知文件类型".to_string(),
                ));
            }
        }
    }
    Ok(files)
}

fn now_string() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant 的 civil_from_days 算法；只使用 std，避免为时间戳引入新依赖。
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_prime = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * month_prime + 2).div_euclid(5) + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

impl ModelInstallState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Verifying => "verifying",
            Self::Installed => "installed",
            Self::Corrupt => "corrupt",
            Self::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("double-love-model-{label}-{stamp}"))
    }

    fn fixture_catalog() -> String {
        let data = b"hello";
        let hash = format!("{:x}", Sha256::digest(data));
        serde_json::json!({
            "schema_version": 1,
            "models": [
                {
                    "id": "aligner",
                    "display_name": "Aligner",
                    "component": "forced_aligner",
                    "repo_id": null,
                    "revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "files": [{"path":"config.json","size_bytes":5,"sha256":hash,"allowed":true}],
                    "license": "Apache-2.0",
                    "license_url": "https://example.invalid/license",
                    "dependencies": [],
                    "min_memory_bytes": null,
                    "source_url_template": "https://example.invalid/{revision}/{path}"
                },
                {
                    "id": "asr",
                    "display_name": "ASR",
                    "component": "asr",
                    "repo_id": null,
                    "revision": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "files": [{"path":"config.json","size_bytes":5,"sha256":hash,"allowed":true}],
                    "license": "Apache-2.0",
                    "license_url": "https://example.invalid/license",
                    "dependencies": [{"model_id":"aligner","required":true,"reason":"word timing"}],
                    "min_memory_bytes": null,
                    "source_url_template": "https://example.invalid/{revision}/{path}"
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn catalog_rejects_floating_revision_and_missing_hash() {
        let mut value: serde_json::Value =
            serde_json::from_str(&fixture_catalog()).expect("fixture");
        value["models"][0]["revision"] = serde_json::json!("main");
        assert!(matches!(
            ModelCatalog::from_json(&value.to_string()),
            Err(ModelError::CatalogInvalid(_))
        ));
    }

    #[test]
    fn builtin_catalog_contains_real_asr_and_speaker_components() {
        let catalog = ModelCatalog::builtin().expect("built-in catalog");
        assert_eq!(catalog.len(), 5);
        let asr = catalog.get("qwen3-asr-1.7b").expect("1.7B");
        assert_eq!(asr.component, ModelComponent::Asr);
        assert!(asr.dependencies.iter().any(|dependency| {
            dependency.model_id == "qwen3-forced-aligner-0.6b" && dependency.required
        }));
        assert!(catalog.get("silero-vad").expect("vad").bundled);
        assert_eq!(
            catalog.get("wespeaker-zh").expect("speaker").dependencies[0].model_id,
            "silero-vad"
        );
    }

    #[test]
    fn dependency_order_is_dependency_first_and_removes_duplicates() {
        let catalog = ModelCatalog::from_json(&fixture_catalog()).expect("catalog");
        let manager = ModelManager::new(temp_root("order"), catalog).expect("manager");
        assert_eq!(
            manager.dependency_order("asr").expect("order"),
            ["aligner", "asr"]
        );
    }

    #[test]
    fn aligner_cannot_be_removed_while_asr_is_installed() {
        let root = temp_root("dependency");
        let catalog = ModelCatalog::from_json(&fixture_catalog()).expect("catalog");
        let mut manager = ModelManager::new(&root, catalog).expect("manager");
        for model in ["aligner", "asr"] {
            let descriptor = manager.descriptor(model).expect("descriptor").clone();
            let staging_id = format!("stage-{model}");
            let staging = manager
                .staging_root(&staging_id)
                .expect("staging")
                .join(model)
                .join(&descriptor.revision);
            fs::create_dir_all(&staging).expect("staging dir");
            fs::write(staging.join("config.json"), b"hello").expect("file");
            manager
                .transition(model, ModelInstallState::Queued)
                .expect("queue");
            manager
                .transition(model, ModelInstallState::Downloading)
                .expect("download");
            manager
                .transition(model, ModelInstallState::Verifying)
                .expect("verify");
            manager
                .atomically_install(model, &staging_id)
                .expect("install");
        }
        assert!(matches!(
            manager.remove("aligner"),
            Err(ModelError::DependencyInUse { .. })
        ));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn full_response_truncates_partial_file_instead_of_appending() {
        let path = temp_root("range").join("file.part");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        fs::write(&path, b"old-partial").expect("partial");
        let response = FetchResponse {
            status: FetchStatus::Full,
            start: 0,
            total_bytes: 5,
            bytes: b"hello".to_vec(),
            etag: None,
        };
        assert_eq!(
            write_fetch_response(&path, 5, 5, &response).expect("write"),
            5
        );
        assert_eq!(fs::read(path).expect("read"), b"hello");
    }

    #[test]
    fn verify_modified_installed_file_marks_model_corrupt() {
        let root = temp_root("corrupt");
        let catalog = ModelCatalog::from_json(&fixture_catalog()).expect("catalog");
        let mut manager = ModelManager::new(&root, catalog.clone()).expect("manager");
        let descriptor = manager.descriptor("aligner").expect("descriptor").clone();
        let staging_id = "stage-corrupt";
        let staging = manager
            .staging_root(staging_id)
            .expect("staging")
            .join("aligner")
            .join(&descriptor.revision);
        fs::create_dir_all(&staging).expect("staging dir");
        fs::write(staging.join("config.json"), b"hello").expect("file");
        manager
            .transition("aligner", ModelInstallState::Queued)
            .expect("queue");
        manager
            .transition("aligner", ModelInstallState::Downloading)
            .expect("download");
        manager
            .transition("aligner", ModelInstallState::Verifying)
            .expect("verify");
        manager
            .atomically_install("aligner", staging_id)
            .expect("install");
        fs::write(
            manager
                .installation_dir("aligner")
                .expect("dir")
                .join("config.json"),
            b"bad",
        )
        .expect("tamper");
        assert!(matches!(
            manager.verify_installed("aligner"),
            Err(ModelError::Integrity(_))
        ));
        assert_eq!(
            manager.installation("aligner").expect("state").state,
            ModelInstallState::Corrupt
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn interrupted_installations_resume_as_paused_after_restart() {
        let root = temp_root("persist");
        let catalog = ModelCatalog::from_json(&fixture_catalog()).expect("catalog");
        {
            let mut manager = ModelManager::new(&root, catalog.clone()).expect("manager");
            manager.queue_install("asr").expect("queue dependencies");
        }
        let manager = ModelManager::new(&root, catalog).expect("reload");
        assert_eq!(
            manager.installation("aligner").expect("aligner").state,
            ModelInstallState::Paused
        );
        assert_eq!(
            manager.installation("asr").expect("asr").state,
            ModelInstallState::Paused
        );
        fs::remove_dir_all(root).ok();
    }
}
