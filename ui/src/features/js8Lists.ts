// THE JS8 LISTS AS JS8Call 3.0.3 DRAWS THEM — the RX pane, Band activity, the call list and the
// message inbox, read from JS8Call's source as the specification (behaviour, never its code):
//
//   · the RX pane: the traffic on MY offset plus what is addressed to me, each message on one line
//     as its frames arrive, ♢ after its last frame (processRxActivity.cpp, displayTextForFreq and
//     writeMessageTextToUI in mainwindow.cpp:3106-3240), with JS8Call 3.0's RX block list;
//   · Band activity: one row per offset holding its last ten frames, joined, ♢ after each message's
//     last and …… where a frame went missing (displayBandActivity.cpp, processIdleActivity at
//     mainwindow.cpp:6127-6162), the heartbeat-and-ACK toggle and the highlight lists;
//   · the call list: JS8Call's columns, its one mark per row (✕ ⚑ ★ ☎ ⚟, displayCallActivity.cpp
//     :196-245), its Sort By choices (mainwindow.cpp:4772-4800), @ALLCALL and the joined groups on top;
//   · the inbox: JS8Call's MessagePanel — per-station filter, sort, reply text (MessagePanel.cpp).
//
// Pure: no clock, no storage, no React. The cockpit hands in the engine's rows and the operator's
// choices; everything here is a function of them, which is what the tests pin.
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry, Js8Speed } from '../types'
import { JS8_SPEEDS, bandActivityByOffset, js8BaseCall, js8ShownOffsetRows, js8UnreadFirst } from '../js8Vocab'
import { bearingDeg, gridToLatLon, haversineKm } from '../grid'

// ── shared ────────────────────────────────────────────────────────────────────────────────────

/** JS8Call's `Varicode::rstrip`: the trailing whitespace off. */
export const rstrip = (s: string): string => s.replace(/\s+$/u, '')

/** A last frame's text with JS8Call's end mark: `"%1 %2 "` of the stripped text and the mark
 *  (processRxActivity.cpp:116-123, displayBandActivity.cpp:207-212). */
export function withEot(text: string, eot: string): string {
  return `${rstrip(text)} ${eot} `
}

/** `JS8::Submode::period`, in ms. */
const periodMs = (speed: Js8Speed): number => (JS8_SPEEDS[speed]?.periodS ?? 15) * 1000

/** JS8Call inserts its missing-frame mark once an offset has waited one and a half periods past a
 *  frame that was not a message's last (`secsTo(now) < period * 1.50` keeps waiting,
 *  mainwindow.cpp:6146-6148). */
const missedAfter = (speed: Js8Speed): number => periodMs(speed) * 1.5

/** A speed's name, for a tooltip (the mode's own vocabulary, js8Vocab). */
export const speedName = (speed: Js8Speed): string => JS8_SPEEDS[speed]?.label ?? speed

/** JS8Call's speed column: the name's first letter (S, N, F, T), or the number of a "JS8 40" or
 *  "JS8 60" (displayBandActivity.cpp:251-255, displayCallActivity.cpp:301-305). */
export function speedMark(speed: Js8Speed): string {
  const name = speedName(speed)
  return /^JS8 \d+$/u.test(name) ? name.slice(-2) : name.slice(0, 1)
}

/** A row's offset as JS8Call files it: the decoder's frequency held in an int, so truncated. */
export const offsetOf = (r: { freqHz: number }): number => Math.trunc(r.freqHz)

/** Whether a call, as heard or by its base call, is on a list of calls (JS8Call's
 *  `blocklist.contains(sender) || blocklist.contains(senderBase)`, mainwindow.cpp:3114-3118). */
export function onCallList(list: readonly string[], call: string): boolean {
  const c = call.trim().toUpperCase()
  if (!c || list.length === 0) return false
  const base = js8BaseCall(c)
  return list.some((x) => {
    const e = x.trim().toUpperCase()
    return e !== '' && (e === c || e === base)
  })
}

/** The frames of an activity feed, in the order they were decoded: never the reassembled line of
 *  a multi-frame message, which repeats its frames' words. */
const framesOf = (rows: readonly Js8ActivityRow[]): Js8ActivityRow[] => rows.filter((r) => r.flags && !r.flags.assembled)

/** A feed from a station that does not say where each row stands in its message (one older than
 *  that, on the Remote page): each row is then shown as it stands, on its own. */
export const isLegacyFeed = (rows: readonly Js8ActivityRow[]): boolean => rows.length > 0 && rows.every((r) => r.flags === undefined)

/** The call a frame's text opens with, `"W1AW: …"` → W1AW (`text.section(':', 0, 0)`). */
export function senderOf(text: string): string {
  const i = text.indexOf(':')
  return (i < 0 ? text : text.slice(0, i)).trim().toUpperCase()
}

/** The addressee of a directed frame's text, `"W1AW: KD9TAW SNR? "` → KD9TAW, or '' when the text is
 *  not `CALL: TO…`. */
export function addresseeOf(text: string): string {
  const m = /^[^:\s]+:\s+([@A-Z0-9/]+)/u.exec(text.toUpperCase())
  return m ? m[1] : ''
}

/** Mine, by my call or its base, or one of my joined groups: JS8Call's buffered display for a
 *  message `cmd.to == my_callsign() || isGroupCallIncluded(cmd.to)` (processRxActivity.cpp:39-43). */
function toMeOrMyGroup(to: string, myCall: string, groups: readonly string[]): boolean {
  if (!to) return false
  if (to.startsWith('@')) return groups.some((g) => g.trim().toUpperCase() === to)
  const me = myCall.trim().toUpperCase()
  return me !== '' && (to === me || to === js8BaseCall(me) || js8BaseCall(to) === js8BaseCall(me))
}

/** An @ALLCALL message JS8Call prints in its RX pane: free text only, never a command or a
 *  heartbeat to @ALLCALL ("don't display ping allcalls", processCommandActivity.cpp:224-227). */
const allcallFreeText = (text: string): boolean => /^[^:\s]+:\s+@ALLCALL\s*$/u.test(text.toUpperCase())

// ── the RX pane ───────────────────────────────────────────────────────────────────────────────

/** One line of the RX pane: a message as far as it has come. */
export interface Js8RxLine {
  /** Stable: the first frame's time, offset and place in the feed. */
  key: string
  atMs: number
  offsetHz: number
  text: string
  /** My own transmission (JS8Call prints what it sends here too, in its TX colour). */
  mine: boolean
  /** Addressed to me or a group I joined. */
  directed: boolean
  /** The call the line opens with, when it opens with one. */
  from: string
}

export interface Js8RxOptions {
  /** "My offset": RX's, the offset JS8Call's `freq()` listens on. */
  rxOffsetHz: number
  myCall: string
  groups: readonly string[]
  /** JS8Call 3.0's RX block list (Settings ▸ JS8). */
  blocklist: readonly string[]
  eot: string
  mfi: string
  nowMs: number
  /** Rows heard at or before this were cleared from the pane (0: never). */
  clearedAtMs: number
}

/**
 * The RX pane: JS8Call's `processRxActivity` → `displayTextForFreq` → `writeMessageTextToUI`, run over
 * the engine's feed in the order it was decoded.
 *
 * What prints: a frame within its speed's `rxThreshold` of my offset; every frame of a message
 * addressed to me or a group I joined once its header has come (the message buffer, wherever it is
 * in the passband); a single-frame command to me; free text to @ALLCALL; and my own frames. Not a
 * partial directed frame (`<....>`) and not a heartbeat outside a message to me. A frame from a call
 * on the RX block list prints nothing, and neither do that offset's next frames until a new message
 * starts there (mainwindow.cpp:3108-3135).
 *
 * How it prints: a message's first frame starts a line ("time (offset) text"); the next frames at
 * that offset, or 10 Hz either side, join it until its end mark; the last frame adds ♢, and ANY
 * message ending clears the open lines, so the next frame anywhere starts afresh, as JS8Call's
 * `m_rxFrameBlockNumbers.clear()` does. A line whose next frame has not come within one and a half
 * periods takes the missing-frame mark (……), there or at its end.
 */
export function js8RxLines(rows: readonly Js8ActivityRow[], o: Js8RxOptions): Js8RxLine[] {
  if (isLegacyFeed(rows)) return legacyRxLines(rows, o)
  const lines: Js8RxLine[] = []
  /** JS8Call's `m_rxFrameBlockNumbers`: an offset (and its 10 Hz neighbours) → the open line. */
  const open = new Map<number, number>()
  /** The open lines' last frame, for the missing-frame mark. */
  const tail = new Map<number, { atMs: number; speed: Js8Speed; done: boolean }>()
  /** Message buffers addressed to me: their offset → the speed whose `rxThreshold` matches them. */
  const buffers = new Map<number, Js8Speed>()
  /** JS8Call's `m_rxBlockedOffsets`. */
  const blocked = new Set<number>()

  const bufferNear = (hz: number, speed: Js8Speed): number | null => {
    const range = JS8_SPEEDS[speed]?.rxThresholdHz ?? 10
    for (const at of buffers.keys()) if (Math.abs(at - hz) <= range) return at
    return null
  }

  const display = (r: Js8ActivityRow, text: string, hz: number, isNewLine: boolean, isLast: boolean, directed: boolean) => {
    // JS8Call's lookup: the offset itself, else the 10 Hz step below or above it, which then names
    // the line (mainwindow.cpp:3136-3148).
    const lowFreq = Math.floor(hz / 10) * 10
    const highFreq = lowFreq + 10
    const freq = open.has(hz) ? hz : open.has(lowFreq) ? lowFreq : open.has(highFreq) ? highFreq : hz
    let block = open.get(freq) ?? -1
    if (isNewLine) {
      open.delete(freq)
      open.delete(lowFreq)
      open.delete(highFreq)
      block = -1
    }
    // An open line takes the frame unless it already holds the end mark (an empty mark is in every
    // line, so then every frame starts its own, as Qt's `contains("")` has it).
    const line = block >= 0 ? lines[block] : undefined
    if (line && !(o.eot === '' || line.text.includes(o.eot))) {
      const t = tail.get(block)
      if (t && !t.done && !r.mine && r.atMs - t.atMs >= missedAfter(t.speed)) line.text += o.mfi
      line.text += text
      line.directed ||= directed
    } else {
      // A line still waiting near here is over: its message went on in this one, or never came back.
      for (const [i, t] of tail) if (!t.done && Math.abs(lines[i].offsetHz - freq) <= 10) t.done = true
      lines.push({
        key: `${r.atMs}-${freq}-${lines.length}`,
        atMs: r.atMs,
        offsetHz: freq,
        text,
        mine: r.mine,
        directed,
        from: text.includes(':') ? senderOf(text) : '',
      })
      block = lines.length - 1
    }
    tail.set(block, { atMs: r.atMs, speed: r.speed, done: isLast || r.mine })
    if (isLast) open.clear()
    else {
      open.set(freq, block)
      open.set(lowFreq, block)
      open.set(highFreq, block)
    }
  }

  for (const r of framesOf(rows)) {
    if (r.atMs <= o.clearedAtMs) continue
    const hz = offsetOf(r)
    const first = r.flags!.first
    const last = r.flags!.last
    const text = last ? withEot(r.text, o.eot) : r.text
    if (r.mine) {
      display(r, text, hz, first, last, false)
      continue
    }
    // A message starting closes any buffer it lands on; a directed header to me opens one.
    let buffer = bufferNear(hz, r.speed)
    if (first && buffer !== null) {
      buffers.delete(buffer)
      buffer = null
    }
    const to = r.from ? addresseeOf(r.text) : ''
    const toMe = r.from !== '' && r.directedToMe && toMeOrMyGroup(to, o.myCall, o.groups)
    const toAll = r.from !== '' && r.directedToMe && to === '@ALLCALL' && allcallFreeText(rstrip(r.text))
    if ((toMe || toAll) && !last) {
      buffers.set(hz, r.speed)
      buffer = hz
    }
    let shown = Math.abs(hz - o.rxOffsetHz) <= (JS8_SPEEDS[r.speed]?.rxThresholdHz ?? 10)
    if (buffer !== null || toMe) shown = true
    else if (r.from && (r.text.includes('<....>') || r.text.includes(': HB ') || r.text.includes(': @ALLCALL HB'))) shown = false
    if (last && buffer !== null) buffers.delete(buffer)
    // The RX block list: a frame from a listed call, and that offset's frames after it.
    const sender = senderOf(r.text)
    if (sender && r.text.includes(':') && onCallList(o.blocklist, sender)) {
      blocked.add(hz)
      continue
    }
    if (blocked.has(hz)) {
      if (last || first) {
        blocked.delete(hz)
        if (!first) continue
      } else continue
    }
    if (!shown) continue
    display(r, text, hz, first, last, toMe || buffer !== null)
  }
  // The missing-frame mark at a line's end: its next frame is overdue.
  for (const [i, t] of tail) {
    if (!t.done && o.nowMs - t.atMs >= missedAfter(t.speed) && !lines[i].text.endsWith(o.mfi)) lines[i].text += o.mfi
  }
  return lines
}

/** A feed that says nothing of where each row stands (an older station's): each row that is on my
 *  offset, addressed to me or my own, on its own line, as it stands. */
function legacyRxLines(rows: readonly Js8ActivityRow[], o: Js8RxOptions): Js8RxLine[] {
  return rows
    .filter((r) => r.atMs > o.clearedAtMs)
    .filter((r) => r.mine || r.directedToMe || Math.abs(offsetOf(r) - o.rxOffsetHz) <= (JS8_SPEEDS[r.speed]?.rxThresholdHz ?? 10))
    .filter((r) => !(r.from && onCallList(o.blocklist, r.from)))
    .map((r, i) => ({
      key: `${r.atMs}-${offsetOf(r)}-${i}`,
      atMs: r.atMs,
      offsetHz: offsetOf(r),
      text: r.text,
      mine: r.mine,
      directed: r.directedToMe,
      from: r.from,
    }))
}

// ── Band activity ─────────────────────────────────────────────────────────────────────────────

/** One Band activity row: an offset and what was heard there. */
export interface Js8BandRow {
  offsetHz: number
  /** The newest shown item's: its time, SNR, time delta and speed. */
  atMs: number
  snrDb: number
  dtS: number
  speed: Js8Speed
  /** Its shown items joined, with the marks (`Varicode::rstrip(text.join(""))`). */
  text: string
  /** Traffic to me heard there, or my call in its last item (JS8Call's MyCall colour). */
  directed: boolean
  /** "CQ" among its words (JS8Call's CQ colour). */
  cq: boolean
  /** The highlight list a word of it is on; the primary wins. */
  highlight: 'primary' | 'secondary' | null
  /** The newest shown item was low-confidence copy. */
  lowConf: boolean
}

export interface Js8BandOptions {
  nowMs: number
  /** JS8Call's ActivityAging, in minutes; 0 is off. */
  agingMin: number
  /** The selected offset (RX's): never aged off. */
  selectedHz: number | null
  /** View ▸ "Show Band Heartbeats and ACKs" (JS8Call's default: on). */
  showHb: boolean
  eot: string
  mfi: string
  myCall: string
  primary: readonly string[]
  secondary: readonly string[]
  /** Rows heard at or before this were cleared (0: never). */
  clearedAtMs: number
  /** Offsets removed one at a time ("Remove Activity"), each with when: heard after it, it is back. */
  removed: Readonly<Record<string, number>>
}

/** A Band activity item: a frame, or the missing-frame mark JS8Call files in its place. */
interface BandItem {
  atMs: number
  snrDb: number
  dtS: number
  speed: Js8Speed
  text: string
  last: boolean
  lowConf: boolean
  directed: boolean
  mfi: boolean
}

/**
 * JS8Call's `m_bandActivity`: every frame heard, filed under its whole-hertz offset. A frame at an
 * offset not yet filed takes over the filed offset within its speed's `rxThreshold` that
 * `generateOffsets` meets first counting up (processDecodeEvent.cpp:280-298), and each offset keeps
 * its last ten frames (:345-348). My own frames are not decodes and are not filed.
 */
export function js8BandBuckets(rows: readonly Js8ActivityRow[]): Map<number, Js8ActivityRow[]> {
  const buckets = new Map<number, Js8ActivityRow[]>()
  for (const r of framesOf(rows)) {
    if (r.mine) continue
    const hz = offsetOf(r)
    if (!buckets.has(hz)) {
      const range = JS8_SPEEDS[r.speed]?.rxThresholdHz ?? 10
      for (let prev = hz - range; prev <= hz + range; prev++) {
        const held = buckets.get(prev)
        if (held) {
          buckets.delete(prev)
          buckets.set(hz, held)
          break
        }
      }
    }
    const items = buckets.get(hz) ?? []
    items.push(r)
    while (items.length > 10) items.shift()
    buckets.set(hz, items)
  }
  return buckets
}

/** Words as JS8Call splits a row for its colours: on ':', '>' and spaces (displayBandActivity.cpp:304). */
const wordsOf = (text: string): Set<string> => new Set(text.toUpperCase().split(/[:> ]/u).filter(Boolean))

/** Which highlight list names one of `words`: the primary wins, as JS8Call paints it last. */
export function highlightOf(words: ReadonlySet<string>, primary: readonly string[], secondary: readonly string[]): 'primary' | 'secondary' | null {
  const on = (list: readonly string[]) => list.some((w) => words.has(w.trim().toUpperCase()))
  return on(primary) ? 'primary' : on(secondary) ? 'secondary' : null
}

/** JS8Call's `isMyCallIncluded`: my base call among the callsigns of `text` (mainwindow.cpp:6073-6086). */
function myCallIncluded(text: string, myCall: string): boolean {
  const me = myCall.trim().toUpperCase()
  if (!me) return false
  const base = js8BaseCall(me)
  return text
    .toUpperCase()
    .split(/[^A-Z0-9/]+/u)
    .some((w) => w === base || w === me)
}

/**
 * The Band activity rows (unsorted; `sortBandRows` orders them), as `displayBandActivity` builds
 * each one: the offset's items with the missing-frame marks in place, minus what is aged
 * (unless its offset is selected), heartbeats and their ACKs while those are hidden (and the
 * compound-call frame before one, and a `MSG ID` reply that follows a hidden one), and anything
 * empty; low-confidence copy in brackets, ♢ after each message's last frame, all joined.
 */
export function js8BandRows(rows: readonly Js8ActivityRow[], o: Js8BandOptions): Js8BandRow[] {
  if (isLegacyFeed(rows)) return legacyBandRows(rows, o)
  // What "Clear" and "Remove Activity" took out of JS8Call's map is not filed again: a frame heard by
  // then, at the removed offset or within its speed's rxThreshold of it, is gone.
  const removed = Object.entries(o.removed).map(([hz, at]) => [Number(hz), at] as const)
  const gone = (r: Js8ActivityRow) =>
    r.atMs <= o.clearedAtMs ||
    removed.some(([hz, at]) => r.atMs <= at && Math.abs(offsetOf(r) - hz) <= (JS8_SPEEDS[r.speed]?.rxThresholdHz ?? 10))
  const out: Js8BandRow[] = []
  for (const [offsetHz, kept] of js8BandBuckets(rows.filter((r) => !gone(r)))) {
    if (kept.length === 0) continue
    // The missing-frame mark goes where an offset waited one and a half periods past a frame that
    // was not a message's last: between two frames, or after the newest.
    const items: BandItem[] = []
    kept.forEach((r, i) => {
      items.push({
        atMs: r.atMs,
        snrDb: r.snrDb,
        dtS: r.dtS,
        speed: r.speed,
        text: r.text,
        last: r.flags!.last,
        lowConf: r.lowConf,
        directed: r.directedToMe && addresseeOf(r.text) !== '@ALLCALL',
        mfi: false,
      })
      const next = kept[i + 1]
      const waited = (next ? next.atMs : o.nowMs) - r.atMs
      if (!r.flags!.last && waited >= missedAfter(r.speed)) {
        items.push({ ...items[items.length - 1], text: o.mfi, last: false, lowConf: false, directed: false, mfi: true })
      }
    })
    const selected = offsetHz === o.selectedHz
    const shown: boolean[] = []
    items.forEach((it, i) => {
      let show = true
      if (!selected && o.agingMin > 0 && Math.trunc((o.nowMs - it.atMs) / 60_000) >= o.agingMin) show = false
      if (!o.showHb) {
        if (it.text.includes(' @HB ') || it.text.includes(' HEARTBEAT ')) {
          show = false
          if (i > 0 && shown[i - 1] && items[i - 1].text.endsWith(': ')) shown[i - 1] = false
        }
        if ((i === 0 || !shown[i - 1]) && it.text.includes(' MSG ID ')) show = false
      }
      if (it.text === '') show = false
      shown.push(show)
    })
    const parts: string[] = []
    let newest: BandItem | null = null
    let directed = false
    items.forEach((it, i) => {
      if (!shown[i]) return
      let text = it.lowConf ? `[${it.text}]` : it.text
      if (it.last) text = withEot(text, o.eot)
      parts.push(text)
      newest = it
      directed ||= it.directed
    })
    const joined = rstrip(parts.join(''))
    if (!joined || !newest) continue
    const n = newest as BandItem
    const words = wordsOf(joined)
    out.push({
      offsetHz,
      atMs: n.atMs,
      snrDb: n.snrDb,
      dtS: n.dtS,
      speed: n.speed,
      text: joined,
      directed: directed || myCallIncluded(parts[parts.length - 1], o.myCall),
      cq: words.has('CQ'),
      highlight: highlightOf(words, o.primary, o.secondary),
      lowConf: n.lowConf,
    })
  }
  return out
}

/** A feed that says nothing of where each row stands (an older station's): the newest row per
 *  offset, as this pane always showed one. */
function legacyBandRows(rows: readonly Js8ActivityRow[], o: Js8BandOptions): Js8BandRow[] {
  const filed = bandActivityByOffset(rows.filter((r) => r.atMs > o.clearedAtMs)).filter((r) => r.atMs > (o.removed[String(r.offsetHz)] ?? 0))
  return js8ShownOffsetRows(filed, { agingMin: o.agingMin, nowMs: o.nowMs, selectedHz: o.selectedHz })
    .map((r) => {
      const words = wordsOf(r.text)
      return {
        offsetHz: r.offsetHz,
        atMs: r.atMs,
        snrDb: r.snrDb,
        dtS: r.dtS,
        speed: r.speed,
        text: r.text,
        directed: r.directedToMe,
        cq: words.has('CQ'),
        highlight: highlightOf(words, o.primary, o.secondary),
        lowConf: r.lowConf,
      }
    })
}

/** JS8Call's Band activity "Sort By…" (mainwindow.cpp:4772-4781): `-` is the reverse. */
export const JS8_BAND_SORTS = ['offset', 'timestamp', '-timestamp', 'snr', '-snr', 'submode', '-submode'] as const
export type Js8BandSort = (typeof JS8_BAND_SORTS)[number]

/** JS8Call's SNR order: a value past ±60 (no real report) goes last in the forward order. */
function snrKey(snr: number, reverse: boolean): number {
  return !reverse && (snr < -60 || snr > 60) ? -snr : snr
}

/** Slowest first: JS8Call orders its submodes with Slow ahead of the rest (displayBandActivity.cpp
 *  :93-106) — by the period, which holds for any speed added after these. */
const slowness = (speed: Js8Speed): number => -(JS8_SPEEDS[speed]?.periodS ?? 0)

/** Sort as JS8Call's `displayBandActivity` does: by offset first, then stably by the choice, then
 *  the whole list reversed for a `-` choice. */
export function sortBandRows(rows: readonly Js8BandRow[], sort: Js8BandSort): Js8BandRow[] {
  const reverse = sort.startsWith('-')
  const by = reverse ? sort.slice(1) : sort
  const out = [...rows].sort((a, b) => a.offsetHz - b.offsetHz)
  if (by === 'timestamp') out.sort((a, b) => a.atMs - b.atMs)
  else if (by === 'snr') out.sort((a, b) => snrKey(a.snrDb, reverse) - snrKey(b.snrDb, reverse))
  else if (by === 'submode') out.sort((a, b) => slowness(a.speed) - slowness(b.speed))
  return reverse ? out.reverse() : out
}

// ── the call list ─────────────────────────────────────────────────────────────────────────────

/** JS8Call's call list mark, one per row, in its order of precedence (displayCallActivity.cpp:
 *  234-252): ✕ blocked, ⚑ a message for me, ★ hearing me, ☎ a CQ in the last five minutes, ⚟ heard
 *  through a relay. */
export type Js8CallMark = 'blocked' | 'message' | 'ack' | 'cq' | 'relay'
export const JS8_MARK_GLYPH: Record<Js8CallMark, string> = { blocked: '✕', message: '⚑', ack: '★', cq: '☎', relay: '⚟' }

/** One station row of the call list. */
export interface Js8CallRow {
  call: string
  /** What the station's own frames said, or null for a call added by hand or heard only through a
   *  relay (JS8Call's empty CallDetail). */
  heard: Js8Heard | null
  mark: Js8CallMark | null
  /** When the mark's event happened (★, ☎ and ⚟), for its tooltip. */
  markAtMs: number | null
  /** The relay a ⚟ station was heard through. */
  through: string | null
  /** The time delta of the station's newest frame in the feed, seconds; null when none is there. */
  dtS: number | null
  /** Last heard: its own frames, or the relayed message that named it. */
  lastMs: number | null
}

export interface Js8CallOptions {
  nowMs: number
  myCall: string
  blocklist: readonly string[]
  /** Calls with an unread message for me (`js8UnreadFrom`). */
  unreadFrom: ReadonlySet<string>
  /** The feed, for each station's newest time delta. */
  activity: readonly Js8ActivityRow[]
  /** The inbox, for the stations heard only through a relay. */
  inbox: readonly Js8InboxEntry[]
  /** Calls added by hand ("Add New Station or Group…"), each with when. */
  added: Readonly<Record<string, number>>
  /** Calls removed one at a time ("Remove This Station"), each with when: heard after it, it is back. */
  removed: Readonly<Record<string, number>>
  /** The list was cleared at this time (0: never): a station heard before it is not listed again
   *  until it is heard again, unless it has a message for me (JS8Call's inbox count brings it back). */
  clearedAtMs: number
}

/** JS8Call's ☎: a CQ less than five whole minutes ago (`cqTimestamp.secsTo(now) / 60 < 5`). */
const CQ_MARK_MIN = 5

/**
 * The call list's station rows (unsorted; `sortCallRows` orders them): the heard stations JS8Call
 * would list — the caller has already aged them (`js8ListedStations`) — less the ones cleared or
 * removed since they were heard, plus the calls added by hand and the stations heard only through a
 * relay of a message to me, each with JS8Call's one mark and the time delta of its newest frame.
 */
export function js8CallRows(listed: readonly Js8Heard[], o: Js8CallOptions): Js8CallRow[] {
  const latestDt = new Map<string, number>()
  for (const r of o.activity) {
    if (!r.from || r.mine) continue
    latestDt.set(js8BaseCall(r.from), r.dtS)
  }
  // ⚟: the stations a relayed message to me names after its relay (`parseRelayPathCallsigns`, each
  // logged with `cd.through = d.from`, processCommandActivity.cpp:440-450).
  const relayed = new Map<string, { through: string; atMs: number }>()
  for (const e of o.inbox) {
    if (e.state !== 'unread' && e.state !== 'read') continue
    if (e.path.length < 2) continue
    for (const call of e.path.slice(1)) {
      const c = call.trim().toUpperCase()
      const was = relayed.get(c)
      if (!was || was.atMs < e.atMs) relayed.set(c, { through: e.path[0].trim().toUpperCase(), atMs: e.atMs })
    }
  }
  const gone = (call: string, heardMs: number | null): boolean => {
    if (o.unreadFrom.has(call)) return false
    const at = Math.max(o.clearedAtMs, o.removed[call] ?? 0)
    return at > 0 && (heardMs ?? 0) <= at
  }
  const rows: Js8CallRow[] = []
  const listedCalls = new Set<string>()
  const markFor = (call: string, h: Js8Heard | null, relay: { through: string; atMs: number } | undefined) => {
    const since = (ms: number | null | undefined) => (ms != null && ms > o.clearedAtMs ? ms : null)
    const ack = since(h?.ackMs)
    // A station from before ☎ was kept (an older one, on the Remote page) shows it by its last CQ.
    const cqAt = since(h ? (h.cqMs ?? (h.lastCq ? h.lastMs : null)) : null)
    const cq = cqAt != null && Math.trunc((o.nowMs - cqAt) / 60_000) < CQ_MARK_MIN ? cqAt : null
    const viaRelay = relay && (!h || h.lastMs < relay.atMs) ? relay : undefined
    const mark: Js8CallMark | null = onCallList(o.blocklist, call)
      ? 'blocked'
      : o.unreadFrom.has(call)
        ? 'message'
        : ack != null
          ? 'ack'
          : cq != null
            ? 'cq'
            : viaRelay
              ? 'relay'
              : null
    return {
      mark,
      markAtMs: mark === 'ack' ? ack : mark === 'cq' ? cq : mark === 'relay' ? viaRelay!.atMs : null,
      through: viaRelay ? viaRelay.through : null,
    }
  }
  for (const h of listed) {
    const call = h.call
    const relay = relayed.get(call)
    const lastMs = Math.max(h.lastMs, relay?.atMs ?? 0)
    if (gone(call, lastMs)) continue
    listedCalls.add(call)
    rows.push({ call, heard: h, ...markFor(call, h, relay), dtS: latestDt.get(js8BaseCall(call)) ?? null, lastMs })
  }
  for (const [call, relay] of relayed) {
    if (listedCalls.has(call) || gone(call, relay.atMs)) continue
    listedCalls.add(call)
    rows.push({ call, heard: null, ...markFor(call, null, relay), dtS: null, lastMs: relay.atMs })
  }
  for (const [raw, addedAt] of Object.entries(o.added)) {
    const call = raw.trim().toUpperCase()
    if (!call || listedCalls.has(call) || gone(call, addedAt)) continue
    listedCalls.add(call)
    rows.push({ call, heard: null, ...markFor(call, null, undefined), dtS: null, lastMs: null })
  }
  return rows
}

/** JS8Call's call list "Sort By…" (mainwindow.cpp:4784-4800): `-` is the reverse. */
export const JS8_CALL_SORTS = [
  'callsign',
  'ackTimestamp',
  'offset',
  'distance',
  '-distance',
  'azimuth',
  'timestamp',
  '-timestamp',
  'snr',
  '-snr',
  'submode',
  '-submode',
] as const
export type Js8CallSort = (typeof JS8_CALL_SORTS)[number]

/** Distance (km) and azimuth (°) from my grid to the station's, or null where either is unknown. */
export function pathTo(myGrid: string, grid: string | null | undefined): { km: number; deg: number } | null {
  const me = myGrid ? gridToLatLon(myGrid) : null
  const them = grid ? gridToLatLon(grid) : null
  if (!me || !them) return null
  return { km: haversineKm(me, them), deg: bearingDeg(me, them) }
}

/**
 * Sort as JS8Call's `displayCallActivity` does: by callsign first, then stably by the choice, then
 * the whole list reversed for a `-` choice — a station with no distance or azimuth always last —
 * and then the stations with a message for me stably to the top ("pin messages to the top",
 * displayCallActivity.cpp:181-191). `grids` is each call's grid (its own, else the log's).
 */
export function sortCallRows(
  rows: readonly Js8CallRow[],
  sort: Js8CallSort,
  o: { myGrid: string; grids: ReadonlyMap<string, string>; unreadFrom: ReadonlySet<string> },
): Js8CallRow[] {
  const reverse = sort.startsWith('-')
  const by = reverse ? sort.slice(1) : sort
  const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0)
  let out = [...rows].sort((a, b) => cmp(a.call, b.call))
  const path = (r: Js8CallRow) => pathTo(o.myGrid, o.grids.get(r.call))
  // JS8Call's invalid-last comparator: forward, a missing value sorts after every real one; the
  // reverse is built so that after the list is reversed the missing ones are still last.
  const missingLast = (a: number | null, b: number | null): number => {
    if (a == null && b == null) return 0
    if (a == null) return reverse ? -1 : 1
    if (b == null) return reverse ? 1 : -1
    return a - b
  }
  if (by === 'offset') out.sort((a, b) => (a.heard?.freqHz ?? -1) - (b.heard?.freqHz ?? -1))
  else if (by === 'distance') out.sort((a, b) => missingLast(path(a)?.km ?? null, path(b)?.km ?? null))
  else if (by === 'azimuth') out.sort((a, b) => missingLast(path(a)?.deg ?? null, path(b)?.deg ?? null))
  else if (by === 'timestamp') out.sort((a, b) => (a.lastMs ?? 0) - (b.lastMs ?? 0))
  else if (by === 'ackTimestamp') out.sort((a, b) => (b.heard?.ackMs ?? -1) - (a.heard?.ackMs ?? -1))
  else if (by === 'snr') out.sort((a, b) => snrKey(a.heard?.snrDb ?? -64, reverse) - snrKey(b.heard?.snrDb ?? -64, reverse))
  else if (by === 'submode') out.sort((a, b) => slowness(a.heard?.speed ?? 'normal') - slowness(b.heard?.speed ?? 'normal'))
  if (reverse) out = out.reverse()
  return js8UnreadFirst(out, o.unreadFrom)
}

/** The rows above the stations: @ALLCALL (while I take part in it) and my groups in order, each
 *  flagged ⚑ while it holds an unread message (JS8Call's `createGroupCallsignTableRows`,
 *  mainwindow.cpp:3040-3102). */
export function js8GroupRows(groups: readonly string[], inbox: readonly Js8InboxEntry[], allcall: boolean): { call: string; unread: boolean }[] {
  const unread = (g: string) => inbox.some((e) => e.state === 'unread' && e.to.trim().toUpperCase() === g)
  const mine = [...new Set(groups.map((g) => g.trim().toUpperCase()).filter((g) => g.startsWith('@') && g.length > 1))].sort()
  return [...(allcall ? ['@ALLCALL'] : []), ...mine].map((call) => ({ call, unread: unread(call) }))
}

// ── the inbox (JS8Call's MessagePanel) ────────────────────────────────────────────────────────

/** JS8Call's `Inbox::fetchForCall`: for one station, what it sent me (unread or read) and what I hold
 *  for it; for everyone ("%", null here), all of it. */
export function js8InboxFor(inbox: readonly Js8InboxEntry[], call: string | null): Js8InboxEntry[] {
  if (!call) return [...inbox]
  const c = call.trim().toUpperCase()
  // A @GROUP sends nothing: its messages are the ones addressed to it.
  if (c.startsWith('@')) return inbox.filter((e) => e.to.trim().toUpperCase() === c)
  return inbox.filter((e) =>
    e.state === 'store' || e.state === 'delivered' ? e.to.trim().toUpperCase() === c : e.from.trim().toUpperCase() === c,
  )
}

/** The columns the MessagePanel sorts by (its flag and text columns are not sortable). */
export const JS8_INBOX_SORTS = ['id', 'time', 'from', 'to'] as const
export type Js8InboxSortKey = (typeof JS8_INBOX_SORTS)[number]
export interface Js8InboxSort {
  key: Js8InboxSortKey
  descending: boolean
}
/** JS8Call opens the panel newest first (`setSortIndicator(2, Qt::DescendingOrder)`). */
export const JS8_INBOX_DEFAULT_SORT: Js8InboxSort = { key: 'time', descending: true }

/** The From column as JS8Call shows it: the path, last hop first, " via " between
 *  (`pathSegs(path).join(" via ")`, MessagePanel.cpp:17-21, :144-149). */
export const inboxFromLabel = (e: Js8InboxEntry): string => (e.path.length > 0 ? [...e.path].reverse().join(' via ') : e.from)

export function sortInbox(rows: readonly Js8InboxEntry[], sort: Js8InboxSort): Js8InboxEntry[] {
  const key = (e: Js8InboxEntry): number | string =>
    sort.key === 'id' ? e.id : sort.key === 'time' ? e.atMs : sort.key === 'from' ? inboxFromLabel(e) : e.to
  const out = [...rows].sort((a, b) => {
    const x = key(a)
    const y = key(b)
    const c = x < y ? -1 : x > y ? 1 : a.id - b.id
    return c
  })
  return sort.descending ? out.reverse() : out
}

/** JS8Call's Reply: the compose box gets `"<path> MSG [MESSAGE]"`, the path the message came by
 *  (`prepareReplyMessage`, MessagePanel.cpp:312-314, :336-350). */
export const JS8_REPLY_PLACEHOLDER = '[MESSAGE]'
export function js8ReplyText(e: Js8InboxEntry): string {
  const path = e.path.length > 0 ? e.path.join('>') : e.from
  return `${path} MSG ${JS8_REPLY_PLACEHOLDER}`
}
