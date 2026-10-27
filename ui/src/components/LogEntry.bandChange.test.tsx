// @vitest-environment jsdom
//
// A BAND CHANGE CLEARS THE LOG LINE'S UN-LOGGED CALL (operator ruling, 2026-10-26: "Log strip call
// (Phone/CW/digital)"). The engine bumps `bandChangeTick` on every band change, by any route; a
// strip whose cockpit asks for it (`clearCallOnBandChange`) drops the call then and only then.
// Because that can drop a half-typed contact, a line that held more than the call says so, with a
// Restore that puts the line back as it was — the callbook's fill for the call included. A draft
// kept while the cockpit was away clears the same way when it comes back.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { LogEntry } from './LogEntry'
import { logQso, lookupPark, qrzLookup } from '../api'
import type { AppSnapshot } from '../types'

// THE BUDGET: the slowest cases wait out the callbook's 700 ms debounce twice; a loaded full suite
// runs cases many times slower than one core. 15 s is the house budget.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (LogEntry.huntPrefill's pattern); the overrides are
  // the callbook, the park directory and the log write this file reads.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    // The logbook never answers: the recall card is not what this file is about.
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedLog = vi.mocked(logQso)
const mockedPark = vi.mocked(lookupPark)

/** The station at band-change count `tick`, on `band`. */
const at = (tick: number, band = '20m', dialMhz = 14.25) =>
  ({ mycall: 'KD9TAW', mygrid: 'EN52', radio: { band, dialMhz }, hunt: null, bandChangeTick: tick }) as unknown as AppSnapshot

type StripProps = Partial<ComponentProps<typeof LogEntry>>
/** The Phone cockpit's strip, as PhoneCockpit renders it. */
const strip = (tick: number, props: StripProps = {}, band = '20m', dialMhz = band === '20m' ? 14.25 : 7.2) => (
  <LogEntry snap={at(tick, band, dialMhz)} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} draft="phone" clearCallOnBandChange {...props} />
)

const input = (cls: string) => document.querySelector<HTMLInputElement>(`input.${cls}`)!
const call = () => input('le-call')
const comment = () => document.querySelector<HTMLTextAreaElement>('textarea.le-comment')!
const type = (el: HTMLInputElement | HTMLTextAreaElement, value: string) => fireEvent.change(el, { target: { value } })
const notes = () => document.querySelector<HTMLTextAreaElement>('textarea.le-notes')!
const rsts = () => [...document.querySelectorAll<HTMLInputElement>('input.le-rst')]
const notice = () => document.querySelector('.le-band-cleared')
const restore = () => screen.getByRole('button', { name: 'Restore' })
const dismiss = () => screen.getByRole('button', { name: 'Dismiss' })
const logButton = () => screen.getByRole('button', { name: /^log$/i })
type Logged = { call: string; band: string; freqMhz: number; mode: string; name: string | null; qth: string | null; comment: string | null; notes: string | null; rstSent: string; rstRcvd: string; whenUnix: number }
const loggedRec = (i = 0) => mockedLog.mock.calls[i][0] as unknown as Logged

/** EA8XX worked on 20 m phone, every box of the line filled in. */
function fillEa8xx() {
  type(call(), 'EA8XX')
  type(input('le-name'), 'Pedro')
  type(input('le-qth'), 'Tenerife')
  type(comment(), 'first QSO')
  type(notes(), 'pileup')
  type(rsts()[0], '57')
  type(rsts()[1], '55')
}
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })

const HIRAM = { call: 'W1AW', name: 'Hiram', qth: 'Newington', state: 'CT', country: 'United States', grid: 'FN31PR', image: null }

beforeEach(() => {
  mockedQrz.mockReset().mockImplementation(async () => null as never)
  mockedLog.mockReset().mockImplementation(async (rec: unknown) => rec as never)
  mockedPark.mockReset().mockImplementation(async () => null as never)
})

describe('a band change clears the log line\'s call', () => {
  it('a call alone clears too, and Restore can put it back', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect(call().value).toBe('')
    expect(notice()?.textContent).toContain('EA8XX')
    fireEvent.click(restore())
    await settle()
    expect(call().value).toBe('EA8XX')
  })

  it('a snapshot with no band change keeps the call', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    view.rerender(<LogEntry snap={at(1, '20m', 14.26)} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} draft="phone" clearCallOnBandChange />)
    await settle()
    expect(call().value).toBe('EA8XX')
  })

  it('a strip that does not ask (Satellites) keeps the call', async () => {
    const view = render(strip(1, { clearCallOnBandChange: false }))
    type(call(), 'EA8XX')
    view.rerender(strip(2, { clearCallOnBandChange: false }, '40m'))
    await settle()
    expect(call().value).toBe('EA8XX')
  })

  it('the WHOLE line clears, and Restore puts every box back, to log on the band it was made on', async () => {
    const now = vi.spyOn(Date, 'now').mockReturnValue(1_792_342_800_000)
    const view = render(strip(1))
    fillEa8xx()
    now.mockReturnValue(1_792_343_100_000) // five minutes later, the QSY
    view.rerender(strip(2, { mode: 'LSB' }, '40m'))
    await settle()
    expect(call().value).toBe('')
    for (const [box, v] of [['name', input('le-name').value], ['qth', input('le-qth').value], ['comment', comment().value], ['notes', notes().value]])
      expect(v, `${box} goes with the call`).toBe('')
    expect(rsts().map((r) => r.value), 'the reports go back to the default').toEqual(['59', '59'])
    expect(notice()?.textContent).toContain('EA8XX')
    expect(notice()?.textContent, 'the notice says where it will log').toContain('20m')
    fireEvent.click(restore())
    await settle()
    expect(call().value).toBe('EA8XX')
    expect([input('le-name').value, input('le-qth').value, comment().value, notes().value]).toEqual(['Pedro', 'Tenerife', 'first QSO', 'pileup'])
    expect(rsts().map((r) => r.value)).toEqual(['57', '55'])
    expect(notice(), 'the notice goes with the restore').toBeNull()
    now.mockReturnValue(1_792_343_400_000)
    fireEvent.click(logButton())
    await settle()
    expect(mockedLog).toHaveBeenCalledTimes(1)
    const rec = loggedRec()
    expect(rec).toMatchObject({ call: 'EA8XX', name: 'Pedro', qth: 'Tenerife', comment: 'first QSO', notes: 'pileup', rstSent: '57', rstRcvd: '55' })
    expect([rec.band, rec.freqMhz, rec.mode], 'the band, frequency and mode it was made on').toEqual(['20m', 14.25, 'SSB'])
    expect(rec.whenUnix, 'its own start, from when the call was entered').toBe(1_792_342_800)
    now.mockRestore()
  })

  it('the cleared line leaves nothing for the next call', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    type(call(), 'K1ABC')
    await settle()
    fireEvent.click(logButton())
    await settle()
    const rec = loggedRec()
    expect(rec).toMatchObject({ call: 'K1ABC', name: null, qth: null, comment: null, notes: null, rstSent: '59', rstRcvd: '59', band: '40m', freqMhz: 7.2 })
  })

  it('Restore brings back the callbook fill the cleared call took with it', async () => {
    mockedQrz.mockImplementation(async (c: string) => (c === 'W1AW' ? HIRAM : null) as never)
    const view = render(strip(1))
    type(call(), 'W1AW')
    await waitFor(() => expect(input('le-name').value).toBe('Hiram'), { timeout: 3000 })
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect(call().value).toBe('')
    expect(input('le-name').value, 'the callbook fill left with its call').toBe('')
    fireEvent.click(restore())
    await settle()
    expect(call().value).toBe('W1AW')
    expect(input('le-name').value).toBe('Hiram')
    expect(input('le-qth').value).toBe('Newington')
    // …and it is W1AW's again: another call in the line takes none of it.
    type(call(), 'K1ABC')
    await settle()
    expect(input('le-name').value, 'W1AW’s callbook name stayed on K1ABC').toBe('')
  })

  it('typing a new call keeps the notice: Restore stays until a log or Dismiss', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    type(call(), 'K1ABC')
    await settle()
    expect(notice()?.textContent, 'still there with another call typed').toContain('EA8XX')
    fireEvent.click(restore())
    await settle()
    expect(call().value).toBe('EA8XX')
    expect(input('le-name').value).toBe('Pedro')
  })

  it('Dismiss drops the notice, and the line stays as it is', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    type(call(), 'K1ABC')
    fireEvent.click(dismiss())
    await settle()
    expect(notice()).toBeNull()
    expect(call().value).toBe('K1ABC')
  })

  it('logging ANOTHER call keeps the notice: Restore stays until restored, dismissed or the next band change', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    type(call(), 'K1ABC')
    fireEvent.click(logButton())
    await settle()
    expect(loggedRec().call).toBe('K1ABC')
    expect(notice()?.textContent, 'still on offer after a log of another call').toContain('EA8XX')
    fireEvent.click(restore())
    await settle()
    expect([call().value, input('le-name').value]).toEqual(['EA8XX', 'Pedro'])
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec(1).call, loggedRec(1).band, loggedRec(1).freqMhz]).toEqual(['EA8XX', '20m', 14.25])
  })

  it('logging the set-aside call itself on its own band ends the notice', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    view.rerender(strip(3, {}, '20m'))
    await settle()
    type(call(), 'EA8XX')
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().band]).toEqual(['EA8XX', '20m'])
    expect(notice()).toBeNull()
  })

  it('the next band change with another line in it replaces the notice', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    type(call(), 'K1ABC')
    view.rerender(strip(3, {}, '20m', 14.25))
    await settle()
    expect(notice()?.textContent).toContain('K1ABC')
    expect(notice()?.textContent).not.toContain('EA8XX')
  })

  it('the notice and its Restore come back with the cockpit after a page switch', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    view.unmount()
    render(strip(2, {}, '40m'))
    await settle()
    expect(notice()?.textContent).toContain('EA8XX')
    fireEvent.click(restore())
    await settle()
    expect([call().value, input('le-name').value]).toEqual(['EA8XX', 'Pedro'])
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().band, loggedRec().freqMhz]).toEqual(['20m', 14.25])
  })

  it('a band change made while the cockpit was away clears the kept call when it comes back', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    type(comment(), 'first QSO')
    view.unmount()
    render(strip(2, {}, '40m'))
    await settle()
    expect(call().value).toBe('')
    expect(notice()?.textContent).toContain('EA8XX')
    expect(comment().value, 'the whole line goes').toBe('')
    fireEvent.click(restore())
    await settle()
    expect([call().value, comment().value]).toEqual(['EA8XX', 'first QSO'])
  })

  it('…and with no band change meanwhile, the kept call comes back as it was', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    view.unmount()
    render(strip(1))
    await settle()
    expect(call().value).toBe('EA8XX')
  })
})

describe('a band change after a listen off the bands, and a line with no call', () => {
  /** The dial off the bands on a listen (WWV): no band, and no band change counted. */
  const wwv = (tick: number) => strip(tick, {}, '', 10.0)

  it('20 m, a WWV listen, then 40 m: the line is set aside to log on 20 m, at its own frequency', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(wwv(1))
    await settle()
    expect(call().value, 'a listen sets nothing aside').toBe('EA8XX')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect(notice()?.textContent, 'the notice names the band it was made on').toContain('20m')
    fireEvent.click(restore())
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().band, loggedRec().freqMhz]).toEqual(['EA8XX', '20m', 14.25])
  })

  it('…and the same with the cockpit away during the listen and the band change', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(wwv(1))
    await settle()
    view.unmount()
    render(strip(2, {}, '40m'))
    await settle()
    expect(notice()?.textContent).toContain('20m')
    fireEvent.click(restore())
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().band, loggedRec().freqMhz]).toEqual(['20m', 14.25])
  })

  it('a line with a name and a comment but no call is set aside too, and Restore puts it back', async () => {
    const view = render(strip(1))
    type(input('le-name'), 'Pedro')
    type(comment(), 'first QSO')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect([input('le-name').value, comment().value], 'nothing left for the next call').toEqual(['', ''])
    expect(notice()?.textContent, 'the notice says where it will log').toContain('20m')
    fireEvent.click(restore())
    await settle()
    expect([input('le-name').value, comment().value]).toEqual(['Pedro', 'first QSO'])
    type(call(), 'EA8XX')
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect(loggedRec()).toMatchObject({ call: 'EA8XX', name: 'Pedro', comment: 'first QSO', band: '20m', freqMhz: 14.25 })
  })

  it('an empty line sets nothing aside', async () => {
    const view = render(strip(1))
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect(notice()).toBeNull()
  })
})

describe('a hunt\'s prefill is not a line, and a restored line keeps the band it was made on', () => {
  /** A POTA hunt pending for K1ABC at US-0001, a park the directory places in Connecticut. */
  const HUNT = { program: 'POTA', reference: 'US-0001', call: 'K1ABC' }
  const hunted = (tick: number, band = '20m', dialMhz = 14.25) => ({ ...at(tick, band, dialMhz), hunt: HUNT }) as unknown as AppSnapshot
  const park = () => input('le-park-ref')
  const state = () => input('le-state')
  const ct = { reference: 'US-0001', name: 'Test Park', states: ['US-CT'] }

  it('a hunt prefilling the empty line does not replace the set-aside contact at the next band change', async () => {
    mockedPark.mockImplementation(async (r: string) => (r === 'US-0001' ? ct : null) as never)
    const view = render(strip(1, { snap: hunted(1) }))
    type(call(), 'EA8XX')
    type(input('le-name'), 'Pedro')
    await settle()
    view.rerender(strip(2, { snap: hunted(2, '40m', 7.2) }, '40m'))
    await settle()
    expect(notice()?.textContent).toContain('EA8XX')
    // The hunt fills the empty line: its park, and the state the park is in. Neither was typed.
    await waitFor(() => expect([call().value, park().value, state().value]).toEqual(['', 'US-0001', 'CT']), { timeout: 3000 })
    view.rerender(strip(3, { snap: hunted(3, '15m', 21.3) }, '15m'))
    await settle()
    expect(notice()?.textContent, 'EA8XX is still the set-aside contact').toContain('EA8XX')
    expect(notice()?.textContent).toContain('20m')
    fireEvent.click(restore())
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().name, loggedRec().band]).toEqual(['EA8XX', 'Pedro', '20m'])
  })

  it('…and with no hunt, the empty line keeps the set-aside contact the same way', async () => {
    const view = render(strip(1))
    type(call(), 'EA8XX')
    type(input('le-name'), 'Pedro')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    view.rerender(strip(3, {}, '15m', 21.3))
    await settle()
    expect(notice()?.textContent).toContain('EA8XX')
  })

  it('a park and a state the operator typed are a line: a band change sets them aside', async () => {
    const view = render(strip(1))
    type(park(), 'US-0002')
    type(state(), 'NY')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    expect(notice()?.textContent).toContain('20m')
    expect([park().value, state().value]).toEqual(['', ''])
  })

  it('a restored line with no call, emptied by hand, logs a new call on the band it is on', async () => {
    const view = render(strip(1))
    type(input('le-name'), 'Pedro')
    type(comment(), 'first QSO')
    view.rerender(strip(2, {}, '40m'))
    await settle()
    fireEvent.click(restore())
    await settle()
    type(input('le-name'), '')
    type(comment(), '')
    await settle()
    type(call(), 'K1ABC')
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().band, loggedRec().freqMhz]).toEqual(['K1ABC', '40m', 7.2])
  })

  it('a restored line set aside again by the next band change keeps the band it was made on', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    fireEvent.click(restore())
    await settle()
    view.rerender(strip(3, {}, '15m', 21.3))
    await settle()
    expect(notice()?.textContent).toContain('EA8XX')
    expect(notice()?.textContent, 'the band it was made on, not the one it was restored on').toContain('20m')
    fireEvent.click(restore())
    await settle()
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().band, loggedRec().freqMhz]).toEqual(['EA8XX', '20m', 14.25])
  })

  it('a restored line kept across a page switch still logs on the band it was made on', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    fireEvent.click(restore())
    await settle()
    view.unmount()
    render(strip(2, {}, '40m'))
    await settle()
    expect(call().value).toBe('EA8XX')
    fireEvent.click(logButton())
    await settle()
    expect([loggedRec().call, loggedRec().band, loggedRec().freqMhz]).toEqual(['EA8XX', '20m', 14.25])
  })

  it('…and a band change while it was away sets it aside with the band it was made on', async () => {
    const view = render(strip(1))
    fillEa8xx()
    view.rerender(strip(2, {}, '40m'))
    await settle()
    fireEvent.click(restore())
    await settle()
    view.unmount()
    render(strip(3, {}, '15m', 21.3))
    await settle()
    expect(notice()?.textContent).toContain('EA8XX')
    expect(notice()?.textContent).toContain('20m')
  })
})
