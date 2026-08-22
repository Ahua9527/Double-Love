# 用户可见行为矩阵（Tauri 现状 → Electron 目标）

来源：`studio/src/App.tsx`（799 行）、`studio/src/tauri.ts`、`src-tauri/`。所有 “保持” 项在阶段 4 对应切片中做 Tauri/Electron 对照验收。

| # | 行为 | 现状证据 | Electron 目标 |
| --- | --- | --- | --- |
| B1 | 首次引导（3 步，完成/重置，onboarding_version=1） | preferences.rs:26,860-978 | 保持 |
| B2 | 项目库：最近项目 ≤20、去重、exists 标记、可移除 | preferences.rs:27,650-742 | 保持 |
| B3 | 创建/打开项目（原生目录对话框） | lib.rs:198-280；tauri.ts:255-260,443-445 | 保持 |
| B4 | 主题 light/dark/system；启动后从持久化 `AppPreferencesV1.theme` 恢复 | App.tsx:70-72,178-183,300-304；tauri.ts:477-483 | 保持；持久化偏好是事实源（`studio.theme` 只用于偏好返回前的初始绘制） |
| B5 | 设置单例窗口 + `Cmd+,` + 关闭即隐藏 | settings_window.rs:12-58；lib.rs:993-1008 | 保持 |
| B6 | 模型目录、下载/暂停/恢复/取消/校验/删除、目录迁移（复制+校验） | models.rs:83-126,541-713 | 保持；网络在 host |
| B7 | 诊断报告 + 打开日志目录 | models.rs:715-772 | 保持；日志仍本地 |
| B8 | 媒体导入（mp4/mov/m4v/webm 过滤器，ffprobe 校验，16kHz 准备音频） | tauri.ts:267-269,447-452；lib.rs:282-302 | 保持 |
| B9 | 媒体播放/seek + 下一段 preload | TimelinePreview.tsx:121,164 | 保持（内部媒体 transport 见 ADR-0001，不作为用户行为） |
| B10 | 主轨 append-full/move/trim/split/remove、undo/redo、历史恢复 | lib.rs:431-524,684-747 | 保持 |
| B11 | 画布、输出帧率（含“跟随首段”）、字幕样式 | lib.rs:750-833 | 保持 |
| B12 | 转录/取消、词删改/恢复；失败/取消不切活动转录 | lib.rs:310-599；engine transcribe 测试 | 保持 |
| B13 | 说话人：分离、候选、Agent 包预览、确认改名/合并 | lib.rs:796-945 | 保持 |
| B14 | 导出：统一 preview、XMEML/ASS/MP4 apply + sha256 落账 | lib.rs:573-631；tauri.ts:425-439,462-469 | 保持 |
| B15 | 外部 CLI 修改 → revision 冲突刷新提示（不覆盖新状态） | lib.rs:362-368 | 保持 |
| B16 | 浏览器 dev 降级（无壳时“未连接”） | tauri.ts:253 | 保持 dev preview；生产 fail closed |
| B17 | **新增（已批准）**：启动后检查更新 → 提示且用户确认后下载 → 完成后确认重启安装 | 无（Tauri 无 updater） | Electron-only，见 behavior-change-register.md |

`utils.ts:245-282` 的 `studio.panels` 读写 helper 目前只有 `utils.test.ts` 调用，产品 UI 未接线；因此面板状态持久化、迁移或重置都不是现有用户行为，也不列入迁移保持项。

窗口规格：主窗口 1440×900（min 960×640）；设置 760×580（min 700×500）。证据：tauri.conf.json:14-20、settings_window.rs:24-25。
