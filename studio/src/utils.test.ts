import { describe, expect, it } from 'vitest'
import type { Diagnostic } from '../../bindings/Diagnostic'
import type { EditOperation } from '../../bindings/EditOperation'
import type { WordAnchor } from '../../bindings/WordAnchor'
import {
  PANEL_STORAGE_KEY,
  TIMELINE_LEFT_INSET,
  TIMELINE_RIGHT_INSET,
  clampSeconds,
  exportBlockMessage,
  formatClock,
  loadPanelState,
  omitRangesToSeconds,
  playheadClock,
  rulerTicks,
  savePanelState,
  seekFractionFromClientX,
} from './utils'

describe('播放头时钟', () => {
  it('格式化 秒 → mm:ss', () => {
    expect(formatClock(0)).toBe('00:00')
    expect(formatClock(77.14)).toBe('01:17')
    expect(formatClock(120)).toBe('02:00')
  })

  it('超过 1 小时 → h:mm:ss', () => {
    expect(formatClock(3600)).toBe('1:00:00')
    expect(formatClock(3661.9)).toBe('1:01:01')
  })

  it('播放头时钟含总长且 clamp 到总长', () => {
    expect(playheadClock(0, 120)).toBe('00:00 / 02:00')
    expect(playheadClock(42.5, 120)).toBe('00:42 / 02:00')
    expect(playheadClock(120, 120)).toBe('02:00 / 02:00')
    expect(playheadClock(999, 120)).toBe('02:00 / 02:00')
  })

  it('clampSeconds 边界', () => {
    expect(clampSeconds(-1, 120)).toBe(0)
    expect(clampSeconds(999, 120)).toBe(120)
    expect(clampSeconds(5, 0)).toBe(0)
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

describe('刻度尺', () => {
  it('空时长返回占位 [0]', () => {
    expect(rulerTicks(0)).toEqual([0])
  })

  it('短素材用秒级步长', () => {
    expect(rulerTicks(30)).toEqual([0, 5, 10, 15, 20, 25, 30])
  })

  it('长素材步长自动放大且数量受控', () => {
    const ticks = rulerTicks(3600)
    expect(ticks[0]).toBe(0)
    expect(ticks[ticks.length - 1]).toBe(3600)
    expect(ticks.length).toBeLessThanOrEqual(9)
    // 步长相等
    const step = ticks[1] - ticks[0]
    for (let i = 1; i < ticks.length; i++) {
      expect(ticks[i] - ticks[i - 1]).toBe(step)
    }
  })

  it('不能被整除的时长刻度不超过总长', () => {
    const ticks = rulerTicks(95)
    for (const tick of ticks) expect(tick).toBeLessThanOrEqual(95)
  })
})

function word(ordinal: number, startSample: number, endSample: number): WordAnchor {
  return {
    word_id: `w${ordinal}`,
    asset_id: 'a1',
    ordinal: BigInt(ordinal),
    raw_text: `词${ordinal}`,
    display_text: `词${ordinal}`,
    language: 'zh',
    start_sample: BigInt(startSample),
    end_sample: BigInt(endSample),
    confidence: 0.99,
    synthetic: false,
    source_word_ids: null,
  }
}

function omit(start: number, end: number): EditOperation {
  return {
    id: `op${start}`,
    asset_id: 'a1',
    edit_type: 'omit',
    behavior: 'ripple_av',
    start_ordinal: BigInt(start),
    end_ordinal: BigInt(end),
    handles_before_ms: BigInt(120),
    handles_after_ms: BigInt(120),
    superseded_by: null,
    revision: BigInt(1),
    created_at: '2026-08-17',
  }
}

describe('omit 区间换算', () => {
  // 48kHz：词0 [0, 48000)，词1 [48000, 96000)，词2 [96000, 144000)
  const words = [word(0, 0, 48_000), word(1, 48_000, 96_000), word(2, 96_000, 144_000)]

  it('词序区间映射为秒区间并排序', () => {
    const ranges = omitRangesToSeconds(words, [omit(2, 2), omit(0, 0)], 48_000)
    expect(ranges).toEqual([
      [0, 1],
      [2, 3],
    ])
  })

  it('端点词缺失（腐烂编辑）跳过该区间', () => {
    expect(omitRangesToSeconds(words, [omit(1, 9)], 48_000)).toEqual([])
  })

  it('采样率为 0 不产出区间', () => {
    expect(omitRangesToSeconds(words, [omit(0, 0)], 0)).toEqual([])
  })
})

describe('导出阻断提示', () => {
  const blocking: Diagnostic = {
    level: 'error',
    code: 'ROUGH_CUT_EMPTY',
    cause: '粗剪为空',
    object_id: 'a1',
    impact: '无内容可导出',
    blocks_export: true,
    suggested_action: null,
  }

  it('有阻断诊断时给出代码与对象', () => {
    const message = exportBlockMessage([blocking])
    expect(message).toContain('ROUGH_CUT_EMPTY')
    expect(message).toContain('a1')
  })

  it('无阻断诊断时为 null', () => {
    expect(exportBlockMessage([])).toBeNull()
  })
})

describe('面板状态持久化', () => {
  function memoryStorage(initial?: string) {
    let value = initial ?? null
    return {
      getItem: () => value,
      setItem: (_key: string, next: string) => {
        value = next
      },
    }
  }

  it('无存储时回退默认（全部展开）', () => {
    expect(loadPanelState(memoryStorage())).toEqual({ left: true, right: true, bottom: true })
  })

  it('坏 JSON 回退默认', () => {
    expect(loadPanelState(memoryStorage('{oops'))).toEqual({
      left: true,
      right: true,
      bottom: true,
    })
  })

  it('字段类型损坏回退默认', () => {
    expect(loadPanelState(memoryStorage('{"left":"yes","right":true,"bottom":true}'))).toEqual({
      left: true,
      right: true,
      bottom: true,
    })
    expect(loadPanelState(memoryStorage('{"left":false}'))).toEqual({
      left: true,
      right: true,
      bottom: true,
    })
  })

  it('合法值原样返回，保存后可再读出', () => {
    const storage = memoryStorage()
    savePanelState(storage, { left: false, right: true, bottom: false })
    expect(loadPanelState(storage)).toEqual({ left: false, right: true, bottom: false })
  })

  it('保存使用固定 key', () => {
    const writes: string[] = []
    savePanelState(
      { setItem: (key: string) => writes.push(key) },
      { left: true, right: true, bottom: true },
    )
    expect(writes).toEqual([PANEL_STORAGE_KEY])
  })
})
