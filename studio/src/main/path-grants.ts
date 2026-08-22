import { randomUUID } from 'node:crypto'
import { basename } from 'node:path'

export const GRANT_KINDS = [
  'project-open',
  'import-media',
  'export-save',
  'model-root',
] as const

export type GrantKind = (typeof GRANT_KINDS)[number]

export interface GrantToken {
  token: string
  displayName?: string
}

export interface InvalidGrant {
  code: 'INVALID_GRANT'
  message: string
}

interface StoredGrant {
  path: string
  kind: GrantKind
  expiresAt: number
}

const DEFAULT_TTL_MS = 60_000

export function invalidGrant(): InvalidGrant {
  return { code: 'INVALID_GRANT', message: 'Path grant is missing, expired, or invalid' }
}

export function isInvalidGrant(value: string | InvalidGrant): value is InvalidGrant {
  return typeof value !== 'string'
}

export class PathGrants {
  private readonly grants = new Map<string, StoredGrant>()

  constructor(
    private readonly ttlMs = DEFAULT_TTL_MS,
    private readonly now: () => number = Date.now,
  ) {}

  create(path: string, kind: GrantKind, includeDisplayName = false): GrantToken {
    this.purgeExpired()
    const token = randomUUID()
    this.grants.set(token, { path, kind, expiresAt: this.now() + this.ttlMs })
    return includeDisplayName ? { token, displayName: basename(path) } : { token }
  }

  consume(token: unknown, expectedKind: GrantKind): string | InvalidGrant {
    this.purgeExpired()
    if (typeof token !== 'string' || token.length === 0) return invalidGrant()

    const grant = this.grants.get(token)
    if (!grant || grant.kind !== expectedKind) return invalidGrant()

    this.grants.delete(token)
    return grant.path
  }

  private purgeExpired(): void {
    const now = this.now()
    for (const [token, grant] of this.grants) {
      if (grant.expiresAt <= now) this.grants.delete(token)
    }
  }
}
