import type { FixtureSet } from '../fixtures'

export function StatusBar({ fixtures }: { fixtures: FixtureSet }) {
  const { counts, diagnostics, revisions, episodeLabel, csvSummary } = fixtures
  const errors = diagnostics.filter((d) => d.level === 'error').length
  const warnings = diagnostics.filter((d) => d.level === 'warning').length
  const infos = diagnostics.filter((d) => d.level === 'info').length

  return (
    <footer className="h-7 flex-none px-3 flex items-center justify-between border-t border-line text-xs text-mutedfg">
      <div className="flex items-center gap-2">
        <span className="text-danger">⛔ {errors} 错误</span>
        <span className="text-warning">{warnings} 警告</span>
        <span>{infos} 提示</span>
        <span>
          ｜共 {counts.total} 片段：{counts.processed} 处理 · {counts.ignored} 忽略 ·{' '}
          {counts.skipped} 跳过 · {counts.failed} 失败
        </span>
      </div>
      <span>
        {episodeLabel} ｜ {csvSummary} ｜ rev {revisions[0]?.revision ?? 0}
      </span>
    </footer>
  )
}
