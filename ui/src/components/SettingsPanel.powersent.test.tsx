// @vitest-environment jsdom
//
// POWER SENT, WHERE AN EXCHANGE CARRIES IT. Settings ▸ Contesting ▸ Your station data shows the
// box only for a contest in `CONTESTS_SENDING_POWER`, which is empty in this release because no
// shipped contest sends the station's power (`SettingsPanel.contestgroups.test.tsx` reads every
// contest off the panel; `fd_rules.rs` holds the list to the rules seed). This file is the other
// direction, the one the shipped list cannot show: with a contest on the list, the box is there for
// that contest and for no other, and what is typed in it is saved and comes back.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { CONTESTS } from '../fdEvent'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 15_000 })

// The California QSO Party as if its exchange sent the station's power. Any contest would do; one
// that is not a Field Day shows that nothing else about the pick decides it.
vi.mock('../fdEvent', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  CONTESTS_SENDING_POWER: ['cqp'],
}))

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
  station = { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station) as never))
  api.get('setSettings').mockImplementation((s: unknown) => {
    station = structuredClone(s as Record<string, unknown>)
    return Promise.resolve(null)
  })
})
afterEach(cleanup)

const openContesting = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))
const picker = () => screen.getByRole('group', { name: 'Which contest you are entering' })
/** The Power sent box, or null when the tab does not show it. */
const powerSent = () =>
  ([...document.querySelectorAll('.settings-label')]
    .find((n) => n.textContent === 'Power sent')
    ?.closest('.settings-field')
    ?.querySelector('input') as HTMLInputElement | undefined) ?? null

describe('a contest whose exchange sends the station\'s power', () => {
  it('shows Power sent for that contest and for no other', async () => {
    renderPanel()
    await openContesting()
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    const shown: string[] = []
    for (const c of CONTESTS) {
      fireEvent.click(within(picker()).getByRole('button', { name: c.name }))
      if (powerSent()) shown.push(c.id)
    }
    expect(shown).toEqual(['cqp'])
    // It sits with the rest of what the station sends.
    fireEvent.click(within(picker()).getByRole('button', { name: 'California QSO Party' }))
    expect(powerSent()!.closest('fieldset')!.querySelector('legend')!.textContent).toBe('Your station data')
  })

  it('a blank pick is ARRL Field Day, which is not on the list: no Power sent', async () => {
    station = { ...station, fdEvent: '' }
    renderPanel()
    await openContesting()
    await screen.findByRole('group', { name: 'Field Day power multiplier' })
    expect(powerSent()).toBeNull()
  })

  it('saves what is typed in it, and shows it again after a reload', async () => {
    station = { ...station, fdEvent: 'cqp' }
    const view = renderPanel()
    await openContesting()
    await waitFor(() => expect(powerSent()).not.toBeNull())
    expect(powerSent()!.value).toBe('')
    fireEvent.change(powerSent()!, { target: { value: 'kw' } })
    fireEvent.click(
      screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
    )
    await waitFor(() => expect(station.contestPower).toBe('KW'))
    view.unmount()
    renderPanel()
    await openContesting()
    await waitFor(() => expect(powerSent()?.value).toBe('KW'))
  })
})
