import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import DoubleLoveUploader from './DoubleLoveUploader'
import { SYNTHETIC_PREMIERE_XML } from '../test/fixtures/webMaintenance'

const downloadedNames: string[] = []

function makeTextFile(content: string, name: string, lastModified = 1): File {
  const file = new File([content], name, { type: 'text/xml', lastModified })
  Object.defineProperty(file, 'text', { value: async () => content })
  return file
}

beforeEach(() => {
  downloadedNames.length = 0
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true,
    value: vi.fn(() => 'blob:synthetic-download'),
  })
  Object.defineProperty(URL, 'revokeObjectURL', {
    configurable: true,
    value: vi.fn(),
  })
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function click(this: HTMLAnchorElement) {
    downloadedNames.push(this.download)
  })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('DoubleLoveUploader', () => {
  it('支持键盘打开上传控件并显示已接受的 XML 与 CSV', () => {
    render(<DoubleLoveUploader />)

    const uploadButton = screen.getByRole('button', { name: '上传 XML 或 CSV 文件' })
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const xml = new File(['<project />'], 'shot.XML', { type: 'text/xml' })
    const csv = new File(['Name,Season,Episode\nshot,1,2\n'], 'metadata.csv', { type: 'text/csv' })

    fireEvent.keyDown(uploadButton, { key: 'Enter' })
    fireEvent.change(fileInput, { target: { files: [xml, csv] } })

    expect(screen.getByText('shot.XML')).toBeTruthy()
    expect(screen.getByText('metadata.csv')).toBeTruthy()
    expect(screen.getByRole('button', { name: /处理 1 个XML文件/ })).toBeTruthy()
  })

  it('拒绝第二个 CSV 并给出明确提示', () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const firstCsv = new File(['Name\nshot\n'], 'first.csv', { type: 'text/csv' })
    const secondCsv = new File(['Name\nshot\n'], 'second.csv', { type: 'text/csv' })

    fireEvent.change(fileInput, { target: { files: [firstCsv] } })
    fireEvent.change(fileInput, { target: { files: [secondCsv] } })

    expect(screen.getByRole('alert').textContent).toContain('second.csv：当前只允许一个 CSV')
    expect(screen.getByText('first.csv')).toBeTruthy()
    expect(screen.queryByText('second.csv')).toBeNull()
  })

  it('混合上传合法 XML 和第二个 CSV 时保留 XML，只拒绝多余 CSV', () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const firstCsv = new File(['Name\nshot\n'], 'first.csv', { type: 'text/csv' })
    const secondCsv = new File(['Name\nshot\n'], 'second.csv', { type: 'text/csv' })
    const xml = new File(['<project />'], 'kept.xml', { type: 'text/xml' })

    fireEvent.change(fileInput, { target: { files: [firstCsv] } })
    fireEvent.change(fileInput, { target: { files: [xml, secondCsv] } })

    expect(screen.getByText('kept.xml')).toBeTruthy()
    expect(screen.queryByText('second.csv')).toBeNull()
    expect(screen.getByRole('alert').textContent).toContain('second.csv：当前只允许一个 CSV')
  })

  it('逐文件说明不支持的上传内容', () => {
    render(<DoubleLoveUploader />)

    fireEvent.change(screen.getByLabelText('选择 XML 或 CSV 文件'), {
      target: { files: [new File(['ignored'], 'notes.txt', { type: 'text/plain' })] },
    })

    expect(screen.getByRole('alert').textContent).toContain('notes.txt：不支持的文件类型')
  })

  it('拒绝非法的 Season/Episode，不生成看似成功的 XML', async () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const xml = makeTextFile(SYNTHETIC_PREMIERE_XML, 'synthetic.xml')
    const csvContent = 'Name,Season,Episode\nSYNTHETIC_CLIP_A001,0,1920abc\n'
    const csv = new File([csvContent], 'invalid.csv', { type: 'text/csv' })
    Object.defineProperty(csv, 'text', { value: async () => csvContent })

    fireEvent.change(fileInput, { target: { files: [xml, csv] } })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Season 必须是 1 到 99')
    expect(alert.textContent).toContain('Episode 必须是 1 到 99')
    expect(downloadedNames).toEqual([])
    expect(screen.queryByText('成功')).toBeNull()
  })

  it('忽略重复选择的同一个文件，并保留一条稳定记录', () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const xml = new File(['<project />'], 'repeat.xml', {
      type: 'text/xml',
      lastModified: 1,
    })

    fireEvent.change(fileInput, { target: { files: [xml] } })
    fireEvent.change(fileInput, { target: { files: [xml] } })

    expect(screen.getAllByText('repeat.xml')).toHaveLength(1)
    expect(screen.getByRole('alert').textContent).toContain('重复')
  })

  it('允许同名但元数据不同的 XML 独立加入和移除', () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const first = new File(['<project />'], 'same.xml', { type: 'text/xml', lastModified: 1 })
    const second = new File(['<project><media /></project>'], 'same.xml', { type: 'text/xml', lastModified: 2 })

    fireEvent.change(fileInput, { target: { files: [first, second] } })
    expect(screen.getAllByText('same.xml')).toHaveLength(2)

    fireEvent.click(screen.getAllByRole('button', { name: '移除 XML 文件 same.xml' })[0])
    expect(screen.getAllByText('same.xml')).toHaveLength(1)
  })

  it('XML 与单个 CSV 合计最多接受 99 个，非法文件不占名额', () => {
    render(<DoubleLoveUploader />)

    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    const xmlFiles = Array.from({ length: 98 }, (_, index) => (
      new File(['<project />'], `clip-${index}.xml`, { type: 'text/xml', lastModified: index })
    ))
    const csv = new File(['Name,Episode\nclip,1\n'], 'metadata.csv', { type: 'text/csv' })
    const invalid = new File(['ignored'], 'notes.txt', { type: 'text/plain' })

    fireEvent.change(fileInput, { target: { files: [...xmlFiles, csv, invalid] } })
    expect(screen.getByText('已上传 XML 文件 (98)')).toBeTruthy()
    expect(screen.getByText(/已上传 CSV 文件 \(1\)/)).toBeTruthy()

    const overflow = new File(['<project />'], 'overflow.xml', { type: 'text/xml' })
    fireEvent.change(fileInput, { target: { files: [overflow] } })
    expect(screen.getByRole('alert').textContent).toContain('overflow.xml：已达到 99 个文件上限')
    expect(screen.queryByText('overflow.xml')).toBeNull()
  })

  it.each(['', '0', '-1', '1920.5', '1e3', '1920abc'])(
    '拒绝非法分辨率“%s”并恢复处理按钮',
    async invalidWidth => {
      render(<DoubleLoveUploader />)

      const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
      const xml = new File(['<project />'], 'resolution.xml', { type: 'text/xml' })
      fireEvent.change(fileInput, { target: { files: [xml] } })
      fireEvent.change(screen.getByLabelText('视频宽度'), { target: { value: invalidWidth } })

      const processButton = screen.getByRole('button', { name: /处理 1 个XML文件/ })
      fireEvent.click(processButton)

      const alert = await screen.findByRole('alert')
      expect(alert.textContent).toContain('分辨率必须是完整的正整数')
      expect((processButton as HTMLButtonElement).disabled).toBe(false)
    }
  )

  it('逐文件展示成功、部分完成与失败，并在失败后继续下载可用结果', async () => {
    render(<DoubleLoveUploader />)

    const partialXml = SYNTHETIC_PREMIERE_XML.replace(
      '</media>',
      '<clip id="synthetic-broken"><name>合成缺字段片段</name></clip></media>'
    )
    const success = makeTextFile(SYNTHETIC_PREMIERE_XML, 'success.XML', 1)
    const failed = makeTextFile('<project><broken></project>', 'failed.xml', 2)
    const partial = makeTextFile(partialXml, 'partial.xMl', 3)
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')

    fireEvent.change(fileInput, { target: { files: [success, failed, partial] } })
    fireEvent.click(screen.getByRole('button', { name: /处理 3 个XML文件/ }))

    await screen.findByText('部分完成')
    expect(screen.getByText('成功')).toBeTruthy()
    expect(screen.getByText('失败')).toBeTruthy()
    expect(screen.getByText(/共 2 个 clip：处理 1，忽略 0，跳过 1，失败 0/)).toBeTruthy()
    expect(screen.getByText(/共 0 个 clip：处理 0，忽略 0，跳过 0，失败 0/)).toBeTruthy()
    expect(screen.getByText(/MISSING_CLIP_FIELDS.*clip 缺少必要字段/)).toBeTruthy()
    expect(screen.getByText(/INVALID_XML.*无效的 XML 文件/)).toBeTruthy()
    await waitFor(() => {
      expect(downloadedNames).toEqual([
        'success_Double_LOVE.xml',
        'partial_Double_LOVE.xml',
      ])
    })
  })

  it('显示 ignored 统计、无 ID 数量，并将相同诊断折叠为可展开的有序 ID 列表', async () => {
    render(<DoubleLoveUploader />)

    const groupedXml = SYNTHETIC_PREMIERE_XML.replace(
      '</media>',
      [
        '<clip id="audio-1"><name>音频 1</name><media><audio /></media></clip>',
        '<clip id="audio-2"><name>音频 2</name><media><audio /></media></clip>',
        '<clip id="audio-3"><name>音频 3</name><media><audio /></media></clip>',
        '<clip id="audio-4"><name>音频 4</name><media><audio /></media></clip>',
        '<clip><name>无 ID 1</name><media><audio /></media></clip>',
        '<clip><name>无 ID 2</name><media><audio /></media></clip>',
        '</media>',
      ].join('')
    )
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    fireEvent.change(fileInput, {
      target: { files: [makeTextFile(groupedXml, 'grouped.xml')] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    await screen.findByText('成功')
    expect(screen.getByText(/共 7 个 clip：处理 1，忽略 6，跳过 0，失败 0/)).toBeTruthy()

    const ignoredSummary = screen.getByText(/IGNORED_AUDIO_ONLY.*数量：6/) as HTMLElement
    const ignoredDetails = ignoredSummary.closest('details') as HTMLDetailsElement | null
    expect(ignoredDetails?.open).toBe(false)
    expect(ignoredSummary.textContent).toContain('audio-1')
    expect(ignoredSummary.textContent).toContain('audio-2')
    expect(ignoredSummary.textContent).toContain('audio-3')
    expect(ignoredSummary.textContent).not.toContain('audio-4')
    expect(ignoredSummary.textContent).toContain('无 clip ID 2 项')
    expect(ignoredSummary.textContent?.match(/无 clip ID 2 项/g)).toHaveLength(1)
    expect(ignoredDetails?.closest('[aria-live]')).toBeNull()

    fireEvent.click(ignoredSummary)
    expect(ignoredDetails?.open).toBe(true)
    expect(ignoredDetails?.textContent).toContain('无 clip ID 2 项')
    expect(ignoredDetails?.textContent?.match(/无 clip ID 2 项/g)).toHaveLength(1)
    expect(ignoredDetails?.textContent).toContain('audio-1')
    expect(ignoredDetails?.textContent).toContain('audio-2')
    expect(ignoredDetails?.textContent).toContain('audio-3')
    expect(ignoredDetails?.textContent).toContain('audio-4')
    expect(ignoredDetails?.textContent?.indexOf('audio-1')).toBeLessThan(
      ignoredDetails?.textContent?.indexOf('audio-4') ?? -1
    )
  })

  it('仅有 ignored clip 时显示失败且不触发下载', async () => {
    render(<DoubleLoveUploader />)

    const ignoredOnlyXml = '<project><media>'
      + '<clip id="audio-only"><media><audio /></media></clip>'
      + '<clip id="still-only"><name>still.jpg</name><file><name>still.jpg</name></file>'
      + '<logginginfo><scene /><shottake>-</shottake></logginginfo>'
      + '<filmdata><cameraroll>STILL001</cameraroll></filmdata>'
      + '<comments><mastercomment2>still</mastercomment2></comments></clip>'
      + '</media></project>'
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    fireEvent.change(fileInput, {
      target: { files: [makeTextFile(ignoredOnlyXml, 'ignored-only.xml')] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    await screen.findByText('失败')
    expect(screen.getByText(/共 2 个 clip：处理 0，忽略 2，跳过 0，失败 0/)).toBeTruthy()
    expect(screen.getByText(/NO_PROCESSABLE_VIDEO_CLIPS/)).toBeTruthy()
    expect(downloadedNames).toEqual([])
  })

  it('用可访问级别文本和颜色区分 info、warning、error 诊断', async () => {
    render(<DoubleLoveUploader />)

    const infoXml = SYNTHETIC_PREMIERE_XML.replace(
      '</media>',
      '<clip id="audio-info"><name>音频</name><media><audio /></media></clip></media>'
    )
    const warningXml = SYNTHETIC_PREMIERE_XML.replace(
      '</media>',
      '<clip id="missing-fields"><name>缺字段</name></clip></media>'
    )
    const errorXml = '<project><broken></project>'
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    fireEvent.change(fileInput, {
      target: { files: [
        makeTextFile(infoXml, 'info.xml'),
        makeTextFile(warningXml, 'warning.xml'),
        makeTextFile(errorXml, 'error.xml'),
      ] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 3 个XML文件/ }))

    const infoSummary = await screen.findByText(/提示 · IGNORED_AUDIO_ONLY/)
    const warningSummary = await screen.findByText(/警告 · MISSING_CLIP_FIELDS/)
    const errorSummary = await screen.findByText(/错误 · INVALID_XML/)

    expect(infoSummary.textContent).toContain('提示')
    expect(warningSummary.textContent).toContain('警告')
    expect(errorSummary.textContent).toContain('错误')
    expect(infoSummary.className).toContain('text-slate')
    expect(warningSummary.className).toContain('text-amber')
    expect(errorSummary.className).toContain('text-red')
  })

  it('意外异常显示文件级诊断且不伪装成 clip 失败', async () => {
    render(<DoubleLoveUploader />)

    vi.spyOn(DOMParser.prototype, 'parseFromString').mockImplementation(() => {
      throw new Error('synthetic parser crash')
    })
    const fileInput = screen.getByLabelText('选择 XML 或 CSV 文件')
    fireEvent.change(fileInput, {
      target: { files: [makeTextFile(SYNTHETIC_PREMIERE_XML, 'unexpected.xml')] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    const summary = await screen.findByText(/错误 · UNEXPECTED_PROCESSING_ERROR/)
    expect(summary.textContent).toContain('文件级处理异常')
    expect(screen.getByText(/共 0 个 clip：处理 0，忽略 0，跳过 0，失败 0/)).toBeTruthy()
  })

  it('CSV 没有可用 Season/Episode 时显示诊断，不宣称正在使用 CSV 命名', async () => {
    render(<DoubleLoveUploader />)

    let resolveText: (value: string) => void = () => undefined
    const pendingText = new Promise<string>(resolve => {
      resolveText = resolve
    })
    const xml = new File(['pending'], 'warning.xml', { type: 'text/xml' })
    Object.defineProperty(xml, 'text', { value: () => pendingText })
    const csvContent = 'Name,Season,Episode\nSYNTHETIC_CLIP_A001,,\n'
    const csv = makeTextFile(csvContent, 'warning.csv')

    fireEvent.change(screen.getByLabelText('选择 XML 或 CSV 文件'), {
      target: { files: [xml, csv] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    expect(await screen.findByText(/正在处理: warning.xml/)).toBeTruthy()
    expect(screen.getByText(/Season 为空/)).toBeTruthy()
    expect(screen.getByText(/Episode 为空/)).toBeTruthy()
    expect(screen.queryByText(/使用 CSV 数据进行 Season\/Episode 命名/)).toBeNull()

    await act(async () => resolveText(SYNTHETIC_PREMIERE_XML))
    await waitFor(() => expect(screen.queryByRole('progressbar')).toBeNull())
  })

  it('处理期间暴露当前文件和可访问的进度状态', async () => {
    render(<DoubleLoveUploader />)

    let resolveText: (value: string) => void = () => undefined
    const pendingText = new Promise<string>(resolve => {
      resolveText = resolve
    })
    const file = new File(['pending'], 'progress.xml', { type: 'text/xml' })
    Object.defineProperty(file, 'text', { value: () => pendingText })

    fireEvent.change(screen.getByLabelText('选择 XML 或 CSV 文件'), {
      target: { files: [file] },
    })
    fireEvent.click(screen.getByRole('button', { name: /处理 1 个XML文件/ }))

    const progressbar = await screen.findByRole('progressbar', { name: 'XML 处理进度' })
    expect(progressbar.getAttribute('aria-valuemin')).toBe('0')
    expect(progressbar.getAttribute('aria-valuemax')).toBe('100')
    expect(screen.getByText(/正在处理: progress.xml/)).toBeTruthy()

    await act(async () => resolveText(SYNTHETIC_PREMIERE_XML))
    await waitFor(() => expect(screen.queryByRole('progressbar')).toBeNull())
    expect(screen.getByText('成功')).toBeTruthy()
  })
})
