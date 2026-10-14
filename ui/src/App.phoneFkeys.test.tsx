// @vitest-environment jsdom
//
// THE VOICE KEYER'S F-KEYS IN THE REAL APP. On the Phone screen F1 in the log line's Call box plays
// slot 1, and Esc there halts TX as the TX strip's Stop TX does (App's Esc stop, which hears the key
// in a box too). Off the Phone screen the keyer is gone with the cockpit: App mounts the Phone
// cockpit only while it is the screen on show, so a key typed in a Settings field, in Tempo's chat
// or in CW's log line plays no recording, as CW's macros send only from the CW screen.
import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest'
import { act, render, screen, waitFor, within, fireEvent } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import { t } from './i18n'
import type { AppSnapshot, Settings, VoiceMessage } from './types'

// The real App mounts per case; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })
const WAIT = { timeout: 10_000 }

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }
const TX_ON = { ...APP_SNAPSHOT, radio: { ...APP_SNAPSHOT.radio, txEnabled: true } } as unknown as AppSnapshot
/** The snapshot stream App subscribes to, so a case can move the station on. */
let pushSnapshot: (snap: AppSnapshot) => void = () => {}
/** Every slot recorded, so any F-key that reached the keyer would play. */
const RECORDED: VoiceMessage[] = [1, 2, 3, 4, 5, 6, 8].map((slot) => ({ slot, label: `M${slot}`, file: `/voice/slot${slot}.wav` }))

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  // Settings as a fresh install has them, so the Settings screen draws its fields.
  vi.mocked(api.getSettings).mockResolvedValue(settingsFixture as unknown as Settings)
  vi.mocked(api.getVoiceMessages).mockResolvedValue(RECORDED)
  // TX on, so a key that reached the keyer would play rather than be refused. Every command that
  // answers with a snapshot answers with this one, or the first section switch turns TX off again.
  for (const answer of [api.getSnapshot, api.setOperatingMode, api.setArea, api.selectPeer]) {
    vi.mocked(answer as Mock).mockImplementation(async () => TX_ON)
  }
  // A halt answers with the station stopped, TX off, as the engine's does.
  vi.mocked(api.haltTx).mockImplementation(async () => APP_SNAPSHOT as unknown as AppSnapshot)
  vi.mocked(api.subscribeSnapshot).mockImplementation((fn) => {
    pushSnapshot = fn
    return () => {}
  })
  vi.mocked(api.playVoiceMessage).mockClear()
  vi.mocked(api.haltTx).mockClear()
})

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), WAIT)
  await act(async () => {})
}
async function openNav(name: string) {
  const nav = await waitFor(() => {
    const n = document.querySelector('nav.mode-nav')
    expect(n).not.toBeNull()
    return n as HTMLElement
  }, WAIT)
  fireEvent.click(within(nav).getByRole('button', { name }))
  await act(async () => {})
}
/** Press `key` in `el`; true when nothing held it. */
async function pressIn(el: Element, key: string) {
  if (el instanceof HTMLElement) el.focus()
  let free = false
  await act(async () => {
    free = fireEvent.keyDown(el, { key })
  })
  return free
}
const played = () => vi.mocked(api.playVoiceMessage).mock.calls.map((c) => c[0])

describe('the voice keyer’s F-keys in the app', () => {
  it('on the Phone screen F1 in the Call box plays slot 1, and Esc there halts TX', async () => {
    await mountOn('phone')
    const call = await waitFor(() => {
      const c = document.querySelector('main .le-call')
      expect(c, 'the Phone log line did not render').not.toBeNull()
      return c as HTMLInputElement
    }, WAIT)
    await waitFor(() => expect(document.querySelectorAll('.vk-slot')).toHaveLength(RECORDED.length), WAIT)
    expect(await pressIn(call, 'F1'), 'F1 reached the window').toBe(false)
    expect(played()).toEqual([1])
    await pressIn(call, 'Escape')
    expect(api.haltTx, 'Esc in the Call box did not halt TX').toHaveBeenCalledTimes(1)
  })

  it('off the Phone screen a key typed in a Settings field, Tempo’s chat or CW’s log line plays no recording', async () => {
    await mountOn('phone')
    await waitFor(() => expect(document.querySelectorAll('.vk-slot')).toHaveLength(RECORDED.length), WAIT)
    // The positive control, in this same app: the keyer is there and hears a key.
    await pressIn(document.body, 'F2')
    expect(played()).toEqual([2])

    await openNav(t('nav.settings.label'))
    const find = await screen.findByLabelText('Find a setting', undefined, WAIT)
    expect(await pressIn(find, 'F5'), 'F5 was held in a Settings field').toBe(true)
    await pressIn(find, 'F1')
    expect(played(), 'a recording played from a Settings field').toEqual([2])
    expect(document.querySelector('.vk'), 'the keyer is still mounted on Settings').toBeNull()

    // Tempo's chat box is its conversation's, so the station has a peer to talk to there.
    await openNav(t('nav.digital.tempo.title'))
    await act(async () => pushSnapshot({ ...TX_ON, activePeer: 'K1ABC', conversations: [{ peer: 'K1ABC', messages: [] }] }))
    const chatBox = await waitFor(() => {
      const box = [...document.querySelectorAll('.composer-input')].find((x) => !x.closest('[hidden]'))
      expect(box, 'the Tempo screen has no chat box').toBeTruthy()
      return box as HTMLInputElement
    }, WAIT)
    await pressIn(chatBox, 'F1')
    expect(played(), 'a recording played from Tempo’s chat').toEqual([2])
    expect(document.querySelector('.vk'), 'the keyer is still mounted on Tempo').toBeNull()

    await openNav(t('nav.cw.title'))
    const cwCall = await waitFor(() => {
      const c = document.querySelector('main.cw-cockpit .le-call')
      expect(c, 'the CW log line did not render').not.toBeNull()
      return c as HTMLInputElement
    }, WAIT)
    await pressIn(cwCall, 'F3')
    expect(played(), 'a recording played from CW’s log line').toEqual([2])
    expect(document.querySelector('.vk'), 'the keyer is still mounted on CW').toBeNull()
  })
})
