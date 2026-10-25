// @vitest-environment jsdom
//
// THE COUNTRY ON THE CONTEST ENTRY — the call's country from the country file, as the strip already
// resolves it on every keystroke, read-only on the Call caption's line: "Fed. Rep. of Germany · CQ 14"
// in a contest that receives a CQ zone, the country alone in one that does not. The zone stays the
// zone box's placeholder and never becomes its value.
//
// ⚠️ jsdom lays out nothing; that the label moves no box is the browser measurement's to say.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve(true)),
  contestWorking: vi.fn(() => Promise.resolve(true)),
  contestEntryReset: vi.fn(() => Promise.resolve(true)),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.25 }, hunt: null } as unknown as AppSnapshot

const COUNTRY: Record<string, string> = { DL1ABC: 'Fed. Rep. of Germany', W6XYZ: 'United States' }
const ZONE: Record<string, number> = { DL1ABC: 14, W6XYZ: 3 }

/** CQ WW SSB as the engine serialises it: RST and a CQ zone. */
const cqww = {
  running: true,
  state: 'run',
  qsoCount: 0,
  sections: 0,
  points: 0,
  event: 'cqww_ssb',
  log: [],
  role: '',
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
  ],
  composing: [
    { key: 'RST', raw: '59' },
    { key: 'ZN', raw: '4' },
  ],
} as unknown as FieldDayStatus

/** A contest with no zone in its exchange (a serial number). */
const serial = {
  ...cqww,
  event: 'wpx_ssb',
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'NR', kind: 'number', required: true, min: 1, max: 99999 },
  ],
  composing: [
    { key: 'RST', raw: '59' },
    { key: 'NR', raw: '1' },
  ],
} as unknown as FieldDayStatus

const strip = (fieldDay: FieldDayStatus) => (
  <LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" titled={false} fieldDay={fieldDay} fdMode="PH" />
)
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const country = () => document.querySelector<HTMLElement>('.le-fd-country')
const zoneBox = () =>
  [...document.querySelectorAll('.le-fd-big .le-fd-field')]
    .find((l) => l.querySelector('.le-fd-cap')?.textContent === 'Zone')!
    .querySelector('input') as HTMLInputElement
const typeCall = async (call: string) => {
  await act(async () => {
    fireEvent.change(callBox(), { target: { value: call } })
  })
}

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  api.resolveEntity.mockImplementation(() => Promise.resolve(null))
  api.contestZoneHint.mockImplementation(() => Promise.resolve(null))
})

describe('the country on the contest entry', () => {
  it('shows the country and its CQ zone on the Call caption line in a CQ-zone contest, and fills nothing', async () => {
    api.resolveEntity.mockImplementation((c: string) => Promise.resolve(COUNTRY[c] ?? null))
    api.contestZoneHint.mockImplementation((c: string) => Promise.resolve(ZONE[c] ?? null))
    render(strip(cqww))
    expect(country()?.textContent ?? '').toBe('')
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe(t('logEntry.contest.country.zone', { country: 'Fed. Rep. of Germany', zone: '14' }))
    expect(country()!.textContent).toBe('Fed. Rep. of Germany · CQ 14')
    // On the Call caption's line, after the DUPE badge's live region and outside it.
    const caption = callBox().closest('label')!.querySelector('.le-fd-cap-marked')!
    expect(country()!.parentElement).toBe(caption)
    expect(country()!.closest('[aria-live]')).toBeNull()
    expect(caption.querySelector('.le-fd-dupe')!.compareDocumentPosition(country()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // The zone is still only the box's hint.
    expect(zoneBox().placeholder).toBe('14')
    expect(zoneBox().value).toBe('')
    // It follows the call, and goes when the call does.
    await typeCall('W6XYZ')
    expect(country()!.textContent).toBe('United States · CQ 3')
    await typeCall('')
    expect(country()?.textContent ?? '').toBe('')
  })

  it('shows the country alone in a contest with no zone in its exchange', async () => {
    api.resolveEntity.mockImplementation((c: string) => Promise.resolve(COUNTRY[c] ?? null))
    api.contestZoneHint.mockImplementation((c: string) => Promise.resolve(ZONE[c] ?? null))
    render(strip(serial))
    await typeCall('DL1ABC')
    expect(country()!.textContent).toBe('Fed. Rep. of Germany')
    expect(api.contestZoneHint).not.toHaveBeenCalled()
  })

  it('shows nothing for a call the country file cannot place', async () => {
    render(strip(cqww))
    await typeCall('ZZ9ZZZ')
    expect(country()?.textContent ?? '').toBe('')
  })
})
