// @vitest-environment node

import { describe, expect, it } from 'vitest'
import type { HostResponse } from '../../../bindings/host-protocol/HostResponse'
import { assertCompatibleHostHello } from './host-handshake'

function hello(overrides: Partial<{
  protocol: number
  host_version: string
  engine_version: string
  capabilities: string[]
}> = {}): HostResponse {
  return {
    v: 1,
    id: 'handshake',
    status: 'ok',
    result: {
      type: 'hello',
      data: {
        protocol: 1,
        host_version: '0.1.0',
        engine_version: '0.1.0',
        capabilities: ['handshake', 'health', 'invoke'],
        ...overrides,
      },
    },
  }
}

describe('desktop host startup handshake', () => {
  it('accepts protocol 1 with invoke and non-empty component versions', () => {
    expect(assertCompatibleHostHello(hello(), 1)).toEqual({
      capabilities: ['handshake', 'health', 'invoke'],
      hostVersion: '0.1.0',
      engineVersion: '0.1.0',
    })
  })

  it.each([
    hello({ protocol: 2 }),
    hello({ capabilities: ['handshake', 'health'] }),
    hello({ host_version: '   ' }),
    hello({ engine_version: '' }),
  ])('rejects an incompatible hello payload', (response) => {
    expect(() => assertCompatibleHostHello(response, 1)).toThrow(/incompatible response/)
  })
})
