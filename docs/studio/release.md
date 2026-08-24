# Studio 发布流程

Studio 发布只产生候选草稿；公开发布永远由人工完成。打包细节见
[Studio 打包](packaging.md)，升级与回退见
[升级与回退](upgrade-rollback.md)。

## 版本与 tag

`studio/package.json` 是 Studio 版本的唯一来源。tag 必须逐字等于：

```text
studio-v${studio/package.json version}
```

例如版本 `0.2.0` 只接受 `studio-v0.2.0`。工作流监听 `studio-v*`，但仍会
读取 package version 并做精确比较；不匹配立即失败。手动
`workflow_dispatch` 也必须输入一个已经存在的明确 tag，工作流会检出并
确认该 tag 指向当前 HEAD。

## 候选到公开

1. 先让常规 Studio quality gate 全部通过。
2. tag push 或手动 dispatch 启动 `studio-release.yml`。
3. job 进入受保护的 `studio-release` environment，等待 required reviewer
   人工批准；environment 未配置 required reviewer 时不得视为发布门禁。
4. 工作流准备并验证运行时，构建、测试、签名、公证，再验证候选文件。
5. `gh release create --draft --verify-tag` 只创建草稿
   GitHub Release 并上传 DMG、ZIP、`latest-mac.yml` 与两个 blockmap；builder 的
   `releaseType: draft` 同时锁住草稿状态，工作流最后再通过 GitHub API 断言 `draft=true`。
6. 在干净的受支持 Mac 上下载草稿候选，完成安装、Gatekeeper 首启、诊断、
   项目打开、代表性导入/转录/导出与更新元数据验收。
7. 验收记录无阻断项后，由有权限的人在 GitHub 页面手动 Publish。

工作流没有把 Release 改为公开的步骤，也不得增加这种步骤。验收失败时
保留草稿或删除草稿，修复后重新生成候选；不要公开不完整资产。

## 凭据与发布机配置

凭据只存放在 GitHub `studio-release` environment 的 secrets 中。工作流
只引用以下 secret 名，不读取或记录其值：

- Developer ID：`CSC_LINK`、`CSC_KEY_PASSWORD`、`CSC_NAME`
- Apple ID 公证：`APPLE_ID`、`APPLE_APP_SPECIFIC_PASSWORD`、
  `APPLE_TEAM_ID`
- 公证替代路径：`APPLE_KEYCHAIN_PROFILE`；非默认 keychain 另用
  `APPLE_KEYCHAIN`
- GitHub 上传：GitHub Actions 自动提供的 `GITHUB_TOKEN`

Apple ID 三项与 keychain profile 二选一。运行时源路径不是凭据，放在同一
protected environment 的 variables：`DOUBLELOVE_FFMPEG_SOURCE`、
`DOUBLELOVE_FFPROBE_SOURCE`、`DOUBLELOVE_MODEL_RUNTIME_SOURCE`。

## 候选验证

工作流对解包后的 `.app` 和 DMG 执行等价检查：

```bash
codesign -vvv --deep --strict \
  "studio/release/mac-arm64/Double Love Studio.app"
spctl --assess --type exec --verbose=4 \
  "studio/release/mac-arm64/Double Love Studio.app"
xcrun stapler validate \
  "studio/release/mac-arm64/Double Love Studio.app"
xcrun stapler validate "studio/release/Double Love Studio-<version>-arm64.dmg"
```

随后运行：

```bash
scripts/migration/package-smoke.sh \
  "studio/release/mac-arm64/Double Love Studio.app"
```

## 草稿资产清单

草稿 Release 缺少任何一项都不合格：

- 一个 arm64 DMG
- 一个 arm64 ZIP（macOS updater 使用）
- `latest-mac.yml`
- DMG 对应的 `.dmg.blockmap`
- ZIP 对应的 `.zip.blockmap`

公开前还要确认文件名中的版本与 tag 一致，`.app` 和 DMG 的 stapler
验证通过，并在干净机器完成验收。公开动作始终是最后的人工步骤。
