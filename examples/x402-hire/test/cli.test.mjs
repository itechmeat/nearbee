// The spawned-CLI proof: `main` is run as a real process against a local
// stub of the market A2A endpoint and the NEAR RPC. With `--dry-run` no
// paid request leaves the process (T-02/T-03); in paid mode the
// X-PAYMENT header and the `returnImmediately` flag sit exactly where the
// A2A spec puts them (T-21, N7). No money moves: the stub is local.

import test from 'node:test'
import assert from 'node:assert/strict'
import http from 'node:http'
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { KeyPair, baseEncode } from 'near-api-js'

const here = path.dirname(fileURLToPath(import.meta.url))
const indexMjs = path.join(here, '..', 'index.mjs')

const USDC = '17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1'

// What the market really quotes: the default plan's sla_seconds + 600.
const ROW = {
  scheme: 'exact',
  network: 'near:mainnet',
  asset: USDC,
  amount: '25000',
  payTo: 'ea667f5f496e60033bf8075cce4793efdeba28d74d116ef95848324347f539fc',
  maxTimeoutSeconds: 86400 + 600,
}

function startStub({ paidAnswer }) {
  const seen = []
  const server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      seen.push({ url: req.url, method: req.method, headers: req.headers, body })
      if (req.url === '/.well-known/agent-card.json') {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({}))
        return
      }
      if (req.url === '/a2a/v1') {
        if (!req.headers['x-payment']) {
          res.writeHead(402, { 'content-type': 'application/json' })
          res.end(JSON.stringify({ x402Version: 2, error: 'no payment', accepts: [ROW] }))
          return
        }
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify(paidAnswer))
        return
      }
      if (req.url === '/rpc') {
        const rpc = JSON.parse(body)
        const result =
          rpc.method === 'block'
            ? { header: { hash: baseEncode(Buffer.alloc(32, 7)), height: 100_000_000 } }
            : {
                nonce: '5000000',
                permission: 'FullAccess',
                block_height: 100_000_000,
                block_hash: baseEncode(Buffer.alloc(32, 7)),
              }
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result }))
        return
      }
      res.writeHead(404)
      res.end()
    })
  })
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port, seen }))
  })
}

function runCli(args, env) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [indexMjs, ...args], {
      env: { ...process.env, ...env },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    child.stdout.on('data', (c) => (out += c))
    child.stderr.on('data', (c) => (out += c))
    child.on('close', (code) => resolve({ code, out }))
  })
}

async function withStub(paidAnswer, fn) {
  const { server, port, seen } = await startStub({ paidAnswer })
  try {
    return await fn(port, seen)
  } finally {
    server.close()
  }
}

const callerEnv = () => {
  const keyPair = KeyPair.fromRandom('ed25519')
  return {
    NEAR_CALLER_ACCOUNT_ID: 'caller-test.near',
    NEAR_CALLER_PRIVATE_KEY: keyPair.toString(),
  }
}

const completedTask = {
  jsonrpc: '2.0',
  id: 1,
  result: {
    task: {
      id: 'task-1',
      status: { state: 'TASK_STATE_COMPLETED' },
      artifacts: [{ parts: [{ text: 'the audit passed' }] }],
    },
  },
}

test('main with --dry-run sends no paid request', async () => {
  await withStub(completedTask, async (port, seen) => {
    const base = `http://127.0.0.1:${port}`
    const { code, out } = await runCli(
      ['repo-audit-fast', 'Audit the repo', '--max-usd', '0.05', '--dry-run'],
      { ...callerEnv(), NEAR_A2A_URL: `${base}/a2a/v1`, NEAR_RPC_URL: `${base}/rpc` },
    )
    assert.equal(code, 0, `exit ${code}: ${out}`)
    assert.match(out, /dry run/)
    // No request carried a payment header, and the paid route was never
    // answered: the process stopped after the local verification.
    assert.equal(seen.filter((r) => r.headers['x-payment']).length, 0)
    assert.equal(
      seen.filter((r) => r.url === '/a2a/v1' && r.headers['x-payment']).length,
      0,
    )
  })
})

test('the paid call carries X-PAYMENT and returnImmediately in params.configuration', async () => {
  await withStub(completedTask, async (port, seen) => {
    const base = `http://127.0.0.1:${port}`
    const { code, out } = await runCli(
      ['repo-audit-fast', 'Audit the repo', '--max-usd', '0.05'],
      { ...callerEnv(), NEAR_A2A_URL: `${base}/a2a/v1`, NEAR_RPC_URL: `${base}/rpc` },
    )
    assert.equal(code, 0, `exit ${code}: ${out}`)
    assert.match(out, /task: task-1/)
    const paid = seen.find((r) => r.url === '/a2a/v1' && r.headers['x-payment'])
    assert.ok(paid, 'no paid call reached the endpoint')
    const body = JSON.parse(paid.body)
    // The A2A spec puts returnImmediately inside params.configuration of
    // SendMessage, not on the envelope (N7).
    assert.equal(body.params.configuration.returnImmediately, true)
    assert.equal(body.configuration, undefined)
    assert.equal(body.params.message.parts[0].text, 'Audit the repo')
    // The payment header decodes and carries the accepted row verbatim.
    const header = JSON.parse(Buffer.from(paid.headers['x-payment'], 'base64').toString())
    assert.equal(header.accepted.maxTimeoutSeconds, ROW.maxTimeoutSeconds)
    assert.equal(header.accepted.amount, ROW.amount)
    assert.ok(header.payload.signedDelegateAction)
    // The deliverable was printed from the completed task.
    assert.match(out, /the audit passed/)
  })
})
