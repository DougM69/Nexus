// @vitest-environment jsdom
//
// ⭐ A JTTY CALL IS LOOKED UP ONLY ONCE THE TWO-DECODE RULE HAS CONFIRMED IT (operator ruling,
// 2026-10-12: lookups, spots and Needed only from calls decoded in two separate messages). JTTY's
// CRC-12 lets the odd false decode through, some of them well-formed calls; a callbook lookup of
// one would spend the operator's quota on a station that does not exist and show its name as if
// it had been heard. The engine lists the confirmed calls on each line (`confirmedCalls`), and
// that list is the only thing in the cockpit that can reach a lookup: a confirmed call is a
// button that fills the log strip's Call as a SURE fill, which is what the strip looks up.
//
// The log strip here is the REAL one, so the lookup is the real `qrzLookup` call the strip makes.
// The positive control is the confirmed call on the same screen, under the same wait.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, within } from '@testing-library/react'
import { JttyCockpit } from './JttyCockpit'
import type { AppSnapshot, JttyLine, JttyState } from '../types'
import * as api from '../api'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  // The log's questions go unanswered: nothing here needs the log, and null is no answer.
  mod.askLog = vi.fn(async () => {
    throw new Error('no log in this test')
  })
  mod.searchParks = vi.fn(async () => [])
  mod.getLicensedBandPlan = vi.fn(async () => [])
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./RfScopePane', () => ({ RfScopePane: () => null }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  hunt: null,
  b4MatchMode: false,
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const T0 = Date.UTC(2026, 9, 13, 22, 13, 20)
const line = (over: Partial<JttyLine>): JttyLine => ({
  id: 1,
  startUtcMs: T0,
  freqHz: 1500,
  dialHz: 14_090_000,
  snr: -12,
  text: '',
  line: '',
  state: 'complete',
  rx: true,
  confirmedCalls: [],
  ...over,
})

const STATE: JttyState = {
  armed: true,
  rxHz: 1500,
  ftolHz: 20,
  lowHz: 200,
  highHz: 3000,
  heard: [],
  notice: null,
  lines: [
    // Heard twice: confirmed.
    line({ id: 1, text: 'CQ W1AW FN31', line: '1500 -12  CQ W1AW FN31', confirmedCalls: ['W1AW'] }),
    // Heard once: may be a false decode (a fabricated but well-formed call).
    line({ id: 2, startUtcMs: T0 + 3_000, text: 'TU DA2PKN CQ', line: '1520 -19  TU DA2PKN CQ', confirmedCalls: [] }),
  ],
}

const qrz = () => vi.mocked(api.qrzLookup)
const logCall = () => document.querySelector('.le-call') as HTMLInputElement
const allPane = () => document.querySelector('[data-pane="all"] .jtty-lines') as HTMLElement

async function mount() {
  vi.mocked(api.getJttyState).mockResolvedValue(STATE)
  vi.mocked(api.jttyAutoArm).mockResolvedValue(STATE)
  render(
    <JttyCockpit snap={snap} active onSetTxEnabled={() => {}} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  await waitFor(() => expect(allPane().querySelectorAll('.jtty-line')).toHaveLength(2))
  expect(logCall(), 'control: the real log strip is on screen').not.toBeNull()
}

/** Longer than the strip's own settle-then-look-up wait (700 ms), with room to spare. */
const pastTheLookupWait = () => new Promise((r) => setTimeout(r, 1_500))

beforeEach(() => {
  localStorage.clear()
  qrz().mockResolvedValue({ call: 'W1AW', name: 'ARRL HQ', grid: 'FN31', country: 'United States' } as never)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('a JTTY call is looked up only once two separate messages confirmed it', () => {
  it('a call heard once offers nothing to click, and pressing its text looks nothing up', async () => {
    await mount()
    const pane = within(allPane())
    expect(pane.queryByRole('button', { name: 'DA2PKN' }), 'not a button').toBeNull()
    fireEvent.click(pane.getByText('DA2PKN', { exact: false }))
    fireEvent.doubleClick(pane.getByText('DA2PKN', { exact: false }))
    await act(pastTheLookupWait)
    expect(qrz()).not.toHaveBeenCalled()
    expect(logCall().value, 'the log line was not filled').toBe('')
  })

  it('POSITIVE CONTROL: the confirmed call, clicked, fills the log line and is looked up once', async () => {
    await mount()
    fireEvent.click(within(allPane()).getByRole('button', { name: 'W1AW' }))
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    await act(pastTheLookupWait)
    expect(qrz()).toHaveBeenCalledTimes(1)
    expect(qrz()).toHaveBeenCalledWith('W1AW')
  })
})
