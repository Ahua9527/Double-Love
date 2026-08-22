# 行为变化登记

规则：任何数据格式、用户流程、交互语义、外部接口变化必须在此单独登记并经主会话确认。本表只列已批准的**用户可见行为**变化；非用户可见的媒体 scheme、内部死命令处置与 path grant 是实现约束，登记在 [ADR-0001](adr-0001-target-architecture.md#非用户可见实现约束)，不冒充已批准行为变化。本表之外的用户可见变化一律视为未批准。

| # | 变化 | 类别 | 状态 | 说明 |
| --- | --- | --- | --- | --- |
| C1 | 桌面容器 Tauri → Electron | 平台容器 | **已批准** | 用户可见行为保持（见 behavior-matrix）；bundle id / product name / identifier 派生的 Application Support 路径不变 |
| C2 | 新增应用内更新提示流：启动后检查 → **提示并由用户确认后才下载** → 完成后确认重启安装；源为 GitHub Releases 非 draft 非 prerelease 签名版本 | 新增用户流程 | **已批准** | 首次 Tauri→Electron 为手动安装 0.2.0；启动检查失败只写脱敏日志，手动检查失败才显示错误；不静默下载/自动退出 |

明确**不变**：`.doublelove` SQLite schema 1–10、manifest、TimelineIR、偏好 schema v1 与文件结构、模型安装 schema、导出格式与 golden、CLI 命令/JSON/退出码、sidecar 协议、Web/PWA 全部行为。

首发版本：Studio **0.2.0**（tag `studio-v0.2.0`，与 `studio/package.json` 版本一致）。
