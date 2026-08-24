import { useState } from 'react'
import { Film, Plus, Trash2, X } from 'lucide-react'
import type { MediaAssetSummary } from '../../../../bindings/MediaAssetSummary'
import { assetStatusLabel, formatTimecodeSeconds, num } from '../utils'

interface MediaDrawerProps {
  assets: MediaAssetSummary[]
  busyAssetId: string | null
  onClose: () => void
  onAddExisting: (asset: MediaAssetSummary) => void
  onImport: () => void
  onDropFiles: (files: File[]) => void
  onRemove: (asset: MediaAssetSummary) => void
  usageCount: (assetId: string) => number
}

export function MediaDrawer({ assets, busyAssetId, onClose, onAddExisting, onImport, onDropFiles, onRemove, usageCount }: MediaDrawerProps) {
  const [deleteTarget, setDeleteTarget] = useState<MediaAssetSummary | null>(null)
  return (
    <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
        className="studio-media-drawer"
        role="dialog"
        aria-modal="true"
        aria-label="添加主轨素材"
        onMouseDown={(event) => event.stopPropagation()}
        onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' }}
        onDrop={(event) => {
          event.preventDefault()
          const files = Array.from(event.dataTransfer.files)
          if (files.length > 0) onDropFiles(files)
        }}
      >
        <header className="studio-drawer-head">
          <div><strong>添加主轨素材</strong><span>原始文件保持只读</span></div>
          <button type="button" aria-label="关闭素材抽屉" onClick={onClose}><X size={17} /></button>
        </header>
        <button type="button" className="studio-import-button" onClick={onImport}>
          <Plus size={16} />导入新的本地视频
        </button>
        <div className="studio-drawer-list">
          {assets.length === 0 ? (
            <div className="studio-drawer-empty">还没有导入媒体。先选择一个本地视频。</div>
          ) : assets.map((asset) => {
            const busy = busyAssetId === asset.id
            return (
              <button
                key={asset.id}
                type="button"
                className="studio-media-row"
                disabled={busy}
                onClick={() => onAddExisting(asset)}
                onContextMenu={(event) => { event.preventDefault(); setDeleteTarget(asset) }}
                onKeyDown={(event) => {
                  if (event.key === 'Delete' || event.key === 'Backspace' || ((event.shiftKey && event.key === 'F10') || event.key === 'ContextMenu')) {
                    event.preventDefault()
                    setDeleteTarget(asset)
                  }
                }}
              >
                <span className="studio-media-icon"><Film size={15} /></span>
                <span className="studio-media-copy"><strong>{asset.display_name}</strong><small>{formatTimecodeSeconds(num(asset.duration_samples) / num(asset.audio_sample_rate), asset.rate)} · {assetStatusLabel(asset.status)}</small></span>
                <span className="studio-media-add">{busy ? '加入中…' : '加入主轨'}</span>
              </button>
            )
          })}
        </div>
        {deleteTarget && (
          <div className="studio-media-delete-confirm" role="alertdialog" aria-label="从项目中删除素材">
            <Trash2 size={16} />
            <strong>从项目中删除“{deleteTarget.display_name}”？</strong>
            <p>将移除 {usageCount(deleteTarget.id)} 个主轨片段{deleteTarget.status === 'transcribed' ? '，并隐藏已有转录' : ''}。外部原始视频不会被删除，可通过撤销恢复。</p>
            <div><button type="button" onClick={() => setDeleteTarget(null)}>取消</button><button type="button" className="is-danger" onClick={() => { onRemove(deleteTarget); setDeleteTarget(null) }}>从项目中删除</button></div>
          </div>
        )}
      </aside>
    </div>
  )
}
