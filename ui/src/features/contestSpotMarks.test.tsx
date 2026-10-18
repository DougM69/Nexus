// @vitest-environment jsdom
// The contest marks on a spot — dupe and new multiplier — in the band strip, the pop-out band map and
// the cockpits' spots list. The dupe is the strip's own check (`contestDupe`); the multipliers are the
// station's, riding the row (`contestMults`). Every assertion is by value.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, within } from '@testing-library/react'
import { contestSpotMarker, spotContestClass, spotMarkWords } from './contestSpotMarks'
import { BandStrip } from '../components/BandStrip'
import { BandMap } from '../components/BandMap'
import { SpotsPanel } from '../components/SpotsPanel'
import type { DupeRule, FieldDayQso, FieldDayStatus, SpotRow } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), openQrzPage: vi.fn() }))

afterEach(() => {
  cleanup()
  sessionStorage.clear()
  localStorage.clear()
})

const rule = (over: Partial<DupeRule> = {}): DupeRule => ({
  byCall: true,
  byBand: true,
  byModeClass: true,
  byFields: [],
  bySentFields: [],
  modeClassGroups: [],
  satelliteIsABand: false,
  fmSatelliteOnce: false,
  logDupes: true,
  ...over,
})

const row = (call: string, band: string, mode: string): FieldDayQso =>
  ({ call, class: '', section: '', band, mode }) as FieldDayQso

/** A contest with W1AW worked on 20 m CW, under a per-band, per-mode dupe rule. */
const contest = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({ running: true, log: [row('W1AW', '20m', 'CW')], dupeRule: rule(), ...over }) as unknown as FieldDayStatus

const spot = (call: string, over: Partial<SpotRow> = {}): SpotRow =>
  ({
    call,
    entity: 'United States',
    zone: 5,
    state: null,
    band: '20m',
    freqMhz: 14.025,
    mode: 'CW',
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 60,
    comment: '',
    licensed: true,
    spotterLocal: true,
    ...over,
  }) as SpotRow

const cook = [{ id: 'county', value: 'COOK' }]

describe('contestSpotMarker', () => {
  it('marks a station worked on this band and mode a dupe, and gives it no multiplier', () => {
    const mark = contestSpotMarker(contest())!
    expect(mark(spot('W1AW', { contestMults: cook }))).toEqual({ dupe: true, mults: [] })
  })

  it('does not mark a station worked on ANOTHER band under a per-band rule', () => {
    const mark = contestSpotMarker(contest())!
    expect(mark(spot('W1AW', { band: '40m', freqMhz: 7.025 }))).toBe(null)
  })

  it('marks it a dupe on another band when the rule works a station once (Sweepstakes)', () => {
    const mark = contestSpotMarker(contest({ dupeRule: rule({ byBand: false, byModeClass: false }) }))!
    expect(mark(spot('W1AW', { band: '40m', freqMhz: 7.025, mode: 'Phone' }))?.dupe).toBe(true)
  })

  it('asks in the spot\'s mode class: the same station on phone is not a CW dupe', () => {
    const mark = contestSpotMarker(contest())!
    expect(mark(spot('W1AW', { mode: 'Phone', freqMhz: 14.25 }))).toBe(null)
  })

  it('carries the multipliers the station computed for a station that is not a dupe', () => {
    const mark = contestSpotMarker(contest())!
    expect(mark(spot('K9AAA', { contestMults: cook }))).toEqual({ dupe: false, mults: cook })
    expect(spotMarkWords(mark(spot('K9AAA', { contestMults: cook })))).toEqual(['new mult: county COOK'])
  })

  it('marks nothing outside a contest', () => {
    expect(contestSpotMarker(null)).toBe(null)
    expect(contestSpotMarker(undefined)).toBe(null)
  })

  it('maps the spot mode classes onto the contest log\'s', () => {
    expect(['CW', 'Phone', 'Digital'].map(spotContestClass)).toEqual(['CW', 'PH', 'DIG'])
  })

  // ⚡ A busy band map against a big log, on every snapshot: hundreds of spots, 5,000 contacts.
  it('marks 600 spots against a 5,000-contact log in well under a frame budget', () => {
    const log = Array.from({ length: 5000 }, (_, i) => row(`K${i}AA`, i % 2 ? '20m' : '40m', 'CW'))
    const spots = Array.from({ length: 600 }, (_, i) => spot(`K${i * 7}AA`, { freqMhz: 14.0 + i / 10_000 }))
    const t0 = performance.now()
    const mark = contestSpotMarker(contest({ log }))!
    const marked = spots.filter((s) => mark(s)?.dupe).length
    const ms = performance.now() - t0
    // K(7i)AA is on 20 m exactly when 7i is odd, i.e. when i is odd: 300 of the 600.
    expect(marked).toBe(300)
    expect(ms).toBeLessThan(50)
  })
})

describe('the band strip', () => {
  const strip = (fieldDay?: FieldDayStatus | null) =>
    render(
      <BandStrip
        band="20m"
        dialMhz={14.03}
        txAllowed
        spotMode="CW"
        spots={[spot('W1AW', { freqMhz: 14.02 }), spot('K9AAA', { freqMhz: 14.04, contestMults: cook })]}
        onWorkSpot={() => {}}
        fieldDay={fieldDay}
      />,
    )

  it('strikes a dupe and tags a new multiplier, in the tooltip and the accessible name', () => {
    strip(contest())
    const dupe = screen.getByRole('button', { name: 'W1AW · dupe' })
    expect(dupe.className).toContain('contest-dupe')
    expect(dupe.getAttribute('title')).toContain('W1AW · dupe')
    const mult = screen.getByRole('button', { name: 'K9AAA · new mult: county COOK' })
    expect(mult.className).not.toContain('contest-dupe')
    expect(mult.getAttribute('title')).toContain('new mult: county COOK')
    expect(within(mult).getByText('MULT')).toBeTruthy()
    expect(within(dupe).queryByText('MULT')).toBe(null)
  })

  it('marks nothing outside a contest', () => {
    strip(null)
    expect(screen.queryByText('MULT')).toBe(null)
    expect(document.querySelector('.contest-dupe')).toBe(null)
    expect(screen.queryByRole('button', { name: /dupe|new mult/ })).toBe(null)
  })
})

describe('the band map', () => {
  it('strikes a dupe and tags a new multiplier, and marks nothing outside a contest', () => {
    const map = (fieldDay?: FieldDayStatus | null) =>
      render(
        <BandMap
          band="20m"
          dialMhz={14.03}
          txAllowed
          spotMode="CW"
          spots={[spot('W1AW', { freqMhz: 14.02 }), spot('K9AAA', { freqMhz: 14.04, contestMults: cook })]}
          onWorkSpot={() => {}}
          fieldDay={fieldDay}
        />,
      )
    map(contest())
    expect(screen.getByRole('button', { name: 'W1AW · dupe' }).className).toContain('contest-dupe')
    const mult = screen.getByRole('button', { name: 'K9AAA · new mult: county COOK' })
    expect(within(mult).getByText('MULT')).toBeTruthy()
    cleanup()
    map(undefined)
    expect(screen.queryByText('MULT')).toBe(null)
    expect(document.querySelector('.contest-dupe')).toBe(null)
  })
})

describe('the spots list', () => {
  const list = (fieldDay?: FieldDayStatus | null) =>
    render(
      <SpotsPanel
        spots={[spot('W1AW'), spot('K9AAA', { freqMhz: 14.04, contestMults: cook })]}
        bandPlan={[]}
        selectedCall={null}
        onSelect={() => {}}
        onWork={() => {}}
        fieldDay={fieldDay}
      />,
    )

  it('says DUPE on a dupe row and MULT on a new-multiplier row, with the words in the tooltip', () => {
    list(contest())
    const rows = screen.getAllByRole('row').filter((r) => r.classList.contains('sp-row'))
    const w1aw = rows.find((r) => r.textContent?.includes('W1AW'))!
    const k9aaa = rows.find((r) => r.textContent?.includes('K9AAA'))!
    expect(w1aw.className).toContain('contest-dupe')
    expect(within(w1aw).getByText('DUPE')).toBeTruthy()
    expect(w1aw.getAttribute('title')).toMatch(/· dupe$/)
    expect(within(k9aaa).getByText('MULT').getAttribute('title')).toBe('new mult: county COOK')
    expect(k9aaa.getAttribute('title')).toMatch(/· new mult: county COOK$/)
    expect(within(k9aaa).queryByText('DUPE')).toBe(null)
  })

  it('marks nothing outside a contest', () => {
    list(null)
    expect(screen.queryByText('DUPE')).toBe(null)
    expect(screen.queryByText('MULT')).toBe(null)
    expect(document.querySelector('.contest-dupe')).toBe(null)
  })
})
