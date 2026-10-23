// The Timeline card's lines and bucket names, held by value. The counting itself — which bucket a
// contact falls in, across UTC midnight, a month's end and a leap day — is the engine's, and its
// tests are tempo-core's (`logbook/query/tests.rs`, "the timeline").
import { describe, expect, it } from 'vitest'
import { TIMELINE_LINES, bucketName, tickName, timelineLines } from './logTimeline'
import type { LogTimeline } from '../types'

/** A three-bucket window holding these modes' counts. */
const window3 = (modes: [string, number[]][]): LogTimeline => ({
  edges: [0, 3_600, 7_200, 10_800],
  modes: modes.map(([label, counts]) => ({ label, counts })),
})
const named = (t: LogTimeline) => timelineLines(t).map((l) => [l.mode, l.total])

describe('which modes get a line', () => {
  it('every mode while there are at most six, most-worked first and ties by name', () => {
    expect(TIMELINE_LINES).toBe(6)
    const t = window3([
      ['CW', [1, 0, 0]],
      ['FT8', [5, 2, 1]],
      ['SSB', [0, 3, 0]],
      ['FT4', [1, 1, 1]],
      ['RTTY', [0, 0, 1]],
      ['FM', [0, 1, 0]],
    ])
    expect(named(t)).toEqual([
      ['FT8', 8],
      ['FT4', 3],
      ['SSB', 3],
      ['CW', 1],
      ['FM', 1],
      ['RTTY', 1],
    ])
  })

  it('past six modes, the five most-worked keep a line each and the rest share "Other", bucket by bucket', () => {
    const t = window3([
      ['PSK31', [0, 1, 0]],
      ['FT8', [9, 9, 9]],
      ['CW', [4, 0, 4]],
      ['SSB', [3, 3, 0]],
      ['FT4', [2, 2, 2]],
      ['RTTY', [0, 0, 5]],
      ['JT65', [1, 0, 0]],
      ['FM', [0, 2, 0]],
    ])
    const lines = timelineLines(t)
    expect(lines.map((l) => l.mode)).toEqual(['FT8', 'CW', 'FT4', 'SSB', 'RTTY', null])
    // FM 2, JT65 1, PSK31 1.
    expect(lines[5].counts).toEqual([1, 3, 0])
    expect(lines[5].total).toBe(4)
    expect(lines.reduce((s, l) => s + l.total, 0), 'no contact lost to the cut').toBe(56)
  })

  it('one mode is one line, and no mode is no line', () => {
    expect(named(window3([['FT8', [0, 0, 2]]]))).toEqual([['FT8', 2]])
    expect(timelineLines(window3([]))).toEqual([])
  })
})

describe('what a bucket is called: UTC, as the log writes it', () => {
  it('an hour across UTC midnight', () => {
    expect(bucketName('pastDay', 1_791_932_400)).toBe('2026-10-13 23:00Z')
    expect(bucketName('pastDay', 1_791_936_000)).toBe('2026-10-14 00:00Z')
    expect(tickName('pastDay', 1_791_936_000)).toBe('00:00Z')
  })

  it('a day across the end of a month, and a leap day', () => {
    expect(bucketName('past30Days', 1_790_726_400)).toBe('2026-09-30')
    expect(bucketName('past30Days', 1_790_812_800)).toBe('2026-10-01')
    expect(bucketName('past30Days', 1_835_395_200)).toBe('2028-02-29')
    expect(tickName('past30Days', 1_835_395_200)).toBe('02-29')
  })

  it('a month across the end of a year', () => {
    expect(bucketName('pastYear', 1_796_083_200)).toBe('2026-12')
    expect(bucketName('pastYear', 1_798_761_600)).toBe('2027-01')
    expect(tickName('pastYear', 1_798_761_600)).toBe('2027-01')
  })
})
