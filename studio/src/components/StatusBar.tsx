import type { MediaAssetSummary } from '../../../bindings/MediaAssetSummary'
import type { ProjectSummary } from '../../../bindings/ProjectSummary'
import { assetStatusLabel } from '../utils'

interface StatusBarProps {
  project: ProjectSummary | null
  assetCount: number
  asset: MediaAssetSummary | null
}

export function StatusBar({ project, assetCount, asset }: StatusBarProps) {
  return (
    <footer className="h-7 flex-none px-3 flex items-center justify-between border-t border-line text-xs text-mutedfg">
      <div className="flex items-center gap-2 min-w-0">
        {project ? (
          <>
            <span className="truncate" title={project.root}>{project.root}</span>
            <span className="flex-none">｜资产 {assetCount}</span>
          </>
        ) : (
          <span>未打开项目</span>
        )}
      </div>
      <span className="flex-none">
        {asset ? `${asset.display_name} · ${assetStatusLabel(asset.status)}` : '未选择资产'}
      </span>
    </footer>
  )
}
