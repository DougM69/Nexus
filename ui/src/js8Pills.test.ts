// JS8Call 3.0's compose-box pills, by value: which stretch of the typed line each pill covers, what
// kind it is, and what it carries (DirectedMessageParser.cpp read as the spec; js8Pills.ts).
import { describe, expect, it } from 'vitest'
import { isJs8Callsign, js8Pills, type Js8Pill } from './js8Pills'

/** Each pill as [the text it covers, its kind, what it carries]. */
const show = (text: string, toBox = '') =>
  js8Pills(text, toBox).map((p: Js8Pill) => [
    text.slice(p.start, p.end),
    p.kind,
    p.kind === 'command' ? `${p.command}${p.arg ? `(${p.arg})` : ''}${p.toBox ? `>${p.toBox}` : ''}` : p.calls.join('>'),
  ])

describe('JS8Call callsigns, as the pills read them', () => {
  it('takes base, portable, compound and @ calls and the specials, and refuses the rest', () => {
    for (const c of ['W1AW', 'KD9TAW', 'KD9TAW/P', 'VE3/KD9TAW', '3DA0AB', '@ALLCALL', '@HB', '@FUN', '<....>']) {
      expect(isJs8Callsign(c), c).toBe(true)
    }
    for (const c of ['HELLO', 'SNR', '73', 'AB', '@TOOLONGGROUP', '1234', '']) {
      expect(isJs8Callsign(c), c).toBe(false)
    }
  })
})

describe('the pills JS8Call draws for a line', () => {
  it('a call then a command: the recipient and the command', () => {
    expect(show('W1AW SNR?')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['SNR?', 'command', 'snrQ'],
    ])
  })

  it('a command alone goes to the call in the To box, and says so', () => {
    expect(show('HW CPY?', 'W1AW')).toEqual([['HW CPY?', 'command', 'hwCpyQ>W1AW']])
    expect(show('hw cpy?', 'w1aw'), 'typed in lower case').toEqual([['hw cpy?', 'command', 'hwCpyQ>W1AW']])
  })

  it('with no address and nothing in the To box, only a CQ or a heartbeat starts a line', () => {
    expect(show('SNR?')).toEqual([])
    expect(show('CQ CQ CQ EN52 QRV')).toEqual([['CQ CQ CQ EN52', 'command', 'cq(EN52)']])
    expect(show('CQ DX')).toEqual([['CQ DX', 'command', 'cqDx']])
    expect(show('HB EN52')).toEqual([['HB EN52', 'command', 'hb(EN52)']])
    // The longest command at the start is the one asked: HEARTBEAT SNR may not start a line.
    expect(show('HEARTBEAT SNR -10')).toEqual([])
  })

  it('takes the longest command, on a word boundary unless it ends in ? or :', () => {
    expect(show('W1AW QUERY MSGS')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['QUERY MSGS', 'command', 'queryMsgs'],
    ])
    expect(show('W1AW SNRX')).toEqual([['W1AW', 'recipient', 'W1AW']])
    expect(show('W1AW MSG TO:K1ABC HELLO')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['MSG TO:K1ABC', 'command', 'msgTo(K1ABC)'],
    ])
  })

  it('an argument joins its command only when it is good', () => {
    expect(show('W1AW GRID EN52HW')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['GRID EN52HW', 'command', 'grid(EN52HW)'],
    ])
    expect(show('W1AW GRID ZZ99')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['GRID', 'command', 'grid'],
    ])
    expect(show('W1AW QUERY MSG 12')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['QUERY MSG 12', 'command', 'queryMsg(12)'],
    ])
    // QUERY CALL's argument may carry its own `?`; the pill covers it, the call is without it.
    expect(show('W1AW QUERY CALL K1ABC?')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['QUERY CALL K1ABC?', 'command', 'queryCall(K1ABC)'],
    ])
  })

  it('a relay route of two or more good hops is one pill, its last hop the recipient', () => {
    expect(show('W1AW>K1ABC MSG HELLO')).toEqual([
      ['W1AW>K1ABC', 'relay', 'W1AW>K1ABC'],
      ['MSG', 'command', 'msg'],
    ])
    expect(show('W1AW > K1ABC > N0AB SNR?')).toEqual([
      ['W1AW > K1ABC > N0AB', 'relay', 'W1AW>K1ABC>N0AB'],
      ['SNR?', 'command', 'snrQ'],
    ])
    // A route whose second hop is no call has one good hop: the recipient.
    expect(show('W1AW>@FUN')).toEqual([['W1AW', 'recipient', 'W1AW']])
  })

  it('a route that starts with > continues from the call in the To box', () => {
    expect(show('>K1ABC SNR?', 'W1AW')).toEqual([
      ['>K1ABC', 'relay', 'K1ABC'],
      ['SNR?', 'command', 'snrQ'],
    ])
    expect(show('>K1ABC SNR?'), 'nothing in the To box').toEqual([])
  })

  it('a group, and a sender before a colon', () => {
    expect(show('@ALLCALL CQ?')).toEqual([['@ALLCALL', 'group', '@ALLCALL']])
    expect(show('@HB HEARTBEAT EN52')).toEqual([
      ['@HB', 'group', '@HB'],
      ['HEARTBEAT EN52', 'command', 'hb(EN52)'],
    ])
    expect(show('KD9TAW: W1AW SNR?')).toEqual([
      ['KD9TAW:', 'sender', 'KD9TAW'],
      ['W1AW', 'recipient', 'W1AW'],
      ['SNR?', 'command', 'snrQ'],
    ])
  })

  it('a lone ? after an address is a command, and plain words are nothing', () => {
    expect(show('W1AW ?')).toEqual([
      ['W1AW', 'recipient', 'W1AW'],
      ['?', 'command', 'other'],
    ])
    expect(show('HELLO THERE', 'W1AW')).toEqual([])
    expect(show('')).toEqual([])
    expect(show('`KD9TAW/P EN52'), 'a compound announcement').toEqual([])
  })

  it('keeps the offsets of the text as typed, whatever upper-casing does to a letter', () => {
    // `ß` upper-cases to two letters; the pills must still cover the right characters.
    expect(show('W1AW ß SNR?')).toEqual([['W1AW', 'recipient', 'W1AW']])
    expect(show('ß W1AW', 'W1AW')).toEqual([])
  })
})
