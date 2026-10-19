// @vitest-environment jsdom
//
// "CONTEST MODE IS ON, BUT THE CONTEST IS NOT RUNNING" — the CW and Phone log line, when the switch is on and
// the session is not (re-entry on opening did not happen, or it dropped while the screen was open). The line
// then falls back to the everyday form, with no exchange boxes and no dupe check, and nothing said so. Now
// one line says it, with Resume, which re-enters the contest in S&P (the host's; never a CQ).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { t } from '../i18n'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const snap = { radio: { band: '40m', dialMhz: 7.03 }, hunt: null, mygrid: 'EN52' } as unknown as AppSnapshot
const notice = () => screen.queryByRole('status', { name: t('logEntry.contest.notRunning.aria') })

afterEach(() => cleanup())

describe('the log line says the contest is not running, with Resume', () => {
  it('shows the line on the everyday form, and Resume asks the host to re-enter', () => {
    const onResume = vi.fn()
    render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} onResumeContest={onResume} />)
    expect(notice()?.textContent).toContain(t('logEntry.contest.notRunning'))
    fireEvent.click(within(notice()!).getByRole('button', { name: t('logEntry.contest.resume') }))
    expect(onResume).toHaveBeenCalledTimes(1)
  })

  it('CONTROL: no line without the host saying so, and none while the contest runs', () => {
    const { unmount } = render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    expect(notice()).toBeNull()
    unmount()
    const fieldDay = { running: false, log: [], event: 'ilqp', receives: [], composing: [] } as unknown as FieldDayStatus
    render(
      <LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} fieldDay={fieldDay} fdMode="CW" onResumeContest={() => {}} />,
    )
    expect(notice()).toBeNull()
  })
})
