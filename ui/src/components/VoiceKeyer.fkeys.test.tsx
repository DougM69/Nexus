// @vitest-environment jsdom
//
// THE VOICE KEYER'S F-KEYS: each slot's own, F1–F6 and F8. F8 is AGN, the slot Enter Sends
// Message plays for AGN in a contest, from the key AGN is on in the CW and RTTY contest sets and
// in N1MM's default message sets (the operator, 2026-10-10). A key with no slot, F7 among them,
// plays nothing and is left to the rest of the window; the line over the slots names no range.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'

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
