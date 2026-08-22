import { Film, Plus, X } from 'lucide-react'
import type { MediaAssetSummary } from '../../../../bindings/MediaAssetSummary'
import { assetStatusLabel, formatClock, num } from '../utils'

interface MediaDrawerProps {
  assets: MediaAssetSummary[]
  busyAssetId: string | null
  onClose: () => void
  onAddExisting: (asset: MediaAssetSummary) => void
  onImport: () => void
}

export function MediaDrawer({ assets, busyAssetId, onClose, onAddExisting, onImport }: MediaDrawerProps) {
  return (
    <div className="studio-popover-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
        className="studio-media-drawer"
        role="dialog"
        aria-modal="true"
        aria-label="添加主轨素材"
        onMouseDown={(event) => event.stopPropagation()}
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
              >
                <span className="studio-media-icon"><Film size={15} /></span>
                <span className="studio-media-copy"><strong>{asset.display_name}</strong><small>{formatClock(num(asset.duration_samples) / num(asset.audio_sample_rate))} · {assetStatusLabel(asset.status)}</small></span>
                <span className="studio-media-add">{busy ? '加入中…' : '加入主轨'}</span>
              </button>
            )
          })}
        </div>
      </aside>
    </div>
  )
}
