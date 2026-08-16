// 合成数据集「青空日记」：仅供界面开发，全部为人造素材名、场景、镜次与时间码，
// 不读取、不上传任何真实 XML/CSV。诊断类型直接复用 Rust 契约的 ts-rs 绑定。

import type { Diagnostic } from '../../bindings/Diagnostic'

export type ClipStatus = 'processed' | 'ignored' | 'skipped' | 'failed'
export type Rating = 'ok' | 'keep' | 'ng' | 'none'

export interface FixtureClip {
  id: string
  newName: string
  sourceName: string
  scene: string
  shot: string
  take: string
  camera: string
  rating: Rating
  tcIn: string
  duration: string
  fromCsv: boolean
  status: ClipStatus
  note: string
}

export interface RevisionEntry {
  revision: number
  operation: string
  committedAt: string
  summary: string
}

// 界面层统计（含 ignored；接 Engine 时由 OperationCounts 映射）
export interface StudioCounts {
  total: number
  processed: number
  ignored: number
  skipped: number
  failed: number
}

export interface FixtureSet {
  projectName: string
  episodeLabel: string
  csvSummary: string
  clips: FixtureClip[]
  diagnostics: Diagnostic[]
  revisions: RevisionEntry[]
  counts: StudioCounts
  csvUnmatched: number
}

const clips: FixtureClip[] = [
  { id: 'c01', newName: '02_015_01_01a', sourceName: 'A003C001_260112DJ', scene: '015', shot: '01', take: '01', camera: 'a', rating: 'ok', tcIn: '01:00:03:12', duration: '00:00:11:08', fromCsv: true, status: 'processed', note: '' },
  { id: 'c02', newName: '02_015_01_02a', sourceName: 'A003C002_260112DJ', scene: '015', shot: '01', take: '02', camera: 'a', rating: 'ng', tcIn: '01:00:19:04', duration: '00:00:08:16', fromCsv: true, status: 'processed', note: '' },
  { id: 'c03', newName: '02_015_02_01b', sourceName: 'B001C003_260112DJ', scene: '015', shot: '02', take: '01', camera: 'b', rating: 'keep', tcIn: '01:00:41:20', duration: '00:00:14:02', fromCsv: true, status: 'processed', note: '' },
  { id: 'c04', newName: '02_016_01_01a', sourceName: 'A003C004_260112DJ', scene: '016', shot: '01', take: '01', camera: 'a', rating: 'ok', tcIn: '01:01:02:10', duration: '00:00:09:21', fromCsv: true, status: 'processed', note: '' },
  { id: 'c05', newName: '02_016_01_02b', sourceName: 'B001C005_260112DJ', scene: '016', shot: '01', take: '02', camera: 'b', rating: 'none', tcIn: '01:01:18:00', duration: '00:00:12:13', fromCsv: true, status: 'processed', note: 'CSV 无评分' },
  { id: 'c06', newName: '02_016_03_01a', sourceName: 'A004C006_260112DJ', scene: '016', shot: '03', take: '01', camera: 'a', rating: 'ok', tcIn: '01:01:47:05', duration: '00:00:18:09', fromCsv: true, status: 'processed', note: '' },
  { id: 'c07', newName: '02_017_01_01a', sourceName: 'A004C007_260112DJ', scene: '017', shot: '01', take: '01', camera: 'a', rating: 'keep', tcIn: '01:02:11:14', duration: '00:00:10:00', fromCsv: true, status: 'processed', note: '' },
  { id: 'c08', newName: '02_017_01_02a', sourceName: 'A004C008_260112DJ', scene: '017', shot: '01', take: '02', camera: 'a', rating: 'ok', tcIn: '01:02:33:19', duration: '00:00:07:17', fromCsv: true, status: 'processed', note: '' },
  { id: 'c09', newName: '02_017_02_01跑', sourceName: 'A004C009_260112DJ', scene: '017', shot: '02', take: '01跑', camera: '', rating: 'ng', tcIn: '01:02:58:08', duration: '00:00:06:22', fromCsv: true, status: 'processed', note: '中文镜次' },
  { id: 'c10', newName: '02_018_01_01a', sourceName: 'A005C010_260112DJ', scene: '018', shot: '01', take: '01', camera: 'a', rating: 'ok', tcIn: '01:03:21:11', duration: '00:00:15:06', fromCsv: true, status: 'processed', note: '' },
  { id: 'c11', newName: '02_018_01_02b', sourceName: 'B002C011_260112DJ', scene: '018', shot: '01', take: '02', camera: 'b', rating: 'keep', tcIn: '01:03:44:16', duration: '00:00:12:19', fromCsv: true, status: 'processed', note: '' },
  { id: 'c12', newName: '02_018_02_01a', sourceName: 'A005C012_260112DJ', scene: '018', shot: '02', take: '01', camera: 'a', rating: 'ok', tcIn: '01:04:09:02', duration: '00:00:13:10', fromCsv: true, status: 'processed', note: '' },
  { id: 'c13', newName: '02_019_01_01a', sourceName: 'A006C013_260112DJ', scene: '019', shot: '01', take: '01', camera: 'a', rating: 'ok', tcIn: '01:04:37:21', duration: '00:00:09:14', fromCsv: true, status: 'processed', note: '' },
  { id: 'c14', newName: '02_019_02_01a', sourceName: 'A006C014_260112DJ', scene: '019', shot: '02', take: '01', camera: 'a', rating: 'none', tcIn: '01:05:01:07', duration: '00:00:11:03', fromCsv: false, status: 'processed', note: 'CSV 未匹配' },
  { id: 'c15', newName: '02_019_02_02b', sourceName: 'B003C015_260112DJ', scene: '019', shot: '02', take: '02', camera: 'b', rating: 'ok', tcIn: '01:05:26:13', duration: '00:00:16:20', fromCsv: true, status: 'processed', note: '' },
  { id: 'c16', newName: '—', sourceName: 'ATMOS_WILD_01', scene: '—', shot: '—', take: '—', camera: '—', rating: 'none', tcIn: '01:05:58:00', duration: '00:00:30:00', fromCsv: false, status: 'ignored', note: '纯音频 clip，保留节点不参与命名' },
  { id: 'c17', newName: '—', sourceName: 'ATMOS_WILD_02', scene: '—', shot: '—', take: '—', camera: '—', rating: 'none', tcIn: '01:06:31:12', duration: '00:00:24:08', fromCsv: false, status: 'ignored', note: '纯音频 clip，保留节点不参与命名' },
  { id: 'c18', newName: '—', sourceName: 'SLATE_STILL_01.jpg', scene: '—', shot: '—', take: '—', camera: '—', rating: 'none', tcIn: '01:06:59:04', duration: '00:00:05:00', fromCsv: false, status: 'ignored', note: '静帧无场记，节点保留' },
  { id: 'c19', newName: '02_020_01_01a', sourceName: 'A007C019_260112DJ', scene: '020', shot: '01', take: '01', camera: 'a', rating: 'ok', tcIn: '01:07:20:18', duration: '00:00:14:11', fromCsv: true, status: 'skipped', note: '用户标记跳过' },
  { id: 'c20', newName: '02_020_01_02a', sourceName: 'A007C020_260112DJ', scene: '020', shot: '01', take: '02', camera: 'a', rating: 'ng', tcIn: '01:07:48:09', duration: '00:00:10:05', fromCsv: true, status: 'skipped', note: '用户标记跳过' },
  { id: 'c21', newName: '—', sourceName: 'A008C021_260112DJ', scene: '021', shot: '03', take: '—', camera: 'a', rating: 'none', tcIn: '01:08:15:22', duration: '00:00:09:02', fromCsv: false, status: 'failed', note: 'shottake 缺少镜次段' },
]

const diagnostics: Diagnostic[] = [
  { level: 'warning', code: 'CSV_UNMATCHED', cause: '片段 A006C014_260112DJ 在 CSV 中无对应 Name', object_id: 'c14', impact: '该片段按无 CSV 格式命名，不含 Episode 前缀', blocks_export: false, suggested_action: '核对场记单是否缺少此镜' },
  { level: 'error', code: 'SHOTTAKE_INVALID', cause: '片段 A008C021_260112DJ 的 shottake 缺少镜次段', object_id: 'c21', impact: '无法生成规范命名，片段保持原名', blocks_export: true, suggested_action: '在场记单补齐 shot-take 后重新导入' },
  { level: 'info', code: 'AUDIO_ONLY_IGNORED', cause: '2 条纯音频 clip 不参与命名', object_id: null, impact: '节点保留在 XML，计入 ignored', blocks_export: false, suggested_action: null },
  { level: 'info', code: 'STILL_FRAME_IGNORED', cause: '1 条无场记的 .jpg 静帧不参与命名', object_id: null, impact: '节点保留在 XML，计入 ignored', blocks_export: false, suggested_action: null },
  { level: 'warning', code: 'RATING_MISSING', cause: '片段 B001C005_260112DJ 无评分记录', object_id: 'c05', impact: '命名不含评分后缀', blocks_export: false, suggested_action: '确认场记单评分列' },
  { level: 'info', code: 'RESOLUTION_NORMALIZED', cause: '文档级分辨率已统一为 3840×2160', object_id: null, impact: '所有序列使用统一帧尺寸', blocks_export: false, suggested_action: null },
]

const revisions: RevisionEntry[] = [
  { revision: 6, operation: 'operation.apply', committedAt: '2026-08-15 21:42', summary: '写回 15 条命名与标签' },
  { revision: 5, operation: 'operation.preview', committedAt: '2026-08-15 21:38', summary: '预演通过：15 命名 / 3 忽略 / 2 跳过 / 1 失败' },
  { revision: 4, operation: 'import.csv', committedAt: '2026-08-15 21:31', summary: '匹配场记单 20 行，1 行未匹配' },
  { revision: 3, operation: 'import.xml', committedAt: '2026-08-15 21:26', summary: '解析序列 21 条 clip' },
  { revision: 2, operation: 'project.open', committedAt: '2026-08-15 21:20', summary: '打开既有项目（幂等）' },
  { revision: 1, operation: 'project.create', committedAt: '2026-08-15 21:02', summary: '创建 .doublelove 项目' },
]

export const aozoraDiary: FixtureSet = {
  projectName: '青空日记 · 第 2 集',
  episodeLabel: 'E02',
  csvSummary: 'CSV 20/21',
  clips,
  diagnostics,
  revisions,
  counts: { total: 21, processed: 15, ignored: 3, skipped: 2, failed: 1 },
  csvUnmatched: 1,
}
