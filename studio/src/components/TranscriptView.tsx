import { useReducer, useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Loader2, Scissors, Undo2 } from 'lucide-react'
import type { MediaAssetSummary } from '../../../bindings/MediaAssetSummary'
import type { TranscriptViewData } from '../../../bindings/TranscriptViewData'
import type { WordAnchor } from '../../../bindings/WordAnchor'
import {
  formatClock,
  needsSpaceBetween,
  num,
  omitCovers,
  omitCovering,
  selectionRange,
  selectionReducer,
  wordOrdinalAtTime,
} from '../utils'

export interface TranscriptionProgress {
  completed: number | null
  total: number | null
  message: string
}

interface TranscriptViewProps {
  asset: MediaAssetSummary
  view: TranscriptViewData | null
  playheadSec: number
  transcription: TranscriptionProgress | null
  speakerNames?: Map<string, string>
  onSeek: (seconds: number) => void
  onOmit: (startOrdinal: number, endOrdinal: number) => void
  onRestore: (operationId: string, startOrdinal: number, endOrdinal: number) => void
  onTranscribeStart: () => void
  onTranscribeCancel: () => void
}

export function TranscriptView(props: TranscriptViewProps) {
  const { asset, transcription, view } = props
  if (asset.status !== 'transcribed') {
    if (transcription) {
      const percent = transcription.completed !== null && transcription.total !== null && transcription.total > 0
        ? Math.round((transcription.completed / transcription.total) * 100)
        : null
      return (
        <div className="studio-transcript-state">
          <Loader2 size={17} className="animate-spin text-selected" />
          <strong>正在转录{percent !== null ? ` ${percent}%` : ''}</strong>
          <p>{transcription.message}</p>
          {percent !== null && <div className="studio-progress"><i style={{ width: `${percent}%` }} /></div>}
          <button type="button" className="studio-secondary-button" onClick={props.onTranscribeCancel}>取消转录</button>
        </div>
      )
    }
    return (
      <div className="studio-transcript-state">
        <strong>这段素材还没有转录</strong>
        <p>转录在本机完成，原始视频不会被修改。</p>
        <button type="button" className="studio-primary-button" onClick={props.onTranscribeStart}>开始转录</button>
      </div>
    )
  }
  if (!view || view.words.length === 0) {
    return <div className="studio-transcript-state"><strong>没有可显示的转录文本</strong><p>可以重新转录这段素材。</p></div>
  }
  return <VirtualTranscript {...props} view={view} />
}

function VirtualTranscript({
  asset,
  view,
  playheadSec,
  speakerNames = new Map(),
  onSeek,
  onOmit,
  onRestore,
}: TranscriptViewProps & { view: TranscriptViewData }) {
  const [selection, dispatch] = useReducer(selectionReducer, { anchor: null, focus: null, dragging: false })
  const scrollRef = useRef<HTMLDivElement>(null)
  const sampleRate = num(asset.audio_sample_rate)
  const wordByOrdinal = new Map(view.words.map((word) => [num(word.ordinal), word]))
  const virtualizer = useVirtualizer({
    count: view.segments.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 92,
    overscan: 8,
    measureElement: (element) => element.getBoundingClientRect().height,
  })
  const range = selectionRange(selection)
  const currentOrdinal = wordOrdinalAtTime(view.words, playheadSec, sampleRate)
  const restoreTarget = range ? omitCovering(view.omits, range[0]) : null
  const allOmitted = range
    ? Array.from({ length: range[1] - range[0] + 1 }, (_, index) => range[0] + index)
      .every((ordinal) => omitCovers(view.omits, ordinal))
    : false
  const seekToWord = (word: WordAnchor) => onSeek(num(word.start_sample) / sampleRate)

  const renderWord = (word: WordAnchor, previous: WordAnchor | undefined) => {
    const ordinal = num(word.ordinal)
    const omitted = omitCovers(view.omits, ordinal)
    const selected = range !== null && ordinal >= range[0] && ordinal <= range[1]
    const current = ordinal === currentOrdinal
    return (
      <span key={word.word_id}>
        {previous && needsSpaceBetween(previous.display_text, word.display_text) ? ' ' : null}
        <span
          data-ordinal={ordinal}
          onPointerDown={(event) => {
            event.preventDefault()
            dispatch({ type: 'down', ordinal })
          }}
          onPointerEnter={() => dispatch({ type: 'enter', ordinal })}
          className={`studio-word ${omitted ? 'is-omitted' : ''} ${selected ? 'is-selected' : ''} ${current && !selected ? 'is-current' : ''}`}
        >{word.display_text}</span>
      </span>
    )
  }

  return (
    <div className="studio-transcript">
      {range && (
        <div className="studio-selection-bar">
          <span>已选 {range[1] - range[0] + 1} 词</span>
          <button type="button" disabled={allOmitted} onClick={() => { onOmit(range[0], range[1]); dispatch({ type: 'clear' }) }}><Scissors size={12} />删除</button>
          {restoreTarget && <button type="button" onClick={() => { onRestore(restoreTarget.id, range[0], range[1]); dispatch({ type: 'clear' }) }}><Undo2 size={12} />恢复</button>}
          <button type="button" className="studio-text-action" onClick={() => dispatch({ type: 'clear' })}>取消</button>
        </div>
      )}
      <div
        ref={scrollRef}
        className="studio-transcript-scroll"
        onPointerUp={() => {
          if (selection.dragging && selection.anchor === selection.focus && selection.anchor !== null) {
            const word = wordByOrdinal.get(selection.anchor)
            if (word) seekToWord(word)
          }
          dispatch({ type: 'up' })
        }}
        onPointerLeave={() => dispatch({ type: 'up' })}
      >
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const segment = view.segments[virtualRow.index]
            const words: WordAnchor[] = []
            for (let ordinal = num(segment.start_ordinal); ordinal <= num(segment.end_ordinal); ordinal += 1) {
              const word = wordByOrdinal.get(ordinal)
              if (word) words.push(word)
            }
            const speakerId = words.find((word) => word.speaker_assignments[0])?.speaker_assignments[0]?.speaker_id
            const speakerName = speakerId ? speakerNames.get(speakerId) ?? '未命名说话人' : null
            return (
              <article
                key={segment.index}
                ref={virtualizer.measureElement}
                data-index={virtualRow.index}
                className={`studio-transcript-segment ${segment.omitted ? 'is-omitted' : ''}`}
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${virtualRow.start}px)` }}
              >
                <time>{formatClock(num(segment.start_sample) / sampleRate)}</time>
                <div>
                  {speakerName && <strong>{speakerName}</strong>}
                  <p>{words.map((word, index) => renderWord(word, words[index - 1]))}</p>
                </div>
              </article>
            )
          })}
        </div>
      </div>
    </div>
  )
}
