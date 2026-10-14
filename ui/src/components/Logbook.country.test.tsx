// @vitest-environment jsdom
//
// THE COUNTRY A CONTACT READS AS. The stored COUNTRY is free text in its source's spelling — another
// logger exports "UNITED STATES", Nexus writes cty.dat's "United States" — so a log with both read
// as two countries side by side. The Logbook shows the DXCC entity the call resolves to, which is
// what every award and the statistics already count; the stored text stays on the tooltip, and is
// what shows when the call resolves to none.
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest'
import { render, waitFor, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: vi.fn(() => Promise.resolve({})),
    markQslCardById: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function contact(id: string, call: string, whenUnix: number, country: string | null, entity: string | null) {
  return {
    id, call, grid: null, band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: null, rstRcvd: null, name: null, qth: null, comment: null, notes: null,
    country, entity, whenUnix,
    confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
  }
}

beforeEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the Logbook shows the DXCC entity as the country', () => {
  it('shows one country for an imported contact and a logged one, and the stored text for a call that resolves to none', async () => {
    engineLog.mockResolvedValue([
      contact('a', 'K1ABC', 1_790_000_300, 'UNITED STATES', 'United States'),
      contact('b', 'K2DEF', 1_790_000_200, 'United States', 'United States'),
      contact('c', 'XX9ABC', 1_790_000_100, 'Kosovo', null),
    ])
    const r = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
    await waitFor(() => expect(r.container.querySelectorAll('.log-country').length).toBe(3))
    const cells = [...r.container.querySelectorAll<HTMLElement>('.log-country')]
    expect(cells.map((c) => c.textContent)).toEqual(['United States', 'United States', 'Kosovo'])
    // The source's own text is still there to read.
    expect(cells.map((c) => c.title)).toEqual(['UNITED STATES', 'United States', 'Kosovo'])
  })
})
