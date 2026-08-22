# 技术债热力图（按严重度排序）

严重度：**P0** 迁移阻断/安全风险；**P1** 必须在对应切片前处理；**P2** 迁移顺手清理；**P3** 记录即可。

| # | 严重度 | 位置 | 问题 | 处置阶段 |
| --- | --- | --- | --- | --- |
| D1 | P0 | tauri.ts:255-269,319-324,429-439 | renderer invoke wrapper 可直传绝对路径（打开/导入/导出）；其中 roughcut apply UI 未用，其余无授权边界 | 3（path grant） |
| D2 | P0 | tauri.ts:170-247；bindings/*.ts | ts-rs `bigint` 类型与 JSON number 运行时错位，靠 renderer `num()`/`normalize*` 兜底 | 3（协议整数策略） |
| D3 | P0 | engine model.rs:267-268 | engine `include_str!` 反向引用 `src-tauri/resources/model-catalog-v1.json`，删除 src-tauri 会先打断 engine | 5 之前必须移动事实源 |
| D4 | P1 | src-tauri/src/lib.rs（1083 行） | 66 命令大杂烩：状态持有、事件转发、资源定位、历史导航全在一个文件；Tauri 类型（AppHandle/State/Emitter）渗透每个函数 | 3-4（提取 service） |
| D5 | P1 | engine storage.rs（3128 行） | 单文件承载 schema V1–V10 + 全部读写 + 历史/说话人/主轨；改动半径大 | 4（不改行为，仅随切片补测试） |
| D6 | P1 | models.rs:296-455 | 下载队列/取消标志/进度节流的并发逻辑绑在 Tauri 命令层（线程+AppHandle emit），无独立可测边界 | 4 切片 5 |
| D7 | P1 | preferences.rs:453-466 `with_store` 签名绑定 `TauriStore<tauri::Wry>` | 偏好读写直接耦合 tauri-plugin-store 类型 | 4 切片 1 |
| D8 | P1 | media_protocol.rs:8,29 | 纯函数部分已可单测，但 `Response` 类型来自 `tauri::http` | 3（解耦为纯 Rust + main 适配） |
| D9 | P2 | App.tsx（799 行） | 单组件承载库/编辑器/任务/导出多屏状态；事件订阅散落在组件内（App.tsx:260-299） | 4（切片内渐进，不重写） |
| D10 | P2 | tauri.ts:253 | `isTauri` 浏览器降级与生产 fail-closed 语义混在一起 | 3（DesktopClient 显式化） |
| D11 | P2 | lib.rs:805,948-975 | 死接口：`speaker_save` + 4 个 `METADATA_MVP_PENDING` 占位 | 5（随 Tauri 适配器删除） |
| D12 | P2 | models.rs:694 | `dl://doctor-result` 已发送但 UI 未订阅（返回值已够用） | 4 切片 5（保留或下线需登记） |
| D13 | P2 | 根 package.json:21-23 | `tauri:*` 脚本与 `tauri:release` 硬编码 verify 门禁 | 5 |
| D14 | P3 | docs/studio-foundation.md、docs/studio-beta.md、CLAUDE.md | 文档以 Tauri 为当前架构叙述 | 5（归档+重写） |
| D15 | P3 | bindings/ 目录在仓库根 | ts-rs 产物与根 Web 项目同级，归属含糊 | 3（随 shared 契约重定位） |

测试空白（相对迁移风险）：renderer 侧无 E2E（仅 45 个 vitest 单测/组件测试）；media 协议只有 Rust 纯函数测试，无经 webview 的集成测试；偏好损坏恢复只有 Rust 单测，无端到端验证。阶段 2/3 由 Playwright Electron E2E 与安全测试补齐。
