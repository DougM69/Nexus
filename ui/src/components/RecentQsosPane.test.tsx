// @vitest-environment jsdom
//
// THE RECENT CONTACTS PANE, by value: it lists the newest `RECENT_QSOS` contacts of the log, newest
// first, whatever order the log holds them in; a row click hands that row's call and id to the
// Logbook's editor; it always draws the same number of lines; and a cockpit behind another view asks
// the log nothing.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RECENT_QSOS, RecentQsos } from './RecentQsosPane'
import { t } from '../i18n'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type AnswerTo, type LogQuestion } from '../features/logAnswers'
import { __resetLogSourceForTests, setLogSource } from '../features/logSource'
import type { LoggedQso } from '../types'

const qso = (id: string | undefined, call: string, minute: number) =>
  ({ id, call, band: '20m', freqMhz: 14.2, mode: 'SSB', whenUnix: 1_700_000_000 + minute * 60 }) as unknown as LoggedQso

let log: LoggedQso[] = []
const asked: LogQuestion[] = []
beforeEach(() => {
  asked.length = 0
  setLogSource(
    createAskingLogSource(async <Q extends LogQuestion>(q: Q) => {
      asked.push(q)
      return answerFrom(log, q, 1) as AnswerTo<Q>
    }),
  )
})
afterEach(() => {
  cleanup()
  __resetLogSourceForTests()
})

const calls = () => [...document.querySelectorAll('.recent-qso-call')].map((c) => c.textContent)

it('lists the newest five by QSO time, newest first — not the last five the log holds', async () => {
  expect(RECENT_QSOS).toBe(5)
  // Out of time order, as an import leaves a log: K5 is the newest, though not the last row.
  log = [qso('a', 'K1', 1), qso('b', 'K2', 2), qso('e', 'K5', 9), qso('c', 'K3', 3), qso('d', 'K4', 4), qso('f', 'K6', 0), qso('g', 'K7', 5)]
  render(<RecentQsos onOpen={vi.fn()} />)
  await waitFor(() => expect(calls()).toEqual(['K5', 'K7', 'K4', 'K3', 'K2']))
})

it('a row click opens THAT contact in the editor: its call and its id', async () => {
  log = [qso('id-1', 'W1AW', 1), qso('id-2', 'W1AW', 2), qso('id-3', 'N2XYZ', 3)]
  const onOpen = vi.fn()
  render(<RecentQsos onOpen={onOpen} />)
  const rows = await screen.findAllByRole('button', { name: t('logbook.row.edit', { call: 'W1AW' }) })
  // Newest first: id-2 above id-1, the same call — only the id tells them apart.
  fireEvent.click(rows[1])
  expect(onOpen.mock.calls).toEqual([['W1AW', 'id-1']])
  fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'N2XYZ' }) }))
  expect(onOpen).toHaveBeenLastCalledWith('N2XYZ', 'id-3')
})

it('draws five lines whatever it holds, so logging into a short log moves nothing below it', async () => {
  log = []
  const { rerender } = render(<RecentQsos onOpen={vi.fn()} logTick={1} />)
  await screen.findByText(t('recentQsos.empty'))
  expect(screen.getAllByRole('listitem')).toHaveLength(RECENT_QSOS)
  log = [qso('a', 'K1', 1), qso('b', 'K2', 2)]
  rerender(<RecentQsos onOpen={vi.fn()} logTick={2} />)
  await waitFor(() => expect(calls()).toEqual(['K2', 'K1']))
  expect(screen.getAllByRole('listitem')).toHaveLength(RECENT_QSOS)
})

it('a row with no id, or no Logbook to open, is a plain line, never a control that goes nowhere', async () => {
  log = [qso(undefined, 'K1', 1), qso('b', 'K2', 2)]
  const { rerender } = render(<RecentQsos onOpen={vi.fn()} />)
  await waitFor(() => expect(calls()).toEqual(['K2', 'K1']))
  expect(screen.getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual([t('logbook.row.edit', { call: 'K2' })])
  rerender(<RecentQsos />)
  expect(screen.queryAllByRole('button')).toHaveLength(0)
  expect(calls()).toEqual(['K2', 'K1'])
})

it('a cockpit behind another view asks the log nothing', async () => {
  log = [qso('a', 'K1', 1)]
  render(<RecentQsos onOpen={vi.fn()} active={false} />)
  await new Promise((r) => setTimeout(r, 20))
  expect(asked).toEqual([])
  expect(calls()).toEqual([])
})
