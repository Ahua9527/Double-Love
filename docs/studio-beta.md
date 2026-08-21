# Double Love Studio 测试版

## 产品边界

Double Love Studio 是一个本地运行的转录驱动粗剪工作台：

- 把多个视频素材放入主轨后，可排序、裁切、拆分、移除。
- 每个素材保留自己的帧率与源时间码；输出帧率默认跟随第一段主轨。
- 文本删减会投影到主轨、字幕、MP4 和 XML。
- 本地说话人分离、逐词归属、姓名候选和跨素材合并都必须由用户确认。
- 字幕样式与画布变换是项目级设置。
- 项目历史可恢复主轨、输出帧率、画布、字幕样式、文字删减和说话人身份／归属；恢复本身会生成新版本。
- 声纹向量只保存在当前项目的本地数据库，不进入项目历史、日志、Agent 数据包或导出物。
- 输出 ASS、带字幕 MP4，以及供 Premiere／Resolve 导入的 XMEML。

不进入首个测试版：OCR、画面文字替换、自由文字元素、B-roll、画中画、逐片段变换、关键帧、变速、自定义字体和跨项目声纹档案。

## 本地开发

    pnpm install
    pnpm --dir studio build
    cargo test --workspace --offline
    pnpm tauri:dev

模型运行时单独准备；这一步只发生在开发或发布机器上：

    bash scripts/prepare-asr.sh
    bash scripts/prepare-speaker.sh
    cargo run -p double-love-cli -- --json doctor
    cargo run -p double-love-cli -- --json model-test asr
    cargo run -p double-love-cli -- --json model-test speaker

model-verify 会强制离线加载本地权重。缺少权重时它只报错，不会自动联网下载：

    cargo run -p double-love-cli -- --json model-verify asr
    cargo run -p double-love-cli -- --json model-verify speaker

## 发布前运行时门禁

测试版发布包必须随 App 带上：

- 含 libass 的 ffmpeg / ffprobe；
- 可重定位的 ASR Python 运行时；
- 可重定位的 Speaker Python 运行时。

发布机器先准备并检查这些资源：

    bash scripts/prepare-media-runtime.sh
    bash scripts/prepare-model-runtime.sh
    pnpm tauri:release

tauri:release 会在打包前运行 verify-release-runtime.sh。没有 libass 或模型运行时会直接失败，不能把依赖 Homebrew/Python 的开发包当成可安装测试版。

## NLE 人工验收

每次候选发布在 Premiere 和 Resolve 各做一次真实导入：

1. 混编 23.976、24、25、29.97 DF、29.97 NDF、30、50、59.94、60 的授权素材。
2. 验证添加、排序、左右裁切、播放头拆分、移除与文字删减后的切点。
3. 验证媒体自动链接或手动重连、源时间码、音画同步与字幕时间。
4. 切点误差不超过一帧，并在两端继续编辑后重新导出。
5. 若 Resolve 的 XMEML 导入丢失混合帧率、时间码或音频关系，再启用 FCPXML Adapter；在此之前不承诺它。

XMEML 的 Compatibility Report 会明确标出无法等价保留的字幕视觉样式和画布属性；ASS 与烧录 MP4 才是完整视觉样式的交付物。
