import { useRef } from 'react'
import {
  TIMELINE_LEFT_INSET,
  TIMELINE_RIGHT_INSET,
  TIMELINE_TOTAL_SEC,
  seekFractionFromClientX,
} from '../utils'

const RULER = ['00:00', '00:20', '00:40', '01:00', '01:20', '01:40', '02:00']
const BAR_COUNT = 160

// 轨道片段（开始秒, 时长秒），与 GPUI 骨架一致的对位示意
const A_TRACK: Array<[number, number]> = [[0, 14], [16, 9], [31, 12], [52, 18], [75, 11], [95, 15]]
const B_TRACK: Array<[number, number]> = [[4, 10], [28, 8], [58, 14], [86, 9]]
const AUDIO_TRACK: Array<[number, number]> = [[0, 30], [34, 26], [64, 30], [98, 18]]

/** 伪随机但确定性的波形高度（0..1）。 */
function pseudoRandom(index: number): number {
  const value = Math.sin(index * 12.9898) * 43758.5453
  return value - Math.floor(value)
}

function secToPct(seconds: number): number {
  return (seconds / TIMELINE_TOTAL_SEC) * 100
}

interface TimelineProps {
  playhead: number
  onSeek: (fraction: number) => void
}

export function Timeline({ playhead, onSeek }: TimelineProps) {
  const areaRef = useRef<HTMLDivElement>(null)
  const draggingRef = useRef(false)

  const seekFromEvent = (clientX: number) => {
    const rect = areaRef.current?.getBoundingClientRect()
    if (!rect) return
    onSeek(seekFractionFromClientX(clientX, rect.left, rect.width))
  }

  return (
    <div className="h-32 flex-none border-t border-line flex flex-col">
      <div className="h-4 flex-none px-2 flex items-center justify-between text-xs text-mutedfg select-none">
        {RULER.map((label) => (
          <span key={label}>{label}</span>
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
        {/* 内容区：扣除左右内边距，SVG 百分比坐标对齐指针换算 */}
        <div
          className="absolute inset-y-0"
          style={{ left: TIMELINE_LEFT_INSET, right: TIMELINE_RIGHT_INSET }}
        >
          <svg className="h-full w-full">
            {Array.from({ length: BAR_COUNT }, (_, i) => {
              const barHeight = 4 + 24 * pseudoRandom(i)
              return (
                <rect
                  key={i}
                  x={`${(i / BAR_COUNT) * 100}%`}
                  y={4 + (32 - barHeight) / 2}
                  width={`${(0.55 / BAR_COUNT) * 100}%`}
                  height={barHeight}
                  className="fill-mutedfg"
                  opacity={0.45}
                />
              )
            })}
            <rect x={0} y={19.5} width="100%" height={0.5} className="fill-mutedfg" opacity={0.25} />
            {A_TRACK.map(([start, dur], i) => (
              <rect key={`a${i}`} x={`${secToPct(start)}%`} y={42} width={`${secToPct(dur)}%`} height={18} rx={3} fill="#3366FF" opacity={0.7} />
            ))}
            {B_TRACK.map(([start, dur], i) => (
              <rect key={`b${i}`} x={`${secToPct(start)}%`} y={66} width={`${secToPct(dur)}%`} height={18} rx={3} fill="#12A594" opacity={0.7} />
            ))}
            {AUDIO_TRACK.map(([start, dur], i) => (
              <rect key={`au${i}`} x={`${secToPct(start)}%`} y={90} width={`${secToPct(dur)}%`} height={12} rx={3} fill="#30A46C" opacity={0.75} />
            ))}
          </svg>
          {/* 红色播放头（HTML 覆盖层保证 1.5px 清晰线宽） */}
          <div className="absolute top-0 h-[108px] w-[1.5px] bg-playhead" style={{ left: `${playhead * 100}%` }} />
          <div className="absolute top-0 h-[6px] w-[9.5px] bg-playhead" style={{ left: `calc(${playhead * 100}% - 4px)` }} />
        </div>
        <span className="absolute left-[10px] top-[42px] text-xs text-mutedfg">A</span>
        <span className="absolute left-[10px] top-[66px] text-xs text-mutedfg">B</span>
        <span className="absolute left-[10px] top-[90px] text-xs text-mutedfg">音频</span>
      </div>
    </div>
  )
}
