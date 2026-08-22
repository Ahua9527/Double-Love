// @vitest-environment node

import { describe, expect, it } from 'vitest'
import { PathGrants } from './path-grants'

describe('PathGrants', () => {
  it('consumes a grant exactly once', () => {
    const grants = new PathGrants()
    const issued = grants.create('/private/project', 'project-open')

    expect(grants.consume(issued.token, 'project-open')).toBe('/private/project')
    expect(grants.consume(issued.token, 'project-open')).toMatchObject({ code: 'INVALID_GRANT' })
  })

  it('expires grants lazily', () => {
    let now = 1_000
    const grants = new PathGrants(60_000, () => now)
    const issued = grants.create('/private/media.mov', 'import-media')

    now += 60_000
    expect(grants.consume(issued.token, 'import-media')).toMatchObject({ code: 'INVALID_GRANT' })
  })

  it('rejects the wrong grant kind', () => {
    const grants = new PathGrants()
    const issued = grants.create('/private/export.xml', 'export-save')

    expect(grants.consume(issued.token, 'import-media')).toMatchObject({ code: 'INVALID_GRANT' })
  })

  it('returns an opaque UUID without the path', () => {
    const grants = new PathGrants()
    const issued = grants.create('/Users/example/secret-project', 'project-open')

    expect(issued).toEqual({ token: expect.any(String) })
    expect(issued.token).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u)
    expect(JSON.stringify(issued)).not.toContain('secret-project')
    expect(JSON.stringify(issued)).not.toContain('/Users/example')
  })
})
