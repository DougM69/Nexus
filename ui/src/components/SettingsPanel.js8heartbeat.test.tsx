// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 — the heartbeat-and-presence switches JS8Call 3.0.3 has: heartbeat
// networking (off), heartbeats outside 500–1000 Hz (off), the QSO pause (ON), the 55-minute
// acknowledgement rule (off) and not taking part in @ALLCALL (off). Each reads its JS8Call
// default, a file from before it reads the same, and a click saves its own key and nothing else
// of theirs. The harness is SettingsPanel.js8.test.tsx's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The five switches' labels, keys and JS8Call defaults. */
const SWITCHES = [
  ['Heartbeat networking', 'js8HbNetworking', false],
  ['Allow heartbeats outside 500–1000 Hz', 'js8HbAnywhere', false],
  ['Pause heartbeats while a call is in the To box', 'js8HbQsoPause', true],
  ['Don’t acknowledge stations that heartbeat more often than every 55 minutes', 'js8HbAckRateLimit', false],
  ['Do not take part in @ALLCALL', 'js8AvoidAllcall', false],
] as const

/** What getSettings answers beyond the fixture (the fixture holds no JS8 key at all). */
const stored = { extra: {} as Record<string, unknown> }

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  stored.extra = {}
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...stored.extra } as never),
  )
})
afterEach(cleanup)

async function openJs8(): Promise<HTMLElement> {
  render(
    <SettingsPanel
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />,
  )
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Transmit speed')
  const fs = document.getElementById('settings-js8')
  expect(fs, 'no <fieldset id="settings-js8">').not.toBeNull()
  return fs as HTMLElement
}

/** The switch in the `.settings-field` whose `.settings-label` span reads `label`. */
function toggle(fs: HTMLElement, label: string): HTMLElement {
  const span = within(fs)
    .getAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  expect(span, `no settings-label span "${label}"`).toBeDefined()
  const el = span!.closest('.settings-field')!.querySelector('button[role="switch"]')
  expect(el, `no switch in the "${label}" field`).not.toBeNull()
  return el as HTMLElement
}

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

describe('Settings ▸ Digital ▸ JS8: heartbeat networking and presence', () => {
  it('a file from before the switches reads each at JS8Call’s default: only the QSO pause on', async () => {
    const fs = await openJs8()
    for (const [label, , on] of SWITCHES) {
      expect(toggle(fs, label).getAttribute('aria-checked'), label).toBe(String(on))
    }
  })

  it('each sits in Automatic transmissions, after the heartbeat deny list', async () => {
    const fs = await openJs8()
    const group = toggle(fs, 'Heartbeat networking').closest('.settings-featgroup')!
    expect(group.querySelector('.settings-featgroup-title')!.textContent).toBe('Automatic transmissions')
    const labels = Array.from(group.querySelectorAll('.settings-label')).map((n) => n.textContent)
    const deny = labels.indexOf('Never acknowledge heartbeats from these callsigns')
    expect(labels.slice(deny + 1, deny + 1 + SWITCHES.length)).toEqual(SWITCHES.map(([label]) => label))
  })

  it.each(SWITCHES)('“%s” flips and saves %s, and only that one of the five', async (label, key, on) => {
    const fs = await openJs8()
    fireEvent.click(toggle(fs, label))
    expect(toggle(fs, label).getAttribute('aria-checked')).toBe(String(!on))
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const calls = api.get('setSettings').mock.calls
    const saved = calls[calls.length - 1][0] as Record<string, unknown>
    // As the Rust side reads the save: a key the form never held takes its default.
    for (const [, k, d] of SWITCHES) {
      expect(saved[k] ?? d, k).toBe(k === key ? !on : d)
    }
  })

  it('a stored choice reads as stored, the QSO pause off included', async () => {
    stored.extra = { js8HbNetworking: true, js8HbAnywhere: true, js8HbQsoPause: false, js8HbAckRateLimit: true, js8AvoidAllcall: true }
    const fs = await openJs8()
    for (const [label, , on] of SWITCHES) {
      expect(toggle(fs, label).getAttribute('aria-checked'), label).toBe(String(!on))
    }
  })
})
