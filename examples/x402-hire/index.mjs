// The Nearbee demo buyer: hire a market agent agent-to-agent and pay per
// call with x402 (USDC on NEAR). See the vendored caller skill
// (.agents/skills/near-agent-market-x402-caller) and docs/market.md.
//
//   node index.mjs <handle> "<task text>" [--max-usd 0.05] [--dry-run]
//
// Environment: NEAR_CALLER_ACCOUNT_ID, NEAR_CALLER_PRIVATE_KEY (ed25519,
// never printed), NEAR_RPC_URL (default
// https://rpc.mainnet.fastnear.com), NEAR_A2A_URL (default
// https://<handle>.market.near.ai/a2a/v1, https only, overridable for
// tests), NEAR_MAX_USD (default of --max-usd).
// .env next to this file is read when present (NEARBEE_* variables are not
// used here).
//
// Safety rails before anything is signed or sent: the handle has the
// market's shape, the A2A and RPC URLs are https, the asset contract is on
// the allowlist for the network, the quoted amount is at most --max-usd,
// and maxTimeoutSeconds is capped.

import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  actions,
  baseDecode,
  buildDelegateAction,
  encodeDelegateAction,
  encodeSignedDelegate,
  JsonRpcProvider,
  KeyPair,
  KeyPairSigner,
} from 'near-api-js'

const DEFAULT_RPC = 'https://rpc.mainnet.fastnear.com'
const YOCTO_DEPOSIT = 1n
const TGAS = 30000000000000n
const USDC = {
  // USDC spl-int-like contract ids on NEAR (solidity separator form).
  'near:mainnet': '17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1',
  'near:testnet': 'usdc.testnet',
}
// The longest block window the caller accepts. The market quotes
// `maxTimeoutSeconds = sla + 600` (docs/market.md, the caller skill), so
// the cap must clear the longest listed SLA: 7 days plus the 600 s the
// market adds. Signing a shorter window is not an option: the skill says
// "Never shorten the validity below maxTimeoutSeconds".
const MAX_TIMEOUT_SECONDS_CAP = 604800 + 600
const USDC_DECIMALS = 6

// --- env --------------------------------------------------------------
export function loadEnv() {
  const vars = { ...process.env }
  try {
    const here = path.dirname(fileURLToPath(import.meta.url))
    const text = readFileSync(path.join(here, '.env'), 'utf8')
    for (const line of text.split('\n')) {
      const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/)
      if (m && !(m[1] in process.env)) vars[m[1]] = m[2].replace(/^["']|["']$/g, '')
    }
  } catch {
    // No .env file: the process environment is enough.
  }
  return vars
}

export function readCaller(vars) {
  const accountId = vars.NEAR_CALLER_ACCOUNT_ID
  if (!accountId) {
    console.error('error: NEAR_CALLER_ACCOUNT_ID is not set')
    process.exitCode = 2
    return null
  }
  const privateKey = vars.NEAR_CALLER_PRIVATE_KEY
  if (!privateKey) {
    console.error('error: NEAR_CALLER_PRIVATE_KEY is not set')
    process.exitCode = 2
    return null
  }
  const rpcUrl = vars.NEAR_RPC_URL || DEFAULT_RPC
  if (!httpsOk(rpcUrl)) {
    console.error('error: NEAR_RPC_URL must be an https:// URL (http only for a loopback host)')
    process.exitCode = 2
    return null
  }
  return { accountId, privateKey, rpcUrl }
}

/** Atomic USDC units of a decimal dollar amount, or null when malformed. */
export function usdToAtomic(usd) {
  const m = /^(\d+)(?:\.(\d{1,6}))?$/.exec(usd.trim())
  if (!m) return null
  const frac = (m[2] || '').padEnd(USDC_DECIMALS, '0')
  return BigInt(m[1] + frac)
}

// --- guard rails ---------------------------------------------------------

/** A market handle: 3 to 30 characters, [a-z][a-z0-9-]*, no dots or
 * slashes, so it cannot change the host it is interpolated into. */
export function handleOk(handle) {
  return /^[a-z][a-z0-9-]{2,29}$/.test(handle)
}

/** The quote checks that run before anything is signed. `maxUsdAtomic` is
 * the caller's price ceiling in atomic USDC units. */
export function checkRow(row, { maxUsdAtomic, network = 'near:mainnet' }) {
  const errors = []
  const allowed = USDC[network]
  if (!allowed || row.asset !== allowed) {
    errors.push(`the asset ${row.asset} is not the allowlisted USDC contract for ${network}`)
  }
  const amount = /^(\d+)$/.test(String(row.amount)) ? BigInt(row.amount) : null
  if (amount === null) {
    errors.push(`the amount ${row.amount} is not an integer atomic amount`)
  } else if (maxUsdAtomic !== null && amount > maxUsdAtomic) {
    errors.push(`the quoted amount exceeds --max-usd`)
  }
  const timeout = Number(row.maxTimeoutSeconds)
  if (!Number.isFinite(timeout) || timeout <= 0 || timeout > MAX_TIMEOUT_SECONDS_CAP) {
    errors.push(`maxTimeoutSeconds ${row.maxTimeoutSeconds} is outside 1..${MAX_TIMEOUT_SECONDS_CAP}`)
  }
  return errors
}

// --- payment construction ----------------------------------------------

/** Picks the accepts[] row of the network the caller holds funds on
 * (`near:mainnet` today; the row is taken verbatim from the challenge). */
export function pickRow(challenge, network = 'near:mainnet') {
  const rows = challenge.accepts || []
  const row = rows.find((r) => r.network === network)
  if (!row) {
    throw new Error(
      `the challenge names no ${network} row; networks offered: ${rows.map((r) => r.network).join(', ') || 'none'}`,
    )
  }
  return row
}

/** The single delegate action the scheme asks for: ft_transfer of `amount`
 * on the asset contract to `payTo`, 1 yoctoNEAR deposit, 30 Tgas. */
export function delegateAction(row) {
  return actions.functionCall(
    'ft_transfer',
    { receiver_id: row.payTo, amount: row.amount },
    TGAS,
    YOCTO_DEPOSIT,
  )
}

/** Signs the delegate exactly as NEP-366 requires, with near-api-js's own
 * delegate signing helper: the signature covers the SHA-256 of the
 * NEP-461-prefixed borsh encoding of the delegate action. The nonce is the
 * access key's nonce plus one, from the chain view. */
export async function signDelegate({ accountId, privateKey, row, blockHash, accessKeyNonce }) {
  const keyPair = KeyPair.fromString(privateKey)
  const action = delegateAction(row)
  const delegate = await buildDelegateAction({
    senderId: accountId,
    publicKey: keyPair.getPublicKey(),
    receiverId: row.asset,
    actions: [action],
    nonce: BigInt(accessKeyNonce) + 1n,
    maxBlockHeight: BigInt(blockHash.height) + BigInt(row.maxTimeoutSeconds),
    blockHash: baseDecode(blockHash.hash),
  })
  const signer = new KeyPairSigner(keyPair)
  const { delegateHash, signedDelegate } = await signer.signDelegateAction(delegate)
  return { delegateAction: delegate, signature: signedDelegate.signature, delegateHash }
}

/** The X-PAYMENT header: base64 of {x402Version, accepted (verbatim),
 * payload{signedDelegateAction}}. */
export function paymentHeader(row, signed) {
  const bytes = encodeSignedDelegate(signed)
  const payload = { signedDelegateAction: Buffer.from(bytes).toString('base64') }
  const header = { x402Version: 2, accepted: row, payload }
  return Buffer.from(JSON.stringify(header)).toString('base64')
}

// --- local verification -------------------------------------------------

/** Verifies the caller's own signed delegate before anything is sent.
 * `expect.accessKey` carries the RPC's `view_access_key` answer; it proves
 * the key exists on the caller account, is full access, and anchors the
 * nonce. Returns { ok, checks: [{name, ok, why}] }. */
export function verifyDelegate(signed, expect) {
  const checks = []
  const add = (name, ok, why = '') => checks.push({ name, ok, why })
  const accessKey = expect.accessKey
  add(
    'the access key exists on the caller account',
    !!accessKey,
    accessKey ? 'view_access_key answered' : 'the RPC has no such access key for this account',
  )
  if (accessKey) {
    const permission = accessKey.permission
    add(
      'the access key is full access',
      permission === 'FullAccess',
      permission === 'FullAccess' ? 'FullAccess' : JSON.stringify(permission).slice(0, 120),
    )
    // Independent check of the signature: the public key derived from the
    // caller's private key verifies the NEP-366 hash of the delegate.
    const keyPair = KeyPair.fromString(expect.privateKey)
    const hash = createHash('sha256')
      .update(Buffer.from(encodeDelegateAction(signed.delegateAction)))
      .digest()
    const verified = keyPair.getPublicKey().verify(
      new Uint8Array(hash),
      new Uint8Array(signed.signature.data),
    )
    add('the signature verifies over the NEP-366 hash', verified, keyPair.getPublicKey().toString())
    add(
      'the nonce is the access key nonce plus one',
      BigInt(signed.delegateAction.nonce) === BigInt(accessKey.nonce) + 1n,
      `signed ${signed.delegateAction.nonce}, key ${accessKey.nonce}`,
    )
  }
  add('signer is the caller account', signed.delegateAction.senderId === expect.accountId, signed.delegateAction.senderId)
  add('receiver is the asset contract', signed.delegateAction.receiverId === expect.asset, signed.delegateAction.receiverId)
  const actions = signed.delegateAction.actions
  add('exactly one action', actions.length === 1, String(actions.length))
  if (actions.length === 1) {
    const a = actions[0]
    const fn = a.functionCall
    let args = fn && fn.args
    if (args instanceof Uint8Array) {
      try {
        args = JSON.parse(Buffer.from(args).toString('utf8'))
      } catch {
        args = {}
      }
    }
    add(
      'the action is ft_transfer to payTo of the quoted amount with 1 yoctoNEAR',
      !!fn &&
        fn.methodName === 'ft_transfer' &&
        fn.deposit?.toString?.() === '1' &&
        args &&
        args.receiver_id === expect.payTo &&
        args.amount === expect.amount,
      fn ? `${fn.methodName} -> ${args && args.receiver_id} ${args && args.amount}` : 'not a function call',
    )
    const gas = fn && fn.gas && fn.gas.toString()
    add('gas is 30 Tgas', gas === TGAS.toString(), gas)
  }
  add(
    'the block window reaches past the quoted timeout',
    Number(signed.delegateAction.maxBlockHeight) >
      Number(expect.currentBlockHeight || 0) + Number(expect.maxTimeoutSeconds || 0) - 2,
    `max ${signed.delegateAction.maxBlockHeight}, now ${expect.currentBlockHeight}`,
  )
  return { ok: checks.every((c) => c.ok), checks }
}

// --- the call ------------------------------------------------------------

/** A market handle interpolated into a hostname is checked first. The
 * endpoint must be https, except for a loopback host, which exists for
 * the tests (the same rule the market client applies). */
function a2aUrl(handle) {
  if (!handleOk(handle)) {
    throw new Error(`"${handle}" is not a market handle (expected [a-z][a-z0-9-]{2,29})`)
  }
  const url = process.env.NEAR_A2A_URL || `https://${handle}.market.near.ai/a2a/v1`
  if (!httpsOk(url)) {
    throw new Error('the A2A endpoint must be an https:// URL (http only for a loopback host)')
  }
  return url
}

/** https required, except for a loopback host (127.0.0.1, [::1],
 * localhost): cleartext only ever travels on this machine. */
export function httpsOk(url) {
  if (url.startsWith('https://')) return true
  if (!url.startsWith('http://')) return false
  let host
  try {
    host = new URL(url).hostname
  } catch {
    return false
  }
  return host === 'localhost' || host === '[::1]' || /^127\.\d+\.\d+\.\d+$/.test(host) || host === '::1'
}

/** The agent card names the A2A endpoint the caller should use. */
export async function readAgentCard(handle) {
  const base = process.env.NEAR_A2A_URL
    ? new URL(process.env.NEAR_A2A_URL).origin
    : `https://${handle}.market.near.ai`
  const res = await fetch(`${base}/.well-known/agent-card.json`)
  if (!res.ok) {
    throw new Error(`the agent card answered HTTP ${res.status}`)
  }
  return res.json()
}

function sendMessageBody(task) {
  return {
    jsonrpc: '2.0',
    id: 1,
    method: 'SendMessage',
    params: {
      message: {
        messageId: crypto.randomUUID(),
        role: 'ROLE_USER',
        parts: [{ text: task }],
      },
      configuration: { historyLength: 0 },
    },
  }
}

async function unpaidCall(a2a, body) {
  const res = await fetch(a2a, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (res.status !== 402) {
    const text = await res.text()
    throw new Error(`expected the 402 challenge, got HTTP ${res.status}: ${text.slice(0, 400)}`)
  }
  return res.json()
}

async function paidCall(a2a, body, xPayment) {
  // `returnImmediately` is part of the SendMessage params configuration
  // (the A2A spec puts it there), not a field of the JSON-RPC envelope.
  const paidBody = {
    ...body,
    params: {
      ...body.params,
      configuration: { ...body.params.configuration, returnImmediately: true },
    },
  }
  const res = await fetch(a2a, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'X-PAYMENT': xPayment },
    body: JSON.stringify(paidBody),
  })
  // A non-JSON failure must not hide its cause after the money is spent.
  const type = res.headers.get('content-type') || ''
  if (res.status >= 400 || !type.includes('json')) {
    const text = await res.text()
    throw new Error(`the paid call answered HTTP ${res.status} (${type || 'no content type'}): ${text.slice(0, 400)}`)
  }
  return res.json()
}

async function getTask(a2a, taskId) {
  const res = await fetch(a2a, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 2,
      method: 'GetTask',
      params: { id: taskId, historyLength: 0 },
    }),
  })
  if (!res.ok) {
    throw new Error(`GetTask answered HTTP ${res.status}`)
  }
  return res.json()
}

/** Strips control characters other than \n and \t from remote text. */
function printable(text) {
  // eslint-disable-next-line no-control-regex
  return text.replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, '')
}

/** The argument parser: positionals are the arguments that carry no flag,
 * so `--dry-run` and `--max-usd` are honoured wherever they appear in the
 * command line (N5: a misplaced flag must never become the task text). */
export function parseArgs(argv) {
  let maxUsd = process.env.NEAR_MAX_USD || null
  let dryRun = false
  const positionals = []
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--max-usd') {
      maxUsd = argv[i + 1]
      i++
    } else if (argv[i] === '--dry-run') {
      dryRun = true
    } else {
      positionals.push(argv[i])
    }
  }
  const [handle, task] = positionals
  return { handle, task, dryRun, maxUsd }
}

async function main() {
  const { handle, task, dryRun, maxUsd } = parseArgs(process.argv.slice(2))
  if (!handle || !task) {
    console.error('usage: node index.mjs <handle> "<task text>" [--max-usd 0.05] [--dry-run]')
    process.exitCode = 2
    return
  }
  if (!handleOk(handle)) {
    console.error(`error: "${handle}" is not a market handle (expected [a-z][a-z0-9-]{2,29})`)
    process.exitCode = 2
    return
  }
  const maxUsdAtomic = usdToAtomic(maxUsd ?? '')
  if (maxUsdAtomic === null) {
    console.error('error: give a price ceiling with --max-usd <usd> (or NEAR_MAX_USD), e.g. 0.05')
    process.exitCode = 2
    return
  }
  const caller = readCaller(loadEnv())
  if (!caller) return

  const a2a = a2aUrl(handle)
  // The agent card names the A2A endpoint; it is read before any call.
  const card = await readAgentCard(handle)
  const cardUrl = card && card.preferredTransport && card.url
  if (cardUrl && cardUrl !== a2a) {
    console.log(`note: the agent card names the endpoint ${cardUrl}`)
  }

  const body = sendMessageBody(task)
  const challenge = await unpaidCall(a2a, body)
  const row = pickRow(challenge)
  const quoteErrors = checkRow(row, { maxUsdAtomic })
  if (quoteErrors.length > 0) {
    for (const e of quoteErrors) console.error(`error: ${e}`)
    process.exitCode = 1
    return
  }
  console.log(
    `quote: ${row.amount} atomic on ${row.network} to ${row.payTo}, window ${row.maxTimeoutSeconds}s`,
  )

  const provider = new JsonRpcProvider({ url: caller.rpcUrl })
  const publicKey = KeyPair.fromString(caller.privateKey).getPublicKey()
  let accessKey
  try {
    // The key check is real: view_access_key with the derived public key
    // (near-api-js v7's viewAccessKey). The answer has no public_key
    // field, so the key identity is proven by asking for this exact key.
    accessKey = await provider.viewAccessKey({
      accountId: caller.accountId,
      publicKey: publicKey.toString(),
      finalityQuery: { finality: 'final' },
    })
  } catch {
    accessKey = null
  }
  const block = await provider.viewBlock({ finality: 'final' })
  const blockHash = { hash: block.header.hash, height: block.header.height }

  const signed = await signDelegate({
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    row,
    blockHash,
    accessKeyNonce: accessKey ? accessKey.nonce : 0,
  })
  const verdict = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey,
    asset: row.asset,
    payTo: row.payTo,
    amount: row.amount,
    maxTimeoutSeconds: row.maxTimeoutSeconds,
    currentBlockHeight: blockHash.height,
  })
  for (const c of verdict.checks) {
    console.log(`  ${c.ok ? 'ok' : 'FAIL'} ${c.name}${c.why ? ` (${c.why})` : ''}`)
  }
  if (!verdict.ok) {
    console.error('error: the local verification failed; nothing was sent')
    process.exitCode = 1
    return
  }
  if (dryRun) {
    console.log('dry run: the signed delegate verified locally; nothing was sent')
    return
  }

  const answer = await paidCall(a2a, body, paymentHeader(row, signed))
  if (answer.error) {
    console.error(`error: ${JSON.stringify(answer.error).slice(0, 400)}`)
    process.exitCode = 1
    return
  }
  const t = answer.result && answer.result.task
  const taskId = t && t.id
  if (!taskId) {
    console.error(
      `error: the paid answer named no task id: ${JSON.stringify(answer.result || answer).slice(0, 400)}`,
    )
    process.exitCode = 1
    return
  }
  console.log(`task: ${taskId}`)
  let state = t && t.status && t.status.state
  const deadline = Date.now() + (Number(row.maxTimeoutSeconds) + 60) * 1000
  while (state !== 'TASK_STATE_COMPLETED' && state !== 'TASK_STATE_REJECTED' && state !== 'TASK_STATE_FAILED' && state !== 'TASK_STATE_CANCELED' && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 10000))
    // A single failed poll must not lose the paid result: each poll is
    // retried before the loop gives up.
    let poll = null
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        poll = await getTask(a2a, taskId)
        break
      } catch (e) {
        if (attempt === 2) throw e
        await new Promise((r) => setTimeout(r, 2000))
      }
    }
    const rt = poll && poll.result && poll.result.id ? poll.result : null
    state = rt && rt.status && rt.status.state
    if (rt && rt.artifacts && t) {
      t.artifacts = rt.artifacts
    }
  }
  console.log(`state: ${state}`)
  for (const a of (t && t.artifacts) || []) {
    for (const p of a.parts || []) {
      if (p.text) console.log(printable(p.text))
    }
  }
  if (state !== 'TASK_STATE_COMPLETED') process.exitCode = 1
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
if (isMain) {
  main().catch((e) => {
    console.error(`error: ${e.message}`)
    process.exitCode = 1
  })
}
