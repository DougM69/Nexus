// @vitest-environment jsdom
//
// CLEAR DX ENDS THE CONTACT, AS WSJT-X'S DOES. F4 and the Clear button beside the DX Call box are
// WSJT-X's `clearDX` (mainwindow.cpp:10075), which ends the contact with the station in the box and
// resets the QSO start time; an edit of the DX Call box resets it too (`on_dxCallEntry_textChanged`,
// :10563). Both reach the engine's `clear_dx_call`, which drops the contact so nothing more goes to
// that station. The Tx panel is drawn at two sites, Classic's and the arranged columns' (where Roster
// shows it), so both are swept, and F4 in Roster, which draws no Tx panel unarranged. CONTROLS: the after-log clear (the stock option) only tidies the screen,
// and an edit with no contact running sends nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import { OPERATE_PANELS, usePanelLayout } from '../features/panelState'
import type { OperateLayout } from '../features/panelState'
import { clearDxCall } from '../api'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module, as the other FT suites do: a hand-kept list omits whatever is
  // added later and the cockpit throws on mount.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    getDeclination: vi.fn(async () => 0),
    resolveEntity: vi.fn(async () => 'United States'),
  }
})
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  stations: [],
  recentDecodes: [],
  conversations: [],
  highlights: [],
  harqRescues: 0,
  clearTick: 0,
  rxFreqClearTick: 0,
  qso: null,
  link: { tier: 'FT8', periodSecs: 15 },
  radio: {
    dialMhz: 14.074,
    band: '20m',
    sideband: 'USB',
    slot: 0,
    source: 'native',
    sourceLabel: 'Native',
    nextSlotMs: 5000,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    txEven: true,
    txCycleAuto: true,
    txEnabled: false,
    txAllowed: true,
    transmitting: false,
    tuning: false,
    atu: true,
    qsoRecording: false,
    catOk: true,
    splitTxMhz: null,
    decodeDepth: 2,
  },
} as unknown as AppSnapshot

/** The contact the sequencer is working: 7P8RU, our roger queued. */
const QSO = { dxcall: '7P8RU', dxgrid: 'KG30', state: 'AwaitRr73', txNow: '7P8RU KR4FQG R-09', cqRunning: false }

beforeEach(() => {
  localStorage.clear()
  vi.mocked(clearDxCall).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const noop = () => {}

function Live({ layoutMode, qso, dxClearTick }: { layoutMode: OperateLayout; qso: typeof QSO | null; dxClearTick: number }) {
  const panels = usePanelLayout(OPERATE_PANELS)
  return (
    <OperateCockpit
      snap={{ ...snap, qso } as AppSnapshot}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panels}
      dxClearTick={dxClearTick}
      active
    />
  )
}

type Site = 'classic' | 'roster' | 'roster-tx'

/** The cockpit at `site` with the Tx panel on screen, and a way to send the next props. */
function mount(site: Site, qso: typeof QSO | null) {
  const layoutMode: OperateLayout = site === 'classic' ? 'classic' : 'roster'
  let props = { qso, dxClearTick: 0 }
  const view = render(<Live layoutMode={layoutMode} {...props} />)
  if (site === 'roster-tx') {
    // Roster draws the Tx panel once ⊞ Panels ticks it on: in the rail, as an arranged column.
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    fireEvent.click(screen.getByLabelText('Tx Messages'))
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    expect(document.querySelector('.cockpit-lower [data-pane="txmsgs"]'), 'the Tx panel stands in the rail').toBeTruthy()
  }
  // Roster unarranged draws no Tx panel; F4 is its clear.
  expect(!!document.querySelector('.tx-panel'), 'premise: the Tx panel is on screen where the layout draws it').toBe(site !== 'roster')
  return (next: Partial<typeof props>) => {
    props = { ...props, ...next }
    view.rerender(<Live layoutMode={layoutMode} {...props} />)
  }
}

const dxBox = () => document.querySelector('.tx-panel .txp-field input') as HTMLInputElement

describe('Clear DX ends the contact, as WSJT-X’s does', () => {
  for (const site of ['classic', 'roster', 'roster-tx'] as const) {
    it(`${site}: F4 ends the contact`, () => {
      mount(site, QSO)
      if (site !== 'roster') expect(dxBox().value, 'premise: the box shows the station worked').toBe('7P8RU')
      fireEvent.keyDown(window, { key: 'F4' })
      expect(clearDxCall).toHaveBeenCalledTimes(1)
      if (site !== 'roster') expect(dxBox().value).toBe('')
    })
  }
  for (const site of ['classic', 'roster-tx'] as const) {

    it(`${site}: the Clear button ends the contact`, () => {
      mount(site, QSO)
      fireEvent.click(document.querySelector('.tx-panel .txp-clear')!)
      expect(clearDxCall).toHaveBeenCalledTimes(1)
      expect(dxBox().value).toBe('')
    })

    it(`${site}: editing the DX Call box away from the station ends the contact`, () => {
      mount(site, QSO)
      fireEvent.change(dxBox(), { target: { value: '7P8R' } })
      expect(clearDxCall).toHaveBeenCalledTimes(1)
      expect(dxBox().value).toBe('7P8R')
    })

    it(`${site}: CONTROL — an edit with no contact running sends nothing`, () => {
      mount(site, null)
      fireEvent.change(dxBox(), { target: { value: 'W1AW' } })
      expect(clearDxCall).not.toHaveBeenCalled()
      expect(dxBox().value).toBe('W1AW')
    })

    it(`${site}: CONTROL — the after-log clear empties the box and ends nothing`, () => {
      const send = mount(site, QSO)
      send({ dxClearTick: 1 })
      expect(dxBox().value).toBe('')
      expect(clearDxCall).not.toHaveBeenCalled()
    })
  }
})
