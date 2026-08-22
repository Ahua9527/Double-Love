import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MainTrackClip } from '../../../../bindings/MainTrackClip'
import type { MediaAssetSummary } from '../../../../bindings/MediaAssetSummary'
import { MainTrackTimeline } from './MainTrackTimeline'

const captureTargets = new Map<number, HTMLElement>()

beforeEach(() => {
  captureTargets.clear()
  Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
    configurable: true,
    value(this: HTMLElement, pointerId: number) {
      captureTargets.set(pointerId, this)
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'hasPointerCapture', {
    configurable: true,
    value(this: HTMLElement, pointerId: number) {
      return captureTargets.get(pointerId) === this
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'releasePointerCapture', {
    configurable: true,
    value(this: HTMLElement, pointerId: number) {
      if (captureTargets.get(pointerId) === this) captureTargets.delete(pointerId)
    },
  })
})

function clip(id: string, sourceAssetId: string): MainTrackClip {
  return {
    id,
    source_asset_id: sourceAssetId,
    source_in_frame: BigInt(0),
    source_out_frame: BigInt(25),
    order_index: BigInt(id === 'clip-a' ? 0 : 1),
  }
}

function asset(id: string): MediaAssetSummary {
  return {
    id,
    display_name: `${id}.mov`,
    duration_samples: BigInt(48_000),
    audio_sample_rate: BigInt(48_000),
    rate: 'fps_25',
    width: BigInt(1920),
    height: BigInt(1080),
    audio_channels: BigInt(2),
    status: 'prepared',
  }
}

function dataTransfer(value: string) {
  return {
    effectAllowed: '',
    setData: vi.fn(),
    getData: vi.fn(() => value),
  }
}

function pointer(
  target: HTMLElement,
  type: 'pointerdown' | 'pointermove' | 'pointerup' | 'pointercancel' | 'lostpointercapture',
  init: { pointerId?: number; clientX?: number; button?: number; isPrimary?: boolean } = {},
) {
  const event = new Event(type, { bubbles: true, cancelable: true })
  Object.defineProperties(event, {
    pointerId: { configurable: true, value: init.pointerId ?? 1 },
    clientX: { configurable: true, value: init.clientX ?? 0 },
    button: { configurable: true, value: init.button ?? 0 },
    isPrimary: { configurable: true, value: init.isPrimary ?? true },
  })
  fireEvent(target, event)
}

function renderTimeline(selectedId: string | null = null) {
  const onSeek = vi.fn()
  const onSelect = vi.fn()
  const onMove = vi.fn()
  const onTrim = vi.fn()
  const onSplit = vi.fn()
  const onRemove = vi.fn()
  const view = render(
    <MainTrackTimeline
      clips={[clip('clip-a', 'asset-a'), clip('clip-b', 'asset-b')]}
      assets={[asset('asset-a'), asset('asset-b')]}
      selectedId={selectedId}
      timeline={null}
      playheadSec={0}
      outputRate="fps_25"
      onSeek={onSeek}
      onSelect={onSelect}
      onMove={onMove}
      onTrim={onTrim}
      onSplit={onSplit}
      onRemove={onRemove}
      onAdd={vi.fn()}
    />,
  )
  return { ...view, onSeek, onSelect, onMove, onTrim, onSplit, onRemove }
}

function setTrackRect(container: HTMLElement, left = 100, width = 500): HTMLElement {
  const track = container.querySelector<HTMLElement>('.studio-track')
  if (!track) throw new Error('track not found')
  Object.defineProperty(track, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({
      left,
      right: left + width,
      top: 0,
      bottom: 63,
      width,
      height: 63,
      x: left,
      y: 0,
      toJSON: () => ({}),
    }),
  })
  return track
}

describe('MainTrackTimeline 播放头', () => {
  it('使用真实内容容器边界并按 timings 对齐片段', () => {
    const { container } = renderTimeline()
    const track = setTrackRect(container)
    const clips = container.querySelectorAll<HTMLElement>('.studio-track-clip')

    expect(clips[0].style.left).toBe('0%')
    expect(clips[0].style.width).toBe('50%')
    expect(clips[1].style.left).toBe('50%')
    expect(clips[1].style.width).toBe('50%')
    expect(track.querySelector('.studio-track-end-drop')).toBeNull()
  })

  it('按下立即定位，capture 后拖出区域仍连续更新，up 后停止', () => {
    const { container, onSeek } = renderTimeline()
    const track = setTrackRect(container)

    pointer(track, 'pointerdown', { pointerId: 1, clientX: 100, button: 0, isPrimary: true })
    pointer(track, 'pointermove', { pointerId: 1, clientX: 600 })
    pointer(track, 'pointerup', { pointerId: 1, clientX: 600 })
    pointer(track, 'pointermove', { pointerId: 1, clientX: 350 })

    expect(onSeek).toHaveBeenNthCalledWith(1, 0)
    expect(onSeek).toHaveBeenNthCalledWith(2, 2)
    expect(onSeek).toHaveBeenCalledTimes(2)
    expect(captureTargets.has(1)).toBe(false)
  })

  it('忽略次要按键和非主指针', () => {
    const { container, onSeek } = renderTimeline()
    const track = setTrackRect(container)

    pointer(track, 'pointerdown', { pointerId: 9, clientX: 350, button: 1, isPrimary: true })
    pointer(track, 'pointerdown', { pointerId: 10, clientX: 350, button: 0, isPrimary: false })

    expect(onSeek).not.toHaveBeenCalled()
    expect(captureTargets.has(9)).toBe(false)
    expect(captureTargets.has(10)).toBe(false)
  })

  it('pointercancel 和 lostpointercapture 都会结束 scrub', () => {
    const { container, onSeek } = renderTimeline()
    const track = setTrackRect(container)

    pointer(track, 'pointerdown', { pointerId: 2, clientX: 200, button: 0, isPrimary: true })
    pointer(track, 'pointercancel', { pointerId: 2 })
    pointer(track, 'pointermove', { pointerId: 2, clientX: 400 })
    expect(onSeek).toHaveBeenCalledTimes(1)
    expect(captureTargets.has(2)).toBe(false)

    pointer(track, 'pointerdown', { pointerId: 3, clientX: 200, button: 0, isPrimary: true })
    pointer(track, 'lostpointercapture', { pointerId: 3 })
    pointer(track, 'pointermove', { pointerId: 3, clientX: 400 })
    expect(onSeek).toHaveBeenCalledTimes(2)
  })

  it('ruler slider 提供 ARIA 信息和键盘定位', () => {
    const { onSeek } = renderTimeline()
    const slider = screen.getByRole('slider', { name: '时间线播放头' })

    expect(slider.getAttribute('aria-valuemin')).toBe('0')
    expect(slider.getAttribute('aria-valuemax')).toBe('2')
    expect(slider.getAttribute('aria-valuenow')).toBe('0')
    fireEvent.keyDown(slider, { key: 'ArrowRight' })
    fireEvent.keyDown(slider, { key: 'End' })
    fireEvent.keyDown(slider, { key: 'Home' })

    expect(onSeek).toHaveBeenNthCalledWith(1, 1)
    expect(onSeek).toHaveBeenNthCalledWith(2, 2)
    expect(onSeek).toHaveBeenNthCalledWith(3, 0)
  })
})

describe('MainTrackTimeline 控件隔离', () => {
  it('裁切、拆分、删除和排序不会触发 seek', () => {
    const { container, onSeek, onMove, onSplit, onRemove } = renderTimeline('clip-a')
    const track = setTrackRect(container)
    const clips = container.querySelectorAll<HTMLElement>('.studio-track-clip')
    const leftTrim = screen.getByRole('button', { name: '裁切 asset-a.mov 的左侧' })
    const split = screen.getByRole('button', { name: '在播放头拆分' })
    const remove = screen.getByRole('button', { name: '移除片段' })

    pointer(leftTrim, 'pointerdown', { pointerId: 4, clientX: 100, button: 0, isPrimary: true })
    pointer(leftTrim, 'pointermove', { pointerId: 4, clientX: 120 })
    pointer(leftTrim, 'pointerup', { pointerId: 4, clientX: 120 })
    pointer(split, 'pointerdown', { pointerId: 5, clientX: 200, button: 0, isPrimary: true })
    fireEvent.click(split)
    pointer(remove, 'pointerdown', { pointerId: 6, clientX: 220, button: 0, isPrimary: true })
    fireEvent.click(remove)

    const transfer = dataTransfer('clip-a')
    pointer(clips[0], 'pointerdown', { pointerId: 7, clientX: 120, button: 0, isPrimary: true })
    fireEvent.dragStart(clips[0], { dataTransfer: transfer })
    fireEvent.drop(clips[1], { dataTransfer: transfer })

    const endDrop = container.querySelector<HTMLElement>('.studio-track-end-drop')
    if (!endDrop) throw new Error('end drop not found')
    pointer(endDrop, 'pointerdown', { pointerId: 8, clientX: 590, button: 0, isPrimary: true })
    fireEvent.drop(endDrop, { dataTransfer: dataTransfer('clip-a') })

    expect(onSeek).not.toHaveBeenCalled()
    expect(onMove).toHaveBeenCalledWith('clip-a', 'clip-b')
    expect(onMove).toHaveBeenCalledWith('clip-a', null)
    expect(onSplit).toHaveBeenCalledWith(expect.objectContaining({ id: 'clip-a' }))
    expect(onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: 'clip-a' }))
    expect(track).toBeTruthy()
  })

  it('片段普通点击仍选择并定位，拖拽后的 click 被抑制', () => {
    const { container, onSeek, onSelect } = renderTimeline()
    const track = setTrackRect(container)
    const clipElement = container.querySelector<HTMLElement>('.studio-track-clip')
    if (!clipElement) throw new Error('clip not found')

    fireEvent.click(clipElement, { clientX: 350 })
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'clip-a' }))
    expect(onSeek).toHaveBeenCalledWith(1)

    const transfer = dataTransfer('clip-a')
    fireEvent.dragStart(clipElement, { dataTransfer: transfer })
    fireEvent.dragEnd(clipElement)
    fireEvent.click(clipElement, { clientX: 350 })
    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSeek).toHaveBeenCalledTimes(1)
    expect(track).toBeTruthy()
  })
})
