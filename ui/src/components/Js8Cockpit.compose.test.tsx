// @vitest-environment jsdom
//
// THE JS8 COMPOSE BOX AS JS8CALL 3.0.3 HAS IT — by value, at the engine seam. What each one-click
// button (CQ, REPLY, SNR, INFO, STATUS, a saved message, a station row's ask) asks the engine to
// send when "send at once" is on, JS8Call's "Immediately transmit…" default; what it puts in the
// box instead when it is off, and that it then sends nothing; the 13 macros replaced as JS8Call
// replaces them (pruned on a button, kept on Send); "Restore previous message"; "Save current
// message"; and the pills and the word-suggestion underline drawn in the box. Nothing here can
// key: every send is an engine call through the mocked api, and the engine re-checks every gate.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import type { AppSnapshot, Js8State, Settings } from '../types'
import { t } from '../i18n'
import { JS8_COMPOSE_DEFAULTS, type Js8ComposeConfig } from '../js8Compose'

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
  idleMinutes: 12,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [
    { atMs: Date.now() - 30_000, speed: 'normal', freqHz: 1500, snrDb: -3, dtS: -0.12, from: 'W1AW', text: 'W1AW: KD9TAW SNR?', directedToMe: true, mine: false, complete: true, lowConf: false },
  ],
  stations: [{ call: 'W1AW', grid: 'FN31', snrDb: -3, freqHz: 1500, speed: 'normal', lastMs: Date.now(), lastHb: false, lastCq: false, storedMsgs: 0 }],
  inbox: [],
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
    js8Send: vi.fn(async () => s()),
    js8SendCommand: vi.fn(async () => s()),
    js8CallCq: vi.fn(async () => s()),
    js8CqRepeat: vi.fn(async () => s()),
    js8Arm: vi.fn(async () => s()),
    js8Composer: vi.fn(async () => null),
    js8LocatorRefusal: vi.fn(async () => null),
    js8WordCheck: vi.fn(async (words: string[]) => words.map((w) => w === 'HELLOX')),
    js8WordSuggestions: vi.fn(async () => ({ words: ['HELLO', 'HELLOS'], found: true })),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
// The settings patch seam, run against a backend holding JS8Call's one saved message.
const patch = vi.hoisted(() => ({
  written: [] as unknown[],
  patchSettings: vi.fn(async (_edit: (cur: Settings) => Partial<Settings>) => ({})),
}))
vi.mock('../settings/patch', () => ({ patchSettings: patch.patchSettings }))
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', () => ({
  pushToast: toast.pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const js8Send = api.js8Send as ReturnType<typeof vi.fn>
const js8SendCommand = api.js8SendCommand as ReturnType<typeof vi.fn>
const js8CallCq = api.js8CallCq as ReturnType<typeof vi.fn>
const js8CqRepeat = api.js8CqRepeat as ReturnType<typeof vi.fn>
const js8WordCheck = api.js8WordCheck as ReturnType<typeof vi.fn>
const js8WordSuggestions = api.js8WordSuggestions as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52hw',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  for (const f of [js8Send, js8SendCommand, js8CallCq, js8CqRepeat, js8WordCheck, js8WordSuggestions]) f.mockClear()
  patch.written = []
  patch.patchSettings.mockReset()
  patch.patchSettings.mockImplementation(async (edit: (cur: Settings) => Partial<Settings>) => {
    patch.written.push(edit({ js8SavedMessages: ['TNX 73 GL'] } as Settings))
    return {}
  })
  toast.pushToast.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(compose: Partial<Js8ComposeConfig> = {}, extra: Partial<Parameters<typeof Js8Cockpit>[0]> = {}) {
  const r = render(<Js8Cockpit snap={snap} compose={{ ...JS8_COMPOSE_DEFAULTS, ...compose }} {...extra} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}
const q = <T extends Element>(sel: string) => document.querySelector(sel) as T
const box = () => q<HTMLInputElement>('.js8-compose')
const toBox = () => q<HTMLInputElement>('.js8-to')
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const click = async (el: Element) => {
  await act(async () => {
    fireEvent.click(el)
  })
}
/** Open a Radix menu by its trigger and pick an item by its text. */
const pick = async (trigger: Element, item: string) => {
  await act(async () => {
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: 'mouse' })
  })
  await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: item }))
}

describe('the CQ button sends the CQ message in Settings, or puts it in the box', () => {
  it('sends at once through the station, clears the To box as JS8Call clears its selected call, and says what it sends', async () => {
    await renderCockpit({ cqText: 'CQ DX <MYGRID4>' })
    type(toBox(), 'W1AW')
    type(box(), 'TNX')
    const cq = q<HTMLButtonElement>('.js8-cq')
    expect(cq.title).toBe(t('js8.dock.cq.send', { text: 'CQ DX EN52' }))
    await click(cq)
    expect(js8CallCq).toHaveBeenCalledTimes(1)
    expect(js8CallCq).toHaveBeenCalledWith()
    expect(toBox().value).toBe('')
    expect(box().value, 'what was typed stays').toBe('TNX')
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('with "send at once" off, puts the CQ message in the box, its macros replaced, and sends nothing', async () => {
    await renderCockpit({ cqText: 'CQ DX <MYGRID4> DE <MYCALL> <CALL>', sendAtOnce: false })
    type(toBox(), 'W1AW')
    const cq = q<HTMLButtonElement>('.js8-cq')
    // `<CALL>` is pruned: JS8Call's CQ clears the selected call before it builds the values.
    expect(cq.title).toBe(t('js8.dock.cq.fill', { text: 'CQ DX EN52 DE KD9TAW ' }))
    await click(cq)
    expect(box().value).toBe('CQ DX EN52 DE KD9TAW ')
    expect(toBox().value).toBe('')
    expect(js8CallCq).not.toHaveBeenCalled()
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('an empty CQ message is CQ CQ CQ and the square', async () => {
    await renderCockpit({ cqText: '', sendAtOnce: false })
    await click(q('.js8-cq'))
    expect(box().value).toBe('CQ CQ CQ EN52')
  })

  it('turning the CQ repeat on clears the To box too, as JS8Call clears its selected call', async () => {
    state.current = { ...base(), cqIntervalMin: 5 }
    await renderCockpit()
    type(toBox(), 'W1AW')
    await click(q('.js8-cq'))
    expect(js8CqRepeat).toHaveBeenCalledWith(true)
    expect(toBox().value).toBe('')
  })
})

describe('REPLY, SNR, INFO and STATUS', () => {
  it('REPLY needs a call in the To box, and sends the reply message to it, leaving the box alone', async () => {
    await renderCockpit()
    const reply = q<HTMLButtonElement>('.js8-reply')
    expect(reply.disabled).toBe(true)
    type(toBox(), 'w1aw')
    type(box(), 'TNX')
    expect(reply.disabled).toBe(false)
    await click(reply)
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'HW CPY?')
    expect(box().value).toBe('TNX')
  })

  it('REPLY replaces every macro it can value and prunes the rest', async () => {
    await renderCockpit({ replyText: '<CALL> DE <MYCALL> <SNR> <TDELTA><FOO>' })
    type(toBox(), 'W1AW')
    await click(q('.js8-reply'))
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'W1AW DE KD9TAW -03 -120 MS')
  })

  it('with "send at once" off, REPLY puts the reply at the caret and clears the command, sending nothing', async () => {
    await renderCockpit({ sendAtOnce: false })
    type(toBox(), 'W1AW')
    fireEvent.change(q<HTMLSelectElement>('.js8-cmd-select'), { target: { value: '9' } })
    type(box(), 'TNX')
    await click(q('.js8-reply'))
    expect(box().value).toBe('TNX HW CPY?')
    expect(q<HTMLSelectElement>('.js8-cmd-select').value).toBe('')
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('SNR sends the report heard from the To box call, and is off for a call not heard', async () => {
    await renderCockpit()
    const snr = q<HTMLButtonElement>('.js8-snr-send')
    type(toBox(), 'K1ABC')
    expect(snr.disabled).toBe(true)
    type(toBox(), 'W1AW')
    expect(snr.disabled).toBe(false)
    await click(snr)
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'SNR -03')
  })

  it('INFO is off until INFO is set; it goes to the To box call, or to everyone', async () => {
    const r = await renderCockpit()
    expect(q<HTMLButtonElement>('.js8-info').disabled).toBe(true)
    r.unmount()
    await renderCockpit({ info: 'QTH <MYGRID4> <FOO>RIG' })
    await click(q('.js8-info'))
    expect(js8Send).toHaveBeenLastCalledWith(null, 'INFO QTH EN52 RIG')
    type(toBox(), 'W1AW')
    await click(q('.js8-info'))
    expect(js8Send).toHaveBeenLastCalledWith('W1AW', 'INFO QTH EN52 RIG')
  })

  it('STATUS sends JS8Call\'s own when none is set: the idle time and the application', async () => {
    await renderCockpit()
    await click(q('.js8-status'))
    expect(js8Send).toHaveBeenCalledWith(null, 'STATUS IDLE 12M VERSION NEXUS')
  })
})

describe('the Saved menu', () => {
  it('shows each saved message with its macros filled in, and sends the one picked', async () => {
    await renderCockpit({ savedMessages: ['TNX 73 GL', 'DE <MYCALL> <FOO>'] })
    type(toBox(), 'W1AW')
    await act(async () => {
      fireEvent.pointerDown(q('.js8-saved'), { button: 0, ctrlKey: false, pointerType: 'mouse' })
    })
    const items = within(screen.getByRole('menu')).getAllByRole('menuitem').map((m) => m.textContent)
    expect(items).toEqual([
      t('js8.dock.saved.restore'),
      'TNX 73 GL',
      // Shown as JS8Call's menu shows it: replaced, with an unknown word left as typed.
      'DE KD9TAW <FOO>',
      t('js8.dock.saved.keep'),
    ])
    await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'DE KD9TAW <FOO>' }))
    // Sent pruned, as JS8Call's saved-message action replaces it.
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'DE KD9TAW ')
  })

  it('with "send at once" off, a saved message goes in at the caret and keeps the command, as a MSG\'s text', async () => {
    await renderCockpit({ sendAtOnce: false })
    type(toBox(), 'W1AW')
    fireEvent.change(q<HTMLSelectElement>('.js8-cmd-select'), { target: { value: '9' } })
    type(box(), 'QRV')
    await pick(q('.js8-saved'), 'TNX 73 GL')
    expect(box().value).toBe('QRV TNX 73 GL')
    expect(q<HTMLSelectElement>('.js8-cmd-select').value).toBe('9')
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('restores the last message sent, To box and all, in place of what is in the box', async () => {
    await renderCockpit()
    await act(async () => {
      fireEvent.pointerDown(q('.js8-saved'), { button: 0, ctrlKey: false, pointerType: 'mouse' })
    })
    expect(within(screen.getByRole('menu')).getByRole('menuitem', { name: t('js8.dock.saved.restore') }).getAttribute('data-disabled')).not.toBeNull()
    await act(async () => {
      fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    })
    type(toBox(), 'W1AW')
    type(box(), 'GM <MYCALL> <FOO>')
    await click(q('.js8-send'))
    // Send replaces the macros WITHOUT pruning, as JS8Call's createMessage does.
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'GM KD9TAW <FOO>')
    expect(box().value).toBe('')
    type(toBox(), 'K1ABC')
    type(box(), 'SOMETHING ELSE')
    await pick(q('.js8-saved'), t('js8.dock.saved.restore'))
    expect(toBox().value).toBe('W1AW')
    expect(box().value).toBe('GM KD9TAW <FOO>')
  })

  it('saves the box\'s words to the saved messages through the settings patch, and tells App', async () => {
    const onSaved = vi.fn()
    await renderCockpit({}, { onSavedMessages: onSaved })
    type(box(), 'qrv <MYGRID4>')
    await pick(q('.js8-saved'), t('js8.dock.saved.keep'))
    expect(patch.patchSettings).toHaveBeenCalledTimes(1)
    // Added to the list the backend holds, as typed (upper-cased, macros kept for each use).
    expect(patch.written).toEqual([{ js8SavedMessages: ['TNX 73 GL', 'QRV <MYGRID4>'] }])
    await act(async () => {
      await Promise.resolve()
    })
    expect(onSaved).toHaveBeenCalledWith(['TNX 73 GL', 'QRV <MYGRID4>'])
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('opens Settings at JS8 to edit them', async () => {
    const onOpenSettings = vi.fn()
    await renderCockpit({}, { onOpenSettings })
    await pick(q('.js8-saved'), t('js8.dock.saved.edit'))
    expect(onOpenSettings).toHaveBeenCalledWith('js8')
  })
})

describe("a station row's asks send at once, or fill the box", () => {
  it('with "send at once" off, SNR? goes in the box in place of what is there, to that station', async () => {
    await renderCockpit({ sendAtOnce: false })
    type(box(), 'HELLO')
    await click(screen.getAllByRole('button', { name: 'SNR?' })[0])
    expect(js8SendCommand).not.toHaveBeenCalled()
    expect(toBox().value).toBe('W1AW')
    expect(box().value).toBe('SNR?')
  })

  it('with it on, the ask is sent at once as it always was', async () => {
    await renderCockpit()
    await click(screen.getAllByRole('button', { name: 'SNR?' })[0])
    expect(js8SendCommand).toHaveBeenCalledWith('W1AW', 0, '')
  })
})

describe('the pills and the word underline in the box', () => {
  it('draws the recipient and the command as pills, and nothing with pills off', async () => {
    const r = await renderCockpit()
    type(box(), 'W1AW SNR?')
    const pills = [...document.querySelectorAll('.js8-pill')].map((p) => [p.className, p.textContent])
    expect(pills).toEqual([
      ['js8-pill recipient first last', 'W1AW'],
      ['js8-pill command first last', 'SNR?'],
    ])
    expect(q('.js8-compose-backdrop').getAttribute('aria-hidden')).toBe('true')
    r.unmount()
    await renderCockpit({ pills: false, wordSuggestions: false })
    type(box(), 'W1AW SNR?')
    expect(q('.js8-compose-backdrop')).toBeNull()
  })

  it('underlines a word the dictionary sends in pieces, and a right-click offers cheaper words', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      await renderCockpit({ pills: false })
      type(box(), 'HELLOX THERE')
      await act(async () => {
        vi.advanceTimersByTime(200)
        await Promise.resolve()
        await Promise.resolve()
      })
      expect(js8WordCheck).toHaveBeenCalledWith(['HELLOX', 'THERE'])
      expect([...document.querySelectorAll('.js8-word-flag')].map((f) => f.textContent)).toEqual(['HELLOX'])
      await act(async () => {
        fireEvent.contextMenu(box(), { clientX: 0, clientY: 0 })
        await Promise.resolve()
      })
      expect(js8WordSuggestions).toHaveBeenCalledWith('HELLOX')
      await act(async () => {
        await Promise.resolve()
      })
      await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'HELLO' }))
      expect(box().value).toBe('HELLO THERE')
    } finally {
      vi.useRealTimers()
    }
  })

  it('asks the station nothing with word suggestions off', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      await renderCockpit({ wordSuggestions: false })
      type(box(), 'HELLOX THERE')
      await act(async () => {
        vi.advanceTimersByTime(500)
        await Promise.resolve()
      })
      expect(js8WordCheck).not.toHaveBeenCalled()
      expect(q('.js8-word-flag')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })
})
