import type { FixtureClip } from '../fixtures'

export function PreviewHero({ clip }: { clip: FixtureClip }) {
  return (
    <div className="h-56 flex-none m-3 mb-2 rounded-md bg-black flex flex-col items-center justify-center gap-1">
      <div className="text-2xl font-semibold font-mono text-white/95">{clip.newName}</div>
      <div className="text-sm font-mono text-white/60">
        {clip.tcIn} ｜ {clip.duration} ｜ 3840×2160 · 25fps
      </div>
      <div className="mt-2 text-xs text-white/60">预览画面占位 —— 真实解码属后续迭代</div>
    </div>
  )
}
