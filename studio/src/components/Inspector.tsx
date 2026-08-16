import type { Diagnostic } from '../../../bindings/Diagnostic'
import type { FixtureClip, FixtureSet } from '../fixtures'
import { ratingLabel, statusLabel } from '../utils'

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="w-full rounded-md border border-line bg-card p-3 flex flex-col gap-2">
      <h3 className="text-sm font-semibold">{title}</h3>
      {children}
    </section>
  )
}

function KvRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-16 flex-none text-xs text-mutedfg">{label}</span>
      <span className={`flex-1 min-w-0 truncate text-xs ${mono ? 'font-mono' : ''}`}>{value}</span>
    </div>
  )
}

function SourceRow({ label, xml, csv }: { label: string; xml: string; csv: string }) {
  const matched = xml === csv || csv.endsWith('（采用）') || xml === csv.replace(/（采用）$/, '')
  return (
    <div className="flex items-center gap-2">
      <span className="w-16 flex-none text-xs text-mutedfg">{label}</span>
      <span className="w-16 flex-none text-xs">{xml}</span>
      <span className={`flex-1 text-xs ${matched ? 'text-fg' : 'text-warning'}`}>{csv}</span>
    </div>
  )
}

const DIAGNOSTIC_DOT: Record<Diagnostic['level'], string> = {
  error: 'bg-danger',
  warning: 'bg-warning',
  info: 'bg-info',
}

interface InspectorProps {
  fixtures: FixtureSet
  clip: FixtureClip
  onNotice: (message: string) => void
  onExport: () => void
}

export function Inspector({ fixtures, clip, onNotice, onExport }: InspectorProps) {
  const clipDiagnostics = fixtures.diagnostics.filter((d) => d.object_id === clip.id)
  const blocking = fixtures.diagnostics.filter((d) => d.blocks_export)
  const blockedTargets = blocking
    .map((d) => d.object_id)
    .filter(Boolean)
    .join('、')

  return (
    <aside className="w-80 flex-none h-full overflow-y-auto border-l border-line p-3 flex flex-col gap-3">
      <Card title="片段元数据">
        <div className="flex flex-col gap-1">
          <KvRow label="新名称" value={clip.newName} mono />
          <KvRow label="源文件" value={clip.sourceName} mono />
          <KvRow label="场景" value={clip.scene} />
          <KvRow label="镜号" value={clip.shot} />
          <KvRow label="镜次" value={clip.take} />
          <KvRow label="机位" value={clip.camera} />
          <KvRow label="评分" value={ratingLabel(clip.rating)} />
          <KvRow label="入点" value={clip.tcIn} mono />
          <KvRow label="时长" value={clip.duration} mono />
          <KvRow label="来源" value={clip.fromCsv ? 'XML + CSV 场记单' : '仅 XML'} />
          <KvRow label="状态" value={statusLabel(clip.status)} />
        </div>
      </Card>

      <Card title="来源对照（XML ｜ CSV）">
        <div className="flex flex-col gap-1">
          <SourceRow label="场景" xml={clip.scene} csv={clip.fromCsv ? clip.scene : '—'} />
          <SourceRow label="Episode" xml="—" csv={clip.fromCsv ? '02（采用）' : '—'} />
          <SourceRow
            label="评分"
            xml={ratingLabel(clip.rating)}
            csv={clip.fromCsv ? ratingLabel(clip.rating) : '—'}
          />
          {!clip.fromCsv ? (
            <div className="text-xs text-warning">⚠ 该片段未匹配 CSV，按无 CSV 格式命名</div>
          ) : (
            clip.note === '' && <div className="text-xs text-mutedfg">XML 与 CSV 取值一致</div>
          )}
        </div>
      </Card>

      <Card title="诊断">
        {clipDiagnostics.length === 0 ? (
          <div className="text-xs text-success">✓ 该片段无诊断</div>
        ) : (
          <div className="flex flex-col gap-2">
            {clipDiagnostics.map((d) => (
              <div key={d.code} className="flex flex-col gap-0.5">
                <div className="flex items-center gap-1">
                  <span className={`w-1.5 h-1.5 rounded-full ${DIAGNOSTIC_DOT[d.level]}`} />
                  <span className="text-xs font-semibold">{d.code}</span>
                  {d.blocks_export && (
                    <span className="px-1 rounded-sm bg-danger/15 text-xs text-danger">
                      ⛔ 阻断导出
                    </span>
                  )}
                </div>
                <div className="text-xs text-mutedfg">{d.cause}</div>
              </div>
            ))}
          </div>
        )}
      </Card>

      <Card title="项目操作">
        <div className="flex flex-col gap-2">
          <button
            type="button"
            onClick={() => onNotice('重新预演属后续迭代（需接入真实 Engine 操作）')}
            className="h-8 rounded-md border border-line text-sm hover:bg-sidebaraccent"
          >
            重新预演
          </button>
          <button
            type="button"
            onClick={onExport}
            className="h-8 rounded-md bg-love hover:bg-love/85 flex items-center justify-center text-sm font-semibold text-white"
          >
            导出 Premiere XML
          </button>
          {blocking.length > 0 && (
            <div className="text-xs text-danger">
              ⛔ 导出被 {blocking.length} 条错误诊断阻断，需先在诊断中处理 {blockedTargets}
            </div>
          )}
          <div className="text-xs text-mutedfg">以上动作用于整个项目</div>
        </div>
      </Card>

      <Card title="版本历史">
        <div className="flex flex-col gap-1.5">
          {fixtures.revisions.map((entry) => (
            <div key={entry.revision} className="flex flex-col gap-0.5">
              <div className="flex justify-between">
                <span className="text-xs font-semibold">
                  r{entry.revision} · {entry.operation}
                </span>
                <span className="text-xs text-mutedfg">{entry.committedAt}</span>
              </div>
              <div className="text-xs text-mutedfg">{entry.summary}</div>
            </div>
          ))}
        </div>
      </Card>
    </aside>
  )
}
