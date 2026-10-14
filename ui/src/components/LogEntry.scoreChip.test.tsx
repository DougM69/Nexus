// @vitest-environment jsdom
//
// THE SCORE CHIP IN THE STRIP — the real `LogEntry`, rendered with the props `PhoneCockpit`
// passes, shows the running contest in its header while a contest runs, and nothing of the
// kind on the everyday log line. Every cockpit that joins a contest (CW, Phone, RTTY, PSK,
// JS8, Satellites, the logger window) renders this same strip, so this is where the chip
// reaches all of them. What the chip reads is pinned by value in ContestScoreChip.test.tsx.
//
// ⚠️ jsdom lays out nothing: the header row it shares, and that the strip's height does not
// move, are a headless-Chrome measurement (the report's table), not an assertion here.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

const now = Math.floor(Date.now() / 1000)
const ilqp = {
  running: true,
  state: 'sp',
  event: 'ilqp',
  role: 'in_state',
  qsoCount: 48,
  sections: 3,
  points: 106,
  totalScore: 212,
  multCount: 19,
  // Two contacts an hour apart, the newest now: one interval in an hour is 1 an hour.
  log: [
    { call: 'K9A', band: '20m', mode: 'CW', whenUnix: now - 3600 },
    { call: 'K9B', band: '20m', mode: 'CW', whenUnix: now },
  ],
  receives: [
    { key: 'RST', kind: 'pattern', required: true },
    { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
  ],
} as unknown as FieldDayStatus

function strip(fieldDay: FieldDayStatus | undefined) {
  return (
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fieldDay}
      fdMode="PH"
    />
  )
}

afterEach(cleanup)

describe('the contest strip shows the score chip', () => {
  it('in its header while a contest runs, reading the session', () => {
    render(strip(ilqp))
    const chip = document.querySelector('.le-fd-header .le-fd-score')
    expect(chip?.textContent ?? null).toBe('ILQP · 212 pts · 48 Q · 19 mults · 1/hr')
  })

  it('between the band hint and what this station is sending', () => {
    // The order the zero-cost slot was measured in: it takes only the room between the two.
    render(strip(ilqp))
    const header = document.querySelector('.le-fd-header')!
    const order = [...header.children].map((el) => el.className)
    expect(order.indexOf('le-fd-score')).toBe(order.indexOf('le-fd-hint') + 1)
    expect(order.indexOf('le-fd-sent')).toBe(order.indexOf('le-fd-score') + 1)
  })

  it('but not when its host shows the chip in the pane head above it', () => {
    render(
      <LogEntry
        onOpenLogbook={() => {}}
        snap={snap}
        mode="SSB"
        defaultRst="59"
        exchange="terrestrial"
        titled={false}
        onSpot={() => {}}
        pendingWork={null}
        onConsumeWork={() => {}}
        fieldDay={ilqp}
        fdMode="PH"
        scoreChip={false}
      />,
    )
    expect(document.querySelector('.log-entry-fd')).not.toBeNull()
    expect(document.querySelector('.le-fd-score')).toBeNull()
  })

  it('and the everyday log line, with no contest running, has none', () => {
    render(strip(undefined))
    expect(document.querySelector('.log-entry-fd')).toBeNull()
    expect(document.querySelector('.le-fd-score')).toBeNull()
  })
})
