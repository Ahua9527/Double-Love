# Studio 排障

先保留现场，再做可逆操作。不要直接编辑应用包、模型状态 JSON 或项目
SQLite，也不要把项目、媒体、日志或凭据上传到公开 issue。

## 日志与诊断页

应用级日志目录：

```text
~/Library/Application Support/space.ahua.doublelove.studio/logs/
```

`main.jsonl` 是当前 main/host/updater 字段白名单日志，旧文件按
`main.jsonl.1` 等轮转。项目内的转录与说话人任务日志位于
`<project>/.doublelove/logs/`。

打开“设置 → 诊断”，先点“运行诊断”。诊断页只检查 App 内置的
ffmpeg/ffprobe、libass、H.264/AAC 编码能力、模型完整性、离线 Python 运行时和模型目录剩余空间；不会回退到 PATH、Homebrew 或系统 Python。
“打开日志目录”只打开上述应用日志目录，不把路径返回 renderer。

报告问题前保存：应用版本、macOS 版本、诊断项状态、可复现步骤和相关日志
时间段。移除项目路径、媒体文字、token 或其他隐私内容后再分享。

## Host 意外退出

意外退出会留下：

```text
~/Library/Application Support/space.ahua.doublelove.studio/logs/host-crash.json
```

marker 只含时间、exit code 与 signal；下一次 host 握手成功会清除它。
安全处理顺序：

1. 退出 Studio，复制 `logs/` 与当前项目的 `.doublelove/logs/`。
2. 重新启动一次并运行诊断；不要在失败期间重复执行写操作。
3. marker 被清除且项目状态正常时，再继续工作。
4. marker 反复出现时，保留项目副本并重新安装同版本的已签名正式包；不要
   删除 `project.sqlite` 或一次性迁移备份。

## libass 不可用或 MP4 渲染失败

正式包应自带 `runtime/ffmpeg` 与 `runtime/ffprobe`，其中 ffmpeg 必须包含
`ass` filter、`libx264` 和 `aac`。诊断显示 App 内置媒体运行时不可用时：

1. 不要用 Homebrew 覆盖应用包内二进制，也不要绕过签名修改 `.app`。
2. 从正式 Release 重新下载 DMG，验证后重装同版本。
3. 在修复前可继续导出不依赖媒体 runtime 的 XMEML 或 ASS；不要把失败 MP4 当成
   完整输出。
4. 发布机运行 `scripts/verify-release-runtime.sh`；它失败就不能
   生成候选。

## 模型未就绪

`MODEL_NOT_READY`、校验失败或暂停状态通常表示权重未完整安装，而不是项目
损坏：

1. 在“设置 → 模型”确认依赖项、剩余空间和状态。
2. 暂停项使用“继续”；已安装但异常的项先运行“验证”。
3. 保留 `.part` 与 staging，让模型管理器恢复；不要手动改名或拼接文件。
4. 只有 UI 明确允许时才移除并重新安装。共享依赖仍被模型使用时不要强删。
5. 模型恢复前保留项目，转录或说话人任务不会自动假装成功。

## 更新检查、下载或安装失败

草稿 Release 不可见，prerelease 也被稳定更新流忽略。正式更新必须同时有
公开 Release、ZIP、`latest-mac.yml` 与对应 blockmap。

1. 在“设置 → 关于”手动检查一次，确认网络可访问 GitHub。
2. 下载失败时退出并重开应用后再试一次；普通退出不会静默安装。
3. 已下载后仍需第二次“重启安装”确认。取消确认不会损坏当前版本。
4. 仍失败时，从公开 Release 手动下载 DMG，先验证 DMG 票据：

```bash
xcrun stapler validate "Double Love Studio-<version>-arm64.dmg"
```

退出旧版并拖入 Applications 后，再验证已安装应用：

```bash
spctl --assess --type exec --verbose=4 \
  "/Applications/Double Love Studio.app"
```

发布人员若发现元数据或 blockmap 缺失，必须保持 Release 为草稿并重新构建，
不能手工公开部分资产。需要降级或恢复迁移前数据时，使用
[升级与回退](upgrade-rollback.md)，不要让 updater 承担降级。
