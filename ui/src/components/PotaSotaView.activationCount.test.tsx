// @vitest-environment jsdom
//
// The activator's strip as the field needs it: today's count per park POTA's way ("7 of 10 · 3
// to go", valid at ten), a two-fer's parks each counted, the P2P count, the grid the contacts carry
// (never Settings'), and a P2P badge on every POTA activator on the board while you activate.
// The station's counts are taken as given (they are the hot index's, tested there); the api is a
// mock and nothing here reaches a backend.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { Activation, AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  setActivationGrid: vi.fn(),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  selfSpot: vi.fn(), clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  setActivationState: vi.fn(), clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(),
  importHuntedParksCsv: vi.fn(),
}))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: vi.fn() }))

import { PotaSotaView } from './PotaSotaView'

const activation = (over: Partial<Activation> = {}): Activation => ({
  program: 'POTA',
  reference: 'US-1234',
  qsoCount: 7,
  states: [],
  myState: null,
  parks: [{ reference: 'US-1234', count: 7, p2p: 0 }],
  grid: 'EN52',
  utcDay: 20_694,
  ...over,
})
const snapWith = (a: Activation | null) =>
  ({ hunt: null, radio: { dialMhz: 14.285 }, activation: a }) as unknown as AppSnapshot

const spot = (activator: string, reference: string, program = 'POTA'): OtaSpot => ({
  program,
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
})

beforeEach(() => {
  api.getOtaSpots.mockReset()
  api.getOtaSpots.mockResolvedValue([])
  api.setActivationGrid.mockReset()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

it('counts today per park: 7 of 10 with 3 to go, and valid at ten', () => {
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  const chip = screen.getByText((_, el) => el?.classList.contains('pota-act-progress') ?? false)
  expect(chip.textContent).toBe(
    `${t('ota.activation.progress', { reference: 'US-1234', count: 7 })} · ${t('ota.activation.toGo', { count: 3 })}`,
  )
  expect(chip.classList.contains('valid')).toBe(false)
  rerender(<PotaSotaView snap={snapWith(activation({ parks: [{ reference: 'US-1234', count: 10, p2p: 0 }] }))} />)
  const valid = document.querySelector('.pota-act-progress')!
  expect(valid.textContent).toBe(
    `${t('ota.activation.progress', { reference: 'US-1234', count: 10 })} · ${t('ota.activation.valid')}`,
  )
  expect(valid.classList.contains('valid')).toBe(true)
})

it('shows each park of a two-fer and the P2P count', () => {
  render(
    <PotaSotaView
      snap={snapWith(
        activation({
          reference: 'US-1234,US-5678',
          parks: [
            { reference: 'US-1234', count: 4, p2p: 2 },
            { reference: 'US-5678', count: 4, p2p: 2 },
          ],
        }),
      )}
    />,
  )
  const chips = [...document.querySelectorAll('.pota-act-progress:not(.p2p)')].map((c) => c.textContent)
  expect(chips).toEqual([
    `${t('ota.activation.progress', { reference: 'US-1234', count: 4 })} · ${t('ota.activation.toGo', { count: 6 })}`,
    `${t('ota.activation.progress', { reference: 'US-5678', count: 4 })} · ${t('ota.activation.toGo', { count: 6 })}`,
  ])
  expect(screen.getByText(t('ota.activation.p2p', { count: 2 }))).toBeTruthy()
})

it('the snapshot is live: the count follows it, and no activation shows the Start form', () => {
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  rerender(<PotaSotaView snap={snapWith(activation({ parks: [{ reference: 'US-1234', count: 8, p2p: 0 }] }))} />)
  expect(document.querySelector('.pota-act-progress')!.textContent).toContain(
    t('ota.activation.progress', { reference: 'US-1234', count: 8 }),
  )
  rerender(<PotaSotaView snap={snapWith(null)} />)
  expect(document.querySelector('.pota-act-progress')).toBeNull()
  expect(screen.getByRole('button', { name: t('ota.activation.start') })).toBeTruthy()
})

it('the grid box shows the activation’s grid and sends a typed one on Enter, only when changed', async () => {
  api.setActivationGrid.mockResolvedValue(activation({ grid: 'EN52WA' }))
  render(<PotaSotaView snap={snapWith(activation())} />)
  const box = screen.getByDisplayValue('EN52') as HTMLInputElement
  fireEvent.keyDown(box, { key: 'Enter' })
  expect(api.setActivationGrid).not.toHaveBeenCalled()
  fireEvent.change(box, { target: { value: 'en52wa' } })
  fireEvent.keyDown(box, { key: 'Enter' })
  await waitFor(() => expect(api.setActivationGrid).toHaveBeenCalledWith('EN52WA'))
})

it('badges every POTA activator P2P while you activate, and none when you do not', async () => {
  api.getOtaSpots.mockImplementation(async (program: string) =>
    program === 'POTA' ? [spot('K1ABC', 'US-0001')] : [spot('W7XYZ', 'W7A/MN-001', 'SOTA')],
  )
  const { rerender } = render(<PotaSotaView snap={snapWith(activation())} />)
  await screen.findByText('K1ABC')
  expect(document.querySelectorAll('.pota-badge-p2p')).toHaveLength(1)
  rerender(<PotaSotaView snap={snapWith(null)} />)
  expect(document.querySelectorAll('.pota-badge-p2p')).toHaveLength(0)
})
