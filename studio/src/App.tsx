import { useEffect, useState } from 'react'
import { aozoraDiary } from './fixtures'
import {
  exportBlockMessage,
  loadPanelState,
  playheadClock,
  savePanelState,
  type PanelState,
} from './utils'
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
  // 面板收起状态（左侧栏/检查器/时间线），重启后保持
  const [panels, setPanels] = useState<PanelState>(() => loadPanelState(window.localStorage))

  useEffect(() => {
    savePanelState(window.localStorage, panels)
  }, [panels])

  const togglePanel = (key: keyof PanelState) => {
    setPanels((prev) => ({ ...prev, [key]: !prev[key] }))
  }

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
        panels={panels}
        onToggle={togglePanel}
        onImport={() => setNotice('导入向导将在后续迭代接入真实解析')}
        onExport={handleExport}
      />
      {notice && (
        <div className="h-7 flex-none px-3 flex items-center bg-info/10 border-b border-line text-xs">
          ℹ︎ {notice}
        </div>
      )}
      <div className="flex-1 min-h-0 flex">
        {/* 抽屉容器：宽/高动画缩到 0；收起时内容同步卸载，DOM 不留痕 */}
        <div
          className={`flex-none overflow-hidden transition-[width] duration-200 ${
            panels.left ? 'w-52' : 'w-0'
          }`}
        >
          {panels.left && <Sidebar fixtures={fixtures} />}
        </div>
        <main className="flex-1 min-w-0 h-full flex flex-col">
          <PreviewHero clip={clip} />
          <Transport clock={playheadClock(playhead)} onNotice={setNotice} />
          <ScrubStrip playhead={playhead} />
          <ClipTable clips={fixtures.clips} selected={selected} onSelect={selectClip} />
        </main>
        <div
          className={`flex-none overflow-hidden transition-[width] duration-200 ${
            panels.right ? 'w-80' : 'w-0'
          }`}
        >
          {panels.right && (
            <Inspector fixtures={fixtures} clip={clip} onNotice={setNotice} onExport={handleExport} />
          )}
        </div>
      </div>
      <div
        className={`flex-none overflow-hidden transition-[height] duration-200 ${
          panels.bottom ? 'h-32' : 'h-0'
        }`}
      >
        {panels.bottom && <Timeline playhead={playhead} onSeek={setPlayhead} />}
      </div>
      <StatusBar fixtures={fixtures} />
    </div>
  )
}
