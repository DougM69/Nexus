// @vitest-environment jsdom
//
// The setup wizard offers the TCI connection where it finds an SDR program on this computer, and
// where the operator picks one: the detect pass looks for a TCI server on 127.0.0.1 (50001 first,
// then 40001), a server it finds is a row to pick, and the "SDR program (TCI)" choice asks for the
// address and the port, with Find TCI server beside them. Picking TCI stores TCI's own model
// number; leaving it takes that number off again, as Settings does.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SetupWizard } from './SetupWizard'
import * as api from '../api'
import type { TciProbeResult, TciServerFound } from '../api'

vi.mock('../api', () => ({
  importAdif: vi.fn(),
  detectRigs: vi.fn(() => Promise.resolve([])),
  discoverFlex: vi.fn(() => Promise.resolve([])),
  tciProbe: vi.fn(),
  getAudioDevices: vi.fn(() => Promise.resolve({ input: [], output: [] })),
  getRigModels: vi.fn(() => Promise.resolve([])),
  getAllRigModels: vi.fn(() => Promise.resolve([])),
  probeCatPorts: vi.fn(() => Promise.resolve({ found: false, detail: 'no rig answered' })),
  addRadio: vi.fn(() => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ radios: [] })),
  updateRadioProfile: vi.fn(() => Promise.resolve(null)),
}))

const SUNSDR: TciServerFound = {
  port: 40001,
  program: 'ExpertSDR3',
  version: '2.0',
  device: 'SunSDR2QRP',
  trxCount: 1,
  dialect: 'expertsdr3',
}

const answer = (found: TciServerFound | null, host = '127.0.0.1'): TciProbeResult => ({
  host,
  resolved: true,
  tried: found
    ? [
        { port: 50001, outcome: 'noAnswer' },
        { port: found.port, outcome: 'found' },
      ]
    : [
        { port: 50001, outcome: 'noAnswer' },
        { port: 40001, outcome: 'noAnswer' },
      ],
  found,
})

function renderWizard(settings: Record<string, unknown> | null = null) {
  const onApply = vi.fn()
  render(
    <SetupWizard
      settings={settings as never}
      onApply={onApply}
      onTestCat={vi.fn(() => Promise.resolve({} as never)) as never}
      onSkip={vi.fn()}
    />,
  )
  return { onApply }
}

const clickNext = () => fireEvent.click(screen.getByRole('button', { name: /Next/ }))

/** Walk the remaining steps and finish; the draft the wizard applied. */
function finish(onApply: ReturnType<typeof vi.fn>): Record<string, unknown> {
  clickNext() // → log
  clickNext() // → goals
  fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
  return onApply.mock.calls[0][4]
}

const tciChoice = () => screen.getByRole('button', { name: /SDR program \(TCI\)/ })

beforeEach(() => {
  cleanup()
  vi.mocked(api.tciProbe).mockReset()
  vi.mocked(api.tciProbe).mockResolvedValue(answer(null))
})

describe('the setup wizard and the TCI connection', () => {
  it('looks for a TCI server on this computer when the rig step opens, and offers the one it finds', async () => {
    vi.mocked(api.tciProbe).mockResolvedValue(answer(SUNSDR))
    const { onApply } = renderWizard()
    expect(api.tciProbe).not.toHaveBeenCalled()
    clickNext() // → rig
    await waitFor(() => expect(api.tciProbe).toHaveBeenCalledWith('127.0.0.1', 50001))
    const row = await screen.findByRole('button', { name: /ExpertSDR3 2\.0, SunSDR2QRP on this computer \(TCI, port 40001\)/ })
    expect(tciChoice().getAttribute('aria-pressed'), 'nothing is picked for the operator').toBe('false')
    fireEvent.click(row)
    expect(tciChoice().getAttribute('aria-pressed')).toBe('true')
    expect((screen.getByLabelText('TCI port') as HTMLInputElement).value).toBe('40001')
    const draft = finish(onApply)
    expect([draft.rigConn, draft.rigModel, draft.rigModelName, draft.tciPort]).toEqual(['tci', 7, 'TCI', 40001])
    expect(draft.tciHost, 'this computer is the default: unchanged, so not written').toBeUndefined()
  })

  it('finds none: no row, and the empty line says to turn the program’s TCI server on', async () => {
    renderWizard()
    clickNext()
    expect(await screen.findByText(/SDR program: turn its TCI server on/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /on this computer \(TCI/ })).toBeNull()
  })

  it('says so when the search itself fails, as it does for the other scans', async () => {
    vi.mocked(api.tciProbe).mockRejectedValue('no backend')
    renderWizard()
    clickNext()
    expect(await screen.findByText(/TCI server search failed: no backend/)).toBeTruthy()
  })

  it('the SDR program (TCI) choice asks for the address and port, and Find TCI server fills in the port', async () => {
    const { onApply } = renderWizard()
    clickNext()
    await waitFor(() => expect(api.tciProbe).toHaveBeenCalledTimes(1))
    expect(screen.queryByLabelText('SDR program address'), 'control: not asked on USB').toBeNull()
    fireEvent.click(tciChoice())
    fireEvent.change(screen.getByLabelText('SDR program address'), { target: { value: '192.168.1.30' } })
    vi.mocked(api.tciProbe).mockResolvedValue(answer(SUNSDR, '192.168.1.30'))
    fireEvent.click(screen.getByRole('button', { name: 'Find TCI server' }))
    await waitFor(() => expect(api.tciProbe).toHaveBeenLastCalledWith('192.168.1.30', 50001))
    expect(await screen.findByText(/Found a TCI server at 192\.168\.1\.30:40001: ExpertSDR3 2\.0, SunSDR2QRP\./)).toBeTruthy()
    expect((screen.getByLabelText('TCI port') as HTMLInputElement).value).toBe('40001')
    expect(screen.getByText(/Nexus controls the radio over TCI, but does not transmit on this connection yet/)).toBeTruthy()
    const draft = finish(onApply)
    expect([draft.rigConn, draft.rigModel, draft.tciHost, draft.tciPort]).toEqual(['tci', 7, '192.168.1.30', 40001])
  })

  it('a USB radio picked after TCI does not keep TCI’s model number', async () => {
    // A bridge chip with no model: nothing the pick names replaces the number, and Auto-test finds
    // no radio, so only taking it off keeps the serial radio from holding a model rigctld refuses.
    vi.mocked(api.detectRigs).mockResolvedValueOnce([
      {
        portName: 'COM7',
        suggestedModel: null,
        suggestedModelName: null,
        suggestedAudio: null,
        suggestedAudioOut: null,
        product: 'USB Serial',
        chip: 'CH340',
        civSide: null,
        interfaceName: null,
        interfacePttMethod: null,
      },
    ] as never)
    const { onApply } = renderWizard()
    clickNext()
    const bridge = await screen.findByRole('button', { name: /on COM7/ })
    fireEvent.click(tciChoice())
    expect(tciChoice().getAttribute('aria-pressed'), 'precondition: on TCI').toBe('true')
    fireEvent.click(bridge)
    await waitFor(() => expect(api.probeCatPorts).toHaveBeenCalled())
    const draft = finish(onApply)
    // Serial is where a new station starts, so the draft need not name it; the model must be 0.
    expect([draft.rigConn ?? 'serial', draft.serialPort, draft.rigModel]).toEqual(['serial', 'COM7', 0])
  })

  it('leaving TCI takes TCI’s model number off, even the one already saved', async () => {
    const { onApply } = renderWizard({ rigConn: 'tci', rigModel: 7, rigModelName: 'TCI', tciHost: '127.0.0.1', tciPort: 50001 })
    clickNext()
    await waitFor(() => expect(api.tciProbe).toHaveBeenCalled())
    expect(tciChoice().getAttribute('aria-pressed'), 'precondition: the saved connection').toBe('true')
    fireEvent.click(screen.getByRole('button', { name: /USB \/ Serial/ }))
    const draft = finish(onApply)
    expect([draft.rigConn, draft.rigModel]).toEqual(['serial', 0])
  })
})
