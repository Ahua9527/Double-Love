import { GripVertical, Plus, Scissors, Trash2 } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import './main-track-timeline.css'
import type { FrameRate } from '../../../../bindings/FrameRate'
import type { MainTrackClip } from '../../../../bindings/MainTrackClip'
import type { MediaAssetSummary } from '../../../../bindings/MediaAssetSummary'
import type { TimelineIRv2 } from '../../../../bindings/TimelineIRv2'
import { clampSeconds, formatTimecodeSeconds, frameRateFps, num, seekFractionFromClientX } from '../utils'
import { Tooltip } from './Tooltip'

interface MainTrackTimelineProps {
  clips: MainTrackClip[]
  assets: MediaAssetSummary[]
  selectedId: string | null
  timeline: TimelineIRv2 | null
  playheadSec: number
  outputRate: FrameRate | null
  onSeek: (seconds: number) => void
  onSelect: (clip: MainTrackClip) => void
  onMove: (clipId: string, beforeId: string | null) => void
  onTrim: (clip: MainTrackClip, sourceInFrame: number, sourceOutFrame: number) => void
  onSplit: (clip: MainTrackClip) => void
  onRemove: (clip: MainTrackClip) => void
  onAdd: () => void
  onDropFiles: (files: File[], beforeClipId: string | null) => void
}

type TrimEdge = 'left' | 'right'

interface TrimState {
  clip: MainTrackClip
  edge: TrimEdge
  startX: number
  sourceIn: number
  sourceOut: number
}

function clipSeconds(clip: MainTrackClip, asset: MediaAssetSummary | undefined): number {
  if (!asset) return 1
  const frames = Math.max(1, num(clip.source_out_frame) - num(clip.source_in_frame))
  return frames / frameRateFps(asset.rate)
}

function clipName(clip: MainTrackClip, assets: MediaAssetSummary[]): string {
  return assets.find((asset) => asset.id === clip.source_asset_id)?.display_name ?? '已丢失素材'
}

interface ClipTiming {
  startSec: number
  seconds: number
}

export function MainTrackTimeline({
  clips,
  assets,
  selectedId,
  timeline,
  playheadSec,
  outputRate,
  onSeek,
  onSelect,
  onMove,
  onTrim,
  onSplit,
  onRemove,
  onAdd,
  onDropFiles,
}: MainTrackTimelineProps) {
  const trackRef = useRef<HTMLDivElement>(null)
  const scrubRef = useRef<{ element: HTMLElement; pointerId: number } | null>(null)
  const draggedRef = useRef(false)
  const [dragId, setDragId] = useState<string | null>(null)
  const [trim, setTrim] = useState<TrimState | null>(null)
  const [draft, setDraft] = useState<Record<string, [number, number]>>({})
  const timings = useMemo(() => {
    const result = new Map<string, ClipTiming>()
    const rate = timeline?.rate
    let fallbackStartSec = 0
    for (const clip of clips) {
      const resolved = timeline?.clips.filter((candidate) => candidate.id.startsWith(`${clip.id}:`)) ?? []
      if (rate && resolved.length > 0) {
        const startFrame = Math.min(...resolved.map((candidate) => num(candidate.timeline_start_frame)))
        const endFrame = Math.max(...resolved.map((candidate) => num(candidate.timeline_end_frame)))
        const startSec = startFrame / frameRateFps(rate)
        const seconds = Math.max(0, (endFrame - startFrame) / frameRateFps(rate))
        result.set(clip.id, { startSec, seconds })
        fallbackStartSec = startSec + seconds
        continue
      }
      const seconds = clipSeconds(clip, assets.find((asset) => asset.id === clip.source_asset_id))
      result.set(clip.id, { startSec: fallbackStartSec, seconds })
      fallbackStartSec += seconds
    }
    return result
  }, [assets, clips, timeline])
  const total = Math.max(1, timeline ? num(timeline.output_duration_frames) / frameRateFps(timeline.rate) : [...timings.values()].reduce((sum, timing) => sum + timing.seconds, 0))
  const clampedPlayheadSec = clampSeconds(playheadSec, total)
  const playheadPercent = (clampedPlayheadSec / total) * 100
  const displayRate = timeline?.rate ?? outputRate ?? assets[0]?.rate ?? 'fps_25'

  const frameRateLabel = outputRate ? `${frameRateFps(outputRate).toFixed(3).replace(/\.000$/, '')} fps` : '跟随首段素材'

  const seekFromClientX = (clientX: number) => {
    const rect = trackRef.current?.getBoundingClientRect()
    if (!rect || rect.width <= 0) return
    const fraction = seekFractionFromClientX(clientX, rect.left, rect.width)
    onSeek(clampSeconds(fraction * total, total))
  }

  const beginScrub = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0 || event.isPrimary === false) return
    const target = event.target
    if (target instanceof Element && target.closest('[data-timeline-control], [draggable="true"]')) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    scrubRef.current = { element: event.currentTarget, pointerId: event.pointerId }
    seekFromClientX(event.clientX)
  }

  const moveScrub = (event: React.PointerEvent<HTMLElement>) => {
    if (scrubRef.current?.pointerId !== event.pointerId) return
    seekFromClientX(event.clientX)
  }

  const endScrub = (event: React.PointerEvent<HTMLElement>) => {
    const active = scrubRef.current
    if (!active || active.pointerId !== event.pointerId) return
    scrubRef.current = null
    if (active.element.hasPointerCapture(event.pointerId)) active.element.releasePointerCapture(event.pointerId)
  }

  const loseScrub = (event: React.PointerEvent<HTMLElement>) => {
    if (scrubRef.current?.pointerId === event.pointerId) scrubRef.current = null
  }

  const handleRulerKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null
    if (event.key === 'ArrowLeft') next = clampedPlayheadSec - 1
    if (event.key === 'ArrowRight') next = clampedPlayheadSec + 1
    if (event.key === 'Home') next = 0
    if (event.key === 'End') next = total
    if (next === null) return
    event.preventDefault()
    event.stopPropagation()
    onSeek(clampSeconds(next, total))
  }

  const updateDraft = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!trim || trim.clip.id !== event.currentTarget.dataset.clipId) return
    const element = event.currentTarget.parentElement
    const rect = element?.getBoundingClientRect()
    if (!rect) return
    const frameSpan = trim.sourceOut - trim.sourceIn
    const delta = Math.round(((event.clientX - trim.startX) / Math.max(rect.width, 1)) * frameSpan)
    const minimum = 2
    const next = trim.edge === 'left'
      ? [Math.min(trim.sourceOut - minimum, Math.max(0, trim.sourceIn + delta)), trim.sourceOut]
      : [trim.sourceIn, Math.max(trim.sourceIn + minimum, trim.sourceOut + delta)]
    setDraft((previous) => ({ ...previous, [trim.clip.id]: next as [number, number] }))
  }

  const commitTrim = (event: React.PointerEvent<HTMLButtonElement>) => {
    const current = trim
    if (!current || current.clip.id !== event.currentTarget.dataset.clipId) return
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
    const [sourceIn, sourceOut] = draft[current.clip.id] ?? [current.sourceIn, current.sourceOut]
    setTrim(null)
    setDraft((previous) => {
      const next = { ...previous }
      delete next[current.clip.id]
      return next
    })
    if (sourceIn !== current.sourceIn || sourceOut !== current.sourceOut) {
      onTrim(current.clip, sourceIn, sourceOut)
    }
  }

  const cancelTrim = (event: React.PointerEvent<HTMLButtonElement>) => {
    const current = trim
    if (!current || current.clip.id !== event.currentTarget.dataset.clipId) return
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
    setTrim(null)
    setDraft((previous) => {
      const next = { ...previous }
      delete next[current.clip.id]
      return next
    })
  }

  return (
    <section className="studio-timeline" aria-label="主轨时间线">
      <header className="studio-timeline-head">
        <div>
          <strong>主轨</strong>
          <span>{clips.length ? `${clips.length} 段 · ${frameRateLabel}` : '还没有素材'}</span>
        </div>
        <Tooltip label="添加素材"><button type="button" className="studio-icon-button" aria-label="添加素材" onClick={onAdd}><Plus size={15} /></button></Tooltip>
      </header>
      {clips.length === 0 ? (
        <div className="studio-timeline-empty">
          <p>先把一个本地视频放入主轨，再用转录文本决定保留哪些部分。</p>
          <button type="button" className="studio-secondary-button" onClick={onAdd}>添加素材</button>
        </div>
      ) : (
        <div className="studio-track-scroll">
          <div
            className="studio-track-ruler"
            role="slider"
            tabIndex={0}
            aria-label="时间线播放头"
            aria-orientation="horizontal"
            aria-valuemin={0}
            aria-valuemax={total}
            aria-valuenow={clampedPlayheadSec}
            aria-valuetext={`${formatTimecodeSeconds(clampedPlayheadSec, displayRate)} / ${formatTimecodeSeconds(total, displayRate)}`}
            onPointerDown={beginScrub}
            onPointerMove={moveScrub}
            onPointerUp={endScrub}
            onPointerCancel={endScrub}
            onLostPointerCapture={loseScrub}
            onKeyDown={handleRulerKeyDown}
          >
            <span>{formatTimecodeSeconds(0, displayRate)}</span><span>{formatTimecodeSeconds(total / 3, displayRate)}</span><span>{formatTimecodeSeconds((total * 2) / 3, displayRate)}</span><span>{formatTimecodeSeconds(total, displayRate)}</span>
          </div>
          <div className="studio-track-layer">
            <div
              ref={trackRef}
              className="studio-track"
              role="list"
              onPointerDown={beginScrub}
              onPointerMove={moveScrub}
              onPointerUp={endScrub}
              onPointerCancel={endScrub}
              onLostPointerCapture={loseScrub}
              onDragOver={(event) => {
                if (event.dataTransfer.types.includes('Files')) event.preventDefault()
              }}
              onDrop={(event) => {
                const files = Array.from(event.dataTransfer.files ?? [])
                if (files.length === 0) return
                event.preventDefault()
                onDropFiles(files, null)
              }}
            >
            <div className="studio-track-playhead" aria-hidden="true" style={{ left: `${playheadPercent}%` }} />
            {clips.map((clip) => {
              const [sourceIn, sourceOut] = draft[clip.id] ?? [num(clip.source_in_frame), num(clip.source_out_frame)]
              const timing = timings.get(clip.id) ?? { startSec: 0, seconds: 0 }
              const seconds = timing.seconds
              const selected = clip.id === selectedId
              return (
                <article
                  key={clip.id}
                  role="listitem"
                  draggable
                  className={`studio-track-clip ${selected ? 'is-selected' : ''} ${dragId === clip.id ? 'is-dragging' : ''}`}
                  style={{ left: `${(timing.startSec / total) * 100}%`, width: `${(seconds / total) * 100}%` }}
                  onPointerDown={(event) => {
                    draggedRef.current = false
                    event.stopPropagation()
                  }}
                  onClick={(event) => {
                    if (draggedRef.current) {
                      draggedRef.current = false
                      return
                    }
                    onSelect(clip)
                    seekFromClientX(event.clientX)
                  }}
                  onDragStart={(event) => {
                    draggedRef.current = true
                    setDragId(clip.id)
                    event.dataTransfer.effectAllowed = 'move'
                    event.dataTransfer.setData('text/plain', clip.id)
                  }}
                  onDragEnd={() => setDragId(null)}
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={(event) => {
                    event.preventDefault()
                    event.stopPropagation()
                    draggedRef.current = true
                    const files = Array.from(event.dataTransfer.files ?? [])
                    if (files.length > 0) {
                      const rect = event.currentTarget.getBoundingClientRect()
                      const index = clips.findIndex((candidate) => candidate.id === clip.id)
                      const before = event.clientX <= rect.left + rect.width / 2
                        ? clip.id
                        : (clips[index + 1]?.id ?? null)
                      onDropFiles(files, before)
                      setDragId(null)
                      return
                    }
                    const moved = event.dataTransfer.getData('text/plain')
                    if (moved && moved !== clip.id) onMove(moved, clip.id)
                    setDragId(null)
                  }}
                >
                  <button
                    type="button"
                    aria-label={`裁切 ${clipName(clip, assets)} 的左侧`}
                    data-clip-id={clip.id}
                    data-timeline-control
                    className="studio-trim-handle is-left"
                    onPointerDown={(event) => {
                      event.stopPropagation()
                      event.currentTarget.setPointerCapture(event.pointerId)
                      setTrim({ clip, edge: 'left', startX: event.clientX, sourceIn, sourceOut })
                    }}
                    onPointerMove={updateDraft}
                    onPointerUp={commitTrim}
                    onPointerCancel={cancelTrim}
                    onClick={(event) => event.stopPropagation()}
                  />
                  <div className="studio-track-clip-inner">
                    <span className="studio-track-grip"><GripVertical size={13} /></span>
                    <span className="studio-track-name">{clipName(clip, assets)}</span>
                    <span className="studio-track-duration">{formatTimecodeSeconds(seconds, displayRate)}</span>
                  </div>
                  <button
                    type="button"
                    aria-label={`裁切 ${clipName(clip, assets)} 的右侧`}
                    data-clip-id={clip.id}
                    data-timeline-control
                    className="studio-trim-handle is-right"
                    onPointerDown={(event) => {
                      event.stopPropagation()
                      event.currentTarget.setPointerCapture(event.pointerId)
                      setTrim({ clip, edge: 'right', startX: event.clientX, sourceIn, sourceOut })
                    }}
                    onPointerMove={updateDraft}
                    onPointerUp={commitTrim}
                    onPointerCancel={cancelTrim}
                    onClick={(event) => event.stopPropagation()}
                  />
                  {selected && (
                    <div className="studio-track-actions" data-timeline-control onPointerDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}>
                      <Tooltip label="在播放头拆分"><button type="button" aria-label="在播放头拆分" onClick={() => onSplit(clip)}><Scissors size={13} /></button></Tooltip>
                      <Tooltip label="移除片段"><button type="button" aria-label="移除片段" onClick={() => onRemove(clip)}><Trash2 size={13} /></button></Tooltip>
                    </div>
                  )}
                </article>
              )
            })}
            </div>
            <div
              className={`studio-track-end-drop ${dragId ? 'is-drop-active' : ''}`}
              aria-label="拖到这里放在主轨末尾"
              data-timeline-control
              onPointerDown={(event) => event.stopPropagation()}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => {
                event.preventDefault()
                event.stopPropagation()
                draggedRef.current = true
                const moved = event.dataTransfer.getData('text/plain')
                if (moved) onMove(moved, null)
                setDragId(null)
              }}
            />
          </div>
        </div>
      )}
    </section>
  )
}
