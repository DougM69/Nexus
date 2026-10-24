// @vitest-environment jsdom
//
// THE VOICE KEYER LEAVING THE SCREEN STOPS ONLY A MESSAGE IT STARTED. On a computer driving a
// station, `stop_voice` is a Stop of everything there, so an unmount that sent it every time turned
// TX off at the station whenever the operator left Phone or hid the keyer. It now goes only while
// this pane has a message it started and has not seen end. The ■ Stop button and Esc are the
// operator's own Stop and always send it, played or not.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { VoiceKeyer } from './VoiceKeyer'

const { stopVoice, playVoiceMessage } = vi.hoisted(() => ({
  stopVoice: vi.fn(async (): Promise<unknown> => ({})),
  playVoiceMessage: vi.fn(async (_slot: number): Promise<unknown> => ({})),
}))

vi.mock('../api', () => ({
  getVoiceMessages: vi.fn(async () => [{ slot: 1, label: 'CQ', file: '/tmp/cq.wav' }]),
  playVoiceMessage,
  stopVoice,
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
}))

afterEach(() => {
  cleanup()
  stopVoice.mockClear()
  playVoiceMessage.mockClear()
})

async function renderKeyer() {
  const view = render(<VoiceKeyer txEnabled keyed={false} transmitting={false} />)
  await act(async () => {})
  return view
}

async function playF1(container: HTMLElement) {
  const slot = container.querySelector<HTMLButtonElement>('.vk-play')
  expect(slot, 'precondition: F1 holds a message').not.toBeNull()
  await act(async () => {
    fireEvent.click(slot!)
  })
  expect(playVoiceMessage).toHaveBeenCalledWith(1)
}

describe('the voice keyer leaving the screen', () => {
  it('stops a message it started', async () => {
    const { container, unmount } = await renderKeyer()
    await playF1(container)
    expect(stopVoice).not.toHaveBeenCalled()
    unmount()
    expect(stopVoice).toHaveBeenCalledTimes(1)
  })

  it('sends nothing when it played nothing', async () => {
    const { unmount } = await renderKeyer()
    unmount()
    expect(stopVoice).not.toHaveBeenCalled()
  })

  it('sends nothing once the message it played has ended', async () => {
    const { container, rerender, unmount } = await renderKeyer()
    await playF1(container)
    rerender(<VoiceKeyer txEnabled keyed transmitting />)
    rerender(<VoiceKeyer txEnabled keyed={false} transmitting={false} />)
    unmount()
    expect(stopVoice).not.toHaveBeenCalled()
  })
})

describe("the voice keyer's own Stop", () => {
  it('■ Stop always sends it, played or not', async () => {
    const { container } = await renderKeyer()
    const stop = container.querySelector<HTMLButtonElement>('.vk-stop')
    expect(stop).not.toBeNull()
    fireEvent.click(stop!)
    expect(stopVoice).toHaveBeenCalledTimes(1)
    await playF1(container)
    fireEvent.click(stop!)
    expect(stopVoice).toHaveBeenCalledTimes(2)
  })

  it('Esc always sends it, played or not', async () => {
    const { container } = await renderKeyer()
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(stopVoice).toHaveBeenCalledTimes(1)
    await playF1(container)
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(stopVoice).toHaveBeenCalledTimes(2)
  })
})
