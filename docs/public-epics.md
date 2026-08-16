# Double Love 公开脱敏 Epic 目录

> 这是公开任务系统的导航草案。Issue 不包含生产文件名、真实路径、原始 XML/CSV、模型未冻结参数、凭据或个人信息。

| Epic | 目标 | 当前边界 |
| --- | --- | --- |
| [Web/PWA Stabilization](https://github.com/Ahua9527/Double-Love/issues/17) | 收口现有浏览器工具的正确性、可访问性、PWA 和测试门槛 | DL-001～DL-014 |
| [csv2xml & Format Lab](https://github.com/Ahua9527/Double-Love/issues/25) | 冻结时间基、字段映射、Golden Fixture 和 NLE 语义 Diff | DL-015～DL-028 |
| [Rust/GPUI Desktop Foundation](https://github.com/Ahua9527/Double-Love/issues/18) | 建立 Rust Engine、权限、项目目录、SQLite、统一契约与 GPUI 原生桌面基础；DL-029 D0 设计确认是界面代码前置依赖 | DL-029～DL-033、DL-074 |
| [Silverstack→Premiere Metadata MVP](https://github.com/Ahua9527/Double-Love/issues/19) | 完成第一个 Metadata round-trip 闭环 | 后续按 DL-034 起拆分；DL-042 已拆分为 #42，被 DL-029 D0 阻断 |
| [Speech Core](https://github.com/Ahua9527/Double-Love/issues/20) | 本地转录、对齐和说话人能力 | Metadata MVP 通过后再拆 |
| [Editorial Intelligence](https://github.com/Ahua9527/Double-Love/issues/21) | Proposal、Search、Selects 和 Soft Edit | Speech Core 通过后再拆 |
| [Interchange Expansion](https://github.com/Ahua9527/Double-Love/issues/22) | Resolve、AAF 等格式扩展 | 先保持 Epic |
| [Automation](https://github.com/Ahua9527/Double-Love/issues/23) | Recipe、Dry Run、确认点和阻断交还 | 先保持 Epic |
| [NLE Bridges](https://github.com/Ahua9527/Double-Love/issues/24) | Premiere/Resolve/Avid Bridge 和同步协议 | 先保持 Epic |

## Issue 统一模板

每个公开 Issue 必须包含：

- DL 编号与结果目标；
- 脱敏证据和依赖；
- 明确实施边界；
- 自动化验收与回归门槛；
- 需要时的人工 Premiere/Resolve/NLE 验收；
- 隐私检查：不得上传生产素材、真实路径、凭据或未公开模型细节。

## 标签约定

固定前缀和标签：`priority:*`、`area:*`、`type:*`、`needs-nle`、`privacy-review`、`research`、`blocked`。

优先级含义：P0 安全阻断，P1 结果正确性，P2 质量与体验，P3 长期维护。

## 第一轮可执行 Issue

公开脱敏拆分已完成：

- DL-001～DL-004：[#26](https://github.com/Ahua9527/Double-Love/issues/26)、[#27](https://github.com/Ahua9527/Double-Love/issues/27)、[#28](https://github.com/Ahua9527/Double-Love/issues/28)、[#29](https://github.com/Ahua9527/Double-Love/issues/29)
- DL-005～DL-008：[#30](https://github.com/Ahua9527/Double-Love/issues/30)、[#31](https://github.com/Ahua9527/Double-Love/issues/31)、[#32](https://github.com/Ahua9527/Double-Love/issues/32)、[#33](https://github.com/Ahua9527/Double-Love/issues/33)
- DL-009～DL-012：[#34](https://github.com/Ahua9527/Double-Love/issues/34)、[#35](https://github.com/Ahua9527/Double-Love/issues/35)、[#36](https://github.com/Ahua9527/Double-Love/issues/36)、[#37](https://github.com/Ahua9527/Double-Love/issues/37)
- DL-014：[#38](https://github.com/Ahua9527/Double-Love/issues/38)

## 第二轮可执行 Issue（Studio 桌面方向，2026-08-15）

- DL-029（治理/门槛）：[#41](https://github.com/Ahua9527/Double-Love/issues/41)——D1 Engine·CLI 边界 / D2 GPUI 技术原型；D0 设计确认已于 2026-08-15 由产品负责人豁免（跳过设计稿阶段，视觉方向以参照截图为准），实现分支 `codex/gpui-foundation`。
- DL-042（基础 GUI 工作区）：[#42](https://github.com/Ahua9527/Double-Love/issues/42)——原被 DL-029 D0 阻断，已随豁免解除；界面按参照截图方向实现。
- DL-074（桌面无障碍）：[#43](https://github.com/Ahua9527/Double-Love/issues/43)——首阶段暂缓；不阻断内部 Metadata MVP，阻断第一个外部测试版。

## 权威来源关系

GitHub Issues 是公开任务来源；本地 `TODO.md` 仅用于迁移和审计，迁移完成后不再作为权威状态。完整 PRD、真实文件名、生产数据、未冻结模型参数和内部验收细节留在私有工作区。
