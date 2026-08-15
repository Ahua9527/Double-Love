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

const MIXED_REGRESSION_XML = `<?xml version="1.0" encoding="UTF-8"?>
<project>
  <media>
    <clip id="processed-a">
      <name>原始 A</name>
      <file id="file-processed-a"><name>camera-a.mov</name><pathurl>file:///media/camera-a.mov</pathurl></file>
      <logginginfo><scene>26</scene><shottake>4-1</shottake></logginginfo>
      <filmdata><cameraroll>A001</cameraroll></filmdata>
      <comments><mastercomment2>备注 A</mastercomment2></comments>
      <labels><label>KEEP</label></labels>
    </clip>
    <clip id="processed-b">
      <name>原始 B</name>
      <file id="file-processed-b"><name>camera-b.mov</name><pathurl>file:///media/camera-b.mov</pathurl></file>
      <logginginfo><scene>27</scene><shottake>1A-1</shottake></logginginfo>
      <filmdata><cameraroll>B001</cameraroll></filmdata>
      <comments><mastercomment2>备注 B</mastercomment2></comments>
    </clip>
    <clip id="processed-c">
      <name>原始 C</name>
      <file id="file-processed-c"><name>camera-c.mov</name><pathurl>file:///media/camera-c.mov</pathurl></file>
      <logginginfo><scene>28</scene><shottake>1-2跑</shottake></logginginfo>
      <filmdata><cameraroll>C001</cameraroll></filmdata>
      <comments><mastercomment2>备注 C</mastercomment2></comments>
    </clip>
    <clip id="audio-only">
      <name>原始音频</name>
      <file id="file-audio"><name>audio-original.wav</name><pathurl>file:///media/audio-original.wav</pathurl></file>
      <media><audio><track><clipitem id="audio-ref"><ref>file-audio</ref></clipitem></track></audio></media>
    </clip>
    <clip id="still-image">
      <name>reference.jpg</name>
      <file id="file-still"><name>reference.jpg</name><pathurl>file:///media/reference.jpg</pathurl></file>
      <logginginfo><scene></scene><shottake>-</shottake></logginginfo>
      <filmdata><cameraroll>STILL001</cameraroll></filmdata>
      <comments><mastercomment2>静帧参考</mastercomment2></comments>
    </clip>
    <clip id="skipped-clip">
      <name>原始跳过</name>
      <file id="file-skipped"><name>camera-skipped.mov</name><pathurl>file:///media/camera-skipped.mov</pathurl></file>
      <logginginfo><scene>29</scene><shottake>4-x</shottake></logginginfo>
      <filmdata><cameraroll>D001</cameraroll></filmdata>
      <comments><mastercomment2>跳过备注</mastercomment2></comments>
    </clip>
  </media>
  <sequence id="sequence_id_processed-a"><name>原始 A</name><video><track><clipitem id="item-a"><ref>file-processed-a</ref><name>原始 A</name></clipitem></track></video></sequence>
  <sequence id="sequence_id_processed-b"><name>原始 B</name><video><track><clipitem id="item-b"><ref>file-processed-b</ref><name>原始 B</name></clipitem></track></video></sequence>
  <sequence id="sequence_id_processed-c"><name>原始 C</name><video><track><clipitem id="item-c"><ref>file-processed-c</ref><name>原始 C</name></clipitem></track></video></sequence>
</project>`

const ONLY_IGNORED_XML = `<?xml version="1.0" encoding="UTF-8"?>
<project><media>
  <clip id="audio-only"><name>原始音频</name><media><audio><track /></audio></media></clip>
  <clip id="still-image"><name>reference.jpeg</name><file id="file-still"><name>reference.jpeg</name></file><logginginfo><scene /><shottake>-</shottake></logginginfo><filmdata><cameraroll>STILL001</cameraroll></filmdata><comments><mastercomment2>静帧参考</mastercomment2></comments></clip>
</media></project>`

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

  it('去空格后接受含拉丁字母的镜头拍次并分别补两位数字', () => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<shottake>4-1</shottake>', '<shottake> 1A - 1 </shottake>'))
    )

    expect(result).toEqual({
      sceneFormatted: '026',
      shotFormatted: '01a',
      takeFormatted: '01',
      cameraId: 'a',
      rating: 'kp',
    })
  })

  it('兼容处理不含数字但非占位的场景值', () => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<scene>26</scene>', '<scene>INSERT</scene>'))
    )

    expect(result).toMatchObject({
      sceneFormatted: 'INSERT',
      shotFormatted: '04',
      takeFormatted: '01',
    })
  })

  it.each(['N/A', 'NA', 'NULL', 'NONE'])('场景通用校验继续接受 %s', sceneValue => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<scene>26</scene>', `<scene>${sceneValue}</scene>`))
    )

    expect(result).toMatchObject({
      sceneFormatted: sceneValue,
      shotFormatted: '04',
      takeFormatted: '01',
    })
  })

  it.each([
    ['B1-2', 'b01', '02'],
    ['1-2跑', '01', '02跑'],
    ['1Ⅳ-2', '01Ⅳ', '02'],
    ['١A-٢', '0١a', '0٢'],
  ])('按 Unicode 字母/数字规则格式化 %s', (shottake, expectedShot, expectedTake) => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<shottake>4-1</shottake>', `<shottake>${shottake}</shottake>`))
    )

    expect(result?.shotFormatted).toBe(expectedShot)
    expect(result?.takeFormatted).toBe(expectedTake)
  })

  it('只将 ASCII A-Z 转为小写，保留其他 Unicode 字母和中文', () => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<shottake>4-1</shottake>', '<shottake>Α1跑-Ж2中</shottake>'))
    )

    expect(result?.shotFormatted).toBe('Α01跑')
    expect(result?.takeFormatted).toBe('Ж02中')
  })

  it.each(['9-', '-1', '4-x', '1--2', '1.2-3'])('拒绝不符合镜头拍次规则的 %s', shottake => {
    const result = processClipData(
      extractElements(SAMPLE_XML.replace('<shottake>4-1</shottake>', `<shottake>${shottake}</shottake>`))
    )

    expect(result).toBeNull()
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

  it('空 XML 保持文件级 NO_CLIPS，不误报全部 ignored', async () => {
    const result = await processXML(makeFile('<project />'))

    expect(result.status).toBe('failed')
    expect(result.counts).toEqual({
      total: 0,
      processed: 0,
      ignored: 0,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics[0]?.code).toBe('NO_CLIPS')
    expect(result.diagnostics.some(diagnostic => diagnostic.code === 'NO_PROCESSABLE_VIDEO_CLIPS')).toBe(false)
    expect(result.xml).toBeUndefined()
  })

  it('有效 XML 全流程：改名、分辨率、DIT', async () => {
    const result = await processXML(makeFile(SAMPLE_XML))
    expect(result.status).toBe('success')
    expect(result.counts).toEqual({ total: 1, processed: 1, ignored: 0, skipped: 0, failed: 0, csvUnmatched: 0 })
    expect(result.xml).toContain('026_04_01a_kp')
    expect(result.xml).toContain('<width>1920</width>')
    expect(result.xml).toContain('<height>1080</height>')
    expect(result.xml).toContain('Generated by https://double-love.ahua.space')
  })

  it('混合 XML 按 6/3/2/1/0 统计，只有成功项改名并保持引用与进度', async () => {
    const onProgress = vi.fn()
    const result = await processXML(makeFile(MIXED_REGRESSION_XML), { onProgress })

    expect(result.status).toBe('partial')
    expect(result.counts).toEqual({
      total: 6,
      processed: 3,
      ignored: 2,
      skipped: 1,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.xml).toContain('<clip id="audio-only">')
    expect(result.xml).toContain('<name>原始音频</name>')
    expect(result.xml).toContain('<name>reference.jpg</name>')
    expect(result.xml).toContain('<name>原始跳过</name>')
    expect(result.xml).toContain('<name>026_04_01a_kp</name>')
    expect(result.xml).toContain('<name>027_01a_01b</name>')
    expect(result.xml).toContain('<name>028_01_02跑c</name>')
    expect(result.xml).toContain('id="file-still"')
    expect(result.xml).toContain('<ref>file-audio</ref>')
    expect(result.xml).toContain('file:///media/reference.jpg')
    expect(onProgress.mock.calls.at(-1)?.[0]).toBe(100)
  })

  it('可处理视频与忽略项共存时仍返回 success', async () => {
    const result = await processXML(makeFile(
      SAMPLE_XML.replace('</media>', '<clip id="audio-only"><name>原始音频</name><media><audio /></media></clip></media>')
    ))

    expect(result.status).toBe('success')
    expect(result.counts).toEqual({
      total: 2,
      processed: 1,
      ignored: 1,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
  })

  it('纯音频只按 clip 的 media 节点分类，不误判其他后代节点', async () => {
    const xmlWithMediaAudio = SAMPLE_XML
      .replace('<filmdata>', '<metadata><audio /></metadata><filmdata>')
      .replace('</media>', '<clip id="audio-media"><name>原始音频</name><media><audio /></media></clip></media>')
    const result = await processXML(makeFile(xmlWithMediaAudio))

    expect(result.status).toBe('success')
    expect(result.counts).toEqual({
      total: 2,
      processed: 1,
      ignored: 1,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual(['IGNORED_AUDIO_ONLY'])
  })

  it('缺少 media 的 clip 不会被归类为纯音频 ignored', async () => {
    const noMediaClip = '<clip id="no-media">'
      + '<name>无 media 视频</name>'
      + '<logginginfo><scene>27</scene><shottake>5-2</shottake></logginginfo>'
      + '<filmdata><cameraroll>B001</cameraroll></filmdata>'
      + '<comments><mastercomment2>无 media</mastercomment2></comments>'
      + '</clip>'
    const result = await processXML(makeFile(SAMPLE_XML.replace('</media>', `${noMediaClip}</media>`)))

    expect(result.counts).toMatchObject({ total: 2, processed: 2, ignored: 0 })
    expect(result.diagnostics).toEqual([])
  })

  it('有完整场记的 JPEG 引用仍按视频 clip 处理', async () => {
    const loggedJpeg = '<clip id="logged-jpeg">'
      + '<name>logged.jpeg</name><file><name>logged.jpeg</name></file>'
      + '<logginginfo><scene>27</scene><shottake>5-2</shottake></logginginfo>'
      + '<filmdata><cameraroll>B001</cameraroll></filmdata>'
      + '<comments><mastercomment2>有效场记</mastercomment2></comments>'
      + '</clip>'
    const result = await processXML(makeFile(SAMPLE_XML.replace('</media>', `${loggedJpeg}</media>`)))

    expect(result.status).toBe('success')
    expect(result.counts).toEqual({
      total: 2,
      processed: 2,
      ignored: 0,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics).toEqual([])
    expect(result.xml).toContain('027_05_02b')
  })

  it.each(['N/A', 'NA', 'NULL', 'NONE'])('JPEG 的 %s 仍按静帧 placeholder 忽略', async placeholder => {
    const still = '<clip id="placeholder-still">'
      + '<name>reference.jpg</name><file><name>reference.jpg</name></file>'
      + `<logginginfo><scene>${placeholder}</scene><shottake>1-1</shottake></logginginfo>`
      + '<filmdata><cameraroll>STILL001</cameraroll></filmdata>'
      + '<comments><mastercomment2>静帧 placeholder</mastercomment2></comments>'
      + '</clip>'
    const result = await processXML(makeFile(SAMPLE_XML.replace('</media>', `${still}</media>`)))

    expect(result.counts).toMatchObject({ total: 2, processed: 1, ignored: 1 })
    expect(result.diagnostics).toContainEqual(expect.objectContaining({
      code: 'IGNORED_STILL_IMAGE',
      clipId: 'placeholder-still',
    }))
  })

  it('全部为忽略项时失败且不提供 XML，ignored 不增加 CSV 未匹配数', async () => {
    const onProgress = vi.fn()
    const result = await processXML(makeFile(ONLY_IGNORED_XML), {
      csvEpisodeMap: new Map([['not-in-xml', '44']]),
      onProgress,
    })

    expect(result.status).toBe('failed')
    expect(result.counts).toEqual({
      total: 2,
      processed: 0,
      ignored: 2,
      skipped: 0,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.xml).toBeUndefined()
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'IGNORED_AUDIO_ONLY',
      'IGNORED_STILL_IMAGE',
      'NO_PROCESSABLE_VIDEO_CLIPS',
    ])
    expect(onProgress.mock.calls.at(-1)?.[0]).toBe(100)
  })

  it('CSV 未匹配统计只针对已处理 clip，不把 ignored 算进去', async () => {
    const result = await processXML(makeFile(
      SAMPLE_XML.replace('</media>', '<clip id="audio-only"><name>原始音频</name><media><audio /></media></clip></media>')
    ), {
      csvEpisodeMap: new Map([['not-in-xml', '44']]),
    })

    expect(result.status).toBe('partial')
    expect(result.counts).toMatchObject({
      total: 2,
      processed: 1,
      ignored: 1,
      skipped: 0,
      failed: 0,
      csvUnmatched: 1,
    })
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
      ignored: 0,
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
      ignored: 0,
      skipped: 1,
      failed: 0,
      csvUnmatched: 0,
    })
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'INVALID_CAMERA_ROLL',
      'NO_CLIPS_PROCESSED',
    ])
    expect(result.xml).toBeUndefined()
  })

  it('场景号无效时给出结构化诊断', async () => {
    const result = await processXML(
      makeFile(SAMPLE_XML.replace('<scene>26</scene>', '<scene>---</scene>'))
    )

    expect(result.counts).toMatchObject({ processed: 0, skipped: 1 })
    expect(result.diagnostics.map(diagnostic => diagnostic.code)).toEqual([
      'INVALID_SCENE',
      'NO_CLIPS_PROCESSED',
    ])
  })

  it('镜号或条号不是完整数字时计为跳过', async () => {
    const result = await processXML(makeFile(SAMPLE_XML.replace('<shottake>4-1</shottake>', '<shottake>4-x</shottake>')))

    expect(result.status).toBe('failed')
    expect(result.counts).toMatchObject({ total: 1, processed: 0, skipped: 1, failed: 0 })
    expect(result.diagnostics[0]).toMatchObject({ code: 'INVALID_SHOT_TAKE' })
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
      ignored: 0,
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
      ignored: 0,
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
