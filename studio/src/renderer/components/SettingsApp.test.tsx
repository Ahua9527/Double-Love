import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { SettingsApp } from './SettingsApp'

describe('SettingsApp', () => {
  it('在浏览器预览中展示七页设置并明确未连接桌面服务', () => {
    render(<SettingsApp />)
    expect(screen.getByText(/浏览器预览：读取和操作模型/)).toBeTruthy()
    expect(screen.getByRole('heading', { name: '通用' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '本地模型' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '诊断' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '本地模型' }))
    expect(screen.getByRole('heading', { name: '本地模型' })).toBeTruthy()
    expect(screen.getAllByText('Qwen3 ASR · 0.6B').length).toBeGreaterThan(0)
  })

  it('浏览器预览的模型操作不会伪装成成功', () => {
    render(<SettingsApp initialPage="models" />)
    fireEvent.click(screen.getAllByRole('button', { name: '安装' })[0])
    expect(screen.getByText('浏览器预览：模型操作需要在桌面应用中执行。')).toBeTruthy()
  })
})
