# ADR-0001：Tauri → Electron 目标架构

状态：已确认。日期：2026-08-22。

## 决定

仅替换桌面容器：Electron 承接窗口/菜单/对话框/协议/升级；**保留** `crates/double-love-engine`（领域与数据格式唯一事实源）与现有 CLI；原 `src-tauri` 业务适配提取为两个新 crate，由长期运行的 Rust host sidecar 承接。不把业务核心重写为 Node，不采用 Node-API 原生模块。

```text
React renderer
  └─ window.doubleLove（显式、冻结、类型化 API）
       └─ sandboxed preload（唯一 privileged bridge）
            └─ Electron main
                 ├─ BrowserWindow / Menu / Dialog / Update / 本地日志
                 ├─ 一次性路径授权 token
                 ├─ dl-media:// 只读媒体协议
                 └─ Rust host supervisor
                      └─ double-love-desktop-host（协议+分派+入口）
                           └─ double-love-desktop-service（状态/偏好/模型/诊断/编排）
                                └─ double-love-engine → .doublelove SQLite / exports / sidecars
现有 double-love CLI ──────────┘
```

## 代码边界

- `studio/src/main/`：窗口、菜单、host 生命周期、IPC 路由、path grant、媒体协议、日志、升级。
- `studio/src/preload/`：唯一 bridge；禁止暴露 `ipcRenderer`、Node API、通用 invoke。
- `studio/src/renderer/`：迁入现有 React 代码，只经 `platform/desktop` 用桌面能力。
- `studio/src/shared/`：无 Node/Electron 依赖的 renderer-facing 契约。
- `crates/double-love-desktop-service/`：从 `src-tauri` 提取；**不得依赖 Tauri/Electron**。
- `crates/double-love-desktop-host/`：协议、分派、入口；不含领域规则。
- 依赖方向由 ESLint `no-restricted-imports`、独立 tsconfig、Cargo 依赖关系强制；禁止 renderer→Node、preload→renderer、shared→platform、engine→desktop 反向依赖。

## Host 与 IPC 契约

- main 以 `shell:false` 启动唯一 host；开发期 `target/debug`，生产 `process.resourcesPath`。
- main↔host：4 字节长度前缀 + JSON envelope；协议版本固定 1；单帧上限 64 MiB；握手校验 protocol/host/engine 版本与能力列表。
- Rust 带 tag 的 method/result union 为事实源，外层统一使用版本化、可关联 envelope 并派生 serde + ts-rs + JSON Schema：request 为 `{"v":1,"id":"<client-id>","method":"...",...}`，response 为 `{"v":1,"id":"<echoed-id>","status":"ok","result":...}` 或 `status=error` + `error`；main 用生成 schema 运行时校验，host 再 serde 校验。v1 的 `u64`/`u32` 在线上均为 JSON number；ts-rs `bigint` 仅是类型层注解，runtime boundary 按既有规则转换并校验安全整数范围。
- 已用 invoke 命令在 host protocol v1 保持同名语义；`speaker_save` 与 4 个 `METADATA_MVP_PENDING` 占位不进入（死接口清理）。
- renderer 不发送任意文件路径：打开/导入/导出/模型目录迁移必须经 main 原生对话框产生的一次性 path grant，main 消费后注入真实路径。
- `dl-media://asset/<asset-id>` 只解析当前项目登记资产；保留 GET/HEAD/单 Range 的 200/206/404/416/501 语义（现状语义见 media_protocol.rs:1-4）。
- host 崩溃：拒绝全部 pending、不重放写操作；可重启一次，renderer 重新读取项目状态。正常退出先 shutdown，超时再 kill。
- 事件广播到主窗口与 settings 窗口；preload 回调剥离 Electron event 对象并返回注销函数。

## 非用户可见实现约束

以下是内部实现与安全边界，不是 behavior-change-register 中的用户可见行为变化：

- 媒体 URL 从 renderer 内部的 `media://localhost/<asset-id>` 换为 `dl-media://asset/<asset-id>`；GET/HEAD/单 Range 语义不变。
- `speaker_save` 与四个固定返回 `METADATA_MVP_PENDING` 的命令（`import_silverstack_preview`、`operation_apply`、`export_premiere_preview`、`export_premiere_apply`）无 renderer/CLI 调用，不进入 Electron host，随 Tauri adapter 清理。
- renderer 不再向 host 传任意绝对路径；打开、导入、当前 UI 使用的三种导出 apply 与模型目录迁移均消费 Electron main 原生对话框签发的一次性 path grant。对用户仍表现为同一原生选择流程。
- `roughcut_preview`、`export_roughcut_apply` 虽有 `tauri.ts` 封装但 UI 未调用；`pickSavePath` 同样未接线。它们属于能力账本中的 UI-unused 项，不作为现有用户行为。

## Application Support 路径兼容

macOS 数据目录由固定 identifier `space.ahua.doublelove.studio` 决定，必须继续为 `$HOME/Library/Application Support/space.ahua.doublelove.studio`。Electron main 必须在创建 session、窗口、Store 或启动 host **之前**执行等价配置：`app.setPath('userData', path.join(app.getPath('appData'), 'space.ahua.doublelove.studio'))`；其中 macOS 的 `appData` 是 `$HOME/Library/Application Support`。main 再把该已解析 `userData` 目录作为显式 host 参数（例如 `--app-data-dir`）传入，host 不得按 product name 或当前工作目录自行推导。这样 `preferences.json`、模型状态等继续读取 identifier 路径，不新建 `Double Love Studio` 命名的数据目录。

## 安全默认值

`contextIsolation:true`、`nodeIntegration:false`、`sandbox:true`、`webSecurity:true`、禁用 webview；生产只加载打包资源；CSP 默认 `connect-src 'none'`（模型下载与升级网络在 host/main）；拒绝新窗口/外部导航/未声明权限；Electron Fuses 关 RunAsNode/NODE_OPTIONS/CLI inspect，开 cookie 加密、ASAR 完整性与 only-load-from-ASAR。日志仅本地轮转：版本/进程/请求 id/方法/耗时/状态/错误码；不记绝对路径、媒体文本、声纹、token、完整 payload；无新增遥测。

## 发布与升级

electron-builder 打 arm64 DMG+ZIP（签名+公证）；GitHub Releases `studio-v0.2.0`；draft→人工批准→公开。updater 仅 packaged 启动后检查；提示→确认下载→确认重启；首次从 Tauri 切换为手动安装 0.2.0。

## 后果

- 正面：单一业务事实源（engine）不动，CLI/GUI 数据兼容天然保持；Rust 核心性能不变；Electron 生态（updater/builder/Playwright）成熟。
- 代价：包体积增大（单独记录审查）；新增 host 进程需要监督与协议治理；`media://` → `dl-media://` 为内部 URL 变化。
- 回退：阶段 5 清理前 Tauri 始终可构建；共享 schema 不变，旧 Tauri/CLI 可重新打开项目。
