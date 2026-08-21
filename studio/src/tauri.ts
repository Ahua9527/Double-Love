// Tauri 命令封装：参数/返回类型直接复用 Rust ts-rs 绑定。
// 浏览器 dev（vite，无桌面壳）下所有命令不可用——调用方先用 isTauri 判断并降级。

import { invoke } from '@tauri-apps/api/core'
import { open as dialogOpen, save as dialogSave } from '@tauri-apps/plugin-dialog'
import type { EditOperation } from '../../bindings/EditOperation'
import type { ExportOutcome } from '../../bindings/ExportOutcome'
import type { CanvasSpec } from '../../bindings/CanvasSpec'
import type { FrameRate } from '../../bindings/FrameRate'
import type { MainTrackClip } from '../../bindings/MainTrackClip'
import type { MediaAssetSummary } from '../../bindings/MediaAssetSummary'
import type { OperationResult } from '../../bindings/OperationResult'
import type { ProjectExportPreview } from '../../bindings/ProjectExportPreview'
import type { ProjectSummary } from '../../bindings/ProjectSummary'
import type { RevisionHistoryEntry } from '../../bindings/RevisionHistoryEntry'
import type { SpeakerDiarizationResult } from '../../bindings/SpeakerDiarizationResult'
import type { SpeakerIdentity } from '../../bindings/SpeakerIdentity'
import type { SpeakerNameAgentPayload } from '../../bindings/SpeakerNameAgentPayload'
import type { SpeakerNameProposal } from '../../bindings/SpeakerNameProposal'
import type { SubtitleStyle } from '../../bindings/SubtitleStyle'
import type { TimelineIRv2 } from '../../bindings/TimelineIRv2'
import type { TranscriptViewData } from '../../bindings/TranscriptViewData'

// 应用级产品壳的契约。Rust 端会用同名 serde/ts-rs DTO 返回这些对象；
// 这里先把界面需要的公共形状固定下来，后端尚未上线时页面只显示“未连接”或测试 mock。
export type ThemeMode = 'light' | 'dark' | 'system'
export type TimecodePrecision = 'frame' | 'millisecond'
export type ModelInstallState = 'not_installed' | 'queued' | 'downloading' | 'paused' | 'verifying' | 'installed' | 'corrupt' | 'failed'

export interface AppPreferencesV1 {
  schema_version: number
  theme: ThemeMode
  restore_last_project: boolean
  timecode_precision: TimecodePrecision
  transcript_section_tint: boolean
  cjk_spacing: boolean
  default_subtitle_style: SubtitleStyle | null
  model_root: string
  model_endpoint: string
  default_asr_model: string
  onboarding_version: number
  onboarding_completed: boolean
  recent_projects: RecentProjectRecord[]
}

export interface PreferencesPatch {
  theme?: ThemeMode
  restore_last_project?: boolean
  timecode_precision?: TimecodePrecision
  transcript_section_tint?: boolean
  cjk_spacing?: boolean
  default_subtitle_style?: SubtitleStyle | null
  default_asr_model?: string
  model_endpoint?: string
  model_root?: string
}

export interface RecentProject {
  project_id: string | null
  root: string
  display_name: string
  last_opened_at: string
  exists: boolean
}

export interface RecentProjectRecord {
  project_id: string | null
  root: string
  display_name: string
  last_opened_at: string
}

export interface SystemProfile {
  memory_bytes: number | bigint
  architecture: string
  os_version: string
  free_model_bytes: number | bigint
  recommended_asr_model: string
}

export interface ModelDependency {
  model_id: string
  required: boolean
  reason?: string
}

export interface ModelDescriptor {
  id: string
  label: string
  kind: 'asr' | 'aligner' | 'speaker' | 'vad' | string
  revision: string
  size_bytes: number | bigint
  memory_bytes: number | bigint
  license: string
  description?: string
  dependencies: ModelDependency[]
  state: ModelInstallState
  installed_revision?: string | null
  downloaded_bytes?: number | bigint | null
  error?: string | null
  can_remove?: boolean
}

export interface ModelInstallation {
  model_id: string
  state: ModelInstallState
  revision: string
  downloaded_bytes: number | bigint
  total_bytes: number | bigint
  error?: string | null
  updated_at?: string
}

export interface ModelDownloadProgress {
  model_id: string
  state: ModelInstallState
  completed_bytes: number | bigint
  total_bytes: number | bigint
  message?: string | null
}

export interface DoctorReport {
  generated_at: string
  app_version: string
  architecture: string
  os_version: string
  ffmpeg: string
  libass: string
  model_integrity: Array<{ model_id: string; state: string; detail?: string | null }>
  free_disk_bytes: number | bigint
  logs_directory: string
  offline_runtime: string
  sanitized_text?: string
}

export interface OnboardingState {
  version: number
  completed: boolean
  step: 1 | 2 | 3
}

type RawModelDescriptor = Partial<ModelDescriptor> & {
  display_name?: string
  component?: string
  files?: Array<{ size_bytes?: number | bigint }>
  min_memory_bytes?: number | bigint | null
  bundled?: boolean
}

type RawModelInstallation = Partial<ModelInstallation> & {
  bytes_downloaded?: number | bigint
  bytes_total?: number | bigint
  staging_id?: string | null
  last_error_code?: string | null
  last_error_message?: string | null
}

type RawModelSnapshot = {
  descriptor?: RawModelDescriptor
  installation?: RawModelInstallation
}

function componentKind(component: string | undefined): ModelDescriptor['kind'] {
  if (component === 'forced_aligner' || component === 'aligner') return 'aligner'
  if (component === 'vad') return 'vad'
  if (component === 'speaker') return 'speaker'
  return 'asr'
}

function sumModelFiles(files: RawModelDescriptor['files']): number {
  return (files ?? []).reduce((total, file) => total + Number(file.size_bytes ?? 0), 0)
}

export function normalizeModelInstallation(raw: RawModelInstallation, descriptor?: RawModelDescriptor): ModelInstallation {
  const state = raw.state ?? 'not_installed'
  return {
    model_id: raw.model_id ?? descriptor?.id ?? '',
    state,
    revision: raw.revision ?? descriptor?.revision ?? '',
    downloaded_bytes: raw.downloaded_bytes ?? raw.bytes_downloaded ?? 0,
    total_bytes: raw.total_bytes ?? raw.bytes_total ?? sumModelFiles(descriptor?.files),
    error: raw.error ?? raw.last_error_message ?? null,
    updated_at: raw.updated_at,
  }
}

export function normalizeModelDescriptor(rawValue: RawModelDescriptor | RawModelSnapshot): ModelDescriptor {
  const raw = 'descriptor' in rawValue && rawValue.descriptor ? rawValue.descriptor : rawValue as RawModelDescriptor
  const installation = 'installation' in rawValue && rawValue.installation ? rawValue.installation : undefined
  const id = raw.id ?? ''
  const descriptor: ModelDescriptor = {
    id,
    label: raw.label ?? raw.display_name ?? id,
    kind: raw.kind ?? componentKind(raw.component),
    revision: raw.revision ?? '',
    size_bytes: raw.size_bytes ?? sumModelFiles(raw.files),
    memory_bytes: raw.memory_bytes ?? raw.min_memory_bytes ?? 0,
    license: raw.license ?? '未知许可',
    description: raw.description,
    dependencies: raw.dependencies ?? [],
    state: installation?.state ?? raw.state ?? 'not_installed',
    installed_revision: installation?.state === 'installed' ? installation.revision ?? raw.installed_revision : raw.installed_revision,
    downloaded_bytes: installation?.downloaded_bytes ?? installation?.bytes_downloaded ?? raw.downloaded_bytes ?? 0,
    error: installation?.error ?? installation?.last_error_message ?? raw.error ?? null,
    can_remove: raw.can_remove ?? (!raw.bundled && raw.component !== 'vad'),
  }
  return descriptor
}

export function normalizeModelProgress(raw: Partial<ModelDownloadProgress> & { bytes_downloaded?: number | bigint; bytes_total?: number | bigint }): ModelDownloadProgress {
  return {
    model_id: raw.model_id ?? '',
    state: raw.state ?? 'not_installed',
    completed_bytes: raw.completed_bytes ?? raw.bytes_downloaded ?? 0,
    total_bytes: raw.total_bytes ?? raw.bytes_total ?? 0,
    message: raw.message ?? null,
  }
}

export function normalizeDoctorReport(raw: Partial<DoctorReport> & {
  model_checks?: Array<{ model_id: string; state: string; error_code?: string | null }>
  ffmpeg_available?: boolean
  libass_available?: boolean
  model_root_available?: boolean
  free_model_bytes?: number | bigint
  asr_runtime_ready?: boolean
  speaker_runtime_ready?: boolean
  warnings?: string[]
}): DoctorReport {
  return {
    generated_at: raw.generated_at ?? new Date().toISOString(),
    app_version: raw.app_version ?? '0.1.0',
    architecture: raw.architecture ?? 'unknown',
    os_version: raw.os_version ?? 'unknown',
    ffmpeg: raw.ffmpeg ?? (raw.ffmpeg_available ? '可用' : '不可用'),
    libass: raw.libass ?? (raw.libass_available ? '可用' : '不可用'),
    model_integrity: raw.model_integrity ?? (raw.model_checks ?? []).map((model) => ({ model_id: model.model_id, state: model.state, detail: model.error_code ?? null })),
    free_disk_bytes: raw.free_disk_bytes ?? raw.free_model_bytes ?? 0,
    logs_directory: raw.logs_directory ?? '应用日志目录',
    offline_runtime: raw.offline_runtime ?? (
      raw.asr_runtime_ready && raw.speaker_runtime_ready
        ? 'ASR 与说话人运行时可用'
        : `ASR ${raw.asr_runtime_ready ? '可用' : '不可用'} · 说话人 ${raw.speaker_runtime_ready ? '可用' : '不可用'}`
    ),
    sanitized_text: raw.sanitized_text,
  }
}

function mapOperation<T, U>(result: OperationResult<T>, map: (value: T) => U): OperationResult<U> {
  return { ...result, data: result.data === null ? null : map(result.data) }
}

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

export function projectRevision() {
  return invoke<OperationResult<bigint>>('project_revision')
}

export function projectHistory(limit = 80) {
  return invoke<OperationResult<RevisionHistoryEntry[]>>('project_history', { limit })
}

export function projectRestoreRevision(revision: number) {
  return invoke<OperationResult<{ restored_revision: bigint; revision: bigint }>>('project_restore_revision', { revision })
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

export function timelineGet() {
  return invoke<OperationResult<TimelineIRv2>>('timeline_get')
}

export function mainTrackList() {
  return invoke<OperationResult<MainTrackClip[]>>('main_track_list')
}

export function mainTrackAppendFull(assetId: string) {
  return invoke<OperationResult<MainTrackClip>>('main_track_append_full', { assetId })
}

export function mainTrackMove(clipId: string, beforeClipId: string | null) {
  return invoke<OperationResult<null>>('main_track_move', { clipId, beforeClipId })
}

export function mainTrackTrim(clipId: string, sourceInFrame: number, sourceOutFrame: number) {
  return invoke<OperationResult<MainTrackClip>>('main_track_trim', {
    clipId,
    sourceInFrame,
    sourceOutFrame,
  })
}

export function mainTrackSplit(clipId: string, sourceAtFrame: number) {
  return invoke<OperationResult<MainTrackClip[]>>('main_track_split', { clipId, sourceAtFrame })
}

export function mainTrackRemove(clipId: string) {
  return invoke<OperationResult<null>>('main_track_remove', { clipId })
}

export function canvasGet() {
  return invoke<OperationResult<CanvasSpec>>('canvas_get')
}

export function canvasSet(canvas: CanvasSpec) {
  return invoke<OperationResult<CanvasSpec>>('canvas_set', { canvas })
}

export function outputRateGet() {
  return invoke<OperationResult<FrameRate | null>>('output_rate_get')
}

export function outputRateSet(rate: FrameRate | null) {
  return invoke<OperationResult<FrameRate | null>>('output_rate_set', { rate })
}

export function subtitleStyleGet() {
  return invoke<OperationResult<SubtitleStyle>>('subtitle_style_get')
}

export function subtitleStyleSet(style: SubtitleStyle) {
  return invoke<OperationResult<SubtitleStyle>>('subtitle_style_set', { style })
}

export function applyDefaultSubtitleStyle() {
  return invoke<OperationResult<SubtitleStyle>>('apply_default_subtitle_style')
}

export function speakerList() {
  return invoke<OperationResult<SpeakerIdentity[]>>('speaker_list')
}

export function speakerDiarizeStart(assetId: string) {
  return invoke<OperationResult<{ task_id: string }>>('speaker_diarize_start', { assetId })
}

export function speakerDiarizationGet(assetId: string) {
  return invoke<OperationResult<SpeakerDiarizationResult>>('speaker_diarization_get', { assetId })
}

export function speakerNameProposals(assetId: string) {
  return invoke<OperationResult<SpeakerNameProposal[]>>('speaker_name_proposals', { assetId })
}

export function speakerAgentPayloadPreview(assetId: string, speakerId: string) {
  return invoke<OperationResult<SpeakerNameAgentPayload>>('speaker_agent_payload_preview', {
    assetId,
    speakerId,
  })
}

export function speakerNameConfirm(speakerId: string, displayName: string) {
  return invoke<OperationResult<SpeakerIdentity>>('speaker_name_confirm', {
    speakerId,
    displayName,
    confirmed: true,
  })
}

export function speakerMergeConfirm(keepSpeakerId: string, mergeSpeakerId: string) {
  return invoke<OperationResult<SpeakerIdentity>>('speaker_merge_confirm', {
    keepSpeakerId,
    mergeSpeakerId,
    confirmed: true,
  })
}

export function projectExportPreview() {
  return invoke<OperationResult<ProjectExportPreview>>('project_export_preview')
}

export function projectExportXmemlApply(targetPath: string) {
  return invoke<OperationResult<ProjectExportPreview>>('project_export_xmeml_apply', { targetPath })
}

export function projectExportAssApply(targetPath: string) {
  return invoke<OperationResult<ProjectExportPreview>>('project_export_ass_apply', { targetPath })
}

export function projectRenderMp4Apply(targetPath: string) {
  return invoke<OperationResult<ProjectExportPreview>>('project_render_mp4_apply', { targetPath })
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

export function pickProjectExportPath(defaultName: string, kind: 'xml' | 'ass' | 'mp4') {
  const filter = {
    xml: { name: 'Premiere / Resolve XML', extensions: ['xml'] },
    ass: { name: 'ASS 字幕', extensions: ['ass'] },
    mp4: { name: '带字幕 MP4', extensions: ['mp4'] },
  }[kind]
  return dialogSave({ title: `导出 ${filter.name}`, defaultPath: defaultName, filters: [filter] })
}

// ---- 应用级设置 / 模型 / 诊断契约 ----

export function settingsOpen() {
  return invoke<OperationResult<null>>('settings_open')
}

export function preferencesGet() {
  return invoke<OperationResult<AppPreferencesV1>>('preferences_get')
}

export function preferencesUpdate(patch: PreferencesPatch) {
  return invoke<OperationResult<AppPreferencesV1>>('preferences_update', { patch })
}

export function recentProjectsList() {
  return invoke<OperationResult<RecentProject[]>>('recent_projects_list')
}

export function recentProjectForget(root: string) {
  return invoke<OperationResult<null>>('recent_project_forget', { root })
}

export function systemProfile() {
  return invoke<OperationResult<SystemProfile>>('system_profile')
}

export function modelCatalog() {
  return invoke<OperationResult<unknown>>('model_catalog').then((result) => mapOperation(result, (value) => Array.isArray(value) ? value.map((item) => normalizeModelDescriptor(item as RawModelDescriptor | RawModelSnapshot)) : []))
}

export function modelInstall(modelId: string) {
  return invoke<OperationResult<unknown>>('model_install', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelPause(modelId: string) {
  return invoke<OperationResult<unknown>>('model_pause', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelResume(modelId: string) {
  return invoke<OperationResult<unknown>>('model_resume', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelCancel(modelId: string) {
  return invoke<OperationResult<unknown>>('model_cancel', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelVerify(modelId: string) {
  return invoke<OperationResult<unknown>>('model_verify', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelRemove(modelId: string) {
  return invoke<OperationResult<unknown>>('model_remove', { modelId }).then((result) => mapOperation(result, (value) => normalizeModelInstallation(value as RawModelInstallation)))
}

export function modelReveal(modelId?: string) {
  return invoke<OperationResult<null>>('model_reveal', modelId ? { modelId } : undefined)
}

export function doctorRun() {
  return invoke<OperationResult<unknown>>('doctor_run').then((result) => mapOperation(result, (value) => normalizeDoctorReport(value as Partial<DoctorReport> & { model_checks?: Array<{ model_id: string; state: string; error_code?: string | null }>; ffmpeg_available?: boolean; libass_available?: boolean; model_root_available?: boolean; warnings?: string[] })) )
}

export function diagnosticsRevealLogs() {
  return invoke<OperationResult<null>>('diagnostics_reveal_logs')
}

export function onboardingGet() {
  return invoke<OperationResult<OnboardingState>>('onboarding_get')
}

export function onboardingComplete(defaultAsrModel?: string) {
  return invoke<OperationResult<OnboardingState>>('onboarding_complete', defaultAsrModel ? { defaultAsrModel } : undefined)
}

export function onboardingReset() {
  return invoke<OperationResult<OnboardingState>>('onboarding_reset')
}

// 固定桌面快捷键的命令边界。撤销与重做由 Rust 侧项目版本历史执行。
export function editUndo() {
  return invoke<OperationResult<null>>('edit_undo')
}

export function editRedo() {
  return invoke<OperationResult<null>>('edit_redo')
}
