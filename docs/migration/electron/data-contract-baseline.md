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

## Fixture 计划

见 [README.md](README.md#fixture-计划阶段-2-实施本阶段不落盘)。本阶段不生成/提交任何二进制或私有 fixture。

## 假绿风险登记

| 风险 | 证据 | 阶段 2 对策 |
| --- | --- | --- |
| `regenerate_golden_file` 恒 ignored | engine export/xmeml 测试 | 仅手工再生成入口，不算失败 |
| ffmpeg/ffprobe 缺失时相关测试可能不可用 | engine render/import 测试；CI studio-quality.yml:35-46 前置检查 | 强门禁模式缺工具 fail 而非 skip |
| Homebrew python3 与系统 python3 混用 | sidecar 测试用 `python3` | baseline 脚本只记录版本并强门禁实际 PATH 上的 `python3`；raw log 中 ROOT/HOME 脱敏 |
