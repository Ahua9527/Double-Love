import { describe, expect, it } from 'vitest'
import { getOutputFileName } from '../utils/download'

describe('getOutputFileName', () => {
  it('为普通 XML 扩展名生成唯一的下载名', () => {
    expect(getOutputFileName('shot.xml')).toBe('shot_Double_LOVE.xml')
  })

  it('为大写 XML 扩展名保留一个规范的小写扩展名', () => {
    expect(getOutputFileName('shot.XML')).toBe('shot_Double_LOVE.xml')
    expect(getOutputFileName('shot.xMl')).toBe('shot_Double_LOVE.xml')
  })

  it('只移除最后一个 XML 扩展名', () => {
    expect(getOutputFileName('day.v1.XML')).toBe('day.v1_Double_LOVE.xml')
    expect(getOutputFileName('day.xml.backup')).toBe('day.xml.backup_Double_LOVE.xml')
  })

  it('没有扩展名时仍生成统一的 XML 下载名', () => {
    expect(getOutputFileName('shot')).toBe('shot_Double_LOVE.xml')
  })

  it('同名文件使用稳定的重复编号', () => {
    expect(getOutputFileName('shot.xml', 2)).toBe('shot_Double_LOVE_2.xml')
  })
})
