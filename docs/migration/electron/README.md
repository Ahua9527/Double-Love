# Tauri → Electron 迁移：阶段账本与文档索引

范围：仅 Studio 桌面端。Web/PWA 与 `double-love` CLI 对外行为不变。首个 Electron 正式版为 Studio 0.2.0（macOS 15+ / arm64，bundle id `space.ahua.doublelove.studio` 不变）。

## 文档索引

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
| 4 纵向业务切片迁移（7 片） | 未开始 | 每片：Electron E2E 正常+失败/取消/恢复路径；Tauri 对照一致 | 每片独立提交，只 revert 当前切片 |
| 5 默认 Electron、打包发布、清理 | 未开始 | 功能矩阵 100%；签名公证门禁通过 | 保留最后可构建 Tauri commit |
| 6 全量验收与回退演练 | 未开始 | 见计划完成判定 | 文档化手动回退 |

## 当前基线证据（2026-08-22，本机 macOS 15.7.7 arm64，Node 24.14.1 / pnpm 10.33.0 / Rust 1.97.1）

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

`scripts/migration/baseline.sh`：幂等采集环境摘要、门禁命令与退出码、测试/skip 摘要、包体积到忽略目录（默认 `evidence/migration-baseline/`，已 gitignore）。`--fast` 把 Tauri 打包记为 `SKIP`，结果为 `verdict=PASS_FAST`、`completeness=INCOMPLETE`，不能冒充完整 PASS。任一必需门禁（含 ffmpeg 精确 `ass` filter）失败则以非零退出。raw log 会把实际 repo root 与 HOME 替换为占位符，Python bytecode cache 也留在该次 evidence 目录内。

## Phase 2A 已交付 fixture 与门禁

全部 fixture 均为 synthetic、privacy-safe 文本或运行时生成代码，不含真实用户数据库、媒体或模型权重；仓库不跟踪生成的 SQLite：

1. Tauri 偏好：`src-tauri/tests/fixtures/preferences/{v1,partial-v0,corrupt}.json`；v1 解码与 fixture 的完整持久化对象相等，partial-v0 迁移与独立完整预期对象相等，损坏 JSON 分类为 decode failure。
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
