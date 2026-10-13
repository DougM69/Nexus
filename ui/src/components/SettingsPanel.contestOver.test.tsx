// @vitest-environment jsdom
//
// ⭐ THE CONTEST-OVER CHECKLIST, from Settings. A Save that turns Contest mode off offers it FIRST and
// saves nothing until it is answered: the merge and the export need the session the mode holds, so
// the save that ends it waits. "Not now" saves nothing and leaves the form as the operator left it.
// "Turn off" is the ordinary Save, with the ticked settings put back — and those reach the backend
// whatever the form held when it loaded, because Settings sends only what changed since then.
// A Save that does not turn the mode off is untouched by any of it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { FieldDayStatus, Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// The house budget for a Settings render under a loaded full suite (SettingsPanel.stalecopy.test.tsx).
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

let station: Record<string, unknown> = {}

/** The end of a club weekend at the Illinois QSO Party: the club's call on the air, the mode on, and
 *  the engine's note of the station's own call. `fieldDay` is the snapshot's contest block, as App
 *  passes it. */
function openOn(held: Partial<Settings> = {}, fieldDay: FieldDayStatus | null = null) {
  station = structuredClone({
    ...defaultSettings,
    mycall: 'W9CLB',
    mygrid: 'EN52',
    fdActive: true,
    fdEvent: 'ilqp',
    contestQthState: 'IL',
    contestQthCounty: 'COOK',
    contestBefore: {
      mycall: 'K9ABC',
      fdOperator: '',
      fdPositionName: '',
      contestCategoryOperator: '',
      contestEntryClass: '',
      contestCategoryPower: '',
      unassistedMode: false,
    },
    ...held,
  })
  const features = {
    enabled: () => true,
    setEnabled: vi.fn(),
    all: () => [],
    profile: 'full',
    setProfile: vi.fn(),
  } as unknown as FeaturesApi
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
      fieldDay={fieldDay}
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
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('setSettings').mockImplementation((s: Record<string, unknown>) => {
    station = structuredClone(s)
    return Promise.resolve(null)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const writes = () => api.get('setSettings').mock.calls.map((c) => c[0] as Record<string, unknown>)

/** The form has loaded, on the Contesting tab, with the mode switched OFF in the form. */
async function switchOffInTheForm() {
  await screen.findAllByDisplayValue('W9CLB')
  fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
  const group = document.getElementById('settings-contest-pick')!
  fireEvent.click(within(group).getByRole('switch', { name: /Disable Contest mode/ }))
}

/** The footer Save (`type="submit"`). */
const pressSave = () =>
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )

describe('a Save that turns Contest mode off', () => {
  it('offers the checklist first and saves nothing until it is answered', async () => {
    openOn()
    await switchOffInTheForm()
    pressSave()
    expect(await screen.findByRole('dialog', { name: 'Contest over?' })).toBeTruthy()
    expect(writes()).toEqual([])
  })

  it('Not now saves nothing and leaves the form as the operator left it', async () => {
    openOn()
    await switchOffInTheForm()
    pressSave()
    fireEvent.click(await screen.findByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(writes()).toEqual([])
    const group = document.getElementById('settings-contest-pick')!
    expect(within(group).getByRole('switch', { name: /Enable Contest mode/ }).getAttribute('aria-checked')).toBe('false')
  })

  it('Turn off is the Save, with the ticked settings put back', async () => {
    openOn()
    await switchOffInTheForm()
    pressSave()
    fireEvent.click(await screen.findByRole('checkbox', { name: /Callsign on the air/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: false, mycall: 'K9ABC' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('at a club, says how many of this laptop\'s contacts the club\'s log does not have yet', async () => {
    // The snapshot's club chip: the link down, three contacts the host never had.
    const fieldDay = {
      running: false,
      state: 'Idle',
      qsoCount: 3,
      sections: 0,
      points: 6,
      event: 'ilqp',
      log: [],
      club: {
        syncState: 'offline',
        queued: 3,
        offlineSinceUnix: 1,
        hosting: false,
        event: 'ILQP 2026',
        hostCall: 'W9CLB',
        score: 0,
        qsos: 0,
        sections: 0,
        skewSecs: 0,
        dupes: [],
        board: [],
      },
    } as FieldDayStatus
    openOn({ fdJoinAddr: '192.168.1.20:42073' }, fieldDay)
    await switchOffInTheForm()
    pressSave()
    await screen.findByRole('dialog', { name: 'Contest over?' })
    expect(document.querySelector('.contest-over-club')?.textContent).toBe(
      "3 contacts from this laptop are not in the club's log yet. They still go to the host when this laptop is back with it, Contest mode on or off. Leave Nexus open, with club sync on, until then.",
    )
    expect(writes()).toEqual([])
  })

  it('puts back a setting the form never changed, over a value that changed since it loaded', async () => {
    // A seat swap in the pop-out scoreboard after Settings opened: the form still holds the old
    // operator, so "what changed since" alone would never send the put-back.
    openOn({ fdOperator: '' })
    await screen.findAllByDisplayValue('W9CLB')
    station = { ...station, fdOperator: 'W9OP' }
    await switchOffInTheForm()
    pressSave()
    fireEvent.click(await screen.findByRole('checkbox', { name: /Operator at the key/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(writes()[0]).toMatchObject({ fdActive: false, fdOperator: '' })
  })
})

describe('any other Save', () => {
  it('saves at once, with no checklist, when the mode stays on', async () => {
    openOn()
    await screen.findAllByDisplayValue('W9CLB')
    fireEvent.click(screen.getByRole('tab', { name: 'Contesting' }))
    pressSave()
    await waitFor(() => expect(writes()).toHaveLength(1))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
