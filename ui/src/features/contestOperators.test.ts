// Per-operator statistics: shares, the show rule and the exported section, by value.
import { describe, expect, it } from 'vitest'
import type { FdOperatorStat } from '../types'
import { operatorRows, operatorSummaryLines, showsOperatorSection, utcHourLabel } from './contestOperators'

/** The top of a UTC clock hour: 2025-10-09 08:00Z. */
const H = 1_759_996_800

const op = (o: Partial<FdOperatorStat> & { operator: string }): FdOperatorStat => ({
  qsos: 0,
  points: 0,
  newMults: 0,
  rate10: 0,
  rate60: 0,
  bestHourStartUnix: 0,
  bestHourQsos: 0,
  bandMode: [],
  ...o,
})

/** The figures tempo-core's `three_operators_get_their_own_figures` computes, as the status ships them. */
const THREE_OPS: FdOperatorStat[] = [
  op({
    operator: 'K9AAA',
    qsos: 4,
    points: 8,
    newMults: 4,
    bestHourStartUnix: H,
    bestHourQsos: 3,
    bandMode: [
      { band: '40m', mode: 'CW', qsos: 1 },
      { band: '20m', mode: 'CW', qsos: 3 },
    ],
  }),
  op({
    operator: 'N9BBB',
    qsos: 2,
    points: 4,
    newMults: 1,
    rate60: 1,
    bestHourStartUnix: H + 3600,
    bestHourQsos: 2,
    bandMode: [{ band: '20m', mode: 'CW', qsos: 2 }],
  }),
  op({
    operator: '',
    qsos: 1,
    points: 2,
    newMults: 1,
    rate10: 6,
    rate60: 1,
    bestHourStartUnix: H + 7200,
    bestHourQsos: 1,
    bandMode: [{ band: '20m', mode: 'CW', qsos: 1 }],
  }),
  op({
    operator: 'W9CCC',
    qsos: 1,
    points: 2,
    newMults: 1,
    rate10: 6,
    rate60: 1,
    bestHourStartUnix: H + 7200,
    bestHourQsos: 1,
    bandMode: [{ band: '40m', mode: 'CW', qsos: 1 }],
  }),
]

describe('operatorRows', () => {
  it('gives each operator their share of contacts, points and new multipliers', () => {
    const rows = operatorRows(THREE_OPS)
    expect(rows.map((r) => [r.operator, r.qsoShare, r.pointShare, r.multShare])).toEqual([
      ['K9AAA', 50, 50, 57],
      ['N9BBB', 25, 25, 14],
      ['', 13, 13, 14],
      ['W9CCC', 13, 13, 14],
    ])
  })

  it('has no share of nothing', () => {
    expect(operatorRows([op({ operator: 'K9AAA', qsos: 3, points: 6 })])[0].multShare).toBeNull()
  })
})

describe('showsOperatorSection', () => {
  it('draws the summary section for two operators or more, never for one', () => {
    expect(showsOperatorSection(THREE_OPS)).toBe(true)
    expect(showsOperatorSection(THREE_OPS.slice(0, 2))).toBe(true)
    expect(showsOperatorSection(THREE_OPS.slice(0, 1))).toBe(false)
    expect(showsOperatorSection([])).toBe(false)
    expect(showsOperatorSection(undefined)).toBe(false)
  })
})

describe('utcHourLabel', () => {
  it('names the UTC clock hour', () => {
    expect(utcHourLabel(H)).toBe('08Z')
    expect(utcHourLabel(H + 7200 + 3599)).toBe('10Z')
  })
})

describe('operatorSummaryLines', () => {
  it('writes the exported section exactly', () => {
    expect(operatorSummaryLines(THREE_OPS, true)).toEqual([
      'OPERATORS',
      '  Operator    QSOs        Points      New mults   Best hour',
      '  K9AAA       4 (50%)     8 (50%)     4 (57%)     08Z 3',
      '  N9BBB       2 (25%)     4 (25%)     1 (14%)     09Z 2',
      '  —           1 (13%)     2 (13%)     1 (14%)     10Z 1',
      '  W9CCC       1 (13%)     2 (13%)     1 (14%)     10Z 1',
      '  K9AAA by band and mode: 40m CW 1, 20m CW 3',
      '  N9BBB by band and mode: 20m CW 2',
      '  — by band and mode: 20m CW 1',
      '  W9CCC by band and mode: 40m CW 1',
      '  — = contacts logged with no operator named.',
      '  Points are QSO points before multipliers and bonuses; a multiplier counts for whoever worked it first.',
      '',
    ])
  })

  it('leaves out the multiplier column for a contest without multipliers', () => {
    expect(operatorSummaryLines(THREE_OPS, false)[1]).toBe('  Operator    QSOs        Points      Best hour')
  })

  it('writes nothing for a single operator', () => {
    expect(operatorSummaryLines(THREE_OPS.slice(0, 1), true)).toEqual([])
  })
})
