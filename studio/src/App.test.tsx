import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import App from './App'
import { PANEL_STORAGE_KEY } from './utils'

// jsdom 没有 Tauri 壳：应用应降级为「引导屏 + 空态面板」，按钮给出提示而不是崩溃。

describe('App 冒烟渲染（无桌面壳）', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('渲染标题栏、引导屏与空态面板', () => {
    render(<App />)
    // 标题栏
    expect(screen.getByText('Double Love Studio')).toBeTruthy()
    // 引导屏
    expect(screen.getByText('打开项目…')).toBeTruthy()
    expect(screen.getByText('新建项目…')).toBeTruthy()
    // 空态面板：侧栏与状态栏各有一处「未打开项目」
    expect(screen.getAllByText('未打开项目').length).toBe(2)
    expect(screen.getByText('资产信息')).toBeTruthy()
    expect(screen.getByText('导入媒体后显示时间线')).toBeTruthy()
    // 无项目时导入/导出不可用
    expect(screen.getByRole('button', { name: '导入…' })).toHaveProperty('disabled', true)
    expect(
      screen.getByRole('button', { name: '导出 Premiere XML' }),
    ).toHaveProperty('disabled', true)
  })

  it('无桌面壳时打开项目给出提示而不是崩溃', () => {
    render(<App />)
    fireEvent.click(screen.getByText('打开项目…'))
    expect(screen.getByText(/需要在 Double Love Studio 桌面应用中运行/)).toBeTruthy()
  })
})

describe('面板抽屉', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('左侧栏可收起再展开', () => {
    render(<App />)
    const toggle = screen.getByLabelText('切换左侧栏')
    expect(screen.getAllByText('未打开项目').length).toBe(2)
    fireEvent.click(toggle)
    expect(screen.getAllByText('未打开项目').length).toBe(1)
    fireEvent.click(toggle)
    expect(screen.getAllByText('未打开项目').length).toBe(2)
  })

  it('检查器可收起', () => {
    render(<App />)
    fireEvent.click(screen.getByLabelText('切换检查器'))
    expect(screen.queryByText('资产信息')).toBeNull()
  })

  it('时间线可收起', () => {
    render(<App />)
    fireEvent.click(screen.getByLabelText('切换时间线'))
    expect(screen.queryByText('导入媒体后显示时间线')).toBeNull()
  })

  it('收起状态写入 localStorage，重挂载后保持', () => {
    const first = render(<App />)
    fireEvent.click(screen.getByLabelText('切换左侧栏'))
    fireEvent.click(screen.getByLabelText('切换时间线'))
    expect(window.localStorage.getItem(PANEL_STORAGE_KEY)).toBe(
      JSON.stringify({ left: false, right: true, bottom: false }),
    )
    first.unmount()

    render(<App />)
    expect(screen.getAllByText('未打开项目').length).toBe(1)
    expect(screen.queryByText('导入媒体后显示时间线')).toBeNull()
    expect(screen.getByText('资产信息')).toBeTruthy()
  })
})
