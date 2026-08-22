import type { HostResponse } from '../../../bindings/host-protocol/HostResponse'

export interface CompatibleHostHello {
  capabilities: readonly string[]
  hostVersion: string
  engineVersion: string
}

export function assertCompatibleHostHello(
  response: HostResponse,
  protocolVersion: number,
): CompatibleHostHello {
  if (response.status !== 'ok' || response.result.type !== 'hello') {
    throw new Error('Desktop host handshake returned an incompatible response')
  }

  const { data } = response.result
  const hostVersion = data.host_version.trim()
  const engineVersion = data.engine_version.trim()
  if (
    data.protocol !== protocolVersion
    || !data.capabilities.includes('invoke')
    || hostVersion.length === 0
    || engineVersion.length === 0
  ) {
    throw new Error('Desktop host handshake returned an incompatible response')
  }

  return {
    capabilities: Object.freeze([...data.capabilities]),
    hostVersion,
    engineVersion,
  }
}
