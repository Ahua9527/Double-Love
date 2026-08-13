// xml.test.ts - xml.ts 核心逻辑单元测试
import { describe, it, expect, vi } from 'vitest'
import {
  processXML,
  parseCSVForSeasonEpisode,
  normalizeMatchKey,
  isStrictPositiveInteger,
  getCameraIdentifier,
  generateNewName,
  getRatingFromLabels,
  processClipData,
  updateResolution,
  type XMLProcessConfig,
} from './xml'
import {
  SYNTHETIC_CLIP_ID,
  SYNTHETIC_PREMIERE_XML,
} from '../test/fixtures/webMaintenance'

// 构造带 text() 方法的伪 File（避免 jsdom File 兼容差异）
function makeFile(xmlStr: string): File {
  return { text: async () => xmlStr } as unknown as File
}

function makeDoc(xmlStr: string): Document {
  return new DOMParser().parseFromString(xmlStr, 'text/xml')
}

// 测试用迷你 Premiere Pro XML：仅含合成字段，不使用生产素材。
const SAMPLE_XML = SYNTHETIC_PREMIERE_XML

describe('parseCSVForSeasonEpisode', () => {
  it('空输入返回空映射', () => {
    const { seasonMap, episodeMap } = parseCSVForSeasonEpisode('')
    expect(seasonMap.size).toBe(0)
    expect(episodeMap.size).toBe(0)
  })

  it('正常解析 Name/Season/Episode 列', () => {
    const { seasonMap, episodeMap } = parseCSVForSeasonEpisode(
      'Name,Season,Episode\nxxx.mxf,1,44\n'
    )
    expect(seasonMap.get('xxx')).toBe('01')
    expect(episodeMap.get('xxx')).toBe('44')
  })

  it('支持 BOM、CRLF、引号和字段内逗号，并统一匹配键', () => {
    const { seasonMap, episodeMap, diagnostics } = parseCSVForSeasonEpisode(
      '\uFEFFName,Season,Episode,Note\r\n"Folder\\Clip.XML", 2, 7, "contains, comma"\r\n'
    )
    expect(seasonMap.get('clip')).toBe('02')
    expect(episodeMap.get('clip')).toBe('07')
    expect(diagnostics).toEqual([])
  })

  it('拒绝缺少 Name 列或非法 Season/Episode', () => {
    const missingName = parseCSVForSeasonEpisode('Foo,Bar\n1,2\n')
    expect(missingName.seasonMap.size).toBe(0)
    expect(missingName.episodeMap.size).toBe(0)
    expect(missingName.diagnostics[0]?.code).toBe('CSV_NAME_COLUMN_MISSING')

    const invalidValues = parseCSVForSeasonEpisode('Name,Season,Episode\nclip.mxf,0,1920abc\n')
    expect(invalidValues.seasonMap.size).toBe(0)
    expect(invalidValues.episodeMap.size).toBe(0)
    expect(invalidValues.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'CSV_SEASON_INVALID',
      'CSV_EPISODE_INVALID',
    ])

    const paddedValues = parseCSVForSeasonEpisode('Name,Season,Episode\nclip.mxf,01,0044\n')
    expect(paddedValues.seasonMap.get('clip')).toBe('01')
    expect(paddedValues.episodeMap.get('clip')).toBe('44')
  })

  it('拒绝重复表头，避免同名列被静默选中', () => {
    const result = parseCSVForSeasonEpisode(
      'Name,Episode,episode\nclip.mxf,1,2\n'
    )

    expect(result.seasonMap.size).toBe(0)
    expect(result.episodeMap.size).toBe(0)
    expect(result.diagnostics).toEqual([
      expect.objectContaining({
        level: 'error',
        code: 'CSV_HEADER_DUPLICATE',
        blocksDownload: true,
      }),
    ])
  })

  it('拒绝列数与表头不一致的行并报告行号', () => {
    const result = parseCSVForSeasonEpisode(
      'Name,Season,Episode\nvalid.mxf,1,2\nbroken.mxf,3\n'
    )

    expect(result.seasonMap.size).toBe(0)
    expect(result.episodeMap.size).toBe(0)
    expect(result.diagnostics).toEqual([
      expect.objectContaining({
        level: 'error',
        code: 'CSV_COLUMN_COUNT_MISMATCH',
        message: expect.stringContaining('第 3 行'),
        blocksDownload: true,
      }),
    ])
  })

  it('只接受 1 到 99，并显式报告空值和非法 Season/Episode', () => {
    const result = parseCSVForSeasonEpisode(
      [
        'Name,Season,Episode',
        'blank.mxf,,',
        'large.mxf,100,100',
        'negative.mxf,-1,-1',
        'fraction.mxf,1.5,1e2',
      ].join('\n')
    )

    expect(result.seasonMap.size).toBe(0)
    expect(result.episodeMap.size).toBe(0)
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'CSV_SEASON_EMPTY',
      'CSV_EPISODE_EMPTY',
      'CSV_SEASON_INVALID',
      'CSV_EPISODE_INVALID',
      'CSV_SEASON_INVALID',
      'CSV_EPISODE_INVALID',
      'CSV_SEASON_INVALID',
      'CSV_EPISODE_INVALID',
    ])
    expect(result.diagnostics.every(diagnostic => /第 [2-5] 行/.test(diagnostic.message))).toBe(true)
    expect(
      result.diagnostics
        .filter(diagnostic => diagnostic.code.endsWith('_INVALID'))
        .every(diagnostic => diagnostic.level === 'error' && diagnostic.blocksDownload === true)
    ).toBe(true)
  })

  it('解析转义引号和引号内换行时不打乱后续列', () => {
    const result = parseCSVForSeasonEpisode(
      'Name,Episode,Note\r\n"clip.mxf",7,"第一行\n第二行写着 ""OK"""\r\n'
    )

    expect(result.episodeMap.get('clip')).toBe('07')
    expect(result.diagnostics).toEqual([])
  })

  it('重复匹配键采用最后一行并给出确定诊断', () => {
    const result = parseCSVForSeasonEpisode(
      'Name,Season,Episode\nfolder/CLIP.MXF,1,2\nclip.xml,3,4\n'
    )

    expect(result.seasonMap.get('clip')).toBe('03')
    expect(result.episodeMap.get('clip')).toBe('04')
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'CSV_SEASON_DUPLICATE',
      'CSV_EPISODE_DUPLICATE',
    ])
  })
})

describe('normalizeMatchKey', () => {
  it('统一扩展名、大小写和路径', () => {
    expect(normalizeMatchKey('/media/Shot.XML')).toBe('shot')
    expect(normalizeMatchKey('SHOT.mxf')).toBe('shot')
  })

  it('保留 basename 中的点，只移除常见媒体扩展名', () => {
    expect(normalizeMatchKey('D:\\media\\Shot.001.MOV')).toBe('shot.001')
    expect(normalizeMatchKey('Shot.001')).toBe('shot.001')
  })
})

describe('isStrictPositiveInteger', () => {
  it('只接受正整数数值', () => {
    expect(isStrictPositiveInteger(1920)).toBe(true)
    expect(isStrictPositiveInteger(0)).toBe(false)
    expect(isStrictPositiveInteger(1920.5)).toBe(false)
    expect(isStrictPositiveInteger('1920')).toBe(false)
  })
})

describe('parseCSVForSeasonEpisode legacy behavior', () => {
  it('缺少 Name 列时返回空映射并记录诊断', () => {
    const { seasonMap, episodeMap, diagnostics } = parseCSVForSeasonEpisode('Foo,Bar\n1,2\n')
    expect(seasonMap.size).toBe(0)
    expect(episodeMap.size).toBe(0)
    expect(diagnostics).toHaveLength(1)
  })
})

describe('getCameraIdentifier', () => {
  it('各类输入返回正确标识', () => {
    expect(getCameraIdentifier('A001')).toBe('a')
    expect(getCameraIdentifier('BCam002')).toBe('bc')
    expect(getCameraIdentifier('')).toBe('')
    expect(getCameraIdentifier('123')).toBe('')
  })
})

describe('generateNewName', () => {
  const data = {
    sceneFormatted: '026',
    shotFormatted: '04',
    takeFormatted: '01',
    cameraId: 'd',
    rating: 'ok',
  }

  it('无 CSV 数据时使用传统格式', () => {
    expect(generateNewName(data, {}, 'x')).toBe('026_04_01d_ok')
  })

  it('仅有 Episode 时前缀集数号（无 E 前缀）', () => {
    const config: XMLProcessConfig = {
      csvEpisodeMap: new Map([['x', '44']]),
    }
    expect(generateNewName(data, config, 'x')).toBe('44_026_04_01d_ok')
  })

  it('Season+Episode 时前缀季号与集号', () => {
    const config: XMLProcessConfig = {
      csvSeasonMap: new Map([['x', '01']]),
      csvEpisodeMap: new Map([['x', '44']]),
    }
    expect(generateNewName(data, config, 'x')).toBe('01_44_026_04_01d_ok')
  })

  it('无评级时不带尾部下划线', () => {
    const noRating = { ...data, rating: '' }
    expect(generateNewName(noRating, {}, 'x')).toBe('026_04_01d')
  })
})

describe('getRatingFromLabels', () => {
  function labelsWith(text: string | null): Element | null {
    if (text === null) return null
    const doc = makeDoc(`<labels><label>${text}</label></labels>`)
    return doc.querySelector('labels')
  }

  it('空标签或 No Label 返回空字符串', () => {
    expect(getRatingFromLabels(null)).toBe('')
    expect(getRatingFromLabels(labelsWith('No Label'))).toBe('')
  })

  it('KEEP/kp 统一映射为 kp', () => {
    expect(getRatingFromLabels(labelsWith('KEEP'))).toBe('kp')
    expect(getRatingFromLabels(labelsWith('kp'))).toBe('kp')
  })

  it('Circle 映射为 ok', () => {
    expect(getRatingFromLabels(labelsWith('Circle'))).toBe('ok')
  })
})

describe('processClipData', () => {
  function extractElements(xmlStr: string) {
    const doc = makeDoc(xmlStr)
    const clip = doc.querySelector('clip')!
    const logginginfo = clip.querySelector('logginginfo')!
    const comments = clip.querySelector('comments')!
    return {
      logginginfo,
      scene: logginginfo.querySelector('scene')!,
      shottake: logginginfo.querySelector('shottake')!,
      filmdata: clip.querySelector('filmdata')!,
      comments,
      mastercomment2: comments.querySelector('mastercomment2')!,
      labels: clip.querySelector('labels'),
    }
  }

  it('合法输入返回格式化数据', () => {
    const elements = extractElements(SAMPLE_XML)
    const result = processClipData(elements)
    expect(result).toEqual({
      sceneFormatted: '026',
      shotFormatted: '04',
      takeFormatted: '01',
      cameraId: 'a',
      rating: 'kp',
    })
  })

  it('非法输入返回 null', () => {
    // 空场景
    expect(processClipData(extractElements(SAMPLE_XML.replace('<scene>26</scene>', '<scene></scene>')))).toBeNull()
    // shottake 非 x-y 格式
    expect(processClipData(extractElements(SAMPLE_XML.replace('4-1', '4')))).toBeNull()
    // 缺 cameraroll
    expect(processClipData(extractElements(SAMPLE_XML.replace('<cameraroll>A001</cameraroll>', '')))).toBeNull()
  })
})

describe('updateResolution', () => {
  it('替换文档中所有 width/height', () => {
    const doc = makeDoc(
      '<root><width>1280</width><width>640</width><height>720</height><height>360</height></root>'
    )
    updateResolution(doc, { width: 1920, height: 1080 })
    expect(doc.getElementsByTagName('width')[0].textContent).toBe('1920')
    expect(doc.getElementsByTagName('width')[1].textContent).toBe('1920')
    expect(doc.getElementsByTagName('height')[0].textContent).toBe('1080')
    expect(doc.getElementsByTagName('height')[1].textContent).toBe('1080')
  })
})

describe('processXML', () => {
  it('无效 XML 抛出 XMLProcessError', async () => {
    const result = await processXML(makeFile('<a><b></a>'))
    expect(result).toMatchObject({
      status: 'failed',
      counts: { processed: 0 },
    })
    expect(result.diagnostics[0]).toMatchObject({
      code: 'INVALID_XML',
      blocksDownload: true,
    })
  })

  it('有效 XML 全流程：改名、分辨率、DIT', async () => {
    const result = await processXML(makeFile(SAMPLE_XML))
    expect(result.status).toBe('success')
    expect(result.counts).toEqual({ total: 1, processed: 1, skipped: 0, failed: 0, csvUnmatched: 0 })
    expect(result.xml).toContain('026_04_01a_kp')
    expect(result.xml).toContain('<width>1920</width>')
    expect(result.xml).toContain('<height>1080</height>')
    expect(result.xml).toContain('Generated by https://double-love.ahua.space')
  })

  it('csvEpisodeMap 命中时输出集数前缀', async () => {
    const result = await processXML(makeFile(SAMPLE_XML), {
      csvEpisodeMap: new Map([[SYNTHETIC_CLIP_ID, '44']]),
    })
    expect(result.status).toBe('success')
    expect(result.xml).toContain('44_026_04_01a_kp')
  })

  it('单个 clip 缺字段时返回 partial 并保留可下载 XML', async () => {
    const result = await processXML(makeFile(`${SAMPLE_XML.replace('</media>', '<clip id="broken"><name>坏片段</name></clip></media>')}`))
    expect(result.status).toBe('partial')
    expect(result.counts).toMatchObject({ total: 2, processed: 1, skipped: 1 })
    expect(result.diagnostics.some(diagnostic => diagnostic.code === 'MISSING_CLIP_FIELDS')).toBe(true)
    expect(result.xml).toBeDefined()
  })

  it('缺少 clip id 时明确跳过，不把未改名的 clip 计为成功', async () => {
    const result = await processXML(makeFile(SAMPLE_XML.replace(` id="${SYNTHETIC_CLIP_ID}"`, '')))

    expect(result.status).toBe('failed')
    expect(result.counts).toEqual({
      total: 1,
      processed: 0,
      skipped: 1,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'MISSING_CLIP_ID',
      'NO_CLIPS_PROCESSED',
    ])
    expect(result.xml).toBeUndefined()
  })

  it('无法提取摄影机标识时跳过 clip，不生成伪完整结果', async () => {
    const result = await processXML(
      makeFile(SAMPLE_XML.replace('<cameraroll>A001</cameraroll>', '<cameraroll>123</cameraroll>'))
    )

    expect(result.status).toBe('failed')
    expect(result.counts).toEqual({
      total: 1,
      processed: 0,
      skipped: 1,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'INVALID_CLIP_DATA',
      'NO_CLIPS_PROCESSED',
    ])
    expect(result.xml).toBeUndefined()
  })

  it('镜号或条号不是完整数字时计为跳过', async () => {
    const result = await processXML(makeFile(SAMPLE_XML.replace('<shottake>4-1</shottake>', '<shottake>4-x</shottake>')))

    expect(result.status).toBe('failed')
    expect(result.counts).toMatchObject({ total: 1, processed: 0, skipped: 1, failed: 0 })
    expect(result.diagnostics[0]).toMatchObject({ code: 'INVALID_CLIP_DATA' })
    expect(result.xml).toBeUndefined()
  })

  it('特殊字符 clip id 使用精确属性匹配，并完整更新关联 sequence', async () => {
    const exceptionalClip = `<clip id="bad&quot;]">
      <name>异常片段</name>
      <logginginfo><scene>27</scene><shottake>5-2</shottake></logginginfo>
      <filmdata><cameraroll>B001</cameraroll></filmdata>
      <comments><mastercomment2>测试</mastercomment2></comments>
    </clip>`
    const exceptionalSequence = `<sequence id="sequence_id_bad&quot;]">
      <name>异常片段</name>
      <video><track><clipitem><name>异常片段</name></clipitem></track></video>
    </sequence>`
    const result = await processXML(makeFile(
      SAMPLE_XML
        .replace('</media>', `${exceptionalClip}</media>`)
        .replace('</project>', `${exceptionalSequence}</project>`)
    ))

    expect(result.status).toBe('success')
    expect(result.counts).toEqual({
      total: 2,
      processed: 2,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics).toEqual([])
    expect(result.xml).toContain('<name>027_05_02b</name>')
  })

  it('CSV 使用带扩展名和不同大小写的 Name 仍可命中', async () => {
    const result = await processXML(makeFile(SAMPLE_XML), {
      csvEpisodeMap: new Map([[SYNTHETIC_CLIP_ID.toLowerCase(), '44']]),
    })
    expect(result.counts.csvUnmatched).toBe(0)
    expect(result.xml).toContain('44_026_04_01a_kp')
  })

  it('CSV 未命中时返回 partial、准确计数和用户可见诊断', async () => {
    const result = await processXML(makeFile(SAMPLE_XML), {
      csvEpisodeMap: new Map([['another-clip', '44']]),
    })

    expect(result.status).toBe('partial')
    expect(result.counts).toEqual({
      total: 1,
      processed: 1,
      skipped: 0,
      failed: 0,
      csvUnmatched: 1,
    })
    expect(result.diagnostics).toEqual([
      expect.objectContaining({ code: 'CSV_CLIP_UNMATCHED', level: 'warning' }),
    ])
    expect(result.xml).toContain('026_04_01a_kp')
  })

  it('非法分辨率返回 failed 且不提供 XML', async () => {
    const result = await processXML(makeFile(SAMPLE_XML), { width: Number.NaN })
    expect(result.status).toBe('failed')
    expect(result.diagnostics[0]?.code).toBe('INVALID_RESOLUTION')
    expect(result.xml).toBeUndefined()
  })

  it('onProgress 回调末次调用为 100', async () => {
    const onProgress = vi.fn()
    await processXML(makeFile(SAMPLE_XML), { onProgress })
    expect(onProgress).toHaveBeenCalled()
    expect(onProgress.mock.calls.at(-1)?.[0]).toBe(100)
  })
})
