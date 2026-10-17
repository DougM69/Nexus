// The room's pipe: a native client's tunnel to its station, passed through unread.
//
// An approved computer opens a socket here (index.ts admits it on its account's token and its
// device key's signature). The room asks the station, over its main socket, to open a second socket
// of its own for that pipe, and the computer's socket opens only once the station's has arrived.
// From then on each binary message is passed from one end to the other, unread and in order, with no
// checkpoint per message. What runs inside is the two ends' own business - TLS 1.3 between the
// station's key and the device's - so the room only ever holds ciphertext.
//
// THE STATION'S MAIN SOCKET IS UNTOUCHED but for one message, `{"type":"pipeOpen","pipeId":…}`, sent
// only to a station that advertised `x-nexus-pipe-version: 1` when it connected. Any other station
// closes its socket on a message it cannot read, and one of 1.13.0 or older then turns Remote off for
// good. The tunnel's bytes never ride that socket, so its message shapes stay exactly as they were.
//
// A pipe lives as long as a browser's lease does: until min(token, approval, entitlement, now + 60 s),
// renewed by `/pipe-renew` past the same checks. A device revoked or approved again, a station
// revoked, or an entitlement that ends closes it at once, from the policy every such change already
// sends the room. Since nobody here can gate what crosses inside it, the account's entitlement is
// also read again as traffic passes, at the stream's own interval, and Remote access switched off by
// hand ends the pipe within about that long, as it ends a stream.
//
// The station's end of a pipe is its own socket in every way: a refusal or a close there is about
// that pipe, never an answer about the station, and the station must never read one as that.
import { observerDeadline } from '../../ui/src/remote-monitor/relay'
import type { BrowserIdentity, Entitlement, StationAccess, StationIdentity } from '../../ui/src/remote-monitor/relay'
import { STREAM_ENTITLEMENT_CHECK_MS } from '../../ui/src/remote-web/stream-relay'
import { readEntitlement, Refusal, requireValue } from './authority'
import type { RemoteEnv } from './authority'

/** Pipes one station carries at once: one per device, and no more than a station's observers. */
export const MAX_PIPES = 4
/** How long a computer's socket waits for the station's end before it is told to try again. */
export const PIPE_OPEN_MS = 10000
/** The largest message either end may send. A TLS record is at most 16 KiB and a few bytes. */
export const PIPE_MESSAGE_BYTES = 65536

type End = 'client' | 'station'
export type PipeAttachment = { version: 1; role: 'pipe'; pipeId: string; end: End
  device: BrowserIdentity; entitlement: Entitlement; until: number; station: StationIdentity }
type Pipe = { id: string; device: BrowserIdentity; entitlement: Entitlement; until: number
  station: StationIdentity | null; client: WebSocket | null; stationEnd: WebSocket | null
  /** When the entitlement was last read while traffic passed. */
  checked: number
  /** Settles the computer's wait for the station's end; null once it is over. */
  arrived: ((ok: boolean) => void) | null }

/** The deadline a browser's session would have, refused under the relay's own code. */
function deadline(access: StationAccess, device: BrowserIdentity, entitlement: Entitlement, now: number): number {
  try { return observerDeadline(access, device, entitlement, now) }
  catch (error) { throw new Refusal(error instanceof Error ? error.message : 'deviceNotApproved') }
}

export class Pipes {
  private pipes = new Map<string, Pipe>()
  private ends = new Map<WebSocket, { pipe: Pipe; end: End }>()

  constructor(private readonly ctx: DurableObjectState, private readonly env: RemoteEnv) {}

  holds(ws: WebSocket): boolean { return this.ends.has(ws) }
  count(): number { return this.pipes.size }

  /** `/client-pipe`, `/station-pipe` and `/pipe-renew`, admitted by index.ts. `station` is the
   *  station's main socket, and whether it advertised the pipe; null while it is not connected. */
  async fetch(path: string, input: { identity: BrowserIdentity & StationIdentity; entitlement: Entitlement; pipeId?: string },
    access: StationAccess, station: { socket: WebSocket; pipe: boolean } | null, now: number): Promise<Response> {
    if (path === '/client-pipe') return this.open(input.identity, input.entitlement, access, station, now)
    if (path === '/station-pipe') return this.attach(input.identity, input.pipeId ?? '', access, now)
    this.renew(input.identity, input.entitlement, access, now)
    return new Response(null, { status: 204 })
  }

  private async open(device: BrowserIdentity, entitlement: Entitlement, access: StationAccess,
    station: { socket: WebSocket; pipe: boolean } | null, now: number): Promise<Response> {
    const until = deadline(access, device, entitlement, now)
    requireValue(station, 'stationOffline', 409)
    requireValue(station.pipe, 'pipeUnsupported', 409)
    // A device's new pipe replaces its old one: it is reconnecting, and the old one is dead to it.
    for (const pipe of [...this.pipes.values()]) if (pipe.device.deviceId === device.deviceId) this.close(pipe, 1000, 'replaced')
    requireValue(this.pipes.size < MAX_PIPES, 'pipeLimit', 409)
    const pipe: Pipe = { id: crypto.randomUUID(), device: { ...device }, entitlement, until, station: null,
      client: null, stationEnd: null, checked: now, arrived: null }
    const arrived = new Promise<boolean>(resolve => {
      const timer = setTimeout(() => resolve(false), PIPE_OPEN_MS)
      pipe.arrived = ok => { clearTimeout(timer); resolve(ok) }
    })
    this.pipes.set(pipe.id, pipe)
    try { station.socket.send(JSON.stringify({ type: 'pipeOpen', pipeId: pipe.id })) }
    catch { pipe.arrived?.(false) }
    const ok = await arrived
    pipe.arrived = null
    // The wait yielded: a revoke, a replacement or the station's end closing may have ended it meanwhile.
    if (!ok || this.pipes.get(pipe.id) !== pipe || !pipe.stationEnd) {
      this.close(pipe, 1011, 'stationUnavailable')
      throw new Refusal('stationUnavailable', 503)
    }
    const pair = new WebSocketPair()
    this.ctx.acceptWebSocket(pair[1])
    this.bind(pipe, 'client', pair[1])
    return new Response(null, { status: 101, webSocket: pair[0] })
  }

  private attach(station: StationIdentity, pipeId: string, access: StationAccess, now: number): Response {
    requireValue(access.enabled && station.accountId === access.accountId && station.stationId === access.stationId &&
      station.generation === access.stationGeneration && station.expiresAt > now, 'stationNotApproved', 401)
    const pipe = this.pipes.get(pipeId)
    requireValue(pipe?.arrived && !pipe.stationEnd, 'pipeUnavailable', 404)
    const pair = new WebSocketPair()
    this.ctx.acceptWebSocket(pair[1])
    pipe.station = { ...station }
    this.bind(pipe, 'station', pair[1])
    pipe.arrived(true)
    return new Response(null, { status: 101, webSocket: pair[0] })
  }

  /** Only after index.ts has read the device, its approval and the entitlement again, as for a
   *  browser's renewal: the same device, approved under the same generation, with both ends open. */
  private renew(device: BrowserIdentity, entitlement: Entitlement, access: StationAccess, now: number): void {
    const pipe = [...this.pipes.values()].find(p => p.device.deviceId === device.deviceId)
    requireValue(pipe?.client && pipe.stationEnd && pipe.device.accountId === device.accountId &&
      pipe.device.deviceGeneration === device.deviceGeneration, 'pipeClosed', 404)
    pipe.until = deadline(access, device, entitlement, now)
    pipe.device = { ...device }
    pipe.entitlement = entitlement
    for (const ws of [pipe.client, pipe.stationEnd]) this.save(pipe, ws === pipe.client ? 'client' : 'station', ws)
  }

  /** Passed on unread, before anything else; then, and never before, the entitlement read again when
   *  it is due, which never holds a message back. */
  async message(ws: WebSocket, message: string | ArrayBuffer, now: number): Promise<void> {
    const held = this.ends.get(ws)
    if (!held) return
    const pipe = held.pipe, other = held.end === 'client' ? pipe.stationEnd : pipe.client
    // Binary only, within bounds, and never before the other end is there: TLS's server never speaks first.
    if (typeof message === 'string' || message.byteLength > PIPE_MESSAGE_BYTES || !other) { this.close(pipe, 1008, 'invalidMessage'); return }
    try { other.send(message) } catch { this.close(pipe, 1011, 'pipeUnavailable'); return }
    if (now - pipe.checked < STREAM_ENTITLEMENT_CHECK_MS) return
    pipe.checked = now
    let entitlement: Entitlement
    // A read that fails changes nothing: the last good reading stands, and the lease still ends it.
    try { entitlement = await readEntitlement(this.env, pipe.device.accountId, now) } catch { return }
    if (this.pipes.get(pipe.id) !== pipe) return
    if (!entitlement.enabled || entitlement.expiresAt <= Date.now()) { this.close(pipe, 1008, 'accessEnded'); return }
    pipe.entitlement = entitlement
    pipe.until = Math.min(pipe.until, entitlement.expiresAt)
  }

  /** One end went, so the pipe goes: the other end is told it closed. */
  closed(ws: WebSocket): void {
    const held = this.ends.get(ws)
    if (held) this.close(held.pipe, 1000, 'pipeClosed')
  }

  /** Every pipe this access and this moment still allow; any other is closed now. */
  enforce(access: StationAccess, now: number): void {
    for (const pipe of [...this.pipes.values()]) {
      const station = pipe.station
      let allowed = pipe.until > now && (!station || (access.enabled && station.accountId === access.accountId &&
        station.stationId === access.stationId && station.generation === access.stationGeneration && station.expiresAt > now))
      if (allowed) try { observerDeadline(access, pipe.device, pipe.entitlement, now) } catch { allowed = false }
      if (!allowed) this.close(pipe, 1008, 'accessEnded')
    }
  }

  nextDeadline(): number | null {
    const deadlines = [...this.pipes.values()].map(pipe => pipe.until)
    return deadlines.length ? Math.min(...deadlines) : null
  }

  /** One end of a pipe, back from a hibernation. `settle()` then closes any that came back alone. */
  restore(ws: WebSocket, saved: PipeAttachment): void {
    let pipe = this.pipes.get(saved.pipeId)
    if (!pipe) {
      // Checked at once: a woken room reads the entitlement again before it passes on much.
      pipe = { id: saved.pipeId, device: saved.device, entitlement: saved.entitlement, until: saved.until, station: saved.station,
        client: null, stationEnd: null, checked: 0, arrived: null }
      this.pipes.set(pipe.id, pipe)
    }
    if (saved.end === 'client') pipe.client = ws
    else pipe.stationEnd = ws
    this.ends.set(ws, { pipe, end: saved.end })
  }
  settle(): void {
    for (const pipe of [...this.pipes.values()]) if (!pipe.client || !pipe.stationEnd) this.close(pipe, 1011, 'pipeUnavailable')
  }

  private bind(pipe: Pipe, end: End, ws: WebSocket): void {
    if (end === 'client') pipe.client = ws
    else pipe.stationEnd = ws
    this.ends.set(ws, { pipe, end })
    this.save(pipe, end, ws)
  }
  private save(pipe: Pipe, end: End, ws: WebSocket | null): void {
    if (!ws || !pipe.station) return
    const saved: PipeAttachment = { version: 1, role: 'pipe', pipeId: pipe.id, end, device: pipe.device,
      entitlement: pipe.entitlement, until: pipe.until, station: pipe.station }
    ws.serializeAttachment(saved)
  }
  private close(pipe: Pipe, code: number, reason: string): void {
    if (this.pipes.get(pipe.id) === pipe) this.pipes.delete(pipe.id)
    for (const ws of [pipe.client, pipe.stationEnd]) {
      if (!ws) continue
      this.ends.delete(ws)
      try { ws.close(code, reason) } catch { /* already closing */ }
    }
    pipe.client = null
    pipe.stationEnd = null
    pipe.arrived?.(false)
  }
}
