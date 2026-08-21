import { FolderOpen, Plus, Search, Trash2 } from 'lucide-react'
import type { MediaAssetSummary } from '../../../bindings/MediaAssetSummary'
import type { ProjectSummary } from '../../../bindings/ProjectSummary'
import type { RecentProject } from '../tauri'

interface ProjectLibraryProps {
  project: ProjectSummary | null
  assets: MediaAssetSummary[]
  onCreate: () => void
  onOpen: () => void
  onEnterEditor: () => void
  recentProjects?: RecentProject[]
  modelReady?: boolean
  onOpenModels?: () => void
  onForgetRecent?: (root: string) => void
}

function projectName(project: ProjectSummary): string {
  return project.root.split('/').filter(Boolean).pop() ?? '未命名项目'
}

export function ProjectLibrary({ project, assets, onCreate, onOpen, onEnterEditor, recentProjects = [], modelReady = true, onOpenModels, onForgetRecent }: ProjectLibraryProps) {
  if (!project) {
    return (
      <section className="studio-library-empty" aria-labelledby="library-empty-title">
        <div className="studio-library-mark" aria-hidden="true" />
        <h1 id="library-empty-title">从一个本地项目开始</h1>
        <p>项目保存转录、说话人和剪辑记录。原始视频始终留在你原来的位置。</p>
        <div className="studio-library-actions">
          <button type="button" className="studio-primary-button" onClick={onCreate}><Plus size={16} />新建项目</button>
          <button type="button" className="studio-secondary-button" onClick={onOpen}><FolderOpen size={16} />打开项目</button>
        </div>
        {recentProjects.length > 0 && <div className="studio-recent-projects" aria-label="最近项目"><strong>最近项目</strong>{recentProjects.slice(0, 3).map((recent) => <div key={recent.root}><button type="button" onClick={onOpen}><FolderOpen size={14} /><span><b>{recent.display_name}</b><small>{recent.exists ? '本地项目' : '文件夹已移动'}</small></span></button>{onForgetRecent && <button type="button" aria-label={`移除${recent.display_name}`} onClick={() => onForgetRecent(recent.root)}><Trash2 size={13} /></button>}</div>)}</div>}
      </section>
    )
  }

  return (
    <section className="studio-library" aria-labelledby="library-title">
      <header className="studio-library-head">
        <div><h1 id="library-title">我的项目</h1><p>本机文件，随时可离线继续。</p></div>
        <button type="button" className="studio-primary-button" onClick={onCreate}><Plus size={16} />新建项目</button>
      </header>
      <label className="studio-library-search"><Search size={16} /><input aria-label="搜索项目" placeholder="搜索项目、标题或转录文本" /></label>
      <div className="studio-project-list">
        <button type="button" className="studio-project-row" onClick={onEnterEditor}>
          <span className="studio-project-poster" aria-hidden="true"><i></i><i></i><i></i></span>
          <span className="studio-project-info"><strong>{projectName(project)}</strong><small>{assets.length} 个素材 · 本地项目</small></span>
          <span className="studio-project-open">继续编辑</span>
        </button>
      </div>
      {!modelReady && <div className="studio-model-notice" role="status"><span className="studio-model-notice-icon">i</span><div><strong>转录模型还没有安装</strong><small>现在可以管理项目和导入媒体；需要转录时再安装本地模型即可。</small></div><button type="button" onClick={onOpenModels}>查看本地模型</button></div>}
      <p className="studio-library-footnote">要打开另一个项目，请从左侧的“新建转录”旁边选择“打开项目”。</p>
    </section>
  )
}
