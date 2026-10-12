// @vitest-environment jsdom
//
// A LOGGED CONTACT'S BOXES END WITH IT, WITH THE CONTEST LOGGER WINDOW OPEN. The main window's strip and
// the logger's show one contact in progress (`features/contestEntryShare`). When it is logged, each
// strip's after-log clear starts the next contact (the county box empty, the report the mode's own), and
// the shared entry still holds the logged contact's boxes until a window puts the next contact's
// (`contest_entry.rs`, `consume`). Whatever order the two land in, the next contact never starts with
// the logged contact's county, in either window or in the shared entry, so a plain Enter can never log
// it again.
//
// The engine is stood in for by `engine` below, as in LogEntry.sharedEntry.test.tsx: a put replaces the
// entry, a log moves it on (the call and the marks go, the boxes stay) and checks the claim as the
// engine does. Each window polls on its own: a poll is a re-render of THAT window with the snapshot the
// engine would hand it, and when it lands is chosen here, by the test. `engine.atLog` is the snapshot a
// window would get if its poll was built at the moment of the log, before any window put again.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act, within } from '@testing-library/react'
import type { ReactElement } from 'react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, ContestEntryShared, FieldDayQso, FieldDayStatus } from '../types'
import type { EntryClaim } from '../api'
import { ENTRY_CHANGED, ENTRY_LOGGED } from '../features/contestEntryShare'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestEntryPut: vi.fn(),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestRemoveLast: vi.fn(() => Promise.resolve({ outcome: 'refused', refusal: 'changed' })),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 0 })),
  getScpCalls: vi.fn(() => Promise.resolve([])),
  getCallHistory: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800

type ModeClass = 'PH' | 'CW'
type Win = 'main' | 'logger'

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const ilqp = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'MCHE', domain: 'il_counties' },
    ],
    composingText: '599 MCHE',
    composingTextPhone: '59 MCHE',
    assistanceOn: [],
  }) as unknown as FieldDayStatus

/** ARRL Field Day, class 2A from Illinois: a row carries the class and section it received. */
const fieldDay = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    ...ilqp(log),
    event: 'arrlfd',
    role: undefined,
    receives: [
      { key: 'CLASS', kind: 'pattern', required: true },
      { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
    ],
    composing: [
      { key: 'CLASS', raw: '2A' },
      { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
    ],
    composingText: '2A IL',
    composingTextPhone: '2A IL',
  }) as unknown as FieldDayStatus

// ── the engine, as the windows see it ──────────────────────────────────────────────────────────

const upper = (o: Record<string, string>) =>
  JSON.stringify(Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k].trim().toUpperCase()])))

const engine = {
  open: true,
  session: ilqp as (log: FieldDayQso[]) => FieldDayStatus,
  entry: { rev: 1, call: '', fields: {}, marks: null } as ContestEntryShared,
  /** The last contact logged from the entry, as its claim showed it, and the rev that log left. */
  logged: null as { call: string; fields: string; at: number } | null,
  log: [] as FieldDayQso[],
  /** What each log wrote, by value: the call, the received vector and the mode class. */
  rows: [] as { call: string; ex: [string, string][]; mode: string }[],
  /** The snapshot a poll built at the moment of the last log, before any window put again. */
  atLog: null as AppSnapshot | null,
  put(call: string, fields: Record<string, string>, marks: unknown): number {
    if (!this.open) throw new Error('contestEntryClosed')
    this.entry = { rev: this.entry.rev + 1, call, fields, marks }
    return this.entry.rev
  },
  /** `Engine::contest_log_entry`: the claim is checked (`admits`), the row written, the entry moved
   *  on (`consume`): the call and the marks go, the boxes stay. */
  write(call: string, ex: [string, string][], mode: string, claim?: EntryClaim): void {
    if (claim && this.open) {
      const shown = [claim.call.trim().toUpperCase(), upper(claim.fields)].join('|')
      if (this.logged && claim.rev < this.logged.at) {
        throw shown === [this.logged.call, this.logged.fields].join('|') ? ENTRY_LOGGED : ENTRY_CHANGED
      }
      const held = [this.entry.call.trim().toUpperCase(), upper(this.entry.fields)].join('|')
      if (claim.rev !== this.entry.rev && held !== shown) throw ENTRY_CHANGED
    }
    const value = (k: string) => ex.find(([key]) => key === k)?.[1] ?? ''
    this.rows.push({ call, ex, mode })
    this.log = [
      ...this.log,
      {
        call,
        class: value('CLASS'),
        section: value('SECTION'),
        band: '40m',
        mode,
        submode: mode === 'PH' ? 'SSB' : '',
        whenUnix: T0 + this.log.length * 60,
        rcvd: ex.map(([, v]) => v),
      } as FieldDayQso,
    ]
    if (claim && this.open) {
      this.entry = { ...this.entry, rev: this.entry.rev + 1, call: '', marks: null }
      this.logged = { call: claim.call.trim().toUpperCase(), fields: upper(claim.fields), at: this.entry.rev }
    }
    this.atLog = snapshot()
  },
}

/** The snapshot a window polls: the session, and the shared entry while the logger is open. */
function snapshot(): AppSnapshot {
  return {
    radio: { band: '40m', dialMhz: 7.04 },
    hunt: null,
    fieldDay: engine.session(engine.log),
    ...(engine.open ? { contestEntry: { ...engine.entry } } : {}),
  } as unknown as AppSnapshot
}

/** A strip as each window mounts it: the cockpits pass their own mode class, and the logger window
 *  passes the one its section maps to (`loggerMode`). */
function strip(snap: AppSnapshot, fdMode: ModeClass, role?: 'logger') {
  return (
    <LogEntry
      snap={snap}
      mode={fdMode === 'PH' ? 'SSB' : 'CW'}
      defaultRst={fdMode === 'PH' ? '59' : '599'}
      exchange="terrestrial"
      titled={false}
      fieldDay={snap.fieldDay}
      fdMode={fdMode}
      sharedEntry={role}
    />
  )
}

/** The main window's cockpit and the logger window, each on the snapshot it last polled. The main
 *  strip is KEYED by its cockpit, so a move from Phone to CW mounts the CW cockpit's strip, as the app
 *  does. */
function Windows({ main, logger, section }: { main: AppSnapshot; logger: AppSnapshot | null; section: ModeClass }) {
  return (
    <>
      <div data-testid="main" key={section}>
        {strip(main, section)}
      </div>
      {logger && <div data-testid="logger">{strip(logger, section, 'logger')}</div>}
    </>
  )
}

let view: { main: AppSnapshot; logger: AppSnapshot | null; section: ModeClass }
let rerender: (ui: ReactElement) => void = () => {}
function open(withLogger = true) {
  const s = snapshot()
  view = { main: s, logger: withLogger ? s : null, section: 'PH' }
  rerender = render(<Windows {...view} />).rerender
}
/** A poll lands in these windows: each re-renders on the snapshot the engine hands it now. */
function poll(...windows: Win[]) {
  const s = snapshot()
  for (const w of windows) view = { ...view, [w]: s }
  rerender(<Windows {...view} />)
}
/** A snapshot built earlier lands in this window now. */
function deliver(w: Win, s: AppSnapshot) {
  view = { ...view, [w]: s }
  rerender(<Windows {...view} />)
}

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
/** A few polls of both windows, so each has taken up whatever the other put. */
async function pollsSettle() {
  for (let i = 0; i < 3; i++) {
    poll('main', 'logger')
    await settle()
  }
}
const inWindow = (w: Win) => within(screen.getByTestId(w))
const callIn = (w: Win) => inWindow(w).getByPlaceholderText('W1AW') as HTMLInputElement
const rstIn = (w: Win) => inWindow(w).getByLabelText('RST') as HTMLInputElement
const qthIn = (w: Win) => inWindow(w).getByLabelText('County / State') as HTMLInputElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const enter = (w: Win) => fireEvent.keyDown(callIn(w), { key: 'Enter' })
const loggedAs = (call: string) => engine.rows.filter((r) => r.call === call).map((r) => r.ex)

/** K9BBB worked from Lake County with a typed 57, shown in both windows. */
async function aContactInProgress(w: Win = 'main') {
  open()
  await settle()
  type(callIn(w), 'K9BBB')
  type(rstIn(w), '57')
  type(qthIn(w), 'LAKE')
  await settle()
  poll('main', 'logger')
  await settle()
  expect(qthIn('logger').value, 'CONTROL: the contact in progress, logger window').toBe('LAKE')
  expect(qthIn('main').value, 'CONTROL: the contact in progress, main window').toBe('LAKE')
}

/** Both windows and the shared entry start the next contact: no county, the mode's own report. */
function expectTheNextContact(when: string) {
  expect(qthIn('main').value, `${when}: the next contact's county, main window`).toBe('')
  expect(qthIn('logger').value, `${when}: the next contact's county, logger window`).toBe('')
  expect(engine.entry.fields.QTH ?? '', `${when}: the next contact's county, shared entry`).toBe('')
  expect(rstIn('main').value, `${when}: the next contact's report, main window`).toBe('59')
  expect(rstIn('logger').value, `${when}: the next contact's report, logger window`).toBe('59')
}

beforeEach(() => {
  engine.open = true
  engine.session = ilqp
  engine.entry = { rev: 1, call: '', fields: {}, marks: null }
  engine.logged = null
  engine.log = []
  engine.rows = []
  engine.atLog = null
  api.contestEntryPut.mockImplementation((call: string, fields: Record<string, string>, marks: unknown) =>
    Promise.resolve(engine.put(call, fields, marks)),
  )
  api.contestLogManual.mockImplementation(
    (call: string, ex: [string, string][], mode: string, _sub: unknown, claim?: EntryClaim) =>
      new Promise((ok, refuse) => {
        try {
          engine.write(call, ex, mode, claim)
          ok({})
        } catch (e) {
          refuse(e)
        }
      }),
  )
})

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('a logged contact’s county never carries into the next contact, with the logger window open', () => {
  it('the snapshot carrying the contact lands in the same render as the main window’s own clear', async () => {
    await aContactInProgress()
    enter('main')
    // The log answers, and the poll carrying the contact lands in that same render, in both windows.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    await pollsSettle()
    expectTheNextContact('after the polls')
    // The logger starts the next call; a plain Enter in the main window must not log Lake for it.
    type(callIn('logger'), 'K9CCC')
    await settle()
    await pollsSettle()
    expect(callIn('main').value, 'CONTROL: the next call reached the main window').toBe('K9CCC')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9CCC'), 'a plain Enter on the next call').toEqual([])
  })

  it('the same, with Enter in the logger window', async () => {
    await aContactInProgress('logger')
    enter('logger')
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('the logger window polls once the main window has put its own clear, before the main window polls', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    poll('logger')
    await settle()
    expect(qthIn('logger').value, 'the logger window, at its first snapshot after the contact').toBe('')
    expect(rstIn('logger').value, 'the logger window’s report, then').toBe('59')
    // The logger operator starts the next call before the main window's next poll.
    type(callIn('logger'), 'K9DDD')
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
    expect(callIn('main').value, 'CONTROL: the next call reached the main window').toBe('K9DDD')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9DDD'), 'a plain Enter on the next call').toEqual([])
  })

  it('the logger window’s poll was built at the moment of the log, and lands after the main window’s own put', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    const atLog = engine.atLog!
    expect(atLog.contestEntry?.fields.QTH, 'CONTROL: the entry the engine kept at the log').toBe('LAKE')
    deliver('logger', atLog)
    await settle()
    expect(qthIn('logger').value, 'the logger window, on the snapshot of the moment of the log').toBe('')
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('the operator moves to the CW cockpit as the contact is logged: its new strip starts the next contact', async () => {
    await aContactInProgress()
    enter('main')
    // The Phone cockpit's strip goes with the log in flight, and the CW cockpit's mounts on the poll
    // carrying the contact; the logger window follows the section on the same poll.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0))
      view = { ...view, section: 'CW' }
      poll('main', 'logger')
    })
    expect(loggedAs('K9BBB'), 'logged as typed').toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    expect(qthIn('main').value, 'the CW cockpit’s strip, as it mounts').toBe('')
    await pollsSettle()
    expect(qthIn('main').value, 'the CW cockpit’s strip').toBe('')
    expect(qthIn('logger').value, 'the logger window, on CW').toBe('')
    expect(engine.entry.fields.QTH ?? '', 'the shared entry').toBe('')
    expect(rstIn('main').value, 'the CW report').toBe('599')
  })

  it('CONTROL: a county typed in the other window for the NEXT contact, landing with this window’s clear, is kept', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // The logger window sees the contact first; its operator types the next station's call and county.
    poll('logger')
    await settle()
    type(callIn('logger'), 'K9DDD')
    type(qthIn('logger'), 'COOK')
    await settle()
    // The main window's first snapshot after the contact carries them.
    poll('main')
    await settle()
    expect(callIn('main').value, 'the next call, main window').toBe('K9DDD')
    expect(qthIn('main').value, 'the next county, main window').toBe('COOK')
    await pollsSettle()
    expect(qthIn('main').value, 'the next county, main window, after the polls').toBe('COOK')
    expect(qthIn('logger').value, 'the next county, logger window').toBe('COOK')
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9DDD'), 'the next contact, as typed for it').toEqual([[['RST', '59'], ['QTH', 'COOK']]])
  })

  it('CONTROL: two people at one moment, Esc in the logger window as the main window logs: the county ends in both once the logger window sees the contact', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    // The logger window has not polled since the contact went in. Its Esc clears the line it still
    // shows and puts it; Clear keeps the boxes, so the logged contact's county goes with it.
    fireEvent.keyDown(window, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(callIn('logger').value, 'CONTROL: the logger window cleared its line').toBe('')
    expect(engine.entry.fields.QTH, 'CONTROL: what the logger window put').toBe('LAKE')
    // The main window takes that up (the engine's last writer wins), then the logger window sees
    // the contact, and its own after-log clear ends the county in both.
    poll('main')
    await settle()
    poll('logger')
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('CONTROL: a contact with no race clears as before, in both windows', async () => {
    await aContactInProgress()
    enter('main')
    await settle()
    await settle()
    await pollsSettle()
    expectTheNextContact('after the polls')
  })

  it('CONTROL: typed in the logger window while the main window is idle, it shows in both', async () => {
    open()
    await settle()
    type(callIn('logger'), 'K9EEE')
    type(qthIn('logger'), 'DUPA')
    await settle()
    poll('main')
    await settle()
    expect(callIn('main').value).toBe('K9EEE')
    expect(qthIn('main').value).toBe('DUPA')
  })

  it('CONTROL: with the logger window closed, a contact clears as before', async () => {
    engine.open = false
    open(false)
    await settle()
    type(callIn('main'), 'K9BBB')
    type(rstIn('main'), '57')
    type(qthIn('main'), 'LAKE')
    enter('main')
    await settle()
    poll('main')
    await settle()
    expect(loggedAs('K9BBB')).toEqual([[['RST', '57'], ['QTH', 'LAKE']]])
    expect(callIn('main').value, 'the call').toBe('')
    expect(qthIn('main').value, 'the county').toBe('')
    expect(rstIn('main').value, 'the report').toBe('59')
    expect(api.contestEntryPut, 'nothing shared').not.toHaveBeenCalled()
  })

  it('CONTROL: Field Day keeps the class and section it logged for the next contact, in both windows', async () => {
    engine.session = fieldDay
    open()
    await settle()
    const box = (w: Win, i: number) => inWindow(w).getAllByRole('textbox')[i] as HTMLInputElement
    type(callIn('main'), 'K9FFF')
    type(box('main', 1), '3A')
    type(box('main', 2), 'WI')
    await settle()
    poll('main', 'logger')
    await settle()
    enter('main')
    await settle()
    await settle()
    expect(loggedAs('K9FFF'), 'logged').toEqual([[['CLASS', '3A'], ['SECTION', 'WI']]])
    // As the log lands, before any snapshot shows it: the class and section stay.
    expect(callIn('main').value, 'the call, as the log lands').toBe('')
    expect(box('main', 1).value, 'the class, as the log lands').toBe('3A')
    expect(box('main', 2).value, 'the section, as the log lands').toBe('WI')
    expect(engine.entry.fields, 'what the main window put').toMatchObject({ CLASS: '3A', SECTION: 'WI' })
    // The logger window's snapshot, built at the moment of the log: the entry the log left.
    deliver('logger', engine.atLog!)
    await settle()
    await pollsSettle()
    for (const w of ['main', 'logger'] as const) {
      expect(callIn(w).value, `${w}: the call`).toBe('')
      expect(box(w, 1).value, `${w}: the class carried`).toBe('3A')
      expect(box(w, 2).value, `${w}: the section carried`).toBe('WI')
    }
  })
})
