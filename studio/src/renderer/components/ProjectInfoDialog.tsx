import { X } from 'lucide-react'
import type { FrameRate } from '../../../../bindings/FrameRate'
import type { RevisionHistoryEntry } from '../../../../bindings/RevisionHistoryEntry'
import { frameRateLabel, num } from '../utils'

interface ProjectInfoDialogProps {
  outputRate: FrameRate | null
  resolvedRate: FrameRate | null
  history: RevisionHistoryEntry[]
  onRateChange: (rate: FrameRate | null) => void
  onRestore: (revision: number) => void
  onClose: () => void
}

const RATES: FrameRate[] = ['fps_24_ntsc', 'fps_24', 'fps_25', 'fps_30_ntsc', 'fps_30', 'fps_50', 'fps_60_ntsc', 'fps_60']

export function ProjectInfoDialog({ outputRate, resolvedRate, history, onRateChange, onRestore, onClose }: ProjectInfoDialogProps) {
  return <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="project-info-dialog" role="dialog" aria-modal="true" aria-labelledby="project-info-title" onMouseDown={(event) => event.stopPropagation()}>
      <header><div><h2 id="project-info-title">项目设置</h2><p>输出帧率和可恢复的本地编辑历史。</p></div><button type="button" aria-label="关闭项目设置" onClick={onClose}><X size={16} /></button></header>
      <div className="project-info-row"><div><strong>输出帧率</strong><small>{outputRate ? '项目已固定输出帧率。' : `跟随第一段素材${resolvedRate ? ` · ${frameRateLabel(resolvedRate)}` : ''}`}</small></div><select aria-label="项目输出帧率" value={outputRate ?? 'auto'} onChange={(event) => onRateChange(event.target.value === 'auto' ? null : event.target.value as FrameRate)}><option value="auto">跟随第一段</option>{RATES.map((rate) => <option key={rate} value={rate}>{frameRateLabel(rate)}</option>)}</select></div>
      <div className="project-history-heading"><strong>历史版本</strong><span>恢复会生成一个新版本</span></div>
      <div className="project-info-history">{history.length === 0 ? <p>还没有可恢复的编辑记录。</p> : history.slice(0, 12).map((entry) => <article key={num(entry.revision)}><div><strong>{entry.operation.replace(/_/g, ' ')}</strong><small>版本 {num(entry.revision)} · {entry.committed_at}</small></div><button type="button" disabled={!entry.restorable} onClick={() => onRestore(num(entry.revision))}>恢复</button></article>)}</div>
    </section>
  </div>
}
