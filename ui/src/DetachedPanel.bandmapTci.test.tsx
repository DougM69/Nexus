// @vitest-environment jsdom
//
// THE BAND-MAP POP-OUT DRAWS PROGRAMS' SPOTS AND CLICKS THEM ONLY TO TUNE (Share by TCI).
//
// The real pop-out on a fake station: the snapshot's TCI server line says programs' spots changed
// (`radio.tciServer.spots`), so the pop-out asks for them (`getTciSpots`) and draws each with its
// source. ⛔ A click on a program's spot asks the station to tune there and tell the programs
// (`clickTciSpot`) and never works it (`workSpot`, whose section entry can arm). CONTROL: a cluster
// spot's click still works it, and a station with no TCI server line is never asked.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import type { AppSnapshot, SpotRow } from './types'

const station = vi.hoisted(() => ({
  snap: null as unknown,
  listeners: new Set<(s: unknown) => void>(),
}))

const api = vi.hoisted(() => ({
  getTciSpots: vi.fn(),
  clickTciSpot: vi.fn(),
  workSpot: vi.fn(),
}))

vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  out.subscribeSnapshot = vi.fn((cb: (s: unknown) => void) => {
    station.listeners.add(cb)
    cb(station.snap)
    return () => station.listeners.delete(cb)
  })
  out.getBandPlan = vi.fn().mockResolvedValue([])
  out.getNeedAlerts = vi.fn().mockResolvedValue([])
  out.getSettings = vi.fn().mockResolvedValue({})
  out.getPropagation = vi.fn().mockResolvedValue(null)
  out.getFreqMarkers = vi.fn().mockResolvedValue([])
  out.askLog = vi.fn().mockRejectedValue('not in this test')
  out.getAllSpots = vi.fn(() => Promise.resolve([CLUSTER]))
  out.getTciSpots = api.getTciSpots
  out.clickTciSpot = api.clickTciSpot
  out.workSpot = api.workSpot
  return out
})

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({ call: 'K1ABC', entity: '', zone: 0, band: '20m', freqMhz: 14.025, mode: 'CW', spotter: '', corroborators: [], ageSecs: 30, comment: '', licensed: true, ...over }) as SpotRow
const CLUSTER = spot({ call: 'W1AW', freqMhz: 14.03, spotter: 'K3LR' })
const PROGRAM = spot({ call: 'K1ABC', freqMhz: 14.025, comment: 'CQ NA', tci: 3 })

const snapWith = (tciServer: unknown) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    logTick: 1,
    stations: [],
    conversations: [],
    activePeer: null,
    recentDecodes: [],
    hunt: null,
    link: { tier: 'ft8' },
    radio: {
      band: '20m', dialMhz: 14.03, txAllowed: true, phoneSegLo: null, phoneSegHi: null, sideband: 'USB',
      catOk: true, txBusyReason: null, transmitting: false, tciServer,
    },
  }) as unknown as AppSnapshot

const label = (root: HTMLElement, call: string) =>
  [...root.querySelectorAll<HTMLButtonElement>('.bandmap-spot')].find(
    (b) => b.querySelector('.bandmap-call')?.textContent === call,
  )

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  station.snap = snapWith({ port: 40001, error: null, programs: [], spots: 7 })
  api.getTciSpots.mockReset().mockResolvedValue([PROGRAM])
  api.clickTciSpot.mockReset().mockResolvedValue(snapWith({ port: 40001, error: null, programs: [], spots: 7 }))
  api.workSpot.mockReset().mockResolvedValue(snapWith(null))
})
afterEach(cleanup)

describe("the band-map pop-out and programs' spots", () => {
  it("draws a program's spot with its source, and its click only asks the station to tune and tell", async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await waitFor(() => expect(label(container, 'K1ABC')).toBeTruthy())
    expect(label(container, 'K1ABC')!.querySelector('.type-tci')?.textContent).toBe('TCI 3')
    await waitFor(() => expect(label(container, 'W1AW')).toBeTruthy())
    fireEvent.click(label(container, 'K1ABC')!)
    await waitFor(() => expect(api.clickTciSpot).toHaveBeenCalledWith('K1ABC', 14.025))
    expect(api.workSpot, 'a program spot was worked').not.toHaveBeenCalled()
    fireEvent.click(label(container, 'W1AW')!)
    await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
    expect(api.clickTciSpot, 'control: the cluster spot is worked, not tuned as a program’s').toHaveBeenCalledTimes(1)
  })

  it('CONTROL: a station with no TCI server line is never asked for programs’ spots', async () => {
    station.snap = snapWith(undefined)
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await waitFor(() => expect(label(container, 'W1AW')).toBeTruthy())
    expect(api.getTciSpots).not.toHaveBeenCalled()
    expect(label(container, 'K1ABC')).toBeUndefined()
  })
})
