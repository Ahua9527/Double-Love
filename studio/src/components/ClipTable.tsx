import type { ClipStatus, FixtureClip, Rating } from '../fixtures'
import { ratingLabel } from '../utils'

// 列宽（px），与 GPUI 骨架一致
const COLUMNS: Array<{ label: string; width: number }> = [
  { label: '状态', width: 28 },
  { label: '新名称', width: 150 },
  { label: '源文件', width: 170 },
  { label: '场', width: 44 },
  { label: '镜', width: 36 },
  { label: '次', width: 48 },
  { label: '机位', width: 36 },
  { label: '评分', width: 44 },
  { label: '入点', width: 88 },
  { label: '时长', width: 88 },
  { label: '备注', width: 180 },
]

const STATUS_DOT: Record<ClipStatus, string> = {
  processed: 'bg-success',
  ignored: 'bg-mutedfg',
  skipped: 'bg-warning',
  failed: 'bg-danger',
}

function RatingChip({ rating }: { rating: Rating }) {
  if (rating === 'none') {
    return <span className="text-xs text-mutedfg">—</span>
  }
  const color =
    rating === 'ok' ? 'text-success bg-success/15'
    : rating === 'keep' ? 'text-selected bg-selected/15'
    : 'text-danger bg-danger/15'
  return (
    <span className={`px-1 rounded-sm text-xs ${color}`}>{ratingLabel(rating)}</span>
  )
}

interface ClipTableProps {
  clips: FixtureClip[]
  selected: number
  onSelect: (index: number) => void
}

export function ClipTable({ clips, selected, onSelect }: ClipTableProps) {
  return (
    <div className="flex-1 min-h-0 mx-3 mb-3 rounded-md border border-line bg-table overflow-hidden flex flex-col">
      <div className="h-8 flex-none px-2 flex items-center gap-2 bg-tablehead border-b border-tableline">
        {COLUMNS.map((col) => (
          <div
            key={col.label}
            style={{ width: col.width }}
            className="flex-none text-xs text-tableheadfg"
          >
            {col.label}
          </div>
        ))}
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto">
        {clips.map((clip, index) => {
          const ignored = clip.status === 'ignored'
          const isSelected = index === selected
          return (
            <button
              type="button"
              key={clip.id}
              onClick={() => onSelect(index)}
              className={`w-full h-8 px-2 flex items-center gap-2 text-sm text-left border-b border-tableline ${
                ignored ? 'text-mutedfg' : 'text-fg'
              } ${isSelected ? 'bg-selected/15' : 'hover:bg-tablehover'}`}
            >
              <span style={{ width: COLUMNS[0].width }} className="flex-none">
                <span
                  className={`inline-block w-1.5 h-1.5 rounded-full ${STATUS_DOT[clip.status]}`}
                />
              </span>
              <span style={{ width: COLUMNS[1].width }} className="flex-none font-mono truncate">
                {clip.newName}
              </span>
              <span
                style={{ width: COLUMNS[2].width }}
                className="flex-none font-mono text-mutedfg truncate"
              >
                {clip.sourceName}
              </span>
              <span style={{ width: COLUMNS[3].width }} className="flex-none">{clip.scene}</span>
              <span style={{ width: COLUMNS[4].width }} className="flex-none">{clip.shot}</span>
              <span style={{ width: COLUMNS[5].width }} className="flex-none">{clip.take}</span>
              <span style={{ width: COLUMNS[6].width }} className="flex-none">{clip.camera}</span>
              <span style={{ width: COLUMNS[7].width }} className="flex-none">
                <RatingChip rating={clip.rating} />
              </span>
              <span style={{ width: COLUMNS[8].width }} className="flex-none font-mono text-xs">
                {clip.tcIn}
              </span>
              <span style={{ width: COLUMNS[9].width }} className="flex-none font-mono text-xs">
                {clip.duration}
              </span>
              <span
                style={{ width: COLUMNS[10].width }}
                className="flex-none flex items-center gap-1 min-w-0"
              >
                {clip.fromCsv && (
                  <span className="px-1 rounded-sm bg-selected/15 text-xs">CSV</span>
                )}
                {clip.note && (
                  <span
                    className={`text-xs truncate ${
                      clip.status === 'failed' ? 'text-danger' : 'text-mutedfg'
                    }`}
                  >
                    {clip.note}
                  </span>
                )}
              </span>
            </button>
          )
        })}
      </div>
    </div>
  )
}
