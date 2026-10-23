// @vitest-environment jsdom
//
// THE TIMELINE CARD, held by value against a fake engine that answers each range as
// `get_log_timeline` does (the counting, and its UTC windows, are tempo-core's own tests): it asks
// for the range the switch names, draws a line per mode with "Other" past the top five, names every
// line in its legend, says when a range is empty, shows a bucket's counts when it is pointed at or
// stepped to, and lists every count in a table for a screen reader.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { LoggedQso, LogTimeline, TimelineRange } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import { t } from '../i18n'
import { ASK_AGAIN_AFTER_MS, NOT_ANSWERED } from '../features/notAnswered'

vi.mock('../api', () => ({
  getLogTimeline: vi.fn(),
  // For the Statistics view around the card: the engine over a one-contact log, no geography.
  askLog: vi.fn(async (q: LogQuestion) =>
    (await import('../features/logAnswers.testkit')).answerAs(q, [
      { call: 'W1AW', band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1_791_990_000 } as LoggedQso,
    ]),
  ),
  getLogStats: vi.fn(async () => {
    throw new Error('not under test')
  }),
}))

import { getLogTimeline } from '../api'
import { StatsTimeline } from './StatsTimeline'
import { StatsView } from './StatsView'

afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})

/** A window as the engine answers it: buckets starting at `edges` (and ending at the last), and
 * each mode's counts by bucket index. */
function window(edges: number[], modes: Record<string, Record<number, number>>): LogTimeline {
  const n = edges.length - 1
  return {
    edges,
    modes: Object.entries(modes).map(([label, at]) => ({
      label,
      counts: Array.from({ length: n }, (_, i) => at[i] ?? 0),
    })),
  }
}
const every = (first: number, step: number, n: number) => Array.from({ length: n + 1 }, (_, i) => first + i * step)

/** At 2026-10-14 15:37Z: the hours from 2026-10-13 16:00Z, the days from 2026-09-15, the months
 * from 2025-11. */
const PAST_DAY = window(every(1_791_907_200, 3_600, 24), { FT8: { 23: 4, 22: 1 }, CW: { 0: 2 } })
const PAST_30_DAYS = window(every(1_789_430_400, 86_400, 30), {
  FT8: { 29: 10, 0: 5 },
  CW: { 28: 6 },
  SSB: { 27: 5 },
  FT4: { 26: 4 },
  RTTY: { 25: 3 },
  FM: { 24: 2 },
  PSK31: { 24: 1 },
  JT65: { 29: 1 },
})
const PAST_YEAR = window(
  [
    1_761_955_200, 1_764_547_200, 1_767_225_600, 1_769_904_000, 1_772_323_200, 1_775_001_600, 1_777_593_600,
    1_780_272_000, 1_782_864_000, 1_785_542_400, 1_788_220_800, 1_790_812_800, 1_793_491_200,
  ],
  { FT8: { 11: 40, 10: 12 } },
)

function engine(answers: Partial<Record<TimelineRange, LogTimeline>> = {}) {
  const all = { pastDay: PAST_DAY, past30Days: PAST_30_DAYS, pastYear: PAST_YEAR, ...answers }
  vi.mocked(getLogTimeline).mockImplementation(async (range: TimelineRange) => all[range])
}

/** The legend: each line's name and its total in the range. */
async function legend(): Promise<string[][]> {
  const list = await screen.findByRole('list')
  return within(list)
    .getAllByRole('listitem')
    .map((li) => [...li.querySelectorAll('span')].map((s) => s.textContent ?? ''))
}
/** Switch to `range` and wait for its legend. */
async function show(range: string, lines: string[][]) {
  fireEvent.click(chip(range))
  await waitFor(async () => expect(await legend()).toEqual(lines))
}
const chip = (name: string) => screen.getByRole('button', { name })
const plot = () => screen.getByRole('group', { name: /QSOs per mode/ })
const readout = () => document.querySelector('.stats-tl-read')?.textContent

describe('the Timeline card', () => {
  it('opens on the past 30 days and draws the five most-worked modes and one "Other" line', async () => {
    engine()
    const { container } = render(<StatsTimeline />)
    expect(await legend()).toEqual([
      ['FT8', '15'],
      ['CW', '6'],
      ['SSB', '5'],
      ['FT4', '4'],
      ['RTTY', '3'],
      [t('stats.timeline.other'), '4'],
    ])
    expect(getLogTimeline).toHaveBeenCalledWith('past30Days')
    expect(chip(t('stats.timeline.range.past30Days')).getAttribute('aria-pressed')).toBe('true')
    expect(container.querySelectorAll('polyline')).toHaveLength(6)
    expect(container.querySelectorAll('.stats-tl-plot use.stats-tl-mark')).toHaveLength(6 * 30)
  })

  it('the switch asks the engine for each range and shows its answer', async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    await show(t('stats.timeline.range.pastDay'), [
      ['FT8', '5'],
      ['CW', '2'],
    ])
    expect(getLogTimeline).toHaveBeenLastCalledWith('pastDay')
    expect(chip(t('stats.timeline.range.pastDay')).getAttribute('aria-pressed')).toBe('true')
    expect(chip(t('stats.timeline.range.past30Days')).getAttribute('aria-pressed')).toBe('false')

    await show(t('stats.timeline.range.pastYear'), [['FT8', '52']])
    expect(getLogTimeline).toHaveBeenLastCalledWith('pastYear')
  })

  it("while a range's answer is on its way, the card shows neither the last range's lines nor a point", async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    let answer: (t: LogTimeline) => void = () => {}
    vi.mocked(getLogTimeline).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)))
    fireEvent.click(chip(t('stats.timeline.range.pastYear')))
    expect(screen.getByText(t('stats.loading'))).toBeTruthy()
    expect(screen.queryByRole('list'), "the 30 days' lines under the year's name").toBeNull()
    await act(async () => answer(PAST_YEAR))
    expect(await legend()).toEqual([['FT8', '52']])
  })

  it('a point picked in one range is not carried to the next', async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    vi.spyOn(plot(), 'getBoundingClientRect').mockReturnValue({ left: 0, width: 300 } as DOMRect)
    fireEvent(plot(), new MouseEvent('pointermove', { bubbles: true, clientX: 299 }))
    expect(readout()).toMatch(/^2026-10-14/)
    // A click that moves no focus off the chart (the pointer still over it): the year has 12
    // buckets, and the 30th day's place is none of them.
    await show(t('stats.timeline.range.pastYear'), [['FT8', '52']])
    expect(readout()).toBe(t('stats.timeline.hint'))
  })

  it('asks again while a contact logged just before is still being saved', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    try {
      engine()
      vi.mocked(getLogTimeline).mockRejectedValueOnce(`${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`)
      render(<StatsTimeline />)
      await act(async () => {})
      expect(screen.getByText(t('stats.loading')), 'still loading, not failed').toBeTruthy()
      await act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
      expect(getLogTimeline).toHaveBeenCalledTimes(2)
      expect(screen.queryByText(t('stats.failed'))).toBeNull()
      expect(document.querySelectorAll('.stats-tl-legend li')).toHaveLength(6)
    } finally {
      vi.useRealTimers()
    }
  })

  it('draws each point at its bucket and its count', async () => {
    engine()
    const { container } = render(<StatsTimeline />)
    await legend()
    await show(t('stats.timeline.range.pastDay'), [
      ['FT8', '5'],
      ['CW', '2'],
    ])
    // The tallest point (4, at the last of 24 hours) tops the plot at the hour's middle.
    expect(container.querySelector('.stats-tl-y')?.textContent).toBe('40')
    const ft8 = container.querySelector('polyline[data-slot="0"]')?.getAttribute('points')?.split(' ')
    expect(ft8).toHaveLength(24)
    expect(ft8?.[23]).toBe('97.917,0.000')
    expect(ft8?.[22]).toBe('93.750,75.000')
    expect(ft8?.[0]).toBe('2.083,100.000')
    // The axis names the current hour and every sixth before it.
    expect([...container.querySelectorAll('.stats-tl-tick')].map((s) => s.textContent)).toEqual([
      '21:00Z',
      '03:00Z',
      '09:00Z',
      '15:00Z',
    ])
  })

  it('a range with no contact says so, and draws nothing', async () => {
    engine({ pastYear: window(PAST_YEAR.edges, {}) })
    const { container } = render(<StatsTimeline />)
    await legend()
    fireEvent.click(chip(t('stats.timeline.range.pastYear')))
    expect(await screen.findByText(t('stats.timeline.empty'))).toBeTruthy()
    expect(container.querySelector('svg')).toBeNull()
    expect(container.querySelector('table')).toBeNull()
  })

  it('focusing the chart shows the current bucket, and the arrow keys step through the rest', async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    expect(readout()).toBe(t('stats.timeline.hint'))
    fireEvent.focus(plot())
    expect(readout()).toBe(`2026-10-14FT810CW0SSB0FT40RTTY0${t('stats.timeline.other')}1`)
    fireEvent.keyDown(plot(), { key: 'ArrowLeft' })
    expect(readout()).toBe(`2026-10-13FT80CW6SSB0FT40RTTY0${t('stats.timeline.other')}0`)
    fireEvent.keyDown(plot(), { key: 'Home' })
    expect(readout()).toMatch(/^2026-09-15FT85CW0/)
    fireEvent.keyDown(plot(), { key: 'ArrowLeft' })
    expect(readout(), 'no step past the first bucket').toMatch(/^2026-09-15/)
    fireEvent.keyDown(plot(), { key: 'End' })
    expect(readout()).toMatch(/^2026-10-14FT810/)
    fireEvent.blur(plot())
    expect(readout()).toBe(t('stats.timeline.hint'))
  })

  it('pointing at the chart shows the bucket under the pointer', async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    vi.spyOn(plot(), 'getBoundingClientRect').mockReturnValue({ left: 100, width: 300 } as DOMRect)
    // 30 days over 300 px: the 11th day is x 100–110 px into the plot.
    fireEvent(plot(), new MouseEvent('pointermove', { bubbles: true, clientX: 205 }))
    expect(readout()).toMatch(/^2026-09-25FT80/)
    fireEvent(plot(), new MouseEvent('pointermove', { bubbles: true, clientX: 399 }))
    expect(readout()).toMatch(/^2026-10-14FT810/)
    fireEvent.pointerLeave(plot())
    expect(readout()).toBe(t('stats.timeline.hint'))
  })

  it('lists every count in a table for a screen reader', async () => {
    engine()
    render(<StatsTimeline />)
    await legend()
    await show(t('stats.timeline.range.pastDay'), [
      ['FT8', '5'],
      ['CW', '2'],
    ])
    const table = screen.getByRole('table', { name: `QSOs per mode, ${t('stats.timeline.range.pastDay')}` })
    const rows = within(table).getAllByRole('row').map((r) => [...r.children].map((c) => c.textContent))
    expect(rows[0]).toEqual([t('stats.timeline.col.pastDay'), 'FT8', 'CW'])
    expect(rows).toHaveLength(1 + 24)
    expect(rows[1]).toEqual(['2026-10-13 16:00Z', '0', '2'])
    expect(rows[23]).toEqual(['2026-10-14 14:00Z', '1', '0'])
    expect(rows[24]).toEqual(['2026-10-14 15:00Z', '4', '0'])
  })

  it("is the Statistics view's first card on this station's log", async () => {
    engine()
    const { container } = render(<StatsView />)
    await legend()
    expect(container.querySelector('.stats-grid > .stats-card')?.classList.contains('stats-tl')).toBe(true)
    expect(getLogTimeline).toHaveBeenCalledWith('past30Days')
  })

  it('a failed read says so', async () => {
    vi.mocked(getLogTimeline).mockRejectedValue('the logbook could not be read: disk I/O error')
    render(<StatsTimeline />)
    expect(await screen.findByText(t('stats.failed'))).toBeTruthy()
  })
})
