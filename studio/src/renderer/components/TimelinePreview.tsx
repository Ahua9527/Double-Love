import { useEffect, useMemo, useRef } from 'react'
import './timeline-preview.css'
import type { CanvasSpec } from '../../../../bindings/CanvasSpec'
import type { SubtitleCue } from '../../../../bindings/SubtitleCue'
import type { SubtitleStyle } from '../../../../bindings/SubtitleStyle'
import type { TimelineIRv2 } from '../../../../bindings/TimelineIRv2'
import { frameRateFps, num } from '../utils'
import { mediaAssetUrl } from '../media-url'

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

function sourceSeconds(timeline: TimelineIRv2, frame: number, clip: TimelineIRv2['clips'][number]) {
  const source = timeline.sources.find((candidate) => candidate.asset_id === clip.source_asset_id)
  if (!source) return 0
  const outputStart = num(clip.timeline_start_frame)
  const outputLength = Math.max(1, num(clip.timeline_end_frame) - outputStart)
  const sourceStart = num(clip.source_in_frame)
  const sourceLength = num(clip.source_out_frame) - sourceStart
  const sourceFrame = sourceStart + ((frame - outputStart) / outputLength) * sourceLength
  return sourceFrame / frameRateFps(source.rate)
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
  const videoRef = useRef<HTMLVideoElement>(null)
  const sourceTransitionFromClipRef = useRef<string | null>(null)
  const frame = timeline ? frameAt(timeline, outputPlayheadSec) : 0
  const clip = useMemo(
    () => timeline ? clipAt(timeline, frame) : null,
    [frame, timeline],
  )
  const source = timeline?.sources.find((candidate) => candidate.asset_id === clip?.source_asset_id) ?? null
  const sourceTime = timeline && clip ? sourceSeconds(timeline, frame, clip) : 0
  const cue = cues.find((candidate) =>
    frame >= num(candidate.start_frame) && frame < num(candidate.end_frame),
  ) ?? null
  const canvas = requestedCanvas ?? timeline?.canvas ?? null

  useEffect(() => {
    onSourceChange(source?.asset_id ?? null)
  }, [onSourceChange, source?.asset_id])

  useEffect(() => {
    const video = videoRef.current
    if (video && Math.abs(video.currentTime - sourceTime) > 0.18) video.currentTime = sourceTime
  }, [clip?.id, sourceTime])

  useEffect(() => {
    const video = videoRef.current
    if (!video) return
    if (playing) void video.play().catch(() => onPlayState(false))
    else video.pause()
  }, [clip?.id, onPlayState, playing])

  if (!timeline || !clip || !source || !canvas) {
    return <div className="studio-preview-frame studio-preview-empty"><span>主轨还没有可预览的片段</span><small>添加素材后，视频与字幕会按输出时间线显示。</small></div>
  }

  const clipIndex = timeline.clips.findIndex((candidate) => candidate.id === clip.id)
  const nextClip = timeline.clips[clipIndex + 1] ?? null
  const nextSource = nextClip && nextClip.source_asset_id !== source.asset_id
    ? timeline.sources.find((candidate) => candidate.asset_id === nextClip.source_asset_id) ?? null
    : null

  const cueText = cue
    ? (style?.show_speaker && cue.speaker_name ? cue.speaker_name + '：' : '') + cue.text
    : null
  const width = Math.max(1, num(canvas.width))
  const height = Math.max(1, num(canvas.height))
  const overlayStyle = style ? {
    left: String(style.position_x * 100) + '%',
    top: String(style.position_y * 100) + '%',
    maxWidth: String(style.max_width_ratio * 100) + '%',
    fontFamily: style.font_family,
    fontSize: String((style.font_size / width) * 100) + 'cqw',
    color: style.text_color,
    WebkitTextStroke: String((style.outline_width / width) * 100) + 'cqw ' + style.outline_color,
    textShadow: String((style.shadow_offset_x / width) * 100) + 'cqw '
      + String((style.shadow_offset_y / width) * 100) + 'cqw '
      + String((style.shadow_blur / width) * 100) + 'cqw ' + style.shadow_color,
    background: style.background_color,
    borderRadius: String((style.background_radius / width) * 100) + 'cqw',
    padding: String((style.background_padding_y / width) * 100) + 'cqw '
      + String((style.background_padding_x / width) * 100) + 'cqw',
  } : undefined

  return (
    <div className="studio-preview-frame studio-timeline-preview" style={{ background: canvas.background, aspectRatio: `${width} / ${height}` }}>
      <video
        key={clip.id}
        ref={videoRef}
        src={mediaAssetUrl(source.asset_id)}
        preload="auto"
        className="studio-timeline-preview-media"
        style={{
          objectFit: canvas.fit,
          opacity: canvas.opacity,
          transform: 'translate(' + String(canvas.position_x * 100) + 'cqw, ' + String(canvas.position_y * 100)
            + 'cqh) scale(' + String(Math.max(0.1, canvas.scale)) + ') rotate(' + String(canvas.rotation_degrees) + 'deg)',
        }}
        onLoadedMetadata={(event) => {
          event.currentTarget.currentTime = sourceTime
          if (playing) void event.currentTarget.play().catch(() => onPlayState(false))
        }}
        onTimeUpdate={(event) => {
          const currentFrame = Math.floor(event.currentTarget.currentTime * frameRateFps(source.rate))
          const sourceStart = num(clip.source_in_frame)
          const sourceLength = Math.max(1, num(clip.source_out_frame) - sourceStart)
          if (currentFrame >= num(clip.source_out_frame) - 1) {
            if (nextClip) {
              // Changing the keyed video element pauses the outgoing source. That pause
              // must not cancel the still-active output-timeline playback state.
              sourceTransitionFromClipRef.current = clip.id
              onOutputTimeUpdate(num(nextClip.timeline_start_frame) / frameRateFps(timeline.rate))
            } else {
              onOutputTimeUpdate(num(clip.timeline_end_frame) / frameRateFps(timeline.rate))
              event.currentTarget.pause()
            }
            return
          }
          const outputLength = num(clip.timeline_end_frame) - num(clip.timeline_start_frame)
          const outputFrame = num(clip.timeline_start_frame)
            + ((currentFrame - sourceStart) / sourceLength) * outputLength
          onOutputTimeUpdate(outputFrame / frameRateFps(timeline.rate))
        }}
        onPlay={() => onPlayState(true)}
        onPause={() => {
          if (sourceTransitionFromClipRef.current === clip.id) {
            sourceTransitionFromClipRef.current = null
            return
          }
          onPlayState(false)
        }}
      />
      {nextSource && <video className="studio-next-source-preload" src={mediaAssetUrl(nextSource.asset_id)} preload="auto" aria-hidden="true" tabIndex={-1} />}
      {cueText && <div className="studio-subtitle-preview" style={overlayStyle}>{cueText}</div>}
      <div className="studio-preview-label">{source.display_name}</div>
    </div>
  )
}
