import { useRef } from 'react'
import {
  TIMELINE_LEFT_INSET,
  TIMELINE_RIGHT_INSET,
  formatClock,
  rulerTicks,
  seekFractionFromClientX,
} from '../utils'

interface TimelineProps {
  /** 源素材时长（秒）；0 = 空态 */
  durationSec: number
  playheadSec: number
  /** 活跃 omit 的秒区间（源素材域），渲染为红条 */
  omitRanges: Array<[number, number]>
  onSeek: (seconds: number) => void
}

export function Timeline({ durationSec, playheadSec, omitRanges, onSeek }: TimelineProps) {
  const areaRef = useRef<HTMLDivElement>(null)
  const draggingRef = useRef(false)

  const seekFromEvent = (clientX: number) => {
    const rect = areaRef.current?.getBoundingClientRect()
    if (!rect || durationSec <= 0) return
    onSeek(seekFractionFromClientX(clientX, rect.left, rect.width) * durationSec)
  }

  if (durationSec <= 0) {
    return (
      <div className="h-32 flex-none border-t border-line flex items-center justify-center">
        <span className="text-xs text-mutedfg">导入媒体后显示时间线</span>
      </div>
    )
  }

  const pct = (seconds: number) => (seconds / durationSec) * 100
  const playheadPct = Math.min(100, pct(playheadSec))

  return (
    <div className="h-32 flex-none border-t border-line flex flex-col">
      <div className="h-4 flex-none relative text-xs text-mutedfg select-none">
        {rulerTicks(durationSec).map((tick) => (
          <span
            key={tick}
            className="absolute -translate-x-1/2"
            style={{ left: `calc(${TIMELINE_LEFT_INSET}px + (100% - ${TIMELINE_LEFT_INSET + TIMELINE_RIGHT_INSET}px) * ${pct(tick) / 100})` }}
          >
            {formatClock(tick)}
          </span>
        ))}
      </div>
      <div
        ref={areaRef}
        className="flex-1 min-h-0 relative cursor-ew-resize select-none touch-none"
        onPointerDown={(e) => {
          draggingRef.current = true
          e.currentTarget.setPointerCapture(e.pointerId)
          seekFromEvent(e.clientX)
        }}
        onPointerMove={(e) => {
          if (draggingRef.current) seekFromEvent(e.clientX)
        }}
        onPointerUp={(e) => {
          draggingRef.current = false
          e.currentTarget.releasePointerCapture(e.pointerId)
        }}
        onPointerCancel={() => {
          draggingRef.current = false
        }}
      >
        {/* 内容区：扣除左右内边距，百分比坐标对齐指针换算 */}
        <div
          className="absolute inset-y-0"
          style={{ left: TIMELINE_LEFT_INSET, right: TIMELINE_RIGHT_INSET }}
        >
          {/* 源素材整段（粗剪的原材料） */}
          <div className="absolute top-[42px] h-[18px] w-full rounded-sm bg-selected/70" />
          {/* 已删除区间（omit）红条 */}
          {omitRanges.map(([start, end]) => (
            <div
              key={`${start}-${end}`}
              className="absolute top-[42px] h-[18px] bg-danger/80"
              style={{ left: `${pct(start)}%`, width: `${pct(end) - pct(start)}%` }}
              title={`已删除 ${formatClock(start)} – ${formatClock(end)}`}
            />
          ))}
          {/* 红色播放头（HTML 覆盖层保证 1.5px 清晰线宽） */}
          <div className="absolute top-0 h-[72px] w-[1.5px] bg-playhead" style={{ left: `${playheadPct}%` }} />
          <div className="absolute top-0 h-[6px] w-[9.5px] bg-playhead" style={{ left: `calc(${playheadPct}% - 4px)` }} />
        </div>
        <span className="absolute left-[10px] top-[42px] text-xs text-mutedfg">素材</span>
      </div>
    </div>
  )
}
