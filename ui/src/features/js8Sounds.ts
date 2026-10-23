// JS8Call's notification sounds (Settings ▸ Notifications, every one off by default,
// Configuration.cpp:1996-2002, :2464): a sound when a CQ, a heartbeat, an ACK for me, a message
// directed to me, a message for my inbox, a new callsign or a callsign I have worked is heard. JS8Call
// plays a sound file the operator picks for each (`tryNotify`, mainwindow.cpp:5595-5603); Nexus plays
// a tone of its own for each, through the same WebAudio beep its other alerts use (alerts.ts), so
// no file is needed and no two events sound alike.
//
// What is heard is read off the cockpit's polls of the engine (the feed's new frames, the inbox and
// the heard list), as JS8Call's own triggers read its decodes: `tryNotify("cq")` and ("hb") on a
// heartbeat frame (processDecodeEvent.cpp:407, :429), ("directed") on a command addressed to me or a
// group I joined (processCommandActivity.cpp:266-277), ("ack") on an ACK to me (:849), ("inbox") when a
// message is filed for me (:587, :806, :823), ("call_new"/"call_old") when a call first enters the
// call list, by whether the log has it (mainwindow.cpp:2313-2318). The first poll only learns what is
// already there: nothing heard before the screen opened sounds.
import type { Js8ActivityRow, Js8Heard } from '../types'
import { addresseeOf } from './js8Lists'
import { js8BaseCall } from '../js8Vocab'
import { beep, doubleBeep } from '../alerts'

export const JS8_SOUND_EVENTS = ['cq', 'hb', 'ack', 'directed', 'inbox', 'callNew', 'callOld'] as const
export type Js8SoundEvent = (typeof JS8_SOUND_EVENTS)[number]

const rowKey = (r: Js8ActivityRow): string => `${r.atMs}|${r.freqHz}|${r.mine ? 1 : 0}|${r.text}`

/** What one frame announces, if anything. */
export function js8FrameEvent(r: Js8ActivityRow, myCall: string, groups: readonly string[]): Js8SoundEvent | null {
  if (r.mine || !r.from || r.flags?.assembled) return null
  const text = r.text.toUpperCase()
  if (/: @ALLCALL CQ\b/u.test(text)) return 'cq'
  if (/: @HB HEARTBEAT\b/u.test(text)) return 'hb'
  if (!r.directedToMe) return null
  const to = addresseeOf(text)
  const me = myCall.trim().toUpperCase()
  const mine = to.startsWith('@') ? to !== '@ALLCALL' && groups.some((g) => g.trim().toUpperCase() === to) : me !== '' && js8BaseCall(to) === js8BaseCall(me)
  if (!mine) return null
  return /^[^:\s]+:\s+\S+\s+ACK\b/u.test(text) ? 'ack' : 'directed'
}

/**
 * Remembers what the cockpit has already seen, poll to poll, and says what is new. A frame is new
 * once; a call is new once, and is announced only when the log has said whether it is worked
 * (`worked` answers null until then), so the right one of the two sounds plays.
 */
export class Js8SoundWatch {
  private rows: Set<string> | null = null
  private calls: Set<string> | null = null
  private waiting = new Set<string>()

  /** The events among `rows` not seen before; the first call only learns them. */
  frames(rows: readonly Js8ActivityRow[], myCall: string, groups: readonly string[]): Js8SoundEvent[] {
    const keys = rows.map(rowKey)
    const seen = this.rows
    this.rows = new Set(keys)
    if (seen === null) return []
    const out: Js8SoundEvent[] = []
    rows.forEach((r, i) => {
      if (seen.has(keys[i])) return
      const ev = js8FrameEvent(r, myCall, groups)
      if (ev) out.push(ev)
    })
    return out
  }

  /** "callNew" or "callOld" for each call heard for the first time, once the log has answered for it. */
  newCalls(stations: readonly Js8Heard[], worked: (call: string) => boolean | null): Js8SoundEvent[] {
    const now = stations.map((h) => h.call)
    if (this.calls === null) {
      this.calls = new Set(now)
      return []
    }
    for (const c of now) {
      if (!this.calls.has(c)) {
        this.calls.add(c)
        this.waiting.add(c)
      }
    }
    const out: Js8SoundEvent[] = []
    for (const c of [...this.waiting]) {
      const w = worked(c)
      if (w === null) continue
      this.waiting.delete(c)
      out.push(w ? 'callOld' : 'callNew')
    }
    return out
  }
}

/** Each event's tone (Hz), apart from the other alerts' (alerts.ts BEEP_HZ, the 990 Hz alarm, the
 *  660/880 TX pair): a message for me rings twice. */
const TONE: Record<Js8SoundEvent, number> = {
  cq: 587,
  hb: 466,
  ack: 1046,
  directed: 932,
  inbox: 784,
  callNew: 698,
  callOld: 554,
}

/** Play one event's sound. */
export function playJs8Sound(ev: Js8SoundEvent): void {
  if (ev === 'inbox' || ev === 'directed') doubleBeep(TONE[ev])
  else beep(TONE[ev])
}

/** The events of one poll to play, each once, in JS8Call's table order, the enabled ones only. */
export function js8SoundsToPlay(events: readonly Js8SoundEvent[], enabled: readonly string[]): Js8SoundEvent[] {
  return JS8_SOUND_EVENTS.filter((ev) => events.includes(ev) && enabled.includes(ev))
}
