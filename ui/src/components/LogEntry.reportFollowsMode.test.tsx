// @vitest-environment jsdom
//
// THE ORDINARY LOG LINE'S REPORTS FOLLOW THE MODE, AS THE CONTEST STRIP'S DO. One strip can stay on
// screen while the mode it logs under changes: the Satellites section's follows the station, so an FM
// pass and then a CW one are worked in one strip. A report nobody typed is the mode's own, 59 on FM
// and 599 on CW; a report the operator typed (or the CW decoder read off the air) is kept for the
// contact it was set for, and only that one.
//
// Asserted by value on the boxes and on the record handed to `logQso`, never on a class name.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot, LoggedQso } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const mockedLogQso = vi.mocked(logQso)

const snap = {
  radio: { band: '70cm', dialMhz: 435.64332 },
  hunt: null,
} as unknown as AppSnapshot

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

type StripMode = 'FM' | 'CW'
type Live = { call: string | null; rst: string | null; name: null; confirmed: boolean } | null

/** The Satellites section's strip, as it mounts it: the mode and its report follow the station. */
const strip = (m: StripMode, cwLive: Live = null) => (
  <LogEntry snap={snap} mode={m} defaultRst={m === 'CW' ? '599' : '59'} exchange="satellite" cwLive={cwLive} />
)

const box = (name: 'Sent' | 'Rcvd') => screen.getByLabelText(name) as HTMLInputElement
const reports = () => [box('Sent').value, box('Rcvd').value]

/** Log the contact with the call typed, and return the record the backend was handed. */
async function logIt(call: string): Promise<LoggedQso> {
  fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: call } })
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  await waitFor(() => expect(mockedLogQso).toHaveBeenCalledTimes(1))
  return mockedLogQso.mock.calls[0][0] as LoggedQso
}

describe('the ordinary log line: a report follows the mode unless it was set for this contact', () => {
  it('reports nobody typed follow FM to CW and back, and the CW contact logs 599 and 599', async () => {
    const view = render(strip('FM'))
    expect(reports(), 'control: on FM, before any move').toEqual(['59', '59'])
    view.rerender(strip('CW'))
    expect(reports(), 'moved to CW').toEqual(['599', '599'])
    view.rerender(strip('FM'))
    expect(reports(), 'back on FM').toEqual(['59', '59'])
    view.rerender(strip('CW'))
    const rec = await logIt('W1AW')
    expect([rec.mode, rec.rstSent, rec.rstRcvd]).toEqual(['CW', '599', '599'])
  })

  it('a typed report stays through the move and is logged as typed; the next contact starts on the mode’s own', async () => {
    const view = render(strip('FM'))
    fireEvent.change(box('Rcvd'), { target: { value: '57' } })
    view.rerender(strip('CW'))
    expect(reports(), 'the untouched Sent moved, the typed Rcvd stayed').toEqual(['599', '57'])
    const rec = await logIt('W1AW')
    expect([rec.rstSent, rec.rstRcvd]).toEqual(['599', '57'])
    await waitFor(() => expect(reports(), 'the next contact').toEqual(['599', '599']))
    view.rerender(strip('FM'))
    expect(reports(), 'and it follows the next move').toEqual(['59', '59'])
  })

  it('Clear ends the contact for a typed report too: the box goes back to the mode’s own, and follows the next move', () => {
    const view = render(strip('CW'))
    fireEvent.change(box('Sent'), { target: { value: '559' } })
    expect(reports(), 'control: the typed report shows').toEqual(['559', '599'])
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(reports()).toEqual(['599', '599'])
    view.rerender(strip('FM'))
    expect(reports()).toEqual(['59', '59'])
  })

  it('the CW decoder’s read of their report fills an untouched Rcvd, never a typed one, and stays through a move', () => {
    const live = (rst: string): Live => ({ call: 'K9ABC', rst, name: null, confirmed: true })
    const view = render(strip('CW', live('579')))
    expect(reports(), 'filled an untouched box').toEqual(['599', '579'])
    view.rerender(strip('FM', live('579')))
    expect(reports(), 'what was read off the air is this contact’s').toEqual(['59', '579'])
    cleanup()

    const typed = render(strip('CW', { call: 'K9ABC', rst: null, name: null, confirmed: true }))
    fireEvent.change(box('Rcvd'), { target: { value: '559' } })
    typed.rerender(strip('CW', live('579')))
    expect(reports(), 'never over a typed one').toEqual(['599', '559'])
  })
})
