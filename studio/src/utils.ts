// 界面纯函数：秒制播放头、刻度尺、omit 区间换算与诊断汇总。全部无副作用，可单测。
// 时间约定：UI 显示层允许 f64 秒（PRD 边界）；引擎侧采样/帧整数不经过这里。

import type { Diagnostic } from '../../bindings/Diagnostic'
import type { EditOperation } from '../../bindings/EditOperation'
import type { FrameRate } from '../../bindings/FrameRate'
import type { WordAnchor } from '../../bindings/WordAnchor'

// 底部时间线左右内边距（px，与轨道标签占位对齐）
export const TIMELINE_LEFT_INSET = 28
export const TIMELINE_RIGHT_INSET = 12

/** ts-rs 把 i64 标成 bigint；JSON 运行时是 number。统一在边界转 number。 */
export function num(value: bigint | number): number {
  return Number(value)
}

export function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
}

/** 秒数 clamp 到 [0, duration]。 */
export function clampSeconds(seconds: number, durationSec: number): number {
  return Math.min(Math.max(0, seconds), Math.max(0, durationSec))
}

/** 由指针横坐标换算播放头位置（0..1），扣除左右内边距并 clamp。 */
export function seekFractionFromClientX(
  clientX: number,
  rectLeft: number,
  rectWidth: number,
): number {
  const usable = Math.max(0, rectWidth - TIMELINE_LEFT_INSET - TIMELINE_RIGHT_INSET)
  if (usable === 0) return 0
  return clamp01((clientX - rectLeft - TIMELINE_LEFT_INSET) / usable)
}

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

/** 秒 → "mm:ss"；超过 1 小时 → "h:mm:ss"。 */
export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const secs = total % 60
  if (hours > 0) return `${hours}:${pad2(minutes)}:${pad2(secs)}`
  return `${pad2(minutes)}:${pad2(secs)}`
}

/** 传输条上的播放头时钟，如 "00:42 / 02:00"。 */
export function playheadClock(currentSec: number, durationSec: number): string {
  return `${formatClock(clampSeconds(currentSec, durationSec))} / ${formatClock(durationSec)}`
}

// ---- 时间线刻度尺 ----

const RULER_STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600]

/**
 * 按素材时长选「好看」的刻度位置（秒）：最小的步长让刻度不超过 maxTicks+1 个。
 * 时长为 0 时返回 [0]（空态占位）。
 */
export function rulerTicks(durationSec: number, maxTicks = 8): number[] {
  if (durationSec <= 0) return [0]
  const step = RULER_STEPS.find((candidate) => durationSec / candidate <= maxTicks) ??
    RULER_STEPS[RULER_STEPS.length - 1] * Math.ceil(durationSec / maxTicks / RULER_STEPS[RULER_STEPS.length - 1])
  const ticks: number[] = []
  for (let tick = 0; tick <= durationSec; tick += step) {
    ticks.push(tick)
  }
  return ticks
}

// ---- omit 区间：词序闭区间 → 秒区间（时间线红条） ----

/**
 * 活跃 omit 的词序区间映射为源素材秒区间 [start, end)。
 * 词表按 ordinal 连续（引擎落库保证）；端点词缺失（如重转录后的腐烂编辑）跳过该区间。
 */
export function omitRangesToSeconds(
  words: WordAnchor[],
  omits: EditOperation[],
  sampleRate: number,
): Array<[number, number]> {
  if (sampleRate <= 0) return []
  const byOrdinal = new Map(words.map((word) => [num(word.ordinal), word]))
  const ranges: Array<[number, number]> = []
  for (const omit of omits) {
    const first = byOrdinal.get(num(omit.start_ordinal))
    const last = byOrdinal.get(num(omit.end_ordinal))
    if (!first || !last) continue
    ranges.push([num(first.start_sample) / sampleRate, num(last.end_sample) / sampleRate])
  }
  return ranges.sort((a, b) => a[0] - b[0])
}

// ---- 展示标签 ----

const FRAME_RATE_LABELS: Record<FrameRate, string> = {
  fps_24: '24 fps',
  fps_24_ntsc: '23.976 fps (NTSC)',
  fps_25: '25 fps',
  fps_30: '30 fps',
  fps_30_ntsc: '29.97 fps (NTSC)',
  fps_50: '50 fps',
  fps_60: '60 fps',
  fps_60_ntsc: '59.94 fps (NTSC)',
}

export function frameRateLabel(rate: FrameRate): string {
  return FRAME_RATE_LABELS[rate]
}

const ASSET_STATUS_LABELS = {
  imported: '已导入',
  prepared: '已准备',
  transcribed: '已转录',
} as const

export function assetStatusLabel(status: keyof typeof ASSET_STATUS_LABELS): string {
  return ASSET_STATUS_LABELS[status]
}

/** 若存在阻断导出的诊断，返回标题栏/检查器共用的阻断提示；否则为 null。 */
export function exportBlockMessage(diagnostics: Diagnostic[]): string | null {
  const blocking = diagnostics.filter((d) => d.blocks_export)
  if (blocking.length === 0) return null
  const first = blocking[0]
  const suffix = first.object_id ? `（${first.object_id}）` : ''
  return `⛔ 导出被 ${blocking.length} 条错误诊断阻断：${first.code}${suffix}`
}

// ---- 面板收起状态（左侧栏 / 检查器 / 时间线） ----

export interface PanelState {
  left: boolean
  right: boolean
  bottom: boolean
}

export const PANEL_STORAGE_KEY = 'studio.panels'

const DEFAULT_PANELS: PanelState = { left: true, right: true, bottom: true }

/** 读取持久化的面板状态；缺失、损坏或字段类型不对时回退默认（全部展开）。 */
export function loadPanelState(storage: Pick<Storage, 'getItem'>): PanelState {
  try {
    const raw = storage.getItem(PANEL_STORAGE_KEY)
    if (!raw) return DEFAULT_PANELS
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return DEFAULT_PANELS
    const candidate = parsed as Record<string, unknown>
    if (
      typeof candidate.left !== 'boolean' ||
      typeof candidate.right !== 'boolean' ||
      typeof candidate.bottom !== 'boolean'
    ) {
      return DEFAULT_PANELS
    }
    return { left: candidate.left, right: candidate.right, bottom: candidate.bottom }
  } catch {
    return DEFAULT_PANELS
  }
}

export function savePanelState(
  storage: Pick<Storage, 'setItem'>,
  state: PanelState,
): void {
  storage.setItem(PANEL_STORAGE_KEY, JSON.stringify(state))
}
