// @vitest-environment jsdom
//
// A BAND CHANGE AND THE CONTEST STRIP. The line clears as every strip's does (`clearCallOnBandChange`),
// the exchange boxes with it, and Restore puts them back. Two things are the contest's own:
//
//   - a contest contact logs on the band the radio is on (the contest log takes the radio's band),
//     so Restore waits until the radio is back on the band the line was made on;
//   - a contact's edit (Ctrl+Y) has the line's place while it is open; a band change then is not
//     skipped, it clears the line, with its notice, as the edit closes.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(),
  contestEdit: vi.fn(),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  contestLoggedQth: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800
const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

/** An Illinois QSO Party session as the engine serialises it. */
const party = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    role: 'in_state',
    bands: ['40m', '20m'],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties', domains: ['il_counties'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

const LOG = [row('K9AAA', 'COOK', T0)]

/** The CW cockpit's contest strip at band-change count `tick`, on `band`. */
const strip = (tick: number, band = '20m') => (
  <LogEntry
    onOpenLogbook={() => {}}
    snap={{ radio: { band, dialMhz: band === '20m' ? 14.025 : 7.025, txEnabled: true, txAllowed: true }, hunt: null, bandChangeTick: tick } as unknown as AppSnapshot}
    mode="CW"
    defaultRst="599"
    exchange="terrestrial"
    titled={false}
    fieldDay={party(LOG)}
    fdMode="CW"
    active
    clearCallOnBandChange
  />
)

const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const fdInputs = () => [...document.querySelectorAll<HTMLInputElement>('.le-fd-big input')]
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const notice = () => document.querySelector('.le-band-cleared')
const restore = () => screen.getByRole('button', { name: 'Restore' }) as HTMLButtonElement
const editor = () => document.querySelector<HTMLElement>('.contest-edit')
const ctrlY = () => fireEvent.keyDown(window, { key: 'y', code: 'KeyY', ctrlKey: true })
const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('a band change and the contest strip', () => {
  it('the exchange goes with the call; Restore waits for the band the line was made on, then puts it all back', async () => {
    const view = render(strip(1))
    type(callBox(), 'N9XYZ')
    const typedQth = fdInputs().findIndex((b) => b !== callBox() && b.value === '')
    type(fdInputs()[typedQth], 'LAKE')
    const before = fdInputs().map((b) => b.value)
    view.rerender(strip(2, '40m'))
    await settle()
    expect(callBox().value).toBe('')
    expect(fdInputs()[typedQth].value, 'the exchange box clears').toBe('')
    expect(notice()?.textContent).toContain('N9XYZ')
    expect(restore().disabled, 'a contest contact logs on the radio’s band: not on 40 m').toBe(true)
    expect(notice()?.textContent).toContain('20m')
    view.rerender(strip(3, '20m'))
    await settle()
    expect(notice(), 'a band change back to 20 m with an empty line keeps the notice').not.toBeNull()
    expect(restore().disabled, 'back on 20 m').toBe(false)
    fireEvent.click(restore())
    await settle()
    expect(fdInputs().map((b) => b.value)).toEqual(before)
    expect(api.contestWorking).toHaveBeenLastCalledWith('N9XYZ')
  })

  it('a band change while a contact edit is open clears the line, with its notice, as the edit closes', async () => {
    const view = render(strip(1))
    type(callBox(), 'N9XYZ')
    ctrlY()
    expect(editor(), 'precondition: the edit is open').not.toBeNull()
    view.rerender(strip(2, '40m'))
    await settle()
    expect(editor(), 'the edit stays open').not.toBeNull()
    fireEvent.keyDown(editor()!.querySelector('input')!, { key: 'Escape' })
    await settle()
    expect(editor()).toBeNull()
    expect(callBox().value).toBe('')
    expect(notice()?.textContent).toContain('N9XYZ')
  })
})
