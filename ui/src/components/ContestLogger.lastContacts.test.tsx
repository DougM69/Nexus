// @vitest-environment jsdom
//
// THE LAST CONTACTS UNDER THE CONTEST ENTRY — the newest ten, newest first, and each one correctable
// where it stands: its Edit opens the contest screen's own edit (`ContestContactEdit`, the engine's
// `contest_edit`) in the row's place, with the same refusals (a club, a press through Remote) and the
// same line once it is over, the "edit it in the logbook too" warning included.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, fireEvent, cleanup, act, screen } from '@testing-library/react'
import { createRef } from 'react'
import { LAST_CONTACTS, LastContacts } from './ContestLogger'
import type { FdClubStatus, FieldDayQso, FieldDayStatus } from '../types'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import { t } from '../i18n'
import { contactLabel } from '../features/contestRemoval'

vi.mock('../api', () => ({
  contestEdit: vi.fn(),
  resolveEntity: vi.fn(async () => null),
  contestLogManual: vi.fn(),
  fdLogManual: vi.fn(),
  logQso: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, zone: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'PH', submode: 'USB', whenUnix, rcvd: ['59', zone] }) as FieldDayQso

const cqww = (log: FieldDayQso[], club?: FdClubStatus): FieldDayStatus =>
  ({
    running: true,
    state: 'run',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'cqww_ssb',
    rulesYear: 2026,
    role: '',
    bands: ['80m', '40m', '20m', '15m', '10m'],
    receives: [
      { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
      { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
    ],
    composing: [
      { key: 'RST', raw: '59' },
      { key: 'ZN', raw: '4' },
    ],
    club,
  }) as unknown as FieldDayStatus

/** Twelve contacts, oldest first, a minute apart: K00AA … K11AA. */
const LOG = Array.from({ length: 12 }, (_, i) => row(`K${String(i).padStart(2, '0')}AA`, '5', T0 + i * 60))

const CLUB = { syncState: 'synced', queued: 0, hosting: false, dupes: [], board: [] } as unknown as FdClubStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

function show(fieldDay: FieldDayStatus) {
  return render(<LastContacts fieldDay={fieldDay} style={{}} boxRef={createRef<HTMLElement>()} />)
}

const calls = () => [...document.querySelectorAll('.contest-logger-recent .fd-log-row .fd-col.call')].map((c) => c.textContent)
const editButtons = () => [...document.querySelectorAll<HTMLButtonElement>('.contest-logger-recent .fd-edit')]
const editor = () => document.querySelector<HTMLElement>('.contest-logger-recent .contest-edit')
const boxes = () => [...(editor()?.querySelectorAll<HTMLInputElement>('input') ?? [])]
const note = () => document.querySelector('.contest-logger-recent-note')?.textContent ?? null
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the last contacts list the newest ten', () => {
  it('shows the ten newest, newest first, of a longer log', () => {
    expect(LAST_CONTACTS).toBe(10)
    show(cqww(LOG))
    expect(calls()).toEqual(LOG.slice(-10).reverse().map((q) => q.call))
  })
})

describe('a last contact is corrected in place', () => {
  it('every row has an Edit named for its contact, and it opens that contact where it stands', () => {
    show(cqww(LOG))
    expect(editButtons()).toHaveLength(10)
    const third = screen.getByRole('button', { name: t('contest.edit.aria', { contact: contactLabel([LOG[9]]) }) })
    expect(third.getAttribute('aria-label')).toMatch(/^Edit K09AA · 20m USB · /)
    fireEvent.click(third)
    expect(editor()).not.toBeNull()
    expect(boxes().map((b) => b.value)).toEqual(['K09AA', '59', '5'])
    expect(document.activeElement).toBe(boxes()[0])
    expect(calls(), 'the row it edits is the edit now').not.toContain('K09AA')
    expect(calls()).toHaveLength(9)
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('Enter sends the edit through the engine, logs nothing, and says what came of it', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '14', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'k11ab')
    type(boxes()[2], '14')
    await act(async () => {
      fireEvent.keyDown(boxes()[2], { key: 'Enter' })
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('K11AA', LOG[11].whenUnix, {
      call: 'K11AB',
      rows: [[['RST', '59'], ['ZN', '14']]],
      band: '20m',
      // Unchanged, as the contest screen's edit sends it.
      mode: '',
    })
    expect(api.contestLogManual).not.toHaveBeenCalled()
    expect(api.logQso).not.toHaveBeenCalled()
    expect(editor()).toBeNull()
    expect(note()).toMatch(/^Edited K11AA: now K11AB/)
  })

  it('after the merge, says the logbook copy still has the old call', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '5', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: true, uploaded: [] },
    })
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'K11AB')
    await act(async () => {
      fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
    })
    await settle()
    expect(note()).toContain(t('contest.edit.logbook', { call: 'K11AA' }))
  })

  it('Esc leaves it as it was and sends nothing', () => {
    show(cqww(LOG))
    fireEvent.click(editButtons()[1])
    type(boxes()[0], 'N0NE')
    fireEvent.keyDown(boxes()[0], { key: 'Escape' })
    expect(editor()).toBeNull()
    expect(calls()).toEqual(LOG.slice(-10).reverse().map((q) => q.call))
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('closes when its contact leaves the log meanwhile', async () => {
    const view = show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
    view.rerender(<LastContacts fieldDay={cqww(LOG.slice(0, 11))} style={{}} boxRef={createRef<HTMLElement>()} />)
    await settle()
    expect(editor()).toBeNull()
  })
})

describe('the same refusals as the contest screen', () => {
  it('at a club says how a contact is fixed there and opens no edit; CONTROL: alone it opens', () => {
    show(cqww(LOG, CLUB))
    fireEvent.click(editButtons()[0])
    expect(editor()).toBeNull()
    expect(note()).toBe(t('contest.edit.club'))
    cleanup()
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
  })

  describe('a press through the Remote stream edits nothing', () => {
    let under: Element | null = null
    let stream: StreamInputDispatcher | null = null
    beforeEach(() => {
      under = null
      Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
      stream = new StreamInputDispatcher(window)
    })
    afterEach(() => {
      stream?.dispose()
      stream = null
      delete (document as { elementFromPoint?: unknown }).elementFromPoint
    })
    const pointer = (action: 'down' | 'up') => ({
      type: 'pointer',
      action,
      x: 0.5,
      y: 0.5,
      button: 0,
      buttons: action === 'down' ? 1 : 0,
      modifiers: 0,
      pointerType: 'mouse',
      clicks: 1,
    })

    it('refuses Edit clicked through the stream, and says why; CONTROL: at the station it opens', async () => {
      show(cqww(LOG))
      under = editButtons()[0]
      await act(async () => {
        stream!.handle(pointer('down'))
        stream!.handle(pointer('up'))
      })
      expect(editor()).toBeNull()
      expect(note()).toBe(t('contest.edit.remote'))
      fireEvent.click(editButtons()[0])
      expect(editor()).not.toBeNull()
    })
  })

  it('a satellite contact keeps its band in the edit', () => {
    show(cqww([{ ...row('K00AA', '5', T0), sat: 'SO-50' } as FieldDayQso]))
    fireEvent.click(editButtons()[0])
    const [band] = [...editor()!.querySelectorAll<HTMLSelectElement>('select')]
    expect(band.disabled).toBe(true)
  })
})
