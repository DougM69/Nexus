// @vitest-environment jsdom
//
// ⭐ THE CONTEST SETUP: its steps, the engine's own blocker for answers not yet saved, ONE save through
// the patch seam with Contest mode on, and the check that the engine really entered the contest —
// with Contest mode turned off again, and the reason said, when it did not. Plus the quiet "this
// weekend" rule and Field Day's one power question.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { ContestSetupWizard, contestSoon, enteredContest, fdPowerAnswer, fdPowerFrom, SOON_SECS } from './ContestSetupWizard'
import type { AppSnapshot, Settings } from '../types'
import type { ContestWindow, FdRulesetDto } from '../api'

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

const NOW = Date.UTC(2026, 9, 21, 12) / 1000 // Wednesday 2026-10-21 1200Z
const ILQP: ContestWindow = { id: 'ilqp', startUnix: NOW + 3 * 86_400, endUnix: NOW + 4 * 86_400 }
const WINDOWS: ContestWindow[] = [
  { id: 'arrlfd', startUnix: NOW + 240 * 86_400, endUnix: NOW + 241 * 86_400 },
  ILQP,
  { id: 'cqww_cw', startUnix: NOW + 38 * 86_400, endUnix: NOW + 40 * 86_400 },
]

let station: Partial<Settings> = {}
let snapAfterSave: Partial<AppSnapshot> = {}

/** The engine's preview, as the shack answers it: the Illinois QSO Party refuses a blank county and
 *  sends the county once typed; ARRL Field Day sends class and section. */
function preview(patch: Partial<Settings>): FdRulesetDto {
  const s = { ...station, ...patch }
  const event = s.fdEvent || 'arrlfd'
  if (event === 'ilqp') {
    const county = (s.contestQthCounty ?? '').trim()
    return {
      event,
      rulesYear: 2026,
      bannedModes: ['FT8', 'FT4'],
      spottingAllowed: true,
      clusterAllowed: true,
      enforcement: 'warn',
      role: 'in_state',
      exchange: county ? ['599', county] : [],
      problem: county ? '' : 'Your QTH is empty — it is part of the exchange you transmit.',
      sentSettings: ['contest_qth_county', 'contest_qth_state'],
      cabrilloHeaders: ['CLAIMED-SCORE', 'EMAIL', 'IL-COUNTY', 'NAME', 'CLUB', 'ENTRY-CLASS', 'OPERATORS'],
      entryClasses: ['ILLINOIS FIXED HIGH POWER', 'UNLIMITED'],
    }
  }
  return {
    event,
    rulesYear: 2026,
    bannedModes: [],
    spottingAllowed: true,
    clusterAllowed: true,
    enforcement: 'warn',
    role: '',
    exchange: [s.fdClass ?? '', s.fdSection ?? ''],
    problem: '',
    sentSettings: ['fd_class', 'fd_section'],
  }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: NOW * 1000 })
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  station = {
    mycall: 'K9ABC',
    fdActive: false,
    fdEvent: 'ilqp',
    contestQthState: 'IL',
    contestQthCounty: '',
    fdClass: '',
    fdSection: '',
    fdPowerMult: 2,
  }
  snapAfterSave = { mode: 'fieldDay', fieldDay: { event: 'ilqp', running: false } as AppSnapshot['fieldDay'] }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Partial<Settings>) => {
    station = structuredClone(s)
    return Promise.resolve(snapAfterSave)
  })
  api.get('getSnapshot').mockImplementation(() =>
    Promise.resolve({ fieldDay: null, mode: 'chat', radio: { catOk: true, operatingMode: 'digital' }, link: { tier: 'FT8' } }),
  )
  api.get('contestWindows').mockImplementation(() => Promise.resolve(WINDOWS))
  api.get('previewFdRuleset').mockImplementation((patch: Partial<Settings>) => Promise.resolve(preview(patch)))
  api.get('getFdRuleset').mockImplementation(() => Promise.resolve(preview({})))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const writes = () => api.get('setSettings').mock.calls.map((c) => c[0] as Partial<Settings>)
const next = () => fireEvent.click(screen.getByRole('button', { name: 'Next' }))
/** Next, once the preview of the answers so far has come back (Next waits for it). */
const nextWhenChecked = async () => {
  await waitFor(() => expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false))
  next()
}

async function open(props: Partial<Parameters<typeof ContestSetupWizard>[0]> = {}) {
  const onClose = vi.fn()
  render(<ContestSetupWizard onClose={onClose} {...props} />)
  await screen.findByRole('dialog', { name: 'Set up a contest' })
  // The pick list once the windows have arrived.
  await screen.findByText('Running now or this week')
  return { onClose }
}

describe('the quiet "this weekend" line', () => {
  const at = (secs: number): ContestWindow => ({ id: 'ilqp', startUnix: NOW + secs, endUnix: NOW + secs + 86_400 })
  it('names a contest that starts within seven days, and only then', () => {
    expect(contestSoon([at(SOON_SECS)], NOW)?.id).toBe('ilqp')
    expect(contestSoon([at(SOON_SECS + 1)], NOW)).toBeNull()
    // Running already, or not one the picker offers: no line.
    expect(contestSoon([at(-60)], NOW)).toBeNull()
    expect(contestSoon([{ id: 'iaru', startUnix: NOW + 60, endUnix: NOW + 3600 }], NOW)).toBeNull()
    // The soonest wins.
    expect(contestSoon([{ ...at(5 * 86_400), id: 'nyqp' }, at(2 * 86_400)], NOW)?.id).toBe('ilqp')
  })
})

describe("ARRL Field Day's power: one question sets both stored fields", () => {
  it('maps QRP / LOW / HIGH and the battery answer to the category and the multiplier', () => {
    expect(fdPowerAnswer('QRP', true)).toEqual({ contestCategoryPower: 'QRP', fdPowerMult: 5 })
    expect(fdPowerAnswer('QRP', false)).toEqual({ contestCategoryPower: 'QRP', fdPowerMult: 2 })
    expect(fdPowerAnswer('LOW', true)).toEqual({ contestCategoryPower: 'LOW', fdPowerMult: 2 })
    expect(fdPowerAnswer('HIGH', true)).toEqual({ contestCategoryPower: 'HIGH', fdPowerMult: 1 })
    // Read back from what is stored: the declaration first, else the multiplier's.
    expect(fdPowerFrom({ contestCategoryPower: 'LOW', fdPowerMult: 1 })).toEqual({ power: 'LOW', battery: false })
    expect(fdPowerFrom({ contestCategoryPower: '', fdPowerMult: 5 })).toEqual({ power: 'QRP', battery: true })
    expect(fdPowerFrom({ fdPowerMult: 1 })).toEqual({ power: 'HIGH', battery: false })
  })

  it('on the entry step, QRP with battery saves ×5 and QRP; taking the battery away saves ×2', async () => {
    station = { ...station, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'WI' }
    snapAfterSave = { mode: 'fieldDay', fieldDay: { event: 'arrlfd', running: false } as AppSnapshot['fieldDay'] }
    await open()
    next()
    await screen.findByText('You send: 1D WI')
    next()
    fireEvent.click(screen.getByRole('radio', { name: 'QRP: 5 W or less (×5)' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    expect(screen.queryByText('QRP on mains or generator power scores ×2.')).toBeNull()
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    expect(screen.getByText('QRP on mains or generator power scores ×2.')).toBeTruthy()
    fireEvent.click(screen.getByRole('checkbox', { name: /Battery or other natural power/ }))
    await nextWhenChecked()
    next()
    fireEvent.click(await screen.findByRole('button', { name: 'Start ARRL Field Day' }))
    await screen.findByText('ARRL Field Day is on · S&P')
    expect(writes()[0]).toMatchObject({ fdActive: true, contestCategoryPower: 'QRP', fdPowerMult: 5 })
  })
})

describe('the steps', () => {
  it('pick: this week first, in UTC', async () => {
    await open()
    const soon = screen.getByRole('radiogroup', { name: 'Running now or this week' })
    expect(within(soon).getAllByRole('radio').map((r) => r.textContent)).toEqual([
      'Illinois QSO PartyOct 24 1200Z to Oct 25 1200Z',
    ])
    expect(within(soon).getByRole('radio').getAttribute('aria-checked')).toBe('true')
    // Later ones follow, soonest first.
    const later = within(screen.getByRole('radiogroup', { name: 'Later' })).getAllByRole('radio')
    expect(later.slice(0, 2).map((r) => r.querySelector('.contest-setup-pick-name')!.textContent)).toEqual([
      'CQ World-Wide DX Contest (CW)',
      'ARRL Field Day',
    ])
  })

  it('exchange: only what the contest sends, and the engine\'s blocker for the unsaved answers holds Next', async () => {
    await open()
    next()
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Your QTH is empty — it is part of the exchange you transmit.',
    )
    expect(screen.getByLabelText('County')).toBeTruthy()
    expect(screen.getByLabelText('State or province')).toBeTruthy()
    expect(screen.queryByLabelText('FD Class')).toBeNull()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    // The preview is asked for the DRAFT, and nothing is saved while it is.
    expect(api.get('previewFdRuleset')).toHaveBeenCalledWith({})
    fireEvent.change(screen.getByLabelText('County'), { target: { value: 'cook' } })
    await screen.findByText('You send: 599 COOK')
    expect(api.get('previewFdRuleset')).toHaveBeenLastCalledWith({ contestQthCounty: 'COOK' })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    expect(writes()).toEqual([])
  })

  it('entry: the lines this contest writes — its entry classes, club, operators, email — and no band', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    const cls = screen.getByLabelText('Entry class') as HTMLSelectElement
    expect([...cls.options].map((o) => o.value)).toEqual(['', 'ILLINOIS FIXED HIGH POWER', 'UNLIMITED'])
    expect(screen.getByLabelText('Club')).toBeTruthy()
    expect(screen.getByLabelText('Other operators')).toBeTruthy()
    expect(screen.getByLabelText('Email for contest logs')).toBeTruthy()
    expect(screen.queryByLabelText('Band category')).toBeNull()
    expect(screen.queryByLabelText('Power category')).toBeNull()
  })

  it('radio: a banned mode is a warning with a way to CW or Phone, never a refusal', async () => {
    station = { ...station, contestQthCounty: 'COOK' }
    await open()
    next()
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    const warn = document.querySelector('[data-banned]')!
    expect(warn.textContent).toContain('Illinois QSO Party does not allow FT8: contacts in it are logged but score nothing.')
    expect(within(warn as HTMLElement).getByRole('button', { name: 'Work this contest in CW' })).toBeTruthy()
    expect(within(warn as HTMLElement).getByRole('button', { name: 'Work this contest in Phone' })).toBeTruthy()
    expect(screen.getByText('The radio is connected.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    next()
    expect(screen.getByRole('button', { name: 'Start Illinois QSO Party' })).toHaveProperty('disabled', false)
  })

  it('Cancel saves nothing', async () => {
    const { onClose } = await open()
    fireEvent.click(screen.getByRole('radio', { name: /ARRL Field Day/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onClose).toHaveBeenCalled()
    expect(writes()).toEqual([])
  })
})

describe('Start', () => {
  async function toStart() {
    await open()
    next()
    fireEvent.change(await screen.findByLabelText('County'), { target: { value: 'COOK' } })
    await screen.findByText('You send: 599 COOK')
    next()
    next()
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start Illinois QSO Party' }))
  }

  it('is ONE save through the patch seam, with Contest mode on, and says the contest is on in S&P once the engine entered it', async () => {
    await toStart()
    expect(await screen.findByText('Illinois QSO Party is on · S&P')).toBeTruthy()
    expect(writes()).toHaveLength(1)
    // Over the settings the backend held at save time (the patch seam), with the answers and the mode.
    expect(writes()[0]).toMatchObject({ mycall: 'K9ABC', fdEvent: 'ilqp', contestQthCounty: 'COOK', fdActive: true })
    expect(api.get('getSettings').mock.calls.length).toBeGreaterThanOrEqual(2)
  })

  it('when the engine did not enter, says why in its own words and turns Contest mode off again', async () => {
    snapAfterSave = { mode: 'chat', fieldDay: null }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ ...preview({}), problem: 'Your COUNTY is not one the Illinois QSO Party lists.' }),
    )
    await toStart()
    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'The station did not enter Illinois QSO Party: Your COUNTY is not one the Illinois QSO Party lists. Contest mode is off again.',
    )
    await waitFor(() => expect(writes()).toHaveLength(2))
    expect(writes()[1].fdActive).toBe(false)
    expect(screen.queryByText('Illinois QSO Party is on · S&P')).toBeNull()
    // Back on the exchange, where it can be fixed.
    expect(screen.getByRole('heading', { name: 'What Illinois QSO Party sends' })).toBeTruthy()
  })

  it('entered the WRONG contest is not entered', () => {
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: 'ilqp' } } as AppSnapshot, 'ilqp')).toBe(true)
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: 'nyqp' } } as AppSnapshot, 'ilqp')).toBe(false)
    expect(enteredContest({ mode: 'chat', fieldDay: null } as unknown as AppSnapshot, 'ilqp')).toBe(false)
    expect(enteredContest({ mode: 'fieldDay', fieldDay: { event: '' } } as AppSnapshot, 'arrlfd')).toBe(true)
  })

  it('a contest already running: a different pick asks for a tick before it goes on, and Start never runs set_mode', async () => {
    api.get('getSnapshot').mockImplementation(() =>
      Promise.resolve({ fieldDay: { event: 'nyqp' }, mode: 'fieldDay', radio: { catOk: true }, link: { tier: 'FT8' } }),
    )
    await open()
    await screen.findByText(/New York QSO Party is running now/)
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('checkbox', { name: 'End New York QSO Party when I start Illinois QSO Party' }))
    expect(screen.getByRole('button', { name: 'Next' })).toHaveProperty('disabled', false)
    expect(api.get('setMode')).not.toHaveBeenCalled()
  })
})
