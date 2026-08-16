import { Play, SkipBack, SkipForward } from 'lucide-react'

interface TransportProps {
  clock: string
  onNotice: (message: string) => void
}

export function Transport({ clock, onNotice }: TransportProps) {
  return (
    <div className="h-9 flex-none mx-3 flex items-center justify-center gap-4">
      <button
        type="button"
        onClick={() => onNotice('镜头导航属后续迭代')}
        className="flex items-center gap-1 text-xs text-mutedfg hover:text-fg"
      >
        <SkipBack size={12} />
        上一镜
      </button>
      <button
        type="button"
        aria-label="播放"
        onClick={() => onNotice('播放预览属后续迭代')}
        className="w-8 h-8 rounded-full bg-selected hover:bg-selected/85 flex items-center justify-center text-white"
      >
        <Play size={14} className="ml-0.5" />
      </button>
      <button
        type="button"
        onClick={() => onNotice('镜头导航属后续迭代')}
        className="flex items-center gap-1 text-xs text-mutedfg hover:text-fg"
      >
        下一镜
        <SkipForward size={12} />
      </button>
      <span className="text-xs font-mono text-mutedfg">{clock}</span>
    </div>
  )
}
