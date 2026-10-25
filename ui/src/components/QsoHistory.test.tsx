// @vitest-environment jsdom
//
// A CONTACT'S HISTORY and RECENTLY DELETED — what the operator sees of the log's change history,
// and that each Undo, Redo and Restore asks the backend for exactly that one thing.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor } from '@testing-library/react'
import type { QsoHistoryEntry } from '../api'
import { subscribeToasts, type Toast } from '../toast'

const api = vi.hoisted(() => ({
  qsoHistory: vi.fn(),
  undoQsoChange: vi.fn(),
  undoHistoryRun: vi.fn(),
  recentlyDeletedQsos: vi.fn(),
  restoreDeletedQso: vi.fn(),
}))
vi.mock('../api', () => api)

import { QsoHistory } from './QsoHistory'
import { RecentlyDeleted } from './RecentlyDeleted'

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
})

const entry = (over: Partial<QsoHistoryEntry>): QsoHistoryEntry => ({
  seq: 1,
  kind: 'create',
  source: 'operator',
  run: 'r1',
  atMs: Date.UTC(2026, 9, 24, 12, 0, 0),
  undoes: null,
  undoneBy: null,
  changes: [],
  ...over,
})

const done = { changed: 1, run: 'undo-run', kept: [], gone: 0, alreadyThere: 0, changedSince: 0 }

describe("a contact's history", () => {
  it('lists each change newest first, who made it and the fields it changed, old → new', async () => {
    api.qsoHistory.mockResolvedValue([
      entry({ seq: 1 }),
      entry({ seq: 2, kind: 'merge', source: 'lotw', changes: [{ tag: 'LOTW_QSL_RCVD', old: null, new: 'Y' }] }),
      entry({ seq: 3, kind: 'edit', changes: [{ tag: 'NAME', old: 'Bob', new: 'Robert' }] }),
    ])
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('NAME')
    expect(api.qsoHistory).toHaveBeenCalledWith('1:2:3')
    const items = [...document.querySelectorAll('.qso-history-entry')].map((li) => li.textContent ?? '')
    expect(items).toHaveLength(3)
    expect(items[0]).toContain('Edited · you')
    expect(items[0]).toContain('Bob → Robert')
    expect(items[1]).toContain('Merged · LoTW')
    expect(items[1]).toContain('(none) → Y')
    expect(items[2]).toContain('Logged · you')
    // Undo on a change of fields; none on the contact's logging.
    expect(screen.getAllByRole('button', { name: 'Undo' })).toHaveLength(2)
  })

  it('undoes one change, says so with a Redo, and the Redo undoes the undo', async () => {
    api.qsoHistory.mockResolvedValue([entry({ seq: 7, kind: 'edit', changes: [{ tag: 'COMMENT', old: null, new: 'x' }] })])
    api.undoQsoChange.mockResolvedValue(done)
    api.undoHistoryRun.mockResolvedValue({ ...done, run: 'redo-run' })
    const onChanged = vi.fn()
    render(<QsoHistory id="1:2:3" onChanged={onChanged} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }))
    await waitFor(() => expect(api.undoQsoChange).toHaveBeenCalledWith(7))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    const toast = toasts.find((t) => t.message.startsWith('Change undone'))
    expect(toast?.actionLabel).toBe('Redo')
    toast?.action?.()
    await waitFor(() => expect(api.undoHistoryRun).toHaveBeenCalledWith('undo-run'))
  })

  it('shows an undone change as undone, with no Undo of its own', async () => {
    api.qsoHistory.mockResolvedValue([
      entry({ seq: 2, kind: 'edit', undoneBy: 3, changes: [{ tag: 'NAME', old: 'A', new: 'B' }] }),
      entry({ seq: 3, kind: 'edit', source: 'undo', undoes: 2, changes: [{ tag: 'NAME', old: 'B', new: 'A' }] }),
    ])
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('Undone')
    // The undo itself can be undone: that is the redo.
    expect(screen.getAllByRole('button', { name: 'Undo' })).toHaveLength(1)
  })

  it('says when the history cannot be read, rather than showing nothing', async () => {
    api.qsoHistory.mockRejectedValue('no store')
    render(<QsoHistory id="1:2:3" />)
    await screen.findByText('The history could not be read.')
  })
})

describe('recently deleted', () => {
  it('lists the deleted contacts and restores the one asked for', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([
      { id: 'a', call: 'K1ABC', band: '20m', mode: 'FT8', whenUnix: 1_782_583_500, deletedAtMs: 0, source: 'operator' },
    ])
    api.restoreDeletedQso.mockResolvedValue(done)
    const onRestored = vi.fn()
    render(<RecentlyDeleted open onClose={() => {}} onRestored={onRestored} />)
    expect(await screen.findByText('K1ABC')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))
    await waitFor(() => expect(api.restoreDeletedQso).toHaveBeenCalledWith('a'))
    await waitFor(() => expect(onRestored).toHaveBeenCalled())
    expect(toasts.some((t) => t.message.startsWith('Restored K1ABC'))).toBe(true)
  })

  it('says when nothing has been deleted', async () => {
    api.recentlyDeletedQsos.mockResolvedValue([])
    render(<RecentlyDeleted open onClose={() => {}} onRestored={() => {}} />)
    await screen.findByText('No deleted contacts.')
  })
})
