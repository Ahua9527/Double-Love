import { Pause, Play, SkipBack, SkipForward } from 'lucide-react'
import { Tooltip } from './Tooltip'

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
      <Tooltip label="后退 5 秒"><button
        type="button"
        aria-label="后退 5 秒"
        disabled={disabled}
        onClick={() => onSkip(-5)}
        className="studio-transport-skip"
      >
        <SkipBack size={15} />
      </button></Tooltip>
      <Tooltip label={playing ? '暂停' : '播放'}><button
        type="button"
        aria-label={playing ? '暂停' : '播放'}
        disabled={disabled}
        onClick={onTogglePlay}
        className="studio-play-button"
      >
        {playing ? <Pause size={14} /> : <Play size={14} className="ml-0.5" />}
      </button></Tooltip>
      <Tooltip label="前进 5 秒"><button
        type="button"
        aria-label="前进 5 秒"
        disabled={disabled}
        onClick={() => onSkip(5)}
        className="studio-transport-skip"
      >
        <SkipForward size={15} />
      </button></Tooltip>
      <span className="studio-transport-clock">{clock}</span>
    </div>
  )
}
