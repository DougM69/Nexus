// The JS8 lists as JS8Call 3.0.3 draws them (features/js8Lists): each rule below is pinned BY VALUE
// against what JS8Call would print, with the JS8Call source line it reads.
import { describe, expect, it } from 'vitest'
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry } from '../types'
import {
  addresseeOf,
  inboxFromLabel,
  isLegacyFeed,
  js8BandRows,
  js8CallRows,
  js8GroupRows,
  js8InboxFor,
  js8ReplyText,
  js8RxLines,
  onCallList,
  sortBandRows,
  sortCallRows,
  sortInbox,
  speedMark,
  speedName,
  withEot,
  type Js8BandOptions,
  type Js8CallOptions,
  type Js8RxOptions,
} from './js8Lists'

const T0 = 1_760_000_000_000
const P = 15_000 // a Normal period

/** A frame row as the engine sends it: its place in its message, and nothing else unless said. */
function frame(o: Partial<Js8ActivityRow> & { atMs: number; freqHz: number; text: string }, first = true, last = true): Js8ActivityRow {
  return {
    speed: 'normal',
    snrDb: -10,
    dtS: 0.1,
    from: '',
    directedToMe: false,
    mine: false,
    complete: true,
    lowConf: false,
    flags: { first, last, assembled: false },
    ...o,
  }
}

const rx = (o: Partial<Js8RxOptions> = {}): Js8RxOptions => ({
  rxOffsetHz: 1500,
  myCall: 'KD9TAW',
  groups: ['@FUN'],
  blocklist: [],
  eot: '♢',
  mfi: '……',
  nowMs: T0 + 10 * P,
  clearedAtMs: 0,
  ...o,
})
const texts = (rows: Js8ActivityRow[], o: Partial<Js8RxOptions> = {}) => js8RxLines(rows, rx(o)).map((l) => [l.offsetHz, l.text])

describe('the marks', () => {
  it('closes a last frame as JS8Call does: the text stripped, a space, the mark, a space', () => {
    expect(withEot('HELLO   ', '♢')).toBe('HELLO ♢ ')
    expect(withEot('HELLO', '')).toBe('HELLO  ')
  })
  it('names the speed as JS8Call’s columns do: the first letter, or the number of a JS8 40/60', () => {
    // JS8Call 3.0's names: Turbo is JS8 40, and JS8 60 is the fifth speed.
    expect(['slow', 'normal', 'fast', 'turbo', 'ultra'].map((s) => speedMark(s as Js8ActivityRow['speed']))).toEqual(['S', 'N', 'F', '40', '60'])
    expect(['turbo', 'ultra'].map((s) => speedName(s as Js8ActivityRow['speed']))).toEqual(['JS8 40', 'JS8 60'])
  })
  it('reads the addressee of a directed frame and a call by itself or its base call on a list', () => {
    expect(addresseeOf('W1AW: KD9TAW SNR? ')).toBe('KD9TAW')
    expect(addresseeOf('W1AW: @FUN MSG ')).toBe('@FUN')
    expect(addresseeOf('HELLO WORLD')).toBe('')
    expect(onCallList(['W1AW'], 'w1aw/p')).toBe(true)
    expect(onCallList(['W1AW/P'], 'W1AW')).toBe(false)
  })
})

describe('the RX pane (processRxActivity, displayTextForFreq)', () => {
  it('joins a message to me into one line wherever it is, and closes it with the end mark', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1200.6, from: 'W1AW', directedToMe: true, text: 'W1AW: KD9TAW MSG ' }, true, false),
      frame({ atMs: T0 + P, freqHz: 1203.2, text: 'HELLO WORLD' }, false, true),
    ]
    expect(texts(rows)).toEqual([[1200, 'W1AW: KD9TAW MSG HELLO WORLD ♢ ']])
    expect(js8RxLines(rows, rx())[0]).toMatchObject({ directed: true, from: 'W1AW', mine: false })
  })

  it('prints what is heard on my offset, heartbeats and CQs included, and nothing elsewhere that is not for me', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1508, from: 'K1ABC', text: 'K1ABC: @ALLCALL CQ CQ CQ FN42 ' }),
      frame({ atMs: T0, freqHz: 700, from: 'N0FAR', text: 'N0FAR: @ALLCALL CQ CQ CQ EM12 ' }),
      frame({ atMs: T0, freqHz: 900, from: 'W9XYZ', directedToMe: false, text: 'W9XYZ: K1ABC SNR? ' }),
    ]
    expect(texts(rows)).toEqual([[1508, 'K1ABC: @ALLCALL CQ CQ CQ FN42 ♢ ']])
  })

  it('prints a single-frame command to me or my group anywhere, and @ALLCALL only when it is free text', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 900, from: 'W9XYZ', directedToMe: true, text: 'W9XYZ: KD9TAW SNR? ' }),
      frame({ atMs: T0, freqHz: 1100, from: 'G0ABC', directedToMe: true, text: 'G0ABC: @FUN SNR? ' }),
      frame({ atMs: T0, freqHz: 1700, from: 'N0ALL', directedToMe: true, text: 'N0ALL: @ALLCALL SNR? ' }),
      frame({ atMs: T0 + P, freqHz: 2000, from: 'N0TXT', directedToMe: true, text: 'N0TXT: @ALLCALL  ' }, true, false),
      frame({ atMs: T0 + 2 * P, freqHz: 2001, text: 'NET TONIGHT' }, false, true),
    ]
    expect(texts(rows)).toEqual([
      [900, 'W9XYZ: KD9TAW SNR? ♢ '],
      [1100, 'G0ABC: @FUN SNR? ♢ '],
      [2000, 'N0TXT: @ALLCALL  NET TONIGHT ♢ '],
    ])
  })

  it('skips a partial directed frame and a directed heartbeat away from a message to me', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'W1AW', text: 'W1AW: <....> SNR? ' }),
      frame({ atMs: T0 + P, freqHz: 1502, from: 'W1AW', text: 'W1AW: HB EN52 ' }),
    ]
    expect(texts(rows)).toEqual([])
  })

  it('starts a new line at a first frame, and every open line again after any message ends', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: ' }, true, false),
      frame({ atMs: T0, freqHz: 1100, from: 'W9XYZ', directedToMe: true, text: 'W9XYZ: KD9TAW SNR? ' }),
      // K1ABC's next frame at my offset: the 1100 Hz command ended, so JS8Call starts a new line.
      frame({ atMs: T0 + P, freqHz: 1500, text: 'HI ALL' }, false, true),
    ]
    expect(texts(rows)).toEqual([
      [1500, 'K1ABC: '],
      [1100, 'W9XYZ: KD9TAW SNR? ♢ '],
      [1500, 'HI ALL ♢ '],
    ])
  })

  it('marks a missing frame between two, and at the end of a message overdue by a period and a half', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: ' }, true, false),
      frame({ atMs: T0 + 3 * P, freqHz: 1500, text: 'ONE' }, false, false),
      frame({ atMs: T0 + 4 * P, freqHz: 1500, text: ' TWO' }, false, false),
    ]
    expect(texts(rows, { nowMs: T0 + 4 * P + 22_000 })).toEqual([[1500, 'K1ABC: ……ONE TWO']])
    expect(texts(rows, { nowMs: T0 + 4 * P + 22_500 })).toEqual([[1500, 'K1ABC: ……ONE TWO……']])
    // The mark is a setting (JS8Call's MFICharacter): here '...'.
    expect(texts(rows, { nowMs: T0 + 6 * P, mfi: '...' })).toEqual([[1500, 'K1ABC: ...ONE TWO...']])
  })

  it('hides a blocked call’s message and the rest of it at that offset, and prints the next one there', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'N0BAD', text: 'N0BAD: ' }, true, false),
      frame({ atMs: T0 + P, freqHz: 1500, text: 'SPAM' }, false, false),
      frame({ atMs: T0 + 2 * P, freqHz: 1500, text: 'MORE' }, false, true),
      frame({ atMs: T0 + 3 * P, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: ' }, true, false),
      frame({ atMs: T0 + 4 * P, freqHz: 1500, text: 'HELLO' }, false, true),
    ]
    expect(texts(rows, { blocklist: ['N0BAD'] })).toEqual([[1500, 'K1ABC: HELLO ♢ ']])
    // By its base call too, and the list off prints it all.
    expect(texts(rows.map((r) => (r.from ? { ...r, from: r.from + '/P', text: r.text.replace(':', '/P:') } : r)), { blocklist: ['N0BAD'] })).toEqual([
      [1500, 'K1ABC/P: HELLO ♢ '],
    ])
    expect(texts(rows)).toEqual([
      [1500, 'N0BAD: SPAMMORE ♢ '],
      [1500, 'K1ABC: HELLO ♢ '],
    ])
  })

  it('prints my own frames as a line closed by the end mark, wherever I sent them', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1750, mine: true, from: 'KD9TAW', text: 'KD9TAW: K1ABC MSG ' }, true, false),
      frame({ atMs: T0 + P, freqHz: 1750, mine: true, from: 'KD9TAW', text: 'HELLO' }, false, true),
    ]
    expect(texts(rows)).toEqual([[1750, 'KD9TAW: K1ABC MSG HELLO ♢ ']])
    expect(js8RxLines(rows, rx())[0].mine).toBe(true)
  })

  it('leaves out what was heard by the time the pane was cleared, and the reassembled line always', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: @ALLCALL CQ CQ CQ FN42 ' }),
      frame({ atMs: T0 + P, freqHz: 1500, from: 'W1AW', text: 'W1AW: @ALLCALL CQ CQ CQ FN31 ' }),
      { ...frame({ atMs: T0 + P, freqHz: 1500, from: 'W1AW', text: 'whole message' }), flags: { first: false, last: false, assembled: true } },
    ]
    expect(texts(rows, { clearedAtMs: T0 })).toEqual([[1500, 'W1AW: @ALLCALL CQ CQ CQ FN31 ♢ ']])
  })

  it('shows a feed that does not say where rows stand (the Remote page’s) as it stands, a row a line', () => {
    const rows = [
      { ...frame({ atMs: T0, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: ' }), flags: undefined },
      { ...frame({ atMs: T0, freqHz: 900, from: 'W1AW', directedToMe: true, text: 'W1AW: KD9TAW SNR? ' }), flags: undefined },
      { ...frame({ atMs: T0, freqHz: 700, from: 'N0FAR', text: 'N0FAR: ' }), flags: undefined },
    ]
    expect(isLegacyFeed(rows)).toBe(true)
    expect(texts(rows)).toEqual([
      [1500, 'K1ABC: '],
      [900, 'W1AW: KD9TAW SNR? '],
    ])
  })
})

const band = (o: Partial<Js8BandOptions> = {}): Js8BandOptions => ({
  nowMs: T0 + 2 * P,
  agingMin: 0,
  selectedHz: null,
  showHb: true,
  eot: '♢',
  mfi: '……',
  myCall: 'KD9TAW',
  primary: [],
  secondary: [],
  clearedAtMs: 0,
  removed: {},
  ...o,
})

describe('Band activity (displayBandActivity, processIdleActivity)', () => {
  it('holds every frame heard at an offset, joined, with the end mark after each message', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1000, from: 'W1AW', text: 'W1AW: @ALLCALL CQ CQ CQ FN31 ' }),
      frame({ atMs: T0 + P, freqHz: 1004, from: 'W1AW', text: 'W1AW: ' }, true, false),
      frame({ atMs: T0 + 2 * P, freqHz: 1004, text: 'QRV 40M' }, false, true),
    ]
    const [r] = js8BandRows(rows, band())
    expect(r.offsetHz, 'a drifting signal keeps one entry, under its newest offset').toBe(1004)
    expect(r.text).toBe('W1AW: @ALLCALL CQ CQ CQ FN31 ♢ W1AW: QRV 40M ♢')
    expect(r.cq).toBe(true)
    expect(r.atMs).toBe(T0 + 2 * P)
  })

  it('keeps an offset’s last ten frames, brackets low-confidence copy, and never files my own frames', () => {
    const rows = [
      ...Array.from({ length: 12 }, (_, i) => frame({ atMs: T0 + i * P, freqHz: 800, text: `F${i} ` }, i === 0, false)),
      frame({ atMs: T0, freqHz: 1500, mine: true, from: 'KD9TAW', text: 'KD9TAW: HI ' }),
      frame({ atMs: T0, freqHz: 1200, lowConf: true, text: 'ZZZ' }),
    ]
    const rows2 = js8BandRows(rows, band({ nowMs: T0 + 12 * P }))
    expect(rows2.map((r) => r.offsetHz).sort()).toEqual([1200, 800])
    expect(rows2.find((r) => r.offsetHz === 800)!.text).toBe('F2 F3 F4 F5 F6 F7 F8 F9 F10 F11')
    expect(rows2.find((r) => r.offsetHz === 1200)!.text).toBe('[ZZZ] ♢')
  })

  it('marks a missing frame between two and after the newest, a period and a half on', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 1500, from: 'K1ABC', text: 'K1ABC: ' }, true, false),
      frame({ atMs: T0 + 2 * P, freqHz: 1500, text: 'MID' }, false, false),
    ]
    expect(js8BandRows(rows, band({ nowMs: T0 + 2 * P + 22_000 }))[0].text).toBe('K1ABC: ……MID')
    expect(js8BandRows(rows, band({ nowMs: T0 + 2 * P + 22_500 }))[0].text).toBe('K1ABC: ……MID……')
  })

  it('hides heartbeats and their ACKs on request, with a compound call before one and a MSG ID after', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 600, from: 'W1AW', text: 'W1AW: @HB HEARTBEAT FN31 ' }),
      frame({ atMs: T0, freqHz: 700, from: 'K1ABC', text: 'K1ABC: W1AW HEARTBEAT SNR -05 ' }),
      frame({ atMs: T0, freqHz: 800, from: 'KD9TAW/P', text: 'KD9TAW/P: ' }, true, false),
      frame({ atMs: T0 + P, freqHz: 800, text: ' @HB HEARTBEAT EN52' }, false, false),
      frame({ atMs: T0 + P, freqHz: 800, text: ' MSG ID 3' }, false, true),
      frame({ atMs: T0, freqHz: 900, from: 'N0CQ', text: 'N0CQ: @ALLCALL CQ CQ CQ EN12 ' }),
    ]
    expect(js8BandRows(rows, band()).map((r) => r.offsetHz).sort()).toEqual([600, 700, 800, 900])
    expect(js8BandRows(rows, band({ showHb: false })).map((r) => [r.offsetHz, r.text])).toEqual([[900, 'N0CQ: @ALLCALL CQ CQ CQ EN12 ♢']])
  })

  it('ages each item off unless its offset is selected, and loses what was cleared or removed', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 600, text: 'OLD ' }),
      frame({ atMs: T0 + 10 * 60_000, freqHz: 600, text: 'NEW ' }),
      frame({ atMs: T0, freqHz: 900, text: 'GONE ' }),
    ]
    const now = T0 + 10 * 60_000 + 30_000
    expect(js8BandRows(rows, band({ nowMs: now, agingMin: 2 })).map((r) => [r.offsetHz, r.text])).toEqual([[600, 'NEW ♢']])
    expect(js8BandRows(rows, band({ nowMs: now, agingMin: 2, selectedHz: 900 })).map((r) => r.offsetHz).sort()).toEqual([600, 900])
    expect(js8BandRows(rows, band({ nowMs: now, clearedAtMs: T0 })).map((r) => [r.offsetHz, r.text])).toEqual([[600, 'NEW ♢']])
    // "Remove Activity" on 900 Hz, and a frame heard there after it is filed afresh.
    const later = [...rows, frame({ atMs: T0 + 11 * 60_000, freqHz: 903, text: 'BACK ' })]
    expect(js8BandRows(later, band({ nowMs: now, removed: { '900': T0 + 60_000 } })).map((r) => [r.offsetHz, r.text])).toEqual([
      [600, 'OLD ♢ NEW ♢'],
      [903, 'BACK ♢'],
    ])
  })

  it('tints traffic to me, CQs and the two highlight lists, the primary winning', () => {
    const rows = [
      frame({ atMs: T0, freqHz: 600, from: 'W1AW', directedToMe: true, text: 'W1AW: KD9TAW SNR? ' }),
      frame({ atMs: T0, freqHz: 700, from: 'K1ABC', text: 'K1ABC: NET TONIGHT ' }),
      frame({ atMs: T0, freqHz: 800, from: 'N0ALL', directedToMe: true, text: 'N0ALL: @ALLCALL SNR? ' }),
    ]
    const r = js8BandRows(rows, band({ primary: ['net', 'K1ABC'], secondary: ['NET'] }))
    const by = (hz: number) => r.find((x) => x.offsetHz === hz)!
    expect([by(600).directed, by(700).directed, by(800).directed]).toEqual([true, false, false])
    expect(by(700).highlight).toBe('primary')
    expect(js8BandRows(rows, band({ secondary: ['NET'] })).find((x) => x.offsetHz === 700)!.highlight).toBe('secondary')
  })

  it('sorts as JS8Call: by offset first, then by the choice, then reversed for a "-" choice', () => {
    const rows = js8BandRows(
      [
        frame({ atMs: T0 + 2 * P, freqHz: 900, snrDb: -20, speed: 'slow', text: 'A ' }),
        frame({ atMs: T0, freqHz: 600, snrDb: 5, speed: 'turbo', text: 'B ' }),
        frame({ atMs: T0 + P, freqHz: 1200, snrDb: -3, speed: 'normal', text: 'C ' }),
      ],
      band({ nowMs: T0 + 3 * P }),
    )
    const order = (s: Parameters<typeof sortBandRows>[1]) => sortBandRows(rows, s).map((r) => r.offsetHz)
    expect(order('offset')).toEqual([600, 900, 1200])
    expect(order('timestamp')).toEqual([600, 1200, 900])
    expect(order('-timestamp')).toEqual([900, 1200, 600])
    expect(order('snr')).toEqual([900, 1200, 600])
    expect(order('-snr')).toEqual([600, 1200, 900])
    expect(order('submode')).toEqual([900, 1200, 600])
    expect(order('-submode')).toEqual([600, 1200, 900])
  })

  it('sorts JS8 40 and JS8 60 after Fast by speed, JS8 60 the fastest, and marks them by their numbers', () => {
    const rows = js8BandRows(
      [
        frame({ atMs: T0, freqHz: 500, speed: 'ultra', text: 'U ' }),
        frame({ atMs: T0, freqHz: 700, speed: 'turbo', text: 'T ' }),
        frame({ atMs: T0, freqHz: 900, speed: 'fast', text: 'F ' }),
        frame({ atMs: T0, freqHz: 1100, speed: 'slow', text: 'S ' }),
        frame({ atMs: T0, freqHz: 1300, speed: 'normal', text: 'N ' }),
      ],
      band(),
    )
    const marks = (s: Parameters<typeof sortBandRows>[1]) => sortBandRows(rows, s).map((r) => speedMark(r.speed))
    expect(marks('offset'), 'control: by offset').toEqual(['60', '40', 'F', 'S', 'N'])
    expect(marks('submode')).toEqual(['S', 'N', 'F', '40', '60'])
    expect(marks('-submode')).toEqual(['60', '40', 'F', 'N', 'S'])
  })
})

const heard = (call: string, o: Partial<Js8Heard> = {}): Js8Heard => ({
  call,
  grid: null,
  snrDb: -10,
  freqHz: 1500,
  speed: 'normal',
  lastMs: T0,
  lastHb: false,
  lastCq: false,
  storedMsgs: 0,
  ...o,
})
const calls = (o: Partial<Js8CallOptions> = {}): Js8CallOptions => ({
  nowMs: T0 + 60_000,
  myCall: 'KD9TAW',
  blocklist: [],
  unreadFrom: new Set(),
  activity: [],
  inbox: [],
  added: {},
  removed: {},
  clearedAtMs: 0,
  ...o,
})
const mail = (o: Partial<Js8InboxEntry> & { id: number; from: string }): Js8InboxEntry => ({
  to: 'KD9TAW',
  text: 'HELLO',
  path: [o.from],
  state: 'unread',
  atMs: T0,
  freqHz: 1500,
  snrDb: -5,
  ...o,
})

describe('the call list (displayCallActivity)', () => {
  it('gives each row one mark, in JS8Call’s order: ✕ ⚑ ★ ☎ ⚟', () => {
    const rows = js8CallRows(
      [
        heard('N0BAD', { ackMs: T0, cqMs: T0 }),
        heard('W1MSG', { ackMs: T0 }),
        heard('K1ACK', { ackMs: T0, cqMs: T0 }),
        heard('N0CQ', { cqMs: T0 }),
        heard('N0OLD', { cqMs: T0 - 5 * 60_000 + 60_000 - 1 }),
        heard('W2NONE'),
      ],
      calls({ blocklist: ['N0BAD'], unreadFrom: new Set(['W1MSG']) }),
    )
    expect(rows.map((r) => [r.call, r.mark])).toEqual([
      ['N0BAD', 'blocked'],
      ['W1MSG', 'message'],
      ['K1ACK', 'ack'],
      ['N0CQ', 'cq'],
      ['N0OLD', null],
      ['W2NONE', null],
    ])
    expect(rows[2].markAtMs).toBe(T0)
  })

  it('shows ☎ for five whole minutes, and from the last CQ on a station that kept none', () => {
    const at = (min: number) => js8CallRows([heard('N0CQ', { cqMs: T0 })], calls({ nowMs: T0 + min * 60_000 }))[0].mark
    expect([at(4.99), at(5)]).toEqual(['cq', null])
    const legacy = js8CallRows([heard('N0CQ', { lastCq: true, lastMs: T0 })], calls({ nowMs: T0 + 60_000 }))[0]
    expect(legacy.mark).toBe('cq')
  })

  it('lists the stations a relayed message named, ⚟ through its relay, and marks a heard one until heard directly', () => {
    const inbox = [mail({ id: 1, from: 'N0RLY', path: ['N0RLY', 'K9FAR'], atMs: T0 + 30_000 })]
    const rows = js8CallRows([heard('N0RLY', { lastMs: T0 + 30_000 }), heard('K9FAR', { lastMs: T0 })], calls({ inbox }))
    expect(rows.map((r) => [r.call, r.mark, r.through])).toEqual([
      ['N0RLY', null, null],
      ['K9FAR', 'relay', 'N0RLY'],
    ])
    const only = js8CallRows([], calls({ inbox }))
    expect(only.map((r) => [r.call, r.heard, r.mark, r.lastMs])).toEqual([['K9FAR', null, 'relay', T0 + 30_000]])
    const direct = js8CallRows([heard('K9FAR', { lastMs: T0 + 40_000 })], calls({ inbox }))
    expect(direct[0].mark, 'heard directly since').toBeNull()
  })

  it('takes a station’s time delta from its newest frame in the feed', () => {
    const activity = [
      frame({ atMs: T0, freqHz: 1500, from: 'W1AW', dtS: 0.3, text: 'W1AW: ' }),
      frame({ atMs: T0 + P, freqHz: 1500, from: 'W1AW/P', dtS: -0.2, text: 'W1AW/P: ' }),
    ]
    expect(js8CallRows([heard('W1AW'), heard('K1ABC')], calls({ activity })).map((r) => r.dtS)).toEqual([-0.2, null])
  })

  it('lists a call added by hand until it is removed or the list is cleared, and a removed or cleared station until heard again', () => {
    const rows = (o: Partial<Js8CallOptions>) => js8CallRows([heard('W1AW', { lastMs: T0 })], calls(o)).map((r) => r.call)
    expect(rows({ added: { K1NEW: T0 } })).toEqual(['W1AW', 'K1NEW'])
    expect(rows({ added: { K1NEW: T0 }, clearedAtMs: T0 + 1 })).toEqual([])
    expect(rows({ added: { K1NEW: T0 + 2 }, clearedAtMs: T0 + 1 }), 'added after the clear').toEqual(['K1NEW'])
    expect(rows({ removed: { W1AW: T0 } })).toEqual([])
    expect(rows({ removed: { W1AW: T0 - 1 } }), 'heard after its removal').toEqual(['W1AW'])
    expect(
      js8CallRows([heard('W1AW', { lastMs: T0 })], calls({ clearedAtMs: T0 + 1, unreadFrom: new Set(['W1AW']) })).map((r) => r.call),
      'a station with a message for me stays, as JS8Call’s inbox count brings it back',
    ).toEqual(['W1AW'])
  })

  it('sorts as JS8Call: by callsign, then the choice, the missing distances last both ways, messages to the top', () => {
    const rows = js8CallRows(
      [
        heard('W1AW', { grid: 'FN31', snrDb: -5, freqHz: 900, lastMs: T0 + 2, ackMs: T0 + 1 }),
        heard('K1ABC', { grid: null, snrDb: 3, freqHz: 1500, lastMs: T0 + 1 }),
        heard('N0GRD', { grid: 'EN52', snrDb: -20, freqHz: 600, lastMs: T0 + 3, ackMs: T0 + 5 }),
      ],
      calls(),
    )
    const grids = new Map(rows.map((r) => [r.call, r.heard?.grid ?? ''] as [string, string]))
    const order = (sort: Parameters<typeof sortCallRows>[1], unread: string[] = []) =>
      sortCallRows(rows, sort, { myGrid: 'EN61', grids, unreadFrom: new Set(unread) }).map((r) => r.call)
    expect(order('callsign')).toEqual(['K1ABC', 'N0GRD', 'W1AW'])
    expect(order('offset')).toEqual(['N0GRD', 'W1AW', 'K1ABC'])
    expect(order('distance')).toEqual(['N0GRD', 'W1AW', 'K1ABC'])
    expect(order('-distance')).toEqual(['W1AW', 'N0GRD', 'K1ABC'])
    expect(order('ackTimestamp')).toEqual(['N0GRD', 'W1AW', 'K1ABC'])
    expect(order('-timestamp')).toEqual(['N0GRD', 'W1AW', 'K1ABC'])
    expect(order('-snr')).toEqual(['K1ABC', 'W1AW', 'N0GRD'])
    expect(order('callsign', ['W1AW'])).toEqual(['W1AW', 'K1ABC', 'N0GRD'])
  })

  it('sorts stations heard at JS8 40 and JS8 60 after Fast by speed, JS8 60 the fastest', () => {
    const rows = js8CallRows(
      [heard('W1UL', { speed: 'ultra' }), heard('W1TB', { speed: 'turbo' }), heard('W1FA', { speed: 'fast' }), heard('W1SL', { speed: 'slow' })],
      calls(),
    )
    const order = (sort: Parameters<typeof sortCallRows>[1]) =>
      sortCallRows(rows, sort, { myGrid: 'EN61', grids: new Map(), unreadFrom: new Set() }).map((r) => r.call)
    expect(order('callsign'), 'control: by callsign').toEqual(['W1FA', 'W1SL', 'W1TB', 'W1UL'])
    expect(order('submode')).toEqual(['W1SL', 'W1FA', 'W1TB', 'W1UL'])
    expect(order('-submode')).toEqual(['W1UL', 'W1TB', 'W1FA', 'W1SL'])
  })

  it('puts @ALLCALL and my groups on top, in order, ⚑ while one holds an unread message', () => {
    const inbox = [mail({ id: 1, from: 'W1AW', to: '@NET' }), mail({ id: 2, from: 'W1AW', to: '@FUN', state: 'read' })]
    expect(js8GroupRows(['@net', '@FUN', '@NET'], inbox, true)).toEqual([
      { call: '@ALLCALL', unread: false },
      { call: '@FUN', unread: false },
      { call: '@NET', unread: true },
    ])
    expect(js8GroupRows([], inbox, false)).toEqual([])
  })
})

describe('the inbox (MessagePanel)', () => {
  const inbox = [
    mail({ id: 1, from: 'W1AW', atMs: T0 + 2 }),
    mail({ id: 2, from: 'K1ABC', atMs: T0 + 1, state: 'read' }),
    mail({ id: 3, from: 'KD9TAW', to: 'W1AW', atMs: T0 + 3, state: 'store' }),
    mail({ id: 4, from: 'N0RLY', path: ['N0RLY', 'K9FAR'], atMs: T0, state: 'read' }),
    mail({ id: 5, from: 'G0ABC', to: '@FUN', atMs: T0 + 4 }),
  ]
  it('shows one station’s messages: what it sent me and what I hold for it; a group’s, what was sent to it', () => {
    expect(js8InboxFor(inbox, 'w1aw').map((e) => e.id)).toEqual([1, 3])
    expect(js8InboxFor(inbox, '@FUN').map((e) => e.id)).toEqual([5])
    expect(js8InboxFor(inbox, null)).toHaveLength(5)
  })
  it('sorts by a column either way, newest first to begin with', () => {
    expect(sortInbox(inbox, { key: 'time', descending: true }).map((e) => e.id)).toEqual([5, 3, 1, 2, 4])
    expect(sortInbox(inbox, { key: 'from', descending: false }).map((e) => e.id)).toEqual([5, 2, 4, 3, 1])
    expect(sortInbox(inbox, { key: 'id', descending: false }).map((e) => e.id)).toEqual([1, 2, 3, 4, 5])
  })
  it('reads the path last hop first, and replies by the path the message came', () => {
    expect(inboxFromLabel(inbox[3])).toBe('K9FAR via N0RLY')
    expect(js8ReplyText(inbox[3])).toBe('N0RLY>K9FAR MSG [MESSAGE]')
    expect(js8ReplyText(inbox[0])).toBe('W1AW MSG [MESSAGE]')
  })
})
