// @vitest-environment jsdom
//
// THE VOICE KEYER'S F-KEYS: each slot's own, F1–F6 and F8. F8 is AGN, the slot Enter Sends
// Message plays for AGN in a contest, from the key AGN is on in the CW and RTTY contest sets and
// in N1MM's default message sets (the operator, 2026-10-10). A key with no slot, F7 among them,
// plays nothing and is left to the rest of the window; the line over the slots names no range.
// Until the slot list is there, or when it cannot be read, F1–F12 are all held and play nothing,
// so none reaches the webview, where F5 reloads the app on Windows (the operator, 2026-10-11).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'
import { getVoiceMessages } from '../api'
import type { VoiceMessage } from '../types'

const { playVoiceMessage } = vi.hoisted(() => ({
  playVoiceMessage: vi.fn(async (_slot: number): Promise<unknown> => ({})),
}))

vi.mock('../api', () => ({
  // The slots as the settings hold them now, every one recorded.
  getVoiceMessages: vi.fn(async () =>
    [
      [1, 'CQ'],
      [2, 'My Call'],
      [3, 'Report'],
      [4, 'QRZ?'],
      [5, '73'],
      [6, 'Again'],
      [8, 'AGN'],
    ].map(([slot, label]) => ({ slot, label, file: `/voice/slot${slot}.wav` })),
  ),
  playVoiceMessage,
  stopVoice: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))

afterEach(() => {
  cleanup()
  playVoiceMessage.mockClear()
})

async function renderKeyer() {
  const view = render(<VoiceKeyer txEnabled keyed={false} transmitting={false} />)
  await act(async () => {})
  return view
}

/** Press `key` on the window; true when the keyer left it alone (its default not prevented). */
async function press(key: string) {
  let free = false
  await act(async () => {
    free = fireEvent.keyDown(document.body, { key })
  })
  return free
}

const played = () => playVoiceMessage.mock.calls.map((c) => c[0])

describe('the voice keyer’s F-keys', () => {
  it('F1–F6 and F8 each play their own slot', async () => {
    await renderKeyer()
    for (const n of [1, 2, 3, 4, 5, 6, 8]) await press(`F${n}`)
    expect(played()).toEqual([1, 2, 3, 4, 5, 6, 8])
  })

  it('F8 plays the AGN slot, and takes the key', async () => {
    await renderKeyer()
    expect(await press('F8'), 'F8 is the keyer’s').toBe(false)
    expect(played()).toEqual([8])
  })

  it('a key with no slot plays nothing, and is left to the rest of the window', async () => {
    await renderKeyer()
    for (const key of ['F7', 'F9']) {
      expect(await press(key), `the keyer took ${key}`).toBe(true)
    }
    expect(played()).toEqual([])
  })

  it('says how to send without naming a range of keys', async () => {
    const { container } = await renderKeyer()
    expect(container.querySelector('.vk-hint')?.textContent).toBe(
      'click a slot or press its F-key to send · Esc stops',
    )
  })
})

const F1_TO_F12 = Array.from({ length: 12 }, (_, i) => `F${i + 1}`)

describe('the voice keyer’s F-keys before its slots are there', () => {
  it('holds F1–F12 from the moment it opens, and plays nothing', async () => {
    vi.mocked(getVoiceMessages).mockReturnValueOnce(new Promise<VoiceMessage[]>(() => {}))
    await renderKeyer()
    for (const key of F1_TO_F12) {
      expect(await press(key), `${key} reached the window`).toBe(false)
    }
    expect(played()).toEqual([])
    expect(await press('a'), 'a key that is not an F-key is not the keyer’s').toBe(true)
  })

  it('holds F1–F12 when the slot list could not be read', async () => {
    vi.mocked(getVoiceMessages).mockRejectedValueOnce(new Error('unavailable'))
    await renderKeyer()
    for (const key of F1_TO_F12) {
      expect(await press(key), `${key} reached the window`).toBe(false)
    }
    expect(played()).toEqual([])
  })
})
