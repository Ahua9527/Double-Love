import { fireEvent, render, screen } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { describe, expect, it, vi } from 'vitest'
import type { MediaAssetSummary } from '../../../../bindings/MediaAssetSummary'
import { MediaDrawer } from './MediaDrawer'

const importedAsset: MediaAssetSummary = {
  id: 'asset-1',
  display_name: 'interview.mov',
  duration_samples: BigInt(48_000),
  audio_sample_rate: BigInt(48_000),
  rate: 'fps_25',
  width: BigInt(1920),
  height: BigInt(1080),
  audio_channels: BigInt(2),
  status: 'transcribed',
  prepared_available: true,
}

function renderDrawer(overrides: Partial<ComponentProps<typeof MediaDrawer>> = {}) {
  const onDropFiles = vi.fn()
  const onRemove = vi.fn()
  render(
    <MediaDrawer
      assets={[importedAsset]}
      busyAssetId={null}
      onClose={vi.fn()}
      onAddExisting={vi.fn()}
      onImport={vi.fn()}
      onDropFiles={onDropFiles}
      onRemove={onRemove}
      usageCount={() => 2}
      {...overrides}
    />,
  )
  return { onDropFiles, onRemove }
}

describe('MediaDrawer 素材管理', () => {
  it('按 Finder 原始顺序提交多文件拖入', () => {
    const { onDropFiles } = renderDrawer()
    const files = [new File(['a'], 'a.mov'), new File(['b'], 'b.mp4')]
    fireEvent.drop(screen.getByRole('dialog', { name: '添加主轨素材' }), {
      dataTransfer: { files },
    })
    expect(onDropFiles).toHaveBeenCalledWith(files)
  })

  it('右键或键盘删除会先显示影响范围，再由用户确认', () => {
    const { onRemove } = renderDrawer()
    const row = screen.getByRole('button', { name: /interview\.mov/ })
    fireEvent.contextMenu(row)
    expect(screen.getByRole('alertdialog', { name: '从项目中删除素材' }).textContent).toContain('将移除 2 个主轨片段')
    expect(screen.getByRole('alertdialog').textContent).toContain('外部原始视频不会被删除')
    fireEvent.click(screen.getByRole('button', { name: '从项目中删除' }))
    expect(onRemove).toHaveBeenCalledWith(importedAsset)

    fireEvent.keyDown(row, { key: 'Delete' })
    expect(screen.getByRole('alertdialog', { name: '从项目中删除素材' })).toBeTruthy()
  })
})
