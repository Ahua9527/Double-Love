// 界面纯函数：播放头、时间刻度、评分标签与诊断汇总。全部无副作用，可单测。

import type { Diagnostic } from '../../bindings/Diagnostic'
import type { ClipStatus, Rating, StudioCounts } from './fixtures'

// 底部时间线总长（秒）与左右内边距（px，与轨道标签占位对齐）
export const TIMELINE_TOTAL_SEC = 120
export const TIMELINE_LEFT_INSET = 28
export const TIMELINE_RIGHT_INSET = 12

export function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
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

export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  return `${pad2(Math.floor(total / 60))}:${pad2(total % 60)}`
}

/** 传输条上的播放头时钟，如 "00:42 / 02:00"。 */
export function playheadClock(playhead: number): string {
  return `${formatClock(clamp01(playhead) * TIMELINE_TOTAL_SEC)} / ${formatClock(TIMELINE_TOTAL_SEC)}`
}

export function ratingLabel(rating: Rating): string {
  switch (rating) {
    case 'ok': return 'ok'
    case 'keep': return 'kp'
    case 'ng': return 'ng'
    case 'none': return '—'
  }
}

export function statusLabel(status: ClipStatus): string {
  switch (status) {
    case 'processed': return '已处理'
    case 'ignored': return '已忽略'
    case 'skipped': return '已跳过'
    case 'failed': return '失败'
  }
}

/** 统计一致性：total = processed + ignored + skipped + failed。 */
export function countsConsistent(counts: StudioCounts): boolean {
  return (
    counts.total ===
    counts.processed + counts.ignored + counts.skipped + counts.failed
  )
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
