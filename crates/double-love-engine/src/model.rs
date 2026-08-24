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
use zip::ZipArchive;

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

/// 模型在设置页中的呈现层级。依赖、随应用组件和旧权重仍保留给运行时与诊断，
/// 但不会伪装成用户可单独管理的主模型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelUiRole {
    #[default]
    Primary,
    Dependency,
    Bundled,
    Legacy,
}

/// 内置模型的固定下载来源。旧清单默认视为 Hugging Face，以保持状态文件兼容；
/// 归档模型使用 `Official`，并仍须固定 SHA-256。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ModelDownloadSource {
    Modelscope,
    #[default]
    Huggingface,
    Bundled,
    Official,
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

/// 单个可验证归档的下载说明。归档本身和解压后的每个运行时文件都必须通过哈希校验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelArchive {
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
    // 官方归档有时会包一层目录；只允许移除这一层固定、安全的前缀。
    #[serde(default)]
    pub strip_prefix: Option<String>,
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
    #[serde(default)]
    pub ui_role: ModelUiRole,
    pub repo_id: Option<String>,
    #[serde(default)]
    pub download_source: ModelDownloadSource,
    // 本地安装 revision 保持 40 位不可变提交，确保既有安装目录和离线加载兼容。
    // ModelScope 下载也只能接收这个固定提交；每文件 size + SHA-256 是第二道边界。
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub license: String,
    pub license_url: String,
    pub dependencies: Vec<ModelDependency>,
    pub min_memory_bytes: Option<u64>,
    pub source_url_template: String,
    // 新的 MLX 描述符替代的旧模型 ID。仅用于迁移时计算占用空间和清理，
    // 绝不能把旧模型重新作为推理候选。
    #[serde(default)]
    pub replaces_model_id: Option<String>,
    // 有值时安装器只下载该 ZIP，再按 `files` 的白名单安全解压。
    #[serde(default)]
    pub archive: Option<ModelArchive>,
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
                | (Self::Queued, Self::Failed)
                | (Self::Queued, Self::NotInstalled)
                | (Self::Downloading, Self::Paused)
                | (Self::Downloading, Self::Verifying)
                | (Self::Downloading, Self::Failed)
                | (Self::Downloading, Self::NotInstalled)
                | (Self::Paused, Self::Queued)
                | (Self::Paused, Self::NotInstalled)
                | (Self::Verifying, Self::NotInstalled)
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

/// 一个旧模型清理候选。路径故意不出现在该类型中；设置页只需要名称、状态和
/// 可释放空间，实际删除始终由受管模型根中的 `ModelManager` 执行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LegacyModelCleanupItem {
    pub model_id: String,
    pub display_name: String,
    pub bytes_to_free: u64,
    pub reason: Option<String>,
}

/// 当前 MLX 模型安装后可安全清理的旧版本预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LegacyModelCleanupPreview {
    pub target_model_id: String,
    pub bytes_to_free: u64,
    pub removable: Vec<LegacyModelCleanupItem>,
    pub retained: Vec<LegacyModelCleanupItem>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelQueueState {
    Active,
    Queued,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelQueueEntry {
    pub model_id: String,
    pub position: u32,
    pub state: ModelQueueState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelQueueSnapshot {
    pub active_model_id: Option<String>,
    pub entries: Vec<ModelQueueEntry>,
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

/// 诊断中的运行时能力结果。`detail` 和建议不得包含绝对路径或用户媒体内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCapabilityStatus {
    Ready,
    Warning,
    Blocked,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DoctorCapabilityCheck {
    pub id: String,
    pub status: DoctorCapabilityStatus,
    pub detail: String,
    pub suggested_action: Option<String>,
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
    #[serde(default)]
    pub capability_checks: Vec<DoctorCapabilityCheck>,
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
            if let Some(legacy_id) = &descriptor.replaces_model_id {
                let legacy = descriptors.get(legacy_id).ok_or_else(|| {
                    ModelError::CatalogInvalid(format!(
                        "{} 引用了不存在的旧模型 {}",
                        descriptor.id, legacy_id
                    ))
                })?;
                if legacy.ui_role != ModelUiRole::Legacy {
                    return Err(ModelError::CatalogInvalid(format!(
                        "{} 的替代目标 {} 不是旧模型",
                        descriptor.id, legacy_id
                    )));
                }
            }
        }
        Ok(Self { descriptors })
    }

    /// 加载随源码固定的清单；远程文件不能覆盖它。
    pub fn builtin() -> Result<Self, ModelError> {
        Self::from_json(include_str!("../resources/model-catalog-v1.json"))
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
        if let Some(archive) = &self.archive {
            return archive.size_bytes;
        }
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
        if self.archive.is_some() {
            return Err(ModelError::InvalidState(format!(
                "{} 必须通过声明的归档安装",
                self.id
            )));
        }
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

    pub fn requires_noncommercial_confirmation(&self) -> bool {
        let license = self.license.to_ascii_uppercase();
        license.contains("CC BY-NC-SA") || license.contains("RESEARCH-ONLY")
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

    /// 返回某次安装 staging 中归档的固定位置。归档不进入最终模型目录，避免把 ZIP
    /// 当作运行时模型文件或意外带入原子安装结果。
    pub fn archive_staging_path(
        &self,
        model_id: &str,
        staging_id: &str,
    ) -> Result<PathBuf, ModelError> {
        let descriptor = self.descriptor(model_id)?;
        if descriptor.archive.is_none() {
            return Err(ModelError::InvalidState(format!(
                "{} 不使用归档安装",
                descriptor.id
            )));
        }
        Ok(self
            .staging_root(staging_id)?
            .join(".archives")
            .join(&descriptor.id)
            .join(format!("{}.zip", descriptor.revision)))
    }

    /// 归档下载完成后，先检查其本身的固定大小和 SHA-256。
    pub fn verify_archive(&self, model_id: &str, archive_path: &Path) -> Result<(), ModelError> {
        let descriptor = self.descriptor(model_id)?;
        let archive = descriptor
            .archive
            .as_ref()
            .ok_or_else(|| ModelError::InvalidState(format!("{} 不使用归档安装", descriptor.id)))?;
        let metadata = fs::symlink_metadata(archive_path)
            .map_err(|_| ModelError::Integrity("模型归档不存在".to_string()))?;
        if !metadata.file_type().is_file() || metadata.len() != archive.size_bytes {
            return Err(ModelError::Integrity("模型归档大小不匹配".to_string()));
        }
        if sha256_file(archive_path)? != archive.sha256 {
            return Err(ModelError::Integrity("模型归档校验失败".to_string()));
        }
        Ok(())
    }

    /// 仅解压清单明确允许的常规文件。拒绝路径穿越、符号链接、重复项、清单外文件和
    /// 超出清单大小的 ZIP 条目；完成后仍会重新校验解压后的文件哈希。
    pub fn extract_archive_to_staging(
        &self,
        model_id: &str,
        staging_id: &str,
        archive_path: &Path,
    ) -> Result<(), ModelError> {
        self.verify_archive(model_id, archive_path)?;
        let descriptor = self.descriptor(model_id)?;
        let archive_spec = descriptor
            .archive
            .as_ref()
            .ok_or_else(|| ModelError::InvalidState(format!("{} 不使用归档安装", descriptor.id)))?;
        let destination = self
            .staging_root(staging_id)?
            .join(&descriptor.id)
            .join(&descriptor.revision);
        if destination.exists() {
            let metadata = fs::symlink_metadata(&destination)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ModelError::UnsafePath(
                    "归档 staging 目录不是常规目录".to_string(),
                ));
            }
            fs::remove_dir_all(&destination)?;
        }
        fs::create_dir_all(&destination)?;

        let mut expected = BTreeMap::new();
        for file in descriptor.files.iter().filter(|file| file.allowed) {
            expected.insert(file.path.as_str(), file);
        }
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| ModelError::Integrity(format!("模型归档无法读取：{error}")))?;
        let mut extracted = BTreeSet::new();

        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| ModelError::Integrity(format!("模型归档条目无法读取：{error}")))?;
            if entry
                .unix_mode()
                .is_some_and(|mode| (mode & 0o170_000) == 0o120_000)
            {
                return Err(ModelError::Integrity(
                    "模型归档不能包含符号链接".to_string(),
                ));
            }
            if entry.is_dir()
                && archive_spec.strip_prefix.as_deref().is_some_and(|prefix| {
                    entry.name().trim_matches('/') == prefix.trim_matches('/')
                })
            {
                continue;
            }
            let relative =
                archive_member_relative(entry.name(), archive_spec.strip_prefix.as_deref())?;
            if entry.is_dir() {
                let prefix = format!("{relative}/");
                if !expected.keys().any(|path| path.starts_with(&prefix)) {
                    return Err(ModelError::Integrity(format!(
                        "模型归档含有清单外目录：{relative}"
                    )));
                }
                continue;
            }
            let expected_file = expected.get(relative.as_str()).ok_or_else(|| {
                ModelError::Integrity(format!("模型归档含有清单外文件：{relative}"))
            })?;
            if entry.size() != expected_file.size_bytes {
                return Err(ModelError::Integrity(format!(
                    "模型归档文件大小不匹配：{relative}"
                )));
            }
            if !extracted.insert(relative.clone()) {
                return Err(ModelError::Integrity(format!(
                    "模型归档含有重复文件：{relative}"
                )));
            }
            let destination_file = destination.join(safe_relative_path(&relative)?);
            if let Some(parent) = destination_file.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination_file)?;
            let expected_size = expected_file.size_bytes;
            let copied = io::copy(
                &mut entry.take(expected_size.saturating_add(1)),
                &mut output,
            )?;
            if copied != expected_size {
                return Err(ModelError::Integrity(format!(
                    "模型归档文件解压大小不匹配：{relative}"
                )));
            }
            output.sync_all()?;
        }

        let missing = expected
            .keys()
            .filter(|path| !extracted.contains(**path))
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(ModelError::Integrity(format!(
                "模型归档缺少文件：{}",
                missing.join(", ")
            )));
        }
        self.verify_directory(model_id, &destination)
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

    /// Lists only legacy versions that the installed current model can safely replace.
    /// A shared old ForcedAligner remains retained until every installed legacy ASR that
    /// references it has been selected through its own current-model cleanup action.
    pub fn legacy_cleanup_preview(
        &self,
        target_model_id: &str,
    ) -> Result<LegacyModelCleanupPreview, ModelError> {
        let target = self.descriptor(target_model_id)?;
        if target.ui_role == ModelUiRole::Legacy {
            return Err(ModelError::InvalidState(
                "旧版模型不能作为清理目标".to_string(),
            ));
        }
        if self.installation(target_model_id)?.state != ModelInstallState::Installed {
            return Err(ModelError::InvalidState(
                "请先完成当前 MLX 模型安装，再清理旧版本。".to_string(),
            ));
        }
        let Some(legacy_id) = target.replaces_model_id.as_deref() else {
            return Ok(LegacyModelCleanupPreview {
                target_model_id: target_model_id.to_string(),
                bytes_to_free: 0,
                removable: Vec::new(),
                retained: Vec::new(),
            });
        };
        let legacy = self.descriptor(legacy_id)?;
        if legacy.ui_role != ModelUiRole::Legacy {
            return Err(ModelError::InvalidState(
                "当前模型未声明可清理的旧版本。".to_string(),
            ));
        }
        if self.installation(legacy_id)?.state != ModelInstallState::Installed {
            return Ok(LegacyModelCleanupPreview {
                target_model_id: target_model_id.to_string(),
                bytes_to_free: 0,
                removable: Vec::new(),
                retained: Vec::new(),
            });
        }

        let mut removable_ids = vec![legacy_id.to_string()];
        let mut retained = Vec::new();
        for dependency in legacy
            .dependencies
            .iter()
            .filter(|dependency| dependency.required)
        {
            let dependency_descriptor = self.descriptor(&dependency.model_id)?;
            if dependency_descriptor.ui_role != ModelUiRole::Legacy
                || self.installation(&dependency.model_id)?.state != ModelInstallState::Installed
            {
                continue;
            }
            let remaining_users = self
                .catalog
                .iter()
                .filter(|candidate| {
                    candidate.ui_role == ModelUiRole::Legacy
                        && candidate.id != legacy.id
                        && self.installation(&candidate.id).is_ok_and(|installation| {
                            installation.state == ModelInstallState::Installed
                        })
                        && candidate.dependencies.iter().any(|candidate_dependency| {
                            candidate_dependency.required
                                && candidate_dependency.model_id == dependency.model_id
                        })
                })
                .map(|candidate| candidate.id.clone())
                .collect::<Vec<_>>();
            if remaining_users.is_empty() {
                removable_ids.push(dependency.model_id.clone());
            } else {
                retained.push(self.legacy_cleanup_item(
                    &dependency.model_id,
                    Some(format!("仍由 {} 使用", remaining_users.join("、"))),
                )?);
            }
        }
        let removable = removable_ids
            .iter()
            .map(|model_id| self.legacy_cleanup_item(model_id, None))
            .collect::<Result<Vec<_>, _>>()?;
        let bytes_to_free = removable.iter().map(|item| item.bytes_to_free).sum();
        Ok(LegacyModelCleanupPreview {
            target_model_id: target_model_id.to_string(),
            bytes_to_free,
            removable,
            retained,
        })
    }

    /// Removes exactly the candidates from a freshly recomputed preview. The caller cannot
    /// choose arbitrary legacy IDs, so a renderer request cannot bypass shared-dependency checks.
    pub fn cleanup_legacy(
        &mut self,
        target_model_id: &str,
    ) -> Result<LegacyModelCleanupPreview, ModelError> {
        let preview = self.legacy_cleanup_preview(target_model_id)?;
        for item in &preview.removable {
            self.remove(&item.model_id)?;
        }
        Ok(preview)
    }

    fn legacy_cleanup_item(
        &self,
        model_id: &str,
        reason: Option<String>,
    ) -> Result<LegacyModelCleanupItem, ModelError> {
        let descriptor = self.descriptor(model_id)?;
        let bytes_to_free = directory_size(&self.installation_dir(model_id)?).unwrap_or(0);
        Ok(LegacyModelCleanupItem {
            model_id: descriptor.id.clone(),
            display_name: descriptor.display_name.clone(),
            bytes_to_free,
            reason,
        })
    }

    pub fn doctor_report(&mut self, environment: DoctorEnvironment) -> DoctorReport {
        let mut model_checks = Vec::new();
        let mut warnings = Vec::new();
        // Legacy descriptors remain in the catalog only so the settings page can clean them up.
        for descriptor in self
            .catalog
            .iter()
            .filter(|descriptor| descriptor.ui_role != ModelUiRole::Legacy)
        {
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
            capability_checks: Vec::new(),
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
    let immutable_revision = descriptor.revision.len() == 40
        && descriptor
            .revision
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    let source_is_valid = if descriptor.ui_role == ModelUiRole::Legacy {
        !descriptor.bundled
            && immutable_revision
            && descriptor.archive.is_none()
            && descriptor.source_url_template.starts_with("legacy://")
    } else {
        match descriptor.download_source {
            ModelDownloadSource::Modelscope => {
                !descriptor.bundled
                    && immutable_revision
                    && descriptor.archive.is_none()
                    && descriptor
                        .source_url_template
                        .starts_with("https://www.modelscope.cn/api/v1/models/")
                    && descriptor
                        .source_url_template
                        .contains("Revision={revision}")
                    && descriptor.repo_id.is_some()
            }
            ModelDownloadSource::Huggingface => {
                !descriptor.bundled && immutable_revision && descriptor.archive.is_none()
            }
            ModelDownloadSource::Bundled => {
                descriptor.bundled
                    && immutable_revision
                    && descriptor.source_url_template.starts_with("bundled://")
            }
            ModelDownloadSource::Official => {
                !descriptor.bundled && immutable_revision && descriptor.archive.is_some()
            }
        }
    };
    if !source_is_valid {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 的下载来源与 revision 不匹配",
            descriptor.id
        )));
    }
    let role_matches_distribution = match descriptor.ui_role {
        ModelUiRole::Bundled => descriptor.bundled,
        ModelUiRole::Primary | ModelUiRole::Dependency | ModelUiRole::Legacy => !descriptor.bundled,
    };
    if !role_matches_distribution {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 的 UI 层级与分发方式不匹配",
            descriptor.id
        )));
    }
    let direct_source_is_valid = descriptor.archive.is_some()
        || (!descriptor.source_url_template.is_empty()
            && (descriptor.bundled
                || descriptor.source_url_template.starts_with("https://")
                || (descriptor.ui_role == ModelUiRole::Legacy
                    && descriptor.source_url_template.starts_with("legacy://"))));
    if descriptor.license.is_empty()
        || !descriptor.license_url.starts_with("https://")
        || !direct_source_is_valid
    {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 缺少 license 或下载源",
            descriptor.id
        )));
    }
    if let Some(archive) = &descriptor.archive {
        if !archive.url.starts_with("https://")
            || archive.size_bytes == 0
            || !is_sha256(&archive.sha256)
        {
            return Err(ModelError::CatalogInvalid(format!(
                "{} 的归档缺少固定 URL、size 或 SHA-256",
                descriptor.id
            )));
        }
        if let Some(prefix) = archive.strip_prefix.as_deref() {
            safe_relative_path(prefix.trim_matches('/'))?;
        }
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
    if descriptor.archive.is_some() && seen_files.is_empty() {
        return Err(ModelError::CatalogInvalid(format!(
            "{} 的归档必须声明解压后的文件清单",
            descriptor.id
        )));
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
                "json" | "txt" | "safetensors" | "npz" | "pt" | "yaml" | "yml" | "onnx" | "bin"
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

fn archive_member_relative(name: &str, strip_prefix: Option<&str>) -> Result<String, ModelError> {
    if name.contains('\\') {
        return Err(ModelError::UnsafePath(name.to_string()));
    }
    let value = name.trim_end_matches('/');
    if value.is_empty() {
        return Err(ModelError::UnsafePath(name.to_string()));
    }
    let relative = if let Some(prefix) = strip_prefix {
        let prefix = prefix.trim_matches('/');
        if prefix.is_empty() {
            return Err(ModelError::UnsafePath("归档前缀不能为空".to_string()));
        }
        safe_relative_path(prefix)?;
        value
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .ok_or_else(|| ModelError::Integrity(format!("模型归档条目不在声明前缀内：{name}")))?
    } else {
        value
    };
    safe_relative_path(relative)?;
    Ok(relative.to_string())
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

fn directory_size(path: &Path) -> Result<u64, ModelError> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ModelError::Integrity(
            "模型目录不能包含符号链接".to_string(),
        ));
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total.saturating_add(directory_size(&entry.path())?);
    }
    Ok(total)
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
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

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

    fn legacy_cleanup_catalog() -> ModelCatalog {
        ModelCatalog::from_json(
            &serde_json::json!({
                "schema_version": 1,
                "models": [
                    {
                        "id": "old-aligner", "display_name": "Old aligner", "component": "forced_aligner",
                        "ui_role": "legacy", "repo_id": "old/aligner", "download_source": "bundled",
                        "revision": "1111111111111111111111111111111111111111", "files": [],
                        "license": "MIT", "license_url": "https://example.invalid/license", "dependencies": [],
                        "min_memory_bytes": null, "source_url_template": "legacy://old-aligner/{path}", "bundled": false
                    },
                    {
                        "id": "old-asr-a", "display_name": "Old ASR A", "component": "asr",
                        "ui_role": "legacy", "repo_id": "old/asr-a", "download_source": "bundled",
                        "revision": "2222222222222222222222222222222222222222", "files": [],
                        "license": "MIT", "license_url": "https://example.invalid/license",
                        "dependencies": [{"model_id":"old-aligner","required":true,"reason":"old timing"}],
                        "min_memory_bytes": null, "source_url_template": "legacy://old-asr-a/{path}", "bundled": false
                    },
                    {
                        "id": "old-asr-b", "display_name": "Old ASR B", "component": "asr",
                        "ui_role": "legacy", "repo_id": "old/asr-b", "download_source": "bundled",
                        "revision": "3333333333333333333333333333333333333333", "files": [],
                        "license": "MIT", "license_url": "https://example.invalid/license",
                        "dependencies": [{"model_id":"old-aligner","required":true,"reason":"old timing"}],
                        "min_memory_bytes": null, "source_url_template": "legacy://old-asr-b/{path}", "bundled": false
                    },
                    {
                        "id": "new-asr-a", "display_name": "New ASR A", "component": "asr",
                        "ui_role": "primary", "repo_id": "new/asr-a", "download_source": "huggingface",
                        "revision": "4444444444444444444444444444444444444444", "files": [],
                        "license": "MIT", "license_url": "https://example.invalid/license", "dependencies": [],
                        "min_memory_bytes": null, "source_url_template": "https://example.invalid/new-a/{revision}/{path}",
                        "replaces_model_id": "old-asr-a", "bundled": false
                    },
                    {
                        "id": "new-asr-b", "display_name": "New ASR B", "component": "asr",
                        "ui_role": "primary", "repo_id": "new/asr-b", "download_source": "huggingface",
                        "revision": "5555555555555555555555555555555555555555", "files": [],
                        "license": "MIT", "license_url": "https://example.invalid/license", "dependencies": [],
                        "min_memory_bytes": null, "source_url_template": "https://example.invalid/new-b/{revision}/{path}",
                        "replaces_model_id": "old-asr-b", "bundled": false
                    }
                ]
            })
            .to_string(),
        )
        .expect("legacy cleanup catalog")
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8], Option<u32>)]) {
        let file = File::create(path).expect("zip file");
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes, mode) in entries {
            let options = mode.map_or_else(SimpleFileOptions::default, |mode| {
                SimpleFileOptions::default().unix_permissions(mode)
            });
            if name.ends_with('/') {
                writer.add_directory(*name, options).expect("zip directory");
            } else {
                writer.start_file(name, options).expect("zip entry");
                writer.write_all(bytes).expect("zip bytes");
            }
        }
        writer.finish().expect("finish zip");
    }

    fn archive_catalog(archive_path: &Path, strip_prefix: Option<&str>) -> ModelCatalog {
        let config = b"model: simam-resnet34\n";
        let weights = b"fixture-weights";
        let archive = fs::read(archive_path).expect("archive bytes");
        let archive_hash = format!("{:x}", Sha256::digest(&archive));
        let config_hash = format!("{:x}", Sha256::digest(config));
        let weights_hash = format!("{:x}", Sha256::digest(weights));
        ModelCatalog::from_json(
            &serde_json::json!({
                "schema_version": 1,
                "models": [{
                    "id": "speaker", "display_name": "Multilingual speaker", "component": "speaker",
                    "ui_role": "primary", "repo_id": "official/speaker",
                    "download_source": "official", "revision": "cccccccccccccccccccccccccccccccccccccccc",
                    "files": [
                        {"path": "config.yaml", "size_bytes": config.len(), "sha256": config_hash, "allowed": true},
                        {"path": "avg_model.pt", "size_bytes": weights.len(), "sha256": weights_hash, "allowed": true}
                    ],
                    "license": "CC BY-NC-SA 4.0", "license_url": "https://creativecommons.org/licenses/by-nc-sa/4.0/",
                    "dependencies": [], "min_memory_bytes": null, "source_url_template": "",
                    "archive": {
                        "url": "https://example.invalid/speaker.zip", "size_bytes": archive.len(), "sha256": archive_hash,
                        "strip_prefix": strip_prefix
                    }, "bundled": false
                }]
            })
            .to_string(),
        )
        .expect("archive catalog")
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
    fn modelscope_requires_an_immutable_revision_and_sdk_api_template() {
        let mut value: serde_json::Value =
            serde_json::from_str(&fixture_catalog()).expect("fixture");
        value["models"][0]["download_source"] = serde_json::json!("modelscope");
        value["models"][0]["source_url_template"] = serde_json::json!(
            "https://www.modelscope.cn/api/v1/models/fixture/aligner/repo?Revision={revision}&FilePath={path}"
        );
        value["models"][0]["repo_id"] = serde_json::json!("fixture/aligner");
        assert!(ModelCatalog::from_json(&value.to_string()).is_ok());

        value["models"][0]["revision"] = serde_json::json!("master");
        assert!(matches!(
            ModelCatalog::from_json(&value.to_string()),
            Err(ModelError::CatalogInvalid(_))
        ));
    }

    #[test]
    fn builtin_catalog_contains_real_asr_and_speaker_components() {
        let catalog = ModelCatalog::builtin().expect("built-in catalog");
        assert_eq!(catalog.len(), 10);
        let low = catalog.get("qwen3-asr-0.6b-4bit").expect("0.6B MLX");
        assert_eq!(low.revision, "70ccd0ba0c24b0c78efc313ce81c1c78c64a3dd7");
        assert_eq!(
            low.repo_id.as_deref(),
            Some("mlx-community/Qwen3-ASR-0.6B-4bit")
        );
        assert_eq!(low.total_size(), 712_778_816);
        assert_eq!(low.replaces_model_id.as_deref(), Some("qwen3-asr-0.6b"));
        assert!(low.source_url_template.contains("Revision={revision}"));
        assert!(!low.source_url_template.contains("master"));
        assert_eq!(
            catalog
                .get("qwen3-forced-aligner-0.6b-8bit")
                .expect("aligner")
                .revision,
            "998b617c695f61865d444c62051fe51030acef6f"
        );
        let asr = catalog.get("qwen3-asr-1.7b-8bit").expect("1.7B MLX");
        assert_eq!(asr.component, ModelComponent::Asr);
        assert_eq!(asr.download_source, ModelDownloadSource::Modelscope);
        assert_eq!(asr.revision, "579e237ce6ec925252973afe835d2f98a138602f");
        assert_eq!(asr.total_size(), 2_467_856_567);
        assert!(asr.dependencies.iter().any(|dependency| {
            dependency.model_id == "qwen3-forced-aligner-0.6b-8bit" && dependency.required
        }));
        assert_eq!(asr.ui_role, ModelUiRole::Primary);
        assert_eq!(
            catalog
                .get("qwen3-forced-aligner-0.6b-8bit")
                .expect("aligner")
                .ui_role,
            ModelUiRole::Dependency
        );
        let vad = catalog.get("silero-vad-v6").expect("MLX vad");
        assert_eq!(vad.download_source, ModelDownloadSource::Modelscope);
        assert_eq!(vad.total_size(), 1_238_323);
        assert_eq!(vad.ui_role, ModelUiRole::Dependency);
        let speaker = catalog
            .get("wespeaker-voxceleb-resnet34-lm")
            .expect("MLX speaker");
        assert_eq!(speaker.download_source, ModelDownloadSource::Modelscope);
        assert_eq!(speaker.total_size(), 26_614_852);
        assert_eq!(speaker.dependencies[0].model_id, "silero-vad-v6");
        assert_eq!(speaker.ui_role, ModelUiRole::Primary);
        assert_eq!(speaker.license, "MIT");
        assert!(!speaker.requires_noncommercial_confirmation());
        for legacy_id in [
            "qwen3-asr-0.6b",
            "qwen3-asr-1.7b",
            "qwen3-forced-aligner-0.6b",
            "wespeaker-zh",
            "silero-vad",
        ] {
            assert_eq!(
                catalog.get(legacy_id).expect("legacy").ui_role,
                ModelUiRole::Legacy
            );
        }
    }

    #[test]
    fn builtin_catalog_pins_all_mlx_modelscope_revisions_and_runtime_files() {
        let catalog = ModelCatalog::builtin().expect("built-in catalog");
        for (id, repo, revision, path, size, sha256) in [
            (
                "qwen3-asr-0.6b-4bit",
                "mlx-community/Qwen3-ASR-0.6B-4bit",
                "70ccd0ba0c24b0c78efc313ce81c1c78c64a3dd7",
                "model.safetensors",
                708_236_945,
                "70c7e67e588062adce4f10796e47ad42ead51c6671eda61a0987eae38ca95ddf",
            ),
            (
                "qwen3-asr-1.7b-8bit",
                "mlx-community/Qwen3-ASR-1.7B-8bit",
                "579e237ce6ec925252973afe835d2f98a138602f",
                "model.safetensors",
                2_463_307_541,
                "bf304b009cc7eca79283056f787b44c952d24ac22cec787b39732bba3c23c13c",
            ),
            (
                "qwen3-forced-aligner-0.6b-8bit",
                "mlx-community/Qwen3-ForcedAligner-0.6B-8bit",
                "998b617c695f61865d444c62051fe51030acef6f",
                "model.safetensors",
                1_271_924_386,
                "be19ef8ac4326d032e7673342930b14c2df30bd68c1632493b0f563e30829f91",
            ),
            (
                "wespeaker-voxceleb-resnet34-lm",
                "mlx-community/wespeaker-voxceleb-resnet34-LM",
                "d34f9e11f648c7e83d077bf6e10da94ba56f7b72",
                "weights.npz",
                26_614_262,
                "802706880b81ece11a9acefb2cf523ae91473e3b7615858390a1eded4efcdedf",
            ),
            (
                "silero-vad-v6",
                "mlx-community/silero-vad-v6",
                "c34917caf1d6fc01b763a4ab0345ff1724fdb9c2",
                "model.safetensors",
                1_237_860,
                "65b6c5f0293cbc44d109e58bef78b474d9c65dedbee814cf0b90ef5f0d9150ff",
            ),
        ] {
            let descriptor = catalog.get(id).expect("MLX descriptor");
            assert_eq!(descriptor.repo_id.as_deref(), Some(repo));
            assert_eq!(descriptor.revision, revision);
            let file = descriptor.file(path).expect("pinned runtime file");
            assert!(file.allowed);
            assert_eq!(file.size_bytes, size);
            assert_eq!(file.sha256, sha256);
            assert!(
                descriptor
                    .files
                    .iter()
                    .all(|file| !file.path.ends_with(".py"))
            );
        }
    }

    #[test]
    fn archive_install_checks_the_outer_zip_then_each_allowed_file_before_atomic_commit() {
        let root = temp_root("archive-install");
        fs::create_dir_all(&root).expect("root");
        let archive_path = root.join("speaker.zip");
        write_zip(
            &archive_path,
            &[
                ("bundle/", b"", None),
                ("bundle/config.yaml", b"model: simam-resnet34\n", None),
                ("bundle/avg_model.pt", b"fixture-weights", None),
            ],
        );
        let catalog = archive_catalog(&archive_path, Some("bundle"));
        let mut manager = ModelManager::new(&root, catalog).expect("manager");
        let stage = "archive-stage";
        let staged_archive = manager
            .archive_staging_path("speaker", stage)
            .expect("archive staging path");
        fs::create_dir_all(staged_archive.parent().expect("archive parent"))
            .expect("archive parent");
        fs::copy(&archive_path, &staged_archive).expect("stage archive");
        manager
            .extract_archive_to_staging("speaker", stage, &staged_archive)
            .expect("safe archive extraction");
        manager
            .transition("speaker", ModelInstallState::Queued)
            .expect("queue");
        manager
            .transition("speaker", ModelInstallState::Downloading)
            .expect("download");
        manager
            .transition("speaker", ModelInstallState::Verifying)
            .expect("verify");
        manager
            .atomically_install("speaker", stage)
            .expect("atomic install");
        let installed = manager.installation_dir("speaker").expect("installed path");
        assert_eq!(
            fs::read(installed.join("config.yaml")).expect("config"),
            b"model: simam-resnet34\n"
        );
        assert_eq!(
            fs::read(installed.join("avg_model.pt")).expect("weights"),
            b"fixture-weights"
        );
        assert!(
            manager
                .verify_installed("speaker")
                .expect("installed verification")
                .state
                .is_installed()
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn archive_rejects_path_traversal_symlink_and_unexpected_entries() {
        for (label, entries) in [
            (
                "traversal",
                vec![
                    (
                        "bundle/config.yaml",
                        b"model: simam-resnet34\n".as_slice(),
                        None,
                    ),
                    ("bundle/avg_model.pt", b"fixture-weights".as_slice(), None),
                    ("bundle/../escape.txt", b"escape".as_slice(), None),
                ],
            ),
            (
                "symlink",
                vec![
                    (
                        "bundle/config.yaml",
                        b"model: simam-resnet34\n".as_slice(),
                        None,
                    ),
                    ("bundle/avg_model.pt", b"fixture-weights".as_slice(), None),
                    ("bundle/link", b"target".as_slice(), Some(0o120777)),
                ],
            ),
            (
                "unexpected",
                vec![
                    (
                        "bundle/config.yaml",
                        b"model: simam-resnet34\n".as_slice(),
                        None,
                    ),
                    ("bundle/avg_model.pt", b"fixture-weights".as_slice(), None),
                    ("bundle/readme.txt", b"unexpected".as_slice(), None),
                ],
            ),
        ] {
            let root = temp_root(label);
            fs::create_dir_all(&root).expect("root");
            let archive_path = root.join("speaker.zip");
            write_zip(&archive_path, &entries);
            let catalog = archive_catalog(&archive_path, Some("bundle"));
            let manager = ModelManager::new(&root, catalog).expect("manager");
            let result = manager.extract_archive_to_staging("speaker", "unsafe", &archive_path);
            assert!(matches!(
                result,
                Err(ModelError::Integrity(_)) | Err(ModelError::UnsafePath(_))
            ));
            assert!(
                !root.join("escape.txt").exists(),
                "{label} entry must not escape the staging directory"
            );
            fs::remove_dir_all(root).ok();
        }
    }

    #[test]
    fn archive_requires_fixed_hash_and_noncommercial_models_require_confirmation() {
        let root = temp_root("archive-metadata");
        fs::create_dir_all(&root).expect("root");
        let archive_path = root.join("speaker.zip");
        write_zip(
            &archive_path,
            &[
                ("config.yaml", b"model: simam-resnet34\n", None),
                ("avg_model.pt", b"fixture-weights", None),
            ],
        );
        let mut value: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&serde_json::json!({
                "schema_version": 1,
                "models": []
            }))
            .expect("empty catalog json"),
        )
        .expect("value");
        let catalog = archive_catalog(&archive_path, None);
        let descriptor = catalog.get("speaker").expect("speaker");
        assert!(descriptor.requires_noncommercial_confirmation());
        value["models"] = serde_json::to_value(vec![descriptor]).expect("descriptor json");
        value["models"][0]["archive"]["sha256"] = serde_json::json!("bad");
        assert!(matches!(
            ModelCatalog::from_json(&value.to_string()),
            Err(ModelError::CatalogInvalid(_))
        ));
        fs::remove_dir_all(root).ok();
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
    fn cleanup_preview_removes_old_versions_but_retains_a_shared_legacy_aligner() {
        let root = temp_root("legacy-cleanup");
        let mut manager = ModelManager::new(&root, legacy_cleanup_catalog()).expect("manager");
        for model_id in [
            "old-aligner",
            "old-asr-a",
            "old-asr-b",
            "new-asr-a",
            "new-asr-b",
        ] {
            let directory = manager.installation_dir(model_id).expect("directory");
            fs::create_dir_all(&directory).expect("directory");
            fs::write(directory.join("legacy.bin"), model_id.as_bytes()).expect("model bytes");
            manager
                .transition(model_id, ModelInstallState::Installed)
                .expect("installed state");
        }

        let first = manager
            .legacy_cleanup_preview("new-asr-a")
            .expect("first preview");
        assert_eq!(
            first
                .removable
                .iter()
                .map(|item| item.model_id.as_str())
                .collect::<Vec<_>>(),
            ["old-asr-a"]
        );
        assert_eq!(first.retained.len(), 1);
        assert_eq!(first.retained[0].model_id, "old-aligner");
        assert!(
            first.retained[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("old-asr-b"))
        );
        assert!(first.bytes_to_free > 0);
        manager.cleanup_legacy("new-asr-a").expect("first cleanup");
        assert_eq!(
            manager.installation("old-asr-a").expect("old a").state,
            ModelInstallState::NotInstalled
        );
        assert_eq!(
            manager
                .installation("old-aligner")
                .expect("old aligner")
                .state,
            ModelInstallState::Installed
        );

        let second = manager
            .legacy_cleanup_preview("new-asr-b")
            .expect("second preview");
        assert_eq!(
            second
                .removable
                .iter()
                .map(|item| item.model_id.as_str())
                .collect::<Vec<_>>(),
            ["old-asr-b", "old-aligner"]
        );
        manager.cleanup_legacy("new-asr-b").expect("second cleanup");
        assert_eq!(
            manager
                .installation("old-aligner")
                .expect("old aligner")
                .state,
            ModelInstallState::NotInstalled
        );
        assert!(matches!(
            manager.legacy_cleanup_preview("old-asr-a"),
            Err(ModelError::InvalidState(_))
        ));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn doctor_report_excludes_legacy_models_reserved_for_cleanup() {
        let root = temp_root("doctor-legacy-models");
        let mut manager = ModelManager::new(&root, legacy_cleanup_catalog()).expect("manager");

        let report = manager.doctor_report(DoctorEnvironment {
            architecture: "arm64".to_string(),
            os_version: "fixture".to_string(),
            memory_bytes: 16,
            free_model_bytes: 8,
            ffmpeg_available: true,
            libass_available: true,
            asr_runtime_ready: true,
            speaker_runtime_ready: true,
        });

        assert_eq!(
            report
                .model_checks
                .iter()
                .map(|check| check.model_id.as_str())
                .collect::<Vec<_>>(),
            ["new-asr-a", "new-asr-b"]
        );
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

    #[test]
    fn synthetic_installation_state_fixtures_recover_through_manager_api() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/model-installation-states.json"
        ))
        .expect("fixture json");
        let fixture_schema_version = fixture["installations_schema_version"]
            .as_u64()
            .expect("installation envelope schema version");
        assert_eq!(fixture_schema_version, 1);
        assert_eq!(MODEL_INSTALLATIONS_SCHEMA_VERSION, 1);
        let catalog_json = serde_json::to_string(&fixture["catalog"]).expect("catalog json");
        let catalog = ModelCatalog::from_json(&catalog_json).expect("fixture catalog");

        for case in fixture["cases"].as_array().expect("fixture cases") {
            let name = case["name"].as_str().expect("case name");
            let root = temp_root(name);
            fs::create_dir_all(&root).expect("model root");
            let installation = case["installation"].clone();
            let model_id = installation["model_id"].as_str().expect("model id");
            let store = serde_json::json!({
                "schema_version": fixture_schema_version,
                "installations": { model_id: installation }
            });
            fs::write(
                root.join("installations.json"),
                serde_json::to_vec_pretty(&store).expect("state json"),
            )
            .expect("state fixture");
            for file in case["generated_files"].as_array().expect("generated files") {
                let path = root.join(file["path"].as_str().expect("relative path"));
                fs::create_dir_all(path.parent().expect("generated parent"))
                    .expect("generated parent dir");
                fs::write(
                    &path,
                    file["contents"].as_str().expect("synthetic contents"),
                )
                .expect("generated file");
            }

            let mut manager = ModelManager::new(&root, catalog.clone()).expect("manager opens");
            let recovered = manager.installation(model_id).expect("installation");
            assert_eq!(
                recovered.state.as_str(),
                case["expected_state_after_open"]
                    .as_str()
                    .expect("expected state"),
                "case {name}"
            );
            if name == "installed" {
                assert_eq!(
                    manager
                        .verify_installed(model_id)
                        .expect("installed fixture verifies")
                        .state,
                    ModelInstallState::Installed
                );
            } else {
                let staging_id = recovered.staging_id.as_deref().expect("staging id");
                let part = manager
                    .staging_root(staging_id)
                    .expect("safe staging path")
                    .join(model_id)
                    .join(&recovered.revision)
                    .join("config.json.part");
                assert_eq!(fs::read(part).expect("partial file is preserved"), b"he");
                assert_eq!(
                    manager
                        .queue_install(model_id)
                        .expect("paused fixture can resume")[0]
                        .state,
                    ModelInstallState::Queued
                );
            }
            if name == "staging-interrupted" {
                assert_eq!(
                    recovered.last_error_code.as_deref(),
                    Some("MODEL_TASK_INTERRUPTED")
                );
            }
            fs::remove_dir_all(root).ok();
        }
    }
}
