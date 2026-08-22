import { ArrowRight, Download, X } from 'lucide-react'
import type { ModelDescriptor } from '../tauri'

interface ModelInstallDialogProps {
  model: ModelDescriptor | null
  busy: boolean
  onInstall: () => void
  onClose: () => void
  onOpenSettings: () => void
}

export function ModelInstallDialog({ model, busy, onInstall, onClose, onOpenSettings }: ModelInstallDialogProps) {
  if (!model) return null
  return <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}><section className="model-install-dialog" role="dialog" aria-modal="true" aria-labelledby="model-install-title" onMouseDown={(event) => event.stopPropagation()}><header><div><span className="model-dialog-icon"><Download size={17} /></span><div><h2 id="model-install-title">先安装本地转录模型</h2><p>项目状态保持不变，安装完成后可以继续转录。</p></div></div><button type="button" aria-label="关闭模型安装提示" onClick={onClose}><X size={16} /></button></header><div className="model-dialog-summary"><strong>{model.label}</strong><span>{model.size_bytes ? `${formatBytes(model.size_bytes)} 下载` : '下载体积由模型清单提供'}</span><span>revision {model.revision}</span></div><p className="model-dialog-copy">Double Love 不会上传音频或声纹。模型权重只下载到本机 Application Support，校验完成后才会启用。</p><div className="model-dialog-actions"><button type="button" className="studio-secondary-button" onClick={onOpenSettings}>打开本地模型设置 <ArrowRight size={14} /></button><button type="button" className="studio-primary-button" disabled={busy} onClick={onInstall}>{busy ? '准备中…' : '安装模型'}</button></div></section></div>
}

function formatBytes(value: number | bigint): string {
  const bytes = typeof value === 'bigint' ? Number(value) : value
  if (!Number.isFinite(bytes) || bytes <= 0) return '—'
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`
  return `${Math.round(bytes / 1024 ** 2)} MB`
}
