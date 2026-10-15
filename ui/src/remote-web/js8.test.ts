import { describe, expect, it } from 'vitest'
import fixture from './__fixtures__/js8.json'
import { js8DisplayNow, parseJs8Context, parseJs8Sample } from './js8'
import type { QueryPage } from './application-query-protocol'

/** The recorded sample's reasons, writable as the wire carries them (any reason, or null). */
const why = (s: typeof fixture.state) => s.notArmed as unknown as Record<string, string | null>
/** The sample as an older station sends it: no reasons, `armed` the switch with TX on and no idle trip. */
const older = (s: typeof fixture.state) => { delete (s as Partial<typeof s>).notArmed }

describe('JS8 observation values', () => {
  it('keeps every native display field while accounting for clock difference and transport age', () => {
    const current = structuredClone(fixture)
    const state = parseJs8Sample(current,250,current.capturedAtMs+60_000)
    expect(state).toEqual(current.state)
    const browserNow = current.capturedAtMs + 60_000
    expect(js8DisplayNow(state, browserNow)).toBe(current.capturedAtMs + 250)
    expect(js8DisplayNow(state, browserNow + 1000)).toBe(current.capturedAtMs + 1250)
    expect(js8DisplayNow(state, browserNow - 1000)).toBe(current.capturedAtMs + 250)
    // WAN age/clock jitter must not change native activity row identities.
    const later = parseJs8Sample({...current,capturedAtMs:current.capturedAtMs+500},350,browserNow+600)
    expect(later.activity.map(r => r.atMs)).toEqual(state.activity.map(r => r.atMs))
    expect(js8DisplayNow(later,browserNow+600)).toBe(current.capturedAtMs+850)
    expect(state.inbox[0].text).toBe('STORED REMOTE TEST')
    expect(state.armed).toEqual(current.state.armed)
    expect(current).toEqual(fixture)
  })
  // JS8 60, JS8Call 3.0's fifth speed: a station transmitting at it, or decoding every speed (31),
  // or showing a frame heard at it, is a valid sample; a mask past the five speeds is not.
  it('takes JS8 60 — the speed, a row heard at it, the every-speed mask — and refuses a bit past it', () => {
    const value = structuredClone(fixture)
    Object.assign(value.state, { speed: 'ultra', rxSpeeds: 31 })
    value.state.activity[0].speed = 'ultra'
    const state = parseJs8Sample(value,0)
    expect([state.speed, state.rxSpeeds, state.activity[0].speed]).toEqual(['ultra', 31, 'ultra'])
    for (const change of [
      (s: typeof fixture.state) => { s.rxSpeeds = 32 },
      (s: typeof fixture.state) => { s.speed = 'sixty' },
    ]) {
      const bad = structuredClone(value);change(bad.state)
      expect(() => parseJs8Sample(bad,0)).toThrow()
    }
  })
  it('rejects expired, malformed, oversized and contradictory armed state', () => {
    expect(() => parseJs8Sample(fixture,3000)).toThrow()
    for (const change of [
      (s: typeof fixture.state) => { s.armed.autoreply=true },
      (s: typeof fixture.state) => { s.activity[0].text='x'.repeat(1025) },
      (s: typeof fixture.state) => { s.inbox.push({...s.inbox[0]}) },
      (s: typeof fixture.state) => { s.queue=Array.from({length:2049},()=>s.queue[0]) },
      (s: typeof fixture.state) => { s.stations[0].freqHz=NaN },
      (s: typeof fixture.state) => { why(s).hbAck='needsHb' },
      (s: typeof fixture.state) => { why(s).autoreply=null },
      (s: typeof fixture.state) => { why(s).relay='unknown' },
      (s: typeof fixture.state) => { s.armed.autoreply=true; why(s).autoreply=null },
      (s: typeof fixture.state) => { older(s); s.armed.autoreply=true },
      (s: typeof fixture.state) => { older(s); s.txEnabled=true },
      (s: typeof fixture.state) => { s.armed={...s.armed,unreviewed:true} as typeof s.armed },
    ]) {
      const value=structuredClone(fixture);change(value.state)
      expect(() => parseJs8Sample(value,0)).toThrow()
    }
    expect(() => parseJs8Sample({...fixture,unreviewed:true},0)).toThrow()
  })
  it('takes a switch that is on but not armed, with the station\'s reason, and no armed one with a reason', () => {
    const value = structuredClone(fixture)
    Object.assign(value.state, { txEnabled: true, hbOn: false, cqOn: false, hbAck: true })
    value.state.armed = { autoreply: true, relay: true, hbAck: false, hb: false, cq: false }
    Object.assign(why(value.state), { autoreply: null, relay: null, hbAck: 'needsHb', hb: null, cq: null })
    expect(parseJs8Sample(value,0).notArmed?.hbAck).toBe('needsHb')
    const armedWithReason = structuredClone(value)
    armedWithReason.state.armed.hbAck = true
    expect(() => parseJs8Sample(armedWithReason,0)).toThrow()
  })
  it('still takes an older station, which sends no reasons and arms a switch with TX on', () => {
    const value = structuredClone(fixture)
    older(value.state)
    expect(parseJs8Sample(value,0).notArmed).toBeUndefined()
    Object.assign(value.state, { txEnabled: true })
    value.state.armed = { autoreply: true, relay: true, hbAck: false, hb: true, cq: true }
    expect(parseJs8Sample(value,0).armed).toEqual(value.state.armed)
  })
  it('distinguishes an explicit never-worked answer from absent or expired history', () => {
    const page = {collection:'js8Context',offset:0,total:0,retained:0,rows:[],nextCursor:null,
      meta:{capturedAgeMs:0,source:{plan:[],history:{W1AW:{count:2,lastUnix:1700000000,grid:'',name:'',comment:''},
        K2ABC:{count:0,lastUnix:null,grid:'',name:'',comment:''}}}}} as unknown as QueryPage
    expect(parseJs8Context(page).history.K2ABC.count).toBe(0)
    expect(parseJs8Context(page).history.W1AW.grid).toBe('')
    expect(parseJs8Context(page).history.N0CALL).toBeUndefined()
    expect(() => parseJs8Context({...page,meta:{...(page.meta as object),capturedAgeMs:60_000}})).toThrow()
    expect(() => parseJs8Context({...page,retained:1})).toThrow()
  })
})
