// @vitest-environment jsdom
//
// ⭐ CTRL+Y IN THE CONTEST STRIP — N1MM's edit-last: it opens the newest contact for edit in place
// of the entry boxes. Rendered with the props the cockpits give the strip and a live Enter Sends
// Message host, because the danger is a key that SENDS: Enter in the edit saves the edit and
// nothing else, with ESM on. The key is a WINDOW key, fired at `window` as the browser delivers it.
//
//   - Ctrl+Y alone is the key (never with Alt, Shift or Cmd), a held key is one press, and the
//     browser's own Ctrl+Y (redo) does not run;
//   - Enter saves the edit with the identity shown; Esc leaves it, and the boxes come back as
//     they were, the caret in Call;
//   - at a club the line says how a contact is fixed there, and no edit opens;
//   - never from afar, and never through the Remote stream.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'
import { useEsmHost, type EsmMessage } from '../features/esmHost'
import { CONTEST_LAYOUT_ROLES, CW_CONTEST_LAYOUT } from '../features/esmRoles'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import { MOD_CTRL } from '../remote-web/stream-protocol'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(),
  contestEdit: vi.fn(),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
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

/** 2026-10-18 17:00Z and a minute later. */
const T0 = 1_792_342_800

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

/** An Illinois QSO Party session as the engine serialises it. */
const party = (log: FieldDayQso[], over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
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
    ...over,
  }) as unknown as FieldDayStatus

const LOG = [row('K9AAA', 'COOK', T0), row('W9BBB', 'LAKE', T0 + 60)]

/** What Enter Sends Message handed to its send path. */
const sent: EsmMessage[] = []

function Strip({
  fieldDay,
  esmOn = false,
  remote = false,
  logger = false,
}: {
  fieldDay: FieldDayStatus
  esmOn?: boolean
  remote?: boolean
  logger?: boolean
}) {
  const esm = useEsmHost({
    cockpit: 'cw',
    on: esmOn,
    callOnce: false,
    roles: CONTEST_LAYOUT_ROLES,
    slots: CW_CONTEST_LAYOUT,
    guards: () => ({ cockpit: 'cw', txEnabled: true, txAllowed: true, clockRepair: false }),
    send: (m) => {
      sent.push(m)
      return Promise.resolve(null)
    },
  })
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={{ radio: { band: '20m', dialMhz: 14.025, txEnabled: true, txAllowed: true }, hunt: null } as unknown as AppSnapshot}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      fieldDay={fieldDay}
      fdMode="CW"
      esm={esm.host}
      active
      sharedEntry={logger ? 'logger' : undefined}
      remote={
        remote
          ? { submit: vi.fn(), canSubmit: true, busy: false, pending: false, resetKey: 0, recall: () => null }
          : undefined
      }
    />
  )
}

/** Ctrl+Y at the window, as the browser delivers it; answers whether the default was cancelled. */
const ctrlY = (extra: Partial<KeyboardEventInit> = {}) =>
  !fireEvent.keyDown(window, { key: 'y', code: 'KeyY', ctrlKey: true, ...extra })
/** Alt+W at the window (`features/contestKeys.ts`); answers whether the default was cancelled. */
const altW = () => !fireEvent.keyDown(window, { key: 'w', code: 'KeyW', altKey: true })

const line = () => document.querySelector('.le-fd-remove-line')?.textContent ?? null
const editor = () => document.querySelector<HTMLElement>('.contest-edit')
const editBoxes = () => [...(editor()?.querySelectorAll<HTMLInputElement>('input') ?? [])]
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(() => {
  cleanup()
  sent.length = 0
  for (const f of Object.values(api)) f.mockClear()
})

describe('Ctrl+Y opens the newest contact for edit', () => {
  it('in place of the entry boxes, the caret in its call, and changes nothing yet', () => {
    render(<Strip fieldDay={party(LOG)} />)
    expect(ctrlY(), 'Ctrl+Y is the strip’s: the browser’s own is cancelled').toBe(true)
    expect(editBoxes().map((b) => b.value)).toEqual(['W9BBB', '599', 'LAKE'])
    expect(document.querySelector('.le-fd-big'), 'the entry boxes give it their place').toBeNull()
    expect(document.activeElement).toBe(editBoxes()[0])
    expect(line()).toBe('Editing W9BBB · 20m CW · 17:01 · 599 LAKE: Enter saves, Esc leaves it as it was.')
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('Enter saves the edit with the identity shown; the boxes come back as they were, and the line says what changed', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[1]],
      after: [row('W9BBC', 'LAKE', T0 + 60)],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    render(<Strip fieldDay={party(LOG)} />)
    type(callBox(), 'N9XYZ')
    ctrlY()
    type(editBoxes()[0], 'W9BBC')
    await act(async () => {
      fireEvent.keyDown(editBoxes()[0], { key: 'Enter' })
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('W9BBB', T0 + 60, {
      call: 'W9BBC',
      rows: [[['RST', '599'], ['QTH', 'LAKE']]],
      band: '20m',
      mode: '',
    })
    expect(editor()).toBeNull()
    expect(callBox().value, 'what was typed in the strip is still there').toBe('N9XYZ')
    expect(line()).toBe('Edited W9BBB: now W9BBC · 20m CW · 17:01 · 599 LAKE.')
  })

  it('with Enter Sends Message on, Enter in the edit sends nothing; CONTROL: Enter in Call sends', async () => {
    api.contestEdit.mockResolvedValue({ outcome: 'refused', refusal: 'changed' })
    render(<Strip fieldDay={party(LOG)} esmOn />)
    // Enter in every box and list of the edit, the edit opened again after each answer.
    for (let i = 0; i < 5; i++) {
      if (!editor()) ctrlY()
      const field = editor()!.querySelectorAll<HTMLElement>('input, select')[i]
      await act(async () => {
        fireEvent.keyDown(field, { key: 'Enter' })
      })
      await settle()
    }
    expect(sent, 'nothing goes on the air from an edit').toEqual([])
    expect(api.contestEdit).toHaveBeenCalledTimes(5)
    type(callBox(), 'K9ABC')
    await act(async () => {
      fireEvent.keyDown(callBox(), { key: 'Enter' })
    })
    expect(sent.map((m) => m.role), 'the control: Enter in Call is ESM’s').toEqual(['myCall'])
  })

  it('Esc leaves it as it was, sends nothing, and the caret goes back to Call', async () => {
    render(<Strip fieldDay={party(LOG)} />)
    ctrlY()
    type(editBoxes()[0], 'N0NE')
    fireEvent.keyDown(editBoxes()[0], { key: 'Escape' })
    expect(editor()).toBeNull()
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
    expect(document.activeElement).toBe(callBox())
    expect(api.contestEdit).not.toHaveBeenCalled()
    expect(line()).toBeNull()
  })

  it('Alt+W in the edit wipes nothing: the boxes come back as they were typed; CONTROL: with the edit closed it wipes them', () => {
    render(<Strip fieldDay={party(LOG)} />)
    type(callBox(), 'N9XYZ')
    ctrlY()
    expect(altW(), 'still the strip’s key: Option+W types nothing into the edit').toBe(true)
    expect(editBoxes().map((b) => b.value), 'the edit is as it was').toEqual(['W9BBB', '599', 'LAKE'])
    fireEvent.keyDown(editBoxes()[0], { key: 'Escape' })
    expect(editor()).toBeNull()
    expect(callBox().value, 'the line waited for the edit to close').toBe('N9XYZ')
    altW()
    expect(callBox().value, 'CONTROL: on the line, Alt+W wipes it').toBe('')
  })

  it('is Ctrl alone: with Alt, Shift or Cmd it does nothing, and held down it is one press', () => {
    render(<Strip fieldDay={party(LOG)} />)
    for (const extra of [{ altKey: true }, { shiftKey: true }, { metaKey: true }]) {
      expect(ctrlY(extra)).toBe(false)
      expect(editor()).toBeNull()
    }
    ctrlY()
    type(editBoxes()[0], 'W9BBX')
    ctrlY({ repeat: true })
    expect(editBoxes()[0].value, 'auto-repeat opened nothing again').toBe('W9BBX')
  })

  it('closes when its contact leaves the log meanwhile, and the boxes come back', () => {
    const { rerender } = render(<Strip fieldDay={party(LOG)} />)
    ctrlY()
    expect(editor()).not.toBeNull()
    rerender(<Strip fieldDay={party([LOG[0]])} />)
    expect(editor()).toBeNull()
    expect(document.querySelector('.le-fd-big')).not.toBeNull()
    rerender(<Strip fieldDay={party(LOG)} />)
    expect(editor(), 'restored meanwhile, it does not open again by itself').toBeNull()
  })

  it('Ctrl+Y again, after a newer contact is logged, opens that one afresh', () => {
    const { rerender } = render(<Strip fieldDay={party(LOG)} />)
    ctrlY()
    type(editBoxes()[0], 'W9BBX')
    rerender(<Strip fieldDay={party([...LOG, row('N9NEW', 'WILL', T0 + 120)])} />)
    ctrlY()
    expect(editBoxes().map((b) => b.value)).toEqual(['N9NEW', '599', 'WILL'])
  })

  it('in the logger window, Esc in the edit leaves the edit, and the logger’s entry as it was', () => {
    render(<Strip fieldDay={party(LOG)} logger />)
    type(callBox(), 'N9XYZ')
    ctrlY()
    fireEvent.keyDown(editBoxes()[0], { key: 'Escape', code: 'Escape' })
    expect(editor()).toBeNull()
    expect(callBox().value).toBe('N9XYZ')
    // CONTROL: Esc outside the edit is the logger's, and clears its entry.
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    expect(callBox().value).toBe('')
  })

  it('says there is nothing to edit in an empty log', () => {
    render(<Strip fieldDay={party([])} />)
    ctrlY()
    expect(editor()).toBeNull()
    expect(line()).toBe('There is no contest contact to edit.')
  })
})

describe('at a club, and from afar', () => {
  it('at a club the line says how a contact is fixed there, and no edit opens', () => {
    render(<Strip fieldDay={party(LOG, { club: { syncState: 'synced', hosting: false, dupes: [], board: [] } } as never)} />)
    ctrlY()
    expect(editor()).toBeNull()
    expect(line()).toMatch(/^At a club an edit cannot reach the other laptops yet, so nothing was edited\./)
  })

  it('not on the hosted Remote page or in the native client: Ctrl+Y is not the strip’s there', () => {
    render(<Strip fieldDay={party(LOG)} remote />)
    expect(ctrlY()).toBe(false)
    expect(editor()).toBeNull()
  })

  describe('through the Remote stream', () => {
    let stream: StreamInputDispatcher | null = null
    beforeEach(() => {
      stream = new StreamInputDispatcher(window)
    })
    afterEach(() => {
      stream?.dispose()
      stream = null
    })

    it('refuses Ctrl+Y, and says why; CONTROL: at the station it opens', async () => {
      render(<Strip fieldDay={party(LOG)} />)
      await act(async () => {
        stream!.handle({ type: 'key', action: 'down', key: 'y', code: 'KeyY', modifiers: MOD_CTRL, repeat: false })
        stream!.handle({ type: 'key', action: 'up', key: 'y', code: 'KeyY', modifiers: MOD_CTRL, repeat: false })
      })
      expect(editor()).toBeNull()
      expect(line()).toBe('Only at the station: a press through Remote edits no contact.')
      ctrlY()
      expect(editor()).not.toBeNull()
    })
  })
})
