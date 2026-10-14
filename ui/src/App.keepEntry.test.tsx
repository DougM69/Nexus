// @vitest-environment jsdom
//
// A CONTACT TYPED INTO A COCKPIT'S LOG STRIP IS STILL THERE AFTER A TRIP TO ANOTHER PAGE, in the
// real App and by the operator's own path: type the call and the name, click Conditions in the
// navigation, click the cockpit again. The report (a user, 2026-10-14): "while waiting for a
// possibility to work a DX station, I also switch between the Nexus pages. However, if I then have
// to return to, say, the Voice Cockpit, I have to re-enter the call."
//
//  · Phone and CW are drawn by App's view switch, so the trip unmounts them (each case checks that
//    it did, or the case proves nothing), and their strips keep a draft (`LogEntry` `draft`).
//  · RTTY, PSK and JS8 live in keep-alive hosts that never unmount them: their strips always kept
//    the contact, and these cases hold that, so none of them needs a draft.
//  · Logged or cleared, a contact is gone when the cockpit is back; Phone's never shows in CW.
// What may and may not come back, case by case, is LogEntry.draft.test.tsx.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, fireEvent, render, waitFor } from '@testing-library/react'

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
import { COCKPIT_MAIN } from './appCockpits.testkit'

// Each case mounts the real App and navigates it twice; under the full suite's load that outruns
// vitest's default 5 s per test, which is a budget, not a claim about the app.
vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, js8: true, connect: true }
/** Each cockpit's button in the navigation. */
const LABEL: Record<string, string> = { phone: 'Phone', cw: 'CW', rtty: 'RTTY', psk: 'PSK', js8: 'JS8' }

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await act(async () => {})
}

/** ModeNav's button for a section, by its visible label. */
async function navTo(label: string): Promise<void> {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  fireEvent.click(btn!)
  await act(async () => {})
}

/** One of a cockpit's log strip's boxes, once the strip is on screen. */
const box = (section: string, cls: string) =>
  document.querySelector<HTMLInputElement>(`${COCKPIT_MAIN[section]} .log-entry input.${cls}`)
async function strip(section: string): Promise<{ call: HTMLInputElement; name: HTMLInputElement }> {
  return waitFor(() => {
    const call = box(section, 'le-call')
    const name = box(section, 'le-name')
    if (!call || !name) throw new Error(`no log strip in ${section}`)
    return { call, name }
  })
}

/** Over to Conditions and back to `section`, checking the trip really left the cockpit. */
async function roundTrip(section: string): Promise<void> {
  await navTo('Conditions')
  await waitFor(() => expect(document.title).toContain('Conditions'))
  const cockpit = document.querySelector(COCKPIT_MAIN[section])
  // Phone and CW are gone; a keep-alive cockpit is still mounted, behind a hidden host.
  expect(cockpit === null || cockpit.closest('[hidden]') !== null, `${section} is still on screen`).toBe(true)
  await navTo(LABEL[section])
  await waitFor(() => expect(document.querySelector(COCKPIT_MAIN[section])?.closest('[hidden]')).toBeNull())
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  localStorage.setItem('nexus.connect.autopop', 'off')
  localStorage.setItem('nexus.needed.autopop', 'off')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  vi.mocked(api.logQso).mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('Phone and CW: App unmounts them on a page switch, and the contact waits', () => {
  it.each(['phone', 'cw'].map((s) => [s]))('%s: the call and the name are there after Conditions and back', async (section) => {
    await mountOn(section)
    expect(document.querySelector('.view-crash')).toBeNull()
    const { call, name } = await strip(section)
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    fireEvent.change(name, { target: { value: 'Pedro' } })
    await act(async () => {})
    await navTo('Conditions')
    await waitFor(() => expect(document.title).toContain('Conditions'))
    expect(document.querySelector(COCKPIT_MAIN[section]), `premise: ${section} unmounted on the page switch`).toBeNull()
    await navTo(LABEL[section])
    const back = await strip(section)
    expect(back.call.value, `the call typed in ${section} was lost on the page switch`).toBe('EA8XX')
    expect(back.name.value, `the name typed in ${section} was lost on the page switch`).toBe('Pedro')
  })

  it('CW: the call put back is His Call again, so the F-keys send the call the strip shows', async () => {
    await mountOn('cw')
    const { call } = await strip('cw')
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    await act(async () => {})
    await roundTrip('cw')
    await strip('cw')
    const hisCall = () => document.querySelector<HTMLInputElement>(`${COCKPIT_MAIN.cw} input.cw-hiscall-input`)
    await waitFor(() => expect(hisCall()?.value).toBe('EA8XX'))
  })

  it('Phone’s contact never shows in CW, and is still Phone’s afterwards', async () => {
    await mountOn('phone')
    const { call } = await strip('phone')
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    await act(async () => {})
    await navTo('CW')
    const cw = await strip('cw')
    expect(cw.call.value, 'the Phone contact showed up in the CW strip').toBe('')
    await navTo('Phone')
    expect((await strip('phone')).call.value).toBe('EA8XX')
  })
})

describe('RTTY, PSK and JS8: their hosts keep the strip mounted, so the contact was always there', () => {
  it.each(['rtty', 'psk', 'js8'].map((s) => [s]))('%s', async (section) => {
    await mountOn(section)
    expect(document.querySelector('.view-crash')).toBeNull()
    const { call, name } = await strip(section)
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    fireEvent.change(name, { target: { value: 'Pedro' } })
    await act(async () => {})
    await roundTrip(section)
    const back = await strip(section)
    expect(back.call.value).toBe('EA8XX')
    expect(back.name.value).toBe('Pedro')
  })
})

describe('logged or cleared, the contact is gone when the cockpit is back', () => {
  it.each(['phone', 'cw', 'js8'].map((s) => [s]))('%s: logged', async (section) => {
    await mountOn(section)
    const { call, name } = await strip(section)
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    fireEvent.change(name, { target: { value: 'Pedro' } })
    await act(async () => {})
    fireEvent.click(document.querySelector<HTMLButtonElement>(`${COCKPIT_MAIN[section]} .log-entry .le-log-btn`)!)
    await waitFor(() => expect(api.logQso).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(box(section, 'le-call')?.value).toBe(''))
    await roundTrip(section)
    const back = await strip(section)
    expect(back.call.value, `the contact logged in ${section} came back`).toBe('')
    expect(back.name.value).toBe('')
  })

  it.each(['phone', 'cw', 'js8'].map((s) => [s]))('%s: cleared', async (section) => {
    await mountOn(section)
    const { call, name } = await strip(section)
    fireEvent.change(call, { target: { value: 'EA8XX' } })
    fireEvent.change(name, { target: { value: 'Pedro' } })
    await act(async () => {})
    const clear = [...document.querySelectorAll<HTMLButtonElement>(`${COCKPIT_MAIN[section]} .log-entry button`)].find(
      (b) => b.textContent === 'Clear',
    )
    expect(clear, `no Clear in the ${section} strip`).toBeTruthy()
    fireEvent.click(clear!)
    await act(async () => {})
    await roundTrip(section)
    const back = await strip(section)
    expect(back.call.value, `the contact cleared in ${section} came back`).toBe('')
    expect(back.name.value).toBe('')
  })
})
