// @vitest-environment jsdom
//
// The two held-message wrappers name exactly the Tauri commands the Rust side registers, with the
// argument keys it destructures (the api.js8.test.ts pattern): `js8_store_message { to, text }`
// and `js8_inbox_dropped_clear` with none.
import { afterEach, beforeEach, expect, it } from 'vitest'
import * as api from './api'

let calls: { cmd: string; args: unknown }[] = []
beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return { speed: 'normal', rxSpeeds: 15, activity: [], stations: [], inbox: [], queue: [] }
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

it('names the store and clear commands with the exact argument keys', async () => {
  await api.js8StoreMessage('K1ABC', 'SEE YOU FRIDAY')
  await api.js8InboxDroppedClear()
  expect(calls).toEqual([
    { cmd: 'js8_store_message', args: { to: 'K1ABC', text: 'SEE YOU FRIDAY' } },
    { cmd: 'js8_inbox_dropped_clear', args: undefined },
  ])
})
