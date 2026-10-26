// @vitest-environment jsdom
//
// UPDATE FROM CALLBOOK — what the tab asks the station for and tells the operator: the preview's
// counts, time and the day's lookups against the cap; a start only after asking, on exactly the
// contacts previewed with their count; the progress while it runs, and Stop; and however it ends,
// the toast whose Undo takes the whole fill back, the calls nobody knows and the calls left for
// another run. Driving another station, it can be previewed and not started.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor, act } from '@testing-library/react'
import type { CallbookUpdatePreview, CallbookUpdateStatus } from '../api'
import { subscribeToasts, type Toast } from '../toast'
import { setRemoteStation } from '../features/durableStore'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'

const api = vi.hoisted(() => ({
  callbookUpdatePreview: vi.fn(),
  callbookUpdateStart: vi.fn(),
  callbookUpdateStatus: vi.fn(),
  callbookUpdateCancel: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../api', () => api)

import { LogTools } from './LogTools'
import { POLL_MS } from './LogCallbookUpdate'

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  api.callbookUpdateStatus.mockResolvedValue(idle)
  api.callbookUpdateCancel.mockResolvedValue(undefined)
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
  setRemoteStation(false)
  vi.useRealTimers()
})

const idle: CallbookUpdateStatus = {
  running: false, done: 0, total: 0, fromCache: 0, lookedUp: 0, spent: 0, cap: 250, ended: null,
}

const preview: CallbookUpdatePreview = {
  ids: ['x', 'y', 'z'],
  selected: 3,
  missing: 0,
  calls: 2,
  kept: 1,
  toLookUp: 1,
  paceSecs: 2,
  estimateSecs: 2,
  spent: 10,
  cap: 250,
  // Fewer than selected: the start must send the count previewed, not the count it changes.
  couldChange: 2,
  fields: [['GRIDSQUARE', 3], ['NAME', 2]],
  configured: true,
  services: ['qrz', 'hamqth', 'callook'],
  locationTooOld: 0,
}

function open(picked: string[] = []) {
  const onChanged = vi.fn()
  render(
    <LogTools
      open
      tool="callbook"
      onTool={() => {}}
      onClose={() => {}}
      onChanged={onChanged}
      picked={new Set(picked)}
      query={DEFAULT_LOG_QUERY}
      shown={42}
    />,
  )
  return onChanged
}

describe('update from callbook', () => {
  it('previews the calls, the time, the day’s lookups and the fields, by value', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, toLookUp: 300, estimateSecs: 600 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText('3 contacts, 2 different calls: 1 already looked up, 300 to look up.')
    expect(api.callbookUpdatePreview).toHaveBeenCalledWith({ ids: ['x', 'y', 'z'] })
    screen.getByText('About 10 min, at one lookup every 2 s.')
    screen.getByText('Lookups today: 10 of 250 (Settings ▸ Logbook ▸ Callbook lookups).')
    screen.getByText(/Today’s limit stops it after 240 lookups\./)
    screen.getByText('GRIDSQUARE 3 · NAME 2')
    screen.getByText(
      'Asks QRZ, then HamQTH, then callook.info (US calls), the next only when the one before has no record. The pace and the daily limit cover them all.',
    )
    screen.getByText(/^Location \(grid, QTH, state, county, IOTA, CQ and ITU zones, latitude\/longitude, UTC offset\) is filled only on contacts from the last 12 months/)
    expect(screen.queryByText(/older than 12 months/)).toBeNull()
  })

  it('names only the services set up, and counts the older contacts whose location stays blank', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, services: ['hamqth'], locationTooOld: 7 })
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByText('7 contacts are older than 12 months: their location stays blank.')
    screen.getByText(/^Asks HamQTH, the next only when/)
    expect(screen.queryByText(/^Asks QRZ/)).toBeNull()
  })

  it('starts only after asking, on the contacts previewed with their count, then shows progress', async () => {
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Update 2 contacts' }))
    expect(api.callbookUpdateStart).not.toHaveBeenCalled()
    screen.getByText(/Look up 1 call now\?/)
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await waitFor(() => expect(api.callbookUpdateStart).toHaveBeenCalledWith(['x', 'y', 'z'], 3))
    await screen.findByText('Looked up 0 of 2 calls (0 from the cache) · lookups today: 10 of 250')
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(api.callbookUpdateCancel).toHaveBeenCalled()
  })

  it('reports how it ended: one toast whose Undo takes the run back, the unknown calls and the rest', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    api.undoHistoryRun.mockResolvedValue({
      changed: 3, run: 'undo-1', kept: [], held: [], heldUnder: null, gone: 0, alreadyThere: 0,
      changedSince: 0, duplicate: 0, unknownKind: 0, changedSinceCalls: [], duplicateCalls: [],
    })
    const onChanged = open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Update 2 contacts' }))
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await waitFor(() => expect(api.callbookUpdateStart).toHaveBeenCalled())
    api.callbookUpdateStatus.mockResolvedValue({
      running: false, done: 1, total: 2, fromCache: 1, lookedUp: 0, spent: 250, cap: 250,
      ended: {
        stop: 'limit',
        error: null,
        unknown: ['Q9QQQ'],
        notLookedUp: 1,
        made: { changed: 3, run: 'run-7', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
      },
    })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS + 10) })
    await screen.findByText('Stopped at today’s limit of 250 lookups. What was found is filled in.')
    screen.getByText('1 call not looked up yet: run the update again to continue.')
    screen.getByText('Q9QQQ')
    const toast = toasts.find((x) => x.message === 'Updated 3 contacts from the callbook')
    expect(toast?.actionLabel).toBe('Undo')
    expect(onChanged).toHaveBeenCalled()
    await act(async () => { await toast?.action?.() })
    expect(api.undoHistoryRun).toHaveBeenCalledWith('run-7')
    // Told once: a later poll does not toast again.
    const n = toasts.length
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS * 3) })
    expect(toasts.length).toBe(n)
  })

  it('reports the end once when two polls overlap: one toast', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: false })
    api.callbookUpdatePreview.mockResolvedValue(preview)
    api.callbookUpdateStart.mockResolvedValue(undefined)
    open(['x', 'y', 'z'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    fireEvent.click(screen.getByRole('button', { name: 'Update 2 contacts' }))
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(api.callbookUpdateStart).toHaveBeenCalled()
    // Each poll waits on the station until released: the second starts while the first is out.
    const pending: ((s: CallbookUpdateStatus) => void)[] = []
    api.callbookUpdateStatus.mockImplementation(
      () => new Promise<CallbookUpdateStatus>((resolve) => { pending.push(resolve) }),
    )
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS) })
    await act(async () => { await vi.advanceTimersByTimeAsync(POLL_MS) })
    expect(pending.length).toBe(2)
    const ended: CallbookUpdateStatus = {
      running: false, done: 2, total: 2, fromCache: 1, lookedUp: 1, spent: 11, cap: 250,
      ended: {
        stop: 'done', error: null, unknown: [], notLookedUp: 0,
        made: { changed: 5, run: 'run-8', gone: 0, changedSince: 0, needsChoice: 0, distinct: 0 },
      },
    }
    await act(async () => {
      for (const resolve of pending) resolve(ended)
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(toasts.filter((x) => x.message === 'Updated 5 contacts from the callbook')).toHaveLength(1)
  })

  it('with no callbook set up, says so and cannot start', async () => {
    api.callbookUpdatePreview.mockResolvedValue({ ...preview, configured: false })
    open(['x'])
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Update 2 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
    screen.getByText('Set up QRZ or HamQTH in Settings ▸ Logbook ▸ Callbook lookups first.')
  })

  it('driving another station, it previews and cannot be started', async () => {
    setRemoteStation(true)
    api.callbookUpdatePreview.mockResolvedValue(preview)
    open()
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Update 2 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
  })
})
