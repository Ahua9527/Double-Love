# YumLog 复盘对 Double Love 的工程启示

> 研究日期：2026-08-12
>
> 本笔记只记录公开文章中的可迁移工程原则和 Double Love 的推论，不包含生产文件名、真实素材、模型未冻结细节、凭据或用户数据。

## 资料边界

### 文章中直接描述的事实

《[制作我的第一款 iOS App: 干饭手册](https://fenx.work/craft-my-first-ios-app-yumlog/)》记录了一个本地优先、带图片处理、AI 辅助、云同步和订阅能力的 iOS 产品开发过程。文章反复回到这些工程问题：本地状态与云端状态的切换、原始图片与派生图片的区分、批量任务的真实计数、启动状态的未知值、分享/同步边界、性能和本地与生产环境差异。

这些内容是作者对 YumLog 的复盘，不是 Double Love 的产品需求，也不是 Apple 或 Adobe 的兼容性承诺。

### 对 Double Love 的推论

1. 先完成一个可验证的真实闭环：Silverstack XML+CSV → 匹配/审查 → Premiere Package → 导入、重连、再导出对账。
2. 先冻结数据契约、状态模型、诊断和失败语义，再扩展 AI、同步、多平台或商业化。
3. 原始引用、派生值、当前生效值、来源、冲突和人工决定必须是显式数据，不能由文件名或路径隐含表达。
4. 只有在总量可测时才显示百分比；未知工作量显示阶段和已完成事实，不制造伪精度。
5. “按钮执行过”“文件写出来”“部分对象成功”都不能单独代表成功；提交事务、输出物和验收证据要分别记录。
6. 真实 Premiere 导入、Relink、时间码、音轨、Metadata 和再导出仍属于人工 NLE 验收，不能被单元测试替代。
7. CloudKit、账号、订阅、小组件和 iOS 动效是 YumLog 的上下文，不应机械搬入 Double Love。

### 第一方资料核验

- [Tauri 2 前置条件](https://v2.tauri.app/start/prerequisites/)：桌面开发需要 Rust；macOS 需要 Xcode 或 Command Line Tools。
- [Tauri 调用 Rust](https://v2.tauri.app/develop/calling-rust/)：耗时命令应使用异步命令，避免阻塞界面。
- [Tauri Capabilities](https://v2.tauri.app/security/capabilities/)：能力文件用于限制窗口可使用的权限和命令。
- [Tauri Sidecar](https://v2.tauri.app/develop/sidecar/)：外部二进制需要显式打包和权限声明，Apple Silicon 目标需要对应 target 后缀。
- [rusqlite](https://github.com/rusqlite/rusqlite)：应用可选择 bundled SQLite，避免依赖系统 SQLite 版本。
- [ts-rs](https://docs.rs/ts-rs/latest/ts_rs/)：可从 Rust 类型生成 TypeScript 绑定，减少手工维护两套接口的漂移。

## 应用于当前排期

### 先收口 Web/PWA

先完成 DL-001～DL-012、DL-014 的代码、自动化、浏览器和人工验收；`processXML()` 的结构化结果是后续 Engine 状态契约的窄版先行实现。旧 Web XML 核心不在这一阶段做大规模重构，DL-013 暂缓。

### 再做 Studio Foundation

Tauri 2 只负责桌面窗口、权限和命令适配；React 不直接访问 SQLite、任意路径或 Shell；纯 Rust Engine 负责项目存储、稳定 ID、Revision、Operation Log 和统一结果类型。首阶段仅生成 Apple Silicon/macOS 15 内部 `.app`，不承诺签名、公证、自动更新或 App Store。

### 再做 Metadata MVP

Metadata MVP 只在一个脱敏真实项目完成 Silverstack XML+CSV → Graph → Review → Premiere Package → 导入/重连 → 再导出对账后关闭。没有真实 Premiere 环境时，Issue 只能标记“代码完成，等待 NLE 验收”。

### WIP 限制

同时进行的工作最多为：一个实现包、一个等待 Premiere/Resolve 验收包、一个研究任务。新想法先进入 Epic 候选清单，不直接打断当前闭环。
