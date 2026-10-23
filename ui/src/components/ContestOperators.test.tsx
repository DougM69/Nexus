// @vitest-environment jsdom
//
// The Contest summary's Operators section and the Operators box: what each draws, by value, and when
// each draws nothing.
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen, within } from '@testing-library/react'
import type { FdOperatorStat, FieldDayStatus } from '../types'
import nyqpFieldDay from './__fixtures__/nyqpFieldDay.json'
import { OperatorStatsBoard, OperatorsBox } from './ContestOperators'
import { buildSummaryText } from './ContestView'
import { PaneBody } from './connect/PaneFrame'
import type { PaneContext } from './connect/paneContext'

afterEach(cleanup)

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

const OPS: FdOperatorStat[] = [
  op({
    operator: 'K9AAA',
    qsos: 4,
    points: 8,
    newMults: 4,
    rate60: 2,
    bestHourStartUnix: H,
    bestHourQsos: 3,
    bandMode: [
      { band: '40m', mode: 'CW', qsos: 1 },
      { band: '20m', mode: 'CW', qsos: 3 },
    ],
  }),
  op({ operator: 'N9BBB', qsos: 2, points: 4, newMults: 1, rate10: 12, rate60: 2, bestHourStartUnix: H + 3600, bestHourQsos: 2, bandMode: [{ band: '20m', mode: 'PH', qsos: 2 }] }),
  op({ operator: '', qsos: 1, points: 2, newMults: 1, rate10: 6, rate60: 1, bestHourStartUnix: H + 7200, bestHourQsos: 1, bandMode: [{ band: '20m', mode: 'CW', qsos: 1 }] }),
]

const status = (over: Partial<FieldDayStatus>): FieldDayStatus =>
  ({ ...(nyqpFieldDay as unknown as FieldDayStatus), ...over }) as FieldDayStatus

/** Each body row's cells, as text. */
function rows(table: HTMLElement): string[][] {
  return [...table.querySelectorAll('tbody tr')].map((tr) => [...tr.children].map((c) => c.textContent ?? ''))
}

describe('the summary section', () => {
  it('lists every operator with their figures and shares, multipliers included', () => {
    render(<OperatorStatsBoard fieldDay={status({ operators: OPS, multCount: 6 })} />)
    const table = screen.getByRole('table', { name: 'Contest statistics per operator' })
    expect(rows(table)).toEqual([
      ['K9AAA', '4 57%', '8 57%', '4 67%', '0', '2', '08Z 3'],
      ['N9BBB', '2 29%', '4 29%', '1 17%', '12', '2', '09Z 2'],
      ['—', '1 14%', '2 14%', '1 17%', '6', '1', '10Z 1'],
    ])
    // The contacts that name nobody say so.
    expect(within(table).getByTitle('Contacts logged with no operator named').textContent).toBe('—')
  })

  it('draws each operator a band × mode table, bands in rig order', () => {
    const { container } = render(<OperatorStatsBoard fieldDay={status({ operators: OPS, multCount: 6 })} />)
    const k9 = container.querySelector<HTMLElement>('.fd-ops-grid[data-operator="K9AAA"]')!
    expect(rows(k9)).toEqual([
      ['40m', '1'],
      ['20m', '3'],
    ])
    const n9 = container.querySelector<HTMLElement>('.fd-ops-grid[data-operator="N9BBB"]')!
    expect([...n9.querySelectorAll('thead th')].map((th) => th.textContent)).toEqual(['Band', 'PH'])
    expect(rows(n9)).toEqual([['20m', '2']])
  })

  it('has no multiplier column where the contest counts none', () => {
    render(<OperatorStatsBoard fieldDay={status({ operators: OPS, multCount: undefined })} />)
    const table = screen.getByRole('table', { name: 'Contest statistics per operator' })
    expect(rows(table)[0]).toEqual(['K9AAA', '4 57%', '8 57%', '0', '2', '08Z 3'])
  })

  it('is not drawn for a single operator, nor with no contest', () => {
    const { container, rerender } = render(<OperatorStatsBoard fieldDay={status({ operators: OPS.slice(0, 1) })} />)
    expect(container.innerHTML).toBe('')
    rerender(<OperatorStatsBoard fieldDay={null} />)
    expect(container.innerHTML).toBe('')
  })
})

describe('the Operators box', () => {
  it('names who is at the key and lists the leaderboard', () => {
    const { container } = render(<OperatorsBox fieldDay={status({ operators: OPS, operatorNow: 'N9BBB' })} />)
    expect(container.querySelector('.fd-ops-atkey')?.textContent).toBe('At the key: N9BBB')
    expect(rows(screen.getByRole('table'))).toEqual([
      ['K9AAA', '4', '8', '2'],
      ['N9BBB', '2', '4', '2'],
      ['—', '1', '2', '1'],
    ])
  })

  it('shows a single operator as one row', () => {
    render(<OperatorsBox fieldDay={status({ operators: OPS.slice(0, 1), operatorNow: '' })} />)
    expect(screen.getByText('Nobody is named at the key.')).toBeTruthy()
    expect(rows(screen.getByRole('table'))).toEqual([['K9AAA', '4', '8', '2']])
  })

  it('says no contacts yet before the first one', () => {
    render(<OperatorsBox fieldDay={status({ operators: [], operatorNow: 'K9AAA' })} />)
    expect(screen.getByText('No contacts yet.')).toBeTruthy()
    expect(screen.queryByRole('table')).toBeNull()
  })

  it('says no contest is running when there is none', () => {
    const ctx = { fieldDay: null } as unknown as PaneContext
    const { container } = render(<PaneBody pane="operators" ctx={ctx} />)
    expect(container.textContent).toBe('No contest running.')
    cleanup()
    const running = render(<PaneBody pane="operators" ctx={{ fieldDay: status({ operators: OPS, operatorNow: 'K9AAA' }) } as unknown as PaneContext} />)
    expect(running.container.querySelector('.fd-ops-atkey')?.textContent).toBe('At the key: K9AAA')
  })
})

describe('the Summary export', () => {
  const args = (operators?: FdOperatorStat[]) =>
    buildSummaryText({
      eventName: 'New York QSO Party',
      isWfd: false,
      rulesYear: 2026,
      rulesGenerated: '',
      myClass: '',
      mySection: '',
      log: [],
      modes: { dig: 0, cw: 0, ph: 0 },
      workedSet: new Set<string>(),
      powerMult: 1,
      qsoPts: 14,
      poweredPoints: 14,
      bonusPoints: 0,
      totalScore: 84,
      claimedBonuses: [],
      contest: { sending: '1 WI', multCount: 6 },
      operators,
    }).split('\n')

  it('carries the operators section, multipliers included, for a contest that counts them', () => {
    const lines = args(OPS)
    const at = lines.indexOf('OPERATORS')
    expect(at).toBeGreaterThan(0)
    expect(lines[at + 1]).toBe('  Operator    QSOs        Points      New mults   Best hour')
    expect(lines[at + 2]).toBe('  K9AAA       4 (57%)     8 (57%)     4 (67%)     08Z 3')
  })

  it('has no operators section for one operator, or from an older station', () => {
    expect(args(OPS.slice(0, 1))).not.toContain('OPERATORS')
    expect(args(undefined)).not.toContain('OPERATORS')
  })
})
