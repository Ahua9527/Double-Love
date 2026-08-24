import {
  ArrowRight,
  Folder,
  FolderInput,
  Grid2X2,
  List,
  MoreHorizontal,
  Plus,
  Search,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { MediaAssetSummary } from "../../../../bindings/MediaAssetSummary";
import type { ProjectSummary } from "../../../../bindings/ProjectSummary";
import type { ProjectLibraryView, RecentProject } from "../platform/desktop";
import { frameRateLabel, num } from "../utils";
import { Tooltip } from "./Tooltip";

interface ProjectLibraryProps {
  project: ProjectSummary | null;
  assets: MediaAssetSummary[];
  recentProjects?: RecentProject[];
  view: ProjectLibraryView;
  modelReady?: boolean;
  onCreate: () => void;
  onImport: () => void;
  onViewChange: (view: ProjectLibraryView) => void;
  onOpenRecent: (project: RecentProject) => void;
  onRelocate: (project: RecentProject) => void;
  onOpenModels?: () => void;
  onRemoveRecent: (project: RecentProject) => Promise<boolean>;
  onTrash: (project: RecentProject) => Promise<boolean>;
  trashDisabled?: boolean;
}

function dateLabel(value: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function canvasLabel(recent: RecentProject): string {
  if (!recent.canvas) return "—";
  return `${num(recent.canvas.width)} × ${num(recent.canvas.height)}`;
}

function thumbnailUrl(recent: RecentProject): string | null {
  if (!recent.exists || !recent.project_id) return null;
  const version = encodeURIComponent(recent.modified_at ?? recent.last_opened_at);
  return `dl-thumbnail://project/${encodeURIComponent(recent.project_id)}?v=${version}`;
}

export function ProjectLibrary({
  project,
  assets,
  recentProjects = [],
  view,
  modelReady = true,
  onCreate,
  onImport,
  onViewChange,
  onOpenRecent,
  onRelocate,
  onOpenModels,
  onRemoveRecent,
  onTrash,
  trashDisabled = false,
}: ProjectLibraryProps) {
  const [query, setQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<{
    project: RecentProject;
    left: number;
    top: number;
  } | null>(null);
  const [deleteCandidate, setDeleteCandidate] = useState<RecentProject | null>(null);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const filteredProjects = useMemo(() => {
    const keyword = query.trim().toLocaleLowerCase();
    return keyword
      ? recentProjects.filter((recent) =>
          recent.display_name.toLocaleLowerCase().includes(keyword),
        )
      : recentProjects;
  }, [query, recentProjects]);
  useEffect(() => {
    setContextMenu(null);
  }, [query, view]);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setContextMenu(null);
      if (!deleteBusy) setDeleteCandidate(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [deleteBusy]);
  const openContextMenu = (project: RecentProject, left: number, top: number) => {
    setContextMenu({
      project,
      left: Math.max(8, Math.min(window.innerWidth - 158, left)),
      top: Math.max(8, Math.min(window.innerHeight - 48, top)),
    });
  };

  const projectItem = (recent: RecentProject) => {
    const isCurrent = project?.project_id === recent.project_id;
    const canOpen = recent.exists && Boolean(recent.project_id);
    const thumbnail = thumbnailUrl(recent);
    return (
      <article
        className={`studio-project-item is-${view}${isCurrent ? " is-current" : ""}${recent.exists ? "" : " is-missing"}${contextMenu?.project === recent ? " is-context-selected" : ""}`}
        key={recent.project_id ?? recent.root}
        onContextMenu={(event) => {
          event.preventDefault();
          openContextMenu(recent, event.clientX, event.clientY);
        }}
        onKeyDown={(event) => {
          if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
            event.preventDefault();
            const rect = event.currentTarget.getBoundingClientRect();
            openContextMenu(recent, rect.left + 24, rect.top + 32);
          }
        }}
      >
        <button
          type="button"
          className="studio-project-main"
          disabled={!canOpen}
          onClick={() => onOpenRecent(recent)}
        >
          {view === "grid" && (
            <span className="studio-project-thumbnail" aria-hidden="true">
              {thumbnail && (
                <img
                  src={thumbnail}
                  alt=""
                  loading="lazy"
                  onError={(event) => {
                    event.currentTarget.hidden = true;
                  }}
                />
              )}
              <Folder size={24} />
            </span>
          )}
          <span className="studio-project-info">
            <strong>{recent.display_name}</strong>
            {view === "grid" && (
              <small>
                {!recent.exists
                  ? "项目位置已丢失"
                  : `修改于 ${dateLabel(recent.modified_at)}`}
              </small>
            )}
          </span>
          {view === "list" && (
            <>
              <span>{dateLabel(recent.created_at)}</span>
              <span>{dateLabel(recent.modified_at)}</span>
              <span>{canvasLabel(recent)}</span>
              <span>{recent.output_rate ? frameRateLabel(recent.output_rate) : "未设置"}</span>
            </>
          )}
          <span className="studio-project-open">
            {canOpen ? (isCurrent ? "继续编辑" : "打开") : "不可用"}
            {canOpen && <ArrowRight size={14} />}
          </span>
        </button>
        {!canOpen && (
          <div className="studio-project-recovery">
            <button type="button" onClick={() => onRelocate(recent)}>
              {recent.exists ? "重新导入" : "重新定位"}
            </button>
          </div>
        )}
        {isCurrent && view === "grid" && (
          <span className="studio-project-current">{assets.length} 个素材 · 正在使用</span>
        )}
      </article>
    );
  };

  return (
    <section className="studio-library" aria-labelledby="library-title">
      <header className="studio-library-head">
        <div className="studio-library-head-main">
          <h1 id="library-title">我的项目</h1>
          <label className="studio-library-search">
            <Search size={15} />
            <input
              aria-label="搜索项目"
              placeholder="搜索项目"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        </div>
        <div className="studio-library-head-actions">
          <span className="studio-library-count">{recentProjects.length} 个项目</span>
          <div className="studio-library-view-actions" role="group" aria-label="项目视图">
            <Tooltip label="网格视图"><button
              type="button"
              aria-label="网格视图"
              aria-pressed={view === "grid"}
              onClick={() => onViewChange("grid")}
            >
              <Grid2X2 size={15} />
            </button></Tooltip>
            <Tooltip label="列表视图"><button
              type="button"
              aria-label="列表视图"
              aria-pressed={view === "list"}
              onClick={() => onViewChange("list")}
            >
              <List size={16} />
            </button></Tooltip>
          </div>
          <button type="button" className="studio-primary-button studio-library-create" onClick={onCreate}>
            <Plus size={16} />新建项目
          </button>
          <div className="studio-library-menu">
            <Tooltip label="更多项目操作"><button
              type="button"
              className="studio-icon-button"
              aria-label="更多项目操作"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((open) => !open)}
            >
              <MoreHorizontal size={17} />
            </button></Tooltip>
            {menuOpen && (
              <div role="menu">
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    onImport();
                  }}
                >
                  <FolderInput size={14} />导入已有项目…
                </button>
              </div>
            )}
          </div>
        </div>
      </header>

      {view === "list" && filteredProjects.length > 0 && (
        <div className="studio-project-list-head" aria-hidden="true">
          <span>项目名称</span><span>创建时间</span><span>最后修改</span>
          <span>画布大小</span><span>帧率</span><span />
        </div>
      )}
      <div className={`studio-project-collection is-${view}`} aria-live="polite">
        {filteredProjects.length === 0 ? (
          <div className="studio-project-list-empty">
            <Folder size={22} />
            <strong>{query.trim() ? "没有匹配的项目" : "项目库还是空的"}</strong>
            <p>
              {query.trim()
                ? "请换一个项目名搜索。"
                : "使用右上角的新建项目，或从更多菜单导入已有项目。"}
            </p>
          </div>
        ) : (
          filteredProjects.map(projectItem)
        )}
      </div>

      {!modelReady && (
        <div className="studio-model-notice" role="status">
          <span className="studio-model-notice-icon">i</span>
          <div>
            <strong>转录模型还没有安装</strong>
            <small>现在可以管理项目和导入媒体；需要转录时再安装本地模型。</small>
          </div>
          <button type="button" onClick={onOpenModels}>查看本地模型</button>
        </div>
      )}
      {contextMenu && (
        <div className="studio-context-menu-layer" role="presentation" onMouseDown={() => setContextMenu(null)} onContextMenu={(event) => event.preventDefault()}>
          <div
            className="studio-project-context-menu"
            role="menu"
            aria-label={`${contextMenu.project.display_name} 项目操作`}
            style={{ left: contextMenu.left, top: contextMenu.top }}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              autoFocus
              onClick={() => {
                setDeleteFiles(false);
                setDeleteCandidate(contextMenu.project);
                setContextMenu(null);
              }}
            >删除…</button>
          </div>
        </div>
      )}
      {deleteCandidate && (
        <div className="studio-popover-backdrop" role="presentation" onMouseDown={() => !deleteBusy && setDeleteCandidate(null)}>
          <section className="studio-project-trash-dialog" role="alertdialog" aria-modal="true" aria-labelledby="project-trash-title" onMouseDown={(event) => event.stopPropagation()}>
            <h2 id="project-trash-title">删除“{deleteCandidate.display_name}”？</h2>
            <p>默认只从项目库移除，项目文件仍保留在原位置。</p>
            {deleteCandidate.exists && deleteCandidate.project_id && (
              <label className="studio-project-trash-option">
                <input
                  type="checkbox"
                  checked={deleteFiles}
                  disabled={trashDisabled}
                  onChange={(event) => setDeleteFiles(event.target.checked)}
                />
                <span>同时将项目文件夹移到废纸篓</span>
              </label>
            )}
            {trashDisabled && deleteCandidate.exists && (
              <small>后台任务完成后才能移动项目文件夹。</small>
            )}
            {deleteFiles && (
              <p>项目文件夹、转录和编辑记录会移到 macOS 废纸篓；外部原始视频不会被删除。</p>
            )}
            <div>
              <button type="button" disabled={deleteBusy} onClick={() => setDeleteCandidate(null)}>取消</button>
              <button
                type="button"
                className="is-danger"
                disabled={deleteBusy}
                onClick={async () => {
                  setDeleteBusy(true);
                  const removed = deleteFiles
                    ? await onTrash(deleteCandidate)
                    : await onRemoveRecent(deleteCandidate);
                  setDeleteBusy(false);
                  if (removed) setDeleteCandidate(null);
                }}
              >{deleteBusy ? "正在处理…" : deleteFiles ? "移到废纸篓" : "从项目库删除"}</button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
