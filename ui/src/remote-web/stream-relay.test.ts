import { expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { StreamRelay, STREAM_RATE_LIMIT, STREAM_RATE_WINDOW_MS } from './stream-relay'
import { STREAM_SDP_BYTES, STREAM_STATION_BYTES, lanCandidate, lanDescription, parseReceivedMessage } from './stream-protocol'

// THE CONTRACT: the same files the station's Rust parsers are tested against.
type Case = { name: string; message: Record<string, unknown> }
const signal = JSON.parse(readFileSync(resolve(process.cwd(), '../remote/test/fixtures/stream/signal.json'), 'utf8')) as Record<string, Case[]>
const session = '10000000-0000-4000-8000-000000000001', device = '10000000-0000-4000-8000-000000000002'
const lease = '10000000-0000-4000-8000-000000000003'
const NOW = 1_000_000
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn<(c: number, r: string) => void>() })
const byName = (list: Case[], name: string) => list.find(c => c.name === name)!.message
const offer = () => structuredClone(byName(signal.browserToRoom, 'offer'))
const candidate = () => structuredClone(byName(signal.browserToRoom, 'candidate (reflexive)'))
const close = () => structuredClone(byName(signal.browserToRoom, 'close'))
function setup({ supported = true, commandUntil = NOW + 45_000 } = {}) {
  const station = peer(), browser = peer(), relay = new StreamRelay()
  relay.sync({ peer: station, supported }, [{ sessionId: session, deviceId: device, peer: browser, commandUntil }])
  return { station, browser, relay }
}
const sentTo = (p: ReturnType<typeof peer>) => JSON.parse(p.send.mock.lastCall![0]) as Record<string, unknown>
const ends = (p: ReturnType<typeof peer>) => p.send.mock.calls.map(([m]) => JSON.parse(m) as Record<string, unknown>).filter(m => m.type === 'streamEnd')

it('hands the station exactly the stamped message the contract names, for every case a page sends', () => {
  expect(signal.browserToRoom).toHaveLength(signal.roomToStation.length)
  for (const [index, sent] of signal.browserToRoom.entries()) {
    const { station, relay } = setup()
    relay.receiveBrowser(session, structuredClone(sent.message), NOW)
    expect(sentTo(station), sent.name).toEqual(signal.roomToStation[index].message)
    // And within the station's frame budget, measured as the JSON sent.
    expect(new TextEncoder().encode(station.send.mock.lastCall![0]).length, sent.name).toBeLessThanOrEqual(STREAM_STATION_BYTES)
  }
})

it('hands the page exactly what the contract names for every station message, with the address stripped', () => {
  expect(signal.stationToRoom).toHaveLength(signal.roomToBrowser.length)
  for (const [index, sent] of signal.stationToRoom.entries()) {
    const { station, browser, relay } = setup()
    relay.receiveStation(structuredClone(sent.message), NOW)
    expect(station.close, sent.name).not.toHaveBeenCalled()
    const forwarded = sentTo(browser)
    expect(forwarded, sent.name).toEqual(signal.roomToBrowser[index].message)
    expect(forwarded.sessionId, sent.name).toBeUndefined()
    expect(() => parseReceivedMessage(forwarded), sent.name).not.toThrow()
  }
})

it('refuses, and closes the page for, every malformed signal the contract lists - never forwarding one', () => {
  // The station's refused cases, as a page would have to send them: without the stamps the relay
  // itself writes. "no deviceId" is not among them - the missing stamp is the relay's to add.
  // With them, the offers whose device key or signature is out of bounds (A5): the station would take
  // them past its parser and answer invalidOffer, and the relay, which checks the same shapes, never
  // hands them over at all.
  const refusedByShape = [...signal.roomToStationRefused.filter(c => c.name !== 'no deviceId'), ...signal.roomToStationOutOfBounds]
  expect(refusedByShape.length).toBeGreaterThanOrEqual(12)
  for (const refused of refusedByShape) {
    const { sessionId: _s, deviceId: _d, ...asSent } = structuredClone(refused.message)
    const { station, browser, relay } = setup()
    relay.receiveBrowser(session, asSent, NOW)
    expect(station.send, refused.name).not.toHaveBeenCalled()
    expect(browser.close, refused.name).toHaveBeenCalledWith(1008, 'invalidStream')
  }
  // A signal carrying an identity of its own is refused rather than merged: the stamping is only
  // load-bearing if nothing can be smuggled past it.
  const smuggled = setup()
  smuggled.relay.receiveBrowser(session, { ...offer(), deviceId: '10000000-0000-4000-8000-00000000000f' }, NOW)
  expect(smuggled.station.send).not.toHaveBeenCalled()
  expect(smuggled.browser.close).toHaveBeenCalledWith(1008, 'invalidStream')
  // Positive control: every case the contract accepts goes through the same relay untouched.
  const good = setup()
  for (const sent of signal.browserToRoom) good.relay.receiveBrowser(session, structuredClone(sent.message), NOW)
  expect(good.station.send).toHaveBeenCalledTimes(signal.browserToRoom.length)
  expect(good.browser.close).not.toHaveBeenCalled()
})

it('bounds an SDP at the contract\'s ceiling in bytes', () => {
  const at = offer(), over = offer()
  const sdp = (at.payload as { sdp: string }).sdp
  ;(at.payload as { sdp: string }).sdp = sdp + 'a=x:' + 'x'.repeat(STREAM_SDP_BYTES - sdp.length - 6) + '\r\n'
  ;(over.payload as { sdp: string }).sdp = (at.payload as { sdp: string }).sdp + 'a'
  expect(new TextEncoder().encode((at.payload as { sdp: string }).sdp).length).toBe(STREAM_SDP_BYTES)
  const refused = setup()
  refused.relay.receiveBrowser(session, over, NOW)
  expect(refused.station.send).not.toHaveBeenCalled()
  // Positive control: exactly at the ceiling is forwarded.
  const allowed = setup()
  allowed.relay.receiveBrowser(session, at, NOW)
  expect(allowed.station.send).toHaveBeenCalledTimes(1)
})

it('tells the page the lane is unavailable rather than handing an old station a message it cannot parse', () => {
  const { station, browser, relay } = setup({ supported: false })
  relay.receiveBrowser(session, offer(), NOW)
  expect(station.send).not.toHaveBeenCalled()
  expect(sentTo(browser)).toEqual(byName(signal.roomToBrowserFromRelay, 'the station does not stream (it never advertised the header)'))
  expect(browser.close).not.toHaveBeenCalled()
})

it('refuses an offer from an account whose command entitlement has lapsed, and only the offer', () => {
  const { station, browser, relay } = setup({ commandUntil: NOW })
  relay.receiveBrowser(session, offer(), NOW)
  expect(station.send).not.toHaveBeenCalled()
  expect(sentTo(browser)).toEqual(byName(signal.roomToBrowserFromRelay, 'command entitlement lapsed'))
  // A close and a candidate still reach the station: ending a stream never needs an entitlement.
  relay.receiveBrowser(session, close(), NOW)
  expect((sentTo(station).payload as { kind: string }).kind).toBe('close')
  // An absent deadline is "not entitled", never "entitled forever". Built by hand: `setup`'s own
  // default would stand in for the missing value and test nothing.
  const missing = { station: peer(), relay: new StreamRelay() }
  missing.relay.sync({ peer: missing.station, supported: true },
    [{ sessionId: session, deviceId: device, peer: peer(), commandUntil: undefined as unknown as number }])
  missing.relay.receiveBrowser(session, offer(), NOW)
  expect(missing.station.send).not.toHaveBeenCalled()
  // Positive control: one millisecond inside the deadline is admitted.
  const live = setup({ commandUntil: NOW + 1 })
  live.relay.receiveBrowser(session, offer(), NOW)
  expect(live.station.send).toHaveBeenCalledTimes(1)
})

it('bounds a looping page: the signal past the budget is refused with tryLater and never forwarded', () => {
  const { station, browser, relay } = setup()
  for (let i = 0; i < STREAM_RATE_LIMIT; i++) relay.receiveBrowser(session, candidate(), NOW + i)
  expect(station.send).toHaveBeenCalledTimes(STREAM_RATE_LIMIT)
  relay.receiveBrowser(session, candidate(), NOW + STREAM_RATE_LIMIT)
  expect(station.send).toHaveBeenCalledTimes(STREAM_RATE_LIMIT)
  expect(sentTo(browser)).toEqual(byName(signal.roomToBrowserFromRelay, 'over the signalling budget'))
  // A close is never budgeted: ending a negotiation is what a looping page should do next.
  relay.receiveBrowser(session, close(), NOW + STREAM_RATE_LIMIT)
  expect(station.send).toHaveBeenCalledTimes(STREAM_RATE_LIMIT + 1)
  // The window rolls: a whole window later the budget is back.
  relay.receiveBrowser(session, candidate(), NOW + STREAM_RATE_WINDOW_MS + STREAM_RATE_LIMIT)
  expect(station.send).toHaveBeenCalledTimes(STREAM_RATE_LIMIT + 2)
})

it('drops a station message for a page that has gone rather than misrouting or holding it', () => {
  const { browser, relay } = setup()
  relay.receiveStation({ ...byName(signal.stationToRoom, 'streaming'), sessionId: '10000000-0000-4000-8000-00000000000e' }, NOW)
  expect(browser.send).not.toHaveBeenCalled()
  expect(relay.dropped).toBe(1)
  // And a page that left the room entirely is forgotten, budget included.
  relay.sync({ peer: peer(), supported: true }, [])
  relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
  expect(browser.send).not.toHaveBeenCalled()
  expect(relay.dropped).toBe(2)
})

it('closes a station that sends what the page would refuse, or signals without having advertised the lane', () => {
  // The page's refused cases, as the station would have sent them: addressed to a session.
  for (const refused of signal.pageRefused) {
    const { station, browser, relay } = setup()
    relay.receiveStation({ ...structuredClone(refused.message), sessionId: session }, NOW)
    expect(station.close, refused.name).toHaveBeenCalledWith(1008, 'invalidStream')
    expect(browser.send, refused.name).not.toHaveBeenCalled()
  }
  const unadvertised = setup({ supported: false })
  unadvertised.relay.receiveStation(byName(signal.stationToRoom, 'answer'), NOW)
  expect(unadvertised.station.close).toHaveBeenCalledWith(1008, 'invalidStream')
  expect(unadvertised.browser.send).not.toHaveBeenCalled()
  // Positive control: the same station, advertised, is forwarded and left open.
  const advertised = setup()
  advertised.relay.receiveStation(byName(signal.stationToRoom, 'answer'), NOW)
  expect(advertised.station.close).not.toHaveBeenCalled()
  expect(advertised.browser.send).toHaveBeenCalledTimes(1)
})

it('tells the page rather than throwing when the station socket cannot take the signal', () => {
  const { station, browser, relay } = setup()
  station.send.mockImplementation(() => { throw Error('gone') })
  relay.receiveBrowser(session, offer(), NOW)
  expect(sentTo(browser)).toEqual(byName(signal.roomToBrowserFromRelay, 'the station does not stream (it never advertised the header)'))
})

it('the fixtures\' own identities are the relay\'s stamps, and the offer\'s lease is the page\'s claim', () => {
  // Guards the harness above: if the fixture ids ever changed, every "equals the contract" check
  // would still pass against a harness stamping the wrong values - so the harness is pinned to them.
  expect(signal.roomToStation[0].message).toMatchObject({ sessionId: session, deviceId: device, leaseId: lease })
})

// A RUNNING STREAM ENDS WHEN ITS SESSION MAY NO LONGER COMMAND. Once up it runs peer to peer, where the
// relay cannot gate it, so the relay tells the station `streamEnd` - the contract's roomToStationEnd.
it('ends a running stream at the station once its session may no longer command: once, and saying why', () => {
  const { station, browser, relay } = setup()
  relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
  expect(relay.streaming(session), 'the station said it is streaming').toBe(true)
  relay.enforce(NOW + 1000)
  expect(ends(station), 'still entitled: nothing is ended').toEqual([])
  // Remote switched off by hand: the room's next reading lapses the deadline, and says so.
  relay.sync({ peer: station, supported: true }, [{ sessionId: session, deviceId: device, peer: browser, commandUntil: 0, accessOff: true }])
  relay.enforce(NOW + 2000)
  expect(ends(station)).toEqual([byName(signal.roomToStationEnd, 'Remote access switched off')])
  relay.enforce(NOW + 3000)
  expect(ends(station), 'once: the station has been told').toHaveLength(1)
  expect(relay.streaming(session)).toBe(false)
})

it('an entitlement that ran out ends a stream as serviceAccessExpired, even one the station admits after it lapsed', () => {
  const lapsing = setup({ commandUntil: NOW + 500 })
  lapsing.relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
  lapsing.relay.enforce(NOW + 499)
  expect(ends(lapsing.station)).toEqual([])
  lapsing.relay.enforce(NOW + 500)
  expect(ends(lapsing.station)).toEqual([byName(signal.roomToStationEnd, 'command entitlement lapsed')])
  // Offered in time, admitted too late: ended the moment the station says it is up.
  const late = setup({ commandUntil: NOW - 1 })
  late.relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
  expect(ends(late.station)).toEqual([byName(signal.roomToStationEnd, 'command entitlement lapsed')])
})

it('does not end a stream the station or the page has already ended, or one of a station that never advertised', () => {
  for (const [what, finish] of [
    ['the station ended it', (r: StreamRelay) => r.receiveStation(byName(signal.stationToRoom, 'refused: streamClosed'), NOW)],
    ['the page closed it', (r: StreamRelay) => r.receiveBrowser(session, close(), NOW)],
  ] as const) {
    const { station, relay } = setup({ commandUntil: NOW + 500 })
    relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
    finish(relay)
    expect(relay.streaming(session), what).toBe(false)
    relay.enforce(NOW + 1000)
    expect(ends(station), what).toEqual([])
  }
  const unadvertised = setup({ supported: false, commandUntil: 0 })
  unadvertised.relay.receiveStation(byName(signal.stationToRoom, 'streaming'), NOW)
  unadvertised.relay.enforce(NOW)
  expect(ends(unadvertised.station)).toEqual([])
})

// A LAN-ONLY SESSION: its stream is held to the shack's own network, by inspection.
it('takes as LAN only a private host candidate or an mDNS name, trickled or in an SDP', () => {
  const line = (address: string, type = 'host') => `candidate:1 1 udp 2122260223 ${address} 54321 typ ${type} generation 0`
  for (const address of ['10.0.0.5', '172.16.0.1', '172.31.255.254', '192.168.1.20', '4d2a8f1e-8c1b-4f3a-9e2d-2c3b4a5d6e7f.local']) {
    expect(lanCandidate(line(address)), address).toBe(true)
    expect(lanCandidate(`a=${line(address)}`), `a=${address}`).toBe(true)
  }
  for (const address of ['198.51.100.23', '172.15.0.1', '172.32.0.1', '100.64.0.1', '192.169.0.1', '11.0.0.1', '127.0.0.1', 'fd00::1',
    '2001:db8::1', '10.0.0', '10.0.0.256', '010.0.0.1', 'example.com', 'x.local.evil'])
    expect(lanCandidate(line(address)), address).toBe(false)
  for (const type of ['srflx', 'prflx', 'relay']) expect(lanCandidate(line('192.168.1.20', type)), type).toBe(false)
  expect(lanCandidate('candidate:1 1 udp 1 192.168.1.20 1 host')).toBe(false)
  const sdp = 'v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\na=mid:0\r\n'
  expect(lanDescription(sdp)).toBe(true)
  expect(lanDescription(`${sdp}a=${line('192.168.1.20')}\r\n`)).toBe(true)
  expect(lanDescription(`${sdp}a=${line('192.168.1.20')}\r\na=${line('203.0.113.7', 'srflx')}\r\n`)).toBe(false)
})

function lanSetup({ lanVersion = 1, lanUntil = NOW + 45_000 } = {}) {
  const station = peer(), browser = peer(), relay = new StreamRelay()
  relay.sync({ peer: station, supported: true, lanVersion }, [{ sessionId: session, deviceId: device, peer: browser, commandUntil: 0, lanUntil }])
  return { station, browser, relay }
}
const mdns = () => structuredClone(byName(signal.browserToRoom, 'candidate (mDNS host; the station ignores what it cannot resolve)'))

it('forwards a LAN-only session\'s offer and host candidates with lanOnly, on its own deadline, and refuses the rest', () => {
  const { station, browser, relay } = lanSetup()
  relay.receiveBrowser(session, offer(), NOW)
  expect(sentTo(station)).toEqual({ ...signal.roomToStation[0].message, lanOnly: true })
  relay.receiveBrowser(session, mdns(), NOW)
  expect(sentTo(station)).toMatchObject({ lanOnly: true, payload: { kind: 'candidate' } })
  station.send.mockClear()
  relay.receiveBrowser(session, candidate(), NOW)
  expect(station.send).not.toHaveBeenCalled()
  expect(sentTo(browser)).toEqual({ type: 'streamState', streaming: false, reason: 'lanOnly' })
  // An offer whose SDP names a reflexive candidate is refused whole: it is signed, never edited.
  const inline = offer() as { payload: { sdp: string } }
  inline.payload.sdp += 'a=candidate:1 1 udp 1 203.0.113.7 61000 typ srflx raddr 0.0.0.0 rport 0\r\n'
  relay.receiveBrowser(session, inline, NOW)
  expect(station.send).not.toHaveBeenCalled()
  // A close always goes.
  relay.receiveBrowser(session, close(), NOW)
  expect(sentTo(station)).toMatchObject({ payload: { kind: 'close' }, lanOnly: true })
  // Its deadline is lanUntil: past it, the offer is refused as any lapsed one is.
  const late = lanSetup({ lanUntil: NOW })
  late.relay.receiveBrowser(session, offer(), NOW)
  expect(late.station.send).not.toHaveBeenCalled()
  expect(sentTo(late.browser)).toEqual({ type: 'streamState', streaming: false, reason: 'serviceAccessExpired' })
})

it('never hands a LAN-only session to a station that did not advertise the LAN version, and an internet one never carries lanOnly', () => {
  const { station, browser, relay } = lanSetup({ lanVersion: 0 })
  relay.receiveBrowser(session, offer(), NOW)
  expect(station.send).not.toHaveBeenCalled()
  expect(sentTo(browser)).toEqual({ type: 'streamState', streaming: false, reason: 'updateStation' })
  const internet = setup()
  internet.relay.sync({ peer: internet.station, supported: true, lanVersion: 1 }, [{ sessionId: session, deviceId: device, peer: internet.browser, commandUntil: NOW + 45_000 }])
  internet.relay.receiveBrowser(session, candidate(), NOW)
  expect(sentTo(internet.station)).toEqual(signal.roomToStation[2].message)
})

it('holds the station\'s end of a LAN-only session to the LAN, and ends its stream only on its own deadline', () => {
  const { station, browser, relay } = lanSetup()
  const stationCase = (name: string) => ({ ...structuredClone(byName(signal.stationToRoom, name)), sessionId: session })
  relay.receiveStation(stationCase('candidate (host: the shack\'s own LAN address, for a browser on its network)'), NOW)
  expect(sentTo(browser)).toMatchObject({ type: 'streamSignal', payload: { kind: 'candidate' } })
  relay.receiveStation(stationCase('candidate (reflexive)'), NOW)
  expect(sentTo(browser)).toEqual({ type: 'streamState', streaming: false, reason: 'lanOnly' })
  expect(relay.dropped).toBe(1)
  relay.receiveStation(stationCase('streaming'), NOW)
  expect(relay.streaming(session)).toBe(true)
  // commandUntil is 0 (an ended trial), and the stream runs: lanUntil is what it is held to.
  relay.enforce(NOW + 1)
  expect(ends(station)).toEqual([])
  relay.enforce(NOW + 45_000)
  expect(ends(station)).toEqual([{ type: 'streamEnd', sessionId: session, reason: 'serviceAccessExpired' }])
})
