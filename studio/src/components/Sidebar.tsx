import type { FixtureSet } from '../fixtures'

interface Collection {
  label: string
  count: number
  dot: string
  active?: boolean
}

function Section({ title }: { title: string }) {
  return <div className="text-xs font-semibold text-mutedfg">{title}</div>
}

function TreeRow({ label, count }: { label: string; count?: number }) {
  return (
    <div className="h-6 px-2 flex items-center justify-between rounded-sm text-sm">
      <span>{label}</span>
      {count !== undefined && <span className="text-xs text-mutedfg">{count}</span>}
    </div>
  )
}

export function Sidebar({ fixtures }: { fixtures: FixtureSet }) {
  const { counts, csvUnmatched, clips } = fixtures
  const collections: Collection[] = [
    { label: '全部片段', count: counts.total, dot: 'bg-fg', active: true },
    { label: '已处理', count: counts.processed, dot: 'bg-success' },
    { label: '已忽略', count: counts.ignored, dot: 'bg-mutedfg' },
    { label: '已跳过', count: counts.skipped, dot: 'bg-warning' },
    { label: '失败', count: counts.failed, dot: 'bg-danger' },
    { label: 'CSV 未匹配', count: csvUnmatched, dot: 'bg-csvpurple' },
  ]

  return (
    <nav className="w-52 flex-none h-full bg-sidebar border-r border-sidebarline p-3 flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <Section title="项目" />
        <TreeRow label="▾ 青空日记" />
        <TreeRow label="　素材库" count={clips.length} />
        <TreeRow label="　序列" count={3} />
        <TreeRow label="　导出" count={1} />
      </div>
      <div className="flex flex-col gap-1">
        <Section title="智能集合" />
        {collections.map((c) => (
          <div
            key={c.label}
            className={`h-6 px-2 flex items-center gap-2 rounded-sm text-sm ${
              c.active ? 'bg-selected/15 text-selected' : ''
            }`}
          >
            <span className={`w-1.5 h-1.5 flex-none rounded-full ${c.dot}`} />
            <span className="flex-1">{c.label}</span>
            <span className={`text-xs ${c.active ? 'text-selected' : 'text-mutedfg'}`}>
              {c.count}
            </span>
          </div>
        ))}
      </div>
    </nav>
  )
}
