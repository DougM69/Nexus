// @vitest-environment jsdom
// Programs' spots are asked for only when the station says they changed, and again while some are
// on the map; a row naming no program is never taken for one.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, waitFor, cleanup, act } from '@testing-library/react'
import type { SpotRow } from '../types'
import { useTciSpots, TCI_SPOTS_REFRESH_MS } from './tciSpots'

const getTciSpots = vi.hoisted(() => vi.fn())
vi.mock('../api', () => ({ getTciSpots: (...a: unknown[]) => getTciSpots(...a) }))

const row = (call: string, tci: number | null | undefined): SpotRow =>
  ({ call, band: '20m', mode: 'CW', freqMhz: 14.025, spotter: '', ageSecs: 1, comment: '', tci }) as SpotRow

beforeEach(() => {
  getTciSpots.mockReset()
  getTciSpots.mockResolvedValue([row('K1ABC', 2)])
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('useTciSpots', () => {
  it('asks nothing while the station has none or says nothing (0, null, an older station)', async () => {
    for (const generation of [0, null, undefined]) {
      const { result } = renderHook(() => useTciSpots(generation))
      await act(async () => {})
      expect(result.current).toEqual([])
    }
    expect(getTciSpots).not.toHaveBeenCalled()
  })

  it('asks when the generation moves, and not again for the same one; CONTROL: a new one asks again', async () => {
    const { result, rerender } = renderHook(({ g }) => useTciSpots(g), { initialProps: { g: 1 } })
    await waitFor(() => expect(result.current.map((r) => r.call)).toEqual(['K1ABC']))
    expect(getTciSpots).toHaveBeenCalledTimes(1)
    rerender({ g: 1 })
    await act(async () => {})
    expect(getTciSpots).toHaveBeenCalledTimes(1)
    getTciSpots.mockResolvedValue([row('W1AW', 3)])
    rerender({ g: 2 })
    await waitFor(() => expect(result.current.map((r) => r.call)).toEqual(['W1AW']))
    expect(getTciSpots).toHaveBeenCalledTimes(2)
  })

  it('never takes a row naming no program for a program spot', async () => {
    getTciSpots.mockResolvedValue([row('K1ABC', 2), row('W1AW', null), row('N0CALL', undefined)])
    const { result } = renderHook(() => useTciSpots(5))
    await waitFor(() => expect(result.current.map((r) => r.call)).toEqual(['K1ABC']))
  })

  it('clears when the server goes, and asks again every 15 s while some are on the map', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const { result, rerender } = renderHook(({ g }) => useTciSpots(g), { initialProps: { g: 4 as number | undefined } })
    await act(async () => {})
    expect(result.current).toHaveLength(1)
    expect(getTciSpots).toHaveBeenCalledTimes(1)
    await act(async () => {
      vi.advanceTimersByTime(TCI_SPOTS_REFRESH_MS)
    })
    expect(getTciSpots).toHaveBeenCalledTimes(2)
    getTciSpots.mockResolvedValue([])
    await act(async () => {
      vi.advanceTimersByTime(TCI_SPOTS_REFRESH_MS)
    })
    expect(getTciSpots).toHaveBeenCalledTimes(3)
    expect(result.current).toEqual([])
    await act(async () => {
      vi.advanceTimersByTime(3 * TCI_SPOTS_REFRESH_MS)
    })
    expect(getTciSpots, 'none on the map: not asked again until they change').toHaveBeenCalledTimes(3)
    getTciSpots.mockResolvedValue([row('K1ABC', 2)])
    rerender({ g: 5 })
    await act(async () => {})
    expect(result.current).toHaveLength(1)
    rerender({ g: undefined })
    await act(async () => {})
    expect(result.current, 'the server gone: none').toEqual([])
  })
})
