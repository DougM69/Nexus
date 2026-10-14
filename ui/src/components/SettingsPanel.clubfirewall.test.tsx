// @vitest-environment jsdom
//
// Club sync's Windows Firewall button in Settings ▸ Contesting (the club section): what the
// operator sees on Windows and nowhere else, what a press asks the shell for, and what the section
// says afterwards. The words themselves are pinned in features/clubFirewall.test.ts; this pins the
// wiring: the read when the tab opens, the press, the read-back, and the button with the firewall
// off ("Nothing to allow", nothing pressable).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { ClubFirewall } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// The house budget for a Settings render under a loaded full suite.
vi.setConfig({ testTimeout: 15_000 })

const platform = vi.hoisted(() => ({ windows: true }))
vi.mock('../platform', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    get IS_WINDOWS() {
      return platform.windows
    },
  }
})

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (a hand-kept list throws on mount the
// day a verb is missing from it).
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

const MISSING: ClubFirewall = {
  state: 'missing',
  ours: false,
  blockedPrivate: false,
  blockedPublic: false,
  blocksAllPrivate: false,
  blocksAllPublic: false,
  hostPort: 42073,
  searchPort: 42074,
  boardPort: 7373,
}
const IN_PLACE: ClubFirewall = { ...MISSING, state: 'inPlace', ours: true }

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

async function openContesting() {
  fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  await waitFor(() => expect(document.getElementById('settings-field-day-club')).toBeTruthy())
}

const allow = () => screen.queryByRole('button', { name: EN['settings.fdClub.firewall.allow'] })
const remove = () => screen.queryByRole('button', { name: EN['settings.fdClub.firewall.remove'] })
const stateLine = () => document.querySelector('[data-club-firewall="state"]')?.textContent

beforeEach(() => {
  platform.windows = true
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.17.0'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  api.get('clubFirewallStatus').mockImplementation(() => Promise.resolve(MISSING))
})
afterEach(cleanup)

describe('on Windows', () => {
  it('reads the firewall when the tab opens and offers the button', async () => {
    renderPanel()
    await openContesting()
    await waitFor(() => expect(stateLine()).toBe(EN['settings.fdClub.firewall.missing']))
    expect(api.get('clubFirewallStatus')).toHaveBeenCalled()
    expect(allow()?.hasAttribute('disabled')).toBe(false)
    expect(remove(), 'nothing of Nexus to remove').toBeNull()
  })

  it('presses Allow once, and says what the firewall reads back', async () => {
    api.get('clubFirewallPress').mockImplementation(() => Promise.resolve({ ran: true, status: IN_PLACE }))
    renderPanel()
    await openContesting()
    await waitFor(() => expect(allow()).toBeTruthy())
    fireEvent.click(allow()!)
    await waitFor(() =>
      expect(stateLine()).toBe(
        "Nexus's club sync rules are in Windows Firewall for Private and Public networks: TCP 42073, UDP 42074 and TCP 7373.",
      ),
    )
    expect(api.get('clubFirewallPress').mock.calls).toEqual([['allow']])
    expect(document.querySelector('[data-club-firewall="press"]'), 'it did what was asked').toBeNull()
    // Nexus's rules are there now, so they can be removed.
    api.get('clubFirewallPress').mockImplementation(() =>
      Promise.resolve({ ran: true, status: MISSING }),
    )
    fireEvent.click(remove()!)
    await waitFor(() => expect(stateLine()).toBe(EN['settings.fdClub.firewall.missing']))
    expect(api.get('clubFirewallPress').mock.calls).toEqual([['allow'], ['remove']])
  })

  it('says so when Windows did not add the rules, and when a press is refused', async () => {
    api.get('clubFirewallPress').mockImplementation(() => Promise.resolve({ ran: false, status: MISSING }))
    renderPanel()
    await openContesting()
    await waitFor(() => expect(allow()).toBeTruthy())
    fireEvent.click(allow()!)
    await waitFor(() =>
      expect(document.querySelector('[data-club-firewall="press"]')?.textContent).toBe(
        EN['settings.fdClub.firewall.press.allowFailed'],
      ),
    )
    api.get('clubFirewallPress').mockImplementation(() => Promise.reject('onAir'))
    fireEvent.click(allow()!)
    await waitFor(() =>
      expect(document.querySelector('[data-club-firewall="press"]')?.textContent).toBe(
        EN['settings.fdClub.firewall.press.onAir'],
      ),
    )
  })

  it('reads the firewall again after a save, since the rules are for the ports as saved', async () => {
    renderPanel()
    await openContesting()
    await waitFor(() => expect(api.get('clubFirewallStatus')).toHaveBeenCalledTimes(1))
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((b) => (b as HTMLButtonElement).type === 'submit')!
    fireEvent.click(save)
    await waitFor(() => expect(api.get('clubFirewallStatus')).toHaveBeenCalledTimes(2))
  })

  it('with the firewall off, the button says there is nothing to allow and presses nothing', async () => {
    api.get('clubFirewallStatus').mockImplementation(() => Promise.resolve({ ...MISSING, state: 'off' }))
    renderPanel()
    await openContesting()
    await waitFor(() => expect(stateLine()).toBe(EN['settings.fdClub.firewall.off']))
    expect(allow(), 'the button no longer offers to allow').toBeNull()
    const nothing = screen.getByRole('button', { name: EN['settings.fdClub.firewall.nothing'] })
    expect(nothing.hasAttribute('disabled')).toBe(true)
    fireEvent.click(nothing)
    expect(api.get('clubFirewallPress')).not.toHaveBeenCalled()
  })
})

describe('anywhere else', () => {
  it('shows no button and reads nothing, and still names the ports', async () => {
    platform.windows = false
    renderPanel()
    await openContesting()
    expect(
      screen.getByText(
        'A club event uses TCP 42073 (the Host port), UDP 42074 (Find club events) and TCP 7373 (the club TV scoreboard). A firewall that is on must let Nexus in on them.',
      ),
    ).toBeTruthy()
    expect(allow()).toBeNull()
    expect(document.querySelector('[data-club-firewall]')).toBeNull()
    expect(api.get('clubFirewallStatus')).not.toHaveBeenCalled()
  })
})
