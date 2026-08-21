import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import './components/editor-overlays.css'
import { Activity, Captions, Clapperboard, Mic2, Palette, Wand2 } from 'lucide-react'
import type { CanvasSpec } from '../../bindings/CanvasSpec'
import type { FrameRate } from '../../bindings/FrameRate'
import type { MainTrackClip } from '../../bindings/MainTrackClip'
import type { MediaAssetSummary } from '../../bindings/MediaAssetSummary'
import type { OperationResult } from '../../bindings/OperationResult'
import type { ProgressEvent } from '../../bindings/ProgressEvent'
import type { ProjectExportPreview } from '../../bindings/ProjectExportPreview'
import type { ProjectSummary } from '../../bindings/ProjectSummary'
import type { RevisionHistoryEntry } from '../../bindings/RevisionHistoryEntry'
import type { SpeakerIdentity } from '../../bindings/SpeakerIdentity'
import type { SpeakerMergeProposal } from '../../bindings/SpeakerMergeProposal'
import type { SpeakerNameAgentPayload } from '../../bindings/SpeakerNameAgentPayload'
import type { SpeakerNameProposal } from '../../bindings/SpeakerNameProposal'
import type { SubtitleStyle } from '../../bindings/SubtitleStyle'
import type { TaskState } from '../../bindings/TaskState'
import type { TimelineIRv2 } from '../../bindings/TimelineIRv2'
import type { TranscriptViewData } from '../../bindings/TranscriptViewData'
import * as api from './tauri'
import { frameRateFps, num, playheadClock } from './utils'
import { MainTrackTimeline } from './components/MainTrackTimeline'
import { MediaDrawer } from './components/MediaDrawer'
import { ProjectExportDialog } from './components/ProjectExportDialog'
import { ProjectInfoDialog } from './components/ProjectInfoDialog'
import { ProjectLibrary } from './components/ProjectLibrary'
import { ProjectSettings } from './components/ProjectSettings'
import { ModelInstallDialog } from './components/ModelInstallDialog'
import { Onboarding } from './components/Onboarding'
import { Sidebar, type StudioScreen } from './components/Sidebar'
import { TitleBar } from './components/TitleBar'
import { TranscriptView, type TranscriptionProgress } from './components/TranscriptView'
import { TimelinePreview } from './components/TimelinePreview'
import { Transport } from './components/Transport'

type EditorTab = 'transcript' | 'subtitles' | 'speakers'
type TaskKind = 'transcribe' | 'speaker'
type ThemeMode = api.ThemeMode

const FALLBACK_MODEL: api.ModelDescriptor = {
  id: 'qwen3-asr-0.6b',
  label: 'Qwen3 ASR · 0.6B',
  kind: 'asr',
  revision: 'managed-by-desktop',
  size_bytes: 0,
  memory_bytes: 0,
  license: 'Apache-2.0',
  dependencies: [{ model_id: 'qwen3-forced-aligner-0.6b', required: true, reason: '逐词时间锚点' }],
  state: 'not_installed',
}

interface RunningTask extends TranscriptionProgress {
  id: string
  assetId: string
  kind: TaskKind
}

function projectName(project: ProjectSummary | null): string | null {
  return project?.root.split('/').filter(Boolean).pop() ?? null
}

function shortProjectName(project: ProjectSummary | null): string {
  return projectName(project) ?? 'Double Love'
}

export default function App() {
  const [project, setProject] = useState<ProjectSummary | null>(null)
  const [screen, setScreen] = useState<StudioScreen>('library')
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const saved = window.localStorage.getItem('studio.theme')
    return saved === 'dark' || saved === 'system' ? saved : 'light'
  })
  const [sidebarVisible, setSidebarVisible] = useState(true)
  const [assets, setAssets] = useState<MediaAssetSummary[]>([])
  const [mainTrack, setMainTrack] = useState<MainTrackClip[]>([])
  const [timeline, setTimeline] = useState<TimelineIRv2 | null>(null)
  const [timelinePreview, setTimelinePreview] = useState<ProjectExportPreview | null>(null)
  const [currentId, setCurrentId] = useState<string | null>(null)
  const [selectedClipId, setSelectedClipId] = useState<string | null>(null)
  const [transcript, setTranscript] = useState<TranscriptViewData | null>(null)
  const [canvas, setCanvas] = useState<CanvasSpec | null>(null)
  const [outputRate, setOutputRate] = useState<FrameRate | null>(null)
  const [subtitleStyle, setSubtitleStyle] = useState<SubtitleStyle | null>(null)
  const [history, setHistory] = useState<RevisionHistoryEntry[]>([])
  const [loadedRevision, setLoadedRevision] = useState<number | null>(null)
  const [speakers, setSpeakers] = useState<SpeakerIdentity[]>([])
  const [nameProposals, setNameProposals] = useState<SpeakerNameProposal[]>([])
  const [mergeProposals, setMergeProposals] = useState<SpeakerMergeProposal[]>([])
  const [agentPayload, setAgentPayload] = useState<SpeakerNameAgentPayload | null>(null)
  const [renamingSpeaker, setRenamingSpeaker] = useState<SpeakerIdentity | null>(null)
  const [speakerNameDraft, setSpeakerNameDraft] = useState('')
  const [editorTab, setEditorTab] = useState<EditorTab>('transcript')
  const [playheadSec, setPlayheadSec] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [task, setTask] = useState<RunningTask | null>(null)
  const [mediaDrawerOpen, setMediaDrawerOpen] = useState(false)
  const [busyAssetId, setBusyAssetId] = useState<string | null>(null)
  const [canvasMenuOpen, setCanvasMenuOpen] = useState(false)
  const [exportPreview, setExportPreview] = useState<OperationResult<ProjectExportPreview> | null>(null)
  const [exportBusy, setExportBusy] = useState(false)
  const [preferences, setPreferences] = useState<api.AppPreferencesV1 | null>(null)
  const [recentProjects, setRecentProjects] = useState<api.RecentProject[]>([])
  const [models, setModels] = useState<api.ModelDescriptor[]>([])
  const [systemProfile, setSystemProfile] = useState<api.SystemProfile | null>(null)
  const [showOnboarding, setShowOnboarding] = useState(false)
  const [installingModel, setInstallingModel] = useState<string | null>(null)
  const [modelDialogModel, setModelDialogModel] = useState<api.ModelDescriptor | null>(null)
  const [projectInfoOpen, setProjectInfoOpen] = useState(false)
  const taskRef = useRef<RunningTask | null>(null)
  taskRef.current = task

  const asset = assets.find((candidate) => candidate.id === currentId) ?? null
  const sourceDurationSec = asset ? num(asset.duration_samples) / num(asset.audio_sample_rate) : 0
  const durationSec = timeline
    ? num(timeline.output_duration_frames) / frameRateFps(timeline.rate)
    : sourceDurationSec
  const speakerNames = useMemo(() => new Map(speakers.map((speaker) => [speaker.id, speaker.display_name])), [speakers])
  const sourcePlayheadSec = useMemo(() => {
    if (!timeline || !asset) return playheadSec
    const outputFrame = Math.floor(playheadSec * frameRateFps(timeline.rate))
    const clip = timeline.clips.find((candidate) =>
      outputFrame >= num(candidate.timeline_start_frame)
      && outputFrame < num(candidate.timeline_end_frame),
    )
    if (!clip || clip.source_asset_id !== asset.id) return 0
    const sourceStart = num(clip.source_in_frame)
    const sourceLength = num(clip.source_out_frame) - sourceStart
    const outputStart = num(clip.timeline_start_frame)
    const outputLength = Math.max(1, num(clip.timeline_end_frame) - outputStart)
    const sourceFrame = sourceStart + ((outputFrame - outputStart) / outputLength) * sourceLength
    return sourceFrame / frameRateFps(asset.rate)
  }, [asset, playheadSec, timeline])

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && media.matches)
      document.documentElement.classList.toggle('dark', dark)
      window.localStorage.setItem('studio.theme', theme)
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [theme])

  useEffect(() => {
    document.documentElement.classList.toggle('transcript-section-tint', preferences?.transcript_section_tint ?? true)
    return () => document.documentElement.classList.remove('transcript-section-tint')
  }, [preferences?.transcript_section_tint])

  const refreshRecentProjects = useCallback(async () => {
    if (!api.isTauri) return
    try {
      const result = await api.recentProjectsList()
      if (result.status === 'success') setRecentProjects(result.data ?? [])
    } catch {
      // Recent projects are an enhancement; keep the project library usable if the store is unavailable.
    }
  }, [])

  const refreshModelCatalog = useCallback(async () => {
    if (!api.isTauri) return
    try {
      const result = await api.modelCatalog()
      if (result.status === 'success') setModels(result.data ?? [])
    } catch {
      // A missing backend command must not be displayed as an installed model.
    }
  }, [])

  useEffect(() => {
    if (!api.isTauri) return
    let disposed = false
    const load = async () => {
      try {
        const [preferencesResult, onboardingResult, profileResult] = await Promise.allSettled([api.preferencesGet(), api.onboardingGet(), api.systemProfile()])
        if (disposed) return
        if (preferencesResult.status === 'fulfilled' && preferencesResult.value.status === 'success' && preferencesResult.value.data) {
          setPreferences(preferencesResult.value.data)
          setTheme(preferencesResult.value.data.theme)
          setShowOnboarding(!preferencesResult.value.data.onboarding_completed)
        } else if (onboardingResult.status === 'fulfilled' && onboardingResult.value.status === 'success' && onboardingResult.value.data) {
          setShowOnboarding(!onboardingResult.value.data.completed)
        }
        if (profileResult.status === 'fulfilled' && profileResult.value.status === 'success') setSystemProfile(profileResult.value.data ?? null)
        await Promise.all([refreshRecentProjects(), refreshModelCatalog()])
      } catch (error) {
        if (!disposed) setNotice(error instanceof Error ? error.message : '应用设置暂时无法读取')
      }
    }
    void load()
    return () => { disposed = true }
  }, [refreshModelCatalog, refreshRecentProjects])

  useEffect(() => {
    const reset = () => setShowOnboarding(true)
    window.addEventListener('dl://onboarding-reset', reset)
    return () => window.removeEventListener('dl://onboarding-reset', reset)
  }, [])

  const refreshAssets = useCallback(async (selectId?: string) => {
    if (!api.isTauri) return
    const result = await api.assetsList()
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '读取素材失败')
      return
    }
    const next = result.data ?? []
    setAssets(next)
    setCurrentId((previous) => selectId ?? (previous && next.some((candidate) => candidate.id === previous) ? previous : next[0]?.id ?? null))
  }, [])

  const refreshTimeline = useCallback(async () => {
    if (!api.isTauri) return
    const [clips, compiled, preview] = await Promise.all([
      api.mainTrackList(),
      api.timelineGet(),
      api.projectExportPreview(),
    ])
    if (clips.status === 'success') setMainTrack(clips.data ?? [])
    if (compiled.status === 'success') setTimeline(compiled.data ?? null)
    else setTimeline(null)
    setTimelinePreview(preview.status === 'success' ? preview.data ?? null : null)
    setSelectedClipId((previous) => previous && (clips.data ?? []).some((clip) => clip.id === previous) ? previous : clips.data?.[0]?.id ?? null)
  }, [])

  const refreshProjectSettings = useCallback(async () => {
    if (!api.isTauri) return
    const [canvasResult, outputRateResult, styleResult, speakersResult, revisionResult] = await Promise.all([api.canvasGet(), api.outputRateGet(), api.subtitleStyleGet(), api.speakerList(), api.projectRevision()])
    if (canvasResult.status === 'success') setCanvas(canvasResult.data ?? null)
    if (outputRateResult.status === 'success') setOutputRate(outputRateResult.data ?? null)
    if (styleResult.status === 'success') setSubtitleStyle(styleResult.data ?? null)
    if (speakersResult.status === 'success') setSpeakers(speakersResult.data ?? [])
    if (revisionResult.status === 'success' && revisionResult.data !== null) setLoadedRevision(num(revisionResult.data))
  }, [])

  const refreshTranscript = useCallback(async (assetId: string | null) => {
    if (!api.isTauri || !assetId) {
      setTranscript(null)
      return
    }
    const result = await api.transcriptGet(assetId)
    setTranscript(result.status === 'success' ? result.data ?? null : null)
  }, [])

  const refreshAll = useCallback(async (selectId?: string) => {
    await Promise.all([refreshAssets(selectId), refreshTimeline(), refreshProjectSettings()])
  }, [refreshAssets, refreshProjectSettings, refreshTimeline])

  useEffect(() => { void refreshTranscript(currentId) }, [currentId, refreshTranscript])

  useEffect(() => {
    if (!api.isTauri) return
    let disposed = false
    const unlisten: Array<() => void> = []
    import('@tauri-apps/api/event').then(({ listen }) => {
      if (disposed) return
      void listen<ProgressEvent>('dl://progress', (event) => {
        const current = taskRef.current
        if (!current || current.id !== event.payload.task) return
        setTask({
          ...current,
          completed: event.payload.completed === null ? current.completed : num(event.payload.completed),
          total: event.payload.total === null ? current.total : num(event.payload.total),
          message: event.payload.message,
        })
      }).then((remove) => unlisten.push(remove))
      void listen<{ task_id: string; state: TaskState }>('dl://task-state', (event) => {
        const current = taskRef.current
        if (!current || current.id !== event.payload.task_id) return
        setTask(null)
        void refreshAll(current.assetId)
        void refreshTranscript(current.assetId)
        const message = current.kind === 'speaker'
          ? event.payload.state === 'succeeded' ? '说话人分离完成，候选需要你确认后才会合并。' : '说话人分离没有覆盖现有身份。'
          : event.payload.state === 'succeeded' ? '转录完成。' : event.payload.state === 'cancelled' ? '转录已取消，旧版本保持不变。' : '转录没有完成，旧版本保持不变。'
        setNotice(message)
      }).then((remove) => unlisten.push(remove))
      void listen<Partial<api.ModelDownloadProgress> & { bytes_downloaded?: number | bigint; bytes_total?: number | bigint }>('dl://model-progress', (event) => {
        const progress = api.normalizeModelProgress(event.payload)
        setModels((current) => current.map((model) => model.id === progress.model_id ? {
          ...model,
          state: progress.state,
          downloaded_bytes: progress.completed_bytes,
        } : model))
      }).then((remove) => unlisten.push(remove))
      void listen<Partial<api.ModelInstallation> & { bytes_downloaded?: number | bigint; bytes_total?: number | bigint; last_error_message?: string | null }>('dl://model-state', (event) => {
        const installation = api.normalizeModelInstallation(event.payload)
        setModels((current) => current.map((model) => model.id === installation.model_id ? {
          ...model,
          state: installation.state,
          downloaded_bytes: installation.downloaded_bytes,
          error: installation.error ?? null,
          installed_revision: installation.state === 'installed' ? installation.revision : model.installed_revision,
        } : model))
      }).then((remove) => unlisten.push(remove))
      void listen<{ changed_keys: string[] }>('dl://preferences-changed', () => {
        void api.preferencesGet().then((result) => {
          if (result.status === 'success' && result.data) {
            setPreferences(result.data)
            setTheme(result.data.theme)
            setShowOnboarding(!result.data.onboarding_completed)
          }
        }).catch(() => undefined)
      }).then((remove) => unlisten.push(remove))
    }).catch(() => undefined)
    return () => { disposed = true; unlisten.forEach((remove) => remove()) }
  }, [refreshAll, refreshTranscript])

  const openSettings = async () => {
    if (!api.isTauri) {
      // Browser preview keeps the legacy settings screen reachable for tests and local UI review.
      setScreen('settings')
      return
    }
    try {
      const result = await api.settingsOpen()
      if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '设置窗口无法打开')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '设置窗口无法打开')
    }
  }

  const openProject = async (create: boolean) => {
    if (!api.isTauri) {
      setNotice('请在 Double Love Studio 桌面应用中打开本地项目。')
      return
    }
    const picked = await api.pickDirectory(create ? '选择新项目所在位置' : '选择已有项目')
    if (!picked || Array.isArray(picked)) return
    const result = create ? await api.projectCreate(picked) : await api.projectOpen(picked)
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '项目无法打开')
      return
    }
    setProject(result.data)
    setScreen('library')
    setCurrentId(null)
    setSelectedClipId(null)
    setTranscript(null)
    setNotice(null)
    setShowOnboarding(false)
    void api.onboardingComplete(preferences?.default_asr_model).catch(() => undefined)
    void refreshRecentProjects()
    await refreshAll()
  }

  const addAssetToTrack = async (candidate: MediaAssetSummary) => {
    if (!await ensureFreshBeforeWrite()) return
    setBusyAssetId(candidate.id)
    const result = await api.mainTrackAppendFull(candidate.id)
    setBusyAssetId(null)
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '素材无法加入主轨')
      return
    }
    setCurrentId(candidate.id)
    setSelectedClipId(result.data.id)
    setMediaDrawerOpen(false)
    setScreen('editor')
    if (result.revision !== null) setLoadedRevision(num(result.revision))
    await refreshTimeline()
  }

  const importNewMedia = async () => {
    if (!api.isTauri) {
      setNotice('请在桌面应用中导入本地媒体。')
      return
    }
    const picked = await api.pickMediaFile()
    if (!picked || Array.isArray(picked)) return
    const result = await api.importMedia(picked)
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '媒体导入失败')
      return
    }
    await refreshAssets(result.data.id)
    await addAssetToTrack(result.data)
  }

  const chooseClip = (clip: MainTrackClip) => {
    setSelectedClipId(clip.id)
    setCurrentId(clip.source_asset_id)
    const resolved = timeline?.clips.find((candidate) => candidate.id.startsWith(clip.id + ':'))
    setPlayheadSec(resolved ? num(resolved.timeline_start_frame) / frameRateFps(timeline?.rate ?? 'fps_25') : 0)
    setPlaying(false)
  }

  const moveClip = async (clipId: string, beforeId: string | null) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.mainTrackMove(clipId, beforeId)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '排序失败')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTimeline()
    }
  }

  const trimClip = async (clip: MainTrackClip, sourceInFrame: number, sourceOutFrame: number) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.mainTrackTrim(clip.id, sourceInFrame, sourceOutFrame)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '裁切失败')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTimeline()
    }
  }

  const splitClip = async (clip: MainTrackClip) => {
    if (!await ensureFreshBeforeWrite()) return
    const sourceIn = num(clip.source_in_frame)
    const sourceOut = num(clip.source_out_frame)
    const outputFrame = timeline ? Math.floor(playheadSec * frameRateFps(timeline.rate)) : 0
    const resolved = timeline?.clips.find((candidate) => candidate.id.startsWith(`${clip.id}:`) && outputFrame > num(candidate.timeline_start_frame) && outputFrame < num(candidate.timeline_end_frame))
    const sourceAt = resolved
      ? Math.round(num(resolved.source_in_frame) + ((outputFrame - num(resolved.timeline_start_frame)) / Math.max(1, num(resolved.timeline_end_frame) - num(resolved.timeline_start_frame))) * (num(resolved.source_out_frame) - num(resolved.source_in_frame)))
      : Math.round((sourceIn + sourceOut) / 2)
    const result = await api.mainTrackSplit(clip.id, Math.max(sourceIn + 1, Math.min(sourceOut - 1, sourceAt)))
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '拆分失败')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTimeline()
    }
  }

  const removeClip = async (clip: MainTrackClip) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.mainTrackRemove(clip.id)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '移除失败')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTimeline()
    }
  }

  const seek = (seconds: number) => {
    const target = Math.max(0, Math.min(durationSec, seconds))
    setPlayheadSec(target)
  }

  const seekTranscript = (sourceSeconds: number) => {
    if (!timeline || !asset) {
      seek(sourceSeconds)
      return
    }
    const sourceFrame = sourceSeconds * frameRateFps(asset.rate)
    const outputFrame = Math.floor(playheadSec * frameRateFps(timeline.rate))
    const matching = timeline.clips.find((clip) =>
      clip.source_asset_id === asset.id
      && sourceFrame >= num(clip.source_in_frame)
      && sourceFrame < num(clip.source_out_frame)
      && outputFrame >= num(clip.timeline_start_frame)
      && outputFrame < num(clip.timeline_end_frame),
    ) ?? timeline.clips.find((clip) =>
      clip.source_asset_id === asset.id
      && sourceFrame >= num(clip.source_in_frame)
      && sourceFrame < num(clip.source_out_frame),
    )
    if (!matching) return
    const sourceStart = num(matching.source_in_frame)
    const sourceLength = Math.max(1, num(matching.source_out_frame) - sourceStart)
    const output = num(matching.timeline_start_frame)
      + ((sourceFrame - sourceStart) / sourceLength)
        * (num(matching.timeline_end_frame) - num(matching.timeline_start_frame))
    seek(output / frameRateFps(timeline.rate))
  }

  const togglePlay = () => {
    setPlaying((value) => !value)
  }

  const installModel = async (modelId: string) => {
    if (!api.isTauri) {
      setNotice('请在 Double Love Studio 桌面应用中安装本地模型。')
      return
    }
    setInstallingModel(modelId)
    try {
      const result = await api.modelInstall(modelId)
      if (result.status === 'failed') {
        setNotice(result.diagnostics[0]?.cause ?? '模型安装没有开始')
      } else {
        setNotice('模型已进入下载队列；完成校验后才会启用。')
        await refreshModelCatalog()
      }
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '模型安装没有开始')
    } finally {
      setInstallingModel(null)
    }
  }

  const completeOnboarding = async (defaultAsrModel?: string) => {
    if (api.isTauri) {
      try {
        const result = await api.onboardingComplete(defaultAsrModel)
        if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '新手引导状态没有保存')
      } catch (error) {
        setNotice(error instanceof Error ? error.message : '新手引导状态没有保存')
      }
    }
    setShowOnboarding(false)
  }

  const selectedModelId = preferences?.default_asr_model ?? systemProfile?.recommended_asr_model ?? 'qwen3-asr-0.6b'
  const selectedModel = models.find((model) => model.id === selectedModelId) ?? (models.find((model) => model.kind === 'asr') ?? FALLBACK_MODEL)
  const modelReady = selectedModel.state === 'installed'

  const startTranscription = async () => {
    if (!asset) return
    if (!await ensureFreshBeforeWrite()) return
    if (api.isTauri && !modelReady) {
      setModelDialogModel(selectedModel)
      return
    }
    const result = await api.transcribeStart(asset.id, selectedModelId, 'auto')
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '无法启动转录')
      return
    }
    setTask({ id: result.data.task_id, assetId: asset.id, kind: 'transcribe', completed: null, total: null, message: '正在建立候选转录…' })
  }

  const cancelTask = async () => {
    if (!task) return
    const result = await api.taskCancel(task.id)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '取消失败')
  }

  const omitWords = async (start: number, end: number) => {
    if (!asset) return
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.editOmit(asset.id, start, end)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '删除失败')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTranscript(asset.id)
      await refreshTimeline()
    }
  }

  const restoreWords = async (operationId: string, start: number, end: number) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.editRestore(operationId, start, end)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '恢复失败')
    else if (asset) {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshTranscript(asset.id)
      await refreshTimeline()
    }
  }

  const startDiarization = async () => {
    if (!asset) return
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.speakerDiarizeStart(asset.id)
    if (result.status === 'failed' || !result.data) {
      setNotice(result.diagnostics[0]?.cause ?? '无法启动说话人分离')
      return
    }
    setTask({ id: result.data.task_id, assetId: asset.id, kind: 'speaker', completed: null, total: null, message: '正在进行本地说话人分离…' })
  }

  useEffect(() => {
    if (editorTab !== 'speakers' || !asset || !api.isTauri) return
    void Promise.all([api.speakerNameProposals(asset.id), api.speakerDiarizationGet(asset.id)]).then(([names, diarization]) => {
      setNameProposals(names.status === 'success' ? names.data ?? [] : [])
      setMergeProposals(diarization.status === 'success' ? diarization.data?.merge_proposals.filter((proposal) => proposal.status === 'pending') ?? [] : [])
    })
  }, [asset, editorTab])

  useEffect(() => {
    if ((screen !== 'settings' && !projectInfoOpen) || !api.isTauri) return
    void api.projectHistory().then((result) => setHistory(result.status === 'success' ? result.data ?? [] : []))
  }, [screen, projectInfoOpen, loadedRevision])

  const ensureFreshBeforeWrite = async (): Promise<boolean> => {
    if (!api.isTauri || loadedRevision === null) return true
    const result = await api.projectRevision()
    if (result.status === 'success' && result.data !== null && num(result.data) !== loadedRevision) {
      await refreshAll()
      setNotice('项目刚被外部命令更新，已重新读取最新状态；请再执行一次操作。')
      return false
    }
    return true
  }

  const applyNameProposal = async (proposal: SpeakerNameProposal) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.speakerNameConfirm(proposal.speaker_id, proposal.candidate_name)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '名称没有应用')
    else {
      if (result.revision !== null) setLoadedRevision(num(result.revision))
      await refreshProjectSettings()
      setNotice(`已确认说话人名称：${proposal.candidate_name}`)
    }
  }

  const saveManualSpeakerName = async () => {
    if (!renamingSpeaker) return
    const displayName = speakerNameDraft.trim()
    if (!displayName) {
      setNotice('请先输入说话人名称。')
      return
    }
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.speakerNameConfirm(renamingSpeaker.id, displayName)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '名称没有应用')
      return
    }
    setRenamingSpeaker(null)
    if (result.revision !== null) setLoadedRevision(num(result.revision))
    await refreshProjectSettings()
    if (asset) await refreshTranscript(asset.id)
    setNotice(`已更新说话人名称：${displayName}`)
  }

  const previewAgentPayload = async (speakerId: string) => {
    if (!asset) return
    const result = await api.speakerAgentPayloadPreview(asset.id, speakerId)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '无法生成最小数据包')
    else setAgentPayload(result.data ?? null)
  }

  const confirmSpeakerMerge = async (proposal: SpeakerMergeProposal) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.speakerMergeConfirm(proposal.left_speaker_id, proposal.right_speaker_id)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '说话人没有合并')
      return
    }
    setMergeProposals((proposals) => proposals.filter((candidate) => candidate.id !== proposal.id))
    if (result.revision !== null) setLoadedRevision(num(result.revision))
    await refreshProjectSettings()
    if (asset) await refreshTranscript(asset.id)
    setNotice('已确认合并说话人。')
  }

  const saveCanvas = async (next: CanvasSpec) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.canvasSet(next)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '画布设置没有保存')
    else {
      setCanvas(next)
      if (result.revision !== null) setLoadedRevision(num(result.revision))
    }
  }

  const saveOutputRate = async (next: FrameRate | null) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.outputRateSet(next)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '输出帧率没有保存')
      return
    }
    setOutputRate(next)
    if (result.revision !== null) setLoadedRevision(num(result.revision))
    await refreshTimeline()
  }

  const saveSubtitleStyle = async (next: SubtitleStyle) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.subtitleStyleSet(next)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '字幕样式没有保存')
    else {
      setSubtitleStyle(next)
      if (result.revision !== null) setLoadedRevision(num(result.revision))
    }
  }

  const restoreHistory = async (revision: number) => {
    if (!await ensureFreshBeforeWrite()) return
    const result = await api.projectRestoreRevision(revision)
    if (result.status === 'failed') {
      setNotice(result.diagnostics[0]?.cause ?? '历史版本没有恢复')
      return
    }
    setNotice(`已恢复版本 ${revision}，并创建了一条新的历史记录。`)
    await refreshAll()
    const historyResult = await api.projectHistory()
    if (historyResult.status === 'success') setHistory(historyResult.data ?? [])
  }

  const showExport = async () => {
    const result = await api.projectExportPreview()
    setExportPreview(result)
  }

  const exportProject = async (kind: 'xml' | 'ass' | 'mp4') => {
    const base = `${shortProjectName(project).replace(/\s+/g, '_')}_ROUGH_CUT`
    const target = await api.pickProjectExportPath(`${base}.${kind}`, kind)
    if (!target || Array.isArray(target)) return
    setExportBusy(true)
    const result = kind === 'xml' ? await api.projectExportXmemlApply(target) : kind === 'ass' ? await api.projectExportAssApply(target) : await api.projectRenderMp4Apply(target)
    setExportBusy(false)
    if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '导出失败')
    else { setExportPreview(null); setNotice(`已导出：${target}`) }
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const editingText = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.tagName === 'SELECT' || target?.isContentEditable
      const command = event.metaKey || event.ctrlKey
      if (editingText && !(command && event.key.toLowerCase() === 'z')) return
      const key = event.key.toLowerCase()
      if (command && key === ',') { event.preventDefault(); void openSettings(); return }
      if (command && key === 'n') { event.preventDefault(); void openProject(true); return }
      if (command && key === 'o') { event.preventDefault(); void openProject(false); return }
      if (command && key === 'e') { event.preventDefault(); if (screen === 'editor') void showExport(); return }
      if (command && key === 'z') {
        event.preventDefault()
        if (!api.isTauri) { setNotice('撤销需要在 Double Love Studio 桌面应用中执行。'); return }
        void (event.shiftKey ? api.editRedo() : api.editUndo()).then((result) => {
          if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '编辑历史没有改变')
          else void refreshAll()
        }).catch(() => setNotice('编辑历史没有改变'))
        return
      }
      if (screen !== 'editor') return
      if (key === ' ' || event.code === 'Space') { event.preventDefault(); togglePlay(); return }
      if (key === 'arrowleft') { event.preventDefault(); seek(playheadSec - 5); return }
      if (key === 'arrowright') { event.preventDefault(); seek(playheadSec + 5); return }
      if (key === 's' && selectedClipId) {
        event.preventDefault()
        const clip = mainTrack.find((candidate) => candidate.id === selectedClipId)
        if (clip) void splitClip(clip)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })

  const renderEditorTab = () => {
    if (!asset) return <div className="studio-editor-empty"><Clapperboard size={22} /><strong>主轨还没有素材</strong><p>添加一个本地视频后，就可以开始按文本粗剪。</p><button type="button" className="studio-primary-button" onClick={() => setMediaDrawerOpen(true)}>添加素材</button></div>
    if (editorTab === 'transcript') return <TranscriptView asset={asset} view={transcript} playheadSec={sourcePlayheadSec} transcription={task?.kind === 'transcribe' && task.assetId === asset.id ? task : null} speakerNames={speakerNames} onSeek={seekTranscript} onOmit={omitWords} onRestore={restoreWords} onTranscribeStart={startTranscription} onTranscribeCancel={cancelTask} />
    if (editorTab === 'subtitles') return <div className="studio-editor-panel"><Captions size={20} /><h2>项目级字幕样式</h2><p>字幕会跟随文字删减和主轨重排重新投影。ASS 与 MP4 完整保留样式。</p><button type="button" className="studio-secondary-button" onClick={() => setScreen('settings')}>打开字幕设置</button></div>
    return <div className="studio-speaker-panel"><header><div><Mic2 size={19} /><h2>说话人</h2></div>{asset.status === 'transcribed' && <button type="button" className="studio-secondary-button" disabled={task !== null} onClick={startDiarization}>{task?.kind === 'speaker' ? '处理中…' : '本地分离'}</button>}</header>{speakers.length === 0 ? <p>完成转录后，可以在本机识别说话人。姓名和跨素材合并都需要你确认。</p> : <div className="studio-speaker-list">{speakers.map((speaker) => <article key={speaker.id}><i style={{ background: speaker.color }}></i><div><strong>{speaker.display_name}</strong><small>{speaker.confirmed ? '已确认名称' : '匿名说话人'}</small></div><div className="studio-speaker-actions"><button type="button" className="studio-text-action" onClick={() => { setRenamingSpeaker(speaker); setSpeakerNameDraft(speaker.display_name) }}>改名</button><button type="button" className="studio-text-action" onClick={() => previewAgentPayload(speaker.id)}>查看 Agent 数据包</button></div></article>)}</div>}{nameProposals.length > 0 && <section className="studio-name-proposals"><h3>本地名称候选</h3>{nameProposals.map((proposal) => <article key={`${proposal.speaker_id}-${proposal.candidate_name}`}><p>“{proposal.quote}”<small>{proposal.reason} · 置信度 {Math.round(proposal.confidence * 100)}%</small></p><button type="button" onClick={() => applyNameProposal(proposal)}>确认「{proposal.candidate_name}」</button></article>)}</section>}{mergeProposals.length > 0 && <section className="studio-name-proposals"><h3>跨素材合并候选</h3>{mergeProposals.map((proposal) => <article key={proposal.id}><p>{speakerNames.get(proposal.left_speaker_id) ?? '匿名说话人'} 与 {speakerNames.get(proposal.right_speaker_id) ?? '匿名说话人'}<small>{proposal.evidence} · 相似度 {Math.round(proposal.similarity * 100)}%</small></p><button type="button" onClick={() => confirmSpeakerMerge(proposal)}>确认合并</button></article>)}</section>}</div>
  }

  const editor = (
    <section className="studio-editor" aria-label="粗剪编辑器">
      <div className="studio-editor-split">
        <section className="studio-player-pane">
          <TimelinePreview
            timeline={timeline}
            canvas={canvas}
            style={subtitleStyle}
            cues={timelinePreview?.subtitle_cues ?? []}
            outputPlayheadSec={playheadSec}
            playing={playing}
            onOutputTimeUpdate={setPlayheadSec}
            onPlayState={setPlaying}
            onSourceChange={(assetId) => {
              if (assetId) setCurrentId(assetId)
            }}
          />
          <div className="studio-player-tools"><div><button type="button" onClick={() => setEditorTab('subtitles')}><Captions size={15} />字幕样式</button><button type="button" onClick={() => setCanvasMenuOpen((open) => !open)}><Palette size={15} />画布</button><button type="button" onClick={() => setEditorTab('speakers')}><Mic2 size={15} />说话人</button></div><span>{asset ? `${asset.width ?? '—'} × ${asset.height ?? '—'}` : ''}</span>{canvasMenuOpen && canvas && <div className="studio-canvas-menu"><strong>统一画布</strong><label>适配<select value={canvas.fit} onChange={(event) => void saveCanvas({ ...canvas, fit: event.target.value as CanvasSpec['fit'] })}><option value="contain">完整显示</option><option value="cover">铺满裁切</option></select></label><label>缩放<input type="number" step="0.05" defaultValue={canvas.scale} onBlur={(event) => void saveCanvas({ ...canvas, scale: Math.max(0.1, Number(event.target.value) || 1) })} /></label><button type="button" onClick={() => setScreen('settings')}>更多画布设置</button></div>}</div>
          <Transport playing={playing} clock={playheadClock(playheadSec, durationSec, preferences?.timecode_precision === 'millisecond')} disabled={!asset} onTogglePlay={togglePlay} onSkip={(delta) => seek(playheadSec + delta)} />
        </section>
        <section className="studio-transcript-pane">
          <header className="studio-editor-tabs"><button type="button" className={editorTab === 'transcript' ? 'is-active' : ''} onClick={() => setEditorTab('transcript')}>转录</button><button type="button" className={editorTab === 'subtitles' ? 'is-active' : ''} onClick={() => setEditorTab('subtitles')}>字幕</button><button type="button" className={editorTab === 'speakers' ? 'is-active' : ''} onClick={() => setEditorTab('speakers')}>说话人</button></header>
          {renderEditorTab()}
        </section>
      </div>
      <MainTrackTimeline clips={mainTrack} assets={assets} selectedId={selectedClipId} timeline={timeline} playheadSec={playheadSec} outputRate={timeline?.rate ?? outputRate} onSeek={seek} onSelect={chooseClip} onMove={(clipId, beforeId) => void moveClip(clipId, beforeId)} onTrim={(clip, sourceIn, sourceOut) => void trimClip(clip, sourceIn, sourceOut)} onSplit={(clip) => void splitClip(clip)} onRemove={(clip) => void removeClip(clip)} onAdd={() => setMediaDrawerOpen(true)} />
    </section>
  )

  if (showOnboarding && api.isTauri) {
    return <Onboarding recommendedModel={systemProfile?.recommended_asr_model ?? preferences?.default_asr_model ?? 'qwen3-asr-0.6b'} systemProfile={systemProfile} models={models} installingModel={installingModel} onInstallModel={(modelId) => void installModel(modelId)} onCreateProject={() => void openProject(true)} onOpenProject={() => void openProject(false)} onSkip={() => void completeOnboarding()} onFinish={() => void completeOnboarding(systemProfile?.recommended_asr_model ?? preferences?.default_asr_model)} />
  }

  return (
    <div className="studio-app">
      <a className="studio-skip-link" href="#studio-main-content">跳到主要内容</a>
      <TitleBar projectName={projectName(project)} screen={screen} sidebarVisible={sidebarVisible} onToggleSidebar={() => setSidebarVisible((visible) => !visible)} onBackToLibrary={() => setScreen('library')} onAddMedia={() => setMediaDrawerOpen(true)} onExport={() => void showExport()} onOpenProjectInfo={() => setProjectInfoOpen(true)} addDisabled={!project || !api.isTauri} exportDisabled={!project || mainTrack.length === 0 || !api.isTauri} />
      {notice && <div className="studio-notice" role="status" aria-live="polite"><span>{notice}</span><button type="button" aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
      <div className="studio-app-body">
        {sidebarVisible && <Sidebar project={project} screen={screen} onNavigate={setScreen} onCreate={() => void openProject(true)} onOpen={() => void openProject(false)} onOpenSettings={() => void openSettings()} />}
        <main className="studio-main" id="studio-main-content">
          {screen === 'library' && <ProjectLibrary project={project} assets={assets} recentProjects={recentProjects} modelReady={modelReady} onCreate={() => void openProject(true)} onOpen={() => void openProject(false)} onEnterEditor={() => setScreen('editor')} onOpenModels={() => void openSettings()} onForgetRecent={(root) => { if (!api.isTauri) return; void api.recentProjectForget(root).then(() => refreshRecentProjects()).catch(() => setNotice('最近项目记录没有移除')) }} />}
          {screen === 'editor' && editor}
          {screen === 'tasks' && <section className="studio-tasks"><header><h1>后台任务</h1><p>所有模型任务在本机运行，完成前可以继续浏览项目。</p></header>{task ? <article><Activity size={18} /><div><strong>{task.kind === 'speaker' ? '说话人分离' : '转录'}</strong><p>{task.message}</p></div><button type="button" className="studio-secondary-button" onClick={cancelTask}>取消</button></article> : <div className="studio-tasks-empty">当前没有后台任务。</div>}</section>}
          {screen === 'settings' && <ProjectSettings projectOpen={project !== null} canvas={canvas} outputRate={outputRate} subtitleStyle={subtitleStyle} theme={theme} onThemeChange={setTheme} history={history} onCanvasSave={(next) => void saveCanvas(next)} onOutputRateSave={(next) => void saveOutputRate(next)} onStyleSave={(next) => void saveSubtitleStyle(next)} onRestoreRevision={(revision) => void restoreHistory(revision)} />}
        </main>
      </div>
      {mediaDrawerOpen && <MediaDrawer assets={assets} busyAssetId={busyAssetId} onClose={() => setMediaDrawerOpen(false)} onAddExisting={(candidate) => void addAssetToTrack(candidate)} onImport={() => void importNewMedia()} />}
      {modelDialogModel && <ModelInstallDialog model={modelDialogModel} busy={installingModel === modelDialogModel.id} onInstall={() => void installModel(modelDialogModel.id)} onClose={() => setModelDialogModel(null)} onOpenSettings={() => { setModelDialogModel(null); void openSettings() }} />}
      {exportPreview && <ProjectExportDialog result={exportPreview} busy={exportBusy} onClose={() => setExportPreview(null)} onExport={(kind) => void exportProject(kind)} />}
      {projectInfoOpen && <ProjectInfoDialog outputRate={outputRate} resolvedRate={timeline?.rate ?? null} history={history} onRateChange={(rate) => void saveOutputRate(rate)} onRestore={(revision) => void restoreHistory(revision)} onClose={() => setProjectInfoOpen(false)} />}
      {agentPayload && <div className="studio-popover-backdrop" role="presentation" onMouseDown={() => setAgentPayload(null)}><section className="studio-agent-payload" role="dialog" aria-modal="true" aria-label="Agent 数据包预览" onMouseDown={(event) => event.stopPropagation()}><header><Wand2 size={18} /><div><strong>Agent 数据包预览</strong><span>只含匿名说话人的必要发言</span></div></header><p>{agentPayload.instruction}</p><pre>{agentPayload.utterances.join('\n\n')}</pre><button type="button" className="studio-secondary-button" onClick={() => setAgentPayload(null)}>关闭</button></section></div>}
      {renamingSpeaker && <div className="studio-popover-backdrop" role="presentation" onMouseDown={() => setRenamingSpeaker(null)}><form className="studio-agent-payload studio-speaker-rename" role="dialog" aria-modal="true" aria-label="修改说话人名称" onMouseDown={(event) => event.stopPropagation()} onSubmit={(event) => { event.preventDefault(); void saveManualSpeakerName() }}><header><Mic2 size={18} /><div><strong>修改说话人名称</strong><span>只修改项目里的身份映射，不会改写文字或时间。</span></div></header><label>显示名称<input aria-label="说话人显示名称" value={speakerNameDraft} maxLength={64} autoFocus onChange={(event) => setSpeakerNameDraft(event.target.value)} /></label><div><button type="button" className="studio-secondary-button" onClick={() => setRenamingSpeaker(null)}>取消</button><button type="submit" className="studio-primary-button">确认名称</button></div></form></div>}
    </div>
  )
}
