import { describe, it, expect } from 'vitest'
import { operatingTime, formatOpTime, OFF_TIME_MIN_SEC } from './contestOpTime'

const at = (...mins: number[]) => mins.map((m) => ({ whenUnix: 1_800_000_000 + m * 60 }))

describe('operatingTime', () => {
  it('counts a break of 30 minutes or more as off time, the rest from first to last as on time', () => {
    // 0 → 10 → 40 (a 30-minute gap: off) → 69 (29 minutes: on) → 169 (100 minutes: off)
    expect(operatingTime(at(0, 10, 40, 69, 169))).toEqual({ onSec: (10 + 29) * 60, offSec: (30 + 100) * 60, breaks: 2 })
  })

  it('reads the contacts in time order, whatever order the log holds them in', () => {
    expect(operatingTime(at(40, 0, 10))).toEqual({ onSec: 10 * 60, offSec: 30 * 60, breaks: 1 })
  })

  it('takes a contest definition’s own minimum break', () => {
    expect(operatingTime(at(0, 40, 99), 60 * 60)).toEqual({ onSec: 99 * 60, offSec: 0, breaks: 0 })
    expect(operatingTime(at(0, 40, 100), 60 * 60)).toEqual({ onSec: 40 * 60, offSec: 60 * 60, breaks: 1 })
    expect(OFF_TIME_MIN_SEC).toBe(30 * 60)
  })

  it('is nothing before the first contact, and no time at all with one', () => {
    expect(operatingTime([])).toBeNull()
    expect(operatingTime([{ whenUnix: 0 }, {}])).toBeNull()
    expect(operatingTime(at(5))).toEqual({ onSec: 0, offSec: 0, breaks: 0 })
  })

  it('leaves out a row with no time', () => {
    expect(operatingTime([...at(0, 10), { whenUnix: 0 }])).toEqual({ onSec: 600, offSec: 0, breaks: 0 })
  })
})

describe('formatOpTime', () => {
  it('writes hours and minutes', () => {
    expect(formatOpTime(0)).toBe('0:00')
    expect(formatOpTime(59)).toBe('0:00')
    expect(formatOpTime(65 * 60)).toBe('1:05')
    expect(formatOpTime(36 * 3600 + 7 * 60)).toBe('36:07')
  })
})
