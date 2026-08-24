# Double Love Studio v2 实施核对

> **历史实施快照（已被 Phase 5D 后的 Electron 实现取代）**：下文命令式表述和旧容器路径只记录 2026-08-21 的设计输入，不是现行开发或发布指导。

日期：2026-08-21

范围：设置窗口、全局偏好、模型生命周期、ForcedAligner 接线、首次启动引导、诊断。
本文件是迁移前的接口和验收契约；不包含视觉稿，也不替代产品代码。

## 1. 当前基线与可复用能力

### 已存在、应直接复用

| 能力 | 现状 | 复用边界 |
| --- | --- | --- |
| Tauri 壳 | 旧 Rust 桌面 adapter 使用 Tauri 2 `Builder`、`AppHandle`、`State`、`Emitter`；`tauri.conf.json` 只声明一个 `main` 窗口（1440×900，最小 960×640）。 | 保留当时主窗口和 `media://` 只读协议；新增设置窗口时仍从同一 `studio/dist` 加载。 |
| Rust 状态 | `AppState` 只有当前 `ProjectStore` 和 `TaskRegistry`。转录、说话人任务已经由 Rust 持有，不依赖 React 生命周期。 | 把 `PreferencesStore` 和 `ModelManager` 放入 `AppState`，模型下载任务不得放进 React。 |
| 命令结果 | 所有现有命令返回 `OperationResult<T>`，状态为 `success | partial | failed | cancelled`，带诊断和 revision。 | 新增命令全部沿用 `OperationResult<T>`；模型内部状态另用下列固定状态机。 |
| 项目存储 | `crates/double-love-engine/src/project.rs` 创建 `.doublelove/{project.sqlite,manifest.json,cache,logs,exports}`；`ProjectSummary` 有稳定 `project_id`、绝对 `root`、`database`、`revision`。 | `project_create/open` 成功后记录最近项目；不把全局偏好写入项目 SQLite。 |
| 项目字幕设置 | `SubtitleStyle` 与 `subtitle_style_get/set` 已存在，默认样式在引擎中定义；`ProjectSettings` 当前同时混合了外观、画布、输出帧率、历史和字幕。 | 保留项目级命令；全局默认字幕样式只在创建新项目时复制，不能隐式改写已有项目。 |
| 转录版本 | `ProjectStore` 有候选 `transcript_run`，只有成功 run 才原子切成 active；取消/失败保留旧版本。 | 模型缺失拦截在启动转录前发生，不改变项目和旧转录。 |
| 侧车 | `sidecar.rs` 已有 JSONL 握手、事件、stderr 日志和取消终止链；`resolve_python` 当前顺序为显式路径 → `.venv/bin/python` → PATH 的 `python3`。 | 发布/用户路径必须禁用 PATH 回退；开发期回退只能保留在开发配置。 |
| 模型运行时 | `scripts/prepare-model-runtime.sh` 将一个共享 `.venv`、`double_love_asr/` 与 `double_love_speaker/` 复制到 `studio/build/model-runtime`；`verify-release-runtime.sh` 检查共享 Python、两个包、固定版本和禁用依赖。 | 运行时随 App 分发，模型权重不进 App；模型管理器只管理权重和校验。 |
| 现有诊断 | CLI 有 `doctor`、`model-verify`、`model-test`；`model_verify` 已设置 `HF_HUB_OFFLINE=1` 和 `TRANSFORMERS_OFFLINE=1`，但 Tauri 转录路径没有设置。 | 把检查逻辑抽到可被 Tauri 和 CLI 复用的 Rust 服务；报告必须脱敏。 |
| 前端事件 | 当前只有 `dl://progress`、`dl://task-state`；React 在 `studio/src/App.tsx` 里监听。 | 新增事件广播给所有窗口；设置窗口关闭后模型任务继续运行。 |

### 当前明确缺口

- 没有 `settings` WebviewWindow、原生菜单、`Cmd+,` 或单例窗口契约。`Sidebar` 的“设置”只是同一主窗口内的 `StudioScreen='settings'`。
- 没有全局偏好持久化；主题仅写入 `window.localStorage`，设置页无项目时反而不可用。
- 没有模型清单、依赖图、下载队列、staging、Range 续传、校验和、安装状态或用户可见恢复动作。
- `sidecars/asr/double_love_asr/transcriber.py` 只调用 `Session(...).transcribe(return_timestamps=True)`，没有传模型目录或显式 `ForcedAligner` 实例；当前包会在 `return_timestamps=True` 时隐式创建默认对齐器，可能联网或加载错误权重。
- `sidecars/speaker/double_love_speaker/engine.py` 调用 `wespeaker.load_model("chinese")` 和包内 `load_silero_vad()`；Tauri 未传本地模型目录，也未设置离线环境变量。
- 旧 capability 配置只允许 `main` 窗口；没有 settings 窗口权限、Store 插件权限或打开日志目录的受限能力。

## 2. 全局偏好与最近项目

### 存储决策

使用 Tauri 官方 Store 插件的 Rust API：`tauri_plugin_store::Builder::default().build()`，通过 `StoreExt::store("preferences.json")` 取得同一份 app-data store。前端不得直接写 store；所有读写经过强类型 Tauri 命令。Store 的路径由 Tauri 解析到 `app_data_dir`，与模型目录同属 Application Support，但分开管理。

- Store key 固定为 `app_preferences`，值为 `AppPreferencesV1` 的 JSON 对象。
- `auto_save` 使用默认 100ms；每个命令完成后再显式 `save()`，确保窗口重启时不会丢最后一次设置。
- `schema_version` 必须等于 `1`。读取时先做 serde 反序列化和字段校验，再按版本迁移；未知字段忽略。
- JSON 损坏时把原文件移动为 `preferences.corrupt.<UTC timestamp>.json`，写入完整默认值并返回 `PREFERENCES_RECOVERED` 警告，不让应用启动失败。
- 设置更新通过一个锁串行化；主窗口和设置窗口不能各自缓存并覆盖整份对象。更新命令只修改允许的字段，然后广播 `dl://preferences-changed`。

### 公共类型（Rust `serde` + `ts-rs`）

```rust
pub struct AppPreferencesV1 {
    pub schema_version: u32,                 // 固定 1
    pub theme: ThemeMode,                    // light | dark | system，默认 light
    pub restore_last_project: bool,          // 默认 true
    pub timecode_precision: TimecodePrecision, // frame | millisecond，默认 frame
    pub transcript_section_tint: bool,       // 默认 true
    pub cjk_spacing: bool,                   // 默认 true
    pub default_subtitle_style: SubtitleStyle,
    pub model_root: String,                  // 默认 <app_data_dir>/models
    pub model_endpoint: String,              // 默认 https://huggingface.co
    pub default_asr_model: String,           // qwen3-asr-0.6b 或 qwen3-asr-1.7b
    pub onboarding_version: u32,              // 当前为 1
    pub onboarding_completed: bool,
    pub recent_projects: Vec<RecentProjectRecord>, // 持久化最多 20 条
}

#[serde(rename_all = "lowercase")]
pub enum ThemeMode { Light, Dark, System }
#[serde(rename_all = "snake_case")]
pub enum TimecodePrecision { Frame, Millisecond }

pub struct PreferencesPatch {
    pub theme: Option<ThemeMode>,
    pub restore_last_project: Option<bool>,
    pub timecode_precision: Option<TimecodePrecision>,
    pub transcript_section_tint: Option<bool>,
    pub cjk_spacing: Option<bool>,
    pub default_subtitle_style: Option<SubtitleStyle>,
    pub default_asr_model: Option<String>,
    pub model_endpoint: Option<String>,
    pub model_root: Option<String>,              // 仅由模型目录迁移流程处理
}

pub struct RecentProjectRecord {
    pub project_id: Option<String>,
    pub root: String,                         // canonical absolute path，仅本地存储
    pub display_name: String,
    pub last_opened_at: String,               // UTC RFC3339
}

pub struct RecentProject {
    pub project_id: Option<String>,
    pub root: String,
    pub display_name: String,
    pub last_opened_at: String,
    pub exists: bool,                         // 每次 list 时重新计算，不持久化
}

pub struct SystemProfile {
    pub memory_bytes: u64,
    pub architecture: String,                 // arm64
    pub os_version: String,
    pub free_model_bytes: u64,
    pub recommended_asr_model: String,
}
```

`memory_bytes < 16 GiB` 时推荐 `qwen3-asr-0.6b`，否则推荐 `qwen3-asr-1.7b`。系统信息只用于推荐和诊断，不上传。

`recent_projects_list` 返回按 `last_opened_at DESC` 的最多 20 条；`recent_project_forget` 只删记录，不删项目目录。`project_create`/`project_open` 成功后自动 upsert 记录并裁剪到 20 条。项目路径不存在时仍返回记录（`exists=false`），UI 提供“移除记录”，而不是删除文件。

### 偏好命令

| 命令 | 参数 | 返回/副作用 | 错误 |
| --- | --- | --- | --- |
| `preferences_get` | 无 | `OperationResult<AppPreferencesV1>`；缺失时返回默认值并落盘 | `PREFERENCES_READ_FAILED`、`PREFERENCES_RECOVERED`（warning） |
| `preferences_update` | `patch: PreferencesPatch`，只允许已定义字段 | 校验后写入 Store，返回完整 `AppPreferencesV1`，广播 `dl://preferences-changed` | `PREFERENCES_INVALID_FIELD`、`PREFERENCES_WRITE_FAILED`、`MODEL_ENDPOINT_INVALID` |
| `recent_projects_list` | 无 | `OperationResult<Vec<RecentProject>>` | `PREFERENCES_READ_FAILED` |
| `recent_project_forget` | `root: String`（必须精确匹配已记录 canonical path） | 删除单条记录；不触碰文件 | `RECENT_PROJECT_NOT_FOUND` |
| `system_profile` | 无 | `OperationResult<SystemProfile>` | `SYSTEM_PROFILE_FAILED` |

`PreferencesPatch` 不接受任意 JSON key；`model_endpoint` 只能是 `https` URL（测试 fixture 例外仅在测试编译配置中允许 `http://127.0.0.1`），不能包含用户名、密码或 token。`model_root` 虽由同一命令传入，但必须先调用 `ModelManager::migrate_root` 完成复制、校验和切换；迁移失败时偏好和旧目录都不变。

## 3. 独立设置窗口与原生菜单

### 窗口契约

- 主窗口 label 固定为 `main`，继续由 `tauri.conf.json` 创建。
- 设置窗口 label 固定为 `settings`，尺寸 760×580，最小 700×500，单例、可调整大小、关闭只隐藏不销毁。
- `settings_open` 先调用 `app.get_webview_window("settings")`；已存在则 `show()`、`set_focus()`，不存在才用 `WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html?window=settings"))` 创建。所有窗口使用同一个 `studio/dist`，React 根据当前 window label 选择主应用或七页设置应用。
- `settings_open` 不传项目路径、不复制项目状态；设置页只通过偏好/模型/诊断命令工作。
- 旧 capability 配置至少同时覆盖 `main`、`settings`；窗口、事件、Store、dialog/open-directory 权限只授予这两个 label，不开放任意远程 URL。
- `model_reveal` 与 `diagnostics_reveal_logs` 使用旧容器 opener 插件的 Rust API，但只传 Rust 自己解析出的模型根/日志根；不接受前端路径，也不开放任意 shell 命令。新增 opener capability 仅给 `main`、`settings`。

### 侧栏和 `Cmd+,`

- 主窗口侧栏“设置”按钮只调用 `settings_open`，不再把主窗口切到项目设置页。
- Rust `setup` 构造 macOS App menu，菜单项 id 固定为 `settings-open`，标题“设置…”，accelerator 固定 `Cmd+,`；`app.on_menu_event` 与侧栏调用同一个 `open_settings_window(app)` helper，不能复制两套窗口逻辑。
- 快捷键页展示固定快捷键：新建 `Cmd+N`、打开 `Cmd+O`、设置 `Cmd+,`、播放/暂停 `Space`、前后跳转 `←/→`、拆分 `S`、撤销/重做 `Cmd+Z`/`Cmd+Shift+Z`、导出 `Cmd+E`。首版不提供改键入口；菜单 accelerator 与前端键盘处理必须共用这些 id。
- `dl://preferences-changed`、`dl://model-progress`、`dl://model-state`、`dl://doctor-result` 使用 `AppHandle::emit` 广播；设置窗口关闭后事件仍由 Rust 任务产生，重新打开时通过命令读取当前快照。

Tauri 官方窗口 API 与菜单事件参考：

<https://v2.tauri.app/learn/window-customization/>

<https://v2.tauri.app/learn/window-menu/>
Store 插件参考：<https://v2.tauri.app/plugin/store/>

## 4. 模型目录、清单与安装状态

### 目录和清单

模型默认根目录为 `<app_data_dir>/models`，结构固定：

```text
models/
  catalog-cache/                    # 只读内置清单副本，不接受远程覆盖
  installations.json                # 安装状态与 staging 恢复信息，原子写入
  .staging/<install_id>/<model_id>/ # 未完成下载，永不被运行时当作已安装
  qwen3-asr-0.6b/<revision>/
  qwen3-asr-1.7b/<revision>/
  qwen3-forced-aligner-0.6b/<revision>/
  silero-vad/<revision>/
  wespeaker-zh/<revision>/
```

内置 `crates/double-love-engine/resources/model-catalog-v1.json` 是唯一权威清单；每个文件必须有路径、字节数、SHA-256、许可和允许文件类型。revision 只能是已解析的 commit hash，禁止 `main`、浮动 tag 或未锁定的 repo URL。清单缺少大小、hash、license 或 revision 时，`model_catalog` 失败，不显示“可安装”。

```rust
pub struct ModelDescriptor {
    pub id: String,                          // 固定 id，见下表
    pub display_name: String,
    pub component: ModelComponent,           // asr | forced_aligner | vad | speaker
    pub repo_id: Option<String>,             // HF repo 或官方项目来源
    pub revision: String,                    // commit hash
    pub files: Vec<ModelFile>,
    pub license: String,
    pub license_url: String,
    pub dependencies: Vec<ModelDependency>,
    pub min_memory_bytes: Option<u64>,
    pub source_url_template: String,
}

pub struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub allowed: bool,
}

pub struct ModelDependency {
    pub model_id: String,
    pub required: bool,
    pub reason: String,
}

#[serde(rename_all = "snake_case")]
pub enum ModelInstallState {
    NotInstalled,
    Queued,
    Downloading,
    Paused,
    Verifying,
    Installed,
    Corrupt,
    Failed,
}

pub struct ModelInstallation {
    pub model_id: String,
    pub revision: String,
    pub state: ModelInstallState,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub staging_id: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>, // 用户可读，不含路径
    pub updated_at: String,
}

pub struct ModelDownloadProgress {
    pub model_id: String,
    pub state: ModelInstallState,
    pub current_file: Option<String>,       // 清单相对路径，不是本地绝对路径
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub file_bytes_downloaded: u64,
    pub file_bytes_total: u64,
    pub speed_bytes_per_second: Option<u64>,
}

pub struct ModelDescriptorWithInstallation {
    pub descriptor: ModelDescriptor,
    pub installation: ModelInstallation,
}
```

固定展示项与依赖：

| id | 组件 | 依赖/实际来源 |
| --- | --- | --- |
| `qwen3-asr-0.6b` | Qwen ASR 0.6B | 必须安装 `qwen3-forced-aligner-0.6b` |
| `qwen3-asr-1.7b` | Qwen ASR 1.7B | 必须安装 `qwen3-forced-aligner-0.6b` |
| `qwen3-forced-aligner-0.6b` | 共享逐词对齐器 | 无模型依赖 |
| `silero-vad` | 说话活动检测 | 当前 Python 包自带权重；以 `installed` 的只读运行时组件展示，不提供删除按钮 |
| `wespeaker-zh` | 中文说话人识别 | 依赖 `silero-vad`；权重由模型管理器下载 |

`silero-vad` 的只读处理是对当前实现的明确兼容决策：官方 `load_silero_vad()` 从已安装包的数据目录加载 ONNX/JIT 文件，而不是从用户目录读取。若要让它成为可移动下载项，必须先把官方 `silero_vad.onnx` 抽出并改用 `OnnxWrapper` 的绝对路径加载；在此之前不能把它伪装成可删除的独立下载。

Hugging Face 下载适配器必须使用固定 `revision`，并把 endpoint 仅作为 `https` host 前缀；`snapshot_download`/`hf_hub_download` 的 `local_files_only`、`allow_patterns`、`endpoint` 和 revision 语义可参考：
<https://huggingface.co/docs/huggingface_hub/package_reference/file_download>

### 下载器实现契约

下载器放在 Rust `ModelManager`，使用 `reqwest 0.12`（`default-features=false`，启用 `rustls-tls`、`stream`）处理 HTTPS、流式响应和 Range；当前工作区没有可复用的 HTTP 客户端，因此这是唯一新增的网络库。不要让 React `fetch` 或 Python sidecar 负责生命周期。实现必须满足：

1. `model_install(model_id)` 读取内置清单，递归解析依赖，去重后按依赖先后放入单一队列；同一 `model_id + revision` 已安装时返回成功快照，不重复下载。
2. 队列一次只运行一个模型 descriptor，避免两个任务同时写同一缓存；设置窗口关闭、主窗口切换或应用进入后台不影响队列。
3. 每个文件写入 `.staging/<install_id>/<model_id>/<revision>/<relative-file>.part`，旁边保存 `manifest.json`（revision、目标大小、已下载偏移、sha256、endpoint、时间）。相对路径必须经过规范化，拒绝 `..`、绝对路径和清单外文件。
4. 先 `HEAD` 获取长度/ETag；已有 `.part` 时从其字节偏移发送 `Range: bytes=<offset>-`。服务器返回 `206` 才追加；返回 `200` 时截断并从零开始，不能把完整响应追加到部分文件。无 Range 且无法重启时进入 `failed`，错误码 `MODEL_RANGE_UNSUPPORTED`。
5. 每个 chunk 检查取消令牌并持久化偏移；`model_pause` 和 `model_cancel` 都停止网络读取。若已有字节，取消后的状态为 `paused`（保留 staging 可恢复）；尚未写入字节则回到 `not_installed`。
6. 所有文件完成后进入 `verifying`：文件数量、允许扩展名、字节数、SHA-256、revision 和 license 元数据逐项匹配。失败进入 `corrupt`（校验错误）或 `failed`（网络/IO），保留 staging 和恢复信息，不得显示 `installed`。
7. 校验成功后执行离线加载测试：Qwen 显式加载 ASR 目录和 ForcedAligner 目录；Speaker 显式加载 WeSpeaker 目录和 VAD 运行时。测试进程必须设置 `HF_HUB_OFFLINE=1`、`TRANSFORMERS_OFFLINE=1`，并拒绝任何网络回退。
8. 离线测试成功后把临时目录 `rename` 到最终 `<model_root>/<model_id>/<revision>`，再原子更新 `installations.json` 为 `installed`。最终目录和 staging 必须在同一文件系统；旧的 installed revision 不得在新 revision 验证前被删除。
9. `model_verify(model_id)` 可重新哈希已安装目录并重复离线加载；文件被用户改动时转为 `corrupt`。`model_remove` 只允许删除已安装项，且若仍有已安装 Qwen 依赖该 Aligner，则返回 `MODEL_DEPENDENCY_IN_USE`。
10. 更改 `model_root` 时先在新根创建 staging、复制并校验所有已安装项，成功后写入 prefs 并切换 manager；旧根只在新根完全可用后删除。磁盘不足在下载前用清单总大小和 `statfs` 预检，运行中不足进入 `failed` 并保留 staging。

状态转换固定为：

```text
not_installed -> queued -> downloading -> verifying -> installed
                         |                    |           |
                         v                    v           v
                      paused                corrupt     corrupt
                         |                    ^
                         +---- queued -------+
downloading --(IO/network)--> failed --(retry)--> queued
```

`model_cancel` 不新增 `cancelled` 状态；它按上面的 `paused/not_installed` 规则落态。`model_resume` 只能从 `paused` 或 `failed` 进入 `queued`；`model_install` 可以从 `not_installed`、`corrupt` 重新建 staging，但不能覆盖正在 `downloading/verifying` 的同一项。

### 模型命令与事件

| 命令 | 参数 | 返回/前置条件 |
| --- | --- | --- |
| `model_catalog` | 无 | `OperationResult<Vec<ModelDescriptorWithInstallation>>`；清单缺失/非法时失败 |
| `model_install` | `model_id` | 依赖入队，返回 `Vec<ModelInstallation>` 的快照；不等待下载完成 |
| `model_pause` | `model_id` | 仅 `queued/downloading`；返回该项快照 |
| `model_resume` | `model_id` | 仅 `paused/failed`；保留 staging 继续 |
| `model_cancel` | `model_id` | 取消当前网络任务，按取消规则落态 |
| `model_verify` | `model_id` | 重哈希并离线加载；成功为 installed，校验失败为 corrupt |
| `model_remove` | `model_id` | 依赖保护后删除 installed；失败不改变目录 |
| `model_reveal` | `model_id` | 只接受 catalog 中的已安装 id；Rust 通过受限 opener 打开 model root，禁止任意路径 |
| `doctor_run` | 无 | `OperationResult<DoctorReport>`；纯本地检查并广播 `dl://doctor-result` |
| `diagnostics_reveal_logs` | 无 | 只打开 App 固定日志目录；不接收路径参数 |

事件 payload 固定为：

- `dl://model-progress` → `ModelDownloadProgress`（不含绝对路径、token、音频文本）。
- `dl://model-state` → `ModelInstallation`（错误消息只允许用户可读摘要）。
- `dl://preferences-changed` → `{ changed_keys: string[] }`；窗口收到后重新调用 `preferences_get`，不广播完整路径。

## 5. ForcedAligner、ASR 和 Speaker 的真实离线接线

### ASR

本机 `sidecars/asr/.venv` 的 `mlx-qwen3-asr==0.3.5` 探针确认：`Session(model: str)` 接受本地目录；`ForcedAligner(model_path: str)` 和 `ForcedAligner.align(audio, text, language)` 可用；`Session.transcribe(..., forced_aligner=...)` 会把每个词的 `text/start/end` 放到 `result.segments`。因此不需要把段落时间伪装成逐词锚点。

实现改动契约：

- `TranscribeConfig` 增加 `model_dir: PathBuf`、`aligner_dir: PathBuf`；`transcribe_start` 只接受 catalog id，Rust 从 `ModelManager` 解析两个已安装目录，禁止前端传任意路径。
- `SidecarCommand::Transcribe` 增加两个绝对路径字段（序列化到本地 JSONL，不返回前端）：`model_dir`、`aligner_dir`。
- `transcriber.py` 的 session cache key 改为绝对模型目录；`Session(model=model_dir)`，并按目录缓存 `ForcedAligner(model_path=aligner_dir)`，调用 `session.transcribe(..., return_timestamps=True, forced_aligner=aligner)`。
- 运行前设置 `HF_HUB_OFFLINE=1`、`TRANSFORMERS_OFFLINE=1`；若给定目录不存在、缺 `config.json`/safetensors 或对齐器加载失败，返回明确 fatal 错误，不能退回 repo id。
- 每个返回 segment 必须有非空 text、`end > start`；pipeline 继续把浮点秒一次性换成源采样域整数。`WordAnchor`/SQLite 结构无需改变。

### Speaker

- `DiarizeConfig` 增加 `vad_model_dir`、`speaker_model_dir`；Rust 从 manager 解析 `silero-vad`（当前为 bundled runtime 校验结果）和 `wespeaker-zh` 的绝对目录。
- `wespeaker.load_model(speaker_model_dir)` 传本地目录；不再传 `"chinese"`，避免自动下载。WeSpeaker 官方 Python API 也支持把包含 `config.yaml`/`avg_model.pt` 的目录作为本地模型。
- Silero 当前包的 `load_silero_vad()` 只读包内数据文件；保持为 bundled runtime，或在后续把 `silero_vad.onnx` 放入 `silero-vad/<revision>` 后用 `silero_vad.utils_vad.OnnxWrapper(absolute_path, force_onnx_cpu=True)`，两者都必须设置离线环境变量。
- 声纹向量继续只写当前项目 SQLite；不进入 `ModelInstallation`、日志、诊断、事件或导出物。

### 转录缺模型拦截

`transcribe_start` 先根据选择模型检查 ASR 与 Aligner 为 `installed`，Speaker 任务检查 Speaker 依赖。缺失时返回 `MODEL_NOT_READY`（包括 `missing_model_ids`），不创建 task、不改项目；React 打开安装弹层，项目当前状态保持不变。

## 6. 首次启动引导与七个设置页

### 引导状态机

新增轻量 `OnboardingState`（`version`, `completed`, `step`）作为 `onboarding_get` 的返回 DTO；最终完成状态仍写入 `AppPreferencesV1`。版本从 1 开始，未来版本大于已完成版本时重新进入，不删除模型和最近项目。

三步固定行为：

1. **本地处理**：说明原始媒体只读引用、音频/声纹不默认上传、处理在本机完成；提供“继续”和“跳过引导”。
2. **模型准备**：调用 `system_profile`、`model_catalog`，按内存显示推荐模型、体积、最低内存和可选说话人组件。点击“安装推荐”调用 `model_install` 并留在后台；点击“稍后”不安装。
3. **开始项目**：提供新建/打开项目动作；`onboarding_complete` 先写完成状态再进入项目库。下载可以继续，关闭应用后下次启动从 staging 恢复为 `paused`。

新增命令：

- `onboarding_get` → `OperationResult<OnboardingState>`；首次无记录返回 `{version:1, completed:false, step:1}`。
- `onboarding_complete` → 写 `completed=true, step=3`，可带 `default_asr_model`；不触碰项目。
- `onboarding_reset` → 仅清除完成标记并回到 step 1；不删除模型、项目和最近记录。

跳过模型安装后，项目库显示低干扰的“尚未安装转录模型”行；转录按钮再次触发 `MODEL_NOT_READY` 时打开相同安装弹层。引导和安装 UI 必须能在没有项目时工作。

### 七个设置页的实际接口

1. **通用**：`preferences_get/update`；主题、恢复上次项目、时间码精度、转录分区底色、CJK 间距和默认字幕行长度。所有布尔值用开关，有限枚举用单选/下拉。
2. **快捷键**：只读展示固定快捷键；校验 `Cmd+,` 菜单和编辑器键盘处理已接通，不做自定义改键。
3. **默认字幕样式**：编辑/保存 `AppPreferencesV1.default_subtitle_style`；页面显式标注“只影响新项目”；“应用到当前项目”在有项目时调用现有 `subtitle_style_set`，无项目时禁用并说明原因。
4. **本地模型**：`model_catalog`、四个操作按钮和实时 `dl://model-progress/state`；展示依赖、大小、最低内存、许可证、当前状态、失败原因、继续/重试/校验/删除/打开目录。
5. **隐私**：只读产品承诺（无默认遥测、无音频/声纹上传、Agent 数据包必须预览确认）和 `diagnostics_reveal_logs`/数据目录动作；不暴露完整私人路径。
6. **诊断**：`doctor_run` 返回短状态表、模型完整性、ffmpeg/ffprobe/libass、剩余空间、runtime 版本；支持复制脱敏文本和 `diagnostics_reveal_logs`。运行检查不得联网。
7. **关于**：只读版本/构建号、Tauri/Engine/sidecar 版本、第三方许可、模型许可和本地处理边界。

设置页没有“稍后接入”按钮：每个可点击动作必须改变真实状态或明确为只读/打开本地目录。

## 7. 诊断和脱敏

```rust
pub struct DoctorReport {
    pub generated_at: String,
    pub healthy: bool,
    pub architecture: String,
    pub os_version: String,
    pub memory_bytes: u64,
    pub free_model_bytes: u64,
    pub ffmpeg_available: bool,
    pub ffprobe_available: bool,
    pub libass_available: bool,
    pub asr_runtime_ready: bool,
    pub speaker_runtime_ready: bool,
    pub model_installations: Vec<ModelInstallation>,
    pub checks: Vec<DoctorCheck>,
}

pub struct DoctorCheck {
    pub id: String,
    pub ok: bool,
    pub detail: String,             // 已脱敏的短说明
    pub suggested_action: Option<String>,
}
```

`doctor_run` 复用 CLI 的 ffmpeg/libass、runtime import、模型哈希和离线加载检查，但不返回原始 `Diagnostic.cause`。脱敏规则固定：

- 将 `home_dir`、`app_data_dir`、`model_root`、项目 root、日志文件名替换为 `<HOME>`、`<APP_DATA>`、`<MODEL_ROOT>`、`<PROJECT>`、`<LOG_FILE>`；只保留文件扩展名和错误类别。
- 删除媒体绝对路径、URL query/token、音频文本、说话人 id、声纹向量、完整模型缓存路径和 Python traceback 中的参数值。
- `DoctorReport` 和 `dl://doctor-result` 只含上述短说明；复制报告先由 Rust 生成同样脱敏后的纯文本。
- `diagnostics_reveal_logs` 无路径参数，只打开当前 App 固定日志目录；不能成为任意路径打开器。若要导出诊断包，必须先在 UI 预览脱敏清单，再由用户确认；本轮不自动发送。

诊断页显示“运行时已安装”和“权重已安装/已校验/已真实加载”三种不同事实，不能把 `import double_love_asr` 当成模型就绪。

## 8. 测试夹具与验收门槛

### Rust/模型管理测试

- 偏好：无文件默认值、v1 读取、未来字段忽略、坏 JSON 备份并恢复、并发 update 串行化、双窗口变更事件。
- 最近项目：20 条裁剪、按时间排序、不存在路径 `exists=false`、forget 不删除文件、project open/create 自动 upsert。
- 本地 HTTP fixture（不依赖公网）：固定 `HEAD`/`GET`/`206 Range`、ETag、断连、404、500、错误长度、错误 SHA、非法相对路径；记录每个请求的 Range，断点恢复测试必须看到正确 offset。
- 状态机：依赖先排队、暂停/恢复、取消保留 staging、IO 失败进入 failed、hash 失败进入 corrupt、校验前不替换旧 installed、原子 rename 后才变 installed、磁盘不足预检、Aligner 删除保护。
- model root 迁移：目标盘空间不足/复制中断时旧 root 保持可用；成功后 prefs 与安装状态一次性切换。
- 测试构建允许 `http://127.0.0.1` fixture；生产命令仍拒绝非 HTTPS endpoint。

### ASR/Speaker 离线测试

- Python 单测用 fake `Session`/fake `ForcedAligner` 记录传入的绝对目录，并断言 `align()` 被调用、每个词 `end > start`；没有 aligner 或返回段落级结果时任务失败而不是写伪词锚点。
- 使用当前已安装 `mlx-qwen3-asr==0.3.5` 做一次 API smoke：本地路径 `Session(model=...)`、`ForcedAligner(model_path=...)`、`forced_aligner=` 参数均可加载。
- Speaker fake runtime 断言 `wespeaker.load_model(<local dir>)`；VAD 不得触发网络。真实权重加载另做 Apple Silicon 人工门槛。
- Tauri/CLI 都设置 `HF_HUB_OFFLINE=1`、`TRANSFORMERS_OFFLINE=1`；可以用阻断网络的 fixture 确认不产生请求。

### Onboarding/设置/窗口

- 全新 Store：三步继续、跳过、模型下载中关闭应用再开、onboarding reset、缺模型点击转录五条路径。
- 设置窗口：侧栏与 `Cmd+,` 复用同一 label；关闭后再次打开不产生第二窗口；主/设置窗口同时监听偏好和模型事件。
- 七页无占位按钮；每个更新在重启后可见；默认字幕样式只影响新项目，显式应用才改变当前项目。
- 键盘、焦点、VoiceOver label、`aria-live` 和减少动效在主窗口与 settings 窗口分别检查。

### 发布/人工验收分层

- 保留现有 `cargo test --workspace`、`pnpm --dir studio test/lint/build` 和 Tauri 调试构建作为代码门槛。
- `scripts/verify-release-runtime.sh` 仍只门禁随 App 分发的 ffmpeg/libass 与可重定位 Python runtime；模型权重由安装器管理，不能把开发机缓存当发布证明。
- 另做干净 Apple Silicon macOS 15+ 机器验收：首次启动、下载/暂停/恢复/校验、无网络真实加载、模型目录迁移、VoiceOver、`Cmd+,` 和打开日志目录。
- 真实模型、真实媒体、Premiere/Resolve、签名/公证分别记录，不能用单元测试或调试包启动替代。

## 9. 已知风险与处理边界

- **上游 API 漂移**：ASR 与 WeSpeaker 依赖已 pin；模型清单 revision/hash 和 `requirements.txt` 变更必须同时更新离线 smoke fixture，不能自动跟随上游。
- **Silero VAD 可移动性**：当前包内加载器没有外部 path 参数，故本轮把它作为 bundled runtime 只读组件；若产品必须让用户单独删除/迁移 VAD，先完成 `OnnxWrapper` 绝对路径适配再开放操作。
- **开发/发布环境混淆**：开发期 PATH Python fallback 不能进入发布包；`model_verify` 必须显示“runtime import / weights verified / real loaded”三种状态。
- **路径隐私**：路径只在 Rust 内部使用；事件、诊断复制文本、模型错误消息和 Agent 数据包均不得带完整路径或媒体文本。
- **下载中断**：任何未完成目录只能留在 `.staging`，应用崩溃重启时扫描 `manifest.json` 并恢复为 paused；最终目录只有校验和离线加载都通过后才可见。

### 一手资料

- Tauri 窗口与 capability：<https://v2.tauri.app/learn/window-customization/>
- Tauri 原生菜单事件与 accelerator：<https://v2.tauri.app/learn/window-menu/>
- Tauri Store（Rust `StoreExt` 与 app-data 路径）：<https://v2.tauri.app/plugin/store/>
- Hugging Face 固定 revision、Range/cache、endpoint、`local_files_only`：<https://huggingface.co/docs/huggingface_hub/package_reference/file_download>
- WeSpeaker 本地模型目录 API：<https://github.com/wenet-e2e/wespeaker/blob/master/docs/python_package.md>
- Silero VAD 当前加载器（包内 ONNX/JIT 数据文件）：<https://github.com/snakers4/silero-vad/blob/master/src/silero_vad/model.py>
