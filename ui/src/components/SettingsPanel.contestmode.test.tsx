// @vitest-environment jsdom
//
// CONTEST MODE IN SETTINGS — the one switch that starts whichever contest is picked, offered on
// the Contesting tab and among the Appearance tab's features, and at the top of the Contest
// screen (`ContestView.modeSwitch.test.tsx`).
//
// It was named "Field Day mode" and described as Field Day's ("Turn on for Field Day weekend: the
// Class/Section exchange appears across all modes"), while it already started the Illinois QSO
// Party or CQ WW just the same. So an operator in a QSO party read that the log strip, the
// operator box and the contest keys were Field Day's. Every sentence here is compared whole:
// each names the contest the operator picked, or Field Day only where the line is Field Day's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the contest-pick suite's reason).
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

function renderPanel() {
  return render(
    <SettingsPanel
      fieldDay={null}
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

/** The station holds these settings, over the shipped defaults. */
const holding = (over: Record<string, unknown>) =>
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52', ...over } as never),
  )

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  holding({})
})
afterEach(cleanup)

const openTab = async (name: string) => fireEvent.click(await screen.findByRole('tab', { name }))

/** The switch by its accessible name, and the hint printed beside it. */
const switchNamed = async (name: string) => (await screen.findByRole('switch', { name })) as HTMLButtonElement
const hintOf = (sw: HTMLElement) => sw.closest('.settings-field')!.querySelector('.settings-hint')!.textContent

const ON_CONTESTING = (contest: string) =>
  `For ${contest}, the contest picked above. When it is on, the log strips in every mode, the Contest screen and the exports follow its rules. Off the rest of the year. Save to apply. The same switch is at the top of the Contest screen.`

describe('Settings ▸ Contesting: the switch is Contest mode, and names the picked contest', () => {
  it('a QSO party: its name on the switch and in the hint, and no Field Day anywhere on it', async () => {
    holding({ fdEvent: 'ilqp' })
    renderPanel()
    await openTab('Contesting')
    const sw = await switchNamed('Enable Contest mode: Illinois QSO Party')
    const field = sw.closest('.settings-field')!
    expect(field.querySelector('.settings-label')!.textContent).toBe('Contest mode')
    expect(hintOf(sw)).toBe(ON_CONTESTING('Illinois QSO Party'))
    expect(field.textContent).not.toMatch(/Field Day|Class|Section/)
  })

  it('nothing picked yet is ARRL Field Day, the contest a blank pick runs', async () => {
    holding({ fdEvent: '' })
    renderPanel()
    await openTab('Contesting')
    expect(hintOf(await switchNamed('Enable Contest mode: ARRL Field Day'))).toBe(ON_CONTESTING('ARRL Field Day'))
  })

  it('on, it says so by name: Disable Contest mode and the contest', async () => {
    holding({ fdEvent: 'cqww_cw', fdActive: true })
    renderPanel()
    await openTab('Contesting')
    expect((await switchNamed('Disable Contest mode: CQ World-Wide DX Contest (CW)')).getAttribute('aria-checked')).toBe('true')
  })
})

describe('Settings ▸ Appearance ▸ Features: the same switch under the same name', () => {
  it('names the picked contest, and its hint says where that is picked', async () => {
    holding({ fdEvent: 'ilqp' })
    renderPanel()
    await openTab('Appearance')
    const sw = await switchNamed('Enable Contest mode: Illinois QSO Party')
    expect(sw.closest('.settings-field')!.querySelector('.settings-label')!.textContent).toBe('Contest mode')
    expect(hintOf(sw)).toBe(
      'For Illinois QSO Party, the contest picked on the Contesting tab. When it is on, the log strips in every mode, the Contest screen and the exports follow its rules. Off the rest of the year. Stays on across restarts until you turn it off; Save settings to apply. The same switch is at the top of the Contest screen.',
    )
  })
})

describe('the Class + Section prompt: Field Day’s own line, naming the switch as Contest mode', () => {
  const PROMPT = (event: string) =>
    `Set your Class + Section to start operating. Contest mode is on, but the station won't enter ${event} until both are filled in below.`
  const prompt = () => document.querySelector('#settings-field-day .settings-note')?.textContent ?? null

  it('ARRL Field Day with no class: the event by name', async () => {
    holding({ fdEvent: 'arrlfd', fdActive: true, fdClass: '', fdSection: 'WI' })
    renderPanel()
    await openTab('Contesting')
    await switchNamed('Disable Contest mode: ARRL Field Day')
    expect(prompt()).toBe(PROMPT('ARRL Field Day'))
  })

  it('Winter Field Day the same way', async () => {
    holding({ fdEvent: 'wfd', fdActive: true, fdClass: '', fdSection: '' })
    renderPanel()
    await openTab('Contesting')
    await switchNamed('Disable Contest mode: Winter Field Day')
    expect(prompt()).toBe(PROMPT('Winter Field Day'))
  })

  it('CONTROL: a QSO party sends no class or section, so it never asks for them', async () => {
    holding({ fdEvent: 'ilqp', fdActive: true, fdClass: '', fdSection: '' })
    renderPanel()
    await openTab('Contesting')
    await switchNamed('Disable Contest mode: Illinois QSO Party')
    expect(prompt()).toBeNull()
  })
})

describe('turned on among the features: Contesting opens when the picked contest cannot start yet', () => {
  // The switch on the Appearance tab sends the operator to the Contesting tab when what the
  // contest needs is not set. It asked Field Day's question of every contest: with the class and
  // section set (left from June) a QSO party with no county did not go there, and the station
  // silently stayed out of the contest; with them blank a QSO party ready to start went there
  // for nothing. It asks the Contest screen's own question now (`startBlocker`).
  const selected = (name: string) => screen.getByRole('tab', { name }).getAttribute('aria-selected')
  const PREVIEW = {
    event: 'ilqp',
    rulesYear: 2026,
    bannedModes: [],
    spottingAllowed: true,
    clusterAllowed: true,
    enforcement: 'warn',
    role: 'in_state',
    exchange: ['599', 'COOK'],
    problem: '',
  }
  const NO_COUNTY =
    'Your COUNTY is empty — it is part of the exchange you transmit. Fill it in on the Contesting tab in Settings.'
  const turnOn = async (contest: string) => {
    renderPanel()
    await openTab('Appearance')
    fireEvent.click(await switchNamed(`Enable Contest mode: ${contest}`))
  }

  it('a QSO party with no county opens Contesting, though June’s class and section are still set', async () => {
    holding({ fdEvent: 'ilqp', fdClass: '1D', fdSection: 'WI', contestQthState: 'IL', contestQthCounty: '' })
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ ...PREVIEW, exchange: [], problem: NO_COUNTY }))
    await turnOn('Illinois QSO Party')
    expect(selected('Contesting')).toBe('true')
  })

  it('a QSO party ready to start stays where it is, though it has no class or section', async () => {
    holding({ fdEvent: 'ilqp', fdClass: '', fdSection: '', contestQthState: 'IL', contestQthCounty: 'COOK' })
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve(PREVIEW))
    await turnOn('Illinois QSO Party')
    expect(selected('Appearance')).toBe('true')
    expect((await switchNamed('Disable Contest mode: Illinois QSO Party')).getAttribute('aria-checked')).toBe('true')
  })

  it('turning it OFF never goes anywhere, whatever the contest is missing', async () => {
    holding({ fdEvent: 'ilqp', fdActive: true, contestQthState: 'IL', contestQthCounty: '' })
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ ...PREVIEW, exchange: [], problem: NO_COUNTY }))
    renderPanel()
    await openTab('Appearance')
    fireEvent.click(await switchNamed('Disable Contest mode: Illinois QSO Party'))
    expect(selected('Appearance')).toBe('true')
  })

  it('CONTROL: ARRL Field Day with no class opens Contesting, as it always has', async () => {
    holding({ fdEvent: 'arrlfd', fdClass: '', fdSection: 'WI' })
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ ...PREVIEW, event: 'arrlfd', role: '', exchange: [] }))
    await turnOn('ARRL Field Day')
    expect(selected('Contesting')).toBe('true')
  })
})
