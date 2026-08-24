// @vitest-environment node

import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mediaResponse, mimeForPath } from './media-response'

async function responseBytes(response: Response): Promise<number[]> {
  return [...new Uint8Array(await response.arrayBuffer())]
}

describe('mediaResponse', () => {
  let directory: string
  let fixture: string
  const bytes = Uint8Array.from({ length: 100 }, (_, index) => index)

  beforeEach(async () => {
    directory = await mkdtemp(join(tmpdir(), 'double-love-media-response-'))
    fixture = join(directory, 'fixture.mp4')
    await writeFile(fixture, bytes)
  })

  afterEach(async () => {
    await rm(directory, { recursive: true, force: true })
  })

  it('returns a full GET with 200 and Accept-Ranges', async () => {
    const response = await mediaResponse('GET', null, fixture)
    expect(response.status).toBe(200)
    expect(response.headers.get('accept-ranges')).toBe('bytes')
    expect(response.headers.get('content-length')).toBe('100')
    expect(response.headers.get('content-type')).toBe('video/mp4')
    expect(await responseBytes(response)).toEqual([...bytes])
  })

  it('returns exact bytes for a single range', async () => {
    const response = await mediaResponse('GET', 'bytes=10-19', fixture)
    expect(response.status).toBe(206)
    expect(response.headers.get('content-range')).toBe('bytes 10-19/100')
    expect(response.headers.get('content-length')).toBe('10')
    expect(await responseBytes(response)).toEqual([...bytes.slice(10, 20)])
  })

  it('supports open and suffix ranges', async () => {
    const open = await mediaResponse('GET', 'bytes=90-', fixture)
    expect(open.headers.get('content-range')).toBe('bytes 90-99/100')
    expect(await responseBytes(open)).toEqual([...bytes.slice(90)])

    const suffix = await mediaResponse('GET', 'bytes=-5', fixture)
    expect(suffix.headers.get('content-range')).toBe('bytes 95-99/100')
    expect(await responseBytes(suffix)).toEqual([...bytes.slice(95)])
  })

  it('clamps an end beyond the file size', async () => {
    const response = await mediaResponse('GET', 'bytes=95-999', fixture)
    expect(response.status).toBe(206)
    expect(response.headers.get('content-range')).toBe('bytes 95-99/100')
    expect(await responseBytes(response)).toEqual([...bytes.slice(95)])
  })

  it('returns 501 for multiple ranges', async () => {
    const response = await mediaResponse('GET', 'bytes=0-9,20-29', fixture)
    expect(response.status).toBe(501)
    expect(await responseBytes(response)).toEqual([])
  })

  it.each(['bytes=100-200', 'bytes=50-40', 'bytes=abc-1', 'bytes=-0'])(
    'returns 416 with the file size for %s',
    async (range) => {
      const response = await mediaResponse('GET', range, fixture)
      expect(response.status).toBe(416)
      expect(response.headers.get('content-range')).toBe('bytes */100')
      expect(await responseBytes(response)).toEqual([])
    },
  )

  it('ignores an unknown range unit', async () => {
    const response = await mediaResponse('GET', 'items=0-9', fixture)
    expect(response.status).toBe(200)
    expect(await responseBytes(response)).toEqual([...bytes])
  })

  it('returns GET headers and an empty body for HEAD', async () => {
    const response = await mediaResponse('HEAD', 'bytes=0-9', fixture)
    expect(response.status).toBe(206)
    expect(response.headers.get('content-length')).toBe('10')
    expect(response.headers.get('content-range')).toBe('bytes 0-9/100')
    expect(await responseBytes(response)).toEqual([])
  })

  it('returns 404 for a missing file', async () => {
    const response = await mediaResponse('GET', null, join(directory, 'missing.mp4'))
    expect(response.status).toBe(404)
    expect(await responseBytes(response)).toEqual([])
  })

  it('returns 416 for ranges against an empty file, matching the Rust media protocol', async () => {
    const empty = join(directory, 'empty.mp4')
    await writeFile(empty, new Uint8Array())
    for (const range of ['bytes=0-', 'bytes=-5', 'bytes=0-0']) {
      const response = await mediaResponse('GET', range, empty)
      expect(response.status, range).toBe(416)
      expect(response.headers.get('content-range')).toBe('bytes */0')
    }
    const full = await mediaResponse('GET', null, empty)
    expect(full.status).toBe(200)
    expect(full.headers.get('content-length')).toBe('0')
  })

  it('maps supported audio/video MIME types and defaults safely', () => {
    expect(mimeForPath('clip.MOV')).toBe('video/quicktime')
    expect(mimeForPath('voice.wav')).toBe('audio/wav')
    expect(mimeForPath('thumbnail.JPG')).toBe('image/jpeg')
    expect(mimeForPath('unknown.bin')).toBe('application/octet-stream')
  })
})
