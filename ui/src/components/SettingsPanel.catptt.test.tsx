// @vitest-environment jsdom
//
// Settings ▸ Radio on a radio Hamlib cannot key over CAT (`NO_CAT_PTT_MODELS`).
//
// PTT Method CAT on an IC-706MKIIG keys nothing: Hamlib answers `T 1` with RPRT 0 and the radio
// stays on receive, so Nexus shows TX and nothing goes out. Settings keeps the choice (an interface
// that keys the radio on audio, a SignaLink, transmits whatever PTT says) and says so on Save, on
// the pre-save checks' warning line. `rigFormChecks.catptt.test.ts` decides when; this file pins
// that the operator SEES it, which no other test reads off the panel for any rig check.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-12). The slowest case here, "IC-706MKIIG with PTT on CAT: it saves, and the…", takes
// 0.36 s and 0.36 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module — a hand-kept list makes the panel
// throw on mount when a verb is added, which reads as a behaviour regression.
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

const IC706 = {
  id: 0,
  name: 'IC-706MKIIG',
  enabled: true,
  serialPort: 'COM3',
  baud: 19200,
  rigModel: 3011,
  rigModelName: 'Icom IC-706MKIIG',
  rigConn: 'serial',
  rigAddr: '',
  omnirigSlot: 1,
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: 'in-0',
  audioOut: 'out-0',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
}

function settingsWith(radio: typeof IC706) {
  return {
    ...defaultSettings,
    ...radio, // the flat mirror describes the ACTIVE radio (id 0)
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
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
}

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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const HINT = /^Hamlib can't key this radio over CAT/

/** Open the Radio tab on `radio` and press Save; resolves once the save has gone out. */
async function saveRadio(radio: typeof IC706) {
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith(radio)))
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Connection')
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
  // A warning never blocks: the save goes out whatever the checks said.
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
}

describe('Settings ▸ Radio on a radio Hamlib cannot key over CAT', () => {
  it('IC-706MKIIG with PTT on CAT: it saves, and the hint is on screen', async () => {
    await saveRadio(IC706)
    expect((await screen.findByText(HINT)).className).toBe('settings-warn')
  })

  it('IC-706MKIIG on VOX: it saves, and there is no hint', async () => {
    await saveRadio({ ...IC706, pttMethod: 'vox' })
    expect(screen.queryByText(HINT)).toBeNull()
  })
})
