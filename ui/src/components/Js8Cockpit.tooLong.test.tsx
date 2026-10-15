// @vitest-environment jsdom
//
// A HELD MESSAGE TOO LONG TO HAND OVER AT THE CURRENT SPEED: the station does not offer it, and its
// own Inbox row says why and at which speed it goes (`Js8State.inboxTooLong`). Read-only: the note
// keys nothing and asks nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import { Js8InboxTooLongNote } from './Js8InboxTooLong'
import type { AppSnapshot, Js8InboxEntry, Js8State } from '../types'
import { t } from '../i18n'

// THE BUDGET: the house's 15 s, as the other cockpit suites carry it.
vi.setConfig({ testTimeout: 15_000 })

const row = (over: Partial<Js8InboxEntry> = {}): Js8InboxEntry => ({
  id: 1,
  from: 'W1AW',
  to: 'K1ABC',
  text: 'QRV QRV QRV',
  path: [],
  state: 'store',
  atMs: 1_757_000_000_000,
  freqHz: 1750,
  snrDb: -5,
  ...over,
})

const base = (): Js8State => ({
  speed: 'slow',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
  notArmed: { autoreply: 'txOff', relay: 'txOff', hbAck: null, hb: null, cq: null },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [row(), row({ id: 2, text: 'T2' })],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: base() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const s = () => state.current
  return {
    ...auto,
    getJs8State: vi.fn(async () => s()),
    js8Enter: vi.fn(async () => s()),
    js8Composer: vi.fn(async () => null),
    js8LocatorRefusal: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit() {
  const r = render(<Js8Cockpit snap={snap} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** Each Inbox row's text, in order (the notes of what a cap removed are not rows of the list). */
const rows = () =>
  Array.from(document.querySelectorAll('.js8-inbox > .js8-inbox-row')).map((r) => r.textContent ?? '')

describe('the Inbox says why a held message is not offered at this speed', () => {
  it('on that message\'s row: the frames, the cap, and the speed it goes at', async () => {
    state.current = { ...base(), inboxTooLong: [{ id: 1, frames: 23, max: 19, fitsAt: 'normal' }] }
    await renderCockpit()
    const note = t('js8.inbox.tooLong.fits', { frames: 23, max: 19, speed: 'Normal' })
    expect(note).toBe('Not offered at this speed: handing it over takes 23 frames, one transmission holds 19. Offered at Normal.')
    const [first, second] = rows()
    expect(first.includes(note), first).toBe(true)
    expect(second.includes(note), 'not on the row of a message that fits').toBe(false)
  })

  it('at no speed: says so', () => {
    render(<Js8InboxTooLongNote id={4} tooLong={[{ id: 4, frames: 120, max: 99, fitsAt: null }]} />)
    expect(document.body.textContent).toBe(t('js8.inbox.tooLong.never', { frames: 120 }))
    expect(document.body.textContent).toBe(
      'Not offered: handing it over takes 120 frames, more than one transmission holds at any speed.',
    )
  })

  it('says nothing while every held message fits, or for another row', async () => {
    await renderCockpit()
    expect(rows().some((r) => r.includes('Not offered'))).toBe(false)
    render(<Js8InboxTooLongNote id={2} tooLong={[{ id: 1, frames: 23, max: 19, fitsAt: 'normal' }]} />)
    expect(document.querySelectorAll('.js8-path').length).toBe(0)
  })
})
