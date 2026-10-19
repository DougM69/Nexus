// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S CLOCK (the Illinois QSO Party, 2026-10-18: "a bigger UTC / local clock in the contest
// screen"; the operator had to squint at the top bar's). UTC first, then this computer's local time beside it,
// on the screen's banner, so it is in the main window's Contest screen, its pop-out and the logger window
// alike. It is the station clock the top bar shows (`UtcClock`), so the two can never read differently.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup, within } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdActive: true, fdEvent: 'ilqp' })),
  setSettings: vi.fn(async () => ({})),
  getFdRuleset: vi.fn(async () => null),
  setFdOperator: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))

const fieldDay = {
  running: true,
  state: 'Running',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'ilqp',
  role: 'in_state',
  receives: [],
  composing: [],
} as unknown as FieldDayStatus

const settle = () =>
  act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })

const tz = process.env.TZ
beforeEach(() => {
  // Central Daylight Time on the evening of the party: five hours behind UTC.
  process.env.TZ = 'America/Chicago'
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] })
  vi.setSystemTime(new Date('2026-10-18T23:05:09Z'))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  process.env.TZ = tz
})

const clock = () => screen.getByRole('group', { name: t('fieldDay.clock.aria') })
const readings = () =>
  [...clock().querySelectorAll('.utc-clock')].map((c) => ({
    time: c.querySelector('.utc-time')?.textContent,
    label: c.querySelector('.utc-label')?.textContent,
  }))

describe("the Contest screen's clock", () => {
  it('shows UTC first and the local time beside it, on the banner', async () => {
    render(<ContestView fieldDay={fieldDay} fdActive />)
    await settle()
    expect(clock().closest('.fd-event-banner')).not.toBeNull()
    expect(readings()).toEqual([
      { time: '23:05:09', label: 'UTC' },
      { time: '18:05:09', label: t('topbar.localClock.label') },
    ])
  })

  it('ticks each second', async () => {
    render(<ContestView fieldDay={fieldDay} fdActive />)
    await settle()
    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(readings().map((r) => r.time)).toEqual(['23:05:10', '18:05:10'])
  })

  it('is there with no contest running too', async () => {
    render(<ContestView fieldDay={null} fdActive={false} />)
    await settle()
    expect(within(clock()).getAllByText(/^\d\d:\d\d:\d\d$/)).toHaveLength(2)
  })
})
