interface ScrubStripProps {
  playhead: number
}

export function ScrubStrip({ playhead }: ScrubStripProps) {
  return (
    <div className="h-12 flex-none mx-3 mb-2 relative">
      <div className="h-full w-full flex gap-1">
        {Array.from({ length: 12 }, (_, i) => (
          <div
            key={i}
            className="flex-1 h-full rounded-sm bg-black/15 dark:bg-white/15 border border-line"
          />
        ))}
      </div>
      <div
        className="absolute top-0 bottom-0 w-0.5 bg-playhead"
        style={{ left: `${playhead * 100}%` }}
      />
    </div>
  )
}
