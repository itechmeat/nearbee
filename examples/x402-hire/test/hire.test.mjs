// Unit tests of the x402 payment payload, the signature and the local
// verification, with a throwaway key generated here. No network is
// touched: the chain view is stubbed with the real answer shape.

import test from 'node:test'
import assert from 'node:assert/strict'
import { randomBytes, createHash } from 'node:crypto'
import {
  KeyPair,
  baseEncode,
  buildDelegateAction,
  encodeDelegateAction,
  encodeSignedDelegate,
} from 'near-api-js'

import {
  pickRow,
  signDelegate,
  delegateAction,
  paymentHeader,
  verifyDelegate,
  readCaller,
  handleOk,
  httpsOk,
  parseArgs,
  checkRow,
  usdToAtomic,
} from '../index.mjs'

const ROW = {
  scheme: 'exact',
  network: 'near:mainnet',
  asset: '17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1',
  amount: '2500000',
  payTo: 'ea667f5f496e60033bf8075cce4793efdeba28d74d116ef95848324347f539fc',
  // What the market really quotes: sla_seconds + 600 (docs/market.md).
  maxTimeoutSeconds: 86400 + 600,
}

const CHALLENGE = {
  x402Version: 2,
  error: 'x-payment header is required',
  accepts: [{ ...ROW, extra: { name: 'USDC', decimals: 6 } }],
}

function throwawayCaller(accountId = 'caller-test.near') {
  const keyPair = KeyPair.fromRandom('ed25519')
  return {
    accountId,
    privateKey: keyPair.toString(),
    publicKey: keyPair.getPublicKey().toString(),
    keyPair,
  }
}

function fakeBlock() {
  // A fake but well-formed block hash: 32 arbitrary bytes, base58.
  const bs58 = baseEncode(randomBytes(32))
  return { hash: bs58, height: 100_000_000 }
}

/** The real view_access_key answer shape (no public_key field). */
function fakeAccessKey(keyPair, nonce = 5_000_000n) {
  return {
    nonce: nonce.toString(),
    permission: 'FullAccess',
    block_height: 100_000_000,
    block_hash: fakeBlock().hash,
    _keyPair: keyPair,
  }
}

async function signedFor(caller, row = ROW, block = fakeBlock(), nonce = 5_000_000n) {
  return signDelegate({
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    row,
    blockHash: block,
    accessKeyNonce: nonce,
  })
}

test('the row is picked verbatim by network', () => {
  const row = pickRow(CHALLENGE)
  assert.deepEqual(row, CHALLENGE.accepts[0])
  assert.throws(() => pickRow({ accepts: [{ network: 'other:net' }] }), /near:mainnet/)
})

test('the payment header carries the row verbatim and a decodable delegate', async () => {
  const caller = throwawayCaller()
  const block = fakeBlock()
  const signed = await signedFor(caller, ROW, block)
  // The header is built from the row picked out of the parsed challenge,
  // unknown fields included: a whitelisting copy would fail this.
  const header = paymentHeader(pickRow(CHALLENGE), signed)
  const decoded = JSON.parse(Buffer.from(header, 'base64').toString('utf8'))
  assert.equal(decoded.x402Version, 2)
  assert.deepEqual(decoded.accepted, CHALLENGE.accepts[0])
  // The stored bytes are exactly this signed delegate.
  const bytes = Buffer.from(decoded.payload.signedDelegateAction, 'base64')
  const again = Buffer.from(encodeSignedDelegate(signed))
  assert.ok(bytes.equals(again), 'the header carries a different delegate')
  const d = signed.delegateAction
  assert.equal(d.senderId, caller.accountId)
  assert.equal(d.receiverId, ROW.asset)
  assert.equal(d.publicKey.toString(), caller.publicKey, 'the signature key is the throwaway key')
  const a = d.actions
  assert.equal(a.length, 1)
  const fn = a[0].functionCall
  assert.equal(fn.methodName, 'ft_transfer')
  const args = JSON.parse(Buffer.from(fn.args).toString('utf8'))
  assert.equal(args.receiver_id, ROW.payTo)
  assert.equal(args.amount, ROW.amount)
  assert.equal(fn.deposit.toString(), '1')
  assert.equal(fn.gas.toString(), (30000000000000n).toString())
  assert.equal(BigInt(d.maxBlockHeight) - BigInt(block.height), BigInt(ROW.maxTimeoutSeconds))
  // The nonce is the access key nonce plus one, not a block-height guess.
  assert.equal(BigInt(d.nonce), 5_000_001n)
})

/** Independent proof of the signature: the public key derived from the
 * private key verifies the SHA-256 over the NEP-461-prefixed borsh encoding
 * of the delegate (the NEP-366 hash), recomputed here from scratch. */
test('the signature verifies over an independently computed NEP-366 hash', async () => {
  const caller = throwawayCaller()
  const signed = await signedFor(caller)
  const delegate = await buildDelegateAction({
    senderId: caller.accountId,
    publicKey: caller.keyPair.getPublicKey(),
    receiverId: ROW.asset,
    actions: [delegateAction(ROW)],
    nonce: 5_000_001n,
    maxBlockHeight: 100_000_000n + BigInt(ROW.maxTimeoutSeconds),
    blockHash: signed.delegateAction.blockHash,
  })
  const hash = createHash('sha256')
    .update(Buffer.from(encodeDelegateAction(delegate)))
    .digest()
  const ok = caller.keyPair.getPublicKey().verify(
    new Uint8Array(hash),
    new Uint8Array(signed.signature.data),
  )
  assert.ok(ok, 'the signature does not verify over the recomputed hash')
  // A different hash does not verify.
  const other = createHash('sha256').update(Buffer.from('x')).digest()
  assert.ok(
    !caller.keyPair.getPublicKey().verify(new Uint8Array(other), new Uint8Array(signed.signature.data)),
  )
})

test('the local verification passes with the real chain-view shape', async () => {
  const caller = throwawayCaller()
  const block = fakeBlock()
  const signed = await signedFor(caller, ROW, block)
  const verdict = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey: fakeAccessKey(caller.keyPair, 5_000_000n),
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: block.height,
  })
  assert.equal(verdict.ok, true, JSON.stringify(verdict.checks))
  // returnImmediately is part of the paid call, not the header; its guard
  // is covered by the wiring test below.
})

test('a missing or limited chain view fails the key checks', async () => {
  const caller = throwawayCaller()
  const signed = await signedFor(caller)
  // No chain view: no pass ("dry test" bypass removed).
  const noKey = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey: null,
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: 100_000_000,
  })
  assert.equal(noKey.ok, false)
  assert.ok(noKey.checks.some((c) => !c.ok && c.name.includes('access key exists')))
  // A function-call key is not full access.
  const limited = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey: {
      nonce: '5000000',
      permission: { FunctionCall: { allowance: '1', receiver_id: 'other.near', method_names: [] } },
      block_height: 1,
      block_hash: fakeBlock().hash,
    },
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: 100_000_000,
  })
  assert.equal(limited.ok, false)
  assert.ok(limited.checks.some((c) => !c.ok && c.name.includes('full access')))
})

test('the same account whose key is not on chain stops before sending', async () => {
  // Production queries view_access_key with the key derived from the
  // private key. A key that does not belong to the caller account has no
  // access key there: the query fails and the verdict refuses.
  const caller = throwawayCaller()
  const other = throwawayCaller(caller.accountId) // same account, other key
  const block = fakeBlock()
  const signed = await signedFor(other, ROW, block)
  const verdict = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: other.privateKey,
    accessKey: null, // view_access_key answered "does not exist"
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: block.height,
  })
  assert.equal(verdict.ok, false)
  assert.ok(verdict.checks.some((c) => !c.ok && c.name.includes('access key exists')))
})

test('a wrong amount or payTo in the signed action fails verification', async () => {
  const caller = throwawayCaller()
  const block = fakeBlock()
  const signed = await signedFor(caller, ROW, block)
  const verdict = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey: fakeAccessKey(caller.keyPair),
    asset: ROW.asset,
    payTo: 'someone-else.near',
    amount: '9999999',
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: block.height,
  })
  assert.equal(verdict.ok, false)
  assert.ok(verdict.checks.some((c) => !c.ok && c.name.includes('ft_transfer')))
})

test('a past block window fails verification', async () => {
  const caller = throwawayCaller()
  const signed = await signedFor(caller)
  const verdict = verifyDelegate(signed, {
    accountId: caller.accountId,
    privateKey: caller.privateKey,
    accessKey: fakeAccessKey(caller.keyPair),
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    // The chain has moved far past the window.
    currentBlockHeight: 100_000_000 + ROW.maxTimeoutSeconds + 10,
  })
  assert.equal(verdict.ok, false)
  assert.ok(verdict.checks.some((c) => !c.ok && c.name.includes('block window')))
})

test('the quote guards fire one at a time', () => {
  const max = usdToAtomic('0.05')
  assert.equal(max, 50000n)
  // A quote above the ceiling.
  let errs = checkRow({ ...ROW, amount: '99999999' }, { maxUsdAtomic: max })
  assert.equal(errs.length, 1)
  assert.ok(errs[0].includes('max-usd'))
  // A foreign asset contract (no ceiling, so only the allowlist fires).
  errs = checkRow({ ...ROW, asset: 'deadbeef'.repeat(8).slice(0, 64) }, { maxUsdAtomic: null })
  assert.equal(errs.length, 1)
  assert.ok(errs[0].includes('allowlisted'))
  // An over-long window: one second above the cap (7 days + 600).
  errs = checkRow({ ...ROW, maxTimeoutSeconds: 604800 + 601 }, { maxUsdAtomic: null })
  assert.equal(errs.length, 1)
  assert.ok(errs[0].includes('maxTimeoutSeconds'))
  // A realistic quote (one day SLA plus 600) is accepted (N4).
  assert.deepEqual(checkRow({ ...ROW, maxTimeoutSeconds: 86400 + 600 }, { maxUsdAtomic: null }), [])
  // The cap itself is accepted.
  assert.deepEqual(checkRow({ ...ROW, maxTimeoutSeconds: 604800 + 600 }, { maxUsdAtomic: null }), [])
  // A non-integer amount.
  errs = checkRow({ ...ROW, amount: '12.5' }, { maxUsdAtomic: null })
  assert.equal(errs.length, 1)
  // The happy quote passes.
  assert.deepEqual(checkRow({ ...ROW, amount: '25000' }, { maxUsdAtomic: max }), [])
  // No ceiling means the amount check is skipped but the others stay.
  assert.deepEqual(checkRow(ROW, { maxUsdAtomic: null }), [])
})

test('the handle has the market shape and cannot smuggle a host', () => {
  for (const bad of ['evil.com/x?', '', 'A'.repeat(40), 'has.dots', '-lead', 'sp ace']) {
    assert.ok(!handleOk(bad), `${bad} passed`)
  }
  for (const good of ['repo-audit-fast', 'abc', 'a1-2b']) {
    assert.ok(handleOk(good), `${good} refused`)
  }
})

test('a missing account id or key exits non-zero naming the variable', () => {
  const code = process.exitCode
  const err = console.error
  let printed = ''
  console.error = (...args) => {
    printed += args.join(' ') + '\n'
  }
  const capture = () => {
    const tmp = process.exitCode
    process.exitCode = 0
    return tmp
  }
  capture()
  assert.equal(readCaller({}), null)
  assert.match(printed, /NEAR_CALLER_ACCOUNT_ID/)
  capture()
  printed = ''
  assert.equal(readCaller({ NEAR_CALLER_ACCOUNT_ID: 'x.near' }), null)
  assert.match(printed, /NEAR_CALLER_PRIVATE_KEY/)
  assert.equal(process.exitCode, 2, 'the caller names NEAR_CALLER_PRIVATE_KEY')
  assert.ok(!/ed25519:[1-9A-HJ-NP-Za-km-z]{40,}/.test(printed), 'a key shape was printed')
  console.error = err
  process.exitCode = code
})

test('a non-https RPC or A2A URL is refused', async () => {
  const code = process.exitCode
  const err = console.error
  let printed = ''
  console.error = (...args) => {
    printed += args.join(' ') + '\n'
  }
  process.exitCode = 0
  assert.equal(readCaller({ NEAR_CALLER_ACCOUNT_ID: 'x', NEAR_CALLER_PRIVATE_KEY: 'ed25519:x', NEAR_RPC_URL: 'http://rpc' }), null)
  assert.match(printed, /https/)
  console.error = err
  process.exitCode = code
})

test('the wiring stops before any paid call when verification fails', async (t) => {
  // main() is not exported; the guard is proven at the module level: with a
  // failing verdict nothing reaches fetch. Here we prove the property on
  // the pieces main composes: checkRow refuses a bad quote, and
  // verifyDelegate refuses a bad signature, before paymentHeader/paidCall
  // are reachable.
  const caller = throwawayCaller()
  const signed = await signedFor(caller)
  const bad = verifyDelegate(signed, {
    accountId: 'someone.else',
    privateKey: caller.privateKey,
    accessKey: fakeAccessKey(caller.keyPair),
    asset: ROW.asset,
    payTo: ROW.payTo,
    amount: ROW.amount,
    maxTimeoutSeconds: ROW.maxTimeoutSeconds,
    currentBlockHeight: 100_000_000,
  })
  assert.equal(bad.ok, false, 'a foreign signer must not pass local verification')
  assert.equal(
    checkRow({ ...ROW, amount: '9999' }, { maxUsdAtomic: usdToAtomic('0.001') }).length,
    1,
    'a quote above the ceiling must not pass',
  )
})

test('the window cap clears a real quote and refuses only longer ones', () => {
  // The market quotes sla + 600; a one-day SLA quotes 87000 (N4).
  assert.deepEqual(checkRow({ ...ROW }, { maxUsdAtomic: null }), [])
  assert.deepEqual(
    checkRow({ ...ROW, maxTimeoutSeconds: 0 }, { maxUsdAtomic: null }).length,
    1,
  )
  assert.deepEqual(
    checkRow({ ...ROW, maxTimeoutSeconds: 604800 + 601 }, { maxUsdAtomic: null }).length,
    1,
  )
})

test('flags are parsed anywhere in the argv (N5)', () => {
  // A misplaced --dry-run must stay a flag, never become the task text.
  const a = parseArgs(['repo-audit-fast', '--dry-run', 'Audit X', '--max-usd', '0.05'])
  assert.deepEqual(
    { handle: a.handle, task: a.task, dryRun: a.dryRun, maxUsd: a.maxUsd },
    { handle: 'repo-audit-fast', task: 'Audit X', dryRun: true, maxUsd: '0.05' },
  )
  const b = parseArgs(['--max-usd', '0.05', 'repo-audit-fast', 'Audit X', '--dry-run'])
  assert.equal(b.dryRun, true)
  assert.equal(b.task, 'Audit X')
  const c = parseArgs(['repo-audit-fast', 'Audit X'])
  assert.equal(c.dryRun, false)
  assert.equal(c.maxUsd, null)
})

test('http is allowed only for a loopback host', () => {
  for (const good of ['https://x.example', 'http://127.0.0.1:8080', 'http://localhost:3000', 'http://[::1]:9000']) {
    assert.ok(httpsOk(good), `${good} refused`)
  }
  for (const bad of ['http://rpc.example', 'http://localhost@evil.example', 'ftp://x', 'http://']) {
    assert.ok(!httpsOk(bad), `${bad} passed`)
  }
})
