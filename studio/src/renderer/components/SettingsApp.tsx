import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  AlertTriangle,
  AudioLines,
  Check,
  ChevronRight,
  ClipboardCheck,
  FolderOpen,
  Info,
  Keyboard,
  LockKeyhole,
  RefreshCw,
  Settings2,
  SlidersHorizontal,
  Trash2,
  Wand2,
} from 'lucide-react'
import * as api from '../platform/desktop'
import type { SubtitleStyle } from '../../../../bindings/SubtitleStyle'
import { num } from '../utils'

type SettingsPage = 'general' | 'shortcuts' | 'subtitle' | 'models' | 'privacy' | 'diagnostics' | 'about'

interface SettingsAppProps {
  initialPage?: SettingsPage
}

const PAGE_ITEMS: Array<{ id: SettingsPage; label: string; icon: typeof Settings2 }> = [
  { id: 'general', label: '通用', icon: Settings2 },
  { id: 'shortcuts', label: '快捷键', icon: Keyboard },
  { id: 'subtitle', label: '默认字幕样式', icon: SlidersHorizontal },
  { id: 'models', label: '本地模型', icon: AudioLines },
  { id: 'privacy', label: '隐私', icon: LockKeyhole },
  { id: 'diagnostics', label: '诊断', icon: ClipboardCheck },
  { id: 'about', label: '关于', icon: Info },
]

const DEFAULT_PREFERENCES: api.AppPreferencesV1 = {
  schema_version: 1,
  theme: 'light',
  restore_last_project: true,
  timecode_precision: 'frame',
  transcript_section_tint: true,
  cjk_spacing: true,
  default_subtitle_style: null,
  model_root: '~/Library/Application Support/Double Love/models',
  model_endpoint: 'https://huggingface.co',
  default_asr_model: 'qwen3-asr-0.6b',
  onboarding_version: 1,
  onboarding_completed: false,
  recent_projects: [],
}

const PREVIEW_MODELS: api.ModelDescriptor[] = [
  {
    id: 'qwen3-asr-0.6b',
    label: 'Qwen3 ASR · 0.6B',
    kind: 'asr',
    revision: 'preview',
    size_bytes: 1_200_000_000,
    memory_bytes: 8_000_000_000,
    license: 'Apache-2.0',
    description: '轻量，适合 8–16 GB 内存。',
    dependencies: [{ model_id: 'qwen3-forced-aligner-0.6b', required: true, reason: '逐词时间锚点' }],
    state: 'not_installed',
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
  {
    id: 'qwen3-asr-1.7b',
    label: 'Qwen3 ASR · 1.7B',
    kind: 'asr',
    revision: 'preview',
    size_bytes: 3_800_000_000,
    memory_bytes: 16_000_000_000,
    license: 'Apache-2.0',
    description: '更高准确率，推荐 16 GB 及以上。',
    dependencies: [{ model_id: 'qwen3-forced-aligner-0.6b', required: true, reason: '逐词时间锚点' }],
    state: 'not_installed',
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
  {
    id: 'qwen3-forced-aligner-0.6b',
    label: 'Forced Aligner · 0.6B',
    kind: 'aligner',
    revision: 'preview',
    size_bytes: 750_000_000,
    memory_bytes: 2_000_000_000,
    license: 'Apache-2.0',
    description: '逐词时间锚点，由 Qwen3 ASR 共享依赖。',
    dependencies: [],
    state: 'not_installed',
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: false,
  },
  {
    id: 'silero-vad-wespeaker-zh',
    label: 'Silero VAD + WeSpeaker',
    kind: 'speaker',
    revision: 'preview',
    size_bytes: 290_000_000,
    memory_bytes: 1_200_000_000,
    license: 'MIT / Apache-2.0',
    description: '可选，用于语音活动检测和中文说话人识别。',
    dependencies: [],
    state: 'not_installed',
    installed_revision: null,
    downloaded_bytes: 0,
    can_remove: true,
  },
]

function readableBytes(value: number | bigint): string {
  const bytes = num(value)
  if (!Number.isFinite(bytes) || bytes <= 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let index = 0
  let current = bytes
  while (current >= 1024 && index < units.length - 1) {
    current /= 1024
    index += 1
  }
  return `${current >= 10 || index === 0 ? Math.round(current) : current.toFixed(1)} ${units[index]}`
}

function stateLabel(state: api.ModelInstallState): string {
  return {
    not_installed: '未安装',
    queued: '排队中',
    downloading: '下载中',
    paused: '已暂停',
    verifying: '校验中',
    installed: '已安装',
    corrupt: '需要修复',
    failed: '下载失败',
  }[state]
}

function defaultStyle(): SubtitleStyle {
  return {
    font_family: 'PingFang SC',
    font_size: 46,
    font_weight: 500 as unknown as bigint,
    text_color: '#ffffff',
    outline_color: '#111318',
    outline_width: 3,
    shadow_color: '#00000080',
    shadow_offset_x: 0,
    shadow_offset_y: 2,
    shadow_blur: 4,
    background_color: '#11131800',
    background_radius: 8,
    background_padding_x: 10,
    background_padding_y: 6,
    position_x: 0.5,
    position_y: 0.84,
    max_width_ratio: 0.86,
    max_lines: 2 as unknown as bigint,
    target_characters_per_line: 18 as unknown as bigint,
    show_speaker: false,
    cjk_spacing: true,
  }
}

function usePreferences() {
  const [preferences, setPreferences] = useState<api.AppPreferencesV1>(DEFAULT_PREFERENCES)
  const [loading, setLoading] = useState(true)
  const [notice, setNotice] = useState<string | null>(null)

  const reload = useCallback(async () => {
    if (!api.isDesktop) {
      setLoading(false)
      return
    }
    try {
      const result = await api.preferencesGet()
      if (result.status === 'success' && result.data) setPreferences(result.data)
      else setNotice(result.diagnostics[0]?.cause ?? '读取应用设置失败')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '设置窗口暂时无法连接桌面服务')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void reload() }, [reload])

  useEffect(() => {
    if (!api.isDesktop) return
    let disposed = false
    let remove: (() => void) | undefined
    void api.listen<{ changed_keys: string[] }>('dl://preferences-changed', () => {
      if (disposed) return
      void api.preferencesGet().then((result) => {
        if (!disposed && result.status === 'success' && result.data) setPreferences(result.data)
      }).catch(() => undefined)
    }).then((unlisten) => { remove = unlisten }).catch(() => undefined)
    return () => { disposed = true; remove?.() }
  }, [])

  const update = useCallback(async (patch: api.PreferencesPatch) => {
    if (!api.isDesktop) {
      setPreferences((current) => ({ ...current, ...patch }))
      setNotice('浏览器预览：设置未写入桌面应用。')
      return
    }
    try {
      const result = await api.preferencesUpdate(patch)
      if (result.status === 'success' && result.data) setPreferences(result.data)
      else setNotice(result.diagnostics[0]?.cause ?? '设置没有保存')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '设置没有保存')
    }
  }, [])

  return { preferences, loading, notice, setNotice, update, reload }
}

export function SettingsApp({ initialPage = 'general' }: SettingsAppProps) {
  const [page, setPage] = useState<SettingsPage>(initialPage)
  const { preferences, loading, notice, setNotice, update } = usePreferences()
  const [models, setModels] = useState<api.ModelDescriptor[]>([])
  const [modelsLoading, setModelsLoading] = useState(true)
  const [modelBusy, setModelBusy] = useState<string | null>(null)
  const [systemProfile, setSystemProfile] = useState<api.SystemProfile | null>(null)
  const [doctor, setDoctor] = useState<api.DoctorReport | null>(null)
  const [doctorLoading, setDoctorLoading] = useState(false)
  const preview = !api.isDesktop

  const loadModels = useCallback(async () => {
    if (!api.isDesktop) {
      setModels(PREVIEW_MODELS)
      setModelsLoading(false)
      return
    }
    try {
      const result = await api.modelCatalog()
      if (result.status === 'success') setModels(result.data ?? [])
      else setNotice(result.diagnostics[0]?.cause ?? '读取模型清单失败')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '模型服务暂时不可用')
    } finally {
      setModelsLoading(false)
    }
  }, [setNotice])

  useEffect(() => { void loadModels() }, [loadModels])

  useEffect(() => {
    if (!api.isDesktop) return
    let disposed = false
    const removers: Array<() => void> = []
    void Promise.all([
      api.listen<Partial<api.ModelDownloadProgress> & { bytes_downloaded?: number | bigint; bytes_total?: number | bigint }>('dl://model-progress', (event) => {
        if (disposed) return
        const progress = api.normalizeModelProgress(event.payload)
        setModels((current) => current.map((model) => model.id === progress.model_id ? { ...model, state: progress.state, downloaded_bytes: progress.completed_bytes } : model))
      }),
      api.listen<Partial<api.ModelInstallation> & { bytes_downloaded?: number | bigint; bytes_total?: number | bigint }>('dl://model-state', () => {
        if (!disposed) void loadModels()
      }),
    ]).then((unlisten) => removers.push(...unlisten)).catch(() => undefined)
    return () => { disposed = true; removers.forEach((remove) => remove()) }
  }, [loadModels])

  useEffect(() => {
    if (!api.isDesktop) return
    void api.systemProfile().then((result) => {
      if (result.status === 'success') setSystemProfile(result.data ?? null)
    }).catch(() => undefined)
  }, [])

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      document.documentElement.classList.toggle('dark', preferences.theme === 'dark' || (preferences.theme === 'system' && media.matches))
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [preferences.theme])

  const runDoctor = async () => {
    if (!api.isDesktop) {
      setNotice('浏览器预览：诊断需要在桌面应用中运行。')
      return
    }
    setDoctorLoading(true)
    try {
      const result = await api.doctorRun()
      if (result.status === 'success') setDoctor(result.data ?? null)
      else setNotice(result.diagnostics[0]?.cause ?? '诊断没有完成')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '诊断没有完成')
    } finally {
      setDoctorLoading(false)
    }
  }

  const performModelAction = async (model: api.ModelDescriptor, action: 'install' | 'pause' | 'resume' | 'cancel' | 'verify' | 'remove') => {
    if (!api.isDesktop) {
      setNotice('浏览器预览：模型操作需要在桌面应用中执行。')
      return
    }
    setModelBusy(model.id)
    try {
      const result = action === 'install' ? await api.modelInstall(model.id)
        : action === 'pause' ? await api.modelPause(model.id)
          : action === 'resume' ? await api.modelResume(model.id)
            : action === 'cancel' ? await api.modelCancel(model.id)
              : action === 'verify' ? await api.modelVerify(model.id)
                : await api.modelRemove(model.id)
      if (result.status === 'failed') setNotice(result.diagnostics[0]?.cause ?? '模型操作没有完成')
      await loadModels()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '模型操作没有完成')
    } finally {
      setModelBusy(null)
    }
  }

  const resetOnboarding = async () => {
    if (!api.isDesktop) {
      window.dispatchEvent(new CustomEvent('dl://onboarding-reset'))
      setNotice('浏览器预览：下次打开桌面应用时会重新显示引导。')
      return
    }
    try {
      const result = await api.onboardingReset()
      if (result.status === 'success') setNotice('已重新打开新手引导；切回主窗口即可开始。')
      else setNotice(result.diagnostics[0]?.cause ?? '新手引导状态没有重置')
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '新手引导状态没有重置')
    }
  }

  const changeModelRoot = async () => {
    if (!api.isDesktop) {
      setNotice('浏览器预览：模型目录迁移需要在桌面应用中执行。')
      return
    }
    try {
      const picked = await api.pickDirectory('选择新的模型目录', 'model-root')
      if (!picked || Array.isArray(picked)) return
      await update({ model_root: picked })
    } catch (error) {
      setNotice(error instanceof Error ? error.message : '模型目录没有改变')
    }
  }

  const currentStyle = preferences.default_subtitle_style ?? defaultStyle()
  const recommended = systemProfile?.recommended_asr_model ?? 'qwen3-asr-0.6b'
  const modelById = useMemo(() => new Map(models.map((model) => [model.id, model])), [models])

  return (
    <div className="settings-window" aria-label="Double Love Studio 设置">
      <a className="studio-skip-link" href="#settings-main">跳到主要内容</a>
      <div className="settings-layout">
        <aside className="settings-sidebar" aria-label="设置分类">
          <div className="settings-brand">DOUBLE LOVE STUDIO</div>
          <nav>
            {PAGE_ITEMS.map(({ id, label, icon: Icon }) => (
              <button key={id} type="button" className={page === id ? 'is-active' : ''} aria-current={page === id ? 'page' : undefined} onClick={() => setPage(id)}>
                <Icon size={14} strokeWidth={1.8} /><span>{label}</span>
              </button>
            ))}
          </nav>
        </aside>
        <main className="settings-content" id="settings-main" tabIndex={-1}>
          {preview && <div className="settings-preview-banner" role="status">浏览器预览：读取和操作模型、偏好与诊断需要桌面应用。</div>}
          {notice && <div className="settings-inline-notice" role="status" aria-live="polite"><span>{notice}</span><button type="button" aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
          {loading ? <div className="settings-loading" role="status">正在读取设置…</div> : (
            <>
              {page === 'general' && <GeneralPage preferences={preferences} onUpdate={update} onResetOnboarding={() => void resetOnboarding()} />}
              {page === 'shortcuts' && <ShortcutsPage />}
              {page === 'subtitle' && <SubtitlePage style={currentStyle} onUpdate={(style) => update({ default_subtitle_style: style })} onApply={async () => {
                if (!api.isDesktop) { setNotice('浏览器预览：需要在桌面应用中应用到当前项目。'); return }
                try {
                  const result = await api.applyDefaultSubtitleStyle()
                  setNotice(result.status === 'success' ? '已应用到当前项目。' : result.diagnostics[0]?.cause ?? '没有可应用的当前项目')
                } catch (error) {
                  setNotice(error instanceof Error ? error.message : '没有可应用的当前项目')
                }
              }} />}
              {page === 'models' && <ModelsPage models={models} modelsLoading={modelsLoading} modelBusy={modelBusy} recommended={recommended} endpoint={preferences.model_endpoint} modelRoot={preferences.model_root} defaultModel={preferences.default_asr_model} modelById={modelById} onUpdate={update} onAction={performModelAction} onReveal={() => { if (api.isDesktop) void api.modelReveal().catch(() => setNotice('模型目录暂时无法打开')); else setNotice('浏览器预览：模型目录需要在桌面应用中打开。') }} onChangeRoot={() => void changeModelRoot()} />}
              {page === 'privacy' && <PrivacyPage />}
              {page === 'diagnostics' && <DiagnosticsPage doctor={doctor} loading={doctorLoading} onRun={() => void runDoctor()} onReveal={() => { if (api.isDesktop) void api.diagnosticsRevealLogs().catch(() => setNotice('日志目录暂时无法打开')); else setNotice('浏览器预览：日志目录需要在桌面应用中打开。') }} />}
              {page === 'about' && <AboutPage />}
            </>
          )}
        </main>
      </div>
    </div>
  )
}

function PageHeader({ title, description }: { title: string; description: string }) {
  return <header className="settings-page-header"><h1>{title}</h1><p>{description}</p></header>
}

function SettingRow({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return <div className="settings-row"><div><strong>{title}</strong>{description && <small>{description}</small>}</div><div className="settings-row-control">{children}</div></div>
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (value: boolean) => void; label: string }) {
  return <label className="settings-toggle"><input type="checkbox" aria-label={label} checked={checked} onChange={(event) => onChange(event.target.checked)} /><span aria-hidden="true" /></label>
}

function GeneralPage({ preferences, onUpdate, onResetOnboarding }: { preferences: api.AppPreferencesV1; onUpdate: (patch: api.PreferencesPatch) => void; onResetOnboarding: () => void }) {
  return <section className="settings-page" aria-labelledby="general-title">
    <PageHeader title="通用" description="调整应用在这台 Mac 上的工作方式。" />
    <div className="settings-group-title">启动</div>
    <SettingRow title="打开应用时恢复上次项目" description="从最近一次编辑的位置继续。"><Toggle checked={preferences.restore_last_project} label="打开应用时恢复上次项目" onChange={(value) => onUpdate({ restore_last_project: value })} /></SettingRow>
    <SettingRow title="显示新手引导" description="可随时重新查看三步介绍。"><button type="button" className="settings-text-button" onClick={onResetOnboarding}>重新打开</button></SettingRow>
    <div className="settings-group-title">编辑</div>
    <SettingRow title="时间码精度" description="项目和导出预览中的时间显示。"><select aria-label="时间码精度" value={preferences.timecode_precision} onChange={(event) => onUpdate({ timecode_precision: event.target.value as api.TimecodePrecision })}><option value="frame">帧</option><option value="millisecond">毫秒</option></select></SettingRow>
    <SettingRow title="转录分区底色" description="帮助你在文本中定位当前片段。"><Toggle checked={preferences.transcript_section_tint} label="转录分区底色" onChange={(value) => onUpdate({ transcript_section_tint: value })} /></SettingRow>
    <SettingRow title="中日韩文字间距" description="导出字幕时在中日韩字符之间加入可读间距。"><Toggle checked={preferences.cjk_spacing} label="中日韩文字间距" onChange={(value) => onUpdate({ cjk_spacing: value })} /></SettingRow>
    <div className="settings-group-title">界面</div>
    <SettingRow title="外观" description="亮色界面适合长时间剪辑。"><select aria-label="应用主题" value={preferences.theme} onChange={(event) => onUpdate({ theme: event.target.value as api.ThemeMode })}><option value="light">亮色</option><option value="dark">深色</option><option value="system">跟随系统</option></select></SettingRow>
  </section>
}

const SHORTCUTS = [
  ['新建项目', '⌘ N'], ['打开项目', '⌘ O'], ['设置', '⌘ ,'], ['播放 / 暂停', 'Space'], ['前后跳转', '← / →'], ['拆分', 'S'], ['撤销', '⌘ Z'], ['重做', '⇧ ⌘ Z'], ['导出', '⌘ E'],
]

function ShortcutsPage() {
  return <section className="settings-page" aria-labelledby="shortcuts-title"><PageHeader title="快捷键" description="固定快捷键已经接入编辑器；首版不提供改键入口。" /><div className="settings-shortcut-list">{SHORTCUTS.map(([label, key]) => <div key={label} className="settings-shortcut-row"><span>{label}</span><kbd>{key}</kbd></div>)}</div><p className="settings-footnote">快捷键遵循 macOS 习惯。文本输入框获得焦点时，播放和编辑动作不会抢走按键。</p></section>
}

function SubtitlePage({ style, onUpdate, onApply }: { style: SubtitleStyle; onUpdate: (style: SubtitleStyle) => void; onApply: () => void }) {
  const update = (patch: Partial<SubtitleStyle>) => onUpdate({ ...style, ...patch })
  return <section className="settings-page" aria-labelledby="subtitle-title"><PageHeader title="默认字幕样式" description="只影响新建项目；已有项目不会被自动改写。" /><div className="settings-callout"><Wand2 size={15} /><span>当前项目的字幕样式仍在编辑器右侧单独调整。</span></div><div className="settings-group-title">文字</div><SettingRow title="字体" description="使用系统字体，确保中文输入法和导出一致。"><select aria-label="默认字幕字体" value={style.font_family} onChange={(event) => update({ font_family: event.target.value })}><option>PingFang SC</option><option>Hiragino Sans GB</option><option>Helvetica Neue</option></select></SettingRow><SettingRow title="字号" description="以像素为单位。"><input aria-label="默认字幕字号" type="number" min="12" max="160" value={style.font_size} onChange={(event) => update({ font_size: Math.max(12, Number(event.target.value) || style.font_size) })} /></SettingRow><SettingRow title="每行目标字数" description="用于生成新项目的默认换行。"><input aria-label="默认字幕每行目标字数" type="number" min="4" max="80" value={num(style.target_characters_per_line)} onChange={(event) => update({ target_characters_per_line: Math.max(4, Math.round(Number(event.target.value) || num(style.target_characters_per_line))) as unknown as bigint })} /></SettingRow><SettingRow title="显示说话人名称"><Toggle checked={style.show_speaker} label="默认显示说话人名称" onChange={(value) => update({ show_speaker: value })} /></SettingRow><div className="settings-group-title">预览与应用</div><div className="settings-subtitle-preview" style={{ color: style.text_color, fontSize: `${Math.min(32, Math.max(16, style.font_size / 2))}px`, textShadow: `0 1px ${style.outline_color}` }}>这是新项目的字幕预览</div><button type="button" className="settings-secondary-button" onClick={onApply}>在当前项目中应用</button></section>
}

interface ModelsPageProps {
  models: api.ModelDescriptor[]
  modelsLoading: boolean
  modelBusy: string | null
  recommended: string
  endpoint: string
  modelRoot: string
  defaultModel: string
  modelById: Map<string, api.ModelDescriptor>
  onUpdate: (patch: api.PreferencesPatch) => void
  onAction: (model: api.ModelDescriptor, action: 'install' | 'pause' | 'resume' | 'cancel' | 'verify' | 'remove') => void
  onReveal: () => void
  onChangeRoot: () => void
}

function ModelsPage({ models, modelsLoading, modelBusy, recommended, endpoint, modelRoot, defaultModel, modelById, onUpdate, onAction, onReveal, onChangeRoot }: ModelsPageProps) {
  const asrModels = models.filter((model) => model.kind === 'asr')
  const dependencies = models.filter((model) => model.kind !== 'asr')
  return <section className="settings-page" aria-labelledby="models-title"><PageHeader title="本地模型" description="模型权重保存在 Application Support，运行时保持离线。" /><div className="settings-model-recommendation"><div><strong>推荐配置 · {recommended.includes('1.7') ? 'Qwen 1.7B' : 'Qwen 0.6B'}</strong><small>根据设备内存自动推荐，可随时升级</small></div><select aria-label="默认转录模型" value={defaultModel} onChange={(event) => onUpdate({ default_asr_model: event.target.value })}>{asrModels.map((model) => <option key={model.id} value={model.id}>{model.label}</option>)}</select></div><div className="settings-group-title">语音转录</div>{modelsLoading ? <div className="settings-loading-row">正在读取模型清单…</div> : asrModels.map((model) => <ModelRow key={model.id} model={model} busy={modelBusy === model.id} onAction={onAction} />)}<div className="settings-group-title">依赖组件</div>{dependencies.map((model) => <ModelRow key={model.id} model={model} busy={modelBusy === model.id} onAction={onAction} />)}<div className="settings-group-title">存储</div><SettingRow title="下载源" description="固定 revision；只接受 HTTPS 地址。"><input aria-label="模型下载源" defaultValue={endpoint} onBlur={(event) => onUpdate({ model_endpoint: event.target.value })} /></SettingRow><SettingRow title="模型目录" description="更改目录会先复制并校验，成功后才切换。"><span className="settings-mono-value">{modelRoot}</span><button type="button" className="settings-text-button" onClick={onChangeRoot}>更改</button><button type="button" className="settings-text-button" onClick={onReveal}><FolderOpen size={14} />打开目录</button></SettingRow><p className="settings-footnote">Forced Aligner 由 Qwen3 ASR 共享使用，仍有依赖时不能单独删除。</p>{modelById.size === 0 && <p className="settings-footnote">当前没有模型清单。请运行诊断或检查本地运行时。</p>}</section>
}

function ModelRow({ model, busy, onAction }: { model: api.ModelDescriptor; busy: boolean; onAction: ModelsPageProps['onAction'] }) {
  const progress = model.downloaded_bytes != null ? Math.min(100, (num(model.downloaded_bytes) / Math.max(1, num(model.size_bytes))) * 100) : 0
  const primaryAction = model.state === 'not_installed' || model.state === 'failed' || model.state === 'corrupt' ? 'install' : model.state === 'downloading' || model.state === 'queued' ? 'pause' : model.state === 'paused' ? 'resume' : null
  return <article className="settings-model-row"><div className="settings-model-icon"><AudioLines size={15} /></div><div className="settings-model-copy"><strong>{model.label}</strong><small>{model.description ?? `${readableBytes(model.size_bytes)} · revision ${model.revision}`}</small>{(model.state === 'downloading' || model.state === 'paused' || model.state === 'verifying') && <div className="settings-model-progress" aria-label={`${model.label} 下载进度`}><i style={{ width: `${progress}%` }} /></div>}{model.error && <em>{model.error}</em>}</div><span className={`settings-model-state is-${model.state}`}>{stateLabel(model.state)}</span><div className="settings-model-actions">{primaryAction && <button type="button" className={primaryAction === 'install' || primaryAction === 'resume' ? 'settings-primary-button' : 'settings-secondary-button'} disabled={busy} onClick={() => onAction(model, primaryAction)}>{busy ? '处理中…' : primaryAction === 'install' ? '安装' : primaryAction === 'resume' ? '继续' : '暂停'}</button>}{model.state === 'installed' && <button type="button" className="settings-secondary-button" disabled={busy} onClick={() => onAction(model, 'verify')}>{busy ? '校验中…' : '校验'}</button>}{(model.state === 'downloading' || model.state === 'paused' || model.state === 'queued') && <button type="button" className="settings-text-button" disabled={busy} onClick={() => onAction(model, 'cancel')}>取消</button>}{model.state === 'installed' && model.can_remove !== false && <button type="button" className="settings-icon-action" aria-label={`删除${model.label}`} disabled={busy} onClick={() => onAction(model, 'remove')}><Trash2 size={14} /></button>}{model.state === 'installed' && model.can_remove === false && <span className="settings-cannot-remove">不可移除</span>}</div></article>
}

function PrivacyPage() {
  return <section className="settings-page" aria-labelledby="privacy-title"><PageHeader title="隐私" description="Double Love 的默认工作方式是本地优先。" /><div className="settings-privacy-list"><div><LockKeyhole size={16} /><div><strong>没有默认遥测</strong><p>应用不会自动发送使用数据、崩溃报告或项目路径。</p></div></div><div><AudioLines size={16} /><div><strong>音频和声纹不上传</strong><p>转录、说话人识别和声纹只在本机运行并保存在项目中。</p></div></div><div><Wand2 size={16} /><div><strong>Agent 数据包需要预览确认</strong><p>外部名称建议只允许你查看并确认最小匿名文本。</p></div></div><div><FolderOpen size={16} /><div><strong>本地数据</strong><p>偏好、模型和日志都位于你的 Mac。打开目录前会由系统确认。</p></div></div></div></section>
}

function DiagnosticsPage({ doctor, loading, onRun, onReveal }: { doctor: api.DoctorReport | null; loading: boolean; onRun: () => void; onReveal: () => void }) {
  const mediaReady = doctor?.ffmpeg === '可用' && doctor.libass === '可用'
  const runtimeReady = doctor ? !doctor.offline_runtime.includes('不可用') : false
  return <section className="settings-page" aria-labelledby="diagnostics-title"><PageHeader title="诊断" description="检查离线运行环境、模型完整性和可用空间。" /><div className="settings-diagnostics-actions"><button type="button" className="settings-primary-button" disabled={loading} onClick={onRun}><RefreshCw size={14} />{loading ? '检查中…' : '运行诊断'}</button><button type="button" className="settings-secondary-button" onClick={onReveal}><FolderOpen size={14} />打开日志目录</button></div>{doctor ? <div className="settings-doctor-report"><SettingRow title="应用与系统" description={`${doctor.app_version} · ${doctor.architecture} · ${doctor.os_version}`}><Check size={15} className="settings-ok" /></SettingRow><SettingRow title="ffmpeg / libass" description={`${doctor.ffmpeg} / ${doctor.libass}`}>{mediaReady ? <Check size={15} className="settings-ok" /> : <AlertTriangle size={15} className="settings-warning" />}</SettingRow><SettingRow title="离线运行时" description={doctor.offline_runtime}>{runtimeReady ? <Check size={15} className="settings-ok" /> : <AlertTriangle size={15} className="settings-warning" />}</SettingRow><SettingRow title="可用磁盘" description={readableBytes(doctor.free_disk_bytes)}><span className="settings-mono-value">{readableBytes(doctor.free_disk_bytes)}</span></SettingRow><div className="settings-model-integrity">{doctor.model_integrity.map((model) => <div key={model.model_id}><span>{model.model_id}</span><strong>{stateLabel(model.state as api.ModelInstallState) ?? model.state}</strong></div>)}</div></div> : <div className="settings-empty-state"><ClipboardCheck size={18} /><strong>还没有诊断报告</strong><p>运行一次检查，结果只保存在本地。</p></div>}</section>
}

function AboutPage() {
  return <section className="settings-page" aria-labelledby="about-title"><PageHeader title="关于" description="Double Love Studio · 本地粗剪工作台。" /><div className="settings-about-mark"><span>⌃</span><div><strong>Double Love Studio</strong><small>版本 0.1.0 · Apple Silicon</small></div></div><div className="settings-about-list"><SettingRow title="本地处理" description="原始媒体不会被复制，模型运行时不主动联网。"><Check className="settings-ok" size={15} /></SettingRow><SettingRow title="模型许可" description="Qwen3 ASR、Forced Aligner、Silero VAD 和 WeSpeaker 的许可随安装清单提供。"><ChevronRight size={15} /></SettingRow><SettingRow title="第三方许可" description="查看构建中使用的开源组件和版本。"><ChevronRight size={15} /></SettingRow></div><p className="settings-footnote">感谢你用本地工具整理声音和故事。问题反馈请附上诊断页中经过脱敏的报告。</p></section>
}

export { PAGE_ITEMS }
