import { X } from 'lucide-react'
import type { FrameRate } from '../../../../bindings/FrameRate'
import type { RevisionHistoryEntry } from '../../../../bindings/RevisionHistoryEntry'
import { frameRateLabel, historyRecordTitle, historyTimestamp, num } from '../utils'

interface ProjectInfoDialogProps {
  outputRate: FrameRate | null
  resolvedRate: FrameRate | null
  history: RevisionHistoryEntry[]
  onRateChange: (rate: FrameRate | null) => void
  onRestore: (revision: number) => void
  onClose: () => void
}

const RATES: FrameRate[] = ['fps_24_ntsc', 'fps_24', 'fps_25', 'fps_30_ntsc', 'fps_30', 'fps_50', 'fps_60_ntsc', 'fps_60', 'fps_120_ntsc', 'fps_120']

export function ProjectInfoDialog({ outputRate, resolvedRate, history, onRateChange, onRestore, onClose }: ProjectInfoDialogProps) {
  return <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="project-info-dialog" role="dialog" aria-modal="true" aria-labelledby="project-info-title" onMouseDown={(event) => event.stopPropagation()}>
      <header><div><h2 id="project-info-title">项目设置</h2></div><button type="button" aria-label="关闭项目设置" onClick={onClose}><X size={16} /></button></header>
      <div className="project-info-row"><div><strong>输出帧率</strong>{!outputRate && resolvedRate && <small>{frameRateLabel(resolvedRate)}</small>}</div><select aria-label="项目输出帧率" value={outputRate ?? 'auto'} onChange={(event) => onRateChange(event.target.value === 'auto' ? null : event.target.value as FrameRate)}><option value="auto">跟随第一段</option>{RATES.map((rate) => <option key={rate} value={rate}>{frameRateLabel(rate)}</option>)}</select></div>
      <div className="project-history-heading"><strong>自动保存记录</strong></div>
      <div className="project-info-history">{history.length === 0 ? <p>还没有可恢复的编辑记录。</p> : history.map((entry) => <article key={num(entry.revision)}><div><strong>{historyRecordTitle(entry.operation)}</strong><small>版本 {num(entry.revision)} · {historyTimestamp(entry.committed_at)}</small></div><button type="button" disabled={!entry.restorable} onClick={() => onRestore(num(entry.revision))}>{entry.restorable ? '恢复' : '已超出上限'}</button></article>)}</div>
    </section>
  </div>
}
