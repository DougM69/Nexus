// @vitest-environment jsdom
//
// THE JTTY SCREEN, AS THE ENGINE FEEDS IT. Receive only: the cockpit shows the engine's lines in
// WSJT-X's two panes (All Decodes: every message where it started; QSO Frequency: the ones first
// heard within F Tol of Rx), WSJT-X's two display options (lower case, the start time in front),
// the engine's notice, and sends the receive controls the engine owns (auto-arm on entry, Arm,
// Erase, Rx and F Tol, the waterfall's range). The dock's compose bar is drawn disabled.
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
const wf = vi.hoisted(() => ({ props: null as null | { onTune?: (hz: number) => void; onViewChange?: (lo: number, hi: number) => void; rxOffsetHz: number } }))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { onTune?: (hz: number) => void; onViewChange?: (lo: number, hi: number) => void; rxOffsetHz: number }) => {
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
    act(() => wf.props!.onTune!(1312.6))
    expect(m('jttySetRx')).toHaveBeenLastCalledWith(1313, 20)
  })

  it('hands the waterfall’s visible range to the decoder, and again when it moves', async () => {
    await mount(state())
    await waitFor(() => expect(wf.props).not.toBeNull())
    act(() => wf.props!.onViewChange!(200, 3000))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(200, 3000)
    act(() => wf.props!.onViewChange!(1012.5, 1612.5))
    expect(m('jttySetBand')).toHaveBeenLastCalledWith(1012, 1613)
  })

  it('the dock’s compose bar is drawn disabled, and nothing in the cockpit can send', async () => {
    await mount(state())
    expect((screen.getByRole('textbox', { name: t('jtty.compose.aria') }) as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: t('jtty.compose.send.label') }) as HTMLButtonElement).disabled).toBe(true)
    for (const k of Object.keys(api) as (keyof typeof api)[]) {
      if (/send|^psk|^rtty|^js8|setPtt|setTxEnabled|startCq|callStation/i.test(String(k)) && typeof api[k] === 'function') {
        expect(m(k), `${String(k)} was called`).not.toHaveBeenCalled()
      }
    }
  })
})
