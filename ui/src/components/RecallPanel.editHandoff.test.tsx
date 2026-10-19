// @vitest-environment jsdom
//
// A PREVIOUS CONTACT IN THE LOOKUP CARD OPENS THE LOGBOOK'S EDITOR ON THAT CONTACT. The row hands its
// contact's id over with the call (the Logbook opens its form on that id); a row without an id, from a
// station older than 1.14, hands the call alone, as every row did before. By value: the id handed over
// is the clicked row's, not its neighbour's — two contacts with one call are told apart only by it.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import type { CallHistory } from '../features/callHistory'
import type { LoggedQso } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn(async () => {}) }))

afterEach(cleanup)

const qso = (id: string | undefined, day: number) =>
  ({
    id, call: 'W1ABC', grid: 'FN31', band: '20m', freqMhz: 14.25, mode: 'SSB',
    rstSent: '59', rstRcvd: '57', whenUnix: Date.UTC(2026, 2, day) / 1000,
  }) as LoggedQso

const hist = (qsos: LoggedQso[]): CallHistory =>
  ({
    qsos, count: qsos.length, workedBefore: true, dupeThisBand: false,
    lastUnix: qsos[0].whenUnix, confirmedCount: 0, bands: ['20m'], modes: ['SSB'],
  }) as CallHistory

function rows(qsos: LoggedQso[], onOpenLog: (call: string, id?: string | null) => void) {
  const { container } = render(<RecallPanel call="W1ABC" band="20m" hist={hist(qsos)} onOpenLog={onOpenLog} />)
  return [...container.querySelectorAll<HTMLElement>('.recall-log-row')]
}

it('a click on a previous contact hands over THAT contact’s id', () => {
  const onOpenLog = vi.fn()
  // Newest first on the card: id-new (14 March) above id-old (2 March).
  const [first, second] = rows([qso('id-old', 2), qso('id-new', 14)], onOpenLog)
  fireEvent.click(second.querySelector('.recall-log-bm')!)
  expect(onOpenLog).toHaveBeenLastCalledWith('W1ABC', 'id-old')
  fireEvent.click(first.querySelector('.recall-log-bm')!)
  expect(onOpenLog).toHaveBeenLastCalledWith('W1ABC', 'id-new')
})

it('Enter on a focused row hands over that row’s id too', () => {
  const onOpenLog = vi.fn()
  const [, second] = rows([qso('id-old', 2), qso('id-new', 14)], onOpenLog)
  fireEvent.focus(second)
  fireEvent.keyDown(second, { key: 'Enter' })
  expect(onOpenLog).toHaveBeenLastCalledWith('W1ABC', 'id-old')
})

it('a row without an id hands over the call alone (a search, as before)', () => {
  const onOpenLog = vi.fn()
  const [row] = rows([qso(undefined, 2)], onOpenLog)
  fireEvent.click(row.querySelector('.recall-log-bm')!)
  expect(onOpenLog.mock.calls).toEqual([['W1ABC']])
})
