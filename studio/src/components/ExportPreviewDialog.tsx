import type { ExportOutcome } from '../../../bindings/ExportOutcome'
import type { OperationResult } from '../../../bindings/OperationResult'
import { exportBlockMessage, formatClock, frameRateFps, num } from '../utils'

const LEVEL_DOT = {
  error: 'bg-danger',
  warning: 'bg-warning',
  info: 'bg-info',
} as const

interface ExportPreviewDialogProps {
  /** roughcut_preview 的结果（失败时也展示，确认键禁用） */
  result: OperationResult<ExportOutcome>
  busy: boolean
  onConfirm: () => void
  onCancel: () => void
}

/** 导出预览：PRD 不变量「Preview 先于 Apply」——确认键在有任何阻断诊断时禁用。 */
export function ExportPreviewDialog({ result, busy, onConfirm, onCancel }: ExportPreviewDialogProps) {
  const ir = result.data?.ir ?? null
  const blocked = result.status === 'failed' || result.diagnostics.some((d) => d.blocks_export)
  const blockMessage = exportBlockMessage(result.diagnostics)
  const outputSeconds = ir ? num(ir.output_duration_frames) / frameRateFps(ir.rate) : null

  return (
    <div className="fixed inset-0 z-20 bg-black/40 flex items-center justify-center">
      <div className="w-96 rounded-md border border-line bg-surface p-4 flex flex-col gap-3 shadow-lg">
        <div className="text-sm font-semibold">导出预览 · Premiere XML</div>
        {ir ? (
          <div className="flex flex-col gap-1 text-xs">
            <div className="flex justify-between">
              <span className="text-mutedfg">粗剪片段</span>
              <span className="font-mono">{ir.clips.length} 段</span>
            </div>
            <div className="flex justify-between">
              <span className="text-mutedfg">输出时长</span>
              <span className="font-mono">
                {formatClock(outputSeconds ?? 0)}（{num(ir.output_duration_frames)} 帧）
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-mutedfg">序列名</span>
              <span className="font-mono truncate max-w-56">{ir.name}</span>
            </div>
          </div>
        ) : (
          <div className="text-xs text-mutedfg">无法编译粗剪时间线，请根据下方诊断处理。</div>
        )}
        {result.diagnostics.length > 0 && (
          <div className="max-h-36 overflow-y-auto flex flex-col gap-1.5 border-t border-line pt-2">
            {result.diagnostics.map((diagnostic, index) => (
              <div key={`${diagnostic.code}-${index}`} className="flex items-start gap-1.5">
                <span className={`mt-1 w-1.5 h-1.5 flex-none rounded-full ${LEVEL_DOT[diagnostic.level]}`} />
                <div className="flex-1 min-w-0">
                  <span className="text-xs font-semibold">{diagnostic.code}</span>
                  <div className="text-xs text-mutedfg">{diagnostic.cause}</div>
                </div>
              </div>
            ))}
          </div>
        )}
        {blockMessage && <div className="text-xs text-danger">{blockMessage}</div>}
        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onCancel}
            className="h-7 px-3 rounded-md border border-line text-xs hover:bg-sidebaraccent"
          >
            取消
          </button>
          <button
            type="button"
            disabled={blocked || busy}
            onClick={onConfirm}
            className="h-7 px-3 rounded-md bg-love hover:bg-love/85 text-xs font-semibold text-white disabled:opacity-40 disabled:hover:bg-love"
          >
            {busy ? '正在导出…' : '选择保存位置…'}
          </button>
        </div>
      </div>
    </div>
  )
}
