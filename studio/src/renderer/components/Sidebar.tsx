import { Clock3, FolderOpen, Grid2X2, PlusCircle, Settings2 } from 'lucide-react'
import type { ProjectSummary } from '../../../../bindings/ProjectSummary'

export type StudioScreen = 'library' | 'editor' | 'tasks' | 'settings'

interface SidebarProps {
  project: ProjectSummary | null
  screen: StudioScreen
  onNavigate: (screen: StudioScreen) => void
  onCreate: () => void
  onOpen: () => void
  onOpenSettings?: () => void
}

const ITEMS: Array<{ id: StudioScreen; label: string; icon: typeof Grid2X2 }> = [
  { id: 'library', label: '我的项目', icon: FolderOpen },
  { id: 'tasks', label: '后台任务', icon: Clock3 },
]

export function Sidebar({ project, screen, onNavigate, onCreate, onOpen, onOpenSettings }: SidebarProps) {
  const projectName = project?.root.split('/').filter(Boolean).pop()
  return (
    <aside className="studio-sidebar" aria-label="工作区导航">
      <div className="studio-sidebar-top">
        <button type="button" className="studio-new-transcription" onClick={onCreate}><PlusCircle size={17} />新建转录</button>
        <nav className="studio-nav-list">
          {ITEMS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              aria-current={screen === id ? 'page' : undefined}
              className={screen === id ? 'is-active' : ''}
              onClick={() => onNavigate(id)}
            ><Icon size={17} /><span>{label}</span>{id === 'tasks' && <i aria-hidden="true" />}</button>
          ))}
        </nav>
      </div>
      <div className="studio-sidebar-recent">
        <span>最近</span>
        {projectName ? (
          <button type="button" onClick={() => onNavigate('editor')}><FolderOpen size={14} /><b>{projectName}</b><small>本地项目</small></button>
        ) : (
          <button type="button" onClick={onOpen}><FolderOpen size={14} /><b>打开已有项目</b><small>选择本地文件夹</small></button>
        )}
      </div>
      <button type="button" className="studio-sidebar-settings" onClick={() => onOpenSettings ? onOpenSettings() : onNavigate('settings')}><Settings2 size={17} />设置</button>
    </aside>
  )
}
