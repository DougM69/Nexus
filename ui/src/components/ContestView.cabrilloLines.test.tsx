// @vitest-environment jsdom
//
// THE CONTEST SCREEN AND A CQ CONTEST'S CABRILLO FILE, by value.
//
//  1. What the sponsor requires and the entry has left out is named before the export, in words:
//     each line as the Settings control that fills it and the Cabrillo line it writes, with the way
//     to Settings ▸ Contesting ▸ Your entry. The running session's list when Contest mode is on,
//     the preview's before it starts; and, while a session runs, how to make a declaration the
//     session reads as it starts reach the file.
//  2. The Cabrillo file is the station's own text, written to Downloads unchanged: the screen
//     composes none of it. The Summary's "Sending" line reads the exchange the station renders,
//     with the issued serial, never the composing vector's "0" placeholder.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { ContestView } from './ContestView'
import { exportLog, saveTextToDownloads, type FdRulesetDto } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  getFdRuleset: vi.fn(async () => null),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))

afterEach(cleanup)

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const row = (call: string, serial: string, when: number): FieldDayQso =>
  ({
    call,
    class: '',
    section: '',
    band: '20m',
    mode: 'CW',
    submode: '',
    whenUnix: when,
    sat: '',
    mex: `599 ${serial}`,
    sent: ['599', serial],
    rcvd: ['599', '42'],
    dkey: [call, '20m'],
    dupe: false,
    seq: when,
  }) as unknown as FieldDayQso

/** A running CQ WPX CW session three contacts in: the composing vector keeps the serial's "0"
 *  placeholder, and the rendered exchange carries the next issued serial, 4. */
const wpx = (extra: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    event: 'cqwpx_cw',
    running: false,
    state: 'Listening',
    qsoCount: 3,
    sections: 0,
    points: 3,
    multCount: 1,
    log: [row('K1ABC', '1', 1_790_380_800), row('W4XYZ', '2', 1_790_380_860), row('N0DEF', '3', 1_790_380_920)],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'NR', raw: '0' },
    ],
    composingText: '599 4',
    ...extra,
  }) as unknown as FieldDayStatus

const ssb = (missing: string[] | undefined): FieldDayStatus =>
  ({
    event: 'cqww_ssb',
    running: false,
    state: 'Listening',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    composing: [],
    cabrilloMissing: missing,
  }) as unknown as FieldDayStatus

const SSB = 'CQ World-Wide DX Contest (SSB)'
const NAMED =
  `Before you export, set what ${SSB} requires in its Cabrillo file under Settings ▸ Contesting ▸ Your entry: ` +
  'Spotting assistance (CATEGORY-ASSISTED), Band category (CATEGORY-BAND), Power category (CATEGORY-POWER).'
const RESTART =
  'Power category and Spotting assistance are read when Contest mode turns on, so after setting them turn Contest mode off and on again. Your contacts stay in the log.'

/** The line naming what is missing, as the screen shows it, or null. */
const missingLine = () =>
  [...document.querySelectorAll('.fd-mode-line')].find((p) => p.textContent?.startsWith('Before you export')) ?? null

describe('what the sponsor requires and the entry left out, named before the export', () => {
  it('a running session: each line in words, the restart a session-read declaration needs, and the way to Settings', async () => {
    const onOpenSettings = vi.fn()
    render(
      <ContestView
        fieldDay={ssb(['CATEGORY-ASSISTED', 'CATEGORY-BAND', 'CATEGORY-POWER'])}
        onSetMode={() => {}}
        onOpenSettings={onOpenSettings}
      />,
    )
    await settle()
    const line = missingLine()
    expect(line?.textContent).toBe(`${NAMED} ${RESTART} Open in Settings`)
    fireEvent.click(screen.getByRole('button', { name: 'Open in Settings' }))
    expect(onOpenSettings).toHaveBeenCalledWith('contest-category')
  })

  it('a running session missing only what the export reads (the band, a multi-operator class): no restart', async () => {
    render(<ContestView fieldDay={ssb(['CATEGORY-BAND', 'CATEGORY-TRANSMITTER', 'OPERATORS'])} onSetMode={() => {}} />)
    await settle()
    expect(missingLine()?.textContent).toBe(
      `Before you export, set what ${SSB} requires in its Cabrillo file under Settings ▸ Contesting ▸ Your entry: ` +
        'Band category (CATEGORY-BAND), Multi-operator class (CATEGORY-TRANSMITTER), Other operators (OPERATORS).',
    )
  })

  it('before Contest mode turns on: the preview\'s list, with no restart to make', async () => {
    const preview = {
      event: 'cqww_ssb',
      cabrilloMissing: ['CATEGORY-ASSISTED', 'CATEGORY-BAND', 'CATEGORY-POWER'],
    } as unknown as FdRulesetDto
    render(<ContestView fieldDay={null} fdRuleset={preview} onSetMode={() => {}} />)
    await settle()
    expect(missingLine()?.textContent).toBe(NAMED)
  })

  it('the Illinois QSO Party: a blank entry class or email, each as the Settings field that fills it', async () => {
    const onOpenSettings = vi.fn()
    render(
      <ContestView
        fieldDay={{ ...ssb(['ENTRY-CLASS', 'EMAIL']), event: 'ilqp' } as FieldDayStatus}
        onSetMode={() => {}}
        onOpenSettings={onOpenSettings}
      />,
    )
    await settle()
    expect(missingLine()?.textContent).toBe(
      'Before you export, set what Illinois QSO Party requires in its Cabrillo file under Settings ▸ Contesting ▸ Your entry: ' +
        'Entry class (ENTRY-CLASS), Email for contest logs (EMAIL). Open in Settings',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Open in Settings' }))
    expect(onOpenSettings).toHaveBeenCalledWith('contest-category')
  })

  it('nothing missing, nothing said', async () => {
    render(<ContestView fieldDay={ssb(undefined)} onSetMode={() => {}} />)
    await settle()
    expect(missingLine()).toBeNull()
    cleanup()
    render(<ContestView fieldDay={ssb([])} onSetMode={() => {}} />)
    await settle()
    expect(missingLine()).toBeNull()
    // CONTROL: the same screen does say it when a line is missing.
    cleanup()
    render(<ContestView fieldDay={ssb(['CATEGORY-POWER'])} onSetMode={() => {}} />)
    await settle()
    expect(missingLine()).not.toBeNull()
  })
})

describe('a CQ WPX export: the serials are the ones the contacts were logged with', () => {
  /** Press one export button and read back the text it wrote to Downloads. */
  const written = async (label: string) => {
    vi.mocked(saveTextToDownloads).mockClear()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: label }))
    })
    await settle()
    return vi.mocked(saveTextToDownloads).mock.calls.map((c) => c[1])
  }

  it('the Cabrillo file is the station\'s own text, written unchanged', async () => {
    const cabrillo =
      'START-OF-LOG: 3.0\nCONTEST: CQ-WPX-CW\nQSO: 14025 CW 2026-09-26 0000 W9XYZ 599 1 K1ABC 599 42 0\n' +
      'QSO: 14025 CW 2026-09-26 0001 W9XYZ 599 2 W4XYZ 599 42 0\nQSO: 14025 CW 2026-09-26 0002 W9XYZ 599 3 N0DEF 599 42 0\nEND-OF-LOG:\n'
    vi.mocked(exportLog).mockImplementation(async () => cabrillo)
    render(<ContestView fieldDay={wpx()} onSetMode={() => {}} />)
    await settle()
    expect(await written('Export Cabrillo')).toEqual([cabrillo])
    expect(exportLog).toHaveBeenCalledWith('cabrillo')
  })

  it('the Summary sends what the station renders, with the issued serial, not the placeholder', async () => {
    render(<ContestView fieldDay={wpx()} onSetMode={() => {}} />)
    await settle()
    const [summary] = await written('Summary')
    const sending = summary.split('\n').find((l: string) => l.startsWith('Sending'))
    expect(sending).toBe('Sending 599 4')
  })
})
