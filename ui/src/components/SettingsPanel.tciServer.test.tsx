// @vitest-environment jsdom
//
// "Share by TCI" in the Settings panel: it sits in Transmit limits & sharing, right after "Share
// this radio with other programs"; a Save carries the server's settings; the address is the one
// the station reports; Disconnect asks the station to close that program; and a Save that would
// put the CAT broker and the TCI server on one port is refused, naming the port, while a station
// that already had them so still saves anything else.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { RadioStatus, Settings, TciServerStatus } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'
import { t } from '../i18n'

// The house budget: a loaded full suite has run cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module.
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

/** What the station holds: the shipped settings (the CAT broker on 4532, the TCI server off on
 *  40001) with a callsign, and whatever a case puts over them. */
let stored: Partial<Settings> = {}

beforeEach(() => {
  stored = {}
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.19.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...stored } as never),
  )
  // The panel scrolls to the section it opens at; jsdom has no layout to scroll.
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

async function openSharing(radio?: Partial<RadioStatus>): Promise<HTMLElement> {
  render(
    <SettingsPanel
      target="transmit-limits"
      activeRadioId={0}
      radio={radio as RadioStatus | undefined}
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
  await screen.findByRole('switch', { name: EN['settings.transmit.tciServer.label'] })
  return document.querySelector('#settings-transmit-limits') as HTMLElement
}

const shareByTci = () => screen.getByRole('switch', { name: EN['settings.transmit.tciServer.label'] })

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: EN['settings.panel.save'] })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}
const saved = () => api.get('setSettings').mock.lastCall?.[0] as Settings | undefined

const serving = (over: Partial<TciServerStatus> = {}): TciServerStatus => ({
  port: 40001,
  error: null,
  programs: [],
  ...over,
})

describe('Share by TCI in Settings', () => {
  it('sits in Transmit limits & sharing, right after Share this radio with other programs', async () => {
    const section = await openSharing()
    const labels = [...section.querySelectorAll('.settings-label')].map((n) => n.textContent)
    const broker = labels.indexOf(EN['settings.transmit.share.label'])
    expect(broker, 'CONTROL: the CAT broker is in the section').toBeGreaterThanOrEqual(0)
    expect(labels.indexOf(EN['settings.transmit.tciServer.label'])).toBeGreaterThan(broker)
    // Nothing else comes between the two blocks (the broker's own switch for programs' PTT aside).
    const between = labels.slice(broker + 1, labels.indexOf(EN['settings.transmit.tciServer.label']))
    expect(between).toEqual([EN['settings.transmit.foreignPtt.label']])
    expect(section.contains(shareByTci())).toBe(true)
  })

  it('saves the server, its port and "Programs on TCI may transmit" with the form', async () => {
    await openSharing()
    fireEvent.click(shareByTci())
    const port = screen.getByLabelText(EN['settings.transmit.tciServer.port.label'], { exact: false })
    fireEvent.change(port, { target: { value: '40010' } })
    fireEvent.click(screen.getByRole('switch', { name: EN['settings.transmit.tciServer.tx.label'] }))
    fireEvent.click(screen.getByRole('switch', { name: EN['settings.transmit.tciServer.spots.label'] }))
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()).toMatchObject({
      tciServer: true,
      tciServerPort: 40010,
      tciServerTx: true,
      tciServerCw: true,
      tciServerSpots: false,
    })
  })

  it("shows the station's address for the server, and Disconnect asks the station for that program", async () => {
    stored = { tciServer: true }
    const status = serving({
      port: 40002,
      programs: [
        { id: 3, peer: '127.0.0.1:53412', audio: true, keyed: false, refused: null },
        { id: 4, peer: '127.0.0.1:53413', audio: false, keyed: false, refused: null },
      ],
    })
    await openSharing({ tciServer: status })
    expect(document.querySelector('[data-tci-server-address]')?.textContent).toBe('127.0.0.1:40002')
    const rows = screen.getAllByRole('listitem')
    fireEvent.click(within(rows[1]).getByRole('button', { name: EN['settings.transmit.tciServer.disconnect.action'] }))
    await waitFor(() => expect(api.get('tciServerDisconnect')).toHaveBeenCalled())
    expect(api.get('tciServerDisconnect').mock.calls).toEqual([[4]])
  })

  it('refuses a Save that puts the CAT broker and the TCI server on one port, and names it', async () => {
    await openSharing()
    fireEvent.click(shareByTci())
    fireEvent.change(screen.getByLabelText(EN['settings.transmit.tciServer.port.label'], { exact: false }), {
      target: { value: '4532' },
    })
    await clickSave()
    const refusal = await screen.findByText(t('settings.save.tciServerPortClash', { port: 4532 }))
    expect(refusal.getAttribute('role')).toBe('alert')
    expect(api.get('setSettings')).not.toHaveBeenCalled()
    // CONTROL: a port of its own saves.
    fireEvent.change(screen.getByLabelText(EN['settings.transmit.tciServer.port.label'], { exact: false }), {
      target: { value: '40001' },
    })
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()).toMatchObject({ tciServer: true, tciServerPort: 40001, catBrokerPort: 4532 })
  })

  it('still saves anything else for a station that already had the two on one port', async () => {
    // Refused on the change, never on the state: a settings file that already holds the clash (or
    // a computer driving the station, where both ports are the station's) must not lock every
    // other setting out.
    stored = { tciServer: true, tciServerPort: 4532 }
    await openSharing()
    fireEvent.click(screen.getByRole('switch', { name: EN['settings.transmit.bandEdgeTones.label'] }))
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(t('settings.save.tciServerPortClash', { port: 4532 }))).toBeNull()
  })
})
