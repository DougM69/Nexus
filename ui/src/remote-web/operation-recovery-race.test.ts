import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import type { OperationState } from './operation-protocol'

afterEach(() => vi.useRealTimers())

function fixture() {
  vi.useFakeTimers()
  let now = 1000, unlock!: () => void
  const gate = new Promise<void>(resolve => { unlock = resolve })
  const exclusive = async <T,>(run: () => T | Promise<T>): Promise<T> => { await gate; return run() }
  const operationId = crypto.randomUUID(), sent: { request: { type: string; requestId: string; operationId?: string } }[] = []
  let saved: string | null = operationId
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now,
    { read: () => saved, write: value => { saved = value }, exclusive }, 3)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: ['log.manual'], txArmed: false }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  client.open(); reply(state)
  const resolve = () => client.resolve()
  return { client, sent, operationId, unlock, reply, resolve, state,
    advance: async () => { now += 1000; await vi.advanceTimersByTimeAsync(1000) },
    outcome: { operationId, outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline' } }
}

it('keeps a background poll from overtaking the explicit result check while its browser lock opens', async () => {
  const h = fixture(), checked = h.resolve()
  void checked.catch(() => {})
  try {
    await h.advance()
    expect(h.sent.map(w => w.request.type)).toEqual(['state'])
    expect(h.client.getSnapshot().busy).toBe(true)
    await expect(h.resolve()).rejects.toThrow('remoteBusy')
    h.unlock(); await Promise.resolve(); await Promise.resolve()
    expect(h.sent.map(w => w.request.type)).toEqual(['state', 'result'])
    expect(h.sent[1]!.request.operationId).toBe(h.operationId)
    h.reply(h.outcome)
    expect(await checked).toMatchObject({ outcome: 'applied', operationId: h.operationId })
    expect(h.client.getSnapshot().busy).toBe(false)
  } finally { h.client.disconnected(); h.unlock(); await checked.catch(() => {}) }
})

it('does not send a delayed result gesture through a reopened connection', async () => {
  const h = fixture(), checked = h.resolve()
  void checked.catch(() => {})
  try {
    h.client.disconnected(); await h.advance(); h.client.open(); h.reply(h.state)
    h.unlock(); await Promise.resolve(); await Promise.resolve()
    expect(h.sent.map(w => w.request.type)).toEqual(['state', 'state'])
    await expect(checked).rejects.toThrow('stationUnavailable')
    expect(h.client.getSnapshot().busy).toBe(false)
  } finally { h.client.disconnected(); h.unlock(); await checked.catch(() => {}) }
})
