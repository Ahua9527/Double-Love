// Tauri 命令封装：参数/返回类型直接复用 Rust ts-rs 绑定。
// 浏览器 dev（vite，无桌面壳）下所有命令不可用——调用方先用 isTauri 判断并降级。

import { invoke } from '@tauri-apps/api/core'
import { open as dialogOpen, save as dialogSave } from '@tauri-apps/plugin-dialog'
import type { EditOperation } from '../../bindings/EditOperation'
import type { ExportOutcome } from '../../bindings/ExportOutcome'
import type { MediaAssetSummary } from '../../bindings/MediaAssetSummary'
import type { OperationResult } from '../../bindings/OperationResult'
import type { ProjectSummary } from '../../bindings/ProjectSummary'
import type { TranscriptViewData } from '../../bindings/TranscriptViewData'

export const isTauri = '__TAURI_INTERNALS__' in window

export function projectOpen(path: string) {
  return invoke<OperationResult<ProjectSummary>>('project_open', { path })
}

export function projectCreate(path: string) {
  return invoke<OperationResult<ProjectSummary>>('project_create', { path })
}

export function assetsList() {
  return invoke<OperationResult<MediaAssetSummary[]>>('assets_list')
}

export function importMedia(path: string) {
  return invoke<OperationResult<MediaAssetSummary>>('import_media', { path })
}

export function transcriptGet(assetId: string) {
  return invoke<OperationResult<TranscriptViewData>>('transcript_get', { assetId })
}

export function transcribeStart(assetId: string, model: string, language: string) {
  return invoke<OperationResult<{ task_id: string }>>('transcribe_start', {
    assetId,
    model,
    language,
  })
}

export function taskCancel(taskId: string) {
  return invoke<OperationResult<{ task_id: string }>>('task_cancel', { taskId })
}

export function editOmit(assetId: string, startOrdinal: number, endOrdinal: number) {
  return invoke<OperationResult<EditOperation>>('edit_omit', {
    assetId,
    startOrdinal,
    endOrdinal,
  })
}

export function editRestore(operationId: string, startOrdinal: number, endOrdinal: number) {
  return invoke<OperationResult<EditOperation>>('edit_restore', {
    operationId,
    startOrdinal,
    endOrdinal,
  })
}

export function roughcutPreview(assetId: string) {
  return invoke<OperationResult<ExportOutcome>>('roughcut_preview', { assetId })
}

export function exportRoughcutApply(assetId: string, targetPath: string) {
  return invoke<OperationResult<ExportOutcome>>('export_roughcut_apply', {
    assetId,
    targetPath,
  })
}

// ---- 系统对话框（打开目录 / 选择媒体 / 保存路径） ----

export function pickDirectory(title: string) {
  return dialogOpen({ title, directory: true })
}

export function pickMediaFile() {
  return dialogOpen({
    title: '选择要导入的媒体文件',
    filters: [{ name: '视频', extensions: ['mp4', 'mov', 'm4v', 'webm'] }],
  })
}

export function pickSavePath(defaultName: string) {
  return dialogSave({
    title: '导出粗剪时间线',
    defaultPath: defaultName,
    filters: [{ name: 'Premiere XML', extensions: ['xml'] }],
  })
}
