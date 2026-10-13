import { describe, expect, it } from 'vitest'
import type { FieldDayQso } from '../types'
import {
  WFD_OBJECTIVES,
  wfdLogHints,
  wfdObjectiveMultiplier,
  wfdObjectiveState,
  wfdObjectiveTally,
} from './wfdObjectives'

// The sponsor's own numbers (2027 rules, worksheet p.11): thirteen objectives, OM
// 1,2,3,1,2,3,1,1,6,6,2,4,2.
describe('the Winter Field Day objectives', () => {
  it('are the sponsor\'s thirteen, worth 34 together', () => {
    expect(WFD_OBJECTIVES.map((o) => o.multiplier)).toEqual([1, 2, 3, 1, 2, 3, 1, 1, 6, 6, 2, 4, 2])
    expect(wfdObjectiveMultiplier(WFD_OBJECTIVES.map((o) => o.id))).toBe(34)
  })

  it('count what a ticked objective comes with, once', () => {
    // 100% alternative power (×2) qualifies for station equipment on alternative power (×1),
    // and QRP is ×4: 2 + 1 + 4 = 7.
    expect(wfdObjectiveMultiplier(['wfd-alt-power-100', 'wfd-qrp'])).toBe(7)
    // …ticking the implied one as well does not count it twice.
    expect(wfdObjectiveMultiplier(['wfd-alt-power-100', 'wfd-alt-power-equipment', 'wfd-qrp'])).toBe(7)
    // Twelve bands carries the six: 6 + 6.
    expect(wfdObjectiveMultiplier(['wfd-twelve-bands'])).toBe(12)
  })

  it('score nothing for an ARRL bonus or an unknown id', () => {
    expect(wfdObjectiveMultiplier(['emergency-power', 'satellite', 'not-an-objective'])).toBe(0)
  })

  it('reads an implied objective as earned, and earned over planned', () => {
    expect(wfdObjectiveState('wfd-alt-power-equipment', ['wfd-alt-power-100'], [])).toBe('implied')
    expect(wfdObjectiveState('wfd-qrp', ['wfd-qrp'], ['wfd-qrp'])).toBe('earned')
    expect(wfdObjectiveState('wfd-qrp', [], ['wfd-qrp'])).toBe('planned')
    expect(wfdObjectiveState('wfd-qrp', [], [])).toBe('none')
  })

  it('tallies the chase in OM without counting a planned objective the ticks already earn', () => {
    expect(wfdObjectiveTally(['wfd-alt-power-100'], ['wfd-alt-power-equipment', 'wfd-qrp'])).toEqual({
      earnedCount: 2,
      earnedOm: 3,
      plannedCount: 1,
      plannedOm: 4,
      ceilingOm: 7,
    })
  })
})

const q = (band: string, mode: string, dupe = false): FieldDayQso => ({
  call: 'K1ABC',
  class: '1H',
  section: 'WI',
  band,
  mode,
  dupe,
})

describe('the log hints', () => {
  it('count the bands holding three counting contacts and the mode classes worked', () => {
    const six = ['160m', '80m', '40m', '20m', '15m', '10m'].flatMap((b) => [q(b, 'CW'), q(b, 'PH'), q(b, 'DIG')])
    expect(wfdLogHints(six)).toEqual({ bands: 6, modes: 3 })
  })

  it('leave out a band with two contacts, and a duplicate, which counts for nothing', () => {
    const log = [q('20m', 'CW'), q('20m', 'CW'), q('20m', 'CW', true), q('40m', 'CW')]
    expect(wfdLogHints(log)).toEqual({ bands: 0, modes: 1 })
  })
})

describe('the log hints and satellites', () => {
  it('leave out a satellite contact, which counts for nothing at Winter Field Day', () => {
    const pass = (call: string): FieldDayQso => ({ ...q('2m', 'PH'), call, sat: 'ISS (ZARYA)' })
    expect(wfdLogHints([pass('K1ABC'), pass('W1AW'), pass('N0XYZ')])).toEqual({ bands: 0, modes: 0 })
  })
})

describe('the log hints and the WSJT modes', () => {
  it('leave out a contact the station marked as in a banned mode, which counts for nothing', () => {
    // Three FT8 contacts on 20 m and one CW contact on 40 m: no band holds three counting
    // contacts and one mode class was worked.
    const ft8 = (call: string): FieldDayQso => ({ ...q('20m', 'DIG'), call, submode: 'FT8', banned: true })
    expect(wfdLogHints([ft8('K1ABC'), ft8('W1AW'), ft8('N0XYZ'), q('40m', 'CW')])).toEqual({ bands: 0, modes: 1 })
    // CONTROL: the same three on RTTY are a band and a second mode.
    const rtty = (call: string): FieldDayQso => ({ ...q('20m', 'DIG'), call, submode: 'RTTY' })
    expect(wfdLogHints([rtty('K1ABC'), rtty('W1AW'), rtty('N0XYZ'), q('40m', 'CW')])).toEqual({ bands: 1, modes: 2 })
  })
})
