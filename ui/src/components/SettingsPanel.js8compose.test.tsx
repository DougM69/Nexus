// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 ▸ Composing — the compose box's settings as JS8Call 3.0.3 keeps them, at
// its defaults (`CQMessage` CQ CQ CQ <MYGRID4>, `Reply` HW CPY?, `Macros` TNX 73 GL,
// `TransmitDirected`, `PillsEnabled` and `Spellcheck` on), each saved by the panel's Save, and the
// Save refused for a CQ message JS8Call's Settings would refuse (Configuration.cpp:3003-3008).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { Settings } from '../types'
import { t } from '../i18n'
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

/** A settings file from before these keys: the panel must show JS8Call's defaults. */
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, js8Groups: [], mycall: 'KD9TAW', mygrid: 'EN52', ...stored } as never),
  )
})
afterEach(cleanup)

async function openComposing(): Promise<HTMLElement> {
  render(
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText(t('settings.js8.cqText.label'))
  const group = document.querySelector('#settings-js8 .js8-compose-settings') as HTMLElement
  expect(group, 'the Composing group sits in the JS8 fieldset').not.toBeNull()
  return group
}

/** The control in the field a `.settings-label` names. */
function control(group: HTMLElement, label: string): HTMLElement {
  const span = within(group)
    .getAllByText(label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  expect(span, `no settings-label "${label}"`).toBeDefined()
  const el = span!.closest('label, .settings-field')!.querySelector('button[role="switch"], input')
  expect(el, `no control for "${label}"`).not.toBeNull()
  return el as HTMLElement
}

async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find((b) => (b as HTMLButtonElement).type === 'submit')!
  fireEvent.click(save)
}
const saved = () => api.get('setSettings').mock.lastCall?.[0] as Settings | undefined
const rows = (group: HTMLElement) => [...group.querySelectorAll<HTMLInputElement>('.js8-saved-row input')].map((i) => i.value)

describe('Settings ▸ Digital ▸ JS8 ▸ Composing', () => {
  it("shows JS8Call 3.0.3's defaults to a settings file from before these keys", async () => {
    const group = await openComposing()
    expect((control(group, t('settings.js8.cqText.label')) as HTMLInputElement).value).toBe('CQ CQ CQ <MYGRID4>')
    expect((control(group, t('settings.js8.replyText.label')) as HTMLInputElement).value).toBe('HW CPY?')
    expect(rows(group)).toEqual(['TNX 73 GL'])
    for (const label of ['settings.js8.sendAtOnce.label', 'settings.js8.pills.label', 'settings.js8.wordSuggestions.label'] as const) {
      expect(control(group, t(label)).getAttribute('aria-checked'), label).toBe('true')
    }
    // The CQ field offers JS8Call's eight one-frame CQ forms, each with the square.
    expect([...document.querySelectorAll('#js8-cq-forms option')].map((o) => o.getAttribute('value'))).toEqual([
      'CQ CQ CQ <MYGRID4>', 'CQ DX <MYGRID4>', 'CQ QRP <MYGRID4>', 'CQ CONTEST <MYGRID4>',
      'CQ FIELD <MYGRID4>', 'CQ FD <MYGRID4>', 'CQ CQ <MYGRID4>', 'CQ <MYGRID4>',
    ])
    // Every macro is named under the saved messages.
    expect(group.querySelector('.js8-macro-list code')?.textContent).toBe(
      '<CALL> <MYCALL> <MYCQ> <MYGRID12> <MYGRID4> <MYHB> <MYIDLE> <MYINFO> <MYREPLY> <MYSTATUS> <MYVERSION> <SNR> <TDELTA>',
    )
  })

  it('saves the CQ and reply messages upper-cased, and the three switches', async () => {
    const group = await openComposing()
    fireEvent.change(control(group, t('settings.js8.cqText.label')), { target: { value: 'cq dx <MYGRID4>' } })
    fireEvent.change(control(group, t('settings.js8.replyText.label')), { target: { value: 'tnx fer call' } })
    for (const label of ['settings.js8.sendAtOnce.label', 'settings.js8.pills.label', 'settings.js8.wordSuggestions.label'] as const) {
      fireEvent.click(control(group, t(label)))
    }
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()).toMatchObject({
      js8CqText: 'CQ DX <MYGRID4>',
      js8ReplyText: 'TNX FER CALL',
      js8SendAtOnce: false,
      js8Pills: false,
      js8WordSuggestions: false,
    })
  })

  it('adds, edits, orders and deletes saved messages, and saves the list in its order', async () => {
    const group = await openComposing()
    const add = group.querySelector<HTMLInputElement>('.js8-saved-add input')!
    fireEvent.change(add, { target: { value: 'qrv <mygrid4>' } })
    fireEvent.keyDown(add, { key: 'Enter' })
    fireEvent.change(add, { target: { value: 'gm' } })
    fireEvent.click(within(group.querySelector('.js8-saved-add') as HTMLElement).getByRole('button', { name: t('settings.js8.saved.add') }))
    expect(rows(group)).toEqual(['TNX 73 GL', 'QRV <MYGRID4>', 'GM'])
    fireEvent.click(within(group).getByRole('button', { name: t('settings.js8.saved.up.aria', { n: 3 }) }))
    expect(rows(group)).toEqual(['TNX 73 GL', 'GM', 'QRV <MYGRID4>'])
    fireEvent.click(within(group).getByRole('button', { name: t('settings.js8.saved.remove.aria', { n: 1 }) }))
    fireEvent.change(within(group).getByRole('textbox', { name: t('settings.js8.saved.row.aria', { n: 1 }) }), { target: { value: 'gm es tnx' } })
    expect(rows(group)).toEqual(['GM ES TNX', 'QRV <MYGRID4>'])
    expect(within(group).getByRole('button', { name: t('settings.js8.saved.down.aria', { n: 2 }) })).toHaveProperty('disabled', true)
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()?.js8SavedMessages).toEqual(['GM ES TNX', 'QRV <MYGRID4>'])
  })

  it('an emptied list saves empty, not the default', async () => {
    const group = await openComposing()
    fireEvent.click(within(group).getByRole('button', { name: t('settings.js8.saved.remove.aria', { n: 1 }) }))
    expect(within(group).getByText(t('settings.js8.saved.empty'))).toBeTruthy()
    await clickSave()
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(saved()?.js8SavedMessages).toEqual([])
  })

  it('refuses a CQ message that neither starts with CQ nor holds the callsign, as JS8Call does, and says why', async () => {
    const group = await openComposing()
    fireEvent.change(control(group, t('settings.js8.cqText.label')), { target: { value: 'QRV <MYGRID4>' } })
    await clickSave()
    await waitFor(() => expect(screen.queryByRole('alert')?.textContent ?? '').toContain(t('settings.save.js8CqText')))
    expect(api.get('setSettings'), 'nothing saved').not.toHaveBeenCalled()
    fireEvent.change(control(group, t('settings.js8.cqText.label')), { target: { value: 'QRV DE KD9TAW' } })
    await clickSave()
    await waitFor(() => expect(api.get('setSettings'), 'it holds the callsign: saved').toHaveBeenCalled())
    expect(saved()?.js8CqText).toBe('QRV DE KD9TAW')
  })

  it('keeps what a settings file holds', async () => {
    stored = { js8CqText: '', js8SavedMessages: [], js8SendAtOnce: false, js8Pills: true, js8WordSuggestions: false }
    const group = await openComposing()
    expect((control(group, t('settings.js8.cqText.label')) as HTMLInputElement).value).toBe('')
    expect(rows(group)).toEqual([])
    expect(control(group, t('settings.js8.sendAtOnce.label')).getAttribute('aria-checked')).toBe('false')
    expect(control(group, t('settings.js8.wordSuggestions.label')).getAttribute('aria-checked')).toBe('false')
  })
})
