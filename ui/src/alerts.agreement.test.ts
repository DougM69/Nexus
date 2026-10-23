// ONE DECISION, TWO LANGUAGES: which decodes are worth interrupting the operator for.
//
// Today the screen decides (`processDecodes`, here). The station is to decide instead, once, so a
// computer driving it and a phone hear the same alerts the desktop does: `NeededAlerts`
// (crates/tempo-app/src/needed_alert.rs) is that decision in Rust. Both are held to one table of
// cases that lives with the Rust crate (crates/tempo-app/tests/fixtures/needed-alerts.json): runs of
// decode batches, each with the settings, QSO, watch list, dial and geographic scope it arrives
// under, and the alerts it raises — which toast, its words' parts, how long, how loud, and every
// tone in the order it sounds. The expected values were captured from this function as it stood
// before the Rust port, and each half has a test that walks every case.

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { __resetAlertsForTest, processDecodes, type QsoContext } from './alerts'
import { processPotaAlert, resetPotaAlertsForTest } from './features/potaAlert'
import { pushToast } from './toast'
import type { DecodeRow, OtaMapSpot, Settings } from './types'
import type { WatchFilter } from './watchlist'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
// Every word as its catalog key and parts, so a case reads what the toast is made of, not English.
vi.mock('./i18n', async (actual) => ({
  ...(await actual<typeof import('./i18n')>()),
  t: (key: string, parts?: Record<string, unknown>) => JSON.stringify(parts ? { key, parts } : { key }),
}))
const toasts = vi.mocked(pushToast)

// Every tone the alerts sound, in the order they sound.
const tones: number[] = []
class Tones {
  state = 'running'
  currentTime = 0
  destination = {}
  createOscillator() {
    const osc = {
      type: 'sine',
      frequency: { value: 0, setValueAtTime() {}, exponentialRampToValueAtTime() {} },
      connect() {},
      start: () => tones.push(osc.frequency.value),
      stop() {},
    }
    return osc
  }
  createGain() {
    return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }
  }
}
vi.stubGlobal('window', {
  setTimeout: (run: () => void, ms: number) => globalThis.setTimeout(run, ms),
  AudioContext: Tones,
} as unknown as Window & typeof globalThis)

interface Step {
  atMs: number
  settings?: Record<string, unknown>
  qso?: QsoContext
  watchlist?: WatchFilter[]
  dialMhz?: number
  geoScope?: string[]
  decodes: Record<string, unknown>[]
  alerts: Record<string, unknown>[]
  sounds: number[]
}
interface Table {
  baseDecode: Record<string, unknown>
  baseSettings: Record<string, unknown>
  scenarios: { name: string; steps: Step[] }[]
  pota: { name: string; ticks: { refs: string[]; sounds: number[] }[] }[]
}

const table = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../crates/tempo-app/tests/fixtures/needed-alerts.json'),
    'utf8',
  ),
) as Table

const KINDS: Record<string, string> = {
  'alerts.watch': 'watch',
  'alerts.newDxcc': 'newDxcc',
  'alerts.rareGrid': 'rareGrid',
  'alerts.newGrid': 'newGrid',
  'alerts.myCall': 'myCall',
  'alerts.cq': 'cq',
}

function keyOf(text: unknown): string | null {
  return typeof text === 'string' && text.startsWith('{') ? (JSON.parse(text) as { key: string }).key : null
}

/** One toast, as the table writes an alert. */
function alertOf(call: unknown[]): Record<string, unknown> {
  const [text, tone, ms, options] = call as [string, string, number, { prominent?: boolean; actionLabel: string }]
  const { key, parts } = JSON.parse(text) as { key: string; parts: Record<string, string> }
  const where = parts.where ? (JSON.parse(parts.where) as { parts: { entity: string } }).parts.entity : null
  const tier = keyOf(parts.tier)
  return {
    kind: KINDS[key] ?? key,
    call: keyOf(parts.call) === 'alerts.station' ? null : parts.call,
    entity: where,
    grid: parts.grid ? parts.grid.trim() : null,
    tier: tier ? tier.replace('alerts.tier.', '') : null,
    watch: parts.what ?? null,
    tone,
    ms,
    prominent: options.prominent ?? false,
    action: keyOf(options.actionLabel)?.replace('alerts.action.', ''),
  }
}

/** The table run through the screen's functions: each step's alerts and tones, each tick's tones. */
function observe(): Table {
  const scenarios = table.scenarios.map((scenario) => {
    __resetAlertsForTest()
    const steps = scenario.steps.map((step) => {
      vi.setSystemTime(step.atMs)
      toasts.mockClear()
      tones.length = 0
      processDecodes(
        step.decodes.map((d) => ({ ...table.baseDecode, ...d }) as unknown as DecodeRow),
        { ...table.baseSettings, ...step.settings } as unknown as Settings,
        () => {},
        step.qso ?? { state: null, dxcall: null },
        step.watchlist ?? [],
        step.dialMhz,
        step.geoScope ? new Set(step.geoScope) : null,
      )
      vi.runAllTimers()
      return { ...step, alerts: toasts.mock.calls.map(alertOf), sounds: [...tones] }
    })
    return { ...scenario, steps }
  })
  const pota = table.pota.map((run) => {
    resetPotaAlertsForTest()
    const ticks = run.ticks.map((tick) => {
      tones.length = 0
      processPotaAlert(tick.refs.map((reference) => ({ reference }) as OtaMapSpot))
      vi.runAllTimers()
      return { ...tick, sounds: [...tones] }
    })
    return { ...run, ticks }
  })
  return { ...table, scenarios, pota }
}

beforeEach(() => {
  vi.useFakeTimers()
})
afterEach(() => {
  vi.useRealTimers()
})

describe('the screen and the station raise the same needed-station alerts', () => {
  it('the shared table is there, and raises every kind', () => {
    const kinds = new Set(table.scenarios.flatMap((s) => s.steps.flatMap((st) => st.alerts.map((a) => a.kind))))
    expect([...kinds].sort()).toEqual(['cq', 'myCall', 'newDxcc', 'newGrid', 'rareGrid', 'watch'])
    // Silence occurs too: a table of alerts alone could not tell a decider from one that raises
    // everything.
    expect(table.scenarios.some((s) => s.steps.some((st) => st.alerts.length === 0))).toBe(true)
    expect(table.pota.some((p) => p.ticks.some((t) => t.sounds.length === 0))).toBe(true)
  })

  it('the desktop agrees with the table on every step', () => {
    const seen = observe()
    for (const [i, scenario] of table.scenarios.entries())
      for (const [j, step] of scenario.steps.entries())
        expect(
          { alerts: seen.scenarios[i].steps[j].alerts, sounds: seen.scenarios[i].steps[j].sounds },
          `${scenario.name}, step ${j + 1}`,
        ).toEqual({ alerts: step.alerts, sounds: step.sounds })
    for (const [i, run] of table.pota.entries())
      for (const [j, tick] of run.ticks.entries())
        expect(seen.pota[i].ticks[j].sounds, `${run.name}, tick ${j + 1}`).toEqual(tick.sounds)
  })
})
