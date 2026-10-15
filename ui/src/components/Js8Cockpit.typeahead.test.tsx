// @vitest-environment jsdom
//
// JS8CALL'S TYPEAHEAD IN THE COMPOSE BOX (mainwindow.cpp:3590-3628; the user guide: "you can start
// transmitting and continue typing your message as each frame is transmitted. Checksummed messages
// like MSG or Relays cannot use typeahead."). After a Send the box keeps the message as its frames
// render while it goes out; what has gone moves out of the box into the line beside it; typing
// hands the engine what is still to go (`js8Typeahead`), never a new message; a MSG or relay is
// read-only; the box empties once the message has gone. The engine frames the text and keys; this
// file drives the box against a mocked engine.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import { sentTail } from './js8Typeahead'
import * as api from '../api'
import type { AppSnapshot, Js8State, Js8Typeahead } from '../types'
import { t } from '../i18n'

// THE BUDGET: the house's 15 s (a loaded full suite runs cases many times slower than one core).
vi.setConfig({ testTimeout: 15_000 })

const base = (): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: true,
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
  armed: { autoreply: true, relay: true, hbAck: false, hb: false, cq: false },
  notArmed: { autoreply: null, relay: null, hbAck: null, hb: null, cq: null },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})

/** What the engine says of the box's message (`null` while none goes out), and the reply it
 * offers the box (AUTO off). */
const engine = vi.hoisted(() => ({
  typeahead: null as Js8Typeahead | null,
  offer: null as { id: number; text: string } | null,
}))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getJs8State: vi.fn(async () => base()),
    js8Enter: vi.fn(async () => base()),
    js8Send: vi.fn(async () => base()),
    js8SendCommand: vi.fn(async () => base()),
    js8Composer: vi.fn(async () => engine.offer),
    js8LocatorRefusal: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
    js8TypeaheadState: vi.fn(async () => engine.typeahead),
    js8Typeahead: vi.fn(async () => engine.typeahead),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const js8Send = api.js8Send as ReturnType<typeof vi.fn>
const js8Typeahead = api.js8Typeahead as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  engine.typeahead = null
  engine.offer = null
  js8Send.mockClear()
  js8Typeahead.mockClear()
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
const q = <T extends Element>(sel: string) => document.querySelector(sel) as T
const box = () => q<HTMLInputElement>('.js8-compose')
const type = (value: string) => fireEvent.change(box(), { target: { value } })

/** Send `text` as the operator does, with the engine taking it as `taken`. */
async function send(text: string, taken: Js8Typeahead | null) {
  type(text)
  engine.typeahead = taken
  await act(async () => {
    fireEvent.keyDown(box(), { key: 'Enter' })
  })
  expect(js8Send).toHaveBeenCalledWith(null, text)
}

describe('typeahead in the JS8 compose box', () => {
  it('keeps the message in the box as its frames render while it goes out, and moves what has gone beside it', async () => {
    await renderCockpit()
    await send('hello world', { sent: '', unsent: 'KD9TAW: HELLO WORLD', locked: false })
    await waitFor(() => expect(box().value).toBe('KD9TAW: HELLO WORLD'))
    expect(q('.js8-typeahead-sent').textContent, 'nothing gone yet').toBe('')
    expect(q('.js8-estimate').textContent, 'no estimate for a message already going out').toBe('')
    // The first frame goes: the cockpit's poll hears it.
    engine.typeahead = { sent: 'KD9TAW: HELLO ', unsent: 'WORLD', locked: false }
    await waitFor(() => expect(box().value).toBe('WORLD'), { timeout: 2000 })
    expect(q('.js8-typeahead-sent').textContent).toBe('KD9TAW: HELLO ')
    expect(q('.js8-typeahead-sent').getAttribute('title')).toBe(t('js8.dock.typeahead.sent'))
  })

  it('hands the engine what is still to go as the operator types, and never sends a new message', async () => {
    await renderCockpit()
    await send('hello world', { sent: 'KD9TAW: HELLO ', unsent: 'WORLD', locked: false })
    await waitFor(() => expect(box().value).toBe('WORLD'))
    await act(async () => {
      type('WORLD AND MORE')
    })
    expect(js8Typeahead).toHaveBeenLastCalledWith('KD9TAW: HELLO '.length, 'WORLD AND MORE')
    await act(async () => {
      fireEvent.keyDown(box(), { key: 'Enter' })
    })
    expect(js8Send, 'Enter while it goes out queues nothing new').toHaveBeenCalledTimes(1)
    expect(js8Typeahead).toHaveBeenLastCalledWith('KD9TAW: HELLO '.length, 'WORLD AND MORE')
    expect(box().value).toBe('WORLD AND MORE')
  })

  it('keeps what was typed after the part that went, and takes only what went off the front', async () => {
    await renderCockpit()
    await send('hello world', { sent: '', unsent: 'KD9TAW: HELLO WORLD', locked: false })
    await waitFor(() => expect(box().value).toBe('KD9TAW: HELLO WORLD'))
    await act(async () => {
      type('KD9TAW: HELLO WORLD, 73')
    })
    engine.typeahead = { sent: 'KD9TAW: HELLO ', unsent: 'WORLD, 73', locked: false }
    await waitFor(() => expect(box().value).toBe('WORLD, 73'), { timeout: 2000 })
  })

  it('a MSG or relay message is read-only while it goes, as in JS8Call', async () => {
    await renderCockpit()
    await send('w1aw msg hi', { sent: '', unsent: 'KD9TAW: W1AW MSG HI', locked: true })
    await waitFor(() => expect(box().readOnly).toBe(true))
    expect(box().title).toBe(t('js8.dock.typeahead.locked'))
    await act(async () => {
      type('KD9TAW: W1AW MSG HI THERE')
    })
    expect(js8Typeahead, 'nothing handed to the engine').not.toHaveBeenCalled()
  })

  it('empties the box once the message has gone, as JS8Call does, and the next Send queues a message', async () => {
    await renderCockpit()
    await send('hello', { sent: '', unsent: 'KD9TAW: HELLO', locked: false })
    await waitFor(() => expect(box().value).toBe('KD9TAW: HELLO'))
    engine.typeahead = null
    await waitFor(() => expect(box().value).toBe(''), { timeout: 2000 })
    expect(q('.js8-typeahead-sent')).toBeNull()
    expect(box().readOnly).toBe(false)
    await send('again', null)
    expect(js8Send).toHaveBeenCalledTimes(2)
    expect(box().value, 'nothing going out with typeahead: the box clears as before').toBe('')
  })

  it('takes no reply the station offers while its message is still going out, and takes it after', async () => {
    await renderCockpit()
    await send('hello', { sent: '', unsent: 'KD9TAW: HELLO', locked: false })
    await waitFor(() => expect(box().value).toBe('KD9TAW: HELLO'))
    // The last frame has gone and the box is empty; the station offers a reply (AUTO off).
    engine.typeahead = { sent: 'KD9TAW: HELLO', unsent: '', locked: false }
    engine.offer = { id: 1, text: 'W1AW SNR -07' }
    await waitFor(() => expect(box().value).toBe(''), { timeout: 2000 })
    await act(async () => {
      await new Promise((r) => setTimeout(r, 1200))
    })
    expect(box().value, 'not while the box still holds the message going out').toBe('')
    engine.typeahead = null
    await waitFor(() => expect(box().value).toBe('W1AW SNR -07'), { timeout: 3000 })
  })

  it('shows only the end of a long sent line', () => {
    expect(sentTail('KD9TAW: HELLO ')).toBe('KD9TAW: HELLO ')
    expect(sentTail('KD9TAW: A LONG MESSAGE THAT HAS GONE OUT')).toBe('…ESSAGE THAT HAS GONE OUT')
  })
})
