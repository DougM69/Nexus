import { describe, expect, it } from 'vitest'
import type { Activation, LoggedQso } from '../types'
import { activationParks, isLegacyUsRef, parkProgress, potaDupe, potaKey } from './potaActivation'

// 2026-10-25 10:40:00Z; the next 00:00Z is 48 000 s later.
const T0 = 1_788_000_000
const MIDNIGHT = T0 + 48_000

const act: Activation = {
  program: 'POTA',
  reference: 'US-1234',
  qsoCount: 1,
  parks: [{ reference: 'US-1234', count: 1, p2p: 0 }],
  utcDay: Math.floor(T0 / 86_400),
}

function qso(over: Partial<LoggedQso> & { whenUnix: number }): LoggedQso {
  return {
    call: 'W1ABC',
    band: '20m',
    mode: 'SSB',
    ota: { myProgram: 'POTA', myRef: 'US-1234' },
    ...over,
  } as LoggedQso
}

const entry = { call: 'W1ABC', band: '20m', mode: 'SSB', nowUnix: T0 + 600 }

describe('potaDupe — POTA’s key: call, band, mode, today, my park (+ theirs)', () => {
  it('same call, band, mode, UTC day and park is a dupe, naming the park and the contact', () => {
    const earlier = qso({ whenUnix: T0 })
    expect(potaDupe([earlier], act, entry)).toEqual({ park: 'US-1234', qso: earlier })
  })

  it('USB is SSB, as the upload check folds it', () => {
    expect(potaDupe([qso({ whenUnix: T0, mode: 'USB' })], act, entry)?.park).toBe('US-1234')
  })

  it('the next UTC day is a new activation day: not a dupe', () => {
    const late = qso({ whenUnix: MIDNIGHT - 30 })
    expect(potaDupe([late], act, { ...entry, nowUnix: MIDNIGHT - 1 })?.park).toBe('US-1234')
    expect(potaDupe([late], act, { ...entry, nowUnix: MIDNIGHT + 30 })).toBeNull()
  })

  it('another park, band, mode, call or hunted park is not a dupe', () => {
    expect(potaDupe([qso({ whenUnix: T0, ota: { myRef: 'US-5678' } })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, band: '40m' })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, mode: 'CW' })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, call: 'W1ABC/P' })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, ota: { myRef: 'US-1234', theirRef: 'US-0099' } })], act, entry)).toBeNull()
    expect(
      potaDupe([qso({ whenUnix: T0, ota: { myRef: 'US-1234', theirRef: 'us-0099' } })], act, { ...entry, theirRef: 'US-0099' })
        ?.park,
    ).toBe('US-1234')
  })

  it('a two-fer contact is a dupe at whichever activation park it shares', () => {
    const twoFer: Activation = { ...act, reference: 'US-1234,US-5678', parks: undefined }
    const earlier = qso({ whenUnix: T0, ota: { myRef: 'US-1234,US-5678' } })
    expect(potaDupe([earlier], twoFer, entry)?.park).toBe('US-1234')
  })

  it('not activating: no POTA dupe at all', () => {
    expect(potaDupe([qso({ whenUnix: T0 })], null, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0 })], { ...act, reference: null, parks: [] }, entry)).toBeNull()
  })
})

describe('the rest of the activator’s arithmetic', () => {
  it('potaKey folds the same way the counter does', () => {
    expect(potaKey(' w1abc ', '20M', 'lsb', ' us-1 ; us-2 ')).toBe(potaKey('W1ABC', '20m', 'SSB', 'US-1,US-2'))
    expect(potaKey('W1ABC/P', '20m', 'SSB', null)).not.toBe(potaKey('W1ABC', '20m', 'SSB', null))
  })

  it('parkProgress: 7 of 10 is 3 to go; 10 is valid', () => {
    expect(parkProgress(7)).toEqual({ count: 7, toGo: 3, valid: false })
    expect(parkProgress(10)).toEqual({ count: 10, toGo: 0, valid: true })
    expect(parkProgress(12)).toEqual({ count: 12, toGo: 0, valid: true })
  })

  it('activationParks reads the two-fer list when the counts have not arrived', () => {
    expect(activationParks({ ...act, reference: 'US-1234,US-5678', parks: undefined })).toEqual(['US-1234', 'US-5678'])
    expect(activationParks(null)).toEqual([])
  })

  it('isLegacyUsRef: K- is the pre-2024 US prefix', () => {
    expect(isLegacyUsRef('k-1234')).toBe(true)
    expect(isLegacyUsRef('US-1234')).toBe(false)
    expect(isLegacyUsRef('KA-1234')).toBe(false)
  })
})
