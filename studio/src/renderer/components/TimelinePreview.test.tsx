import { describe, expect, it } from 'vitest'
import { mediaAssetUrl } from '../media-url'

describe('TimelinePreview media URLs', () => {
  it('uses the internal dl-media asset scheme without accepting path segments', () => {
    expect(mediaAssetUrl('asset-id')).toBe('dl-media://asset/asset-id')
    expect(mediaAssetUrl('/private/source.mp4')).toBe('dl-media://asset/%2Fprivate%2Fsource.mp4')
  })
})
