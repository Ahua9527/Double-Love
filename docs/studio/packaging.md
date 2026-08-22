# Studio 打包

本文只描述当前 Electron 桌面包。发布步骤见[发布流程](release.md)。

## 包内布局

`studio/electron-builder.yml` 以 `studio/out/` 为应用代码输入，并生成
macOS 15+、arm64 的 DMG 与 ZIP。应用使用 ASAR：

```text
Double Love Studio.app/Contents/Resources/
├── app.asar                         # main、preload、renderer
├── double-love-desktop-host         # release Rust host
├── bindings/host-protocol/schema/   # host JSON Schema
├── runtime/
│   ├── ffmpeg                       # 必须包含 ass/libass filter
│   └── ffprobe
└── model-runtime/
    ├── asr/{double_love_asr,.venv/}
    └── speaker/{double_love_speaker,.venv/}
```

图标源为 `studio/build/icon.png`；builder 转换后写入应用资源。
`runtime` 与 `model-runtime` 的 README 只是仓库占位，不能代替实际运行时。

## 准备与硬门禁

发布机从受控的本机路径准备资源。四个源路径由环境变量提供：

- `DOUBLELOVE_FFMPEG_SOURCE`
- `DOUBLELOVE_FFPROBE_SOURCE`
- `DOUBLELOVE_ASR_RUNTIME_SOURCE`
- `DOUBLELOVE_SPEAKER_RUNTIME_SOURCE`

```bash
scripts/prepare-media-runtime.sh
scripts/prepare-model-runtime.sh
scripts/verify-release-runtime.sh
```

验证脚本必须在 `electron-builder` 前通过。它会拒绝缺少 libass 的 ffmpeg、
缺少 Python 的模型运行时，以及仍引用构建机 Python 的普通 virtualenv。
模型权重不放入应用包；用户仍通过模型管理器安装权重。

## 未签名本地包

目录包只用于开发 smoke，不是可分发候选：

```bash
cargo build --release -p double-love-desktop-host --locked
pnpm --dir studio install --frozen-lockfile
pnpm --dir studio exec electron-vite build
CSC_IDENTITY_AUTO_DISCOVERY=false pnpm --dir studio pack:dir
scripts/migration/package-smoke.sh
```

`electron-builder.yml` 保持 `notarize: false`，因此本地命令不会意外调用
Apple 公证。`package-smoke.sh` 检查 ASAR、host、schema、资源布局、Electron
Fuses，并从打包后的 `.app` 启动一次隔离的 Playwright smoke。它不证明
Developer ID 签名、公证或干净机器兼容性。

可显式传入应用路径：

```bash
scripts/migration/package-smoke.sh \
  "studio/release/mac-arm64/Double Love Studio.app"
```

## 已签名发布包

已签名包只由 `.github/workflows/studio-release.yml` 生成。工作流在受保护的
`studio-release` environment 中启用 `-c.mac.notarize=true`，并强制要求
Developer ID 签名。工作流生成完整 DMG/ZIP 后检查签名、公证票据、更新
元数据与 blockmap，再把文件上传到 GitHub 的草稿 Release。

不要把未签名目录包、跳过运行时验证的包，或本机手动生成的 DMG 当作
发布候选。候选的最终清单与人工放行步骤见[发布流程](release.md)。
