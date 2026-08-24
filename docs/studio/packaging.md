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
    ├── .venv/bin/python             # ASR 与 Speaker 共用
    ├── double_love_asr/
    └── double_love_speaker/
```

图标源为 `studio/build/icon.png`；builder 转换后写入应用资源。
`runtime` 与 `model-runtime` 的 README 只是仓库占位，不能代替实际运行时。

## 准备与硬门禁

发布机从受控的本机路径准备资源。媒体使用两个源路径，模型使用一个共享
runtime root：

- `DOUBLELOVE_FFMPEG_SOURCE`
- `DOUBLELOVE_FFPROBE_SOURCE`
- `DOUBLELOVE_MODEL_RUNTIME_SOURCE`

```bash
scripts/prepare-media-runtime.sh
scripts/prepare-model-runtime.sh
scripts/verify-release-runtime.sh
```

如需从 sidecar 依赖声明构建共享 runtime 源目录，可运行：

```bash
scripts/migration/build-model-runtime.sh build/model-runtime-sources-shared
```

该脚本只要求 uv 支持 `uv python install` 和 `uv pip compile`，不固定 uv 版本；每次构建前会用当前 uv 将
`sidecars/model-runtime-requirements.in` 编译到临时文件，去掉 uv 自动生成的输出路径头部后，和仓库内 lockfile
正文严格比较。发生漂移时必须先更新 lockfile。依赖安装仍使用哈希校验，构建需要网络。

验证脚本必须在 `electron-builder` 前通过。它会拒绝缺少 libass 的 ffmpeg、
缺少共享 Python 或任一 sidecar 包的模型运行时、禁用依赖，以及仍引用构建机
Python 的普通 virtualenv。发布资源不再接受 `model-runtime/asr` 或
`model-runtime/speaker` 旧布局。
模型 runtime 先按 lockfile 完整安装并做 import/version 验证，再执行显式路径裁剪：
移除 pip、setuptools、wheel 及其对应 dist-info、`_distutils_hack`、
`pkg_resources`、标准库 `ensurepip`、锁定依赖中已知的 tests/test data/examples/docs，
以及所有 `__pycache__`/`.pyc`/`.pyo`。不会按泛化业务目录名删除；其他 dist-info、
LICENSE、METADATA、模型 assets、tokenizer 配置和 dylib/metallib/so 均保留。构建、
prepare、release verify 与 package smoke 共用同一组 import/version、禁用包、旧布局、
字节码和 ASR/Speaker mock hello 门禁。
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
