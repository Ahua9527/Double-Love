import { useReducer } from 'react'
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
  /** 转录任务进行中；null = 无任务 */
  transcription: TranscriptionProgress | null
  onSeek: (seconds: number) => void
  onOmit: (startOrdinal: number, endOrdinal: number) => void
  onRestore: (operationId: string, startOrdinal: number, endOrdinal: number) => void
  onTranscribeStart: () => void
  onTranscribeCancel: () => void
}

export function TranscriptView({
  asset,
  view,
  playheadSec,
  transcription,
  onSeek,
  onOmit,
  onRestore,
  onTranscribeStart,
  onTranscribeCancel,
}: TranscriptViewProps) {
  const [selection, dispatch] = useReducer(selectionReducer, {
    anchor: null,
    focus: null,
    dragging: false,
  })
  const sampleRate = num(asset.audio_sample_rate)

  // 未转录：入口或进度
  if (asset.status !== 'transcribed') {
    if (transcription) {
      const percent =
        transcription.completed !== null &&
        transcription.total !== null &&
        transcription.total > 0
          ? Math.round((transcription.completed / transcription.total) * 100)
          : null
      return (
        <div className="flex-1 min-h-0 mx-3 mb-2 rounded-md border border-line flex flex-col items-center justify-center gap-2">
          <Loader2 size={16} className="animate-spin text-selected" />
          <div className="text-xs text-fg">
            正在转录{percent !== null ? ` ${percent}%` : '…'}
          </div>
          {percent !== null && (
            <div className="w-48 h-1 rounded-full bg-sidebaraccent overflow-hidden">
              <div className="h-full bg-selected" style={{ width: `${percent}%` }} />
            </div>
          )}
          <div className="text-xs text-mutedfg">{transcription.message}</div>
          <button
            type="button"
            onClick={onTranscribeCancel}
            className="h-7 px-3 rounded-md border border-line text-xs hover:bg-sidebaraccent"
          >
            取消转录
          </button>
        </div>
      )
    }
    return (
      <div className="flex-1 min-h-0 mx-3 mb-2 rounded-md border border-line flex flex-col items-center justify-center gap-2">
        <div className="text-xs text-mutedfg">转录全部在本机完成，原始媒体不会被修改</div>
        <button
          type="button"
          onClick={onTranscribeStart}
          className="h-8 px-4 rounded-md bg-selected hover:bg-selected/85 text-sm font-semibold text-white"
        >
          开始转录
        </button>
      </div>
    )
  }

  if (!view || view.words.length === 0) {
    return (
      <div className="flex-1 min-h-0 mx-3 mb-2 rounded-md border border-line flex items-center justify-center">
        <span className="text-xs text-mutedfg">没有转录文本，可重新转录</span>
      </div>
    )
  }

  const range = selectionRange(selection)
  const currentOrdinal = wordOrdinalAtTime(view.words, playheadSec, sampleRate)
  const wordByOrdinal = new Map(view.words.map((word) => [num(word.ordinal), word]))
  const restoreTarget = range ? omitCovering(view.omits, range[0]) : null
  const allOmitted = range
    ? Array.from({ length: range[1] - range[0] + 1 }, (_, i) => range[0] + i).every((ordinal) =>
        omitCovers(view.omits, ordinal),
      )
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
          className={`cursor-text rounded-[2px] ${
            omitted ? 'line-through text-mutedfg' : ''
          } ${selected ? 'bg-selected/30' : ''} ${current && !selected ? 'bg-warning/50' : ''}`}
        >
          {word.display_text}
        </span>
      </span>
    )
  }

  return (
    <div className="flex-1 min-h-0 mx-3 mb-2 rounded-md border border-line relative flex flex-col">
      {range && (
        <div className="sticky top-0 z-10 flex items-center gap-2 px-3 py-1.5 bg-card border-b border-line text-xs">
          <span className="text-mutedfg">已选 {range[1] - range[0] + 1} 词</span>
          <button
            type="button"
            disabled={allOmitted}
            onClick={() => {
              onOmit(range[0], range[1])
              dispatch({ type: 'clear' })
            }}
            className="h-6 px-2 rounded-sm bg-danger/15 text-danger hover:bg-danger/25 flex items-center gap-1 disabled:opacity-40"
          >
            <Scissors size={11} />
            删除
          </button>
          {restoreTarget && (
            <button
              type="button"
              onClick={() => {
                onRestore(restoreTarget.id, range[0], range[1])
                dispatch({ type: 'clear' })
              }}
              className="h-6 px-2 rounded-sm bg-selected/15 text-selected hover:bg-selected/25 flex items-center gap-1"
            >
              <Undo2 size={11} />
              恢复
            </button>
          )}
          <button
            type="button"
            onClick={() => dispatch({ type: 'clear' })}
            className="h-6 px-2 rounded-sm text-mutedfg hover:text-fg"
          >
            取消
          </button>
        </div>
      )}
      <div
        className="flex-1 min-h-0 overflow-y-auto p-3 flex flex-col gap-3 select-none"
        onPointerUp={() => {
          // 单击（按下未拖动）→ seek 到该词；拖动 → 形成选区
          if (selection.dragging && selection.anchor === selection.focus && selection.anchor !== null) {
            const word = wordByOrdinal.get(selection.anchor)
            if (word) seekToWord(word)
          }
          dispatch({ type: 'up' })
        }}
        onPointerLeave={() => dispatch({ type: 'up' })}
      >
        {view.segments.map((segment) => {
          const words: WordAnchor[] = []
          for (let ordinal = num(segment.start_ordinal); ordinal <= num(segment.end_ordinal); ordinal++) {
            const word = wordByOrdinal.get(ordinal)
            if (word) words.push(word)
          }
          const segmentOmitted = segment.omitted
          return (
            <div key={segment.index} className="flex gap-2">
              <span className="w-10 flex-none pt-0.5 text-right text-[10px] font-mono text-mutedfg">
                {formatClock(num(segment.start_sample) / sampleRate)}
              </span>
              <p
                className={`flex-1 text-sm leading-6 ${segmentOmitted ? 'opacity-60' : ''}`}
              >
                {words.map((word, index) => renderWord(word, words[index - 1]))}
              </p>
            </div>
          )
        })}
      </div>
    </div>
  )
}
