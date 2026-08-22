# 数据契约基线（冻结清单）

原则：本轮迁移**禁止**新增 `.doublelove` SQLite migration、修改 manifest schema、TimelineIR schema、偏好 schema、模型安装 schema 或导出格式。下表为冻结对象与事实源。

| 契约 | 事实源 | 关键约束 |
| --- | --- | --- |
| 偏好文件 | `preferences.rs:22-24`（`preferences.json` / key `app_preferences`），schema v1 | identifier `space.ahua.doublelove.studio` 对应 `$HOME/Library/Application Support/space.ahua.doublelove.studio`；字段见 `AppPreferencesV1`；缺字段合并默认、损坏重命名为 `*.corrupt.<ts>.json`；endpoint 仅 HTTPS 无凭据/查询；recent ≤20。主题在启动后由此持久化偏好恢复 |
| 模型安装状态 | engine `model.rs`（ModelManager）；安装状态机 not_installed→queued→downloading→paused→verifying→installed/corrupt/failed | 中断安装重启后恢复 paused（engine 测试 `interrupted_installations_resume_as_paused_after_restart`）；staging 目录 `.part` 续传（models.rs:189-275） |
| 项目库 | `<root>/.doublelove/{project.sqlite,manifest.json,cache,logs,exports,prepared}` | schema migration V1–V10，`MIGRATIONS` 表 storage.rs:252-264；WAL + 外键（storage 测试）；revision/operation log/历史快照；声纹向量只在库内、不进快照/日志/导出（storage.rs:240-248 注释） |
| TimelineIR | `TimelineIRv2`（engine contracts；bindings/TimelineIRv2.ts） | 序列化含 schema_version（contracts 测试） |
| 导出 golden | `crates/double-love-engine/src/export/xmeml_golden_25fps.xml`（.gitignore 白名单放行） | 再生成测试故意 `ignored`；XMEML/ASS/MP4 由 preview 共享同一 TimelineIR |
| CLI 契约 | `crates/double-love-cli`：命令、JSON 输出（OperationResult）、退出码（0/1/2） | CLI E2E 9 个（cli 4 + export_roughcut 1 + import_media 3 + transcribe 1）；GUI/CLI 并发靠 revision 保护 |
| sidecar 协议 | `sidecars/asr`、`sidecars/speaker`（Python，stdio JSON） | mock 协议自检；取消语义；各 2 个单测 |
| ts-rs 绑定 | `bindings/*.ts`（git 跟踪，build.rs 生成） | **i64/u64 → TS `bigint` 但 JSON 运行时为 number**；renderer 用 `num()`（utils.ts:10）与 `normalize*`（tauri.ts:170-247）在边界转换 |

`studio.theme` 目前只是偏好读取完成前的初始绘制缓存（App.tsx:70-72,141），随后由 `preferences_get` 返回的 `AppPreferencesV1.theme` 覆盖（App.tsx:178-183）；它不是需要“同 origin 自然迁移”的独立数据契约。`studio.panels` 的 helper（utils.ts:245-282）仅被单测调用，产品 UI 未接线，因此不登记面板 localStorage 迁移或重置语义。

## 整数序列化决议（host protocol v1 必须遵守）

i64/u64 字段（revision、frame、sample、字节数）在 JSON 中按 number 传输；host 侧 serde 校验范围，超过 2^53 的值在协议层拒绝（当前领域值均远小于该界）。renderer 继续使用 `num()` 边界转换，禁止把 `bigint` 类型泄漏进新 shared 契约。

## Phase 2A fixture 与冻结门禁

见 [README.md](README.md#phase-2a-已交付-fixture-与门禁)。已提交的 source fixture 仅为合成 JSON：偏好 v1/partial-v0/corrupt 与模型 installed/paused/staging-interrupted。偏好完整结果与模型安装 envelope schema=1 均由独立 fixture/literal 冻结；schema 1–10 SQLite 在逐条 migration SHA-256 快照通过后才由测试运行时生成并立即删除，未跟踪二进制数据库。项目测试冻结 manifest camelCase 且 schemaVersion 仍为 1。

`scripts/migration/check-bindings-contract.sh` 是确定性 contract gate：现有 ts-rs cargo tests 再生成 bindings 后，`bindings/` 相对 HEAD 的 tracked diff 与 untracked 文件都必须为空，随后运行现有 CLI JSON contract E2E。TimelineIR schema、migration SQL、生产 schema 与导出 schema/golden 均未修改；`regenerate_golden_file` 继续保持 ignored。

## 假绿风险登记

| 风险 | 证据 | 阶段 2 对策 |
| --- | --- | --- |
| `regenerate_golden_file` 恒 ignored | engine export/xmeml 测试 | 仅手工再生成入口，不算失败；Phase 2A 未改变 ignored 属性 |
| ffmpeg/ffprobe/libass 缺失时相关测试可能不可用 | engine render/import 测试；CI prerequisite 检查 | `DOUBLELOVE_REQUIRE_TEST_TOOLS=1` 时缺失立即 fail；普通本地测试仍可 self-skip；Studio CI 已设置 |
| python3 缺失或 Homebrew/system Python 混用 | engine/CLI sidecar 测试用实际 PATH 上的 `python3` | 同一严格 flag 令缺失 fail；baseline 继续记录实际版本且 raw log 中 ROOT/HOME 脱敏 |
| ts-rs 测试再生成但未核对 tracked/untracked 输出 | `#[ts(export)]` 生成测试 | contract 脚本对 HEAD tracked diff 与 untracked 文件分别强检，CI 必跑 |
