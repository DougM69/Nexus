// @vitest-environment jsdom
//
// NON-ASSISTED IS A DECLARATION, AND THE UNASSISTED SWITCH IS WHAT MAKES IT TRUE. Picking
// NON-ASSISTED under Spotting assistance writes the Cabrillo log's CATEGORY-ASSISTED line and
// nothing else: the switch that turns the CW decoder, the spots, Super Check Partial and call
// history off is the operator's own act (`contest_category_assisted` is "asked rather than
// inferred"). So while the switch is off, one line beside the picker says so and offers the
// switch. Picking never turns it on; the offer does when it is clicked, and the line goes once the
// switch is on.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n'
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

/** The settings the station holds. */
let station: Record<string, unknown> = {}

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
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  station = { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station) as never))
})
afterEach(cleanup)

const PICKER = 'Whether your entry uses spotting assistance'

/** Settings ▸ Contesting, with the Spotting assistance picker on screen. */
const openPicker = async () => {
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))
  return screen.findByRole('group', { name: PICKER })
}
const pick = (group: HTMLElement, token: string) => fireEvent.click(within(group).getByRole('button', { name: token }))
const offers = () => document.querySelectorAll('[data-unassisted-offer]')
const unassistedSwitch = () => screen.getByRole('switch', { name: / an unassisted contest entry$/ })

describe('NON-ASSISTED with the Unassisted switch off', () => {
  it('picking NON-ASSISTED shows one line beside the picker offering the switch, and turns nothing on', async () => {
    const group = await openPicker()
    expect(offers(), 'control: nothing declared, no line').toHaveLength(0)
    pick(group, 'NON-ASSISTED')
    // First, so a declaration that turned the switch on is named for what it did.
    expect(api.get('setUnassistedMode'), 'asked, never inferred').not.toHaveBeenCalled()
    expect(unassistedSwitch().getAttribute('aria-checked')).toBe('false')
    expect(offers(), 'one line').toHaveLength(1)
    const line = offers()[0] as HTMLElement
    expect(line.textContent).toContain(EN['settings.contestPick.assisted.offer'])
    expect(line.closest('.settings-field'), 'beside the picker').toBe(group.closest('.settings-field'))
    expect(within(line).getByRole('button', { name: EN['settings.contestPick.assisted.offer.action'] })).toBeTruthy()
  })

  it('the offer turns the switch on, and the line goes; switched off again, it comes back', async () => {
    const group = await openPicker()
    pick(group, 'NON-ASSISTED')
    fireEvent.click(within(offers()[0] as HTMLElement).getByRole('button'))
    expect(api.get('setUnassistedMode')).toHaveBeenCalledTimes(1)
    expect(api.get('setUnassistedMode')).toHaveBeenCalledWith(true)
    await waitFor(() => expect(unassistedSwitch().getAttribute('aria-checked')).toBe('true'))
    expect(offers(), 'the switch is on: nothing to offer').toHaveLength(0)
    expect(
      within(group).getByRole('button', { name: 'NON-ASSISTED' }).getAttribute('aria-pressed'),
      'the declaration is left as it was',
    ).toBe('true')
    fireEvent.click(unassistedSwitch())
    await waitFor(() => expect(unassistedSwitch().getAttribute('aria-checked')).toBe('false'))
    expect(offers()).toHaveLength(1)
  })

  it('a stored NON-ASSISTED declaration is offered the switch when Settings opens', async () => {
    station = { ...station, contestCategoryAssisted: 'NON-ASSISTED', unassistedMode: false }
    await openPicker()
    expect(offers()).toHaveLength(1)
    expect(api.get('setUnassistedMode')).not.toHaveBeenCalled()
  })

  it('control: ASSISTED or nothing declared shows no line', async () => {
    const group = await openPicker()
    pick(group, 'ASSISTED')
    expect(offers()).toHaveLength(0)
    pick(group, EN['settings.contestPick.entryAxes.unset'])
    expect(offers()).toHaveLength(0)
  })

  it('control: with the switch already on, NON-ASSISTED shows no line', async () => {
    station = { ...station, contestCategoryAssisted: 'NON-ASSISTED', unassistedMode: true }
    await openPicker()
    expect(unassistedSwitch().getAttribute('aria-checked')).toBe('true')
    expect(offers()).toHaveLength(0)
  })

  it("the pt-BR kit carries the line and its button, as English has them", () => {
    // A path, not `new URL('<literal>', import.meta.url)`: in a jsdom file Vite rewrites that form
    // as an asset URL against the page's location, which is not a file.
    const csv = readFileSync(
      resolve(dirname(fileURLToPath(import.meta.url)), '../../../translations/pt-BR/nexus-ptbr-translation.csv'),
      'utf8',
    )
    for (const key of ['settings.contestPick.assisted.offer', 'settings.contestPick.assisted.offer.action'] as const) {
      expect(csv, key).toContain(`"${key}","${String(EN[key])}"`)
    }
  })
})
