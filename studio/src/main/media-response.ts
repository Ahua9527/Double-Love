import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import { extname } from 'node:path'
import { Readable } from 'node:stream'

const MIME_TYPES: Readonly<Record<string, string>> = Object.freeze({
  '.mp4': 'video/mp4',
  '.mov': 'video/quicktime',
  '.m4v': 'video/x-m4v',
  '.webm': 'video/webm',
  '.mp3': 'audio/mpeg',
  '.wav': 'audio/wav',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
})

const U64_MAX = 0xffff_ffff_ffff_ffffn

type ParsedRange =
  | { kind: 'full' }
  | { kind: 'range'; start: number; end: number }
  | { kind: 'multi' }
  | { kind: 'invalid' }

export function mimeForPath(path: string): string {
  return MIME_TYPES[extname(path).toLowerCase()] ?? 'application/octet-stream'
}

function parseUnsigned(value: string): bigint | null {
  if (!/^\d+$/u.test(value)) return null
  const parsed = BigInt(value)
  return parsed <= U64_MAX ? parsed : null
}

function parseRange(value: string | null, size: number): ParsedRange {
  if (value === null || !value.startsWith('bytes=')) return { kind: 'full' }
  const spec = value.slice('bytes='.length)
  if (spec.includes(',')) return { kind: 'multi' }
  const separator = spec.indexOf('-')
  if (separator < 0) return { kind: 'invalid' }

  const startText = spec.slice(0, separator)
  const endText = spec.slice(separator + 1)
  if (startText.length === 0) {
    const suffix = parseUnsigned(endText)
    if (suffix === null || suffix === 0n || size === 0) return { kind: 'invalid' }
    const suffixNumber = suffix > BigInt(size) ? size : Number(suffix)
    return { kind: 'range', start: size - suffixNumber, end: size - 1 }
  }

  const start = parseUnsigned(startText)
  const end = endText.length === 0 ? BigInt(Math.max(0, size - 1)) : parseUnsigned(endText)
  if (start === null || end === null || size === 0 || start >= BigInt(size) || start > end) {
    return { kind: 'invalid' }
  }
  return {
    kind: 'range',
    start: Number(start),
    end: Number(end >= BigInt(size) ? BigInt(size - 1) : end),
  }
}

function baseHeaders(mime: string): Headers {
  return new Headers({
    'Accept-Ranges': 'bytes',
    'Content-Type': mime,
  })
}

export function mediaNotFound(): Response {
  return new Response(null, {
    status: 404,
    headers: baseHeaders('text/plain; charset=utf-8'),
  })
}

export async function mediaResponse(
  method: string,
  rangeHeader: string | null,
  path: string,
): Promise<Response> {
  if (method.toUpperCase() !== 'GET' && method.toUpperCase() !== 'HEAD') {
    return new Response(null, { status: 501, headers: baseHeaders(mimeForPath(path)) })
  }

  const mime = mimeForPath(path)
  let size: number
  try {
    const metadata = await stat(path)
    if (!metadata.isFile()) return mediaNotFound()
    size = metadata.size
  } catch {
    return mediaNotFound()
  }

  const parsed = parseRange(rangeHeader, size)
  if (parsed.kind === 'multi') {
    return new Response(null, { status: 501, headers: baseHeaders(mime) })
  }
  if (parsed.kind === 'invalid') {
    const headers = baseHeaders(mime)
    headers.set('Content-Range', `bytes */${size}`)
    return new Response(null, { status: 416, headers })
  }

  const start = parsed.kind === 'range' ? parsed.start : 0
  const end = parsed.kind === 'range' ? parsed.end : Math.max(0, size - 1)
  const length = size === 0 ? 0 : end - start + 1
  const headers = baseHeaders(mime)
  headers.set('Content-Length', String(length))
  if (parsed.kind === 'range') {
    headers.set('Content-Range', `bytes ${start}-${end}/${size}`)
  }

  const isHead = method.toUpperCase() === 'HEAD'
  const body = isHead || length === 0
    ? null
    : Readable.toWeb(createReadStream(path, { start, end })) as ReadableStream<Uint8Array>
  return new Response(body, {
    status: parsed.kind === 'range' ? 206 : 200,
    headers,
  })
}
