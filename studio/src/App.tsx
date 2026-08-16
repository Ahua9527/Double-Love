import { useEffect, useState } from 'react'
import { aozoraDiary } from './fixtures'
import { exportBlockMessage, playheadClock } from './utils'
import { TitleBar } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { PreviewHero } from './components/PreviewHero'
import { Transport } from './components/Transport'
import { ScrubStrip } from './components/ScrubStrip'
import { ClipTable } from './components/ClipTable'
import { Inspector } from './components/Inspector'
import { Timeline } from './components/Timeline'
import { StatusBar } from './components/StatusBar'

const fixtures = aozoraDiary

export default function App() {
  const [selected, setSelected] = useState(0)
  const [playhead, setPlayhead] = useState(0.35)
  const [notice, setNotice] = useState<string | null>(null)

  // 主题跟随系统：index.html 已设首帧，这里接管后续切换
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => document.documentElement.classList.toggle('dark', media.matches)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [])

  // Finder 拖入：仅在 Tauri 壳内有效；只记录数量并提示，不解析（真实解析属后续迭代）
  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) => {
        if (cancelled) return
        void getCurrentWebview()
          .onDragDropEvent((event) => {
            if (event.payload.type !== 'drop') return
            const media = event.payload.paths.filter((p) => /\.(xml|csv)$/i.test(p))
            if (media.length > 0) {
              setNotice(`已收到 ${media.length} 个拖入文件（仅记录数量，解析属后续迭代）`)
            }
          })
          .then((fn) => {
            if (cancelled) fn()
            else unlisten = fn
          })
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const selectClip = (index: number) => {
    setSelected(index)
    // 原型联动：选中行把播放头带到示意位置，接真实数据时移除
    setPlayhead((index + 0.5) / fixtures.clips.length)
  }

  const handleExport = () => {
    setNotice(exportBlockMessage(fixtures.diagnostics) ?? '导出预演属后续迭代')
  }

  const clip = fixtures.clips[selected]

  return (
    <div className="h-full flex flex-col bg-surface text-fg">
      <TitleBar
        fixtures={fixtures}
        onImport={() => setNotice('导入向导将在后续迭代接入真实解析')}
        onExport={handleExport}
      />
      {notice && (
        <div className="h-7 flex-none px-3 flex items-center bg-info/10 border-b border-line text-xs">
          ℹ︎ {notice}
        </div>
      )}
      <div className="flex-1 min-h-0 flex">
        <Sidebar fixtures={fixtures} />
        <main className="flex-1 min-w-0 h-full flex flex-col">
          <PreviewHero clip={clip} />
          <Transport clock={playheadClock(playhead)} onNotice={setNotice} />
          <ScrubStrip playhead={playhead} />
          <ClipTable clips={fixtures.clips} selected={selected} onSelect={selectClip} />
        </main>
        <Inspector fixtures={fixtures} clip={clip} onNotice={setNotice} onExport={handleExport} />
      </div>
      <Timeline playhead={playhead} onSeek={setPlayhead} />
      <StatusBar fixtures={fixtures} />
    </div>
  )
}
