import { useState } from 'react'
import type { ReactNode } from 'react'
import { ArrowRight, Check, Download, FolderOpen, HardDrive, LockKeyhole, ShieldCheck } from 'lucide-react'
import type { ModelDescriptor, SystemProfile } from '../platform/desktop'

interface OnboardingProps {
  recommendedModel: string
  systemProfile: SystemProfile | null
  models: ModelDescriptor[]
  installingModel: string | null
  onInstallModel: (modelId: string) => void
  onCreateProject: () => void
  onOpenProject: () => void
  onSkip: () => void
  onFinish: () => void
}

const STEP_ITEMS = [
  ['1', '本地处理'],
  ['2', '安装转录模型'],
  ['3', '创建第一个项目'],
]

function readableBytes(value: number | bigint): string {
  const bytes = typeof value === 'bigint' ? Number(value) : value
  if (!Number.isFinite(bytes) || bytes <= 0) return '—'
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`
  return `${Math.round(bytes / 1024 ** 2)} MB`
}

export function Onboarding({ recommendedModel, systemProfile, models, installingModel, onInstallModel, onCreateProject, onOpenProject, onSkip, onFinish }: OnboardingProps) {
  const [step, setStep] = useState(1)
  const recommended = models.find((model) => model.id === recommendedModel) ?? models.find((model) => model.kind === 'asr')
  const aligner = models.find((model) => model.kind === 'aligner')

  return <div className="onboarding-shell" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
    <section className="onboarding-card">
      <header className="onboarding-card-header"><strong><i />开始使用 Double Love</strong><button type="button" onClick={onSkip}>跳过引导</button></header>
      <div className="onboarding-body">
        <aside className="onboarding-steps" aria-label="引导步骤"><strong>三步完成设置</strong>{STEP_ITEMS.map(([number, label], index) => <button key={number} type="button" className={step === index + 1 ? 'is-active' : ''} onClick={() => setStep(index + 1)}><span>{number}</span><b>{label}</b></button>)}<p>随时可以跳过。模型下载会在后台继续，不会阻塞你打开项目。</p></aside>
        <main className="onboarding-content">
          {step === 1 && <WelcomeStep onContinue={() => setStep(2)} />}
          {step === 2 && <ModelStep recommended={recommended} aligner={aligner} systemProfile={systemProfile} installingModel={installingModel} onInstall={onInstallModel} onContinue={() => setStep(3)} />}
          {step === 3 && <ProjectStep onCreate={onCreateProject} onOpen={onOpenProject} onFinish={onFinish} />}
        </main>
      </div>
      <footer className="onboarding-footer"><span>{step === 3 ? '也可以稍后从项目库继续。' : '你可以在设置中重新打开新手引导。'}</span><div>{step < 3 && <button type="button" className="onboarding-secondary" onClick={onSkip}>稍后再做</button>}{step < 3 ? <button type="button" className="onboarding-primary" onClick={() => setStep(step + 1)}>继续 <ArrowRight size={15} /></button> : <button type="button" className="onboarding-primary" onClick={onFinish}>进入项目库 <ArrowRight size={15} /></button>}</div></footer>
    </section>
  </div>
}

function WelcomeStep({ onContinue }: { onContinue: () => void }) {
  return <div className="onboarding-step-content"><span className="onboarding-kicker"><i />第 1 步 · 关于你的素材</span><h1 id="onboarding-title">视频留在你的 Mac 上。</h1><p className="onboarding-lead">Double Love 在本机完成转录、说话人分离和粗剪。原始媒体不会被复制，音频和声纹也不会默认离开这台电脑。</p><div className="onboarding-illustration"><div className="onboarding-window"><i /><b /><b /><b /><b /><span><LockKeyhole size={18} /></span></div><div className="onboarding-points"><Point icon={<ShieldCheck size={15} />} title="离线也能工作" copy="模型和媒体在本机运行，飞机上或片场都可以继续。" /><Point icon={<Check size={15} />} title="不改动原始文件" copy="项目只保存转录、剪辑记录和输出映射。" /><Point icon={<HardDrive size={15} />} title="每一步都能撤回" copy="转录、命名和粗剪都会保留本地版本历史。" /></div></div><button type="button" className="onboarding-inline-link" onClick={onContinue}>查看模型推荐 <ArrowRight size={14} /></button></div>
}

function ModelStep({ recommended, aligner, systemProfile, installingModel, onInstall, onContinue }: { recommended?: ModelDescriptor; aligner?: ModelDescriptor; systemProfile: SystemProfile | null; installingModel: string | null; onInstall: (modelId: string) => void; onContinue: () => void }) {
  return <div className="onboarding-step-content"><span className="onboarding-kicker"><i />第 2 步 · 设备上的模型</span><h1>先准备一套本地转录模型。</h1><p className="onboarding-lead">我们按设备内存给出建议。安装后可以完全离线运行，也可以跳过，在设置中稍后继续。</p><div className="onboarding-model-summary"><div><strong>{recommended?.label ?? 'Qwen3 ASR · 0.6B'}</strong><small>{recommended ? `${readableBytes(recommended.size_bytes)} · ${recommended.description ?? '适合大多数 Mac'}` : '模型清单将在桌面应用中读取'}</small></div><span>{systemProfile ? `${readableBytes(systemProfile.memory_bytes)} 内存` : '正在检测内存'}</span></div>{aligner && <div className="onboarding-dependency"><Download size={15} /><span>安装 ASR 时会自动带上 {aligner.label}，用于生成逐词时间。</span></div>}<div className="onboarding-model-actions"><button type="button" className="onboarding-primary" disabled={!recommended || installingModel !== null} onClick={() => recommended && onInstall(recommended.id)}>{installingModel === recommended?.id ? '准备中…' : recommended?.state === 'installed' ? '已安装' : '安装推荐'} <ArrowRight size={15} /></button><button type="button" className="onboarding-secondary" onClick={onContinue}>跳过安装</button></div></div>
}

function ProjectStep({ onCreate, onOpen, onFinish }: { onCreate: () => void; onOpen: () => void; onFinish: () => void }) {
  return <div className="onboarding-step-content"><span className="onboarding-kicker"><i />第 3 步 · 开始工作</span><h1>把第一个项目放在手边。</h1><p className="onboarding-lead">项目文件夹只保存转录、说话人和剪辑记录，原始视频始终留在原来的位置。</p><div className="onboarding-project-actions"><button type="button" className="onboarding-action-card" onClick={onCreate}><span><FolderOpen size={17} /></span><div><strong>新建本地项目</strong><small>选择一个文件夹保存项目数据</small></div><ArrowRight size={15} /></button><button type="button" className="onboarding-action-card" onClick={onOpen}><span><HardDrive size={17} /></span><div><strong>打开已有项目</strong><small>继续上一次编辑的位置</small></div><ArrowRight size={15} /></button></div><button type="button" className="onboarding-inline-link" onClick={onFinish}>先进入项目库 <ArrowRight size={14} /></button></div>
}

function Point({ icon, title, copy }: { icon: ReactNode; title: string; copy: string }) {
  return <div className="onboarding-point"><span>{icon}</span><div><strong>{title}</strong><small>{copy}</small></div></div>
}
