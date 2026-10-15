import type { BandChannel, Js8State } from '../types'
import type { QueryPage } from './application-query-protocol'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'

const integer = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const text = (v: unknown, max = 1024): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= max && !/[\uD800-\uDFFF]/u.test(v)
const bool = (v: unknown) => typeof v === 'boolean'
const nullableTime = (v: unknown) => v === null || integer(v)
const speed = (v: unknown) => ['slow','normal','fast','turbo'].includes(String(v))
// `aprsRelay`: an APRS-IS message the station relays to a heard JS8 station (off by default). A page
// without it refuses a sample whose queue holds one, so this page must be deployed before a
// station that sends it.
const origin = (v: unknown) => ['operator','heartbeat','hbAck','autoReply','relay','cqRepeat','aprsRelay'].includes(String(v))
const unarmed = (v: unknown) => ['idle','txOff','needsHb','needsAuto','needsHbAndAuto','unscheduled'].includes(String(v))
function object(v: unknown, keys: string[], optional: string[] = []): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || !keys.every(k => Object.prototype.hasOwnProperty.call(v,k)) ||
    Object.keys(v).some(k => !keys.includes(k) && !optional.includes(k))) throw new Error('invalidJs8')
  return v as Record<string,unknown>
}
function rows(v: unknown, max: number): unknown[] {
  if (!Array.isArray(v) || v.length > max) throw new Error('invalidJs8')
  return v
}
// Clock provenance belongs to this observation, outside the native DTO. Rewriting
// activity timestamps changes React row identities on every poll and loses the
// operator's scroll anchor. Native UTC labels and event identities stay exact.
const displayClocks = new WeakMap<Js8State, { stationAt: number; receivedAt: number }>()
export function js8DisplayNow(state: Js8State | null, browserNow = Date.now()): number {
  const clock = state && displayClocks.get(state)
  return clock ? clock.stationAt + Math.max(0, browserNow - clock.receivedAt) : browserNow
}
/** Validate the native display DTO and retain conservative display-clock provenance.
 * Transport age advances relative ages/countdowns without rewriting event timestamps. */
export function parseJs8Sample(raw: unknown, ageMs: number, now = Date.now()): Js8State {
  const sample = object(raw,['state','capturedAtMs'])
  if (!integer(sample.capturedAtMs) || !finite(ageMs) || ageMs < 0 || ageMs >= APPLICATION_TIMEOUT_MS ||
    new TextEncoder().encode(JSON.stringify(raw)).length > 384*1024) throw new Error('invalidJs8')
  const s = object(sample.state,['speed','rxSpeeds','txEnabled','sending','hbOn','hbNextAtMs','hbIntervalMin','cqOn','cqNextAtMs','cqIntervalMin',
    'autoreply','relay','hbAck','armed','idleMinutes','idleLimitMin','idleTripped','activity','stations','inbox','queue','pendingReply','lastError'],['notArmed'])
  if (!speed(s.speed) || !integer(s.rxSpeeds) || s.rxSpeeds > 15 ||
    ![s.txEnabled,s.sending,s.hbOn,s.cqOn,s.autoreply,s.relay,s.hbAck,s.idleTripped].every(bool) ||
    ![s.hbNextAtMs,s.cqNextAtMs].every(nullableTime) || ![s.hbIntervalMin,s.cqIntervalMin,s.idleMinutes,s.idleLimitMin].every(integer) ||
    (s.lastError !== null && !text(s.lastError))) throw new Error('invalidJs8')
  // `notArmed`, the station's reason a switch that is on is not armed, is OPTIONAL. An older station
  // does not send it, and its `armed` is the switch with TX on and no idle trip. One that sends it
  // arms a switch only with no reason against it, gives a reason only to a switch that is on, and
  // never arms one with TX off or the idle watchdog standing. An older page refuses such a sample
  // whole, so this page must be deployed before a station that sends it.
  const armed = object(s.armed,['autoreply','relay','hbAck','hb','cq'])
  const notArmed = s.notArmed === undefined ? null : object(s.notArmed,['autoreply','relay','hbAck','hb','cq'])
  if (!Object.values(armed).every(bool) || (notArmed && !Object.values(notArmed).every(v => v === null || unarmed(v))) ||
    Object.entries({autoreply:s.autoreply,relay:s.relay,hbAck:s.hbAck,hb:s.hbOn,cq:s.cqOn}).some(([key,on]) => notArmed
      ? armed[key] !== (on === true && notArmed[key] === null) || (on !== true && notArmed[key] !== null) ||
        (armed[key] === true && (s.txEnabled !== true || s.idleTripped !== false))
      : armed[key] !== (on === true && s.txEnabled === true && s.idleTripped === false))) throw new Error('invalidJs8')
  for (const raw of rows(s.activity,200)) {
    const r = object(raw,['atMs','speed','freqHz','snrDb','dtS','from','text','directedToMe','mine','complete','lowConf'])
    if (!integer(r.atMs) || !speed(r.speed) || ![r.freqHz,r.snrDb,r.dtS].every(finite) || !text(r.from) || !text(r.text) ||
      ![r.directedToMe,r.mine,r.complete,r.lowConf].every(bool)) throw new Error('invalidJs8')
  }
  const calls = new Set<string>()
  for (const raw of rows(s.stations,500)) {
    const r = object(raw,['call','grid','snrDb','freqHz','speed','lastMs','lastHb','lastCq','storedMsgs'])
    if (!text(r.call,32) || !r.call || calls.has(r.call) || (r.grid !== null && !text(r.grid)) || ![r.snrDb,r.freqHz].every(finite) ||
      !speed(r.speed) || !integer(r.lastMs) || !bool(r.lastHb) || !bool(r.lastCq) || !integer(r.storedMsgs)) throw new Error('invalidJs8')
    calls.add(r.call)
  }
  const ids = new Set<number>()
  for (const raw of rows(s.inbox,100)) {
    const r = object(raw,['id','from','to','text','path','state','atMs','freqHz','snrDb'])
    if (!integer(r.id) || ids.has(r.id) || ![r.from,r.to,r.text].every(v => text(v)) || !rows(r.path,8).every(v => text(v)) ||
      !['unread','read','store','delivered'].includes(String(r.state)) || !integer(r.atMs) || ![r.freqHz,r.snrDb].every(finite)) throw new Error('invalidJs8')
    ids.add(r.id)
  }
  for (const raw of rows(s.queue,2048)) {
    const r = object(raw,['origin','display','first','last'])
    if (!origin(r.origin) || !text(r.display) || !bool(r.first) || !bool(r.last)) throw new Error('invalidJs8')
  }
  if (s.pendingReply !== null) {
    const p = object(s.pendingReply,['origin','to','display','firesAtMs'])
    if (!origin(p.origin) || !text(p.to) || !text(p.display) || !integer(p.firesAtMs)) throw new Error('invalidJs8')
  }
  const state = structuredClone(sample.state) as Js8State
  displayClocks.set(state, { stationAt: sample.capturedAtMs + ageMs, receivedAt: now })
  return state
}
export type Js8HistoryDetail = { count: number; lastUnix: number | null; grid: string; name: string; comment: string }
export type Js8Context = { plan: BandChannel[]; history: Record<string,Js8HistoryDetail>; capturedAgeMs: number }
export function parseJs8Context(page: QueryPage): Js8Context {
  const meta = object(page.meta,['capturedAgeMs','source'])
  if (page.collection !== 'js8Context' || page.rows.length || page.total || page.retained || page.offset || page.nextCursor !== null ||
    !integer(meta.capturedAgeMs) || meta.capturedAgeMs >= 60_000) throw new Error('invalidJs8')
  const value = object(meta.source,['plan','history'])
  if (new TextEncoder().encode(JSON.stringify(value)).length > 192*1024) throw new Error('invalidJs8')
  for (const raw of rows(value.plan,64)) {
    const c = object(raw,['band','group','dialMhz','mode','label','note','tx'])
    if (![c.band,c.group,c.mode,c.label,c.note].every(v => text(v)) || !finite(c.dialMhz) || c.dialMhz <= 0 || !bool(c.tx)) throw new Error('invalidJs8')
  }
  if (!value.history || typeof value.history !== 'object' || Array.isArray(value.history) || Object.keys(value.history).length > 500) throw new Error('invalidJs8')
  for (const [call,raw] of Object.entries(value.history)) {
    const h = object(raw,['count','lastUnix','grid','name','comment'])
    if (!text(call,32) || !call || !integer(h.count) || h.count > 1_000_000 || !nullableTime(h.lastUnix) ||
      (h.count === 0) !== (h.lastUnix === null) || ![h.grid,h.name,h.comment].every(v => text(v))) throw new Error('invalidJs8')
  }
  return {...value as unknown as Omit<Js8Context,'capturedAgeMs'>, capturedAgeMs:meta.capturedAgeMs}
}
