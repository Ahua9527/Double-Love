import type { MediaAssetSummary } from '../../../bindings/MediaAssetSummary'
import type { ProjectSummary } from '../../../bindings/ProjectSummary'
import { assetStatusLabel, formatClock, num } from '../utils'

interface SidebarProps {
  project: ProjectSummary | null
  assets: MediaAssetSummary[]
  currentId: string | null
  onSelect: (assetId: string) => void
  onImport: () => void
}

const STATUS_DOT = {
  imported: 'bg-mutedfg',
  prepared: 'bg-info',
  transcribed: 'bg-success',
} as const

/** 路径只显示最后一段（完整路径放 title）。 */
function baseName(path: string): string {
  return path.split('/').filter(Boolean).pop() ?? path
}

export function Sidebar({ project, assets, currentId, onSelect, onImport }: SidebarProps) {
  return (
    <nav className="w-52 flex-none h-full bg-sidebar border-r border-sidebarline p-3 flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <div className="text-xs font-semibold text-mutedfg">项目</div>
        {project ? (
          <div className="h-6 px-2 flex items-center rounded-sm text-sm truncate" title={project.root}>
            {baseName(project.root)}
          </div>
        ) : (
          <div className="h-6 px-2 flex items-center rounded-sm text-sm text-mutedfg">未打开项目</div>
        )}
      </div>
      <div className="flex-1 min-h-0 flex flex-col gap-1">
        <div className="flex items-center justify-between">
          <div className="text-xs font-semibold text-mutedfg">资产（{assets.length}）</div>
          {project && (
            <button
              type="button"
              onClick={onImport}
              className="text-xs text-selected hover:underline"
            >
              导入…
            </button>
          )}
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto flex flex-col gap-0.5">
          {assets.length === 0 && project && (
            <div className="px-2 py-1 text-xs text-mutedfg">还没有媒体，点「导入…」开始</div>
          )}
          {assets.map((asset) => (
            <button
              key={asset.id}
              type="button"
              onClick={() => onSelect(asset.id)}
              className={`px-2 py-1 flex items-center gap-2 rounded-sm text-left text-sm ${
                asset.id === currentId ? 'bg-selected/15 text-selected' : 'hover:bg-sidebaraccent'
              }`}
              title={`${asset.display_name} · ${assetStatusLabel(asset.status)}`}
            >
              <span className={`w-1.5 h-1.5 flex-none rounded-full ${STATUS_DOT[asset.status]}`} />
              <span className="flex-1 min-w-0 truncate">{asset.display_name}</span>
              <span className="text-xs text-mutedfg">
                {formatClock(num(asset.duration_samples) / num(asset.audio_sample_rate))}
              </span>
            </button>
          ))}
        </div>
      </div>
    </nav>
  )
}
