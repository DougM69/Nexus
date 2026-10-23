// ---------------------------------------------------------------------------
// PER-OPERATOR CONTEST STATISTICS — the shapes the Contest summary, its exported score summary and
// the Operators box draw from `FieldDayStatus.operators`.
//
// Every figure is the station's (`FieldDayLog::operator_stats` in tempo-core): the contacts that count
// under the contest's own rules, grouped by the operator stamped on each, with the QSO points and the
// multipliers the contest's own scorer gives them. Nothing here scores; it only divides and lays out.
//
// The operator '' is the contacts logged with no operator named. They are their own row, shown as
// '—', never folded into the station's call: a club call is not a person, and a row nobody chose to
// name is not one somebody did.
// ---------------------------------------------------------------------------

import type { FdOperatorStat } from '../types'

/** The label of the contacts that name nobody. */
export const UNNAMED_OPERATOR = '—'

/** Canonical band order (HF → VHF), so a band list reads top-down like a rig. */
const BAND_ORDER = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm']
export function bandRank(b: string): number {
  const i = BAND_ORDER.indexOf(b)
  return i === -1 ? BAND_ORDER.length : i
}

export function operatorLabel(op: string): string {
  return op === '' ? UNNAMED_OPERATOR : op
}

/** A whole-number percentage of `whole`, or null when there is nothing to divide. */
export function share(part: number, whole: number): number | null {
  return whole > 0 ? Math.round((part * 100) / whole) : null
}

export interface OperatorRow extends FdOperatorStat {
  qsoShare: number | null
  pointShare: number | null
  multShare: number | null
}

/** Each operator with their shares of the whole: every counted contact belongs to exactly one
 *  operator, so the operators' sums ARE the log's totals. */
export function operatorRows(ops: readonly FdOperatorStat[]): OperatorRow[] {
  const sum = (f: (o: FdOperatorStat) => number) => ops.reduce((n, o) => n + f(o), 0)
  const qsos = sum((o) => o.qsos)
  const points = sum((o) => o.points)
  const mults = sum((o) => o.newMults)
  return ops.map((o) => ({
    ...o,
    qsoShare: share(o.qsos, qsos),
    pointShare: share(o.points, points),
    multShare: share(o.newMults, mults),
  }))
}

/** ⭐ The summary's per-operator section is drawn only when there is more than one operator to
 *  compare: a single operator's one row would repeat the score tiles above it. The Operators box
 *  always draws its rows, because there the row says who is on the board at all. */
export function showsOperatorSection(ops: readonly FdOperatorStat[] | undefined): boolean {
  return (ops?.length ?? 0) > 1
}

/** A UTC clock hour as `HHZ`. */
export function utcHourLabel(unix: number): string {
  return `${String(new Date(unix * 1000).getUTCHours()).padStart(2, '0')}Z`
}

/** An operator's band × mode cells, bands in rig order. */
export function bandModeCells(o: FdOperatorStat): FdOperatorStat['bandMode'] {
  return [...o.bandMode].sort(
    (a, b) => bandRank(a.band) - bandRank(b.band) || a.band.localeCompare(b.band) || a.mode.localeCompare(b.mode),
  )
}

function withShare(n: number, pct: number | null): string {
  return pct === null ? String(n) : `${n} (${pct}%)`
}

/**
 * The exported score summary's OPERATORS section — English, fixed-width, like the rest of that
 * document (ContestView's `buildSummaryText` says why it is not translated). Empty when the summary
 * draws no per-operator section. Rates over the last minutes describe a moment, not a contest, so
 * the document carries each operator's best clock hour instead.
 */
export function operatorSummaryLines(ops: readonly FdOperatorStat[] | undefined, withMults: boolean): string[] {
  if (!ops || !showsOperatorSection(ops)) return []
  const rows = operatorRows(ops)
  const L: string[] = ['OPERATORS']
  const head = ['Operator'.padEnd(12), 'QSOs'.padEnd(12), 'Points'.padEnd(12)]
  if (withMults) head.push('New mults'.padEnd(12))
  head.push('Best hour')
  L.push(`  ${head.join('')}`)
  for (const r of rows) {
    const cols = [operatorLabel(r.operator).padEnd(12), withShare(r.qsos, r.qsoShare).padEnd(12), withShare(r.points, r.pointShare).padEnd(12)]
    if (withMults) cols.push(withShare(r.newMults, r.multShare).padEnd(12))
    cols.push(r.bestHourQsos > 0 ? `${utcHourLabel(r.bestHourStartUnix)} ${r.bestHourQsos}` : '—')
    L.push(`  ${cols.join('')}`.trimEnd())
  }
  for (const r of rows) {
    const cells = bandModeCells(r).map((c) => `${c.band} ${c.mode} ${c.qsos}`)
    L.push(`  ${operatorLabel(r.operator)} by band and mode: ${cells.join(', ')}`)
  }
  if (rows.some((r) => r.operator === '')) L.push(`  ${UNNAMED_OPERATOR} = contacts logged with no operator named.`)
  L.push('  Points are QSO points before multipliers and bonuses; a multiplier counts for whoever worked it first.')
  L.push('')
  return L
}
