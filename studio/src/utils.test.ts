import { describe, expect, it } from 'vitest'
import { aozoraDiary } from './fixtures'
import {
  TIMELINE_LEFT_INSET,
  TIMELINE_RIGHT_INSET,
  countsConsistent,
  exportBlockMessage,
  formatClock,
  playheadClock,
  ratingLabel,
  seekFractionFromClientX,
} from './utils'

describe('播放头时钟', () => {
  it('格式化 秒 → mm:ss', () => {
    expect(formatClock(0)).toBe('00:00')
    expect(formatClock(77.14)).toBe('01:17')
    expect(formatClock(120)).toBe('02:00')
  })

  it('播放头时钟含总长', () => {
    expect(playheadClock(0)).toBe('00:00 / 02:00')
    expect(playheadClock(0.35)).toBe('00:42 / 02:00')
    expect(playheadClock(1)).toBe('02:00 / 02:00')
  })
})

describe('seek 换算', () => {
  const left = 100
  const width = 500
  const usable = width - TIMELINE_LEFT_INSET - TIMELINE_RIGHT_INSET

  it('命中可用区两端', () => {
    expect(seekFractionFromClientX(left + TIMELINE_LEFT_INSET, left, width)).toBe(0)
    expect(seekFractionFromClientX(left + width - TIMELINE_RIGHT_INSET, left, width)).toBe(1)
    expect(seekFractionFromClientX(left + TIMELINE_LEFT_INSET + usable / 2, left, width)).toBe(0.5)
  })

  it('越界 clamp 到 0..1', () => {
    expect(seekFractionFromClientX(0, left, width)).toBe(0)
    expect(seekFractionFromClientX(9999, left, width)).toBe(1)
  })

  it('可用区为 0 时不产生 NaN', () => {
    expect(seekFractionFromClientX(left, left, TIMELINE_LEFT_INSET + TIMELINE_RIGHT_INSET)).toBe(0)
  })
})

describe('合成数据集一致性', () => {
  it('total = processed + ignored + skipped + failed', () => {
    expect(countsConsistent(aozoraDiary.counts)).toBe(true)
    expect(aozoraDiary.clips.length).toBe(aozoraDiary.counts.total)
  })

  it('计数与片段状态逐条对应', () => {
    const clips = aozoraDiary.clips
    const byStatus = (s: string) => clips.filter((c) => c.status === s).length
    expect(byStatus('processed')).toBe(aozoraDiary.counts.processed)
    expect(byStatus('ignored')).toBe(aozoraDiary.counts.ignored)
    expect(byStatus('skipped')).toBe(aozoraDiary.counts.skipped)
    expect(byStatus('failed')).toBe(aozoraDiary.counts.failed)
  })
})

describe('评分标签映射', () => {
  it('Circle→ok / KEEP→kp / NG→ng / 无→—', () => {
    expect(ratingLabel('ok')).toBe('ok')
    expect(ratingLabel('keep')).toBe('kp')
    expect(ratingLabel('ng')).toBe('ng')
    expect(ratingLabel('none')).toBe('—')
  })
})

describe('导出阻断提示', () => {
  it('有阻断诊断时给出代码与对象', () => {
    const message = exportBlockMessage(aozoraDiary.diagnostics)
    expect(message).toContain('SHOTTAKE_INVALID')
    expect(message).toContain('c21')
  })

  it('无阻断诊断时为 null', () => {
    expect(exportBlockMessage([])).toBeNull()
  })
})
