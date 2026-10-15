// JS8Call's notification sounds (features/js8Sounds): which frame says what, the first poll learning
// what is already there, a new call waiting for the log, and only the enabled sounds, once each.
import { describe, expect, it } from 'vitest'
import type { Js8ActivityRow, Js8Heard } from '../types'
import { JS8_SOUND_EVENTS, Js8SoundWatch, js8FrameEvent, js8SoundsToPlay } from './js8Sounds'

const row = (text: string, o: Partial<Js8ActivityRow> = {}): Js8ActivityRow => ({
  atMs: 1,
  speed: 'normal',
  freqHz: 1500,
  snrDb: -5,
  dtS: 0,
  from: text.split(':')[0],
  text,
  directedToMe: false,
  mine: false,
  complete: true,
  lowConf: false,
  flags: { first: true, last: true, assembled: false },
  ...o,
})
const heard = (call: string): Js8Heard => ({
  call,
  grid: null,
  snrDb: 0,
  freqHz: 1500,
  speed: 'normal',
  lastMs: 1,
  lastHb: false,
  lastCq: false,
  storedMsgs: 0,
})

describe('what a frame announces', () => {
  it('names a CQ, a heartbeat, an ACK for me and a message to me or my group, and nothing else', () => {
    const ev = (text: string, o: Partial<Js8ActivityRow> = {}) => js8FrameEvent(row(text, o), 'KD9TAW', ['@FUN'])
    expect(ev('W1AW: @ALLCALL CQ CQ CQ FN31 ')).toBe('cq')
    expect(ev('W1AW: @HB HEARTBEAT FN31 ')).toBe('hb')
    expect(ev('W1AW: KD9TAW ACK ', { directedToMe: true })).toBe('ack')
    expect(ev('W1AW: KD9TAW SNR? ', { directedToMe: true })).toBe('directed')
    expect(ev('W1AW: KD9TAW/P MSG ', { directedToMe: true }), 'my call portable').toBe('directed')
    expect(ev('W1AW: @FUN SNR? ', { directedToMe: true })).toBe('directed')
    expect(ev('W1AW: @ALLCALL SNR? ', { directedToMe: true }), '@ALLCALL is not directed to me').toBeNull()
    expect(ev('W1AW: K1ABC SNR? ')).toBeNull()
    expect(ev('KD9TAW: @ALLCALL CQ CQ CQ EN61 ', { mine: true }), 'my own').toBeNull()
    expect(ev('HELLO WORLD', { from: '' }), 'a data frame').toBeNull()
    expect(ev('W1AW: @ALLCALL CQ', { flags: { first: false, last: false, assembled: true } }), 'the reassembled line').toBeNull()
  })
})

describe('the watch', () => {
  it('learns what the first poll holds, then says each new frame once', () => {
    const w = new Js8SoundWatch()
    const cq = row('W1AW: @ALLCALL CQ CQ CQ FN31 ')
    expect(w.frames([cq], 'KD9TAW', [])).toEqual([])
    const hb = row('K1ABC: @HB HEARTBEAT FN42 ', { atMs: 2 })
    expect(w.frames([cq, hb], 'KD9TAW', [])).toEqual(['hb'])
    expect(w.frames([cq, hb], 'KD9TAW', [])).toEqual([])
  })

  it('says a new call once the log has answered, old or new, and not the calls already listed', () => {
    const w = new Js8SoundWatch()
    expect(w.newCalls([heard('W1AW')], () => true)).toEqual([])
    const answers = new Map<string, boolean | null>([['K1ABC', null], ['N0NEW', false]])
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual(['callNew'])
    answers.set('K1ABC', true)
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual(['callOld'])
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual([])
  })
})

it('plays the enabled sounds of a poll, each once, in JS8Call’s table order; none when none is on', () => {
  expect(JS8_SOUND_EVENTS).toEqual(['cq', 'hb', 'ack', 'directed', 'inbox', 'callNew', 'callOld'])
  expect(js8SoundsToPlay(['inbox', 'cq', 'cq', 'hb'], ['cq', 'inbox'])).toEqual(['cq', 'inbox'])
  expect(js8SoundsToPlay(['inbox', 'cq'], [])).toEqual([])
})
