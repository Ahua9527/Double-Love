import type { MediaAssetSummary } from '../../../bindings/MediaAssetSummary'
import { assetStatusLabel, formatClock, frameRateLabel, num } from '../utils'

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

interface InspectorProps {
  asset: MediaAssetSummary | null
}

export function Inspector({ asset }: InspectorProps) {
  if (!asset) {
    return (
      <aside className="w-80 flex-none h-full overflow-y-auto border-l border-line p-3">
        <Card title="资产信息">
          <div className="text-xs text-mutedfg">选择左侧资产后显示详情</div>
        </Card>
      </aside>
    )
  }
  const durationSec = num(asset.duration_samples) / num(asset.audio_sample_rate)
  return (
    <aside className="w-80 flex-none h-full overflow-y-auto border-l border-line p-3 flex flex-col gap-3">
      <Card title="资产信息">
        <div className="flex flex-col gap-1">
          <KvRow label="文件名" value={asset.display_name} mono />
          <KvRow
            label="时长"
            value={`${formatClock(durationSec)} · ${frameRateLabel(asset.rate)}`}
          />
          <KvRow
            label="分辨率"
            value={
              asset.width !== null && asset.height !== null
                ? `${num(asset.width)}×${num(asset.height)}`
                : '未知'
            }
          />
          <KvRow
            label="音频"
            value={`${num(asset.audio_sample_rate)} Hz · ${
              asset.audio_channels !== null ? `${num(asset.audio_channels)} 声道` : '未知声道'
            }`}
          />
          <KvRow label="状态" value={assetStatusLabel(asset.status)} />
        </div>
      </Card>
    </aside>
  )
}
