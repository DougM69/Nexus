// The keys the desktop's JS8 screen gained in 1.19.0, as the station now sends them to the hosted
// page: `presence` (heartbeat networking), `controls` (the waterfall's decode filter, the QSY centre
// and the band schedule's notice), `inboxTooLong` (held messages too long for this speed), each
// activity row's `flags` and each station's `ackMs` and `cqMs` (★ and ☎). Each is taken with the
// values the station sends and refused, by value, with one it never sends; a key the page does not
// list is still refused inside each. A 1.18.0 station sends none of them, and the same page takes
// its sample as it always did: `js8-1.18.0.json` is the fixture as tagged v1.18.0, whose keys that
// release's native Remote test held to that station's own sample.
import { describe, expect, it } from 'vitest'
import fixture from './__fixtures__/js8.json'
import v118 from './__fixtures__/js8-1.18.0.json'
import { parseJs8Sample } from './js8'

type Sample = { state: Record<string, unknown>; capturedAtMs: number }
const current = (): Sample => structuredClone(fixture) as unknown as Sample
/** The sample with one change made to its state. */
const changed = (change: (s: Record<string, any>) => void): Sample => {
  const value = current()
  change(value.state)
  return value
}
const takes = (value: Sample) => parseJs8Sample(value, 0, value.capturedAtMs)
/** A scheduled change waiting out its notice, as the station sends it. */
const hop = { mhz: 7.078, firesAtMs: 1700000030000, description: '40 m evening net', held: false }

describe('JS8 observation: the desktop keys a 1.19.0 station sends', () => {
  it('takes the sample whole, every new key as the station sent it', () => {
    const value = current()
    // The control: the fixture carries every one of them, or this test reads nothing new.
    expect(Object.keys(value.state)).toEqual(expect.arrayContaining(['presence', 'controls', 'inboxTooLong']))
    const s = fixture.state
    expect(s.activity.every((r) => 'flags' in r)).toBe(true)
    expect(s.stations.some((h) => 'ackMs' in h) && s.stations.some((h) => 'cqMs' in h)).toBe(true)
    const state = takes(value)
    expect(state).toEqual(value.state)
    expect(state.presence?.hbNetworking).toBe(true)
    expect(state.controls?.filter).toEqual({ enabled: true, centerHz: 1500, widthHz: 2000 })
    expect(state.inboxTooLong?.[0].id).toBe(2)
    expect(state.activity[0].flags).toEqual({ first: true, last: true, assembled: false })
    expect(state.stations.map((h) => [h.call, h.ackMs ?? null, h.cqMs ?? null])).toEqual([
      ['W1AW', null, 1700000000000],
      ['K2ABC', 1700000000000, null],
    ])
  })

  it('takes a 1.18.0 station, which sends none of them, as it sends it', () => {
    // Exactly the keys v1.18.0's Js8State serialises (dto.rs at the tag): no reasons, no notes.
    expect(Object.keys(v118.state)).toEqual(['speed', 'rxSpeeds', 'txEnabled', 'sending', 'hbOn', 'hbNextAtMs', 'hbIntervalMin',
      'cqOn', 'cqNextAtMs', 'cqIntervalMin', 'autoreply', 'relay', 'hbAck', 'armed', 'idleMinutes', 'idleLimitMin', 'idleTripped',
      'activity', 'stations', 'inbox', 'queue', 'pendingReply', 'lastError'])
    expect(v118.state.activity.some((r) => 'flags' in r)).toBe(false)
    expect(v118.state.stations.some((h) => 'ackMs' in h || 'cqMs' in h)).toBe(false)
    const value = structuredClone(v118)
    const state = parseJs8Sample(value, 0, value.capturedAtMs)
    expect(state).toEqual(v118.state)
    expect([state.presence, state.controls, state.inboxTooLong, state.notArmed]).toEqual([undefined, undefined, undefined, undefined])
  })

  it.each([
    ['an HB switch that is not a boolean', (s: Record<string, any>) => { s.presence.hbNetworking = 'on' }],
    ['the heartbeat speed as a number', (s: Record<string, any>) => { s.presence.hbSpeed = 1 }],
    ['the QSO pause as null', (s: Record<string, any>) => { s.presence.qsoPause = null }],
    ['a reason HB ACK is held that the station does not give', (s: Record<string, any>) => { s.presence.hbAckSuppressed = 'busy' }],
    ['a reason in an array', (s: Record<string, any>) => { s.presence.hbAckSuppressed = ['speed'] }],
    ['501 stations stopped by the 55-minute rule', (s: Record<string, any>) => {
      s.presence.hbAckRateBlocked = Array.from({ length: 501 }, (_, i) => `K${i}AA`) }],
    ['an empty call stopped by it', (s: Record<string, any>) => { s.presence.hbAckRateBlocked = [''] }],
    ['a 33-byte call stopped by it', (s: Record<string, any>) => { s.presence.hbAckRateBlocked = ['W'.repeat(33)] }],
    ['a call that is not text', (s: Record<string, any>) => { s.presence.hbAckRateBlocked = [7] }],
    ['the stopped calls as one string', (s: Record<string, any>) => { s.presence.hbAckRateBlocked = 'K9XYZ' }],
    ['a key presence does not list', (s: Record<string, any>) => { s.presence.hbAckDue = true }],
    ['presence without a key', (s: Record<string, any>) => { delete s.presence.avoidAllcall }],
    ['presence as null', (s: Record<string, any>) => { s.presence = null }],
  ])('refuses presence with %s', (_, change) => {
    expect(() => takes(changed(change))).toThrow('invalidJs8')
  })

  it('takes every reason the station gives for holding HB ACK back, and the 55-minute rule at its cap', () => {
    for (const why of [null, 'speed', 'arriving', 'qsoPause'])
      expect(takes(changed((s) => { s.presence.hbAckSuppressed = why })).presence?.hbAckSuppressed).toBe(why)
    const full = Array.from({ length: 500 }, (_, i) => `K${i}AA`)
    expect(takes(changed((s) => { s.presence.hbAckRateBlocked = full })).presence?.hbAckRateBlocked).toEqual(full)
    expect(takes(changed((s) => { s.presence.hbAckRateBlocked = ['W'.repeat(32)] })).presence?.hbAckRateBlocked).toEqual(['W'.repeat(32)])
  })

  it.each([
    ['the filter switch as text', (s: Record<string, any>) => { s.controls.filter.enabled = 'true' }],
    ['a filter centre past 65535 Hz', (s: Record<string, any>) => { s.controls.filter.centerHz = 65536 }],
    ['a negative filter width', (s: Record<string, any>) => { s.controls.filter.widthHz = -1 }],
    ['a fractional filter centre', (s: Record<string, any>) => { s.controls.filter.centerHz = 1500.5 }],
    ['a QSY centre past 65535 Hz', (s: Record<string, any>) => { s.controls.qsyCenterHz = 70000 }],
    ['a key the filter does not list', (s: Record<string, any>) => { s.controls.filter.lowHz = 500 }],
    ['the controls without the schedule notice', (s: Record<string, any>) => { delete s.controls.hop }],
    ['a key the controls do not list', (s: Record<string, any>) => { s.controls.bandHop = true }],
    ['a schedule change to a negative dial', (s: Record<string, any>) => { s.controls.hop = { ...hop, mhz: -7.078 } }],
    ['a schedule change to a dial that is text', (s: Record<string, any>) => { s.controls.hop = { ...hop, mhz: '7.078' } }],
    ['a schedule change firing at a negative time', (s: Record<string, any>) => { s.controls.hop = { ...hop, firesAtMs: -1 } }],
    ['a schedule description over 1024 bytes', (s: Record<string, any>) => { s.controls.hop = { ...hop, description: 'x'.repeat(1025) } }],
    ['a schedule change held as text', (s: Record<string, any>) => { s.controls.hop = { ...hop, held: 'no' } }],
    ['a key the schedule notice does not list', (s: Record<string, any>) => { s.controls.hop = { ...hop, band: '40m' } }],
    ['the controls as null', (s: Record<string, any>) => { s.controls = null }],
  ])('refuses the controls with %s', (_, change) => {
    expect(() => takes(changed(change))).toThrow('invalidJs8')
  })

  it('takes the controls at their limits, and a schedule change waiting or held', () => {
    expect(takes(changed((s) => { Object.assign(s.controls.filter, { centerHz: 0, widthHz: 65535 }); s.controls.qsyCenterHz = 65535 })).controls)
      .toEqual({ filter: { enabled: true, centerHz: 0, widthHz: 65535 }, qsyCenterHz: 65535, hop: null })
    for (const notice of [hop, { ...hop, firesAtMs: 0, held: true, description: '' }, { ...hop, mhz: 0 }])
      expect(takes(changed((s) => { s.controls.hop = notice })).controls?.hop).toEqual(notice)
  })

  it.each([
    ['a speed it fits at that is not a speed', (s: Record<string, any>) => { s.inboxTooLong[0].fitsAt = 'sixty' }],
    ['a negative id', (s: Record<string, any>) => { s.inboxTooLong[0].id = -1 }],
    ['the frames as text', (s: Record<string, any>) => { s.inboxTooLong[0].frames = '45' }],
    ['a fractional cap', (s: Record<string, any>) => { s.inboxTooLong[0].max = 39.5 }],
    ['a key the note does not list', (s: Record<string, any>) => { s.inboxTooLong[0].to = 'K2ABC' }],
    ['a note without its cap', (s: Record<string, any>) => { delete s.inboxTooLong[0].max }],
    ['101 notes', (s: Record<string, any>) => { s.inboxTooLong = Array.from({ length: 101 }, () => s.inboxTooLong[0]) }],
    ['the notes as one object', (s: Record<string, any>) => { s.inboxTooLong = s.inboxTooLong[0] }],
  ])('refuses the too-long notes with %s', (_, change) => {
    expect(() => takes(changed(change))).toThrow('invalidJs8')
  })

  it('takes a note that fits at no speed, one that fits at JS8 60, and a hundred', () => {
    for (const fitsAt of [null, 'ultra'])
      expect(takes(changed((s) => { s.inboxTooLong[0].fitsAt = fitsAt })).inboxTooLong?.[0].fitsAt).toBe(fitsAt)
    expect(takes(changed((s) => { s.inboxTooLong = Array.from({ length: 100 }, () => s.inboxTooLong[0]) })).inboxTooLong).toHaveLength(100)
  })

  it.each([
    ['a row whose first-frame bit is text', (s: Record<string, any>) => { s.activity[0].flags.first = 'yes' }],
    ['a row whose place lacks a bit', (s: Record<string, any>) => { delete s.activity[0].flags.assembled }],
    ['a key a row’s place does not list', (s: Record<string, any>) => { s.activity[0].flags.block = 3 }],
    ['a row placed as null', (s: Record<string, any>) => { s.activity[0].flags = null }],
    ['one row that does not say where it stands', (s: Record<string, any>) => { delete s.activity[1].flags }],
  ])('refuses the activity with %s', (_, change) => {
    expect(() => takes(changed(change))).toThrow('invalidJs8')
  })

  it('takes a feed whose rows all say where they stand, or none does', () => {
    const none = takes(changed((s) => { for (const r of s.activity) delete r.flags }))
    expect(none.activity.some((r) => r.flags !== undefined)).toBe(false)
    const assembled = { first: false, last: false, assembled: true }
    expect(takes(changed((s) => { s.activity[3].flags = assembled })).activity[3].flags).toEqual(assembled)
  })

  it.each([
    ['a ★ time that is negative', (s: Record<string, any>) => { s.stations[1].ackMs = -1 }],
    ['a ★ time that is a fraction', (s: Record<string, any>) => { s.stations[1].ackMs = 1700000000000.5 }],
    ['a ☎ time as text', (s: Record<string, any>) => { s.stations[0].cqMs = '1700000000000' }],
    ['a ☎ time as null', (s: Record<string, any>) => { s.stations[0].cqMs = null }],
    ['a key a station does not list', (s: Record<string, any>) => { s.stations[0].hbMs = 1 }],
  ])('refuses a station with %s', (_, change) => {
    expect(() => takes(changed(change))).toThrow('invalidJs8')
  })
})

