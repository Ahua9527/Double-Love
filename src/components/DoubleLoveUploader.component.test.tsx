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
    expect(screen.getByText(/共 2 个 clip：处理 1，跳过 1，失败 0/)).toBeTruthy()
    expect(screen.getByText(/共 0 个 clip：处理 0，跳过 0，失败 1/)).toBeTruthy()
    expect(screen.getByText(/MISSING_CLIP_FIELDS.*clip 缺少必要字段/)).toBeTruthy()
    expect(screen.getByText(/INVALID_XML.*无效的 XML 文件/)).toBeTruthy()
    await waitFor(() => {
      expect(downloadedNames).toEqual([
        'success_Double_LOVE.xml',
        'partial_Double_LOVE.xml',
      ])
    })
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
