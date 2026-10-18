// @vitest-environment jsdom
//
// Settings ▸ Confirmations ▸ ClubLog API key. The station's screen may be streamed, and screens get
// shared: the key shows as •••• until Reveal, at the station's own screen only. Through a stream it
// can be neither revealed nor changed. Read from a computer driving the station it is not sent at
// all: the field says whether the station has one saved, and takes nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { pushToast } from '../toast'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 15_000 })

const KEY = 'FAKE-OVERRIDE-KEY-0001'

/** Whether the event being handled came through a stream, as `isStreamInput` answers it. */
const stream = vi.hoisted(() => ({ on: false }))
vi.mock('../remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (see SettingsPanel.lotwupload.test.tsx
// for why a hand-kept list is the trap).
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

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  stream.on = false
})


async function open(settings: Record<string, unknown>) {
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', ...settings } as never),
  )
  render(
    <SettingsPanel
      target="confirmations"
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
  const label = await screen.findByText(t('settings.confirmations.clublog.apiKey.label'))
  const field = label.closest('label') as HTMLLabelElement
  const input = field.querySelector('input') as HTMLInputElement
  const reveal = () =>
    field.querySelector('button') as HTMLButtonElement | null
  return { input, reveal }
}

const refused = () => vi.mocked(pushToast).mock.calls.filter((c) => c[0] === t('remote.lan.atShackOnly')).length

describe('the ClubLog key at the station', () => {
  it('shows as •••• until Reveal, and Hide masks it again', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    expect([input.type, input.value]).toEqual(['password', KEY])
    expect(reveal()?.textContent).toBe(t('settings.confirmations.clublog.apiKey.reveal'))
    fireEvent.click(reveal()!)
    expect(input.type).toBe('text')
    expect(reveal()?.textContent).toBe(t('settings.confirmations.clublog.apiKey.hide'))
    fireEvent.click(reveal()!)
    expect(input.type).toBe('password')
  })

  it('is neither revealed nor changed through a stream, and is changed at the screen', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    stream.on = true
    fireEvent.click(reveal()!)
    expect(input.type).toBe('password')
    fireEvent.change(input, { target: { value: 'FAKE-STREAMED-KEY-0002' } })
    expect(input.value).toBe(KEY)
    expect(refused()).toBe(2)
    stream.on = false
    fireEvent.change(input, { target: { value: 'FAKE-TYPED-KEY-0003' } })
    expect(input.value).toBe('FAKE-TYPED-KEY-0003')
    expect(refused()).toBe(2)
  })

  it('masks again at the first press or key that comes through a stream', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    fireEvent.click(reveal()!)
    fireEvent.pointerDown(document.body)
    expect(input.type, 'CONTROL: a press at the screen').toBe('text')
    stream.on = true
    fireEvent.pointerDown(document.body)
    expect(input.type).toBe('password')
    stream.on = false
    fireEvent.click(reveal()!)
    stream.on = true
    fireEvent.keyDown(document.body, { key: 'a' })
    expect(input.type).toBe('password')
  })
})

describe('the ClubLog key read from afar', () => {
  it('says the station has one saved, and takes nothing', async () => {
    const { input, reveal } = await open({ clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: true } })
    expect(input.disabled).toBe(true)
    expect(input.value).toBe('')
    expect(input.placeholder).toBe(t('settings.confirmations.clublog.apiKey.savedAtStation'))
    expect(reveal()).toBeNull()
  })

  it('says the station has none saved', async () => {
    const { input } = await open({ clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: false } })
    expect(input.disabled).toBe(true)
    expect(input.placeholder).toBe(t('settings.confirmations.clublog.apiKey.notSavedAtStation'))
  })
})
