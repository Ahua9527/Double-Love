# Double Love Studio Foundation

> **2026-08-23 Phase 5D 修订：当前桌面容器为 Electron + React。** 更早的桌面容器与 GPUI 方向决策只保留在迁移归档和 Git 历史中，不再作为开发或发布指导。Engine、`.doublelove` 存储与命令契约决策保持不变。

## 当前实现边界

当前可运行产品是 Electron + React 的本地 Studio；`studio/` 提供 renderer、受限 preload 与 Electron main，长期运行的 Rust host 通过 desktop service 调用 Rust Engine。Silverstack Metadata MVP 仍不在测试版范围；XMEML 已经可以生成，但 Premiere／Resolve 的真实导入、重连和再次导出仍必须按测试版验收清单人工确认。

```text
React renderer (studio/src/renderer/)
    │ frozen preload bridge / IPC
Electron main (studio/src/main/)
    │ framed local JSON protocol
Rust desktop host (crates/double-love-desktop-host/)
    │
Desktop service (crates/double-love-desktop-service/)
    │ typed DTOs and commands
Rust Engine (crates/double-love-engine/)
    ├── .doublelove/project.sqlite
    ├── .doublelove/manifest.json
    ├── .doublelove/cache/
    ├── .doublelove/logs/
    └── .doublelove/exports/
CLI (crates/double-love-cli/) ──┘
```

## 不变量

- 原始 XML、CSV 和媒体只读引用，不复制或覆盖。
- 路径不是稳定身份；后续实体使用 Stable ID、Revision 和来源记录。
- Apply 之前必须有 Preview/Dry Run；事务提交成功后才能发成功事件。
- 统一使用 `success | partial | failed | cancelled`，未知总量不发送伪百分比。
- React 不直接访问 SQLite、任意路径或 Shell；能力文件保持最小权限。

## 首批命令契约

所有 Studio 命令共用 `OperationResult<T>`、Revision 和本地 SQLite 边界。Metadata MVP 的占位命令保留为明确排除项，不能误认为测试版能力。

| 命令 | Foundation 状态 |
| --- | --- |
| `project_create` | 已建立 SQLite/WAL/外键/迁移表和 `.doublelove` 目录 |
| `project_open` | 已建立本地项目目录检查与 Revision 读取 |
| `import_media` / `transcribe` | 已接入本地媒体探测、版本化转录与取消保护 |
| `main_track_*` | 已接入多素材添加、排序、裁切、拆分、移除与 TimelineIR v2 |
| `speaker_diarize` | 已接入本地 VAD／声纹聚类、逐词归属和人工确认身份流程 |
| `project_export_*` | 已接入 ASS、烧录 MP4 和供 Premiere／Resolve 导入的 XMEML |
| `import_silverstack_preview` | 待 Metadata MVP 阶段接入 |
| `export_premiere_preview` | Metadata MVP 的旧入口；测试版使用 `project_export_preview` |
| `export_premiere_apply` | Metadata MVP 的旧入口；测试版使用 `project_export_xmeml_apply` |
| `task_cancel` | 已接入转录与说话人任务取消 |

## 验证边界

本机开发门禁覆盖 Rust workspace、Studio renderer、Electron 构建、目录包启动 smoke 与全量 Playwright：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
scripts/migration/check-bindings-contract.sh
pnpm --dir studio lint
pnpm --dir studio test
pnpm --dir studio build
pnpm --dir studio electron:build
CSC_IDENTITY_AUTO_DISCOVERY=false pnpm --dir studio pack:dir
scripts/migration/package-smoke.sh
pnpm --dir studio exec playwright test
```

代码与目录包门禁通过仍不等于 Developer ID 签名、公证、真实 Premiere／Resolve 导入、Relink、时间码和再导出验收通过；这些继续作为独立发布门槛。
