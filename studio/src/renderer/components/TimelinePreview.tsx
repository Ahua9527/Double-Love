import { useEffect, useMemo, useRef, useState } from 'react'
import './timeline-preview.css'
import type { CanvasSpec } from '../../../../bindings/CanvasSpec'
import type { SubtitleCue } from '../../../../bindings/SubtitleCue'
import type { SubtitleStyle } from '../../../../bindings/SubtitleStyle'
import type { TimelineIRv2 } from '../../../../bindings/TimelineIRv2'
import { frameRateFps, num } from '../utils'
import * as api from '../platform/desktop'
import type { NativePlayerState } from '../platform/electron'

interface TimelinePreviewProps {
  timeline: TimelineIRv2 | null
  canvas: CanvasSpec | null
  style: SubtitleStyle | null
  cues: SubtitleCue[]
  outputPlayheadSec: number
  playing: boolean
  onOutputTimeUpdate: (seconds: number) => void
  onPlayState: (playing: boolean) => void
  onSourceChange: (assetId: string | null) => void
}

function frameAt(timeline: TimelineIRv2, seconds: number) {
  return Math.max(0, Math.floor(seconds * frameRateFps(timeline.rate)))
}

function clipAt(timeline: TimelineIRv2, frame: number) {
  return timeline.clips.find((clip) =>
    frame >= num(clip.timeline_start_frame) && frame < num(clip.timeline_end_frame),
  ) ?? timeline.clips[timeline.clips.length - 1] ?? null
}

export function TimelinePreview(props: TimelinePreviewProps) {
  const {
    canvas: requestedCanvas,
    cues,
    onOutputTimeUpdate,
    onPlayState,
    onSourceChange,
    outputPlayheadSec,
    playing,
    style,
    timeline,
  } = props
  const previewRef = useRef<HTMLDivElement>(null)
  const outputPlayheadRef = useRef(outputPlayheadSec)
  outputPlayheadRef.current = outputPlayheadSec
  const [playerState, setPlayerState] = useState<NativePlayerState | null>(null)
  const frame = timeline ? frameAt(timeline, outputPlayheadSec) : 0
  const clip = useMemo(
    () => timeline ? clipAt(timeline, frame) : null,
    [frame, timeline],
  )
  const source = timeline?.sources.find((candidate) => candidate.asset_id === clip?.source_asset_id) ?? null
  const cue = cues.find((candidate) =>
    frame >= num(candidate.start_frame) && frame < num(candidate.end_frame),
  ) ?? null
  const canvas = requestedCanvas ?? timeline?.canvas ?? null
  const cueText = cue
    ? (style?.show_speaker && cue.speaker_name ? cue.speaker_name + '：' : '') + cue.text
    : ''

  useEffect(() => {
    onSourceChange(source?.asset_id ?? null)
  }, [onSourceChange, source?.asset_id])

  useEffect(() => {
    if (!api.isDesktop) return
    const element = previewRef.current
    if (!element) return
    const update = () => {
      const rect = element.getBoundingClientRect()
      void api.playerSetBounds({ x: rect.x, y: rect.y, width: rect.width, height: rect.height })
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(element)
    window.addEventListener('resize', update)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', update)
      void api.playerSetBounds({ x: 0, y: 0, width: 0, height: 0 })
    }
  }, [source?.asset_id])

  useEffect(() => {
    if (!api.isDesktop) return
    return api.onPlayerState((state) => {
      setPlayerState(state)
      if (state.state === 'playing') onPlayState(true)
      else if (state.state === 'paused' || state.state === 'ended' || state.state === 'error') onPlayState(false)
      if (state.state === 'playing') onOutputTimeUpdate(state.seconds)
    })
  }, [onOutputTimeUpdate, onPlayState])

  useEffect(() => {
    if (!api.isDesktop || !timeline) return
    const sources = new Map(timeline.sources.map((item) => [item.asset_id, item]))
    const outputFps = frameRateFps(timeline.rate)
    const clips = timeline.clips.flatMap((item) => {
      const itemSource = sources.get(item.source_asset_id)
      if (!itemSource) return []
      const sourceFps = frameRateFps(itemSource.rate)
      return [{
        assetId: item.source_asset_id,
        sourceStartSeconds: num(item.source_in_frame) / sourceFps,
        sourceDurationSeconds: (num(item.source_out_frame) - num(item.source_in_frame)) / sourceFps,
        outputStartSeconds: num(item.timeline_start_frame) / outputFps,
        outputDurationSeconds: (num(item.timeline_end_frame) - num(item.timeline_start_frame)) / outputFps,
      }]
    })
    if (clips.length > 0) void api.playerLoadTimeline(clips, outputPlayheadRef.current)
  }, [timeline])

  useEffect(() => {
    if (!api.isDesktop) return
    if (playing) void api.playerPlay().catch(() => onPlayState(false))
    else void api.playerPause()
  }, [onPlayState, playing])

  useEffect(() => {
    if (!api.isDesktop || playing || !timeline) return
    void api.playerSeek(outputPlayheadSec)
  }, [outputPlayheadSec, playing, timeline])

  useEffect(() => {
    if (!api.isDesktop) return
    void api.playerSetSubtitle({
      text: cueText,
      canvasWidth: canvas ? Math.max(1, num(canvas.width)) : 1920,
      fontFamily: style?.font_family ?? 'PingFang SC',
      fontSize: style?.font_size ?? 52,
      textColor: style?.text_color ?? '#FFFFFF',
      outlineColor: style?.outline_color ?? '#000000',
      outlineWidth: style?.outline_width ?? 2,
      shadowColor: style?.shadow_color ?? '#00000080',
      shadowX: style?.shadow_offset_x ?? 0,
      shadowY: style?.shadow_offset_y ?? 2,
      shadowBlur: style?.shadow_blur ?? 4,
      backgroundColor: style?.background_color ?? '#00000000',
      radius: style?.background_radius ?? 0,
      paddingX: style?.background_padding_x ?? 0,
      paddingY: style?.background_padding_y ?? 0,
      x: style?.position_x ?? 0.5,
      y: style?.position_y ?? 0.84,
      maxWidth: style?.max_width_ratio ?? 0.82,
    })
  }, [canvas, cueText, style])

  useEffect(() => {
    if (!api.isDesktop || !canvas) return
    void api.playerSetPresentation({
      fit: canvas.fit,
      canvasWidth: Math.max(1, num(canvas.width)),
      canvasHeight: Math.max(1, num(canvas.height)),
      positionX: canvas.position_x,
      positionY: canvas.position_y,
      scale: canvas.scale,
      rotation: canvas.rotation_degrees,
      opacity: canvas.opacity,
      background: canvas.background,
    })
  }, [canvas])

  if (!timeline || !clip || !source || !canvas) {
    return <div className="studio-preview-frame studio-preview-empty"><span>主轨还没有可预览的片段</span><small>添加素材后，视频与字幕会按输出时间线显示。</small></div>
  }

  const width = Math.max(1, num(canvas.width))
  const height = Math.max(1, num(canvas.height))

  return (
    <div className="studio-preview-frame studio-timeline-preview" style={{ background: canvas.background, aspectRatio: `${width} / ${height}` }}>
      <div ref={previewRef} className="studio-timeline-preview-media" aria-label="AVFoundation 视频预览" />
      {playerState?.state === 'loading' && <div className="studio-preview-status">正在载入视频…</div>}
      {playerState?.state === 'error' && <div className="studio-preview-status is-error">{playerState.error || '视频无法播放'}</div>}
      <div className="studio-preview-label">{source.display_name}</div>
    </div>
  )
}
