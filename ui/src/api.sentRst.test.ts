// @vitest-environment jsdom
//
// The three contest log commands carry the entry's typed "RST sent" report (`sentRst`) ONLY when one
// was typed: without it the arguments are exactly what they always were, key for key, so a station
// on an older build and the hosted page's control transport see nothing new. Pinned the way
// api.workSpot.test.ts pins its own: stub the bridge, record every call, compare strictly.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import * as api from './api'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return {}
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const ex: [string, string][] = [
  ['RST', '599'],
  ['QTH', 'COOK'],
]

describe('the contest log commands and the typed sent report', () => {
  it('send no sentRst key when nothing was typed', async () => {
    await api.contestLogManual('K9NR', ex, 'CW')
    await api.contestLogManualRows('K9NR', [ex], 'CW')
    await api.contestLogSatellite('K9NR', ex, 'CW')
    expect(calls).toStrictEqual([
      { cmd: 'contest_log_manual', args: { call: 'K9NR', fields: ex, mode: 'CW', submode: null } },
      { cmd: 'contest_log_manual_rows', args: { call: 'K9NR', rows: [ex], mode: 'CW', submode: null } },
      { cmd: 'contest_log_satellite', args: { call: 'K9NR', fields: ex, mode: 'CW', submode: null } },
    ])
  })

  it('send the typed report as sentRst on each of the three', async () => {
    await api.contestLogManual('K9NR', ex, 'CW', undefined, undefined, '579')
    await api.contestLogManualRows('K9NR', [ex], 'CW', undefined, undefined, '579')
    await api.contestLogSatellite('K9NR', ex, 'PH', undefined, undefined, '57')
    expect(calls).toStrictEqual([
      { cmd: 'contest_log_manual', args: { call: 'K9NR', fields: ex, mode: 'CW', submode: null, sentRst: '579' } },
      { cmd: 'contest_log_manual_rows', args: { call: 'K9NR', rows: [ex], mode: 'CW', submode: null, sentRst: '579' } },
      { cmd: 'contest_log_satellite', args: { call: 'K9NR', fields: ex, mode: 'PH', submode: null, sentRst: '57' } },
    ])
  })
})
