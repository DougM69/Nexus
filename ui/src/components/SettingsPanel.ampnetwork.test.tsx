// @vitest-environment jsdom
//
// A KPA1500 reached over the network. There is no separate setting for it: the Connection
// choice writes `host:port` into the amplifier's existing port field, which is what the poller
// reads as the network (`kpa_network_target` in crates/tempo-audio/src/amplifier.rs). So the
// thing to hold down is the STRING that is saved, and that a saved one reads back as the network.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "sends the station-wide write at all — it used to be…", takes
// 0.73 s and 0.74 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Mock EVERY export of `../api`, derived from the real module (the flexperradio pattern — a
// hand-kept list makes the panel throw on mount when a verb is added).
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

/** The ACTIVE radio. Its config is what a station-wide write must carry in the flat mirror. */
/** Newest call to a mocked api verb. (`Array.prototype.at` is outside this project's lib
 *  target, so index explicitly.) */
function lastCall(name: string): unknown[] | undefined {
  const calls = api.get(name).mock.calls
  return calls.length ? (calls[calls.length - 1] as unknown[]) : undefined
}

const FTDX10 = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: 'COM3',
  baud: 38400,
  rigModel: 1042,
  rigModelName: 'Yaesu FTDX10',
  rigConn: 'serial',
  rigAddr: '',
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
  ampModel: 'kpa',
  ampPort: '',
  ampFollowBand: false,
}
/** The NON-ACTIVE radio the operator clicks Edit on — the state the bug lives in. */
const SECOND = {
  ...FTDX10,
  id: 1,
  name: 'IC-7300',
  serialPort: 'COM9',
  rigModel: 3073,
  rigModelName: 'Icom IC-7300',
  rigctldPort: 4534,
  rotctldPort: 4535,
  audioIn: 'in-1',
  audioOut: 'out-1',
}

function settingsFixture() {
  return {
    ...defaultSettings,
    ...FTDX10, // the flat mirror describes the ACTIVE radio
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [FTDX10, SECOND],
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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture()))
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
function clickSave() {
  const save = screen
    .getAllByRole('button', { name: 'Save' })
    .find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}


async function openAmplifier() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  return screen.findByRole('combobox', { name: 'Connection' })
}

describe('Settings: a KPA1500 over the network', () => {
  it('saves the typed address and port as host:port in the amplifier port', async () => {
    renderPanel()
    const connection = await openAmplifier()
    // Serial to begin with: the device box is there and the address boxes are not.
    expect(screen.getByRole('textbox', { name: 'Amplifier port' })).toBeTruthy()
    expect(screen.queryByRole('textbox', { name: 'Amplifier IP address' })).toBeNull()

    fireEvent.change(connection, { target: { value: 'network' } })
    const host = await screen.findByRole('textbox', { name: 'Amplifier IP address' })
    expect(screen.queryByRole('textbox', { name: 'Amplifier port' })).toBeNull()
    expect((screen.getByRole('spinbutton', { name: 'Port' }) as HTMLInputElement).value).toBe('1500')
    fireEvent.change(host, { target: { value: '192.168.1.50' } })
    clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.ampPort).toBe('192.168.1.50:1500')
    expect(payload.ampModel).toBe('kpa')
  })

  it('reads a saved address back as the network, and another family drops it', async () => {
    const saved = { ...FTDX10, ampPort: '10.0.0.7:1501' }
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...(settingsFixture() as object), ...saved, radios: [saved, SECOND] } as never),
    )
    renderPanel()
    const connection = (await openAmplifier()) as HTMLSelectElement
    expect(connection.value).toBe('network')
    expect((screen.getByRole('textbox', { name: 'Amplifier IP address' }) as HTMLInputElement).value).toBe(
      '10.0.0.7',
    )
    expect((screen.getByRole('spinbutton', { name: 'Port' }) as HTMLInputElement).value).toBe('1501')

    // The SPE has no network port: picking it puts the port back to an empty serial device.
    fireEvent.change(screen.getByRole('combobox', { name: 'Amplifier' }), { target: { value: 'spe' } })
    expect(screen.queryByRole('combobox', { name: 'Connection' })).toBeNull()
    expect((screen.getByRole('textbox', { name: 'Amplifier port' }) as HTMLInputElement).value).toBe('')
    clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const payload = lastCall('setSettings')![0] as Record<string, unknown>
    expect(payload.ampModel).toBe('spe')
    expect(payload.ampPort).toBe('')
  })
})
