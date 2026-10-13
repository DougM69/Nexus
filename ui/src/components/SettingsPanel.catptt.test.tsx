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
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
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

// The note under PTT Method, the same sentence the setup wizard shows beside its keying choice. It
// is there whatever the method, as you look, not on Save: on VOX, RTS or DTR it says what the choice
// has to match, and on CAT it says why CAT keys nothing. Only for the transceivers among these
// radios (`NO_CAT_PTT_TRANSCEIVERS`): Hamlib keys an IC-7300 itself, and the Malachite DSP is a
// receiver with nothing to key.
describe('Settings ▸ Radio explains the keying under PTT Method, live', () => {
  const NOTE = /^Hamlib has no way to key this radio over CAT\./

  async function openRadio(radio: typeof IC706) {
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsWith(radio)))
    renderPanel()
    fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
    await screen.findByText('Connection')
  }

  it.each(['vox', 'rts', 'cat'])('IC-706MKIIG on %s: the note sits under PTT Method', async (ptt) => {
    await openRadio({ ...IC706, pttMethod: ptt })
    const note = await screen.findByText(NOTE)
    const field = note.closest('.settings-field') as HTMLElement
    expect(within(field).getByText('PTT Method')).toBeTruthy()
    // Nothing was saved: the note is not the save-time warning.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it.each([
    [3073, 'Icom IC-7300'],
    [2049, 'Malachite DSP SDR'],
  ])('no note for model %s (%s)', async (rigModel, rigModelName) => {
    // RTS and CAT both show the note on the IC-706MKIIG above; the model is the only difference.
    for (const pttMethod of ['rts', 'cat']) {
      await openRadio({ ...IC706, rigModel, rigModelName, pttMethod })
      // The panel is up with the PTT Method field on screen, so a missing note is a real absence.
      expect(screen.getByText('PTT Method')).toBeTruthy()
      expect(screen.queryByText(NOTE)).toBeNull()
      cleanup()
    }
  })

  it('appears the moment the IC-706MKIIG is picked, before any Save', async () => {
    api.get('getRigModels').mockImplementation(() =>
      Promise.resolve([
        [3011, 'Icom IC-706MKIIG'],
        [3073, 'Icom IC-7300'],
      ]),
    )
    await openRadio({ ...IC706, rigModel: 3073, rigModelName: 'Icom IC-7300', pttMethod: 'vox' })
    expect(screen.queryByText(NOTE)).toBeNull()
    const model = screen.getByRole('combobox', { name: 'Rig Model' }) as HTMLSelectElement
    await waitFor(() => expect(within(model).getByText(/IC-706MKIIG/)).toBeTruthy())
    fireEvent.change(model, { target: { value: '3011' } })
    expect(await screen.findByText(NOTE)).toBeTruthy()
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })
})
