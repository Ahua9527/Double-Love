import type { FixtureSet } from '../fixtures'

interface TitleBarProps {
  fixtures: FixtureSet
  onImport: () => void
  onExport: () => void
}

export function TitleBar({ fixtures, onImport, onExport }: TitleBarProps) {
  return (
    <header className="h-10 flex-none flex items-center justify-between pr-3 border-b border-line">
      <div className="flex items-center gap-2 px-3">
        <span className="w-2 h-2 flex-none rounded-full bg-love" />
        <span className="text-sm font-semibold">Double Love Studio</span>
        <span className="text-sm text-mutedfg">{fixtures.projectName}</span>
        <span className="h-5 px-1.5 rounded-sm bg-love/15 text-xs leading-5 text-love">
          {fixtures.episodeLabel}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <div className="w-44 h-7 px-2 rounded-md bg-sidebaraccent text-xs leading-7 text-mutedfg select-none">
          搜索片段…
        </div>
        <button
          type="button"
          onClick={onImport}
          className="h-7 px-3 rounded-md border border-line text-xs hover:bg-sidebaraccent"
        >
          导入…
        </button>
        <button
          type="button"
          onClick={onExport}
          className="h-7 px-3 rounded-md bg-love hover:bg-love/85 text-xs font-semibold text-white"
        >
          导出 Premiere XML
        </button>
      </div>
    </header>
  )
}
