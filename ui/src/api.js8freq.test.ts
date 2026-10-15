// @vitest-environment jsdom
//
// The JS8 frequency, filter, QSY and schedule wrappers name exactly the Tauri commands src-tauri
// registers, with exactly the argument keys the Rust side destructures (`filter`, `hz`, `go`). A
// misspelt key compiles and fails at the IPC boundary in the field, so the wire is pinned here as
// api.js8.test.ts pins the rest.
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

describe('the JS8 frequency, filter, QSY and schedule wrappers', () => {
  it('name the commands with the exact argument keys', async () => {
    await api.js8SetFilter({ enabled: true, centerHz: 1500, widthHz: 2000 })
    await api.js8DefaultFrequencies()
    await api.js8SetQsyCenter(1500)
    await api.js8QsyToCentre()
    await api.js8HopAnswer(false)
    expect(calls).toEqual([
      { cmd: 'js8_set_filter', args: { filter: { enabled: true, centerHz: 1500, widthHz: 2000 } } },
      { cmd: 'js8_default_frequencies', args: undefined },
      { cmd: 'js8_set_qsy_center', args: { hz: 1500 } },
      { cmd: 'js8_qsy_to_centre', args: undefined },
      { cmd: 'js8_hop_answer', args: { go: false } },
    ])
  })
})
