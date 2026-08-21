# Double Love Studio Foundation

> **2026-08-16 修订：桌面 UI 方向回调为 Tauri 2 + React。** GPUI 五区骨架落地并经负责人验收后，反馈 GPUI 过重（界面代码离开 AI 辅助难以自改、小交互迭代慢、生态小文档少担心长期维护、工具链需完整 Xcode + Metal），当日回调：壳复用并扩展现有 `src-tauri` 原型，界面为独立 `studio/` React 包（与 Web 版同款 React + Tailwind 技术栈）。下方「当前实现边界」架构图与命令契约本为 Tauri 时代所定，恢复有效；StudioController 角色由 `studio/` 前端与 `src-tauri` 命令层共同承担，不变量约束不变。GPUI 骨架作为视觉方向参照实现与技术归档留档于 `codex/gpui-foundation` 分支与已关闭的 PR #44，不合并进 main。Engine、`.doublelove` 存储与命令契约决策不变。

> **2026-08-15 修订（已被 2026-08-16 修订取代，留作历史记录）：桌面 UI 方向调整为 GPUI 原生。** 桌面界面由「Tauri 2 + React」改为 GPUI 原生应用（锁定 `gpui =0.2.2`、`gpui-component =0.5.1`、Rust 1.97.1），目标工作区布局为 `apps/double-love-studio` + `crates/double-love-engine` + `crates/double-love-cli`。DL-029 D0（GitHub Issue #41）原为设计确认门槛；2026-08-15 深夜由产品负责人口头豁免（两轮设计工具尝试未达预期，跳过独立设计稿阶段直接进入 GPUI 实现，视觉方向以负责人提供的参照截图为准：预览窗为视觉中心 + 底部时间线 + 卡片面板 + 极简左栏），实现分支为 `codex/gpui-foundation`。现有 Tauri 适配器保留为过渡 harness，Engine、CLI 与图标迁移完成后删除。本文其余 Engine、`.doublelove` 存储与命令契约决策不变；GPUI View 同样受下方不变量约束（不直接访问 SQLite、任意路径或 Shell），并经 StudioController 作为 View 与 Engine 的唯一连接层。

## 当前实现边界

当前可运行产品是 Tauri 2 + React 的本地 Studio；`studio/` 提供桌面界面，Rust Engine
负责本地项目、转录驱动粗剪和导出。Silverstack Metadata MVP 仍不在测试版范围；XMEML
已经可以生成，但 Premiere／Resolve 的真实导入、重连和再次导出仍必须按测试版验收清单人工确认。

```text
React UI (src/)
    │ Tauri invoke / events
Tauri adapter (src-tauri/)
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

本机已安装 macOS Command Line Tools、Apple Silicon stable Rust 1.97.1，并固定到 `rust-toolchain.toml`。首次 `cargo check --workspace` 因 crates.io 索引下载未完成而中止；在依赖缓存可用后必须运行：

当前 `tauri info` 还显示未安装完整 Xcode；Command Line Tools 已安装，当前环境可以生成未发布的 Apple Silicon `.app`，但桌面窗口、签名、公证和真实 Studio 行为仍不能视为已验收。

workspace 的 `cargo check`、`cargo fmt`、`cargo clippy`、`cargo test` 以及 Tauri 调试 `.app` 构建是代码门槛；真实桌面窗口、签名、公证、干净机器模型运行时和两款 NLE 的人工验收仍是独立门槛。

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

这三项通过仍不等于真实 Premiere 导入、Relink、时间码和再导出验收通过。
