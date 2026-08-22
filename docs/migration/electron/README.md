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
| 1 审计、基线与蓝图 | **本阶段产物** | 能力矩阵覆盖全部登记命令/事件/资源；失败/skip 唯一分类；ADR 经主会话确认 | 只含审计/脚本/fixture 元数据，直接 revert |
| 2 回归护栏 + Electron 骨架 | 未开始 | 窗口/host 握手/settings 单例/边界 smoke 全过；Tauri 仍完整可用 | 删除/回退骨架，不触碰用户数据 |
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

## Fixture 计划（阶段 2 实施，本阶段不落盘）

阶段 2 将新增以下**脱敏合成** fixture（全部 synthetic，无真实素材/权重/私有路径）：

1. Tauri `preferences.json`：合法 v1、缺字段 v0→v1 迁移、损坏 JSON、非法 `model_endpoint`、超限 recent_projects（>20）。
2. 模型安装状态：`installed` / `paused`（含 `.part` 残留）/ `staging` 中断三种最小目录形态（占位空文件 + catalog 元数据，不含真实权重）。
3. 项目库：schema 1–10 逐版本最小 SQLite（由合成 SQL 生成脚本产出，不含用户文本/声纹向量真实值）。
4. CLI JSON 输出 golden、TimelineIR v2 序列化 golden、XMEML/ASS 导出 golden（复用并扩展现有 `crates/double-love-engine/src/export/xmeml_golden_25fps.xml` 模式）。
5. 缺工具即 skip 的假绿风险清单：现有 Rust 测试中仅 `regenerate_golden_file` 为 ignored；ffmpeg 依赖测试（`render::tests::renders_a_mixed_rate_project_when_local_ffmpeg_is_available`、`import_media_end_to_end_with_synthetic_mp4` 等）在 CI 由 “Verify test runtime prerequisites” 步骤保证工具存在（`.github/workflows/studio-quality.yml:35-46`），本地需在强门禁模式下把缺失工具判 fail 而非 skip。
