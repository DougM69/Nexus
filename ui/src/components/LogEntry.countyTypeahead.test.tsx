// @vitest-environment jsdom
//
// ⭐ **THE COUNTY TYPE-AHEAD** — "Cook" reaches the log as COOK.
//
// A QSO-party county exchange is four letters an operator has to know (JODA, SCLA, MCDN)
// and the NAME is the part they actually know. The strip therefore accepts either, offers
// the codes while they type, and — this is the part that matters — LOGS the code, because
// a strip that shows a value good and logs a different one is the screen lying about what
// was written.
//
// Rendered with the props `PhoneCockpit` passes and a `fieldDay` of the shape the DTO
// really produces, the shape `LogEntry.contestStrip.test.tsx` established. jsdom lays
// nothing out; not one assertion here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { subscribeToasts } from '../toast'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** The Illinois QSO Party as the engine serialises it: RST plus ONE QTH box whose value
 *  may come from either of two universes — which is why the slot carries `domains` and
 *  no `domain` (a `oneOf` has no membership verdict; its free-text arm is how the
 *  sponsor says "or a country"). */
const ilqp = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

function renderStrip(fieldDay: FieldDayStatus = ilqp(), fdMode: 'CW' | 'PH' | 'DIG' = 'CW') {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fieldDay}
      fdMode={fdMode}
    />,
  )
}

/** A RECEIVED box by its caption — scoped to `.le-fd-big`, the row of boxes the operator
 *  types the other station's exchange into. The read-only sent display beside it captions
 *  the same slots, so an unscoped query can hand back the wrong input and pass. */
const box = (caption: string): HTMLInputElement => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find(
    (n) => n.textContent === caption,
  )
  if (!cap) throw new Error(`no received box captioned ${caption}`)
  return cap.closest('label')!.querySelector('input')! as HTMLInputElement
}
const qthBox = () => box('QTH')
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const hits = () => [...document.querySelectorAll('.le-fd-suggest .le-fd-hit-code')].map((n) => n.textContent)

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the exchange type-ahead', () => {
  it('offers nothing until something is typed, and nothing for a slot with no domain', () => {
    renderStrip()
    expect(hits()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'C' } })
    expect(hits().length).toBeGreaterThan(0)
    // The RST box draws on no domain at all: typing in it opens no list — and closes the
    // one under the box the operator has left. (`579`, not `599`: the box already holds
    // its default report, and React fires no change for a value that did not change.)
    fireEvent.change(box('RST'), { target: { value: '579' } })
    expect(hits()).toEqual([])
  })

  it('offers county codes for a name and fills the box when one is picked', () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'Cook' } })
    expect(hits()).toEqual(['COOK'])
    fireEvent.mouseDown(document.querySelector('.le-fd-suggest button')!)
    expect(qthBox().value).toBe('COOK')
    expect(hits()).toEqual([]) // the list closes behind the pick
  })

  it('resolves a typed name to its code when SPACE leaves the box', () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'st clair' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('SCLA')
  })

  it('leaves an ambiguous name exactly as typed — it never guesses', () => {
    renderStrip()
    // Macon, Macoupin, Madison, Marion, Marshall, Mason, Massac…
    fireEvent.change(qthBox(), { target: { value: 'Ma' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('MA')
    // …and a country a DX station sends belongs to no universe here, so it survives too.
    fireEvent.change(qthBox(), { target: { value: 'Germany' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value).toBe('GERMANY')
  })

  it('LOGS the code for a name typed straight into the box', async () => {
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    fireEvent.change(qthBox(), { target: { value: 'Kankakee' } })
    // Enter from the box itself — no space, no click, nothing that could have resolved
    // it on the way.
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await vi.waitFor(() => expect(api.contestLogManual).toHaveBeenCalled())
    expect(api.contestLogManual.mock.calls[0][1]).toEqual([
      ['RST', '599'],
      ['QTH', 'KANK'],
    ])
  })

  it('costs no IPC: the whole type-ahead is a lookup on data already in hand', () => {
    renderStrip()
    for (const v of ['C', 'CO', 'COO', 'Cook', 'st clair', 'Ma']) {
      fireEvent.change(qthBox(), { target: { value: v } })
      fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    }
    for (const [name, f] of Object.entries(api)) {
      expect(f, `${name} was called`).not.toHaveBeenCalled()
    }
  })

  it('does not change a contest whose slot has one universe — Field Day still refuses a bad section', () => {
    renderStrip({
      ...ilqp(),
      event: 'arrlfd',
      receives: [
        { key: 'CLASS', kind: 'pattern', required: true },
        { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections', domains: ['fd_sections'] },
      ],
    } as unknown as FieldDayStatus)
    const section = box('Section')
    fireEvent.change(section, { target: { value: 'ZZ' } })
    expect(hits()).toEqual([]) // no section code or name begins ZZ
    // …and the section NAME resolves to its code, which is the same feature reaching a
    // contest that has always had a closed universe.
    fireEvent.change(section, { target: { value: 'Wisconsin' } })
    expect(hits()).toEqual(['WI'])
    fireEvent.keyDown(section, { key: ' ', code: 'Space' })
    expect(section.value).toBe('WI')
  })
})

// ---------------------------------------------------------------------------
// ⭐ THE DUPE BADGE, over a contest whose dupe rule counts two mode classes as ONE.
//
// ILQP works a station "once per band and mode (phone and CW/digital)", so a station
// worked on CW is a dupe on RTTY. The engine refuses it either way; the BADGE is what
// stops the operator calling them first and finding out after the over.
// ---------------------------------------------------------------------------

const worked = (mode: string) => ({
  call: 'K9NR',
  band: '20m',
  mode,
  class: '',
  section: '',
  submode: '',
  whenUnix: 1_792_342_800,
})

describe('the while-typing DUPE badge', () => {
  const dupeShown = () =>
    [...document.querySelectorAll('.le-fd-hint')].some((n) => /Dupe:/.test(n.textContent ?? ''))

  it('warns on the other half of a grouped mode: worked on CW, now on RTTY', () => {
    renderStrip(
      {
        ...ilqp(),
        dupeModeGroups: [['CW', 'DIG']],
        log: [worked('CW')],
      } as unknown as FieldDayStatus,
      'DIG',
    )
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
    // POSITIVE CONTROL: the identical fixture WITHOUT the grouping — the shipped
    // behaviour — leaves the badge off, so the true above is the fold and nothing else.
    cleanup()
    renderStrip({ ...ilqp(), log: [worked('CW')] } as unknown as FieldDayStatus, 'DIG')
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
  })

  it('does NOT warn when the ruleset groups nothing — the shipped behaviour', () => {
    renderStrip({
      ...ilqp(),
      event: 'ohqp',
      log: [worked('DIG')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
    // POSITIVE CONTROL for the query itself: the SAME station on the same mode class
    // still raises the badge, so a false above is the grouping and not a broken check.
    cleanup()
    renderStrip({ ...ilqp(), event: 'ohqp', log: [worked('CW')] } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
  })

  it('warns across the group in the other direction too', () => {
    renderStrip({
      ...ilqp(),
      dupeModeGroups: [['CW', 'DIG']],
      log: [worked('DIG')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(true)
    // …and PHONE is still its own mode: the grouping names two classes, not three.
    cleanup()
    renderStrip({
      ...ilqp(),
      dupeModeGroups: [['CW', 'DIG']],
      log: [worked('PH')],
    } as unknown as FieldDayStatus)
    fireEvent.change(callBox(), { target: { value: 'K9NR' } })
    expect(dupeShown()).toBe(false)
  })
})

// ⭐ "I MOVED" READS A COUNTY THE WAY THE QTH BOX DOES. A mobile knows the county it has just
// driven into by its NAME; the box sent exactly what was typed, so `DuPage` was refused as "not
// one of this contest's counties" while `DUPG` went through. Each box is now read by the QTH
// box's own function, and the engine still decides what is a county.
describe('"I moved" reads a county the way the QTH box does', () => {
  /** Open "I moved", type into its QTH box and press Save. */
  const moveTo = async (typed: string) => {
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    const qth = [...document.querySelectorAll('.le-fd-sent input')].find(
      (i) => i.getAttribute('aria-label') === 'QTH',
    )!
    fireEvent.change(qth, { target: { value: typed } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
  }

  it.each([
    ['a name', 'MONROE', 'MNRO'],
    ['a code', 'DUPG', 'DUPG'],
    ['a mixed-case name', 'DuPage', 'DUPG'],
    ['a name with its punctuation and spacing', 'st. clair', 'SCLA'],
  ])('%s: %s goes to the engine as %s', async (_what, typed, code) => {
    renderStrip()
    await moveTo(typed)
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', code]])
  })

  it("reads New York's own list in the New York QSO Party", async () => {
    renderStrip({
      ...ilqp(),
      event: 'nyqp',
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        { key: 'QTH', kind: 'oneOf', required: true, domains: ['ny_counties', 'ny_mults', 'dx_literal'] },
      ],
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'ALB', domain: 'ny_counties' },
      ],
    } as unknown as FieldDayStatus)
    await moveTo('Monroe')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'MON']])
  })

  /** A QSO party as the engine serialises it for a `role` station sending `raw`, from the list
   *  `domain` names (none for a value typed free, as a DX station's is). */
  const party = (st: 'il' | 'ny', role: string, raw: string, domain?: string): FieldDayStatus =>
    ({
      ...ilqp(),
      event: `${st}qp`,
      role,
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        {
          key: 'QTH',
          kind: 'oneOf',
          required: true,
          domains: st === 'il' ? ['il_counties', 'il_mults'] : ['ny_counties', 'ny_mults', 'dx_literal'],
        },
      ],
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw, ...(domain ? { domain } : {}) },
      ],
    }) as unknown as FieldDayStatus

  // Five counties share their name with a state or province on the same contest's list. The
  // QTH box cannot tell which was meant; I moved can, from the list the station sends from.
  it.each([
    ['il', 'COOK', 'il_counties', 'Washington', 'WASH'],
    ['ny', 'ALB', 'ny_counties', 'Delaware', 'DEL'],
    ['ny', 'ALB', 'ny_counties', 'Ontario', 'ONT'],
    ['ny', 'ALB', 'ny_counties', 'Washington', 'WAS'],
    ['ny', 'ALB', 'ny_counties', 'Wyoming', 'WYO'],
  ])('%sqp in-state, sending %s (%s): %s is the county %s', async (st, sent, list, typed, code) => {
    renderStrip(party(st as 'il' | 'ny', 'in_state', sent, list))
    await moveTo(typed)
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', code]])
  })

  it('outside the state, the same name is the state the station sends', async () => {
    renderStrip(party('il', 'w_ve', 'IN', 'il_mults'))
    await moveTo('Washington')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'WA']])
  })

  it('a value sent from no list (a DX station types its own) is read as the QTH box reads it', async () => {
    renderStrip(party('il', 'dx', 'GERMANY'))
    await moveTo('DuPage')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'DUPG']])
  })

  it("leaves the QTH box's own reading alone: a name two lists hold stays as typed there", () => {
    renderStrip()
    fireEvent.change(qthBox(), { target: { value: 'Washington' } })
    fireEvent.keyDown(qthBox(), { key: ' ', code: 'Space' })
    expect(qthBox().value.toUpperCase()).toBe('WASHINGTON')
  })

  it('a name that is no county goes as typed, and the engine refuses it in its own words', async () => {
    const refusal = '"NARNIA" is not one of this contest\'s counties.'
    api.contestIMoved.mockRejectedValueOnce(refusal)
    let shown: string[] = []
    const off = subscribeToasts((ts) => {
      shown = ts.map((x) => x.message)
    })
    renderStrip()
    await moveTo('Narnia')
    off()
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'NARNIA']])
    expect(shown).toContain(`Could not change what you send: ${refusal}`)
  })

  it('naming the county already being sent moves nothing', async () => {
    renderStrip({
      ...ilqp(),
      composing: [
        { key: 'RST', raw: '599' },
        { key: 'QTH', raw: 'SCLA', domain: 'il_counties' },
      ],
    } as unknown as FieldDayStatus)
    await moveTo('St. Clair')
    expect(api.contestIMoved).not.toHaveBeenCalled()
  })

  it('a slot with no received box keeps what was typed, as before', async () => {
    renderStrip({ ...ilqp(), receives: [{ key: 'RST', kind: 'rst', required: true }] } as unknown as FieldDayStatus)
    await moveTo(' DuPage ')
    expect(api.contestIMoved).toHaveBeenCalledWith([['QTH', 'DUPAGE']])
  })
})
