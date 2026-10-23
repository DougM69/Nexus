// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StreamView } from './StreamView'
import { HostedConnection, type BrowserClient } from './client'
import { ANSWER, LEASE, answerChecked, harness } from './stream-link.testkit'

// The stream page on a real connection: what its "Still there?" end leaves open to the relay room. The
// room bills for every second a socket keeps it awake, so a stream nobody answered must let it sleep.
vi.setConfig({ testTimeout: 30_000 })

class Socket {
  static OPEN = 1
  readyState = 1
  bufferedAmount = 0
  sent: string[] = []
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  closeReason: string | undefined
  constructor() { sockets.push(this) }
  send(value: string) { this.sent.push(value) }
  close(_code?: number, reason?: string) { this.readyState = 2; this.closeReason = reason }
  receive(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }) }
}
let sockets: Socket[] = []
const frames = new Map<HTMLVideoElement, (now: number, metadata: { rtpTimestamp?: number }) => void>()
beforeEach(() => {
  Object.defineProperty(HTMLVideoElement.prototype, 'requestVideoFrameCallback', { configurable: true,
    value(this: HTMLVideoElement, callback: (now: number, metadata: { rtpTimestamp?: number }) => void) { frames.set(this, callback); return 1 } })
  Object.defineProperty(HTMLVideoElement.prototype, 'cancelVideoFrameCallback', { configurable: true, value(this: HTMLVideoElement) { frames.delete(this) } })
})
afterEach(() => {
  cleanup(); frames.clear(); vi.useRealTimers(); vi.unstubAllGlobals(); sockets = []
  delete (document as { hidden?: unknown }).hidden
  delete (document as { visibilityState?: unknown }).visibilityState
  delete (HTMLVideoElement.prototype as { requestVideoFrameCallback?: unknown }).requestVideoFrameCallback
  delete (HTMLVideoElement.prototype as { cancelVideoFrameCallback?: unknown }).cancelVideoFrameCallback
  localStorage.clear()
})
function tab(hidden: boolean) {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => hidden })
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => hidden ? 'hidden' : 'visible' })
  act(() => { document.dispatchEvent(new Event('visibilitychange')) })
}

const IDLE_ENDED = 'Nobody answered “Still there?”, so the stream ended.'
const BOOT = crypto.randomUUID()
const station = (phase: 'available' | 'controlling', revision: number) => ({ stationBootId: BOOT, allowed: true, phase,
  leaseId: phase === 'controlling' ? LEASE : null, revision, commandWindowId: phase === 'controlling' ? crypto.randomUUID() : null,
  nextSequence: phase === 'controlling' ? 1 : null, leaseRemainingMs: phase === 'controlling' ? 5000 : null, actions: [],
  txArmed: false, transmitEpoch: phase === 'controlling' ? '000000000000002a' : null })
type Request = { type: string; requestId: string }
const requests = (socket: Socket): Request[] => socket.sent.map(value => JSON.parse(value) as { type: string; request?: Request })
  .filter(m => m.type === 'operationRequest').map(m => m.request!)
const sentTypes = (socket: Socket, from: number) => socket.sent.slice(from).map(value => {
  const m = JSON.parse(value) as { type: string; request?: Request }
  return m.type === 'operationRequest' ? `operation:${m.request!.type}` : m.type
})

/** The station at the far end of the room: it answers every operation request with its state, which
 *  follows what this browser asked for - acquire takes control, release gives it back (and retires the
 *  stop token). `holdRelease` keeps the answer to a release on its way. */
function shack() {
  let phase: 'available' | 'controlling' = 'available', revision = 1, holdRelease = false
  const answered = new Set<string>()
  const answer = () => {
    for (const socket of sockets) {
      if (socket.readyState !== 1) continue
      for (const request of requests(socket)) {
        // Stop's own acceptance is not this test's subject: it is left on its way.
        if (answered.has(request.requestId) || request.type === 'stopTransmit' || (holdRelease && request.type === 'release')) continue
        answered.add(request.requestId)
        if (request.type === 'acquire') phase = 'controlling'
        if (request.type === 'release') phase = 'available'
        socket.receive({ type: 'operationResponse', requestId: request.requestId, value: station(phase, ++revision) })
      }
    }
  }
  return { answer, holdRelease: (value: boolean) => { holdRelease = value } }
}

async function page() {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('WebSocket', Socket)
  const h = harness({ relay: () => Promise.reject(Error('no relay')) })
  const post = vi.fn(async (_path: string) => ({ ok: true }))
  const observationTicket = async () => ({ body: { ticket: 'a'.repeat(64), serverNow: 1000 }, startedAt: performance.now() })
  const remote = new HostedConnection({ post, observationTicket, operationVersion: 4 } as unknown as BrowserClient, crypto.randomUUID(), true, h.env)
  const far = shack()
  remote.start()
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  const opened = () => { const socket = sockets[sockets.length - 1]; socket.receive({ type: 'session', sessionId: crypto.randomUUID() }); return socket }
  const socket = opened()
  const utils = render(<StreamView connection={remote} station="Home" disconnect={() => {}} />)
  /** One second of the operator's page: the station answers, a frame arrives, the clocks move. */
  const second = async (frame = true) => {
    await act(async () => {
      far.answer()
      if (frame) frames.get(utils.container.querySelector('video')!)?.(0, { rtpTimestamp: 90000 + h.env.now() * 90 })
      h.tick(1000)
      await vi.advanceTimersByTimeAsync(1000)
      far.answer()
    })
  }
  const seconds = async (n: number, frame = true) => { for (let i = 0; i < n; i++) await second(frame) }
  await seconds(2)
  return { remote, socket, far, h, post, second, seconds, opened, utils }
}

/** Start the stream from the page, and bring it live. */
async function live(p: Awaited<ReturnType<typeof page>>) {
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await p.seconds(2)
  expect(sentTypes(p.socket, 0), 'the offer went').toContain('streamSignal')
  await act(async () => { p.socket.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await answerChecked() })
  act(() => {
    p.h.peer.ontrack?.({ track: 'track', streams: ['media'] })
    for (const label of ['control', 'ptt', 'audio']) p.h.peer.channel(label).open()
  })
  await p.seconds(1)
  expect(p.remote.stream.getSnapshot().phase).toBe('live')
}

it('IDLE: the stream nobody answered lets the relay room sleep - the socket closes and nothing is sent after it', async () => {
  const p = await page()
  await live(p)
  await p.seconds(16 * 60 + 2)
  expect(screen.getByText(IDLE_ENDED)).toBeTruthy()
  const from = p.socket.sent.length, posts = p.post.mock.calls.length
  await p.seconds(60, false)
  expect(p.post.mock.calls.slice(posts).map(c => String(c[0])), 'and no observer renewal').toEqual([])
  // The measurement: what this page sent the room in the minute after the end, by type.
  const sent = sentTypes(p.socket, from)
  expect({ open: p.socket.readyState === 1, reason: p.socket.closeReason ?? null, sent: sent.length },
    `a minute after the idle end the page sent ${JSON.stringify(sent.reduce<Record<string, number>>((a, k) => ({ ...a, [k]: (a[k] ?? 0) + 1 }), {}))}`)
    .toEqual({ open: false, reason: 'idle', sent: 0 })
  expect(sockets, 'and no new socket opened behind it').toHaveLength(1)
  p.remote.stop()
})

it('CONTROL: a stream the operator ended with End the stream keeps the room (they are there), and the same page hidden lets it go - the harness sees a close when one happens', async () => {
  const p = await page()
  await live(p)
  await p.seconds(60)
  fireEvent.click(screen.getByRole('button', { name: 'End the stream' }))
  await p.seconds(30, false)
  expect(p.socket.readyState, 'ended by the operator, who is looking').toBe(1)
  tab(true)
  await p.seconds(2, false)
  expect(p.socket.readyState).toBe(2)
  expect(p.socket.closeReason).toBe('backgrounded')
  p.remote.stop()
})

const STATUS_WORDS = ['Connecting to the station…', 'Waiting for the station…', "The station isn't online", 'Station data unavailable']
const statusShown = () => STATUS_WORDS.filter(words => document.body.textContent!.includes(words))

it('IDLE: the rested page says why and offers Start, never a dropout; being shown again keeps it asleep; Start wakes it and starts the stream', async () => {
  const p = await page()
  await live(p)
  await p.seconds(16 * 60 + 2)
  await p.seconds(30, false)
  expect(p.socket.closeReason).toBe('idle')
  expect(screen.getByText(IDLE_ENDED)).toBeTruthy()
  expect(statusShown(), 'asleep on purpose is not a lost station').toEqual([])
  expect(screen.getByRole('button', { name: 'Start the stream' })).toBeTruthy()
  // Away and back is not the operator asking: the room stays asleep.
  tab(true); await p.seconds(2, false); tab(false); await p.seconds(5, false)
  expect(sockets).toHaveLength(1)
  expect(statusShown()).toEqual([])
  // Start is: a new socket, and once the station's state is read, the stream is asked for again.
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(sockets).toHaveLength(2)
  expect(screen.queryByText(IDLE_ENDED)).toBeNull()
  const again = p.opened()
  await p.seconds(3)
  expect(sentTypes(again, 0)).toEqual(expect.arrayContaining(['operation:state', 'operation:acquire', 'streamSignal']))
  expect(again.readyState).toBe(1)
  p.remote.stop()
})

// ⚠️ THE STOP LINE. The room is let go only once this browser can no longer stop a transmission: closing
// the socket is what takes Stop away, so the idle end waits for the station to retire the token.
it('IDLE: never lets the room go while Stop TX can still reach the station - Stop works the whole time, and the room sleeps once the token is retired', async () => {
  const p = await page()
  await live(p)
  p.far.holdRelease(true)
  await p.seconds(16 * 60 + 2)
  expect(screen.getByText(IDLE_ENDED)).toBeTruthy()
  await p.seconds(30, false)
  expect(p.remote.operations.getSnapshot().stopAvailable).toBe(true)
  expect(p.socket.readyState, 'the room stays awake while Stop is live').toBe(1)
  const stop = screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
  expect(stop.disabled).toBe(false)
  const before = p.socket.sent.length
  fireEvent.click(stop)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(sentTypes(p.socket, before)).toContain('operation:stopTransmit')
  // The station's answer to the release retires the token, and only then does the room go to sleep.
  p.far.holdRelease(false)
  await p.seconds(2, false)
  expect(p.remote.operations.getSnapshot().stopAvailable).toBe(false)
  expect(p.socket.closeReason).toBe('idle')
  p.remote.stop()
})
