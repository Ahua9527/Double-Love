import { FileText, Film, Subtitles, X } from 'lucide-react'
import type { OperationResult } from '../../../../bindings/OperationResult'
import type { ProjectExportPreview } from '../../../../bindings/ProjectExportPreview'
import { frameRateFps, num } from '../utils'

interface ProjectExportDialogProps {
  result: OperationResult<ProjectExportPreview>
  busy: boolean
  onClose: () => void
  onExport: (kind: 'xml' | 'ass' | 'mp4') => void
}

export function ProjectExportDialog({ result, busy, onClose, onExport }: ProjectExportDialogProps) {
  const preview = result.data
  const blocked = result.status === 'failed' || result.diagnostics.some((diagnostic) => diagnostic.blocks_export)
  return (
    <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="studio-export-dialog" role="dialog" aria-modal="true" aria-labelledby="export-title" onMouseDown={(event) => event.stopPropagation()}>
        <header><div><h2 id="export-title">导出项目</h2><p>所有格式来自同一份主轨时间线。</p></div><button type="button" aria-label="关闭导出" onClick={onClose}><X size={17} /></button></header>
        {preview ? (
          <div className="studio-export-summary"><span>{preview.timeline.clips.length} 个输出片段</span><span>{preview.subtitle_cues.length} 条字幕</span><span>{frameRateFps(preview.timeline.rate).toFixed(3).replace(/\.000$/, '')} fps</span><span>{num(preview.timeline.output_duration_frames)} 帧</span></div>
        ) : <p className="studio-export-error">无法编译当前时间线。</p>}
        {result.diagnostics.length > 0 && <div className="studio-export-diagnostics">{result.diagnostics.map((diagnostic) => <p key={`${diagnostic.code}-${diagnostic.cause}`}>{diagnostic.cause}</p>)}</div>}
        <div className="studio-export-options">
          <button type="button" disabled={blocked || busy} onClick={() => onExport('xml')}><FileText size={18} /><span><strong>Premiere／Resolve XML</strong><small>可继续编辑的多素材时间线</small></span></button>
          <button type="button" disabled={blocked || busy} onClick={() => onExport('ass')}><Subtitles size={18} /><span><strong>ASS 字幕</strong><small>完整保留项目级字幕样式</small></span></button>
          <button type="button" disabled={blocked || busy} onClick={() => onExport('mp4')}><Film size={18} /><span><strong>带字幕 MP4</strong><small>本地渲染，画布与字幕同步预览</small></span></button>
        </div>
        {preview?.compatibility.map((report) => <details key={report.target} className="studio-compatibility"><summary>{report.target} 兼容性</summary><ul>{report.limitations.map((item) => <li key={item}>{item}</li>)}</ul></details>)}
      </section>
    </div>
  )
}
