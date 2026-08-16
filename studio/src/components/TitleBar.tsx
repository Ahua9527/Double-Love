import { PanelBottom, PanelLeft, PanelRight } from 'lucide-react'
import type { PanelState } from '../utils'

interface TitleBarProps {
  /** 当前项目名（未打开项目时为 null） */
  projectName: string | null
  panels: PanelState
  onToggle: (key: keyof PanelState) => void
  onImport: () => void
  onExport: () => void
  /** 无可用资产时禁用导入/导出 */
  importDisabled: boolean
  exportDisabled: boolean
}

function PanelToggle({
  label,
  pressed,
  onClick,
  children,
}: {
  label: string
  pressed: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      onClick={onClick}
      className={`w-7 h-7 rounded-md flex items-center justify-center ${
        pressed ? 'bg-sidebaraccent text-selected' : 'text-mutedfg hover:text-fg'
      }`}
    >
      {children}
    </button>
  )
}

export function TitleBar({
  projectName,
  panels,
  onToggle,
  onImport,
  onExport,
  importDisabled,
  exportDisabled,
}: TitleBarProps) {
  return (
    <header className="h-10 flex-none flex items-center justify-between pr-3 border-b border-line">
      <div className="flex items-center gap-2 pl-1.5 pr-3">
        <PanelToggle label="切换左侧栏" pressed={panels.left} onClick={() => onToggle('left')}>
          <PanelLeft size={15} />
        </PanelToggle>
        <span className="w-2 h-2 flex-none rounded-full bg-love" />
        <span className="text-sm font-semibold">Double Love Studio</span>
        {projectName && <span className="text-sm text-mutedfg truncate max-w-64">{projectName}</span>}
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onImport}
          disabled={importDisabled}
          className="h-7 px-3 rounded-md border border-line text-xs hover:bg-sidebaraccent disabled:opacity-40 disabled:hover:bg-transparent"
        >
          导入…
        </button>
        <button
          type="button"
          onClick={onExport}
          disabled={exportDisabled}
          className="h-7 px-3 rounded-md bg-love hover:bg-love/85 text-xs font-semibold text-white disabled:opacity-40 disabled:hover:bg-love"
        >
          导出 Premiere XML
        </button>
        <PanelToggle label="切换时间线" pressed={panels.bottom} onClick={() => onToggle('bottom')}>
          <PanelBottom size={15} />
        </PanelToggle>
        <PanelToggle label="切换检查器" pressed={panels.right} onClick={() => onToggle('right')}>
          <PanelRight size={15} />
        </PanelToggle>
      </div>
    </header>
  )
}
