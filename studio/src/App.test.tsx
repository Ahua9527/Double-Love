import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import App from './App'

describe('App 冒烟渲染', () => {
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
