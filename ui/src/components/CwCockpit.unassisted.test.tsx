// @vitest-environment jsdom
//
// AN UNASSISTED CONTEST ENTRY: NO CW DECODING, AND THE DECODE PANE SAYS WHY. CQ WW's rules count a
// CW decoder as QSO-finding assistance, so with an unassisted entry declared the station decodes
// nothing (`Settings::cw_decoder_active`): its read carries no copy, no speed, no calls, no report
// or name, and says `unassisted`. The pane shows one line saying why in place of the copy, where a
// screen reader reads it too, and the Copilot pane shows the same line where it promised decoded
// calls. An assisted station, and one outside a contest, read as before.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, screen } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, CwDecodeResult } from '../types'
import { EN } from '../i18n'

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

/** What the station's next read says, over an empty assisted read. */
let read: Partial<CwDecodeResult> & { unassisted?: boolean } = {}
beforeEach(() => {
  read = {}
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    text: '', pending: '', wpm: 0, sent: [], keyerError: null, candidates: [], rst: null, name: null,
    state: 'listening', headline: '', prompt: '', recommended: null, workedCall: null, ...read,
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

/** Let the decode poll (every 200 ms) read the station. */
const nextPoll = async () => {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 260))
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
const decodePane = () => document.querySelector('[data-pane="decode"]') as HTMLElement
const copilotPane = () => document.querySelector('[data-pane="copilot"]') as HTMLElement
const hisCall = () => screen.getByRole('textbox', { name: EN['cw.hisCall.label'] }) as HTMLInputElement

describe('an unassisted contest entry on the CW screen', () => {
  it('the Decode pane says why in place of the copy, where a screen reader reads it', async () => {
    read = { unassisted: true }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    const pane = decodePane()
    expect(pane.textContent, 'not "listening": nothing is decoded').not.toContain(EN['cw.decode.listening'])
    const line = pane.querySelector('.cw-decode-off') as HTMLElement | null
    expect(line?.textContent).toBe(EN['cw.decode.unassisted'])
    expect(line?.closest('[aria-hidden="true"]'), 'read by a screen reader').toBeNull()
    expect(pane.querySelector('.cw-decode-text'), 'no transcript').toBeNull()
  })

  it('control: an assisted station shows its copy and its calls, and no reason line', async () => {
    read = { text: 'CQ TEST W1AW W1AW ', wpm: 25, candidates: [{ call: 'W1AW', best: true }] }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    const pane = decodePane()
    expect(pane.querySelector('.cw-decode-text')?.textContent).toBe('CQ TEST W1AW W1AW ')
    expect(pane.querySelector('.cw-decode-off')).toBeNull()
    expect(screen.getByRole('button', { name: 'W1AW' })).toBeTruthy()
    expect(hisCall().value, "the decoder's best guess fills His Call").toBe('W1AW')
  })

  it('control: outside a contest, an empty read is listening, as before', async () => {
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    const pane = decodePane()
    expect(pane.querySelector('.cw-decode-text')?.textContent).toBe(EN['cw.decode.listening'])
    expect(pane.querySelector('.cw-decode-off')).toBeNull()
  })

  it('declared mid-contact, the copy, the calls and the guess in His Call go', async () => {
    read = { text: 'CQ TEST W1AW W1AW ', wpm: 25, candidates: [{ call: 'W1AW', best: true }] }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    expect(hisCall().value, 'control: the guess').toBe('W1AW')
    read = { unassisted: true }
    await nextPoll()
    expect(decodePane().textContent).not.toContain('W1AW')
    expect(screen.queryByRole('button', { name: 'W1AW' })).toBeNull()
    expect(hisCall().value).toBe('')
    expect(decodePane().querySelector('.cw-decode-off')?.textContent).toBe(EN['cw.decode.unassisted'])
  })

  it('the call the operator chose to work is theirs, and stays', async () => {
    read = { unassisted: true, workedCall: 'K1ABC' }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    expect(hisCall().value).toBe('K1ABC')
  })

  it('the Copilot pane says why too, where it promised decoded calls', async () => {
    read = { unassisted: true }
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    const pane = copilotPane()
    expect(pane.textContent, 'no calls are decoded to appear').not.toContain(EN['cw.copilot.empty'])
    expect(pane.textContent).toContain(EN['cw.decode.unassisted'])
  })

  it('control: outside a contest, the Copilot pane still promises decoded calls', async () => {
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await nextPoll()
    const pane = copilotPane()
    expect(pane.textContent).toContain(EN['cw.copilot.empty'])
    expect(pane.textContent).not.toContain(EN['cw.decode.unassisted'])
  })
})
