# Studio 当前架构

## 组件与依赖方向

```text
React renderer  studio/src/renderer/
      │ 冻结的 window.doubleLove API
sandbox preload studio/src/preload/
      │ 受保护 IPC
Electron main   studio/src/main/
      │ 4-byte length prefix + JSON protocol v1
Rust host       crates/double-love-desktop-host/
      │ 命令分派、事件转发
Desktop service crates/double-love-desktop-service/
      │ 桌面状态、偏好、项目、模型与任务编排
Rust engine     crates/double-love-engine/
      └ SQLite、媒体、转录、时间线与导出
```

`crates/double-love-cli/` 也调用 engine，但不经过 Electron。领域规则、SQLite
schema 和导出格式留在 engine；main 只负责操作系统能力与安全边界。

## Renderer、preload 与 main 边界

Renderer 没有 Node 或 Electron 权限，只能调用 preload 暴露的冻结 API。
窗口使用 `contextIsolation: true`、`sandbox: true`、
`nodeIntegration: false`、`webSecurity: true`，并禁用 webview。main 拒绝新
窗口、非应用导航和所有权限请求。

每个 IPC handler 都校验发送窗口角色、sender frame URL 和 8 MiB JSON
上限。命令名还要经过 renderer allowlist。renderer 不能提交任意路径：
main 的原生对话框签发按用途绑定、60 秒失效、一次消费的 grant token，
消费后才向 host 注入真实路径。

生产 renderer 只从 `dl-app://app` 读取 ASAR 内容。CSP 的
`connect-src 'none'` 禁止 renderer 直接联网；更新网络在 main，模型网络在
Rust service。`dl-media://asset/<id>` 只解析当前项目登记的媒体，支持
GET、HEAD 和单 Range，不把源路径交给 renderer。

## Host 协议

main 启动一个长期运行的 `double-love-desktop-host`。stdin/stdout 帧使用
4 字节大端长度前缀和 UTF-8 JSON，单帧上限 64 MiB。request/response
包含协议版本与 request id；事件没有 id。启动握手必须确认 protocol 1、
`invoke` capability、host version 与 engine version。

生成的 `bindings/host-protocol/schema/` 是 main 的运行时校验输入，Rust
serde 类型是 host 端校验输入。host 意外退出时，main 拒绝 pending，不重放
写命令，并记录 crash marker；正常退出先发送 `shutdown`，超时才终止进程。

## 安全加固

打包 hook 启用 Electron Fuse V1 策略：禁用 RunAsNode、`NODE_OPTIONS`、
CLI inspect 与额外 file privileges；启用 cookie 加密、ASAR 完整性、
OnlyLoadAppFromAsar、browser V8 snapshot 与 Wasm trap handlers。

renderer-facing 诊断会把项目、媒体与模型路径替换为占位符。进度自由文本
另有长度上限，并清除疑似声纹数值数组。reveal 命令由 main 打开 service
解析出的目录，再剥离返回路径。本地 main 日志只写字段 allowlist，不记录
payload、token、媒体文字、声纹或绝对路径。

## 数据目录

应用级目录固定为：

```text
~/Library/Application Support/space.ahua.doublelove.studio/
├── preferences.json
├── preferences.json.pre-electron-backup
├── models/                         # 默认模型根，可由用户迁移
└── logs/
    ├── main.jsonl                  # 轮转日志
    ├── main.jsonl.1 ...
    └── host-crash.json             # 仅意外 host 退出后存在
```

项目数据留在用户选择的项目目录：

```text
<project>/.doublelove/
├── project.sqlite
├── project.pre-electron-backup.sqlite
├── manifest.json
├── prepared/
├── cache/
├── logs/
└── exports/
```

原媒体仍是只读引用。打包资源位于应用的 `Contents/Resources/`：ASAR、host、
协议 schema、`runtime/` 与共享的 `model-runtime/`（一个 `.venv/bin/python`、
`double_love_asr/`、`double_love_speaker/`）。布局细节见
[Studio 打包](packaging.md)，运行问题见[排障指南](troubleshooting.md)。
