import type { RefObject } from 'react'

interface PreviewHeroProps {
  /** media://localhost/<assetId>；null = 未选择资产 */
  src: string | null
  label: string | null
  videoRef: RefObject<HTMLVideoElement>
  onTimeUpdate: (seconds: number) => void
  onPlayState: (playing: boolean) => void
}

export function PreviewHero({ src, label, videoRef, onTimeUpdate, onPlayState }: PreviewHeroProps) {
  if (!src) {
    return (
      <div className="h-56 flex-none m-3 mb-2 rounded-md bg-black flex flex-col items-center justify-center gap-1">
        <div className="text-sm text-white/60">尚未选择媒体</div>
        <div className="text-xs text-white/40">从左侧资产列表选择，或点「导入…」添加本地视频</div>
      </div>
    )
  }
  return (
    <div className="h-56 flex-none m-3 mb-2 rounded-md bg-black relative overflow-hidden">
      <video
        ref={videoRef}
        src={src}
        preload="metadata"
        className="h-full w-full object-contain"
        onTimeUpdate={(event) => onTimeUpdate(event.currentTarget.currentTime)}
        onPlay={() => onPlayState(true)}
        onPause={() => onPlayState(false)}
        onEnded={() => onPlayState(false)}
      />
      {label && (
        <div className="absolute left-2 bottom-1.5 px-1.5 rounded-sm bg-black/60 text-xs font-mono text-white/80 truncate max-w-[80%]">
          {label}
        </div>
      )}
    </div>
  )
}
