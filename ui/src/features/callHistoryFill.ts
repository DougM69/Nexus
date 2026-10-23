// Call history fill — what the operator's imported call-history file may put in the contest
// strip's exchange boxes, and how a fill comes back out. Pure: no React, no IO.
//
// The operator's ruling, and each rule here is a line of it:
//   · the file fills the boxes as the call is typed, and the operator corrects what is wrong;
//   · a filled value is one of the sponsor's own codes, the value the strip would accept typed —
//     a county, state, section or zone the contest's own list holds;
//   · it is marked as coming from call history (the strip's job, from `FillState.filled`);
//   · it never overwrites what the operator typed;
//   · nothing is guessed: a value the strip would refuse is not filled, and neither is anything
//     in a slot the strip has no list for. A Field Day class or a Sweepstakes check is a pattern
//     the strip cannot check, a report is what you hear, a serial is this contest's, so those
//     boxes are never filled at all.
//
// A fill belongs to the call it was made for. When the call in the box changes, a fill the
// operator has not touched goes back to what the box held before it.

import type { CallHistoryFile, ContestFieldSpec, FieldDayQso } from '../types'
import { contestDomain, resolveDomainValue } from './contestDomains'

/** The N1MM columns that can feed a slot, most specific first: by what the slot MEANS (its ADIF
 *  tag) and then by its id. Loc1 is where N1MM's QSO-party files carry the county; State is
 *  where they carry the state or province of a station outside it. */
function columnsFor(f: ContestFieldSpec): readonly string[] {
  if (f.adif === 'ARRL_SECT' || f.key === 'SECTION' || f.key === 'SEC') return ['Sect']
  if (f.adif === 'CQZ' || f.key === 'ZN') return ['CqZone']
  if (f.key === 'QTH') return ['Loc1', 'State']
  return []
}

/** Is `code` a member of a domain this build holds the values of? Unlike `inDomain`, a domain
 *  the UI has no list for is NOT a yes: the strip cannot check it, so nothing goes in. */
function held(domainId: string, code: string): boolean {
  return contestDomain(domainId)?.codes.has(code) ?? false
}

/** The value a history entry means in this slot, if the strip would accept it typed: a name
 *  becomes its code ("Cook" → COOK) through the strip's own resolution, and the code must be in
 *  the slot's list. `undefined` for anything else, including every slot with no list. */
export function acceptedCode(f: ContestFieldSpec, raw: string): string | undefined {
  const v = raw.trim().toUpperCase()
  if (v === '') return undefined
  if (f.kind === 'enum' || f.kind === 'oneOf') {
    const code = resolveDomainValue(f.domains, v) ?? v
    const lists = f.kind === 'enum' ? (f.domain ? [f.domain] : []) : (f.domains ?? [])
    return lists.some((d) => held(d, code)) ? code : undefined
  }
  if (f.kind === 'number' && f.min != null && f.max != null) {
    return /^\d{1,3}$/.test(v) && Number(v) >= f.min && Number(v) <= f.max ? v : undefined
  }
  return undefined
}

/** A mobile or a rover: where it is now is not where it was last time. */
const MOVES = /\/(M|R)$/

/** What the file offers `call`, by slot: only the codes the strip accepts. Empty for a file
 *  bound to another contest, a moving station, or a call the file does not hold. */
export function historyFill(
  call: string,
  slots: readonly ContestFieldSpec[],
  file: CallHistoryFile | null,
  contest: string | undefined,
): Record<string, string> {
  const c = call.trim().toUpperCase()
  if (!file || file.contest !== contest || c === '' || MOVES.test(c)) return {}
  const row = file.entries[c]
  if (!row) return {}
  const out: Record<string, string> = {}
  for (const f of slots) {
    for (const col of columnsFor(f)) {
      const code = row[col] === undefined ? undefined : acceptedCode(f, row[col])
      if (code !== undefined) {
        out[f.key] = code
        break
      }
    }
  }
  return out
}

/** What this contest's own log offers `call`: the last contact with it here, slot by slot, for
 *  the same slots and under the same test as the file (a code the strip would accept typed). A
 *  row carries what it received in `rcvd`, in `slots` order; a Field Day row, which has none,
 *  carries its section. Empty for a moving station, as for the file. */
export function contestLogFill(
  call: string,
  slots: readonly ContestFieldSpec[],
  log: readonly FieldDayQso[],
): Record<string, string> {
  const c = call.trim().toUpperCase()
  if (c === '' || MOVES.test(c)) return {}
  let row: FieldDayQso | undefined
  for (let i = log.length - 1; i >= 0 && !row; i--) if (log[i].call.trim().toUpperCase() === c) row = log[i]
  if (!row) return {}
  const { rcvd, section } = row
  const out: Record<string, string> = {}
  slots.forEach((f, i) => {
    if (columnsFor(f).length === 0) return
    const raw = rcvd ? rcvd[i] : f.key === 'SECTION' ? section : undefined
    const code = raw === undefined ? undefined : acceptedCode(f, raw)
    if (code !== undefined) out[f.key] = code
  })
  return out
}

/** What the whole log offers `call` (the station's `contest_logged_qth` answer, asked for the
 *  call typed): its county or state, in the QTH box only, if the box would accept it typed. */
export function loggedQthFill(
  call: string,
  slots: readonly ContestFieldSpec[],
  logged: { call: string; value: string } | null,
): Record<string, string> {
  const c = call.trim().toUpperCase()
  if (logged === null || logged.call !== c || MOVES.test(c)) return {}
  const f = slots.find((s) => s.key === 'QTH')
  const code = f ? acceptedCode(f, logged.value) : undefined
  return f && code !== undefined ? { [f.key]: code } : {}
}

/** Where a fill came from, which the box's caption names: this contest's last contact, the
 *  call-history file, or the log (with the year of the contact). */
export type FillSource = 'contest' | 'history' | `log:${number}`

/** ⭐ THE ORDER OF THE SOURCES (the operator's ruling): this contest's last contact with the
 *  station, then the call-history file, then the most recent contact anywhere in the log. Per box:
 *  a source that offers nothing for a box leaves it to the next. */
export function offerFill(
  fromContest: Readonly<Record<string, string>>,
  fromFile: Readonly<Record<string, string>>,
  fromLog: Readonly<Record<string, string>>,
  logYear: number,
): { offered: Record<string, string>; sources: Record<string, FillSource> } {
  const offered: Record<string, string> = {}
  const sources: Record<string, FillSource> = {}
  const take = (from: Readonly<Record<string, string>>, source: FillSource) => {
    for (const [key, value] of Object.entries(from)) {
      if (key in offered) continue
      offered[key] = value
      sources[key] = source
    }
  }
  take(fromContest, 'contest')
  take(fromFile, 'history')
  take(fromLog, `log:${logYear}`)
  return { offered, sources }
}

/** What the fill has done to the boxes since the strip last cleared. */
export interface FillState {
  /** The boxes the operator typed in (or grabbed a value into) since the strip last cleared. */
  typed: Readonly<Record<string, true>>
  /** The boxes holding a filled value: for which call, the value, what was there, and where it
   *  came from (absent: the call-history file, which was the only source once). */
  filled: Readonly<Record<string, { call: string; value: string; before: string; source?: FillSource }>>
}

export const EMPTY_FILL: FillState = { typed: {}, filled: {} }

/** The operator typed in `key`: the box is theirs, and nothing fills it until the strip clears. */
export function typedIn(s: FillState, key: string): FillState {
  if (s.typed[key] && !s.filled[key]) return s
  const filled = { ...s.filled }
  delete filled[key]
  return { typed: { ...s.typed, [key]: true }, filled }
}

/** Bring the boxes in line with what the file offers the call now in the Call box. `values` are
 *  the boxes as they stand. Returns the boxes to write and the new state (`s` itself when nothing
 *  moved). */
export function applyFill(
  s: FillState,
  call: string,
  offered: Readonly<Record<string, string>>,
  values: Readonly<Record<string, string>>,
  sources: Readonly<Record<string, FillSource>> = {},
): { writes: Record<string, string>; state: FillState } {
  const c = call.trim().toUpperCase()
  const filled = { ...s.filled }
  const writes: Record<string, string> = {}
  let moved = false
  // Out: a fill that is no longer what the file offers THIS call. A box still showing it goes
  // back to what it held; one that shows something else has been written since, and keeps that.
  for (const [key, fill] of Object.entries(s.filled)) {
    if (fill.call === c && offered[key] === fill.value && sources[key] === fill.source) continue
    delete filled[key]
    moved = true
    if ((values[key] ?? '') === fill.value) writes[key] = fill.before
  }
  // In: what the file offers, wherever the operator has not typed.
  for (const [key, value] of Object.entries(offered)) {
    if (s.typed[key] || filled[key]) continue
    const before = writes[key] ?? values[key] ?? ''
    filled[key] = sources[key] ? { call: c, value, before, source: sources[key] } : { call: c, value, before }
    writes[key] = value
    moved = true
  }
  return moved ? { writes, state: { typed: s.typed, filled } } : { writes: {}, state: s }
}
