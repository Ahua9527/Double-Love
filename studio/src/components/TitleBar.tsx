import { ChevronLeft, FolderPlus, PanelLeft, Upload } from 'lucide-react'
import type { StudioScreen } from './Sidebar'

interface TitleBarProps {
  projectName: string | null
  screen: StudioScreen
  sidebarVisible: boolean
  onToggleSidebar: () => void
  onBackToLibrary: () => void
  onAddMedia: () => void
  onExport: () => void
  addDisabled: boolean
  exportDisabled: boolean
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
  addDisabled,
  exportDisabled,
}: TitleBarProps) {
  return (
    <header className="studio-titlebar" data-tauri-drag-region>
      <button type="button" className="studio-icon-button" aria-label="切换项目栏" aria-pressed={sidebarVisible} onClick={onToggleSidebar}><PanelLeft size={17} /></button>
      {screen === 'editor' && <button type="button" className="studio-icon-button" aria-label="返回项目库" onClick={onBackToLibrary}><ChevronLeft size={18} /></button>}
      <div className="studio-titlebar-title"><strong>{titleFor(screen, projectName)}</strong>{screen === 'editor' && <span>本地粗剪</span>}</div>
      <div className="studio-titlebar-actions">
        {screen === 'editor' && <button type="button" className="studio-subtle-button" disabled={addDisabled} onClick={onAddMedia}><FolderPlus size={15} />添加素材</button>}
        {screen === 'editor' && <button type="button" className="studio-export-button" disabled={exportDisabled} onClick={onExport}><Upload size={15} />导出</button>}
      </div>
    </header>
  )
}
