import { describe, it, expect } from 'vitest'
import { annotate, bandModeGrid, buildSummaryText, modeColumns } from './ContestView'
import type { DupeRule, FieldDayQso } from '../types'

function qso(call: string, band: string, mode: string, dupe?: boolean): FieldDayQso {
  return { call, class: '1A', section: 'IL', band, mode, ...(dupe === undefined ? {} : { dupe }) }
}

// ⭐ THE ENGINE DECIDES WHICH ROW IS A DUPE, and the table reports its answer.
//
// `annotate` used to re-derive the verdict from a fourth copy of the dupe key
// (`call|band|mode`, counted, >1 = dupe). Three things were wrong with that, and the first
// two are the ones an operator sees:
//
//   * COUNTING marks the ORIGINAL as well as the duplicate — a real, scoring contact was
//     styled as a dupe purely because the call came back later;
//   * the triple is the key for two of the eighteen shipped rulesets. Sweepstakes works a
//     station once on ANY band, so its cross-band dupe went unmarked; a QSO party counts a
//     new county as a new contact, so two legal contacts were both marked;
//   * it could not see a row the ENGINE logged as a dupe, which is the only thing that
//     actually determines whether the row scores.
//
// Now that the cross-checked contests LOG a dupe rather than refusing it, the log contains
// zero-scoring rows and guessing at them is no longer merely imprecise.
describe('annotate() reports the engine\'s dupe flag, and never guesses one', () => {
  it('marks the row the engine flagged, and only that row', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW', true)])
    expect(rows.map((r) => r.isDupe)).toEqual([false, true])
  })

  it('marks nothing when the engine flagged nothing, however the calls repeat', () => {
    // Under Sweepstakes' own rule these two would BOTH be one contact and the second a
    // dupe — but that is the engine's call, and here it has said neither is.
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })

  it('marks a cross-band dupe the old triple could never see (Sweepstakes)', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '40m', 'PH', true)])
    expect(rows.map((r) => r.isDupe)).toEqual([false, true])
  })

  it('treats an absent flag as false, so a build older than the field marks nothing', () => {
    const rows = annotate([qso('W1AW', '20m', 'CW'), qso('W1AW', '20m', 'CW')])
    expect(rows.map((r) => r.isDupe)).toEqual([false, false])
  })
})

describe('the Score Summary rules line (design 3f)', () => {
  const args = (rulesYear: number, rulesGenerated: string) => ({
    eventName: 'ARRL Field Day',
    isWfd: false,
    rulesYear,
    rulesGenerated,
    myClass: '1A',
    mySection: 'IL',
    log: [],
    modes: { dig: 0, cw: 0, ph: 0 },
    workedSet: new Set<string>(),
    powerMult: 2,
    qsoPts: 0,
    poweredPoints: 0,
    bonusPoints: 0,
    totalScore: 0,
    claimedBonuses: [],
  })

  it('names the rules vintage scoring the document', () => {
    const text = buildSummaryText(args(2026, '2026-08-29T00:00:00Z'))
    expect(text).toContain('Scored under 2026 rules (data 2026-08-29)')
  })

  it('skips the line on an older backend with no rules stamp', () => {
    const text = buildSummaryText(args(0, ''))
    expect(text).not.toContain('Scored under')
  })
})

// ⭐ THE SUMMARY'S BAND × MODE GRID. ARRL's Field Day summary sheet (2026, page 2, item 18) asks
// for QSOs by band and mode: columns "CW Digital Phone", rows "160 M, 80 M, 40 M, 20 M, 15 M,
// 10 M, 6 M, 2 M, 1.25 M, 70 CM, Other, Satellite, GOTA" over "TOTALS". Proved by value against a
// log whose every cell is computed by hand below.
describe('the band × mode grid', () => {
  const sat = (q: FieldDayQso, bird: string): FieldDayQso => ({ ...q, sat: bird })
  // 20 m: CW 2, Phone 1 · 40 m: Digital 1 · 80 m: Phone 2 · 6 m: CW 1 · 33 cm (no row on the
  // sheet): Phone 1 · through AO-91 on 2 m: Phone 1. Totals CW 3, Digital 1, Phone 5.
  const LOG: FieldDayQso[] = [
    qso('K1ABC', '20m', 'CW'),
    qso('W2DEF', '20m', 'CW'),
    qso('N3GHI', '20m', 'PH'),
    qso('K4JKL', '40m', 'DIG'),
    qso('W5MNO', '80m', 'PH'),
    qso('W6PQR', '80m', 'PH'),
    qso('K7STU', '6m', 'CW'),
    qso('N8VWX', '33cm', 'PH'),
    sat(qso('W9YZA', '2m', 'PH'), 'AO-91'),
  ]
  /** The grid as a table: each line's cells, split where the columns are. */
  const table = (lines: string[]) => lines.map((l) => l.trim().split(/\s{2,}/))
  const RULE = (over: Partial<DupeRule>): DupeRule => ({
    byCall: true,
    byBand: true,
    byModeClass: true,
    byFields: [],
    bySentFields: [],
    modeClassGroups: [],
    logDupes: false,
    ...over,
  })

  it('lays ARRL Field Day out as its summary sheet\'s item 18, cell by cell', () => {
    const grid = bandModeGrid(LOG, modeColumns(RULE({}), LOG), true)
    expect(table(grid)).toEqual([
      ['Band', 'CW', 'Digital', 'Phone'],
      ['160 M', '0', '0', '0'],
      ['80 M', '0', '0', '2'],
      ['40 M', '0', '1', '0'],
      ['20 M', '2', '0', '1'],
      ['15 M', '0', '0', '0'],
      ['10 M', '0', '0', '0'],
      ['6 M', '1', '0', '0'],
      ['2 M', '0', '0', '0'],
      ['1.25 M', '0', '0', '0'],
      ['70 CM', '0', '0', '0'],
      ['Other', '0', '0', '1'],
      ['Satellite', '0', '0', '1'],
      ['GOTA', '—', '—', '—'],
      ['TOTALS', '3', '1', '5'],
    ])
    // The columns line up under their headings, as the sheet's do.
    expect(grid[0]).toBe('  Band            CW  Digital  Phone')
    expect(grid[grid.length - 1]).toBe('  TOTALS           3        1      5')
  })

  it('heads the Field Day summary with it, and its totals are the summary\'s own mode counts', () => {
    const text = buildSummaryText({
      eventName: 'ARRL Field Day',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '2026-08-29T00:00:00Z',
      myClass: '1A',
      mySection: 'IL',
      log: LOG,
      modes: { dig: 1, cw: 3, ph: 5 },
      workedSet: new Set<string>(),
      powerMult: 2,
      qsoPts: 0,
      poweredPoints: 0,
      bonusPoints: 0,
      totalScore: 0,
      claimedBonuses: [],
      fieldDaySheet: true,
    })
    const lines = text.split('\n')
    const at = lines.indexOf('QSOs BY BAND AND MODE (ARRL summary sheet, item 18)')
    expect(at).toBeGreaterThan(0)
    expect(lines[at + 1].trim()).toBe('Band            CW  Digital  Phone')
    expect(lines).toContain("  GOTA: on the GOTA station's own summary.")
    expect(text).toContain('  By mode:  DIG 1   CW 3   PH 5')
  })

  it('gives every other contest the same grid in its own mode classes and bands', () => {
    // The Illinois QSO Party counts CW and digital as one mode: one column for both.
    const ilqp = RULE({ modeClassGroups: [['CW', 'DIG']] })
    expect(modeColumns(ilqp, LOG).map((c) => c.label)).toEqual(['CW/Digital', 'Phone'])
    expect(table(bandModeGrid(LOG, modeColumns(ilqp, LOG), false))).toEqual([
      ['Band', 'CW/Digital', 'Phone'],
      ['80m', '0', '2'],
      ['40m', '1', '0'],
      ['20m', '2', '1'],
      ['6m', '1', '0'],
      ['2m', '0', '1'],
      ['33cm', '0', '1'],
      ['TOTALS', '4', '5'],
    ])
    // A contest that counts no mode class (CQ WW CW) has the classes its log holds: CW alone.
    const cw = LOG.filter((q) => q.mode === 'CW')
    const cqww = RULE({ byModeClass: false })
    expect(table(bandModeGrid(cw, modeColumns(cqww, cw), false))).toEqual([
      ['Band', 'CW'],
      ['20m', '2'],
      ['6m', '1'],
      ['TOTALS', '3'],
    ])
    // CONTROL: the same rule over the whole log shows every class it holds, and three classes
    // counted apart (New York) are three columns.
    expect(modeColumns(cqww, LOG).map((c) => c.label)).toEqual(['CW', 'Digital', 'Phone'])
    expect(modeColumns(RULE({}), cw).map((c) => c.label)).toEqual(['CW', 'Digital', 'Phone'])
  })

  it('counts a duplicate logged at zero points, and says so', () => {
    const log = [qso('K1ABC', '20m', 'CW'), qso('K1ABC', '20m', 'CW', true)]
    const text = buildSummaryText({
      eventName: 'CQ World-Wide DX Contest (CW)',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '',
      myClass: '',
      mySection: '',
      log,
      modes: { dig: 0, cw: 2, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 1,
      qsoPts: 0,
      poweredPoints: 0,
      bonusPoints: 0,
      totalScore: 0,
      claimedBonuses: [],
      contest: { sending: '599 4', multCount: 2 },
      dupeRule: RULE({ byModeClass: false, logDupes: true }),
    })
    expect(text).toContain('QSOs BY BAND AND MODE\n')
    expect(text).not.toContain('item 18')
    expect(text).toContain('  Counted above: 1 duplicate contact logged at zero points.')
    expect(table(text.split('\n').filter((l) => l.startsWith('  TOTALS')))).toEqual([['TOTALS', '2']])
  })
})
