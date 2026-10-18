// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ RTTY ▸ AFSK sideband: LSB (standard, the default) or USB. USB makes the
// backend command the upper DATA submode and swap the tones itself (Rust: `Settings::rig_mode`,
// `Settings::rtty_tone_swap`); this file pins the control's half: what it reads, what it saves,
// and that it is gone for true FSK.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a SettingsPanel render under a loaded full suite.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the SettingsPanel.aprsplacement
// pattern) so a verb missing from a hand-kept list cannot throw on mount.
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
})
afterEach(cleanup)

async function openRtty(stored: Record<string, unknown>): Promise<HTMLElement> {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', ...stored } as never),
  )
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Keying backend')
  return document.getElementById('settings-rtty') as HTMLElement
}

/** The `<select>` in the same field box as the `.settings-label` span `label`, or null. */
function selectFor(fs: HTMLElement, label: string): HTMLSelectElement | null {
  const span = within(fs)
    .queryAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  return (span?.closest('label')?.querySelector('select') as HTMLSelectElement | null) ?? null
}

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

describe('Settings ▸ Digital ▸ RTTY ▸ AFSK sideband', () => {
  it('reads LSB by default, and a USB pick saves under the wire key', async () => {
    const fs = await openRtty({})
    const sel = selectFor(fs, 'AFSK sideband')!
    expect(sel.value).toBe('lsb')
    expect(Array.from(sel.options).map((o) => [o.value, o.textContent])).toEqual([
      ['lsb', 'LSB (standard)'],
      ['usb', 'USB'],
    ])
    fireEvent.change(sel, { target: { value: 'usb' } })
    await clickSave()
    await waitFor(() =>
      expect(api.get('setSettings')).toHaveBeenCalledWith(expect.objectContaining({ rttyAfskSideband: 'usb' })),
    )
  })

  it('reads a stored USB back', async () => {
    const fs = await openRtty({ rttyAfskSideband: 'usb' })
    expect(selectFor(fs, 'AFSK sideband')!.value).toBe('usb')
  })

  it("is not offered with true FSK, where the rig's own RTTY mode sets the sense", async () => {
    const fs = await openRtty({ rttyBackend: 'fsk' })
    expect(selectFor(fs, 'Keying backend'), 'CONTROL: the RTTY section rendered').not.toBeNull()
    expect(selectFor(fs, 'AFSK sideband')).toBeNull()
  })

  it('no longer tells the operator to use Reverse for USB', async () => {
    const fs = await openRtty({})
    expect(fs.textContent).toContain('to work a station sending upside down')
    expect(fs.textContent).not.toContain('AFSK in USB/DATA-U')
  })
})
