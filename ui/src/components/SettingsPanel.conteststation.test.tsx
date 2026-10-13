// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING ▸ YOUR STATION DATA — the block a SENT exchange comes from (§3.4).
//
// Before it, `mycall`/`mygrid` plus `fdClass`/`fdSection` were the whole of "where I am",
// so a QSO party's in-state role — which sends the operator's COUNTY — had nothing to
// send, and Sweepstakes' check had no source anywhere in the build. The rules loader now
// refuses a ruleset naming a sent slot whose source is not one of these fields, which
// makes the gap a red gate rather than an operator who cannot fill their own exchange.
//
// ⚠️ **A settings key lives in more than one place** and `tsc` covers exactly one of
// them: the Rust struct, the TS interface, the panel and the default fixture are four
// separate sites. What this file proves is the half tsc cannot — that a value typed into
// the panel reaches the `set_settings` payload, and comes back when the form reloads.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "⭐ carries every value into the save payload and back…", takes
// 0.28 s and 0.28 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Mock every export of `../api`, derived from the real module — a hand-kept list made the
// panel throw on mount when it fell behind, which reads as a behaviour regression.
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
      fieldDay={null as never}
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

/** The settings the panel is told are on disk. */
let stored: Record<string, unknown> = {}

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
  stored = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve({ ...stored } as never))
  // A save persists, exactly as the backend does — which is what lets the SECOND mount
  // below be a real round trip rather than a re-read of the same fixture.
  api.get('setSettings').mockImplementation((s: unknown) => {
    stored = { ...(s as Record<string, unknown>) }
    return Promise.resolve({} as never)
  })
})
afterEach(cleanup)

const openContesting = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Contesting' }))

/** The input a labelled field owns. */
const inputFor = async (label: string) =>
  (await screen.findByText(label, { selector: '.settings-label' }))
    .closest('.settings-field')!
    .querySelector('input') as HTMLInputElement

const save = async () => fireEvent.click(await screen.findByRole('button', { name: /^Save$/i }))

const lastSaved = () => {
  const calls = api.get('setSettings').mock.calls
  return calls.length ? (calls[calls.length - 1][0] as Record<string, unknown>) : null
}

/** Every field of §3.4's block the tab shows for every contest, with a value and the label it is
 *  entered under. The sixth, Power sent, shows only for a contest whose exchange sends the
 *  station's power — none of the shipped ones (`SettingsPanel.contestgroups.test.tsx`, per
 *  contest), so it is not typed here; a stored value survives a Save (the same file's round trip). */
const BLOCK = [
  { key: 'contestQthCounty', label: 'County', typed: 'FRAN', want: 'FRAN' },
  { key: 'contestQthState', label: 'State or province', typed: 'WI', want: 'WI' },
  { key: 'contestCheck', label: 'Check', typed: '74', want: '74' },
  { key: 'contestCqZone', label: 'CQ zone', typed: '4', want: 4 },
  { key: 'contestItuZone', label: 'ITU zone', typed: '8', want: 8 },
] as const

describe('the station-data block', () => {
  it('renders under the contest picker and above Your entry', async () => {
    renderPanel()
    await openContesting()
    await inputFor('County')
    const legends = [...document.querySelectorAll('fieldset.settings-section legend')].map(
      (n) => n.textContent,
    )
    expect(legends).toContain('Your station data')
    expect(legends.indexOf('Your station data')).toBeGreaterThan(legends.indexOf('Contest'))
    expect(legends.indexOf('Your station data')).toBeLessThan(
      legends.indexOf('Your entry'),
    )
  })

  it('offers every field §3.4 names, and every one of them is empty by default', async () => {
    renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      const el = await inputFor(f.label)
      expect(el, `${f.label} must be on the tab`).toBeTruthy()
      // Nothing here can be guessed, and a guessed exchange goes on the air.
      expect(el.value, `${f.label} starts empty`).toBe('')
    }
  })

  it('⭐ carries every value into the save payload and back out of a reload', async () => {
    const view = renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      fireEvent.change(await inputFor(f.label), { target: { value: f.typed } })
    }
    await save()
    await waitFor(() => expect(lastSaved()).not.toBeNull())

    const sent = lastSaved()!
    for (const f of BLOCK) {
      expect(sent[f.key], `${f.key} in the save payload`).toBe(f.want)
    }
    // ⚠️ The frozen names ride along untouched — §8(c). The `contest_*` block was added
    // BESIDE them; a save that dropped one would silently reset a shipped Field Day
    // setup, which is the exact hazard renaming a persisted serde field carries.
    expect(sent.fdClass).toBe(defaultSettings.fdClass)
    expect(sent.fdSection).toBe(defaultSettings.fdSection)
    expect(sent.fdEvent).toBe(defaultSettings.fdEvent)

    // THE ROUND TRIP. Mount again against what the save persisted: a key that reached
    // the payload but not the form on reload is still a key that does not work.
    view.unmount()
    renderPanel()
    await openContesting()
    for (const f of BLOCK) {
      expect((await inputFor(f.label)).value, `${f.key} after a reload`).toBe(f.typed)
    }
  })

  it('clamps a zone instead of sending a number that is not a zone', async () => {
    renderPanel()
    await openContesting()
    // `<input type="number" max>` is advisory in every browser — typing past it is
    // allowed, and the value would go on the air inside an exchange.
    fireEvent.change(await inputFor('CQ zone'), { target: { value: '99' } })
    fireEvent.change(await inputFor('ITU zone'), { target: { value: '400' } })
    await save()
    await waitFor(() => expect(lastSaved()).not.toBeNull())
    expect(lastSaved()!.contestCqZone).toBe(40)
    expect(lastSaved()!.contestItuZone).toBe(90)
    // POSITIVE CONTROL: a legal zone is not clamped, or the rule is just "always 40".
    fireEvent.change(await inputFor('CQ zone'), { target: { value: '5' } })
    await save()
    await waitFor(() => expect(lastSaved()!.contestCqZone).toBe(5))
  })
})

// The address a contest log's Cabrillo EMAIL line carries. Optional (an empty one writes no
// line), read by the engine at export, and a plain text setting like the rest of this tab — so
// what this proves is the same round trip: typed, saved, and back after a reload.
describe('the contest log email', () => {
  it('is in Your entry, empty by default, and survives a save and a reload', async () => {
    const view = renderPanel()
    await openContesting()
    const email = await inputFor('Email for contest logs')
    expect(email.closest('fieldset')!.querySelector('legend')!.textContent).toBe('Your entry')
    expect(email.value).toBe('')
    fireEvent.change(email, { target: { value: 'op@example.com' } })
    await save()
    await waitFor(() => expect(lastSaved()?.contestEmail).toBe('op@example.com'))
    view.unmount()
    renderPanel()
    await openContesting()
    expect((await inputFor('Email for contest logs')).value).toBe('op@example.com')
  })
})

// ⭐ THE SPONSOR'S OWN ENTRY LINES. The Illinois QSO Party's processing software reads
// ENTRY-CLASS, CLUB and OPERATORS out of the Cabrillo header. The classes come from the
// contest's rules data through the ruleset preview, never from a list in the panel, so the
// picker exists only for a contest that declares them; Club and Other operators are text
// like the email above, read by the engine at export.
describe('the entry class, club and other operators', () => {
  const ILQP_CLASSES = [
    'ILLINOIS FIXED HIGH POWER',
    'ILLINOIS FIXED LOW POWER',
    'ILLINOIS PORTABLE',
    'ILLINOIS MOBILE',
    'ILLINOIS ROVER',
    'OUTSIDE ILLINOIS HIGH POWER',
    'OUTSIDE ILLINOIS LOW POWER',
    'UNLIMITED',
  ]
  const selectFor = async (label: string) =>
    (await screen.findByText(label, { selector: '.settings-label' }))
      .closest('.settings-field')!
      .querySelector('select') as HTMLSelectElement

  it('offers the contest\'s own classes and saves the pick, the club and the operators', async () => {
    stored = { ...stored, fdEvent: 'ilqp' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'ilqp', entryClasses: ILQP_CLASSES } as never),
    )
    renderPanel()
    await openContesting()
    const picker = await selectFor('Entry class')
    const offered = Array.from(picker.options).map((o) => o.value)
    expect(offered).toEqual(['', ...ILQP_CLASSES])
    fireEvent.change(picker, { target: { value: 'UNLIMITED' } })
    fireEvent.change(await inputFor('Club'), {
      target: { value: 'Western Ill Amateur Radio Club' },
    })
    fireEvent.change(await inputFor('Other operators'), { target: { value: 'kb9qrs, w9xyz' } })
    await save()
    await waitFor(() => expect(lastSaved()?.contestEntryClass).toBe('UNLIMITED'))
    expect(lastSaved()!.contestClub).toBe('Western Ill Amateur Radio Club')
    expect(lastSaved()!.contestOperators).toBe('KB9QRS, W9XYZ')
  })

  it('shows a saved class that is not one of this contest\'s as not set', async () => {
    stored = { ...stored, fdEvent: 'ilqp', contestEntryClass: 'MULTI-OP' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'ilqp', entryClasses: ILQP_CLASSES } as never),
    )
    renderPanel()
    await openContesting()
    expect((await selectFor('Entry class')).value).toBe('')
  })

  it('POSITIVE CONTROL: a contest that declares no classes offers no class picker', async () => {
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd' } as never))
    renderPanel()
    await openContesting()
    // The section is there (its Club box is) and the preview has answered, and the class
    // picker is not.
    expect(await inputFor('Club')).toBeTruthy()
    await waitFor(() => expect(api.get('getFdRuleset')).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.queryByText('Entry class', { selector: '.settings-label' })).toBeNull()
  })
})

// ⭐ THE REST OF WHAT A SPONSOR'S TEMPLATE DECLARES: a multi-operator entry's class
// (CATEGORY-TRANSMITTER), an overlay, the mailing address and the soapbox. Each comes from the
// contest's rules data through the ruleset preview and is offered only where the sponsor writes
// it; the class only for a MULTI-OP entry, the only one it is written for.
describe('the multi-operator class, overlay, mailing address and soapbox', () => {
  const WPX = {
    event: 'cqwpx_cw',
    transmitterCategories: ['ONE', 'TWO', 'UNLIMITED'],
    overlayCategories: ['CLASSIC', 'ROOKIE', 'TB-WIRES', 'YOUTH'],
    addressHeaders: ['ADDRESS', 'ADDRESS-CITY', 'ADDRESS-STATE-PROVINCE', 'ADDRESS-POSTALCODE', 'ADDRESS-COUNTRY'],
    soapbox: true,
  }
  const chips = async (group: string) =>
    Array.from((await screen.findByRole('group', { name: group })).querySelectorAll('button'))
  const textareaFor = async (label: string) =>
    (await screen.findByText(label, { selector: '.settings-label' }))
      .closest('.settings-field')!
      .querySelector('textarea') as HTMLTextAreaElement
  const CLASS = 'Which multi-operator class you are entering'
  const OVERLAY = 'Which overlay category you are also entering'

  it('offers the contest\'s own classes and overlays, and saves every declaration', async () => {
    stored = { ...stored, fdEvent: 'cqwpx_cw', contestCategoryOperator: 'MULTI-OP' }
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve(WPX as never))
    renderPanel()
    await openContesting()
    const classes = await chips(CLASS)
    expect(classes.map((b) => b.textContent)).toEqual(['Not set', 'ONE', 'TWO', 'UNLIMITED'])
    expect(classes[0].getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(classes[2])
    const overlays = await chips(OVERLAY)
    expect(overlays.map((b) => b.textContent)).toEqual(['Not set', 'CLASSIC', 'ROOKIE', 'TB-WIRES', 'YOUTH'])
    fireEvent.click(overlays[3])
    fireEvent.change(await textareaFor('Mailing address'), { target: { value: '1 Example Rd\nApt 2' } })
    fireEvent.change(await inputFor('Mailing city'), { target: { value: 'Springfield' } })
    fireEvent.change(await inputFor('Mailing state or province'), { target: { value: 'IL' } })
    fireEvent.change(await inputFor('Postal code'), { target: { value: '62701' } })
    fireEvent.change(await inputFor('Mailing country'), { target: { value: 'USA' } })
    fireEvent.change(await textareaFor('Soapbox'), { target: { value: 'A fine weekend.\nFirst licensed 2024.' } })
    await save()
    await waitFor(() => expect(lastSaved()?.contestCategoryTransmitter).toBe('TWO'))
    const saved = lastSaved()!
    expect([
      saved.contestCategoryOverlay,
      saved.contestAddress,
      saved.contestAddressCity,
      saved.contestAddressState,
      saved.contestAddressPostalcode,
      saved.contestAddressCountry,
      saved.contestSoapbox,
    ]).toEqual(['TB-WIRES', '1 Example Rd\nApt 2', 'Springfield', 'IL', '62701', 'USA', 'A fine weekend.\nFirst licensed 2024.'])
  })

  it('offers no class to a single operator, and shows a class this contest lacks as not set', async () => {
    stored = { ...stored, fdEvent: 'cqwpx_cw', contestCategoryOperator: 'SINGLE-OP' }
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve(WPX as never))
    const view = renderPanel()
    await openContesting()
    expect(await chips(OVERLAY)).toHaveLength(5)
    expect(screen.queryByRole('group', { name: CLASS })).toBeNull()
    view.unmount()
    // CONTROL: the same contest, a multi-operator entry holding a class it does not have
    // (ARRL VHF's LIMITED): the picker is there, and that class reads as not set.
    stored = { ...stored, contestCategoryOperator: 'MULTI-OP', contestCategoryTransmitter: 'LIMITED' }
    renderPanel()
    await openContesting()
    const classes = await chips(CLASS)
    expect(classes.filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.textContent)).toEqual([
      'Not set',
    ])
  })

  it('POSITIVE CONTROL: a contest whose sponsor names none of them offers none of them', async () => {
    stored = { ...stored, contestCategoryOperator: 'MULTI-OP' }
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd' } as never))
    renderPanel()
    await openContesting()
    expect(await inputFor('Club')).toBeTruthy()
    await waitFor(() => expect(api.get('getFdRuleset')).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.queryByRole('group', { name: CLASS })).toBeNull()
    expect(screen.queryByRole('group', { name: OVERLAY })).toBeNull()
    expect(screen.queryByText('Mailing address', { selector: '.settings-label' })).toBeNull()
    expect(screen.queryByText('Soapbox', { selector: '.settings-label' })).toBeNull()
  })
})

// ⭐ THE BAND CATEGORY (CATEGORY-BAND): ALL, or one band for a single-band entry, from the contest's
// own list through the ruleset preview, offered only where the sponsor lists bands. A declaration:
// nothing reads it off the log, and a band this contest does not list shows as not set.
describe('the band category', () => {
  const BAND = 'Which band category you are entering'
  const CQWW = ['ALL', '160M', '80M', '40M', '20M', '15M', '10M']
  const chips = async () =>
    Array.from((await screen.findByRole('group', { name: BAND })).querySelectorAll('button'))
  const pressed = (buttons: HTMLButtonElement[]) =>
    buttons.filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.textContent)

  it('offers the contest\'s own bands, not set by default, and saves the pick', async () => {
    stored = { ...stored, fdEvent: 'cqww_ssb' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'cqww_ssb', bandCategories: CQWW } as never),
    )
    renderPanel()
    await openContesting()
    const bands = await chips()
    expect(bands.map((b) => b.textContent)).toEqual(['Not set', ...CQWW])
    expect(pressed(bands)).toEqual(['Not set'])
    fireEvent.click(bands[5])
    await save()
    await waitFor(() => expect(lastSaved()?.contestCategoryBand).toBe('20M'))
  })

  it('shows a band this contest does not list as not set', async () => {
    // CQ WW RTTY runs on five bands: a 160M picked for CQ WW SSB claims nothing here.
    stored = { ...stored, fdEvent: 'cqww_rtty', contestCategoryBand: '160M' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'cqww_rtty', bandCategories: ['ALL', '80M', '40M', '20M', '15M', '10M'] } as never),
    )
    const view = renderPanel()
    await openContesting()
    expect(pressed(await chips())).toEqual(['Not set'])
    view.unmount()
    // CONTROL: a band it lists reads as picked.
    stored = { ...stored, contestCategoryBand: '40M' }
    renderPanel()
    await openContesting()
    expect(pressed(await chips())).toEqual(['40M'])
  })

  it('POSITIVE CONTROL: a contest whose sponsor lists no bands offers no band category', async () => {
    api.get('getFdRuleset').mockImplementation(() => Promise.resolve({ event: 'arrlfd' } as never))
    renderPanel()
    await openContesting()
    expect(await inputFor('Club')).toBeTruthy()
    await waitFor(() => expect(api.get('getFdRuleset')).toHaveBeenCalled())
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.queryByRole('group', { name: BAND })).toBeNull()
  })
})
