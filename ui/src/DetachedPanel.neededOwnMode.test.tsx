// @vitest-environment jsdom
//
// THE TORN-OFF NEEDED BOARD OPENS A ROW'S OWN SCREEN TOO (operator ruling 2026-10-12: "JS8 rows open the
// JS8 screen, PSK rows the PSK screen, FT4 the FT screen on FT4").
//
// This window cannot navigate the main one. Its Work tells `work_spot` the screen's section, and the engine
// hands that back as the hint the main window follows (App.neededOwnMode.test.tsx holds the main window to
// it). So the proof here is what reached the backend: the section, the dial, the tier, and for a PSK row the
// screen's sub-mode. The same board object serves the dashboard window's Needed box. Real NeededPanel, real
// click on the operator's row; every api stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'

const fx = vi.hoisted(() => {
  const need = (call: string, over: Record<string, unknown>) => ({
    call, entity: 'United States', band: '20m', zone: 5, tags: ['NewMode'], priority: 30, headline: 'New mode',
    mode: 'Digital', exactMode: null, freqMhz: null, park: null, ...over,
  })
  const ch = (band: string, dialMhz: number) => ({ band, group: 'HF', dialMhz, mode: 'USB', label: band, note: '' })
  return {
    rows: [
      need('K1JSE', { exactMode: 'JS8' }),
      need('W1PSK', { exactMode: 'PSK31' }),
      need('W2QPK', { exactMode: 'QPSK31' }),
      need('K4POT', {
        exactMode: 'FT4', freqMhz: 14.0815, tags: ['NewPark', 'Pota'], park: { program: 'POTA', reference: 'US-0004' },
      }),
      // CONTROLS: this window always names the FT tier a row's label says, and nothing for a class-only row.
      need('K6FTE', { mode: 'FT8', exactMode: 'FT8' }),
      need('K7DIG', { tags: ['NewBand'] }),
    ],
    plans: {
      tier: [ch('20m', 14.074)],
      js8: [ch('20m', 14.078)],
      psk: [ch('40m', 7.07), ch('20m', 14.07)],
    } as Record<string, unknown[]>,
    psk: { armed: true, sending: false, latched: false, mode: 'psk31', reverse: true },
  }
})

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    subscribeSnapshot: vi.fn(() => () => {}),
    getNeedAlerts: vi.fn(async () => fx.rows),
    getBandPlan: vi.fn(async () => fx.plans.tier),
    getLicensedBandPlan: vi.fn(async (mode: string) => fx.plans[mode] ?? []),
    getPskState: vi.fn(async () => fx.psk),
    pskSetMode: vi.fn(async (mode: string, reverse: boolean) => ({ ...fx.psk, mode, reverse })),
    workSpot: vi.fn(async () => null),
    setHuntTarget: vi.fn(async () => null),
  }
})

import { DetachedPanel } from './DetachedPanel'
import * as api from './api'

/** Every transmit-capable command the api exposes (App.connectBoards.test.tsx's list). */
const TX_VERBS = [
  'callStation', 'startCq', 'startRunning', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
  'playVoiceMessage', 'setTxEnabled', 'setTune', 'atuTune', 'setBeacon', 'aprsSendBeacon', 'aprsSendMessage',
  'rttyAutoArm', 'rttySend', 'rttySetLatched', 'rttyType', 'rttyAutoCq', 'rttyAutoAnswer', 'pskSend',
  'pskSetLatched', 'pskType', 'js8Send', 'js8SendCommand', 'js8CallCq', 'js8Arm', 'js8CqRepeat', 'sstvSend',
] as const
const called = () =>
  Object.entries(api).filter(([, fn]) => ((fn as { mock?: { calls: unknown[] } }).mock?.calls.length ?? 0) > 0).map(([n]) => n)

const row = (call: string) =>
  screen.getAllByRole('row').find((r) => r.getAttribute('aria-label')?.includes(call)) as HTMLElement

beforeEach(() => {
  localStorage.clear()
  for (const fn of Object.values(api)) (fn as { mockClear?: () => void }).mockClear?.()
})
afterEach(cleanup)

async function work(call: string) {
  render(<DetachedPanel panel="needed" />)
  await waitFor(() => expect(row(call)).toBeTruthy())
  fireEvent.click(row(call))
  await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
  return vi.mocked(api.workSpot).mock.calls.map((c) => [...c])
}

describe('the torn-off Needed board works a row on the screen of its own mode', () => {
  it('a JS8 row: the JS8 section hint, the band’s JS8 channel, the JS8 tier', async () => {
    expect(await work('K1JSE')).toEqual([['js8', 14.078, '20m', 'K1JSE', 'JS8']])
    expect(called().filter((n) => (TX_VERBS as readonly string[]).includes(n))).toEqual([])
  })

  it('a PSK31 row: the PSK section at the band’s PSK31 channel, on the sub-mode it is already on', async () => {
    expect(await work('W1PSK')).toEqual([['keyboard', 14.07, '20m', 'W1PSK', undefined]])
    // After the Work: the screen is read and, already on PSK31, left alone.
    await waitFor(() => expect(api.getPskState).toHaveBeenCalled())
    expect(api.pskSetMode).not.toHaveBeenCalled()
    expect(called().filter((n) => (TX_VERBS as readonly string[]).includes(n))).toEqual([])
  })

  it('a QPSK31 row: the PSK section, and the screen moved to QPSK31 with its Rev kept', async () => {
    expect(await work('W2QPK')).toEqual([['keyboard', 14.07, '20m', 'W2QPK', undefined]])
    await waitFor(() => expect(vi.mocked(api.pskSetMode).mock.calls).toEqual([['qpsk31', true]]))
    expect(called().filter((n) => (TX_VERBS as readonly string[]).includes(n))).toEqual([])
  })

  it('a POTA row heard in FT4: the FT screen on FT4, at the spot’s own frequency', async () => {
    expect(await work('K4POT')).toEqual([['digital', 14.0815, '20m', 'K4POT', 'FT4']])
  })

  it('controls: an FT8 row and a class-only row land where they did', async () => {
    expect(await work('K6FTE')).toEqual([['digital', 14.074, '20m', 'K6FTE', 'FT8']])
    vi.mocked(api.workSpot).mockClear()
    fireEvent.click(row('K7DIG'))
    await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
    expect(vi.mocked(api.workSpot).mock.calls).toEqual([['digital', 14.074, '20m', 'K7DIG', undefined]])
  })

  it('JS8 off in Settings ▸ Features: a JS8 row lands where it did before, on the FT screen', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { js8: false } }))
    expect(await work('K1JSE')).toEqual([['digital', 14.074, '20m', 'K1JSE', undefined]])
  })
})
