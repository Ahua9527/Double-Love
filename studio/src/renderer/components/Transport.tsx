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
    <div className="studio-transport">
      <button
        type="button"
        disabled={disabled}
        onClick={() => onSkip(-5)}
        className="studio-transport-skip"
      >
        <SkipBack size={12} />
        后退 5 秒
      </button>
      <button
        type="button"
        aria-label={playing ? '暂停' : '播放'}
        disabled={disabled}
        onClick={onTogglePlay}
        className="studio-play-button"
      >
        {playing ? <Pause size={14} /> : <Play size={14} className="ml-0.5" />}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => onSkip(5)}
        className="studio-transport-skip"
      >
        前进 5 秒
        <SkipForward size={12} />
      </button>
      <span className="studio-transport-clock">{clock}</span>
    </div>
  )
}
