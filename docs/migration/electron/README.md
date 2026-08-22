# Tauri → Electron 迁移：阶段账本与文档索引

> **归档状态（Phase 5F 已完成）**：本目录只保存迁移决策、旧路径、旧命令与对照证据；不是现行开发、打包或发布指导。

当前 Electron 指南：

- [架构](../../studio/architecture.md)
- [打包](../../studio/packaging.md)
- [发布](../../studio/release.md)
- [升级与回退](../../studio/upgrade-rollback.md)
- [排障](../../studio/troubleshooting.md)

范围：仅 Studio 桌面端。Web/PWA 与 `double-love` CLI 对外行为不变。首个 Electron 正式版为 Studio 0.2.0（macOS 15+ / arm64，bundle id `space.ahua.doublelove.studio` 不变）。

## 回退锚点

- 最后可构建 Tauri 的提交：`cf3831a^`（当前解析为 `c6d43fb`）。
- 删除旧容器、完成 Electron-only 收口的提交：`cf3831a`。

`cf3831a~1` 与 `cf3831a^` 都表示删除提交的父提交；不要把 `cf3831a`
本身当作 Tauri 构建点。数据恢复步骤见现行[升级与回退指南](../../studio/upgrade-rollback.md)。

## 归档文档索引

| 文档 | 内容 |
| --- | --- |
| [capability-matrix.md](capability-matrix.md) | Tauri 能力矩阵：全部命令/事件/窗口/对话框/Store/协议/资源/脚本/CI，按 在用 / UI 未用 / 占位 / 平台能力 分类 |
| [behavior-matrix.md](behavior-matrix.md) | 用户可见行为矩阵与迁移对照 |
| [data-contract-baseline.md](data-contract-baseline.md) | 数据契约基线：偏好、模型状态、SQLite schema 1–10、TimelineIR、导出 golden、CLI JSON、sidecar 协议、fixture 计划 |
| [debt-heatmap.md](debt-heatmap.md) | 技术债热力图（按严重度排序） |
| [adr-0001-target-architecture.md](adr-0001-target-architecture.md) | 目标架构 ADR（main/preload/renderer + Rust host sidecar） |
| [behavior-change-register.md](behavior-change-register.md) | 已批准行为变化登记 |

## 阶段账本

| 阶段 | 状态 | 放行条件 | 回退 |
| --- | --- | --- | --- |
| 1 审计、基线与蓝图 | 已完成 | 能力矩阵覆盖全部登记命令/事件/资源；失败/skip 唯一分类；ADR 经主会话确认 | 只含审计/脚本/fixture 元数据，直接 revert |
| 2A 回归护栏 | **已完成** | 合成偏好/模型 fixture；schema 1–10 构造、迁移与幂等测试；manifest camelCase 冻结；ts-rs/CLI contract 脚本；严格工具门禁 | 回退本阶段 fixture、测试、脚本与 CI flag；不触碰用户数据 |
| 2B Electron 骨架 | **已完成（Batch A host + Batch B Electron 壳）** | 窗口/host 握手/settings 单例/边界 smoke 全过；Tauri 仍完整可用 | 删除/回退骨架，不触碰用户数据 |
| 3A desktop service 与 host 分派基础设施 | **已完成** | service 持有桌面状态容器且不依赖 Tauri/Electron；host 支持无业务命令的通用 invoke 与独立事件 envelope | Tauri adapter 保留，revert service/host 基础设施批次 |
| 3B 安全边界与平台基础设施 | **已完成** | renderer 无 Node 能力；写能力需 path grant；媒体协议只读当前项目资产 | Tauri adapter 保留，revert 当前批次 |
| 3C React renderer 目录整理 | **已完成** | 纯移动至 `studio/src/renderer/`；Tauri/Electron 构建与测试行为不变 | 直接 revert 目录移动与配置路径更新 |
| 3D renderer 平台接缝 | **已完成** | renderer 运行时选择 Electron/Tauri/preview；Electron host 错误归一为既有 `OperationResult.failed`；本地文件无 bridge 时 fail closed | Tauri adapter 保留，revert renderer 平台接缝批次 |
| 3 平台基础阶段（3A–3D） | **已完成** | service/host、安全边界、renderer 目录与平台 adapter 均已就位；Phase 4 可按纵向切片迁移业务命令 | 保留 Tauri adapter，按 3D→3A 逆序回退 |
| 4 纵向业务切片迁移（7 片） | **已完成（Slice 1–7）** | 每片：Electron E2E 正常+失败/取消/恢复路径；Tauri 对照一致 | 每片独立提交，只 revert 当前切片 |
| 5 默认 Electron、打包发布、清理 | **工程收口已完成（5A–5D、5F）；首个真实签名候选仍须运行受保护工作流验收** | 发布工作流只写草稿；真实签名、公证与干净机器验收通过后人工公开 | 最后 Tauri 构建点 `cf3831a^`；Electron-only 收口 `cf3831a` |
| 5A 打包与资源 | **已完成** | ASAR、host/schema/runtime extraResources、Fuses 与 package smoke | 回退 5A 资源和 builder 接线 |
| 5B 更新器 | **已完成** | 两次确认、本地更新链证明、草稿/prerelease 不可见 | 手动安装已保留的旧版 |
| 5C 一次性数据备份 | **已完成** | 首次 Electron 写入前创建偏好与项目备份且不覆盖 | 复制备份恢复，不改写备份原件 |
| 5D 旧桌面容器删除 | **已完成** | renderer/Cargo/npm/CI/基线脚本去旧依赖；Electron package + smoke + 全量 Playwright；workspace 依赖树无旧容器 crate；残留只在本目录历史归档 | 回退到 `cf3831a^`，不覆盖用户一次性备份 |
| 5F 发布工作流与最终文档 | **已完成** | tag/version 精确门禁；受保护 environment；签名、公证、候选验证；只上传草稿；现行文档齐全 | 工作流不公开 Release；按现行回退指南处理 |
| 6 全量验收与回退演练 | 未开始 | 见计划完成判定 | 文档化手动回退 |

## 历史基线证据（2026-08-22，本机 macOS 15.7.7 arm64，Node 24.14.1 / pnpm 10.33.0 / Rust 1.97.1）

| 门禁 | 结果 |
| --- | --- |
| Web `pnpm lint` / `pnpm test` / `pnpm build` | 通过；99 个测试全过（5 个文件） |
| Studio `pnpm --dir studio lint/test/build` | 通过；45 个测试全过（5 个文件） |
| `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过 |
| `cargo test --workspace --locked -- --nocapture` | 通过：engine 146 过 + 1 故意 ignored（`export::xmeml::tests::regenerate_golden_file`，需手工 `--ignored`）；CLI 9 E2E（cli 4 + export_roughcut 1 + import_media 3 + transcribe 1）；project 4；sidecar_mock 2；Tauri 16；baseline 另汇总测试自行提前返回的 `skip:` 行 |
| Python sidecar 测试 | ASR 2 过；speaker 2 过；`py_compile` 通过 |
| Tauri debug app | 通过：`pnpm exec tauri build --debug` → `target/debug/bundle/macos/Double Love Studio.app` |

注：首次曾误用 `pnpm tauri:build -- --debug`（额外 `--` 把 `--debug` 传给 cargo 报 unexpected argument）。这只是命令写法错误，不是基线失败；正确写法为 `pnpm exec tauri build --debug`（与 CI `pnpm tauri build --debug` 等价，见 `.github/workflows/studio-quality.yml:68`）。

## 基线脚本

`scripts/migration/baseline.sh`：幂等采集环境摘要、门禁命令与退出码、测试/skip 摘要、包体积到忽略目录（默认 `evidence/migration-baseline/`，已 gitignore）。完整模式构建 release host 与 Electron 输出，再执行 `pack:dir` 和 `package-smoke.sh`；`--fast` 将这四项记为 `SKIP`，结果为 `verdict=PASS_FAST`、`completeness=INCOMPLETE`，不能冒充完整 PASS。任一必需门禁（含 ffmpeg 精确 `ass` filter）失败则以非零退出。raw log 会把实际 repo root 与 HOME 替换为占位符，Python bytecode cache 也留在该次 evidence 目录内；包体积记录 Web `dist`、Electron `studio/out` 与 unpacked `.app`。

## Phase 2A 已交付 fixture 与门禁

全部 fixture 均为 synthetic、privacy-safe 文本或运行时生成代码，不含真实用户数据库、媒体或模型权重；仓库不跟踪生成的 SQLite：

1. 桌面 service 偏好：`crates/double-love-desktop-service/tests/fixtures/preferences/{v1,partial-v0,corrupt}.json`；v1 解码与 fixture 的完整持久化对象相等，partial-v0 迁移与独立完整预期对象相等，损坏 JSON 分类为 decode failure。
2. 模型状态：`crates/double-love-engine/tests/fixtures/model-installation-states.json` 描述 `installed`、`paused`（保留 `.part`）与下载中 staging 中断；安装 envelope schema 以字面量 1 冻结，并断言生产常量仍为 1；测试按文本生成最小目录并通过 `ModelManager` 验证恢复。
3. 项目库：`storage.rs` 以稳定 SHA-256 分别冻结 `MIGRATION_V1`–`V10` 文本，通过后才运行时生成各版本最小 `.doublelove/project.sqlite`；逐个打开两次，断言 migration ledger 恰为 1–10 且 current=10。migration SQL 与生产 schema 均未修改。`tests/project.rs` 同时冻结 manifest camelCase 与 schemaVersion=1。
4. 绑定/CLI contract：`scripts/migration/check-bindings-contract.sh` 先快照 `bindings/`，运行现有 ts-rs `export_bindings` cargo tests并确认再生成幂等，随后运行 CLI `--json` contract 测试；因此开发新契约时无需暂存或提交生成文件也能执行门禁。XMEML 的 `regenerate_golden_file` 保持故意 ignored，不默认执行。
5. 严格工具门禁：唯一开关 `DOUBLELOVE_REQUIRE_TEST_TOOLS=1` 令依赖 ffmpeg/ffprobe/python3/libass 的 Rust 测试在工具缺失时失败；未设置时保留本地 self-skip。`.github/workflows/studio-quality.yml` 对 push 与 pull request 运行，在存在时优先 `ffmpeg-full`，并以不输出绝对路径的 prerequisite 检查精确要求 `ass` filter。

## Phase 2B Batch A 已交付 host 骨架

`crates/double-love-desktop-host` 是无 Tauri/Electron 依赖的长期运行 Rust 二进制与 library；Batch A 当时只提供 `handshake`、`health`、`shutdown`，Phase 3A 已在其上新增 service 与通用分派，仍不含业务命令。stdin/stdout 只承载 4 字节大端长度前缀与 UTF-8 JSON frame，单帧上限 64 MiB；EOF 正常退出，日志只允许写 stderr。

Protocol v1 使用版本化、可关联的单层 envelope。request 为 `{"v":1,"id":"<client-id>","method":"handshake","client":"electron-main","client_protocol":1}`（`health`/`shutdown` 同样携带 `v` 与 `id`）；response 回显同一 `id`，为 `{"v":1,"id":"<echoed-id>","status":"ok","result":{"type":"hello|health|shutdown",...}}` 或 `{"v":1,"id":"<echoed-id>","status":"error","error":{"code":"...","message":"..."}}`。无法解析的 frame 使用 `id:"unknown"`；缺失/错误版本和缺失/空白 id 会被拒绝。hello data 固定返回 `protocol`、`host_version`、`engine_version`、`capabilities`。Rust tagged union 同时生成 `bindings/host-protocol/*.ts` 与 `bindings/host-protocol/schema/*.schema.json`，contract 脚本检查再生成前后的完整目录快照一致。

v1 的 `u64`/`u32` 在 wire 上都使用 JSON number；ts-rs 的 `bigint` 仅是既有生成绑定的类型层注解，不改变 JSON runtime。Node/renderer runtime boundary 继续按既有规则转换并校验安全整数范围，禁止直接把 JS `BigInt` 交给 JSON 序列化。

## Phase 2B Batch B 已交付 Electron 壳骨架

`studio/electron.vite.config.ts` 在不移动现有 React renderer、不修改原 `vite.config.ts`/`dist/` Tauri 构建路径的前提下，新增 `out/main`、CJS sandbox preload 与 `out/renderer` 构建。main 创建 1440×900 主窗口和 close-to-hide 的 760×580 settings 单例；原生 `Cmd+,` 菜单与受限 preload IPC 共用同一 settings helper。窗口启用 context isolation、sandbox、禁 Node、禁外部导航/新窗口/权限请求，并持有 second-instance 单例锁。

main 监督唯一 Rust host，按 64 MiB 上限实现四字节大端长度前缀 JSON framing；启动时用生成的 `HostResponse` JSON Schema（Ajv）校验 handshake，确认 protocol 1 并记录 capabilities。受限 `window.doubleLove` 只暴露冻结的 `hostHealth()` 与 `openSettings()`。退出先发 shutdown，超时再 kill；崩溃后标记 unhealthy，不重放业务请求。开发时 host 从仓库根 `target/debug/double-love-desktop-host` 解析，schema 从仓库根 `bindings/host-protocol/schema` 读取；打包时 host 从 `process.resourcesPath` 解析。正式打包的 host/schema resource 复制、`electron-builder`、updater 与 Electron Fuses 加固均属于 Phase 5，本批只锁定依赖而不接线。

Playwright Electron smoke 从 `out/main` 启动本地文件 renderer，使用临时 userData 且不访问网络，覆盖单主窗口标题、renderer Node 隔离、host health、`Cmd+,` settings 单例、close-to-hide/reopen 与 host 持续健康。CI 在保留 Tauri debug reference build 的同时构建 host、Electron 输出并实际运行该 smoke。

## Phase 4 Slice 1 已交付应用壳、偏好与引导

`double-love-desktop-service` 已无 Tauri 依赖地迁入 schema v1 偏好、v0 合并迁移、损坏备份恢复、最近项目、系统画像与引导状态；`preferences.json` 继续使用唯一顶层键 `app_preferences`，写入权限为 `0600`。默认 ASR 推荐仍按 16 GiB 内存阈值选择，模型目录仍为应用数据目录下的 `models`。

host 现通过统一 `service::register_commands` 注册 Slice 1 命令，偏好变更继续广播 `dl://preferences-changed`。只读模型清单按当前偏好目录惰性初始化 `ModelManager`，内置 `silero-vad` 自动标记为已安装；模型目录变更先复制、校验并切换已安装模型，失败时不持久化新目录。本切片不迁移下载生命周期，也未修改旧容器目录参考实现。

对照证据复用 `crates/double-love-desktop-service/tests/fixtures/preferences/{v1,partial-v0,corrupt}.json`：service 测试断言完整 v1 与冻结的 v0→v1 结果逐字段相等，并覆盖磁盘损坏恢复。host 集成测试以临时 `--app-data-dir` 覆盖默认往返、更新与事件、非法 endpoint、模型目录迁移失败保持旧偏好、最近项目 20 条上限/forget 错误、引导 complete/reset、arm64 系统画像及内置模型状态。Electron Playwright 使用同一临时 userData 覆盖偏好事件与落盘、重启持久化、启动损坏恢复、非法 endpoint、引导 reset 和系统画像。

Slice 1 门禁已通过：严格工具模式的 `cargo test --workspace --locked`（含未改动 Tauri 参考实现 18 项测试）、workspace clippy `-D warnings`、bindings contract、Studio lint/test/build、Electron build 与全部 8 项 Playwright、根 Web lint/test/build、`git diff --check`。

## Phase 4 Slice 2 已交付项目生命周期与历史导航

host-neutral service 已按 Tauri 参考实现注册 `project_create`、`project_open`、`project_revision`、`project_history`、`project_restore_revision`、`edit_undo`、`edit_redo`，并仅为本切片历史测试与当前 UI 写路径同时接入等价的 `canvas_get/set`、`subtitle_style_get/set`。项目打开失败不会替换当前项目；成功安装前会从实际 store revision 与最多 10,000 条可恢复历史初始化 undo 导航。打开/创建继续记录最多 20 条最近项目，写偏好或创建时写默认字幕样式失败只追加 Tauri 同码 warning，不把已打开项目降级为失败。

新建项目仍严格遵循 Tauri 的无条件默认字幕样式写入：即使偏好样式等于 engine 默认值，也会生成 revision 1，首条可恢复历史为 `subtitle_style_set`，没有做“相同则跳过”的 Electron 特化。`project_history` 默认 limit 80；restore/undo/redo 都由 engine `restore_revision` 新建 revision，不改写旧历史。项目替换与当前项目操作由 project-slot 锁串行化，并统一遵循 project slot → store → history navigation 的加锁顺序，避免旧项目操作在新项目安装后继续执行或覆盖新项目的历史导航。

Rust host 集成覆盖创建、打开、同 id 重开与进程重启、revision/history、canvas mutation、undo/redo、restore 新 revision、未打开、非法创建/打开路径与最近项目；service 并发回归测试覆盖进行中的项目操作会阻塞项目替换，且替换完成后的项目与历史一致。Electron Playwright 通过一次性目录 grant 创建临时项目，验证持久化默认字幕样式及样式写入 revision/history，再覆盖 canvas undo/redo；随后使用现有 `double-love` CLI 导入合成媒体并追加主轨，只验证 host 可观察到外部 revision/history，不定义外部写入后的 undo 冲突策略，最后覆盖 restore、失败打开保留旧状态与重启同 id 打开。

Slice 2 未修改旧容器目录、Web/PWA、SQLite schema/migration、manifest 或导出格式；Tauri 继续作为行为参考。门禁证据：workspace fmt 与 clippy `-D warnings` 通过；严格工具模式 workspace Rust 测试全过（engine 148 过、1 个既有 golden regeneration ignored；host Slice 2 集成 1 过；Tauri 参考测试 18 过）；bindings contract 通过；Studio lint、88 项 Vitest、Studio Vite build、独立 `tsc -b` 与 Electron build 通过；全部 9 项 Playwright 通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 4 Slice 5 已交付模型生命周期、转录编辑与单素材粗剪

host-neutral service 已按 Tauri 参考实现注册 `model_install/pause/resume/cancel/verify/remove/reveal`、`doctor_run`、`diagnostics_reveal_logs`、`transcribe_start`、`task_cancel`、`transcript_get`、`edit_omit/restore`、`roughcut_preview` 与 `export_roughcut_apply`。模型下载继续使用 reqwest blocking 客户端、依赖优先单队列、同一下载锁、安全相对路径、`.part`/staging、Range 恢复及 206 `Content-Range` 校验、清单字节上限、SHA-256、原子安装和原状态机；进度与状态分别广播 `dl://model-progress` / `dl://model-state`，事件不含绝对路径。共享依赖仍被已安装模型使用时删除返回 `MODEL_DEPENDENCY_IN_USE`。

Electron 的 reveal 命令只让 service 返回其自身解析的模型目录或日志目录；main 从 host 响应提取路径后调用 `shell.openPath`，从不接受 renderer 路径，并只向 renderer 返回保留 status/diagnostics、`data:null` 且不含路径的操作结果。main 启动 host 时显式注入 bundled resource 根；service 仍优先遵循 `DOUBLELOVE_ASR_DIR`，开发期回落到仓库 `sidecars/asr`。正式 `transcribe_start` 保持 `mock=false`、30 秒切块、模型状态门禁和项目内日志；service 事件边界把进度自由文本中的项目目录替换为 `<PROJECT>`、模型/对齐器及 sidecar 目录替换为 `<MODEL>`，不改 task id、计数或 `dl://task-state`；只有 debug host 的显式 `--test-transcribe-mock` 集成配置可在自动化测试启用 mock sidecar。

Rust service 的本机 HTTP fixture 使用临时合成清单数据（不下载真实权重、无外网），覆盖依赖顺序安装、进度/状态事件、暂停保留 staging、零字节取消、Range 续传、恢复安装、哈希损坏转 corrupt、verify、依赖删除保护和 doctor。host Slice 5 集成测试另覆盖模型/日志路径返回、合成媒体导入、mock 转录进度与终态、路径型 sidecar 错误脱敏、默认 120ms omit、restore、preview 不写、apply 写 XMEML/SHA-256/outputs/export history、`ROUGH_CUT_EMPTY` 阻断，以及取消候选不切换 active transcript。Electron Playwright 使用预置的合成 installed 状态（不含权重）和 test-only mock host 配置，覆盖 reveal 返回值不含路径，以及同一转录、脱敏事件、编辑、export grant、文件/SHA-256、空剪阻断与取消路径；因生产 host 的偏好 endpoint 校验按设计不允许非测试编译的 HTTP，Electron E2E 不伪造本地下载，完整下载生命周期由 Rust fixture 覆盖。

Slice 5 未修改旧容器目录、Web/PWA、模型清单 JSON、偏好 endpoint 校验、SQLite schema/migration、sidecar 协议或导出格式；Tauri 继续作为行为参考。门禁证据：workspace fmt 与 clippy `-D warnings` 通过；严格工具模式 workspace Rust 测试全过（engine 148 过、1 个既有 golden regeneration ignored；desktop service 17 过；host Slice 5 集成 2 过；Tauri 参考测试 18 过）；bindings contract 通过；Studio lint、89 项 Vitest、Studio Vite build、独立 `tsc -b`、Electron build 与 host build 通过；Slices 1–5 全部 13 项 Playwright 通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 4 Slice 6 已交付说话人分离与项目内身份

host-neutral service 已按 Tauri 参考实现注册 `speaker_list`、`speaker_name_proposals`、`speaker_agent_payload_preview`、`speaker_name_confirm`、`speaker_merge_confirm`、`speaker_diarize_start` 与 `speaker_diarization_get`。列表、名称候选、显式确认错误码、姓名/合并 revision、合并后旧身份的 `merged_into` 归档语义和分离结果均直接复用 engine；姓名与合并在 `confirmed:false` 时统一返回 `SPEAKER_CONFIRM_REQUIRED`，不会写项目。

`speaker_diarize_start` 继续从当前偏好的模型根经 service 模型状态解析已安装的 `wespeaker-zh`，未就绪返回 `MODEL_NOT_READY`；VAD 仍使用 bundled Silero 标识。speaker sidecar 定位顺序为 `DOUBLELOVE_SPEAKER_DIR` → 注入的 resource 根 → 开发仓库 `sidecars/speaker`，日志只写当前项目 `.doublelove/logs`。生产 runtime 默认且始终使用 `mock:false`；host 在无显式 `--test-transcribe-mock` 时会在启动阶段清除继承的 `DOUBLELOVE_ASR_MOCK` 与 `DOUBLELOVE_SPEAKER_MOCK`，避免 Electron main 环境绕过 runtime 配置。只有 debug host 的显式 `--test-speaker-mock`（Electron E2E 对应 `--double-love-e2e-speaker-mock`）可由 speaker 配置重新启用既有确定性 mock，不改变 engine/CLI 或 sidecar 协议。

Electron 的 Agent 预览边界会在返回前仅替换 payload 字符串中当前项目根和当前项目全部媒体源路径（含可解析的 canonical 形式）为 `<PROJECT>` / `<MEDIA>`；speaker id、发言选择、条数/字符上限和 instruction 除路径替换外保持 engine 原样。预览仍只包含请求的匿名说话人发言，不会附加音频、其他说话人的文字、项目上下文或任何外部调用。

说话人隐私不变量保持不变：sidecar 的 embedding 只在 worker 内存中短暂存在并写入当前项目 SQLite 的 `speaker_embedding` 表；它不进入 `OperationResult`、host response、DTO/TS bindings、进度或终态事件、项目 revision/operation log、快照、诊断日志和导出物。Electron speaker 任务复用 Slice 5 的 `ServiceProgressSink`，自由文本中的项目、speaker 模型、模型根和 sidecar 路径分别替换为 `<PROJECT>` / `<MODEL>`；畸形 sidecar 原始 JSON 中连续至少 8 个数值的数组替换为 `<REDACTED>`，phase/message 分别以 UTF-8 边界安全地限制在 4096 bytes，task id、状态和计数不变。main 本地日志仍只记录既有字段白名单，不记录请求、响应或事件 payload。

Rust host Slice 6 集成以合成媒体和预置的 ASR/aligner/Silero/WeSpeaker installed 状态运行既有 ASR 与 speaker mock：覆盖模型/项目门禁、两素材分离、任务成功终态、结果、名称候选、仅目标说话人的 Agent payload 与路径脱敏、姓名确认、拒绝确认不增 revision、合并确认、逐词归属改写、可见列表和旧身份 `merged_into` 留存；同时检查项目 DB 中存在 embedding，而所有 host event/response 和项目日志均无向量形状或 embedding 字段。额外 host 边界测试在无 test flag 且预置两个 mock 环境变量时验证 speaker 子进程观察到 `mock=false`，并由显式 test mock 注入含 12 个声纹浮点值的畸形响应，验证 `dl://progress` 只保留 `<REDACTED>`。Electron Playwright Slice 6 经真实 path grant 创建项目并导入两段合成媒体，复用 Slice 5 mock 转录，再覆盖分离、改名后的转录说话人显示映射、跨素材合并、Agent payload 路径脱敏，以及全部捕获事件无结构化向量、浮点数组形状字符串、项目根或媒体源路径。

Slice 6 未修改旧容器目录、Web/PWA、SQLite schema/migration、bindings/DTO schema、host 或 sidecar 协议、导出格式和既有 speaker 隐私边界；Tauri 继续作为行为参考。门禁证据：workspace fmt 与 clippy `-D warnings` 通过；严格工具模式 workspace Rust 测试全过（engine 148 过、1 个既有 golden regeneration ignored；desktop service 19 过；host Slice 6 集成 3 过；Tauri 参考测试 18 过）；bindings contract 通过；Studio lint、89 项 Vitest、Studio Vite build、独立 `tsc -b`、Electron build 与 host build 通过；Slices 1–6 及平台/骨架共 14 项 Playwright 全过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 4 Slice 7 已交付项目级导出（Phase 4 完成）

host-neutral service 已按 Tauri 参考实现注册 `project_export_preview`、`project_export_xmeml_apply`、`project_export_ass_apply` 与 `project_render_mp4_apply`。预览继续以当前项目目录 basename 加 `Rough Cut` 调用 `preview_project_export`，只返回同一份 TimelineIR v2、字幕 Cue 与 Premiere/Resolve 兼容性报告，不写文件或 revision。XMEML/ASS apply 直接复用 engine 的原子写入、SHA-256、`outputs` 与 `export_artifact` 落账；MP4 继续由 `FfmpegTools::discover` 发现开发运行时，在 `<project>/.doublelove/cache` 写临时 ASS，并保留 engine 的 libass 前置诊断与实际 ffmpeg 渲染语义。

Electron main 已预先限定这三个 apply 只能消费一次性 `export-save` grant；本切片验证 token 重放返回 `INVALID_GRANT`。`project_render_mp4_apply` 不再套用普通 host 请求的 5 秒超时，因为真实项目渲染可以合法运行更久；host 退出或崩溃仍会拒绝 pending。四个导出命令的失败 `OperationResult` 在 renderer 边界只替换当前项目根和已登记媒体源路径为 `<PROJECT>` / `<MEDIA>`，不改 status、data、revision、counts、outputs、SHA-256 或 engine/CLI/Tauri 诊断 code。

Rust host Slice 7 集成使用 25 与 30000/1001 两段真实合成媒体、test-only mock 转录、主轨和 omit，覆盖只读预览、XMEML/ASS 文件与哈希、实际 libass MP4、ffprobe codec/duration、三类导出 ledger、空主轨、未知资产、非法目标、ffmpeg 失败路径脱敏，以及无 libass 时与 Tauri 完全相同的 `RENDER_ASS_FILTER_MISSING` cause/suggested action。Electron Playwright `slice7.spec.ts` 经真实目录/媒体/导出 grant 跑完整链，逐项检查 pathurl、ASS 样式与 cue、真实 MP4、SQLite `export_artifact`、空 cut 阻断和 grant 重放；同一项目再由既有 CLI 写出 XMEML，并与 Electron 产物逐字节相等。

Slice 7 未修改旧容器目录、Web/PWA、SQLite schema/migration、TimelineIR、engine 导出器或 XMEML/ASS golden；Tauri 与 CLI 继续作为行为参考。门禁证据：workspace fmt 与 clippy `-D warnings` 通过；ffmpeg-full/libass 严格模式 workspace Rust 测试全过（engine 148 过、1 个既有 golden regeneration ignored；desktop service 19 过；host Slice 7 集成 2 过；Tauri 参考测试 18 过）；bindings contract 通过；Studio lint、89 项 Vitest、Studio/TypeScript/Electron build 与 host build 通过；Slices 1–7 及平台/骨架共 15 项 Playwright 全过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。至此 Phase 4 七个纵向业务切片全部迁移完成，Phase 5 可开始默认 Electron、打包资源、签名发布与 Tauri 清理。

## Phase 5A 已交付打包基础设施与资源归属

模型清单已逐字节移动到 `crates/double-love-engine/resources/model-catalog-v1.json`，偏好 fixture 已归属 desktop service，512×512 图标与媒体/模型运行时占位树已归属 `studio/build/`；搬移前后 SHA-256 一致。三个 runtime 脚本已改写到 Studio 资源树，Tauri 配置通过 source→target 映射继续生成相同的 `runtime/` 与 `model-runtime/` 包内布局，debug `.app` 仍可构建；旧容器目录及其行为未删除。

`electron-builder` v26 配置现固定 macOS 15+ arm64 的 DMG/ZIP、ASAR、应用身份、视频分类、GitHub publish 元数据与四组 `extraResources`。本次本地证明与 CI 目录包显式关闭证书自动发现，`notarize:false`，本阶段未做发布签名或公证。afterPack 为 Electron 43 配齐全部九项 Fuse V1；浏览器专用 V8 snapshot 随包生成，renderer 改由受限 `dl-app://app` 协议加载，因此 `GrantFileProtocolExtraPrivileges:false` 与 `OnlyLoadAppFromAsar:true` 下仍可启动。

Electron main 在打包模式只从 `process.resourcesPath` 解析 host、协议 schema 与注入给 host 的资源根。desktop service 的导入与 MP4 渲染按 `runtime/{ffmpeg,ffprobe}` → 既有 `resources/runtime` 兼容位置 → 开发期 `FfmpegTools::discover()` 的顺序解析；新增单测冻结 bundled runtime 优先级。打包 smoke 检查 host、schema、ASAR、图标、runtime 树和九项 Fuse，再经 CDP Playwright 启动 unpacked `.app`，验证 `hostHealth` 与偏好调用均成功且隔离 userData 生效。

5A 门禁证据（2026-08-22，macOS arm64）：release host、Studio/Vite/TypeScript/Electron build、无证书目录包、Fuse 工具所需的本地 ad-hoc 完整性校验和 unpacked boot smoke 通过；ffmpeg-full/libass 严格模式 workspace fmt/clippy/test 全过（engine 148 过、1 ignored；desktop service 20 过；Tauri 18 过）；bindings contract、两组 Python sidecar 测试与 py_compile 通过；Studio 89 项 Vitest、全部 16 项 Playwright、根 Web 99 项 Vitest 及 lint/build 通过；Tauri debug `.app` 与新共享资源布局通过；`git diff --check` 通过。真实 runtime 二进制、Developer ID 签名及公证按计划留给后续发布批次，Phase 5D 才删除 Tauri。

## Phase 5B 已交付更新器与本地发布链证明

Studio 首个 Electron 用户版本以 `studio/package.json` 的 `0.2.0` 为唯一 app 版本源，`electron-builder`、`app.getVersion()`、关于页和 Electron `doctor_run` 的 renderer-facing 报告均使用该值。main 通过受保护的 `app:get-info` 只返回应用名与版本；诊断调用由 main 注入版本，desktop service 仅在 renderer 边界用扁平 wrapper 增加 `app_version`，没有修改 engine `DoctorReport` schema。main 启动握手现在同时要求 protocol 1、`invoke` capability 以及非空 host/engine 版本，并把两个版本写入字段白名单本地日志；任一不满足仍走既有启动失败路径。

`electron-updater` 的正式包 provider 继续来自完整 DMG/ZIP 构建生成的 `app-update.yml`。运行策略固定为 `autoDownload:false`、`autoInstallOnAppQuit:false`、`allowPrerelease:false`：打包启动且主窗口加载完成后只静默检查一次，失败只写本地日志；手动检查才把错误作为可读状态返回。只有未打包运行，或带显式 `--double-love-e2e` 的打包进程，才会接受无凭据、无 query/fragment 的 `DOUBLELOVE_UPDATE_FEED_URL` generic override；`electron-builder --dir` 不生成 update config，因此该测试门内会在隔离 userData 写权限 `0600` 的临时 update config，正式运行不走此路径。

更新状态统一经受限 preload 的 `dl://update-status` 发给应用窗口，只允许 `stage`、安全版本和 0–100 进度，不携带 feed URL、下载路径或 token。受保护的 `update:download` 只在 `update-available` 后执行，`update:install` 只在 `update-downloaded` 后执行。关于页先显示真实当前版本；用户点击“下载更新”必须确认，下载完成后点击“重启安装”还必须再次确认，不存在自动下载、静默退出或普通退出时自动安装。

`quitAndInstall` 前由 main 先设置 installing flag。settings 的 close-to-hide 和全局 `before-quit` 都识别此状态：不再 `preventDefault`，同步停止 host 而不等待原 1.5 秒 shutdown timeout，然后让 Squirrel 原生退出安装路径继续；普通退出仍保留原先的 graceful shutdown gate。`quit-flow.test.ts` 冻结这条回归路径。

本地发布链证明（2026-08-23，macOS arm64）由 `scripts/migration/local-update-feed.sh` 全程使用本机 Electron distribution、`--publish never` 和 `127.0.0.1`：在临时目录构建 ad-hoc 的 `0.2.1-feed` `.app`/ZIP，自行计算 size/SHA-512 并合成 `latest-mac.yml`，退出后删除临时 feed。`update-feed.spec.ts` 从当前 `0.2.0` 目录包启动，证明 check 返回 `update-available 0.2.1-feed`；确认前服务端没有 ZIP 请求且没有 progress/downloaded 事件；确认后观察到 ZIP 请求、100% progress 与 `update-downloaded`。捕获的状态均无 localhost URL、feed 目录或 artifact 路径。第二次安装确认被测试明确取消，应用与设置窗口继续存活，因此测试没有调用 `quitAndInstall`。本机未执行真实安装、Developer ID 签名或公证；签名一致性、原生替换和重启后的版本验证仍留给发布机门禁。

5B 门禁证据：Studio lint、102 项 Vitest、Vite/TypeScript/Electron build 通过；本机 Electron distribution 的无证书 `electron-builder --dir` 产物版本为 0.2.0，Fuse/package smoke 通过；设置 ffmpeg-full/libass 后全部 18 项 Playwright（含本地 feed）通过；workspace fmt、clippy `-D warnings`、严格工具模式 Rust 测试及 bindings contract 通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 5C 已交付首次 Electron 写入前的一次性数据备份

Electron host 通过 desktop service 首次接触既有 Tauri 数据时，必须先完成以下备份；备份失败会阻止对应命令继续写入：

- 偏好：首次读取既有 `preferences.json` 前，在同目录创建 `preferences.json.pre-electron-backup`。源文件不存在时不创建；备份使用原始字节、不迁移内容，权限固定为 `0600`。
- 项目：`project_open` 或 `project_create` 发现既有 `.doublelove/project.sqlite` 时，必须在任何 engine `open_project` / `create_project`、WAL 切换、migration 或 snapshot backfill 前创建 `.doublelove/project.pre-electron-backup.sqlite`。engine 以只读连接打开源库，并使用 SQLite online backup API，因此已提交到 WAL 的内容也进入一致快照，而不会先运行 `ProjectStore::open`。
- 两类目标都采用“只创建、不替换”规则：目标名一旦存在，后续命令和后续 Electron 启动都直接保留它，不校验、更新或覆盖。它们记录的是第一次 Electron 可能写入之前的状态，不是持续备份。
- 备份不改变偏好 JSON、项目 manifest、SQLite schema/migration 或任何 CLI/Tauri 调用方行为；未显式调用新增 engine 备份 helper 的 CLI/Tauri 路径保持原样。项目备份仍可由 `ProjectStore::open` 打开，供 Tauri 等价路径恢复。

回退时先完全退出 Electron 与 Tauri，另行保留当前文件，再恢复副本；不要移动或改写上述一次性备份本身。偏好回退是把 `preferences.json.pre-electron-backup` 的副本放回 `preferences.json` 并保持 `0600`。项目回退是把 `project.pre-electron-backup.sqlite` 的副本放回同目录的 `project.sqlite`；替换前须把当前 `project.sqlite` 及其 `project.sqlite-wal` / `project.sqlite-shm` 一起移出该目录，避免旧 WAL 套到恢复库。随后可用最后可构建的 Tauri 版本打开原项目目录验证。

## Phase 5D 已交付旧桌面容器删除

renderer 平台接缝现只在 preload bridge 存在时选择 Electron，否则进入 browser preview；`file:` 与 `dl-app:` 缺 bridge 时继续 fail closed。公共 adapter 类型由 Electron adapter 派生，normalize 测试直接使用中立模块；旧 renderer adapter、Studio npm 依赖和专用桥接分支均已删除。

Rust workspace 现只含 engine、CLI、desktop service 与 desktop host；旧 crate 目录整体删除，Cargo lock 由 `cargo build` 重建。根旧容器 scripts/CLI、Studio CI reference build 与 baseline debug-app gate 均已移除；baseline 完整模式改为 release host + Electron build + directory package + packaged smoke，fast 模式将四项记为 `SKIP` 并保持 `INCOMPLETE`，包体积目标改为 `studio/out` 与 unpacked Electron `.app`。

5D 门禁证据（2026-08-23，macOS arm64）：ffmpeg-full 严格模式 workspace fmt/clippy/test 通过（engine 149 过、1 个既有 golden regeneration ignored；desktop service 22 过；全部 host/CLI 集成通过）；bindings contract 通过；Studio lint、102 项 Vitest、Vite/TypeScript/Electron build 通过；release host、`electron-builder --dir`、九项 Fuse 与 packaged boot smoke 通过；全部 18 项 Playwright（含 package smoke、一次性备份与本地 updater feed）通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

依赖树门禁 `cargo tree --workspace | grep -i tauri` 无输出（grep 退出 1，符合空结果预期）。tracked-file 六项残留扫描共 13 行，全部位于本目录已明确标记的历史归档文件；排除本目录、CHANGELOG 与 TODO 后为零。Phase 5D 不执行签名、公证、发布、tag、push 或 merge；发布机签名/公证仍是 Phase 5 的独立剩余门禁。

## Phase 5F 已交付发布工作流与最终迁移文档

`.github/workflows/studio-release.yml` 只接受 `studio-v*` tag push 或带明确
既有 tag 的手动 dispatch，并强制 tag 精确等于 `studio-v` 加
`studio/package.json` 版本。受保护的 `studio-release` environment 在 job
开始前提供人工批准门禁；工作流随后准备并验证 release runtime，运行 Studio
与 Electron 门禁，构建 Developer ID 签名、公证的 arm64 DMG/ZIP，验证
`.app`/DMG 票据、package smoke、更新元数据与两个 blockmap。

GitHub publisher 使用 `onTagOrDraft`、`releaseType: draft`、`EP_DRAFT=true`
与 `studio-v` tag prefix；上传后还会通过 API 断言 Release 仍为非 prerelease
草稿且五项资产齐全。工作流没有公开 Release 的操作；干净机器验收后只能
人工公开。本阶段未执行 workflow、签名、公证、tag、push、上传或 Release
操作，也未读取任何 secret 值。

现行架构、打包、发布、升级回退与排障分别迁至 `docs/studio/` 的五份短文档；
本目录继续只保存历史。回退锚点冻结为最后 Tauri-buildable 的
`cf3831a^` / `cf3831a~1`（`c6d43fb`），以及删除旧容器的 Electron-only
收口提交 `cf3831a`。

## Phase 4 Slice 4 已交付主轨与项目视觉设置

host-neutral service 已按 Tauri 参考实现注册 `timeline_get`、`main_track_append`、`main_track_append_full`、`main_track_list`、`main_track_move`、`main_track_trim`、`main_track_split`、`main_track_remove`、`canvas_get/set`、`output_rate_get/set`、`subtitle_style_get/set` 与 `apply_default_subtitle_style`。主轨写操作直接复用 engine 函数及其未知资产、非法范围、未知片段、revision 和诊断语义；`timeline_get` 继续调用 `compile_project_timeline`，名称严格取当前项目根目录 basename 加 `Rough Cut`。未显式设置输出帧率时仍跟随主轨首段素材，`output_rate_set(null)` 删除显式值；应用默认字幕样式仍从 service 偏好读取 `default_subtitle_style`，并与普通 `subtitle_style_set` 产生相同项目写入语义。

本切片把 Slice 2 为历史导航提前接入的 canvas/subtitle 命令纳入正式迁移范围，并为所有新迁移命令的失败诊断统一替换当前项目根目录为 `<PROJECT>`；未改变成功数据、状态、revision、输出、engine/CLI/Tauri 错误文本或任何存储与 TimelineIR 契约。host 集成测试使用两个真实合成媒体，覆盖完整/区间追加、列表、移动、裁切、拆分、删除、TimelineIR v2 顺序与帧率、canvas、输出帧率设置/清除、项目字幕样式和偏好默认样式应用，以及未打开项目、未知资产、非法范围、未知片段与 renderer 响应路径脱敏。

Electron Playwright 同样仅使用临时目录和两段合成 MP4，经目录/媒体 grant 完成导入与整套主轨变更，断言 TimelineIR source/clip 顺序和 `mainTrackList` 最终状态；并覆盖 canvas、输出帧率、字幕样式、默认样式往返、未知片段脱敏，以及剩余 source 的 `dl-media` 正常 200 与未知资产 404。Slice 4 未修改旧容器目录、Web/PWA、engine 契约、SQLite schema/migration、TimelineIR shape 或导出格式；Tauri 继续作为行为参考。

Slice 4 门禁证据：workspace fmt 与 clippy `-D warnings` 通过；严格工具模式 workspace Rust 测试全过（engine 148 过、1 个既有 golden regeneration ignored；host Slice 4 集成 1 过；Tauri 参考测试 18 过）；bindings contract 通过；Studio lint、89 项 Vitest、Studio Vite build、独立 `tsc -b`、Electron build 与 host build 通过；Slices 1–4 全部 11 项 Playwright 通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 4 Slice 3 已交付媒体导入与项目资产播放

host-neutral service 已按 Tauri 参考实现注册 `import_media` 与 `assets_list`：命令必须先有打开项目，媒体工具继续由 engine `FfmpegTools::discover` 按环境变量 → `PATH` → Homebrew 常见位置发现，准备音频仍写入 `<project.root>/.doublelove/prepared`。导入、重复导入、缺失文件、不支持帧率、工具缺失与存储错误继续复用 engine 的状态、诊断 code、revision 与 data；仅 Electron service 返回 renderer 前把诊断文本中的所选媒体路径和项目根目录替换为 `<SELECTED_MEDIA>` / `<PROJECT>`，engine、CLI 与 Tauri 参考行为不变。开发运行时发现已验证，正式打包内 ffmpeg/ffprobe 的资源解析与复制仍明确留在 Phase 5。

Electron main-only 命令 `resolve_media_asset` 只在当前项目 Store 中按 `asset_id` 查找资产，且源文件仍为普通文件时才返回路径；未知资产或已移除源文件返回 `MEDIA_ASSET_NOT_FOUND`。该命令不在 `RENDERER_COMMANDS`，preload 也没有专用暴露；renderer 可见的 `import_media` / `assets_list` 响应不含媒体路径。既有 `dl-media` handler 和 GET/HEAD/单 Range 行为未改，TimelinePreview 的当前视频与下一源预载统一从旧 `media://localhost/<id>` 切换到批准的 `dl-media://asset/<id>`。

Rust host 集成测试在临时目录生成真实合成 MP4，覆盖未打开项目、成功导入与 `prepared` 状态、重复诊断、资产列表、不支持帧率、缺失文件、合成 ffprobe 失败、main-only 解析成功及未知/源文件缺失，并断言成功与失败的 renderer 可达响应都不含媒体或项目绝对路径、资产摘要无路径字段且 allowlist 不含解析命令。Slice 3 Playwright 同样经目录与媒体一次性 grant 导入真实合成 MP4，验证全量 200、单 Range 206 与精确 `Content-Range`、未知及任意磁盘路径 URL 404、renderer 解析命令被拒绝，以及缺失源文件和 ffprobe 失败均不泄露绝对路径且项目仍可继续列出资产。组件单测冻结新 scheme，生产 renderer 已无 `media://localhost`。

Slice 3 未修改旧容器目录、Web/PWA、SQLite schema/migration、manifest、导出格式或既有 Electron `dl-media` 协议实现；Tauri 继续作为行为参考。门禁证据：workspace fmt 与 clippy `-D warnings` 通过；严格工具模式 workspace Rust 测试全过；bindings contract 通过；Studio lint、89 项 Vitest、Studio Vite build、独立 `tsc -b` 与 Electron build 通过；Slices 1–3 全部 10 项 Playwright 通过；根 Web lint、99 项 Vitest 与 build 通过；`git diff --check` 通过。

## Phase 3C 已交付 React renderer 目录整理

React renderer 已保持内部目录树纯移动至 `studio/src/renderer/`；Electron main 与 preload 继续保留在 `studio/src/main/`、`studio/src/preload/`。仅同步更新入口、TypeScript、ESLint、Vitest 与 repo-root bindings 相对路径；Tauri 的 `dist/`、Electron 的 `out/renderer/`、devUrl 与 frontendDist 均不变，未改变业务逻辑或运行时行为。

## Phase 3D 已交付 renderer 平台接缝（Phase 3 完成）

`studio/src/renderer/platform/desktop.ts` 现在按 preload bridge、Tauri internals、browser preview 的顺序选择 Electron/Tauri/preview adapter；Electron 与 Tauri 均保持既有 renderer 命令和 DTO normalize 表面。Electron adapter 将 host invoke 成功数据解包为既有 `OperationResult`，并把 host error 合成为同形状的 `failed` 结果；对话框仅向 renderer 返回 grant token，需路径的命令再透传 token。事件统一经 `api.listen` 暴露 `{ payload }`，React renderer 不再直接导入 Tauri event API。

无 bridge 的普通 HTTP 开发页继续进入 preview，不改变既有预览 notice 路径；`file:` renderer 若同时缺少 Electron/Tauri bridge 会在模块初始化时明确失败，避免打包壳静默降级。至此 Phase 3A–3D 的 service/host、平台安全、目录整理与 renderer 平台边界全部完成；业务命令实现仍按计划留在 Phase 4。

## Phase 3B 已交付 Electron main 平台基础设施

Electron main/preload 现已提供 60 秒惰性过期、UUID、不透明且按 kind 绑定的一次性 path grant；原生目录、媒体与导出对话框只把 token 返回 renderer，测试路径覆盖仅在未打包且显式 `--double-love-e2e` 时生效。单一 `dl:invoke` 经命令 grant policy 消费 token 后才向 host 注入 `path` / `target_path` / `patch.model_root`；只读未知 policy 保持透传，Phase 4 尚未迁入的命令由 host 返回 `UNKNOWN_COMMAND`。

所有 `ipcMain.handle` 注册统一经过 sender frame URL、应用窗口归属、逐 channel 窗口 allowlist 与 8 MiB payload 上限校验；host response/event 已解复用，允许的六类事件经 preload 剥离 Electron event 后广播并返回 unsubscribe。`dl-media://asset/<asset-id>` 在 app-ready 前注册安全 scheme，并通过 host `resolve_media_asset` 解析；Node 可测纯函数保持 GET/HEAD、单 Range 的 200/206/404/416/501 语义，当前 host 的 `UNKNOWN_COMMAND` 安全映射为 404。

main 本地日志仅写 `userData/logs` 的字段白名单 JSONL，1 MiB 轮转保留五份；意外 host 退出写无路径的 crash marker，握手成功清除。`electron-builder.yml` 与 afterPack Fuse V1 hook 已锁定 bundle identity、ASAR 与六项加固开关；完整打包/签名验证仍留 Phase 5。Node 单测覆盖 grant、媒体响应、日志与 builder/grant policy，Playwright 覆盖 token 全链、协议、事件和未知命令，既有 sandbox/settings smoke 继续保留。

## Phase 3A 已交付 desktop service 与 host 分派基础设施

`crates/double-love-desktop-service` 现在是 Tauri/Electron 无关的桌面业务适配边界，依赖方向为 `desktop-host → desktop-service → engine`。service 接收 Electron main 已解析并通过 `--app-data-dir` 注入的数据目录；缺失时明确失败，不按产品名或工作目录猜测。它持有当前项目槽、engine `TaskRegistry`、历史导航，以及标明为 Phase 4 迁入位置的空偏好/模型状态容器。

host protocol v1 新增 `invoke { name, payload }` 和无 `id` 的 `HostEvent { v, event, payload }`。service 的命令注册表在本阶段刻意为空，因此所有 invoke 命令都返回含命令名的 `UNKNOWN_COMMAND`；`handshake`、`health`、`shutdown` 保持控制面职责。service 通过 `DesktopEventSink` 发事件，host 实现为 stdout 长度前缀帧，供 Electron main 后续按 response `id` 与 event 字段解复用。偏好、项目、媒体、模型等业务命令及事件接线不在本阶段伪实现，统一留给 Phase 4 纵向切片。
