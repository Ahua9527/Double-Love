import { Pause, Play, SkipBack, SkipForward } from 'lucide-react'

interface TransportProps {
  playing: boolean
  clock: string
  /** 无资产时禁用全部按钮 */
  disabled: boolean
  onTogglePlay: () => void
  onSkip: (deltaSec: number) => void
}

export function Transport({ playing, clock, disabled, onTogglePlay, onSkip }: TransportProps) {
  return (
    <div className="h-9 flex-none mx-3 flex items-center justify-center gap-4">
      <button
        type="button"
        disabled={disabled}
        onClick={() => onSkip(-5)}
        className="flex items-center gap-1 text-xs text-mutedfg hover:text-fg disabled:opacity-40 disabled:hover:text-mutedfg"
      >
        <SkipBack size={12} />
        后退 5 秒
      </button>
      <button
        type="button"
        aria-label={playing ? '暂停' : '播放'}
        disabled={disabled}
        onClick={onTogglePlay}
        className="w-8 h-8 rounded-full bg-selected hover:bg-selected/85 flex items-center justify-center text-white disabled:opacity-40"
      >
        {playing ? <Pause size={14} /> : <Play size={14} className="ml-0.5" />}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => onSkip(5)}
        className="flex items-center gap-1 text-xs text-mutedfg hover:text-fg disabled:opacity-40 disabled:hover:text-mutedfg"
      >
        前进 5 秒
        <SkipForward size={12} />
      </button>
      <span className="text-xs font-mono text-mutedfg">{clock}</span>
    </div>
  )
}
