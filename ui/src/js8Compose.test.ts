// The compose box's rules, by value: JS8Call's `addMessageText` insertion, its CQ-message check, the
// selected call's macro values, the settings' defaults, and the words its checker walks.
import { describe, expect, it } from 'vitest'
import { JS8_COMPOSE_DEFAULTS, js8ComposeConfig, js8CqTextRefused, js8InsertText, js8MacroSelected } from './js8Compose'
import { js8Words } from './js8Suggest'
import type { Js8ActivityRow, Js8Heard, Settings } from './types'

describe("JS8Call's addMessageText", () => {
  it('puts a button\'s words at the caret, after a space unless the caret starts the box or follows one', () => {
    expect(js8InsertText('', 0, 0, 'HW CPY?', false)).toEqual({ text: 'HW CPY?', caret: 7 })
    expect(js8InsertText('TNX', 3, 3, 'HW CPY?', false)).toEqual({ text: 'TNX HW CPY?', caret: 11 })
    expect(js8InsertText('TNX ', 4, 4, 'HW CPY?', false)).toEqual({ text: 'TNX HW CPY?', caret: 11 })
    expect(js8InsertText('TNX 73', 0, 0, 'GM', false), 'at the start: no space').toEqual({ text: 'GMTNX 73', caret: 2 })
    expect(js8InsertText('TNX 73', 3, 3, 'GL', false), 'mid-text: before, not after').toEqual({ text: 'TNX GL 73', caret: 6 })
  })

  it('writes over a selection, and with `clear` replaces everything', () => {
    expect(js8InsertText('TNX FER CALL', 4, 7, 'FOR', false)).toEqual({ text: 'TNX FOR CALL', caret: 7 })
    expect(js8InsertText('TNX FER CALL', 7, 4, 'FOR', false), 'a backward selection').toEqual({ text: 'TNX FOR CALL', caret: 7 })
    expect(js8InsertText('TNX FER CALL', 3, 3, 'SNR?', true)).toEqual({ text: 'SNR?', caret: 4 })
  })

  it('keeps a caret past the end at the end', () => {
    expect(js8InsertText('TNX', 99, 99, '73', false)).toEqual({ text: 'TNX 73', caret: 6 })
  })
})

describe("JS8Call's check on the CQ message", () => {
  it('takes an empty message, one starting with CQ, or one holding the callsign', () => {
    expect(js8CqTextRefused('', 'KD9TAW')).toBe(false)
    expect(js8CqTextRefused('CQ CQ CQ <MYGRID4>', 'KD9TAW')).toBe(false)
    expect(js8CqTextRefused('cq dx', 'KD9TAW'), 'read upper-cased').toBe(false)
    expect(js8CqTextRefused('QRV DE KD9TAW', 'kd9taw')).toBe(false)
    expect(js8CqTextRefused('QRV <MYGRID4>', 'KD9TAW')).toBe(true)
    // `<MYCALL>` is not the callsign: JS8Call reads the text as typed.
    expect(js8CqTextRefused('QRV DE <MYCALL>', 'KD9TAW')).toBe(true)
    // No callsign yet: JS8Call's `contains("")` holds, so it takes any message.
    expect(js8CqTextRefused('QRV <MYGRID4>', '')).toBe(false)
  })
})

describe("the selected call's macro values", () => {
  const heard = (call: string, snrDb: number): Js8Heard =>
    ({ call, grid: null, snrDb, freqHz: 1500, speed: 'normal', lastMs: 0, lastHb: false, lastCq: false, storedMsgs: 0 }) as Js8Heard
  const row = (from: string, atMs: number, dtS: number) => ({ from, atMs, dtS }) as unknown as Js8ActivityRow
  it('is the To box call when heard, its report and its newest decode\'s time delta', () => {
    const stations = [heard('W1AW', -3), heard('K1ABC', 4)]
    const activity = [row('W1AW', 1000, 0.5), row('W1AW/P', 3000, -0.1234), row('K1ABC', 4000, 1)]
    expect(js8MacroSelected(' w1aw ', stations, activity)).toEqual({ call: 'W1AW', snrDb: -3, tdeltaMs: -123 })
    expect(js8MacroSelected('K1ABC', stations, activity)).toEqual({ call: 'K1ABC', snrDb: 4, tdeltaMs: 1000 })
  })
  it('is nothing for a call not heard or an empty box, and has no time delta with no decode left', () => {
    expect(js8MacroSelected('N0AB', [heard('W1AW', -3)], [])).toBeNull()
    expect(js8MacroSelected('', [heard('W1AW', -3)], [])).toBeNull()
    expect(js8MacroSelected('W1AW', [heard('W1AW', -3)], [])).toEqual({ call: 'W1AW', snrDb: -3, tdeltaMs: null })
  })
})

describe('the compose settings', () => {
  it('are JS8Call 3.0.3\'s defaults where Settings does not carry them', () => {
    expect(js8ComposeConfig(null)).toEqual(JS8_COMPOSE_DEFAULTS)
    expect(js8ComposeConfig({} as Settings)).toEqual(JS8_COMPOSE_DEFAULTS)
    expect(JS8_COMPOSE_DEFAULTS).toMatchObject({
      cqText: 'CQ CQ CQ <MYGRID4>',
      replyText: 'HW CPY?',
      savedMessages: ['TNX 73 GL'],
      sendAtOnce: true,
      pills: true,
      wordSuggestions: true,
    })
  })
  it('keep what the operator set, an empty list and the switches off included', () => {
    const s = {
      js8CqText: 'CQ DX <MYGRID4>',
      js8ReplyText: '',
      js8SavedMessages: [],
      js8SendAtOnce: false,
      js8Pills: false,
      js8WordSuggestions: false,
      js8Info: 'QTH',
      js8Status: '',
    } as unknown as Settings
    expect(js8ComposeConfig(s)).toEqual({
      cqText: 'CQ DX <MYGRID4>',
      replyText: '',
      savedMessages: [],
      sendAtOnce: false,
      pills: false,
      wordSuggestions: false,
      info: 'QTH',
      status: '',
    })
  })
})

describe('the words the checker walks', () => {
  it('runs of letters and digits, an @ kept with its word, joined across an apostrophe or a full stop', () => {
    expect(js8Words("hello, @allcall don't 14.078 w1aw/p ¡olé!").map((w) => [w.word, w.start, w.end])).toEqual([
      ['HELLO', 0, 5],
      ['@ALLCALL', 7, 15],
      ["DON'T", 16, 21],
      ['14.078', 22, 28],
      ['W1AW', 29, 33],
      ['P', 34, 35],
      ['OLÉ', 37, 40],
    ])
    expect(js8Words('')).toEqual([])
  })
})
