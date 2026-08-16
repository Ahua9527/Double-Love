import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import App from './App'
import { PANEL_STORAGE_KEY } from './utils'

describe('App 冒烟渲染', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('渲染五区工作区骨架', () => {
    render(<App />)
    // 标题栏
    expect(screen.getByText('Double Love Studio')).toBeTruthy()
    expect(screen.getByText('青空日记 · 第 2 集')).toBeTruthy()
    // 侧边栏
    expect(screen.getByText('智能集合')).toBeTruthy()
    // 预览窗（首个片段）
    expect(screen.getAllByText('02_015_01_01a').length).toBeGreaterThan(0)
    // 片段表格（表头与检查器各有一处「新名称」）
    expect(screen.getAllByText('新名称').length).toBeGreaterThan(0)
    // 检查器卡片
    expect(screen.getByText('项目操作')).toBeTruthy()
    expect(screen.getByText('以上动作用于整个项目')).toBeTruthy()
    // 状态栏
    expect(screen.getByText(/共 21 片段/)).toBeTruthy()
  })
})

describe('面板抽屉', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('左侧栏可收起再展开', () => {
    render(<App />)
    const toggle = screen.getByLabelText('切换左侧栏')
    fireEvent.click(toggle)
    expect(screen.queryByText('智能集合')).toBeNull()
    fireEvent.click(toggle)
    expect(screen.getByText('智能集合')).toBeTruthy()
  })

  it('检查器可收起', () => {
    render(<App />)
    fireEvent.click(screen.getByLabelText('切换检查器'))
    expect(screen.queryByText('项目操作')).toBeNull()
  })

  it('时间线可收起', () => {
    render(<App />)
    fireEvent.click(screen.getByLabelText('切换时间线'))
    expect(screen.queryByText('00:20')).toBeNull()
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
    expect(screen.queryByText('智能集合')).toBeNull()
    expect(screen.queryByText('00:20')).toBeNull()
    expect(screen.getByText('项目操作')).toBeTruthy()
  })
})
