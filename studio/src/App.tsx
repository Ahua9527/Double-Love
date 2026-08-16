import { useCallback, useEffect, useRef, useState } from 'react'
import type { ExportOutcome } from '../../bindings/ExportOutcome'
import type { MediaAssetSummary } from '../../bindings/MediaAssetSummary'
import type { OperationResult } from '../../bindings/OperationResult'
import type { ProgressEvent } from '../../bindings/ProgressEvent'
import type { ProjectSummary } from '../../bindings/ProjectSummary'
import type { TaskState } from '../../bindings/TaskState'
import type { TranscriptViewData } from '../../bindings/TranscriptViewData'
import * as api from './tauri'
import {
  clampSeconds,
  loadPanelState,
  num,
  omitRangesToSeconds,
  playheadClock,
  savePanelState,
  type PanelState,
} from './utils'
import { TitleBar } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { PreviewHero } from './components/PreviewHero'
import { Transport } from './components/Transport'
import { Inspector } from './components/Inspector'
import { Timeline } from './components/Timeline'
import { StatusBar } from './components/StatusBar'
import { TranscriptView, type TranscriptionProgress } from './components/TranscriptView'
import { ExportPreviewDialog } from './components/ExportPreviewDialog'

interface RunningTask extends TranscriptionProgress {
  id: string
  assetId: string
}

export default function App() {
  const [project, setProject] = useState<ProjectSummary | null>(null)
  const [assets, setAssets] = useState<MediaAssetSummary[]>([])
  const [currentId, setCurrentId] = useState<string | null>(null)
  const [view, setView] = useState<TranscriptViewData | null>(null)
  const [playheadSec, setPlayheadSec] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [task, setTask] = useState<RunningTask | null>(null)
  const [exportPreview, setExportPreview] = useState<OperationResult<ExportOutcome> | null>(null)
  const [exportBusy, setExportBusy] = useState(false)
  // 面板收起状态（左侧栏/检查器/时间线），重启后保持
  const [panels, setPanels] = useState<PanelState>(() => loadPanelState(window.localStorage))
  const videoRef = useRef<HTMLVideoElement>(null)
  const taskRef = useRef<RunningTask | null>(null)
  taskRef.current = task

  const asset = assets.find((candidate) => candidate.id === currentId) ?? null
  const durationSec = asset ? num(asset.duration_samples) / num(asset.audio_sample_rate) : 0
  const sampleRate = asset ? num(asset.audio_sample_rate) : 0

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

  const refreshAssets = useCallback(async (selectId?: string) => {
    const result = await api.assetsList()
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '读取资产列表失败')
      return
    }
    const list = result.data ?? []
    setAssets(list)
    if (selectId) {
      setCurrentId(selectId)
    } else {
      setCurrentId((previous) =>
        previous && list.some((candidate) => candidate.id === previous)
          ? previous
          : (list[0]?.id ?? null),
      )
    }
  }, [])

  const refreshView = useCallback(async (assetId: string) => {
    const result = await api.transcriptGet(assetId)
    setView(result.status === 'failed' ? null : result.data)
  }, [])

  // 转录任务事件：进度更新任务条；终态清理并刷新资产状态与文本
  useEffect(() => {
    if (!api.isTauri) return
    let cancelled = false
    const unlisteners: Array<() => void> = []
    import('@tauri-apps/api/event')
      .then(({ listen }) => {
        if (cancelled) return
        void listen<ProgressEvent>('dl://progress', (event) => {
          const current = taskRef.current
          if (!current || current.id !== event.payload.task) return
          setTask({
            ...current,
            completed:
              event.payload.completed === null ? current.completed : num(event.payload.completed),
            total: event.payload.total === null ? current.total : num(event.payload.total),
            message: event.payload.message,
          })
        }).then((fn) => unlisteners.push(fn))
        void listen<{ task_id: string; state: TaskState }>('dl://task-state', (event) => {
          const current = taskRef.current
          if (!current || current.id !== event.payload.task_id) return
          setTask(null)
          void refreshAssets(current.assetId)
          void refreshView(current.assetId)
          if (event.payload.state === 'succeeded') setNotice('转录完成')
          else if (event.payload.state === 'partial') setNotice('转录完成，但有部分错误（详见日志）')
          else if (event.payload.state === 'cancelled') setNotice('转录已取消，已产出的文本保留')
          else setNotice('转录失败，详见 .doublelove/logs 日志')
        }).then((fn) => unlisteners.push(fn))
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
      unlisteners.forEach((fn) => fn())
    }
  }, [refreshAssets, refreshView])

  const openProject = async (create: boolean) => {
    if (!api.isTauri) {
      setNotice('打开项目需要在 Double Love Studio 桌面应用中运行')
      return
    }
    const picked = await api.pickDirectory(create ? '选择新项目所在目录' : '选择项目目录')
    if (!picked) return // 用户取消
    const result = create ? await api.projectCreate(picked) : await api.projectOpen(picked)
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '打开项目失败')
      return
    }
    setProject(result.data)
    setAssets([])
    setCurrentId(null)
    setView(null)
    setPlayheadSec(0)
    setPlaying(false)
    setNotice(null)
    await refreshAssets()
  }

  const importMedia = async () => {
    if (!api.isTauri) {
      setNotice('导入媒体需要在 Double Love Studio 桌面应用中运行')
      return
    }
    const picked = await api.pickMediaFile()
    if (!picked || Array.isArray(picked)) return // 用户取消
    const result = await api.importMedia(picked)
    if (result.status === 'failed' || !result.data) {
      const diagnostic = result.diagnostics[0]
      setNotice(
        diagnostic
          ? `${diagnostic.cause}${diagnostic.suggested_action ? `（${diagnostic.suggested_action}）` : ''}`
          : '导入失败',
      )
      return
    }
    await refreshAssets(result.data.id)
    setNotice(
      result.diagnostics.some((d) => d.code === 'MEDIA_ALREADY_IMPORTED')
        ? '该文件之前导入过，已选中现有资产'
        : `已导入 ${result.data.display_name}`,
    )
  }

  // 选中资产 → 拉转录视图（omit 红条数据；转录文本界面属下一步）
  useEffect(() => {
    setPlayheadSec(0)
    setPlaying(false)
    if (!currentId || !api.isTauri) {
      setView(null)
      return
    }
    let cancelled = false
    api
      .transcriptGet(currentId)
      .then((result) => {
        if (cancelled) return
        setView(result.status === 'failed' ? null : result.data)
      })
      .catch(() => {
        if (!cancelled) setView(null)
      })
    return () => {
      cancelled = true
    }
  }, [currentId])

  const seek = (seconds: number) => {
    const target = clampSeconds(seconds, durationSec)
    if (videoRef.current) videoRef.current.currentTime = target
    setPlayheadSec(target)
  }

  const togglePlay = () => {
    const video = videoRef.current
    if (!video) return
    if (video.paused) {
      void video.play()
    } else {
      video.pause()
    }
  }

  const startTranscription = async () => {
    if (!asset) return
    const result = await api.transcribeStart(asset.id, 'qwen3-asr-1.7b', 'auto')
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '无法启动转录')
      return
    }
    setTask({
      id: result.data.task_id,
      assetId: asset.id,
      completed: null,
      total: null,
      message: '正在加载模型…',
    })
  }

  const cancelTranscription = async () => {
    if (!task) return
    const result = await api.taskCancel(task.id)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '取消失败')
    // 终态事件到达后统一清理任务条
  }

  const handleOmit = async (startOrdinal: number, endOrdinal: number) => {
    if (!asset) return
    const result = await api.editOmit(asset.id, startOrdinal, endOrdinal)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '删除失败')
      return
    }
    await refreshView(asset.id)
  }

  const handleRestore = async (operationId: string, startOrdinal: number, endOrdinal: number) => {
    const result = await api.editRestore(operationId, startOrdinal, endOrdinal)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '恢复失败')
      return
    }
    if (asset) await refreshView(asset.id)
  }

  // 导出：preview（不落盘）→ 摘要对话框 → 保存位置 → apply
  const handleExport = async () => {
    if (!asset) return
    const result = await api.roughcutPreview(asset.id)
    setExportPreview(result)
  }

  const confirmExport = async () => {
    if (!asset || !exportPreview) return
    const stem = asset.display_name.replace(/\.[^.]+$/, '')
    const target = await api.pickSavePath(`${stem}_ROUGH_CUT.xml`)
    if (!target) return // 用户取消保存对话框
    setExportBusy(true)
    const result = await api.exportRoughcutApply(asset.id, target)
    setExportBusy(false)
    setExportPreview(null)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '导出失败')
      return
    }
    setNotice(`已导出：${result.data?.artifact_path ?? target}`)
  }

  const omitRanges = view ? omitRangesToSeconds(view.words, view.omits, sampleRate) : []

  return (
    <div className="h-full flex flex-col bg-surface text-fg">
      <TitleBar
        projectName={project ? (project.root.split('/').filter(Boolean).pop() ?? null) : null}
        panels={panels}
        onToggle={togglePanel}
        onImport={importMedia}
        onExport={handleExport}
        importDisabled={!project || !api.isTauri}
        exportDisabled={!asset || !api.isTauri}
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
          {panels.left && (
            <Sidebar
              project={project}
              assets={assets}
              currentId={currentId}
              onSelect={setCurrentId}
              onImport={importMedia}
            />
          )}
        </div>
        <main className="flex-1 min-w-0 h-full flex flex-col">
          {!project ? (
            <div className="flex-1 flex flex-col items-center justify-center gap-3">
              <div className="text-sm font-semibold">从本地项目开始</div>
              <div className="text-xs text-mutedfg max-w-72 text-center">
                项目是一个本地文件夹，保存转录文本与剪辑记录；原始媒体只读引用，不会被修改。
              </div>
              <div className="flex items-center gap-2 mt-1">
                <button
                  type="button"
                  onClick={() => void openProject(false)}
                  className="h-8 px-4 rounded-md bg-selected hover:bg-selected/85 text-sm font-semibold text-white"
                >
                  打开项目…
                </button>
                <button
                  type="button"
                  onClick={() => void openProject(true)}
                  className="h-8 px-4 rounded-md border border-line text-sm hover:bg-sidebaraccent"
                >
                  新建项目…
                </button>
              </div>
            </div>
          ) : (
            <>
              <PreviewHero
                src={currentId ? `media://localhost/${currentId}` : null}
                label={asset?.display_name ?? null}
                videoRef={videoRef}
                onTimeUpdate={setPlayheadSec}
                onPlayState={setPlaying}
              />
              <Transport
                playing={playing}
                clock={playheadClock(playheadSec, durationSec)}
                disabled={!asset}
                onTogglePlay={togglePlay}
                onSkip={(delta) => seek(playheadSec + delta)}
              />
              {asset ? (
                <TranscriptView
                  asset={asset}
                  view={view}
                  playheadSec={playheadSec}
                  transcription={task && task.assetId === asset.id ? task : null}
                  onSeek={seek}
                  onOmit={handleOmit}
                  onRestore={handleRestore}
                  onTranscribeStart={startTranscription}
                  onTranscribeCancel={cancelTranscription}
                />
              ) : (
                <div className="flex-1 min-h-0 mx-3 mb-2 rounded-md border border-line flex items-center justify-center">
                  <span className="text-xs text-mutedfg">选择或导入媒体后，在这里按文本剪辑</span>
                </div>
              )}
            </>
          )}
        </main>
        <div
          className={`flex-none overflow-hidden transition-[width] duration-200 ${
            panels.right ? 'w-80' : 'w-0'
          }`}
        >
          {panels.right && <Inspector asset={asset} />}
        </div>
      </div>
      <div
        className={`flex-none overflow-hidden transition-[height] duration-200 ${
          panels.bottom ? 'h-32' : 'h-0'
        }`}
      >
        {panels.bottom && (
          <Timeline
            durationSec={durationSec}
            playheadSec={playheadSec}
            omitRanges={omitRanges}
            onSeek={seek}
          />
        )}
      </div>
      <StatusBar project={project} assetCount={assets.length} asset={asset} />
      {exportPreview && (
        <ExportPreviewDialog
          result={exportPreview}
          busy={exportBusy}
          onConfirm={confirmExport}
          onCancel={() => setExportPreview(null)}
        />
      )}
    </div>
  )
}
