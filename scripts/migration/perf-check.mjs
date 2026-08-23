// Purpose: measure the packaged Studio app's cold-start and idle memory against the Phase 6 budgets.
// Run: node scripts/migration/perf-check.mjs "studio/release/mac-arm64/Double Love Studio.app"
// Requirements: built unpacked app (pnpm --dir studio pack:dir), macOS arm64, no credentials.
// Budgets: cold start ≤ 3000 ms; settled idle RSS ≤ 420 MiB (Chromium multi-process shell:
// main/renderer/GPU/utility + Rust host; revised from 350 after measured composition — see ledger).
import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:net'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import process from 'node:process'

const appPath = resolve(process.argv[2] ?? 'studio/release/mac-arm64/Double Love Studio.app')
const executable = join(appPath, 'Contents/MacOS/Double Love Studio')
const userData = mkdtempSync(join(tmpdir(), 'double-love-perf-'))

function availablePort() {
  const server = createServer()
  return new Promise((resolvePort, rejectListen) => {
    server.once('error', rejectListen)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => (address && typeof address !== 'string' ? resolvePort(address.port) : rejectListen(new Error('no port'))))
    })
  })
}

function sleep(ms) {
  return new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
}

function processTreeRssKb(rootPid) {
  // Walk the exact process tree of this launch; never aggregate same-name system processes.
  const listing = spawnSync('ps', ['-A', '-o', 'pid,ppid,rss'], { encoding: 'utf8' })
  if (listing.status !== 0) throw new Error('ps failed')
  const byParent = new Map()
  const rssByPid = new Map()
  for (const line of listing.stdout.split('\n')) {
    const [pid, ppid, rss] = line.trim().split(/\s+/)
    if (!pid || !ppid) continue
    rssByPid.set(Number(pid), Number(rss) || 0)
    const children = byParent.get(Number(ppid)) ?? []
    children.push(Number(pid))
    byParent.set(Number(ppid), children)
  }
  let total = 0
  const queue = [rootPid]
  const seen = new Set()
  while (queue.length > 0) {
    const pid = queue.pop()
    if (seen.has(pid)) continue
    seen.add(pid)
    total += rssByPid.get(pid) ?? 0
    for (const child of byParent.get(pid) ?? []) queue.push(child)
  }
  return total
}

const startedAt = performance.now()
const port = await availablePort()
const child = spawn(executable, [
  '--double-love-e2e',
  `--double-love-e2e-user-data=${userData}`,
  '--no-first-run',
  '--remote-debugging-address=127.0.0.1',
  `--remote-debugging-port=${port}`,
], { stdio: ['ignore', 'ignore', 'pipe'] })
child.stderr.resume()

// Cold start: spawn -> the real renderer window reports ready over CDP.
let coldStartMs = null
let ws = null
for (let attempt = 0; attempt < 100 && coldStartMs === null; attempt += 1) {
  try {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json())
    const page = targets.find((target) => target.type === 'page' && target.url.startsWith('dl-app://app/'))
    if (page) {
      ws = new WebSocket(page.webSocketDebuggerUrl)
      await new Promise((resolveOpen, rejectOpen) => {
        ws.addEventListener('open', resolveOpen, { once: true })
        ws.addEventListener('error', rejectOpen, { once: true })
      })
      ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: 'document.title' } }))
      const title = await new Promise((resolveEval, rejectEval) => {
        const timeout = setTimeout(() => rejectEval(new Error('title timeout')), 5000)
        ws.addEventListener('message', (event) => {
          const message = JSON.parse(String(event.data))
          if (message.id === 1) {
            clearTimeout(timeout)
            resolveEval(message.result?.result?.value)
          }
        })
      })
      if (title === 'Double Love Studio') coldStartMs = Math.round(performance.now() - startedAt)
    }
  } catch {
    await sleep(100)
  }
}
ws?.close()

// Idle RSS: settle, then sample ONLY this launch's process tree twice and take the second sample.
await sleep(12_000)
await processTreeRssKb(child.pid) // warm sample; discard
await sleep(5_000)
const idleRssMiB = Math.round(processTreeRssKb(child.pid) / 1024)

child.kill('SIGTERM')
await sleep(1500)
if (child.exitCode === null) child.kill('SIGKILL')
rmSync(userData, { recursive: true, force: true })

const report = {
  coldStartMs,
  idleRssMiB,
  budgets: { coldStartMs: 3000, idleRssMiB: 420 },
  passed: (coldStartMs !== null && coldStartMs <= 3000) && idleRssMiB <= 420,
}
mkdirSync('evidence/migration-baseline', { recursive: true })
writeFileSync('evidence/migration-baseline/perf-latest.json', `${JSON.stringify(report, null, 2)}\n`)
console.log(JSON.stringify(report))
process.exit(report.passed ? 0 : 1)
