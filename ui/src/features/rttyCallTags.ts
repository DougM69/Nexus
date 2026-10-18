// THE RTTY WATERFALL'S CALLSIGN TAGS: which call was heard at which audio frequency.
//
// Nexus runs ONE RTTY decoder, and it copies only where it is netted, so the decoder keeps no call
// per frequency. The attribution is made here, from the decoded text, by these rules and no other:
//   1. Text belongs to the frequency the decoder was listening on when it was copied: the netted
//      centre plus the AFC, read from the same poll that delivered the text.
//   2. If that frequency moved more than `TAG_NET_MOVE_HZ` between two polls (a net, a waterfall
//      click, Auto-tune), or the polls are more than `TAG_POLL_GAP_MS` apart (the cockpit was away),
//      the text that arrived is NOT attributed: it may have been copied on either side of the move.
//      The context before it is dropped too. The first poll after mount attributes nothing either:
//      the ring's history was copied wherever the decoder was then.
//   3. Only characters we received count. Anything we keyed (`charTx`) is cut out.
//   4. A call is attributed only where the text says it is the SENDER: `DE <CALL>`, or a call sent
//      twice running within a few words of `CQ` (`CQ TEST K1ABC K1ABC`). A call that is only named
//      is never tagged: in `K1ABC DE W2XYZ` the tag is W2XYZ, and K1ABC is the station being called.
//   5. Only complete words: a call still arriving at the end of the text waits for its space.
//   6. Our own call is never tagged.
// A tag is one signal: a new call within `TAG_SAME_SIGNAL_HZ` of a tag replaces it (a station that
// answers on the runner's frequency takes the runner's tag), and a call heard somewhere new moves.
import type { RttyState } from '../types'
import { clean, isCall } from './rttyGrab'

/** A move of the listening frequency bigger than this between two polls is a new net (rule 2).
 *  The AFC's own pull onto a signal is smaller; the next RTTY signal is further away. */
export const TAG_NET_MOVE_HZ = 40
/** Two polls further apart than this leave a gap the text cannot be placed across (rule 2). */
export const TAG_POLL_GAP_MS = 2000
/** Tags closer than this are the same signal. */
export const TAG_SAME_SIGNAL_HZ = 40
/** A tag stays at full strength this long after the call was last heard… */
export const TAG_FADE_MS = 3 * 60_000
/** …then fades, and is gone this long after. */
export const TAG_GONE_MS = 5 * 60_000
/** The faintest a fading tag draws, just before it goes. */
const TAG_FADED = 0.35
/** Text kept from one poll to the next, so a `DE` and its call may arrive in different polls. */
const CONTEXT_CHARS = 120
/** How many words after `CQ` a doubled call may sit (`CQ CQ TEST K1ABC K1ABC`). */
const CQ_REACH = 4

export interface CallTag {
  call: string
  hz: number
  heardMs: number
}

export interface TagTracker {
  /** The last ring seen, to find what is new in the next one. */
  text: string
  /** Received text of the current net that the next poll's words may need as context. */
  context: string
  /** Where the decoder listened at the last poll; null before the first. */
  hz: number | null
  atMs: number
  tags: CallTag[]
}

export const NO_TAGS: TagTracker = { text: '', context: '', hz: null, atMs: 0, tags: [] }

/** Where the new text starts in `next`, given the ring `prev` it grew from. The ring is trimmed at
 *  the front as it grows, so `next` is a tail of `prev` with new text after it; the longest such
 *  overlap wins. No overlap (a cleared ring) ⇒ all of `next` is new. */
export function newTextStart(prev: string, next: string): number {
  for (let d = 0; d < prev.length; d++) {
    const keep = prev.length - d
    if (keep <= next.length && next.startsWith(prev.slice(d))) return keep
  }
  return 0
}

/** The calls sent as their own sender (rule 4) by words that END at or after `from` in `text`.
 *  `text` is upper- or lower-case decoded copy; a word with nothing after it is incomplete. */
export function senderCalls(text: string, from: number, mycall = ''): string[] {
  const words: { w: string; end: number }[] = []
  for (const m of text.matchAll(/\S+/g)) {
    const end = (m.index ?? 0) + m[0].length
    if (end >= text.length) break // still arriving (rule 5)
    words.push({ w: clean(m[0]), end })
  }
  const me = clean(mycall)
  const out: string[] = []
  for (let i = 1; i < words.length; i++) {
    if (words[i].end <= from) continue
    const w = words[i].w
    if (!/[0-9]/.test(w) || !isCall(w) || w === me) continue
    const afterDe = words[i - 1].w === 'DE'
    const doubledAfterCq =
      words[i - 1].w === w && words.slice(Math.max(0, i - 1 - CQ_REACH), i - 1).some((x) => x.w === 'CQ')
    if ((afterDe || doubledAfterCq) && !out.includes(w)) out.push(w)
  }
  return out
}

/** Where the decoder listens: the netted centre moved by the AFC. */
export function listeningHz(s: Pick<RttyState, 'markHz' | 'spaceHz' | 'afcHz'>): number {
  return (s.markHz + s.spaceHz) / 2 + s.afcHz
}

/** One poll's worth of tracking: the next tracker, given the last one and the state just read. */
export function trackTags(
  tr: TagTracker,
  s: Pick<RttyState, 'text' | 'charTx' | 'markHz' | 'spaceHz' | 'afcHz'>,
  nowMs: number,
  mycall = '',
): TagTracker {
  const hz = listeningHz(s)
  const start = newTextStart(tr.text, s.text)
  // Rule 3: what we keyed is not a signal we heard; a space keeps words from joining across it.
  let fresh = ''
  for (let i = start; i < s.text.length; i++) fresh += s.charTx?.[i] ? ' ' : s.text[i]
  const placeable = tr.hz !== null && Math.abs(hz - tr.hz) <= TAG_NET_MOVE_HZ && nowMs - tr.atMs <= TAG_POLL_GAP_MS
  let tags = tr.tags.filter((t) => nowMs - t.heardMs < TAG_GONE_MS)
  let context = ''
  if (placeable) {
    const seg = tr.context + fresh
    for (const call of senderCalls(seg, tr.context.length, mycall)) {
      tags = tags.filter((t) => t.call !== call && Math.abs(t.hz - hz) >= TAG_SAME_SIGNAL_HZ)
      tags.push({ call, hz, heardMs: nowMs })
    }
    context = seg
  }
  // Keep a tail of whole words as the next poll's context (rule 2 drops it on a move).
  if (context.length > CONTEXT_CHARS) {
    const cut = context.slice(context.length - CONTEXT_CHARS).search(/\s/)
    context = cut < 0 ? '' : context.slice(context.length - CONTEXT_CHARS + cut)
  }
  return { text: s.text, context, hz, atMs: nowMs, tags }
}

/** A tag's strength as it ages: full for `TAG_FADE_MS`, then down to `TAG_FADED` by
 *  `TAG_GONE_MS`, then 0 (gone). */
export function tagOpacity(heardMs: number, nowMs: number): number {
  const age = nowMs - heardMs
  if (age >= TAG_GONE_MS) return 0
  if (age <= TAG_FADE_MS) return 1
  return 1 - ((age - TAG_FADE_MS) / (TAG_GONE_MS - TAG_FADE_MS)) * (1 - TAG_FADED)
}
