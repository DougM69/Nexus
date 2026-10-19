// @vitest-environment jsdom
//
// THE AI CW DECODER'S PROVISIONAL LETTERS. The station sends the confirmed copy (`text`) and the
// unconfirmed tail the latest pass heard (`pending`). The pane draws both at once, the tail dimmed;
// a later pass confirms the tail (it turns normal) or replaces it. The tail is for the eye only:
// the screen reader's log, like every reader of the copy as data, gets the confirmed text.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, CwDecodeResult } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: ReactNode }) => <header className="cockpit-header">{children}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

let copy: Partial<CwDecodeResult> = {}
beforeEach(() => {
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    wpm: 25, sent: [], keyerError: null, candidates: [], rst: null, name: null, state: 'listening',
    headline: '', prompt: '', recommended: null, workedCall: null, ...copy,
  }))
  api.get('previewCw').mockImplementation(async (text: string) => text)
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const snap = {
  mycall: 'KD9TAW',
  aiCw: { enabled: true, status: '', text: '' },
  radio: { dialMhz: 14.025, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false, txEnabled: true, txAllowed: true, cwWpm: 25, cwKeyer: 'cat' },
} as unknown as AppSnapshot

/** Let the decode poll (every 200 ms) read the station's current copy. */
const nextPoll = async () => {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 260))
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
const pane = () => document.querySelector('.cw-decode-text') as HTMLElement
const dim = () => pane().querySelector('.cw-decode-unconfirmed')
const spoken = () => (document.querySelector('.cw-decode [role="log"]') as HTMLElement).textContent

describe("the AI decoder's provisional letters", () => {
  it('draw at once and dimmed after the confirmed copy, and turn normal once confirmed', async () => {
    copy = { text: 'CQ TEST ', pending: 'K9A' }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    expect(pane().textContent, 'both at once: no reveal delay').toBe('CQ TEST K9A')
    expect(dim()?.textContent, 'only the tail is dimmed').toBe('K9A')
    expect(spoken(), 'the screen reader hears confirmed copy only').toBe('CQ TEST ')

    copy = { text: 'CQ TEST K9ABC ', pending: '' }
    await nextPoll()
    expect(pane().textContent).toBe('CQ TEST K9ABC ')
    expect(dim(), 'confirmed letters are drawn like the rest').toBeNull()
    expect(spoken()).toBe('CQ TEST K9ABC ')
  })

  it('a later pass may replace them; the confirmed copy is untouched', async () => {
    copy = { text: 'CQ TEST ', pending: 'K9A' }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    copy = { text: 'CQ TEST ', pending: 'K8AB' }
    await nextPoll()
    expect(pane().textContent).toBe('CQ TEST K8AB')
    expect(dim()?.textContent).toBe('K8AB')
    expect(pane().firstChild?.textContent, 'the confirmed copy is its own, undimmed text').toBe('CQ TEST ')
  })

  it("an older station's copy, with no tail at all, draws as before", async () => {
    copy = { text: 'CQ TEST K9ABC ' }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    expect(pane().textContent).toBe('CQ TEST K9ABC ')
    expect(dim()).toBeNull()
  })
})
