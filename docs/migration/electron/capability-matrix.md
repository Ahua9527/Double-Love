# Tauri 能力矩阵

登记来源：`src-tauri/src/lib.rs`（invoke_handler 注册表 `lib.rs:1013-1078`）、`preferences.rs`、`models.rs`、`settings_window.rs`、`media_protocol.rs`、`tauri.conf.json`、`capabilities/default.json`、`studio/src/tauri.ts`（renderer 唯一封装）、`.github/workflows/studio-quality.yml`、`scripts/`。

分类：**在用** = 产品 UI 实际调用；**UI 未用** = 已注册但产品 UI 无调用（可能只有 `studio/src/tauri.ts` wrapper）；**占位** = 固定返回 `METADATA_MVP_PENDING`；**平台能力** = 窗口/菜单/对话框/Store/协议等容器能力。

## 1. 已注册 Tauri 命令（66 个）

### 偏好 / 引导 / 设置（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `settings_open` | settings_window.rs:70 | `settingsOpen()` | 打开/聚焦 settings 单例窗口 |
| `preferences_get` | preferences.rs:476 | `preferencesGet()` | 读偏好，损坏时恢复默认+warning |
| `preferences_update` | preferences.rs:502 | `preferencesUpdate()` | patch 更新；model_root 变更触发目录迁移 |
| `recent_projects_list` | preferences.rs:682 | `recentProjectsList()` | 最近项目（≤20，含 exists） |
| `recent_project_forget` | preferences.rs:700 | `recentProjectForget()` | 移除最近项目 |
| `system_profile` | preferences.rs:870 | `systemProfile()` | 内存/架构/OS/磁盘/推荐模型 |
| `onboarding_get` | preferences.rs:890 | `onboardingGet()` | 引导状态 |
| `onboarding_complete` | preferences.rs:907 | `onboardingComplete()` | 完成引导（步骤 1–3 校验） |
| `onboarding_reset` | preferences.rs:958 | `onboardingReset()` | 重置引导 |

### 模型与诊断（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `model_catalog` | models.rs:541 | `modelCatalog()` | 目录+安装状态快照 |
| `model_install` | models.rs:560 | `modelInstall()` | 依赖序下载（后台线程） |
| `model_pause` | models.rs:609 | `modelPause()` | 暂停 |
| `model_resume` | models.rs:637 | `modelResume()` | 恢复=重新 install |
| `model_cancel` | models.rs:623 | `modelCancel()` | 取消（0 字节→NotInstalled） |
| `model_verify` | models.rs:646 | `modelVerify()` | 完整性校验 |
| `model_remove` | models.rs:666 | `modelRemove()` | 删除 |
| `model_reveal` | models.rs:686 | `modelReveal()` | Finder 打开模型目录 |
| `doctor_run` | models.rs:715 | `doctorRun()` | 环境诊断报告 |
| `diagnostics_reveal_logs` | models.rs:758 | `diagnosticsRevealLogs()` | 打开日志目录 |

### 项目生命周期（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `project_create` | lib.rs:229 | `projectCreate()` | 创建+默认字幕样式+最近项目 |
| `project_open` | lib.rs:282 | `projectOpen()` | 打开+最近项目 |
| `project_revision` | lib.rs:362 | `projectRevision()` | 当前 revision（外部 CLI 冲突检测） |
| `project_history` | lib.rs:371 | `projectHistory()` | 历史（默认 80） |
| `project_restore_revision` | lib.rs:385 | `projectRestoreRevision()` | 恢复到指定 revision |
| `edit_undo` | lib.rs:431 | `editUndo()` | 撤销（HistoryNavigation） |
| `edit_redo` | lib.rs:474 | `editRedo()` | 重做 |

### 媒体（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `import_media` | lib.rs:318 | `importMedia(path)` | ffprobe 校验+准备 16kHz 音频；**renderer 直传绝对路径** |
| `assets_list` | lib.rs:341 | `assetsList()` | 资产列表 |

### 转录 / 任务（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `transcribe_start` | lib.rs:347 | `transcribeStart()` | 异步任务，进度走事件 |
| `task_cancel` | lib.rs:352 | `taskCancel()` | 取消任务 |
| `transcript_get` | lib.rs:562 | `transcriptGet()` | 词+分段+omit |
| `edit_omit` | lib.rs:568 | `editOmit()` | 删除词区间（默认 120ms handles） |
| `edit_restore` | lib.rs:587 | `editRestore()` | 恢复 omit |

### 主轨 / 项目设置（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `timeline_get` | lib.rs:676 | `timelineGet()` | TimelineIR v2 |
| `main_track_append_full` | lib.rs:693 | `mainTrackAppendFull()` | 整段追加 |
| `main_track_list` | lib.rs:702 | `mainTrackList()` | 主轨列表 |
| `main_track_move` | lib.rs:711 | `mainTrackMove()` | 移动 |
| `main_track_trim` | lib.rs:722 | `mainTrackTrim()` | 裁剪（源帧） |
| `main_track_split` | lib.rs:734 | `mainTrackSplit()` | 拆分 |
| `main_track_remove` | lib.rs:744 | `mainTrackRemove()` | 移除 |
| `canvas_get` / `canvas_set` | lib.rs:750 / 758 | `canvasGet/Set()` | 画布 |
| `output_rate_get` / `output_rate_set` | lib.rs:774 / 784 | `outputRateGet/Set()` | 输出帧率（None=跟随首段） |
| `subtitle_style_get` / `subtitle_style_set` | lib.rs:811 / 821 | `subtitleStyleGet/Set()` | 字幕样式 |
| `apply_default_subtitle_style` | lib.rs:836 | `applyDefaultSubtitleStyle()` | 应用偏好默认样式 |

### 说话人（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `speaker_list` | lib.rs:796 | `speakerList()` | 身份列表 |
| `speaker_name_proposals` | lib.rs:816 | `speakerNameProposals()` | 本地姓名候选 |
| `speaker_agent_payload_preview` | lib.rs:828 | `speakerAgentPayloadPreview()` | Agent 最小数据包预览 |
| `speaker_name_confirm` | lib.rs:843 | `speakerNameConfirm()` | 确认改名（confirmed=true） |
| `speaker_merge_confirm` | lib.rs:868 | `speakerMergeConfirm()` | 确认合并 |
| `speaker_diarize_start` | lib.rs:895 | `speakerDiarizeStart()` | 本地分离（声纹只留项目库） |
| `speaker_diarization_get` | lib.rs:936 | `speakerDiarizationGet()` | 分离结果 |

### 导出（在用）

| 命令 | 注册位置 | renderer 封装 | 说明 |
| --- | --- | --- | --- |
| `project_export_preview` | lib.rs:575 | `projectExportPreview()`（tauri.ts:425-427） | 统一预览 |
| `project_export_xmeml_apply` | lib.rs:582 | `projectExportXmemlApply(targetPath)`（tauri.ts:429-431） | XMEML；**直传路径** |
| `project_export_ass_apply` | lib.rs:596 | `projectExportAssApply(targetPath)`（tauri.ts:433-435） | ASS；**直传路径** |
| `project_render_mp4_apply` | lib.rs:610 | `projectRenderMp4Apply(targetPath)`（tauri.ts:437-439） | 烧录 MP4；**直传路径** |

### UI 未用（已注册，renderer 无调用）

| 命令 | 注册位置 | 处置 |
| --- | --- | --- |
| `main_track_append` | lib.rs:643 | 区间追加；`tauri.ts` 只封装 `append_full`。Electron host protocol v1 保留同名语义 |
| `roughcut_preview` | lib.rs:546 | `roughcutPreview()` 仅在 tauri.ts:315-317 封装，UI 无调用；不列为用户可见行为 |
| `export_roughcut_apply` | lib.rs:555 | `exportRoughcutApply()` 仅在 tauri.ts:319-324 封装，UI 无调用；不列为用户可见行为 |
| `speaker_save` | lib.rs:805 | `tauri.ts` 与 CLI 均无调用 → **不进入 Electron**，随 Tauri 适配器删除（内部死接口清理） |

### 占位（固定返回 METADATA_MVP_PENDING，lib.rs:948-975）

`import_silverstack_preview`、`operation_apply`、`export_premiere_preview`、`export_premiere_apply` —— renderer/CLI 均未调用 → **不进入 Electron**，随 Tauri 适配器删除（非用户可见的内部死接口清理，登记于 ADR-0001）。

## 2. 事件（Rust emit → renderer listen）

| 事件 | 发送位置 | renderer 订阅 | 状态 |
| --- | --- | --- | --- |
| `dl://progress` | lib.rs:77 | App.tsx:260 | 在用 |
| `dl://task-state` | lib.rs:82 | App.tsx:270 | 在用 |
| `dl://model-progress` | models.rs:426 | App.tsx:281, SettingsApp.tsx:259 | 在用 |
| `dl://model-state` | models.rs:184 | App.tsx:289, SettingsApp.tsx:264 | 在用 |
| `dl://preferences-changed` | preferences.rs:512 | App.tsx:299, SettingsApp.tsx:197 | 在用 |
| `dl://doctor-result` | models.rs:694 | 无（doctor_run 直接返回报告） | 已发送但 UI 未订阅 |

## 3. 平台能力

| 能力 | 证据 | Electron 对应 |
| --- | --- | --- |
| 主窗口 1440×900 / min 960×640，标题 “Double Love Studio” | tauri.conf.json:12-21；前端自绘拖拽 TitleBar（index.css:35，`-webkit-app-region: drag`） | BrowserWindow hiddenInset |
| 设置单例窗口 760×580 / min 700×500，`index.html?window=settings`，关闭=隐藏 | settings_window.rs:12-31；lib.rs:1000-1008；main.tsx:8 | 独立 BrowserWindow 单例，close→hide |
| 原生菜单：App 子菜单（设置… `Cmd+,`、退出）+ 编辑子菜单（undo/redo/cut/copy/paste/selectAll） | settings_window.rs:36-58；lib.rs:993-998 | Electron Menu，同一 openSettings helper |
| Store（tauri-plugin-store）：`preferences.json` 于 app_data_dir，key=`app_preferences` | preferences.rs:23-24,251-256；lib.rs:990 | Electron 侧直接复用同一文件结构与 schema（service 层读写） |
| 对话框（tauri-plugin-dialog）：`pickDirectory` / `pickMediaFile` / `pickProjectExportPath` 在 UI 使用；`pickSavePath` 仅定义、UI 未用 | tauri.ts:443-469；App.tsx:329-374,694；capabilities/default.json:9 | Electron dialog + 一次性 path grant；不把 `pickSavePath` 当用户行为 |
| 自定义协议 `media://localhost/<asset_id>`：GET/HEAD/单 Range，200/206/404/416/501 | lib.rs:985-1010；media_protocol.rs:1-4；renderer 用法 TimelinePreview.tsx:121,164 | `dl-media://asset/<asset_id>`，`protocol.handle`，纯函数迁移 |
| 资源定位：runtime（ffmpeg/ffprobe）、model-runtime/asr|speaker、`DOUBLELOVE_ASR_DIR`/`DOUBLELOVE_SPEAKER_DIR` 覆盖 | lib.rs:148-210；tauri.conf.json:29 | main 侧 resourcesPath + env 覆盖 |
| app_data_dir / app_log_dir 路径 | preferences.rs:170,245；models.rs:703 | Electron `userData` 明确设为 `$HOME/Library/Application Support/space.ahua.doublelove.studio` 并把同一路径传给 host；见 ADR-0001 |

## 4. 资源与脚本 / CI

| 项 | 证据 | 说明 |
| --- | --- | --- |
| `studio/build/runtime/*`、`model-runtime/**/*` 打包资源 | `studio/electron-builder.yml`；`src-tauri/tauri.conf.json` | 仓库内仅 README 占位；发布机由 prepare 脚本填充；5A 起 Electron/Tauri 共用该资源树 |
| `model-catalog-v1.json` 事实源 | engine `model.rs` `include_str!("../resources/model-catalog-v1.json")` | 5A 已归属 `crates/double-love-engine/resources/`，不再反向依赖 `src-tauri` |
| `scripts/prepare-media-runtime.sh` / `prepare-model-runtime.sh` / `prepare-asr.sh` / `prepare-speaker.sh` | scripts/ | 媒体/模型运行时准备（发布机） |
| `scripts/verify-release-runtime.sh` | scripts/ | 硬门禁：libass、可重定位 Python |
| CI `studio-quality.yml` | self-hosted macOS ARM64；严格工具前置检查；cargo fmt/clippy/test；release host；bindings/Python/Studio 门禁；Electron build/E2E/无证书目录包/打包 smoke；Tauri debug reference build；`git diff --check` | 5A 起打包 smoke 读取 unpacked `.app` 的真实 resourcesPath |
| CI `web-quality.yml` | Web/PWA 门禁 | 不变 |

## 5. 已知边界泄漏（迁移时必须收紧）

1. renderer 直传绝对路径：`project_open`/`project_create`（tauri.ts:255-260）、`import_media`（tauri.ts:267-269）、当前 UI 使用的三个导出 apply（tauri.ts:429-439）→ Electron 改为 main 对话框一次性 path grant。另有 UI-unused 的 `export_roughcut_apply` wrapper（tauri.ts:319-324），不算现有用户流程。
2. ts-rs 把 i64/u64 生成为 TS `bigint`，JSON 运行时为 number；renderer 在边界用 `num()` 转换（utils.ts:10）与 `normalize*` 归一化（tauri.ts:170-247）→ host 协议 v1 必须固定整数序列化策略。
3. `isTauri = '__TAURI_INTERNALS__' in window`（tauri.ts:253）浏览器降级；生产 Electron 缺 bridge 时必须 fail closed。
