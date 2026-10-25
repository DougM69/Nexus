// @vitest-environment jsdom
//
// THE CLEAN-UP TOOLS — what each asks the backend for: a merge sends the copy kept and the copy
// each conflicting field is taken from, and waits for a guarded conflict to be chosen; a bulk edit
// previews, names the count, asks once more, and sends exactly the fields and contacts chosen; the
// check sends only the fixes ticked, each with what it found. Each change's toast offers Undo of
// its whole run. Driving another station, nothing that changes the log can be pressed.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen, waitFor } from '@testing-library/react'
import type { DupeGroup, LogProblem } from '../api'
import type { LoggedQso } from '../types'
import { subscribeToasts, type Toast } from '../toast'
import { setRemoteStation } from '../features/durableStore'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'

const api = vi.hoisted(() => ({
  findLogDuplicates: vi.fn(),
  mergeLogDuplicates: vi.fn(),
  bulkEditPreview: vi.fn(),
  bulkEditApply: vi.fn(),
  checkLogbook: vi.fn(),
  applyLogFixes: vi.fn(),
  undoHistoryRun: vi.fn(),
}))
vi.mock('../api', () => api)

import { LogTools, type LogTool } from './LogTools'

/** The last of a list (the lib target here predates `Array.prototype.at`). */
const last = <T,>(xs: T[]): T => xs[xs.length - 1]

let toasts: Toast[] = []
let unsubscribe = () => {}
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset()
  unsubscribe = subscribeToasts((t) => { toasts = t })
})
afterEach(() => {
  unsubscribe()
  cleanup()
  setRemoteStation(false)
})

const made = { changed: 2, run: 'run-1', gone: 0, changedSince: 0, needsChoice: 0 }

const qso = (id: string, whenUnix: number, over: Partial<LoggedQso> = {}): LoggedQso =>
  ({ id, call: 'W1AW', band: '20m', mode: 'FT8', whenUnix, freqMhz: 14.074, ...over }) as LoggedQso

function open(tool: LogTool, picked: string[] = []) {
  const onChanged = vi.fn()
  render(
    <LogTools
      open
      tool={tool}
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

describe('duplicates', () => {
  const group: DupeGroup = {
    copies: [qso('a', 1_767_268_800, { confirmed: true }), qso('b', 1_767_268_860)],
    editKeys: ['ka', 'kb'],
    keep: 1,
    conflicts: [
      { tag: 'GRIDSQUARE', values: [{ id: 'a', value: 'FN31' }, { id: 'b', value: 'FN42' }], guarded: true, chosen: 'b' },
      { tag: 'COMMENT', values: [{ id: 'a', value: 'one' }, { id: 'b', value: 'two' }], guarded: false, chosen: 'b' },
    ],
  }

  it('waits for a guarded conflict to be chosen, then merges with the copy kept and each choice', async () => {
    api.findLogDuplicates.mockResolvedValue({ scanned: 10, groups: [group] })
    api.mergeLogDuplicates.mockResolvedValue(made)
    api.undoHistoryRun.mockResolvedValue({
      changed: 2, run: 'u', kept: [], gone: 0, alreadyThere: 0, changedSince: 0, duplicate: 0,
      unknownKind: 0, changedSinceCalls: [], duplicateCalls: [],
    })
    const onChanged = open('dupes')
    fireEvent.click(last(screen.getAllByRole('button', { name: 'Find' })))
    const merge = await screen.findByRole('button', { name: 'Merge 2 into one' })
    expect(api.findLogDuplicates).toHaveBeenCalledWith('rule')
    expect((merge as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByRole('button', { name: 'Merge all 0 ready' })).toHaveProperty('disabled', true)

    fireEvent.click(screen.getByRole('radio', { name: 'FN31' }))
    expect((merge as HTMLButtonElement).disabled).toBe(false)
    fireEvent.click(merge)
    await waitFor(() => expect(api.mergeLogDuplicates).toHaveBeenCalledTimes(1))
    expect(api.mergeLogDuplicates).toHaveBeenCalledWith([{
      keep: 'b',
      copies: [{ id: 'a', editKey: 'ka' }, { id: 'b', editKey: 'kb' }],
      choices: { GRIDSQUARE: 'a', COMMENT: 'b' },
    }])
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
    const toast = last(toasts)
    expect(toast.message).toBe('Merged 1 contact')
    await toast.action!()
    expect(api.undoHistoryRun).toHaveBeenCalledWith('run-1')
  })

  it('keeping the other copy makes its value the default of an unguarded conflict', async () => {
    api.findLogDuplicates.mockResolvedValue({ scanned: 10, groups: [group] })
    api.mergeLogDuplicates.mockResolvedValue(made)
    open('dupes')
    fireEvent.click(last(screen.getAllByRole('button', { name: 'Find' })))
    await screen.findByRole('button', { name: 'Merge 2 into one' })
    const keeps = screen.getAllByRole('radio').filter((r) => (r as HTMLInputElement).name === 'keep-0')
    fireEvent.click(keeps[0])
    fireEvent.click(screen.getByRole('radio', { name: 'FN42' }))
    fireEvent.click(screen.getByRole('button', { name: 'Merge 2 into one' }))
    await waitFor(() => expect(api.mergeLogDuplicates).toHaveBeenCalledTimes(1))
    expect(api.mergeLogDuplicates.mock.calls[0][0][0]).toMatchObject({
      keep: 'a',
      choices: { GRIDSQUARE: 'b', COMMENT: 'a' },
    })
  })
})

describe('bulk edit', () => {
  it('previews, names the count, asks once more and sends exactly the fields and contacts', async () => {
    api.bulkEditPreview.mockResolvedValue({ selected: 2, change: 2, unchanged: 0, missing: 0 })
    api.bulkEditApply.mockResolvedValue(made)
    open('bulk', ['x', 'y'])
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: ' EM10 ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Change 2 contacts' })
    expect(api.bulkEditPreview).toHaveBeenCalledWith(
      { ids: ['x', 'y'] },
      [{ field: 'MY_GRIDSQUARE', value: 'EM10' }],
    )
    fireEvent.click(apply)
    expect(api.bulkEditApply).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, change 2' }))
    await waitFor(() => expect(api.bulkEditApply).toHaveBeenCalledWith(
      { ids: ['x', 'y'] },
      [{ field: 'MY_GRIDSQUARE', value: 'EM10' }],
    ))
  })

  it('a change after the preview takes the apply away until it is previewed again', async () => {
    api.bulkEditPreview.mockResolvedValue({ selected: 42, change: 40, unchanged: 2, missing: 0 })
    open('bulk')
    fireEvent.change(screen.getByRole('combobox', { name: 'Set or clear' }), { target: { value: 'clear' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    await screen.findByRole('button', { name: 'Change 40 contacts' })
    expect(api.bulkEditPreview.mock.calls[0]).toEqual([
      { query: DEFAULT_LOG_QUERY },
      [{ field: 'MY_GRIDSQUARE', value: null }],
    ])
    fireEvent.click(screen.getByRole('button', { name: 'Add a field' }))
    expect(screen.queryByRole('button', { name: 'Change 40 contacts' })).toBeNull()
  })

  it('driving another station, the change cannot be pressed', async () => {
    setRemoteStation(true)
    api.bulkEditPreview.mockResolvedValue({ selected: 42, change: 40, unchanged: 2, missing: 0 })
    open('bulk')
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'EM10' } })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    const apply = await screen.findByRole('button', { name: 'Change 40 contacts' })
    expect((apply as HTMLButtonElement).disabled).toBe(true)
  })
})

describe('the check', () => {
  const problems: LogProblem[] = [
    { id: 'a', call: 'W1AW', whenUnix: 1_767_268_800, kind: 'grid', field: 'GRIDSQUARE', current: 'FN31PRX', fix: { value: 'FN31PR' } },
    { id: 'b', call: 'K1ABC', whenUnix: 1_767_268_800, kind: 'grid', field: 'GRIDSQUARE', current: 'ZZ', fix: { value: null } },
    { id: 'c', call: 'W3AA', whenUnix: 4_000_000_000, kind: 'future', field: null, current: null, fix: null },
  ]

  it('sends only the fixes ticked, each with what it found, after asking', async () => {
    api.checkLogbook.mockResolvedValue({ checked: 3, problems })
    api.applyLogFixes.mockResolvedValue({ ...made, changed: 1 })
    open('check')
    fireEvent.click(screen.getByRole('button', { name: 'Check the log' }))
    await screen.findByText('3 problems in 3 contacts')
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[]
    // Tick all of the grids, then untick K1ABC; the future date has no fix to tick.
    expect(last(boxes).disabled).toBe(true)
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick all 2 with a fix' }))
    fireEvent.click(boxes[2])
    fireEvent.click(screen.getByRole('button', { name: 'Fix 1 ticked' }))
    expect(api.applyLogFixes).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Yes, fix 1' }))
    await waitFor(() => expect(api.applyLogFixes).toHaveBeenCalledWith([
      { id: 'a', field: 'GRIDSQUARE', expect: 'FN31PRX', value: 'FN31PR' },
    ]))
  })
})
