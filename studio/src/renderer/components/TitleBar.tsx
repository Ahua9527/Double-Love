import { AlertTriangle, ChevronLeft, FolderPlus, Info, LoaderCircle, PanelLeft, Upload } from 'lucide-react'
import type { StudioScreen } from './Sidebar'
import { Tooltip } from './Tooltip'

interface TitleBarProps {
  projectName: string | null
  screen: StudioScreen
  sidebarVisible: boolean
  onToggleSidebar: () => void
  onBackToLibrary: () => void
  onAddMedia: () => void
  onExport: () => void
  onOpenProjectInfo: () => void
  addDisabled: boolean
  exportDisabled: boolean
  saveState: 'idle' | 'saving' | 'failed'
}

function titleFor(screen: StudioScreen, projectName: string | null) {
  if (screen === 'library') return '我的项目'
  if (screen === 'tasks') return '后台任务'
  if (screen === 'settings') return '设置'
  return projectName ?? '编辑器'
}

export function TitleBar({
  projectName,
  screen,
  sidebarVisible,
  onToggleSidebar,
  onBackToLibrary,
  onAddMedia,
  onExport,
  onOpenProjectInfo,
  addDisabled,
  exportDisabled,
  saveState,
}: TitleBarProps) {
  return (
    <header className="studio-titlebar">
      <Tooltip label="切换项目栏"><button type="button" className="studio-icon-button" aria-label="切换项目栏" aria-pressed={sidebarVisible} onClick={onToggleSidebar}><PanelLeft size={17} /></button></Tooltip>
      {screen === 'editor' && <Tooltip label="返回我的项目"><button type="button" className="studio-icon-button" aria-label="返回项目库" onClick={onBackToLibrary}><ChevronLeft size={18} /></button></Tooltip>}
      <div className="studio-titlebar-title"><strong>{titleFor(screen, projectName)}</strong>{screen === 'editor' && saveState === 'saving' && <LoaderCircle size={13} className="studio-save-spinner" aria-label="正在保存到本地" />}{screen === 'editor' && saveState === 'failed' && <AlertTriangle size={13} className="studio-save-error" aria-label="最近一次保存失败" />}{screen === 'editor' && <Tooltip label="项目设置与自动保存记录"><button type="button" className="studio-title-info" aria-label="打开项目设置" onClick={onOpenProjectInfo}><Info size={14} /></button></Tooltip>}</div>
      <div className="studio-titlebar-actions">
        {screen === 'editor' && <Tooltip label="添加素材"><button type="button" className="studio-icon-button" aria-label="添加素材" disabled={addDisabled} onClick={onAddMedia}><FolderPlus size={16} /></button></Tooltip>}
        {screen === 'editor' && <Tooltip label="导出项目"><button type="button" className="studio-icon-button" aria-label="导出项目" disabled={exportDisabled} onClick={onExport}><Upload size={16} /></button></Tooltip>}
      </div>
    </header>
  )
}
