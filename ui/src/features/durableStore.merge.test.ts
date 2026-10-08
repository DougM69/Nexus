// @vitest-environment jsdom
// The durable store's flush when this window is not the file's only writer: a pop-out, or a
// computer driving this station from afar, saves keys of its own. Each case is paired with the
// state that must NOT happen, because the failure here is silent: a write made elsewhere quietly
// put back by this window's next flush.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import {
  loadDurable,
  durableGet,
  durableSet,
  durableRemove,
  flushDurable,
  setRemoteStation,
  __resetDurableForTest,
} from './durableStore'

const mockLoad = vi.fn<(strict?: boolean) => Promise<Record<string, string>>>()
const mockSave = vi.fn<(s: Record<string, string>) => Promise<boolean>>()
vi.mock('../api', () => ({
  uiStateLoad: (strict?: boolean) => mockLoad(strict),
  uiStateSave: (s: Record<string, string>) => mockSave(s),
}))

/** The file as the store reads it: what a save last wrote, unless a test changes it. */
let file: Record<string, string> = {}

beforeEach(() => {
  __resetDurableForTest()
  window.localStorage.clear()
  file = {}
  mockLoad.mockReset().mockImplementation(async () => ({ ...file }))
  mockSave.mockReset().mockImplementation(async (s) => {
    file = { ...s }
    return true
  })
})
afterEach(() => __resetDurableForTest())

const lastSaved = () => mockSave.mock.calls[mockSave.mock.calls.length - 1][0]

describe('a write made elsewhere', () => {
  it('survives this window’s next flush', async () => {
    file = { 'nexus.watchlist': '["K1ABC"]', 'nexus.navOrder': '["ft8"]' }
    await loadDurable()
    // Another writer (the computer driving this station) saves the watch list.
    file = { ...file, 'nexus.watchlist': '["K1ABC","W1AW"]' }
    // This window then changes something of its own, which flushes.
    durableSet('nexus.navOrder', '["cw","ft8"]')
    await flushDurable()
    expect(lastSaved()).toEqual({
      'nexus.watchlist': '["K1ABC","W1AW"]',
      'nexus.navOrder': '["cw","ft8"]',
    })
    // Not the stale copy this window loaded at boot.
    expect(lastSaved()['nexus.watchlist']).not.toBe('["K1ABC"]')
    // …and this window now reads what the other writer wrote.
    expect(durableGet('nexus.watchlist')).toBe('["K1ABC","W1AW"]')
    // The flush read the file again first, strictly.
    expect(mockLoad).toHaveBeenLastCalledWith(true)
  })

  it('loses to a change this window made to the same key, which is the later one', async () => {
    file = { 'nexus.watchlist': '["K1ABC"]' }
    await loadDurable()
    file = { 'nexus.watchlist': '["W1AW"]' }
    durableSet('nexus.watchlist', '["N0CALL"]')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.watchlist': '["N0CALL"]' })
  })

  it('a key removed elsewhere stays removed, and this window lets it go', async () => {
    file = { 'nexus.sats.chasing': '[25544]', 'nexus.navOrder': '["ft8"]' }
    await loadDurable()
    file = { 'nexus.navOrder': '["ft8"]' }
    durableSet('nexus.navOrder', '["cw"]')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.navOrder': '["cw"]' })
    expect('nexus.sats.chasing' in lastSaved()).toBe(false)
  })

  it('a key this window removed is removed, whatever else the file holds', async () => {
    file = { 'nexus.watchlist': '["K1ABC"]', 'nexus.profiles': '[]' }
    await loadDurable()
    file = { ...file, 'nexus.profiles': '[{"name":"x"}]' }
    durableRemove('nexus.watchlist')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.profiles': '[{"name":"x"}]' })
  })
})

describe('with no other writer', () => {
  it('a flush writes exactly the whole cache, as it always did', async () => {
    file = { 'nexus.watchlist': '["K1ABC"]', 'nexus-ui-scale-cap': '1.5' }
    await loadDurable()
    durableSet('nexus.memory.bank.v2', '[1]')
    durableRemove('nexus-ui-scale-cap')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.watchlist': '["K1ABC"]', 'nexus.memory.bank.v2': '[1]' })
  })

  it('a file that does not read again is written whole, as it always was', async () => {
    file = { 'nexus.watchlist': '["K1ABC"]' }
    await loadDurable()
    mockLoad.mockImplementation(async (strict) => {
      if (strict) throw new Error('uiStateUnreadable')
      return { ...file }
    })
    durableSet('nexus.navOrder', '["cw"]')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.watchlist': '["K1ABC"]', 'nexus.navOrder': '["cw"]' })
  })

  it('a change made while a flush is on its way is written by the next one', async () => {
    file = {}
    await loadDurable()
    durableSet('nexus.watchlist', '["A"]')
    let release: () => void = () => {}
    mockSave.mockImplementationOnce(
      (s) =>
        new Promise<boolean>((resolve) => {
          release = () => {
            file = { ...s }
            resolve(true)
          }
        }),
    )
    const first = flushDurable()
    await vi.waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1))
    durableSet('nexus.watchlist', '["A","B"]')
    release()
    await first
    expect(durableGet('nexus.watchlist')).toBe('["A","B"]')
    await flushDurable()
    expect(lastSaved()).toEqual({ 'nexus.watchlist': '["A","B"]' })
  })
})

describe('on a computer driving another station', () => {
  it('the station’s keys never come from this computer’s localStorage', async () => {
    // This computer's own watch list and rail, from when it ran its own station.
    window.localStorage.setItem('nexus.watchlist', '["MINE"]')
    window.localStorage.setItem('nexus.navOrder', '["cw"]')
    setRemoteStation(true)
    file = {} // the station it drives has none
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBeNull()
    await flushDurable()
    for (const [saved] of mockSave.mock.calls) expect('nexus.watchlist' in saved).toBe(false)
    // CONTROL: a key of this computer's own still migrates, as on any computer.
    expect(durableGet('nexus.navOrder')).toBe('["cw"]')
    expect(lastSaved()['nexus.navOrder']).toBe('["cw"]')
  })

  it('a station key changed here goes to the station and never into this computer’s localStorage', async () => {
    window.localStorage.setItem('nexus.watchlist', '["MINE"]')
    setRemoteStation(true)
    file = { 'nexus.watchlist': '["SHACK"]' }
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBe('["SHACK"]')
    durableSet('nexus.watchlist', '["SHACK","W1AW"]')
    durableSet('nexus.navOrder', '["ft8"]')
    await flushDurable()
    expect(lastSaved()['nexus.watchlist']).toBe('["SHACK","W1AW"]')
    expect(window.localStorage.getItem('nexus.watchlist')).toBe('["MINE"]')
    // CONTROL: this computer's own key is kept in its localStorage as ever.
    expect(window.localStorage.getItem('nexus.navOrder')).toBe('["ft8"]')
  })

  it('until the station’s keys have loaded, a change to one is not kept', async () => {
    setRemoteStation(true)
    mockLoad.mockRejectedValue(new Error('notLive'))
    await loadDurable()
    durableSet('nexus.watchlist', '["W1AW"]')
    expect(durableGet('nexus.watchlist')).toBeNull()
    // CONTROL: a key of this computer's own is kept.
    durableSet('nexus.navOrder', '["cw"]')
    expect(durableGet('nexus.navOrder')).toBe('["cw"]')
  })
})
