// The RTTY waterfall's callsign tags: which call is attributed to which audio frequency, and when
// one is NOT. Every case states the frequency by value: the decoder's netted centre plus its AFC.
import { describe, it, expect } from 'vitest'
import {
  NO_TAGS,
  TAG_FADE_MS,
  TAG_GONE_MS,
  newTextStart,
  senderCalls,
  tagOpacity,
  trackTags,
  type TagTracker,
} from './rttyCallTags'

/** Netted on 1415/1585 (centre 1500) with the AFC 3 Hz up: the decoder listens on 1503. */
const AT = { markHz: 1415, spaceHz: 1585, afcHz: 3 }

/** Feed polls of a growing ring, 500 ms apart; returns the last tracker. */
function polls(texts: string[], over: Partial<typeof AT>[] = [], mycall = 'KD9TAW', t0 = 1_000_000): TagTracker {
  let tr = NO_TAGS
  texts.forEach((text, i) => {
    tr = trackTags(tr, { ...AT, ...(over[i] ?? {}), text, charTx: undefined }, t0 + i * 500, mycall)
  })
  return tr
}

describe('a call is tagged where the decoder heard it sent', () => {
  it('DE <call>: tagged at the netted centre plus the AFC', () => {
    const tr = polls(['RYRYRY ', 'RYRYRY CQ CQ DE W1AW W1AW K\n'])
    expect(tr.tags).toEqual([{ call: 'W1AW', hz: 1503, heardMs: 1_000_500 }])
  })

  it('a contest CQ with no DE: the call sent twice after CQ', () => {
    const tr = polls(['', 'CQ TEST K1ABC K1ABC '])
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([['K1ABC', 1503]])
  })

  it('a call only named is not the sender: K1ABC DE W2XYZ tags W2XYZ alone', () => {
    const tr = polls(['', 'TU K1ABC DE W2XYZ W2XYZ K '])
    expect(tr.tags.map((t) => t.call)).toEqual(['W2XYZ'])
    // On its own too: the one-tag-per-signal rule above would hide a K1ABC tag behind W2XYZ's.
    expect(senderCalls('TU K1ABC DE W2XYZ W2XYZ K ', 0)).toEqual(['W2XYZ'])
    expect(senderCalls('QRZ K1ABC 599 TU ', 0)).toEqual([])
  })

  it('a word still arriving waits for its space, and DE may arrive a poll earlier', () => {
    const half = polls(['', 'CQ DE W1A'])
    expect(half.tags).toEqual([])
    const tr = trackTags(half, { ...AT, text: 'CQ DE W1AW ', charTx: undefined }, 1_001_000, 'KD9TAW')
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([['W1AW', 1503]])
  })

  it('a word that is not a call is never tagged: DE 599, DE TEST, DE RR73', () => {
    expect(polls(['', 'DE 599 DE TEST DE RR73 DE 5NN '])).toMatchObject({ tags: [] })
  })

  it('our own call is never tagged', () => {
    expect(polls(['', 'W1AW DE KD9TAW KD9TAW K ']).tags).toEqual([])
  })
})

describe('text that cannot be placed is not attributed', () => {
  it('the first poll: the ring holds history copied wherever the decoder was then', () => {
    expect(polls(['CQ CQ DE W1AW W1AW K\n']).tags).toEqual([])
  })

  it('the decoder moved between polls (a net): that poll’s text is not placed, nor is the DE before it', () => {
    const tr = polls(['', 'CQ DE ', 'CQ DE W1AW '], [{}, {}, { markHz: 2125, spaceHz: 2295 }])
    expect(tr.tags).toEqual([])
    // …and the next poll on the new net is placed there.
    const next = trackTags(tr, { markHz: 2125, spaceHz: 2295, afcHz: 0, text: 'CQ DE W1AW DE K5LLL ', charTx: undefined }, 1_001_500)
    expect(next.tags.map((t) => [t.call, t.hz])).toEqual([['K5LLL', 2210]])
  })

  it('a DE copied before a net does not name the call copied after it', () => {
    // DE on 1503 Hz; a net to 2210 Hz with nothing new; then a call on 2210 Hz.
    const tr = polls(['', 'CQ DE ', 'CQ DE ', 'CQ DE W1AW '], [{}, {}, { markHz: 2125, spaceHz: 2295, afcHz: 0 }, { markHz: 2125, spaceHz: 2295, afcHz: 0 }])
    expect(tr.tags).toEqual([])
  })

  it('an AFC pull within 40 Hz is the same signal, and the text is placed at the new frequency', () => {
    const tr = polls(['', 'DE W1AW '], [{}, { afcHz: 30 }])
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([['W1AW', 1530]])
  })

  it('a gap of more than 2 s between polls (the cockpit was away): not placed', () => {
    let tr = trackTags(NO_TAGS, { ...AT, text: '', charTx: undefined }, 0)
    tr = trackTags(tr, { ...AT, text: 'DE W1AW ', charTx: undefined }, 2_001)
    expect(tr.tags).toEqual([])
  })

  it('what we keyed is cut out: our own over naming a call tags nothing', () => {
    const text = 'DE W1AW W1AW '
    let tr = trackTags(NO_TAGS, { ...AT, text: '', charTx: [] }, 0)
    tr = trackTags(tr, { ...AT, text, charTx: Array.from(text, () => true) }, 500)
    expect(tr.tags).toEqual([])
  })
})

describe('one tag per signal, and tags fade', () => {
  it('a station answering on the runner’s frequency takes its tag; one elsewhere keeps its own', () => {
    let tr = polls(['', 'CQ TEST K1ABC K1ABC '])
    tr = trackTags(tr, { ...AT, text: 'CQ TEST K1ABC K1ABC DE W2XYZ ', charTx: undefined }, 1_001_000)
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([['W2XYZ', 1503]])
    // A net to 2210 Hz, then a call there: both tags stand.
    tr = trackTags(tr, { markHz: 2125, spaceHz: 2295, afcHz: 0, text: 'x', charTx: undefined }, 1_001_500)
    tr = trackTags(tr, { markHz: 2125, spaceHz: 2295, afcHz: 0, text: 'x DE K5LLL ', charTx: undefined }, 1_002_000)
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([
      ['W2XYZ', 1503],
      ['K5LLL', 2210],
    ])
  })

  it('a call heard again elsewhere moves', () => {
    let tr = polls(['', 'DE W1AW '])
    tr = trackTags(tr, { markHz: 2125, spaceHz: 2295, afcHz: 0, text: 'DE W1AW ', charTx: undefined }, 1_001_000)
    tr = trackTags(tr, { markHz: 2125, spaceHz: 2295, afcHz: 0, text: 'DE W1AW DE W1AW ', charTx: undefined }, 1_001_500)
    expect(tr.tags.map((t) => [t.call, t.hz])).toEqual([['W1AW', 2210]])
  })

  it('full strength for 3 minutes, fading to 0.35 by 5, then gone', () => {
    expect(tagOpacity(0, 0)).toBe(1)
    expect(tagOpacity(0, TAG_FADE_MS)).toBe(1)
    expect(tagOpacity(0, 4 * 60_000)).toBeCloseTo(0.675, 6)
    expect(tagOpacity(0, TAG_GONE_MS)).toBe(0)
    const tr = polls(['', 'DE W1AW '])
    const later = trackTags(tr, { ...AT, text: 'DE W1AW ', charTx: undefined }, 1_000_500 + TAG_GONE_MS)
    expect(later.tags).toEqual([])
  })
})

describe('the parts', () => {
  it('newTextStart: the ring trimmed at the front still finds where the new text starts', () => {
    expect(newTextStart('ABCDEF', 'ABCDEFGH')).toBe(6)
    expect(newTextStart('ABCDEF', 'CDEFGH')).toBe(4)
    expect(newTextStart('ABCDEF', 'ABCDEF')).toBe(6)
    expect(newTextStart('ABCDEF', 'XY')).toBe(0)
    expect(newTextStart('', 'XY')).toBe(0)
  })

  it('senderCalls only reports words that end in the new part', () => {
    expect(senderCalls('DE W1AW DE K5LLL ', 8)).toEqual(['K5LLL'])
    expect(senderCalls('DE W1AW DE K5LLL ', 0)).toEqual(['W1AW', 'K5LLL'])
  })
})
