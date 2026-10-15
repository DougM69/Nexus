import { describe, expect, it } from 'vitest'
import {
  hhmmToMinutes,
  js8FilterWindow,
  js8FrequencyFile,
  minutesToHhmm,
  newHop,
  parseJs8FrequencyFile,
  parseMhz,
} from './js8Frequencies'

describe('the JS8 frequency editors’ pure halves', () => {
  it('the filter window is the engine’s: centre ± width/2, inside 100–4000 Hz, at least 50 Hz wide', () => {
    expect(js8FilterWindow({ enabled: true, centerHz: 1500, widthHz: 2000 })).toEqual({ lo: 500, hi: 2500 })
    expect(js8FilterWindow({ enabled: true, centerHz: 120, widthHz: 120 })).toEqual({ lo: 100, hi: 180 })
    expect(js8FilterWindow({ enabled: true, centerHz: 4880, widthHz: 120 })).toEqual({ lo: 3950, hi: 4000 })
    expect(js8FilterWindow({ enabled: true, centerHz: 4880, widthHz: 5000 })).toEqual({ lo: 2380, hi: 4000 })
  })

  it('a dial is a positive finite MHz; a comma reads as the decimal mark', () => {
    expect(parseMhz('7.078')).toBe(7.078)
    expect(parseMhz(' 14,078 ')).toBe(14.078)
    expect([parseMhz(''), parseMhz('0'), parseMhz('-3'), parseMhz('abc'), parseMhz('1e9')]).toEqual([
      null,
      null,
      null,
      null,
      null,
    ])
  })

  it('times of day go to and from minutes after 00:00 UTC', () => {
    expect([minutesToHhmm(0), minutesToHhmm(150), minutesToHhmm(1439)]).toEqual(['00:00', '02:30', '23:59'])
    expect([hhmmToMinutes('02:30'), hhmmToMinutes('23:59'), hhmmToMinutes('24:00'), hhmmToMinutes('7')]).toEqual([
      150,
      1439,
      null,
      null,
    ])
  })

  it('Export’s file reads back as the same rows; a bare list reads too; anything else does not', () => {
    const rows = [
      { mhz: 7.078, description: 'calling' },
      { mhz: 5.363, description: '' },
    ]
    expect(parseJs8FrequencyFile(js8FrequencyFile(rows))).toEqual(rows)
    expect(parseJs8FrequencyFile('[{"mhz": 3.578}]')).toEqual([{ mhz: 3.578, description: '' }])
    expect(parseJs8FrequencyFile('[{"mhz": "10.130", "description": " night "}]')).toEqual([
      { mhz: 10.13, description: 'night' },
    ])
    for (const bad of ['', 'not json', '{}', '[]', '{"frequencies": []}', '[{"mhz": -1}]', '[{"mhz": 7}, {"mhz": "x"}]', '[7.078]']) {
      expect(parseJs8FrequencyFile(bad), bad).toBeNull()
    }
  })

  it('a new schedule row runs from this minute for an hour, across midnight too', () => {
    expect(newHop(7.078, Date.UTC(2026, 9, 15, 13, 5, 30))).toEqual({
      mhz: 7.078,
      fromMin: 785,
      untilMin: 845,
      description: '',
    })
    expect(newHop(3.578, Date.UTC(2026, 9, 15, 23, 30))).toMatchObject({ fromMin: 1410, untilMin: 30 })
  })
})
