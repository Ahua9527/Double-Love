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
| 2B Electron 骨架 | **进行中（Batch A host 骨架已交付）** | 窗口/host 握手/settings 单例/边界 smoke 全过；Tauri 仍完整可用 | 删除/回退骨架，不触碰用户数据 |
| 3 安全边界与平台基础设施 | 未开始 | renderer 无 Node 能力；写能力需 path grant；媒体协议只读当前项目资产 | Tauri adapter 保留，revert 当前批次 |
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
4. 绑定/CLI contract：`scripts/migration/check-bindings-contract.sh` 运行现有 ts-rs `export_bindings` cargo tests，再对 `bindings/` 检查相对 HEAD 的 tracked diff 与全部 untracked 文件，随后运行 CLI `--json` contract 测试。XMEML 的 `regenerate_golden_file` 保持故意 ignored，不默认执行。
5. 严格工具门禁：唯一开关 `DOUBLELOVE_REQUIRE_TEST_TOOLS=1` 令依赖 ffmpeg/ffprobe/python3/libass 的 Rust 测试在工具缺失时失败；未设置时保留本地 self-skip。`.github/workflows/studio-quality.yml` 对 push 与 pull request 运行，在存在时优先 `ffmpeg-full`，并以不输出绝对路径的 prerequisite 检查精确要求 `ass` filter。

## Phase 2B Batch A 已交付 host 骨架

`crates/double-love-desktop-host` 是无 Tauri/Electron 依赖的长期运行 Rust 二进制与 library；本批只提供 `handshake`、`health`、`shutdown`，不含项目状态、偏好、模型或业务命令，`double-love-desktop-service` 仍延后到 Phase 3。stdin/stdout 只承载 4 字节大端长度前缀与 UTF-8 JSON frame，单帧上限 64 MiB；EOF 正常退出，日志只允许写 stderr。

Protocol v1 使用版本化、可关联的单层 envelope。request 为 `{"v":1,"id":"<client-id>","method":"handshake","client":"electron-main","client_protocol":1}`（`health`/`shutdown` 同样携带 `v` 与 `id`）；response 回显同一 `id`，为 `{"v":1,"id":"<echoed-id>","status":"ok","result":{"type":"hello|health|shutdown",...}}` 或 `{"v":1,"id":"<echoed-id>","status":"error","error":{"code":"...","message":"..."}}`。无法解析的 frame 使用 `id:"unknown"`；缺失/错误版本和缺失/空白 id 会被拒绝。hello data 固定返回 `protocol`、`host_version`、`engine_version`、`capabilities`。Rust tagged union 同时生成 `bindings/host-protocol/*.ts` 与 `bindings/host-protocol/schema/*.schema.json`，contract 脚本检查 tracked diff 和 untracked 产物。

v1 的 `u64`/`u32` 在 wire 上都使用 JSON number；ts-rs 的 `bigint` 仅是既有生成绑定的类型层注解，不改变 JSON runtime。Node/renderer runtime boundary 继续按既有规则转换并校验安全整数范围，禁止直接把 JS `BigInt` 交给 JSON 序列化。
