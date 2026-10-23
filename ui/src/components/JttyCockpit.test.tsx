// @vitest-environment jsdom
//
// THE JTTY SCREEN, AS THE ENGINE FEEDS IT. The cockpit shows the engine's lines in WSJT-X's two
// panes (All Decodes: every message received, where it started; QSO Frequency: the ones first
// heard within F Tol of Rx, and what this station sent), WSJT-X's two display options (lower case,
// the start time in front), the engine's notice, and sends the receive controls the engine owns
// (auto-arm on entry, Arm, Erase, Rx and F Tol, the waterfall's range). The dock is WSJT-X's
// transmitter, SWITCHED OFF in every build: drawn disabled with the reason while the engine says
// it is off, and wired to the engine's send commands (tested here with a state saying it is on).
//
// The log strip is a stub here that shows what the cockpit hands it; the lookup it then makes is
// JttyCockpit.lookup.test.tsx's, with the real strip.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { JttyCockpit } from './JttyCockpit'
import { StationControlContext } from '../stationAccess'
import type { AppSnapshot, JttyLine, JttyState } from '../types'
import * as api from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
// The header's own furniture (readout, digit tuning, the ⊞ menu) is not this file's: it draws the
// slots the cockpit hands it, which is where Rx and F Tol live.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator: ReactNode; bandControl: ReactNode; children?: ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.bandControl}
      {p.children}
    </header>
  ),
}))
/** The waterfall's props as last rendered: the test plays the strip's part (a click, a zoom). */
type WfTarget = 'tx' | 'rx' | 'both'
const wf = vi.hoisted(() => ({
  props: null as null | {
    onTune?: (hz: number, target: 'tx' | 'rx' | 'both') => void
    onViewChange?: (lo: number, hi: number) => void
    rxOffsetHz: number
    txOffsetHz: number
  },
}))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { onTune?: (hz: number, target: WfTarget) => void; onViewChange?: (lo: number, hi: number) => void; rxOffsetHz: number; txOffsetHz: number }) => {
    wf.props = p
    return <div className="waterfall-wrap" />
  },
}))
vi.mock('./RfScopePane', () => ({ RfScopePane: () => null }))
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { cwLive?: { call: string | null; confirmed: boolean } | null; mode: string }) => (
    <div data-testid="log-stub" data-mode={p.mode}>
      {p.cwLive?.call ?? ''}
    </div>
  ),
}))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const T0 = Date.UTC(2026, 9, 13, 22, 13, 20, 900) // 22:13:20.900 UTC

function line(over: Partial<JttyLine>): JttyLine {
  return {
    id: 1,
    startUtcMs: T0,
    freqHz: 1500,
    dialHz: 14_090_000,
    snr: -12,
    text: 'CQ K1ABC CQ',
    line: '1500 -12  CQ K1ABC CQ',
    state: 'complete',
    rx: true,
    confirmedCalls: [],
    ...over,
  }
}

function state(over: Partial<JttyState> = {}): JttyState {
  return { armed: true, rxHz: 1500, ftolHz: 20, lowHz: 200, highHz: 3000, lines: [], heard: [], notice: null, ...over }
}

const m = <K extends keyof typeof api>(k: K) => vi.mocked(api[k] as (...a: unknown[]) => Promise<unknown>)

async function mount(s: JttyState, active = true) {
  m('getJttyState').mockResolvedValue(s)
  m('jttyAutoArm').mockResolvedValue(s)
  m('jttyArm').mockResolvedValue(s)
  m('jttySetRx').mockResolvedValue(s)
  m('jttySetBand').mockResolvedValue(s)
  m('jttyClear').mockResolvedValue(state({ lines: [] }))
  m('getLicensedBandPlan').mockResolvedValue([])
  const r = render(
    <StationControlContext.Provider value={true}>
      <JttyCockpit snap={snap} active={active} onSetTxEnabled={() => {}} />
    </StationControlContext.Provider>,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

const pane = (id: 'all' | 'rx') => document.querySelector(`[data-pane="${id}"] .jtty-lines`) as HTMLElement
const paneLines = (id: 'all' | 'rx') => [...pane(id).querySelectorAll('.jtty-line')].map((el) => el.textContent)

beforeEach(() => {
  localStorage.clear()
  wf.props = null
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the JTTY cockpit shows what the engine decoded', () => {
  it('prints each line exactly as the engine (WSJT-X) wrote it, the QSO pane only the Rx ones, and the notice', async () => {
    const lines = [
      line({ id: 7, line: '1500 -12  CQ K1ABC CQ', rx: true }),
      // A message wrapped by WSJT-X: the engine's line carries the newline and the indent.
      line({ id: 8, startUtcMs: T0 + 4_000, freqHz: 1300, line: '1300  -3  THIS IS A LONG MESSAGE THAT WSJT X WRAPS\n  AT FORTY', rx: false }),
    ]
    await mount(state({ lines, notice: 'JTTY receive audio was lost; decoding restarted.' }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ', '1300  -3  THIS IS A LONG MESSAGE THAT WSJT X WRAPS\n  AT FORTY']))
    expect(paneLines('rx')).toEqual(['1500 -12  CQ K1ABC CQ'])
    const notice = document.querySelector('.jtty-notice')
    expect(notice?.getAttribute('role')).toBe('status')
    expect(notice?.textContent).toContain('JTTY receive audio was lost; decoding restarted.')
  })

  it('All decodes is in the order messages started, QSO frequency in the order they arrived', async () => {
    // The engine lists lines as they arrive; a retro search finds message 9 after 10, though it
    // started first.
    const lines = [
      line({ id: 10, startUtcMs: T0 + 6_000, line: '1500 -10  B', rx: true }),
      line({ id: 9, startUtcMs: T0 + 2_000, line: '1510 -11  A', rx: true }),
    ]
    await mount(state({ lines }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1510 -11  A', '1500 -10  B']))
    expect(paneLines('rx')).toEqual(['1500 -10  B', '1510 -11  A'])
  })

  it('WSJT-X’s two options: lower case, and the UTC start in front (hhmmss, truncated); both kept', async () => {
    await mount(state({ lines: [line({ id: 1, line: '1500 -12  CQ K1ABC CQ' })] }))
    await waitFor(() => expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ']))
    fireEvent.click(screen.getByRole('button', { name: t('jtty.lowerCase.label') }))
    expect(paneLines('all')).toEqual(['1500 -12  cq k1abc cq'])
    fireEvent.click(screen.getByRole('button', { name: t('jtty.includeTime.label') }))
    // 22:13:20.900 is 221320: Qt's hhmmss drops the milliseconds, it never rounds them.
    expect(paneLines('all')).toEqual(['221320 1500 -12  cq k1abc cq'])
    cleanup()
    await mount(state({ lines: [line({ id: 1, line: '1500 -12  CQ K1ABC CQ' })] }))
    await waitFor(() => expect(paneLines('all')).toEqual(['221320 1500 -12  cq k1abc cq']))
    expect(screen.getByRole('button', { name: t('jtty.lowerCase.label') }).getAttribute('aria-pressed')).toBe('true')
  })

  it('a click on a CONFIRMED call fills the log strip; a call heard once is plain text', async () => {
    const lines = [
      line({ id: 1, text: 'CQ W1AW CQ', line: '1500 -12  CQ W1AW CQ', confirmedCalls: ['W1AW'] }),
      line({ id: 2, startUtcMs: T0 + 3_000, text: 'CQ K1ABC CQ', line: '1520 -14  CQ K1ABC CQ', confirmedCalls: [] }),
    ]
    await mount(state({ lines }))
    await waitFor(() => expect(paneLines('all')).toHaveLength(2))
    const all = within(pane('all'))
    expect(all.queryByRole('button', { name: 'K1ABC' }), 'heard once: not a button').toBeNull()
    fireEvent.click(all.getByText('K1ABC', { exact: false }))
    expect(screen.getByTestId('log-stub').textContent).toBe('')
    fireEvent.click(all.getByRole('button', { name: 'W1AW' }))
    expect(screen.getByTestId('log-stub').textContent).toBe('W1AW')
    expect(screen.getByTestId('log-stub').dataset.mode).toBe('JTTY')
  })
})

describe('the JTTY cockpit drives the engine’s receive controls, and nothing that transmits', () => {
  it('auto-arms once on entry, never while hidden, and again on the next entry', async () => {
    const r = await mount(state(), false)
    expect(m('jttyAutoArm')).not.toHaveBeenCalled()
    r.rerender(
      <StationControlContext.Provider value={true}>
        <JttyCockpit snap={snap} active={true} onSetTxEnabled={() => {}} />
      </StationControlContext.Provider>,
    )
    await waitFor(() => expect(m('jttyAutoArm')).toHaveBeenCalledTimes(1))
    r.rerender(
      <StationControlContext.Provider value={true}>
        <JttyCockpit snap={snap} active={true} onSetTxEnabled={() => {}} />
      </StationControlContext.Provider>,
    )
    expect(m('jttyAutoArm')).toHaveBeenCalledTimes(1)
  })

  it('Arm/Stop and Erase go to the engine', async () => {
    await mount(state({ armed: true, lines: [line({})] }))
    fireEvent.click(screen.getByRole('button', { name: t('jtty.arm.on.label') }))
    expect(m('jttyArm')).toHaveBeenCalledWith(false)
    fireEvent.click(screen.getByRole('button', { name: t('jtty.erase.label') }))
    expect(m('jttyClear')).toHaveBeenCalledTimes(1)
  })

  it('Rx and F Tol: typed Rx on Enter, F Tol from WSJT-X’s list, a waterfall click sets Rx', async () => {
    await mount(state({ rxHz: 1500, ftolHz: 20 }))
    const rx = screen.getByRole('spinbutton', { name: t('jtty.rx.aria') }) as HTMLInputElement
    expect(rx.value).toBe('1500')
    fireEvent.focus(rx)
    fireEvent.change(rx, { target: { value: '1234' } })
    fireEvent.keyDown(rx, { key: 'Enter' })
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1234, 20)
    const ftol = screen.getByRole('combobox', { name: t('jtty.ftol.aria') }) as HTMLSelectElement
    expect([...ftol.options].map((o) => Number(o.value))).toEqual([2, 5, 10, 20, 50, 100, 150, 200, 250, 300, 350, 400, 450, 500])
    fireEvent.change(ftol, { target: { value: '50' } })
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1500, 50)
    await waitFor(() => expect(wf.props).not.toBeNull())
    act(() => wf.props!.onTune!(1312.6, 'rx'))
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1313, 20)
    expect(m('jttySetTx')).not.toHaveBeenCalled()
  })

  it('Tx: typed on Enter, Shift+click (or right-click) on the waterfall sets it, Ctrl+click both', async () => {
    await mount(state({ tx: txState({ txHz: 1500 }) }))
    m('jttySetTx').mockResolvedValue(state({ tx: txState({ txHz: 1700 }) }))
    const tx = screen.getByRole('spinbutton', { name: t('jtty.tx.aria') }) as HTMLInputElement
    expect(tx.value).toBe('1500')
    fireEvent.focus(tx)
    fireEvent.change(tx, { target: { value: '1700' } })
    fireEvent.keyDown(tx, { key: 'Enter' })
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1700)
    await waitFor(() => expect(wf.props).not.toBeNull())
    expect(wf.props!.txOffsetHz).toBe(1700)
    act(() => wf.props!.onTune!(1622.4, 'tx'))
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1622)
    expect(m('jttySetRx')).not.toHaveBeenCalled()
    act(() => wf.props!.onTune!(1405, 'both'))
    expect(m('jttySetTx')).toHaveBeenLastCalledWith(1405)
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1405, 20)
  })

  it('hands the waterfall’s visible range to the decoder, and again when it moves', async () => {
    await mount(state())
    await waitFor(() => expect(wf.props).not.toBeNull())
    act(() => wf.props!.onViewChange!(200, 3000))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(200, 3000)
    act(() => wf.props!.onViewChange!(1012.5, 1612.5))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(1012, 1613)
  })

  it('the whole transmitter is on screen, even for a state with no transmitter in it; the engine says why it refuses a send, and the text stays to send again', async () => {
    for (const s of [state(), state({ tx: txState({ refusal: 'TX is off — enable TX first' }) })]) {
      await mount(s)
      for (const key of ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8']) {
        expect(screen.getByRole('button', { name: new RegExp(`^${key}`) }), key).toBeTruthy()
      }
      expect(liveKey(), 'Live').toBeTruthy()
      expect(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), 'the Call box').toBeTruthy()
      expect(screen.getByRole('spinbutton', { name: t('jtty.serial.aria') }), 'the serial').toBeTruthy()
      expect(compose().disabled).toBe(false)
      expect(compose().placeholder).toBe(t('jtty.compose.placeholder'))
      cleanup()
    }
    await mount(state({ tx: txState({ refusal: 'TX is off — enable TX first' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('TX is off — enable TX first')
    m('jttySend').mockRejectedValue('TX is off — enable TX first')
    fireEvent.change(compose(), { target: { value: 'CQ TEST' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledWith('CQ TEST', '')
    expect(vi.mocked(pushToast)).toHaveBeenLastCalledWith('TX is off — enable TX first', 'error')
    expect(compose().value, 'the refused text stays in the box').toBe('CQ TEST')
  })
})

/** A transmitter state, as the engine reports it. */
function txState(over: Partial<NonNullable<JttyState['tx']>> = {}): NonNullable<JttyState['tx']> {
  return { refusal: null, txHz: 1500, serial: 1, sending: false, live: false, left: 0, pending: '', notice: null, ...over }
}

const sent = (consumed: number, text: string, s: JttyState) => ({ consumed, sent: text, state: s })
/** The dock's Live key (a sender), by its name: `TX` and Live, or Sign off while a session runs. */
const liveKey = () =>
  screen.getByRole('button', { name: new RegExp(`^TX\\s*(${t('jtty.live.on.label')}|${t('jtty.live.off.label')})$`) }) as HTMLButtonElement
const compose = () => screen.getByRole('textbox', { name: t('jtty.compose.aria') }) as HTMLInputElement
const flush = () =>
  act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })

describe('the JTTY dock, wired to the engine (a state saying the transmitter is on)', () => {
  it('Send sends the typed text with the Call box, takes it out of the box and shows it sent', async () => {
    const on = state({ tx: txState() })
    await mount(on)
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'w1aw' } })
    m('jttySend').mockResolvedValue(sent(14, 'CQ CQ DE KD9TAW', state({ tx: txState({ left: 1, sending: true, pending: 'CQ CQ DE KD9TAW' }) })))
    fireEvent.change(compose(), { target: { value: 'cq cq de %M' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledWith('cq cq de %M', 'W1AW')
    expect(compose().value).toBe('')
    expect(document.querySelector('.jtty-committed')?.textContent).toBe('CQ CQ DE KD9TAW')
    expect(screen.getByRole('button', { name: t('jtty.compose.send.left', { n: 1 }) })).toBeTruthy()
  })

  it('sends go one after another in the order made: an F-key pressed while a send is in flight is neither lost nor first', async () => {
    await mount(state({ tx: txState() }))
    let release: (v: unknown) => void = () => {}
    m('jttySend').mockImplementation(() => new Promise((ok) => (release = ok)))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW 599 001', state({ tx: txState() })))
    fireEvent.change(compose(), { target: { value: 'CQ TEST' } })
    fireEvent.keyDown(compose(), { key: 'Enter' })
    await flush()
    expect(m('jttySend')).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey'), 'the F-key overtook the send in flight').not.toHaveBeenCalled()
    release(sent(7, 'CQ TEST', state({ tx: txState({ left: 1 }) })))
    await flush()
    expect(m('jttyFkey'), 'the F-key was lost').toHaveBeenCalledTimes(1)
    expect(m('jttySend').mock.invocationCallOrder[0]).toBeLessThan(m('jttyFkey').mock.invocationCallOrder[0])
  })

  it('F1–F8, clicked or pressed, send WSJT-X’s templates with the Call box; Alt/Ctrl/Cmd never', async () => {
    await mount(state({ tx: txState() }))
    m('jttyFkey').mockResolvedValue(sent(0, 'W1AW 599 001', state({ tx: txState() })))
    fireEvent.change(screen.getByRole('textbox', { name: t('jtty.hisCall.aria') }), { target: { value: 'W1AW' } })
    fireEvent.click(screen.getByRole('button', { name: /^F2/ }))
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(2, 'W1AW')
    fireEvent.keyDown(window, { key: 'F6' })
    await flush()
    expect(m('jttyFkey')).toHaveBeenLastCalledWith(6, 'W1AW')
    m('jttyFkey').mockClear()
    fireEvent.keyDown(window, { key: 'F1', altKey: true })
    fireEvent.keyDown(window, { key: 'F1', ctrlKey: true })
    fireEvent.keyDown(window, { key: 'F1', repeat: true })
    await flush()
    expect(m('jttyFkey')).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: /^F3/ }).textContent).toBe('F3%H TU CQ %M CQ')
  })

  it('live entry: started with what is typed, typing commits as the engine decides, a pause flushes, Sign off ends it', async () => {
    vi.useFakeTimers()
    try {
      await mount(state({ tx: txState() }))
      const liveOn = state({ tx: txState({ live: true, sending: true }) })
      m('jttyLiveStart').mockResolvedValue(sent(6, 'HELLO ', liveOn))
      // The engine's poll says what it says from now on: a live session, on the air.
      m('getJttyState').mockResolvedValue(liveOn)
      fireEvent.change(compose(), { target: { value: 'HELLO ' } })
      fireEvent.click(liveKey())
      await flush()
      expect(m('jttyLiveStart')).toHaveBeenCalledWith('HELLO ', '')
      expect(compose().value).toBe('')
      m('jttyLiveCommit').mockResolvedValue(sent(0, '', liveOn))
      fireEvent.change(compose(), { target: { value: 'HOW ARE' } })
      await flush()
      expect(m('jttyLiveCommit')).toHaveBeenLastCalledWith('HOW ARE', '', false)
      m('jttyLiveCommit').mockResolvedValue(sent(7, 'HOW ARE', liveOn))
      await act(async () => {
        vi.advanceTimersByTime(1_500)
      })
      await flush()
      expect(m('jttyLiveCommit')).toHaveBeenLastCalledWith('HOW ARE', '', true)
      expect(compose().value).toBe('')
      expect(document.querySelector('.jtty-committed')?.textContent).toBe('HELLO HOW ARE')
      m('jttySignOff').mockResolvedValue(sent(0, 'DE KD9TAW K', state({ tx: txState({ left: 1, sending: true }) })))
      fireEvent.click(liveKey())
      await flush()
      expect(m('jttySignOff')).toHaveBeenCalledWith('', '')
    } finally {
      vi.useRealTimers()
    }
  })

  it('Esc/Stop is live exactly while JTTY is on the air, queued or live, and calls jtty_stop', async () => {
    const stopKey = () => screen.getByRole('button', { name: /^esc\s*stop$/i }) as HTMLButtonElement
    await mount(state({ tx: txState() }))
    expect(stopKey().disabled, 'nothing going').toBe(true)
    cleanup()
    for (const going of [{ sending: true }, { left: 2 }, { live: true }]) {
      await mount(state({ tx: txState(going) }))
      m('jttyStop').mockResolvedValue(state({ tx: txState() }))
      expect(stopKey().disabled, JSON.stringify(going)).toBe(false)
      fireEvent.click(stopKey())
      expect(m('jttyStop')).toHaveBeenCalledTimes(1)
      cleanup()
      vi.clearAllMocks()
    }
  })

  it('the serial is typed and sent on Enter; the notice or the refusal shows under the dock', async () => {
    await mount(state({ tx: txState({ serial: 7, refusal: 'TX is off — enable TX first' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('TX is off — enable TX first')
    const serial = screen.getByRole('spinbutton', { name: t('jtty.serial.aria') }) as HTMLInputElement
    expect(serial.value).toBe('7')
    fireEvent.focus(serial)
    fireEvent.change(serial, { target: { value: '12' } })
    fireEvent.keyDown(serial, { key: 'Enter' })
    expect(m('jttySetSerial')).toHaveBeenLastCalledWith(12)
    cleanup()
    await mount(state({ tx: txState({ notice: 'JTTY transmission complete.' }) }))
    expect(document.querySelector('.cockpit-txdock [role="status"]')?.textContent).toBe('JTTY transmission complete.')
  })

  it('what this station sent shows in the QSO frequency pane only', async () => {
    const lines = [
      line({ id: 3, line: '1500 -12  CQ K1ABC CQ', rx: true }),
      line({ id: -1, startUtcMs: T0 + 5_000, line: '1500      W1AW TU', rx: true, tx: true }),
    ]
    await mount(state({ lines, tx: txState() }))
    await waitFor(() => expect(paneLines('rx')).toEqual(['1500 -12  CQ K1ABC CQ', '1500      W1AW TU']))
    expect(paneLines('all')).toEqual(['1500 -12  CQ K1ABC CQ'])
  })
})
