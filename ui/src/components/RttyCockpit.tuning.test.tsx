// @vitest-environment jsdom
//
// WHERE THE TUNING SCOPE STANDS IN THE RTTY COCKPIT, rendered rather than read off a sheet:
//   · ticked with `stream`, it is the first thing in the RX row, LEFT of the decoded text, inside
//     the stream pane and below its head (so it covers and displaces none of the head's controls);
//   · with `stream` hidden it stands in its own content-height frame, `tuning`;
//   · hidden itself, the transcript is untouched — the same element, so its scroll pin survives;
//   · its only controls are the two nudges and its ✕. It hosts no stop and no sender (THE STOP LINE).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, within } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, RttyState } from '../types'
import type { PanelLayoutApi, RttyPanelId } from '../features/panelState'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

const ARMED = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: 'CQ TEST',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => ARMED),
  rttyAutoArm: vi.fn(async () => ARMED),
  getRttyScope: vi.fn(async () => ({ live: true, seq: 1, peak: 0.4, xy: [], offsetHz: null })),
  getLicensedBandPlan: vi.fn(async () => []),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.083, band: '20m', catOk: true, sideband: 'LSB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

function panels(removed: RttyPanelId[]): PanelLayoutApi<RttyPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id: RttyPanelId) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  } as unknown as PanelLayoutApi<RttyPanelId>
}

async function show(removed: RttyPanelId[]) {
  const r = render(<RttyCockpit snap={snap} panels={panels(removed)} onSetTxEnabled={() => {}} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

afterEach(cleanup)

describe('the RTTY tuning scope in its cockpit', () => {
  it('stands left of the decoded text, inside the stream pane and below its head', async () => {
    const { container } = await show([])
    const stream = container.querySelector('[data-pane="stream"]') as HTMLElement
    const row = stream.querySelector('.rtty-rx-row') as HTMLElement
    expect(row, 'no RX row in the stream pane').not.toBeNull()
    const kids = [...row.children]
    expect(kids.map((k) => k.className)).toEqual(['rtty-tuning', 'cw-decode-text'])
    // Below the head, never in it: the head's controls (Arm, Auto, Clear) keep their places.
    expect(stream.querySelector('.cw-decode-head .rtty-tuning')).toBeNull()
    expect(container.querySelector('[data-pane="tuning"]'), 'no second frame while stream is shown').toBeNull()
  })

  it('hosts the two nudges and its ✕, and nothing that starts or stops a transmission', async () => {
    await show([])
    const group = screen.getByRole('group', { name: 'Tuning' })
    const names = within(group)
      .getAllByRole('button')
      .map((b) => b.getAttribute('aria-label'))
    expect(names).toEqual(['Hide Tuning', 'Tune down 5 Hz', 'Tune up 5 Hz'])
  })

  it('with the text hidden, stands in its own content-height frame', async () => {
    const { container } = await show(['stream'])
    const frame = container.querySelector('[data-pane="tuning"]') as HTMLElement
    expect(frame, 'the scope vanished with a pane the operator did not hide').not.toBeNull()
    expect(frame.getAttribute('data-fit')).toBe('content')
    expect(within(frame).getAllByRole('button', { name: 'Hide Tuning' })).toHaveLength(1)
    expect(container.querySelector('.rtty-rx-row')).toBeNull()
  })

  it('floors the text pane at the scope (the guarantee), and only while the scope is shown', async () => {
    const { container } = await show([])
    const shell = container.querySelector('main.rtty-cockpit') as HTMLElement
    const stream = container.querySelector('[data-pane="stream"]') as HTMLElement
    expect(shell.classList.contains('rtty-tuning-on')).toBe(true)
    expect(stream.style.minHeight).toContain('var(--rtty-stream-floor, 0px)')
    // The log strip does not read the knob: the text pane is what gives way.
    const log = [...container.querySelectorAll('.pane-frame')].find((f) => f.getAttribute('aria-label') === 'Log') as HTMLElement
    expect(log.style.minHeight).not.toContain('--rtty-stream-floor')
    cleanup()
    const hidden = await show(['tuning'])
    expect(hidden.container.querySelector('main.rtty-cockpit')!.classList.contains('rtty-tuning-on')).toBe(false)
  })

  it('hidden itself, leaves the transcript where it was', async () => {
    const { container } = await show(['tuning'])
    expect(container.querySelector('.rtty-tuning')).toBeNull()
    const row = container.querySelector('[data-pane="stream"] .rtty-rx-row') as HTMLElement
    expect([...row.children].map((k) => k.className)).toEqual(['cw-decode-text'])
  })
})
