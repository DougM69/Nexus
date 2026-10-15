// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 ▸ Frequencies, as JS8Call 3.0.3's Settings ▸ Frequencies tab: JS8Call's
// table shows until the first edit makes the list the operator's own; Reset goes back to it (asked
// first); Import replaces the list (asked first when it holds anything) and Export saves it; "return
// to the last used frequency" and the band schedule are switches, both off by default, and the
// schedule's rows are a dial and a UTC window. Asserted by value on what the panel is handed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { useState } from 'react'
import { Js8FrequencySettings, type Js8FrequencyPatch } from './Js8FrequencySettings'
import type { Js8Freq, Settings } from '../types'
import { t } from '../i18n'

const STOCK: Js8Freq[] = [
  { mhz: 1.8435, description: '' },
  { mhz: 7.078, description: '' },
]

vi.mock('../api', () => ({
  js8DefaultFrequencies: vi.fn(async () => STOCK),
  saveTextToDownloads: vi.fn(async (name: string) => `/home/op/Downloads/${name}`),
}))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

/** The panel as Settings mounts it: the form is its state, every patch lands on it. */
function Host({ initial, seen }: { initial: Partial<Settings>; seen: Js8FrequencyPatch[] }) {
  const [form, setForm] = useState({ ...initial } as Settings)
  return (
    <Js8FrequencySettings
      form={form}
      onChange={(patch) => {
        seen.push(patch)
        setForm((prev) => ({ ...prev, ...patch }))
      }}
    />
  )
}

/** The last patch the panel was handed. */
const last = (seen: Js8FrequencyPatch[]) => seen[seen.length - 1]

async function mount(initial: Partial<Settings> = {}) {
  const seen: Js8FrequencyPatch[] = []
  render(<Host initial={initial} seen={seen} />)
  await act(async () => {
    await Promise.resolve()
  })
  return seen
}

const mhzBox = (n: number) => screen.getByLabelText(t('settings.js8.freqs.mhz.aria', { n })) as HTMLInputElement
const descBox = (n: number) =>
  screen.getByLabelText(t('settings.js8.freqs.description.aria', { n })) as HTMLInputElement

describe('the JS8 frequency list', () => {
  it("shows JS8Call's table until an edit makes the list the operator's own", async () => {
    const seen = await mount()
    await waitFor(() => expect(mhzBox(1).value).toBe('1.843500'))
    expect(mhzBox(2).value).toBe('7.078000')
    expect(screen.getByText(t('settings.js8.freqs.hint.stock'))).toBeTruthy()
    fireEvent.change(descBox(2), { target: { value: '40 m net' } })
    expect(last(seen)).toEqual({
      js8Frequencies: [
        { mhz: 1.8435, description: '' },
        { mhz: 7.078, description: '40 m net' },
      ],
    })
    expect(screen.getByText(t('settings.js8.freqs.hint.own'))).toBeTruthy()
  })

  it('a dial commits on Enter, a bad one is refused and nothing is stored', async () => {
    const { pushToast } = await import('../toast')
    const seen = await mount({ js8Frequencies: [{ mhz: 7.078, description: '' }] })
    fireEvent.change(mhzBox(1), { target: { value: '7.' } })
    expect(seen, 'nothing while typing').toEqual([])
    fireEvent.change(mhzBox(1), { target: { value: '7.095' } })
    fireEvent.keyDown(mhzBox(1), { key: 'Enter' })
    expect(last(seen)).toEqual({ js8Frequencies: [{ mhz: 7.095, description: '' }] })
    fireEvent.change(mhzBox(1), { target: { value: 'abc' } })
    fireEvent.blur(mhzBox(1))
    expect(seen).toHaveLength(1)
    expect(pushToast).toHaveBeenCalledWith(t('settings.js8.freqs.badMhz', { example: '7.078' }), 'info', 3500)
  })

  it('adds and removes rows', async () => {
    const seen = await mount({ js8Frequencies: [{ mhz: 7.078, description: '' }] })
    fireEvent.change(screen.getByLabelText(t('settings.js8.freqs.add.mhz.aria')), { target: { value: '10.130' } })
    fireEvent.change(screen.getByLabelText(t('settings.js8.freqs.add.description.aria')), { target: { value: 'night' } })
    fireEvent.click(screen.getAllByRole('button', { name: t('settings.js8.freqs.add.action') })[0])
    expect(last(seen)).toEqual({
      js8Frequencies: [
        { mhz: 7.078, description: '' },
        { mhz: 10.13, description: 'night' },
      ],
    })
    fireEvent.click(screen.getByRole('button', { name: t('settings.js8.freqs.remove.aria', { mhz: '7.078000' }) }))
    expect(last(seen)).toEqual({ js8Frequencies: [{ mhz: 10.13, description: 'night' }] })
  })

  it("Reset asks, then goes back to JS8Call's table (null); with no list of one's own it does not ask", async () => {
    const { confirmDialog } = await import('../confirm')
    const seen = await mount({ js8Frequencies: [{ mhz: 7.095, description: 'mine' }] })
    fireEvent.click(screen.getByRole('button', { name: t('settings.js8.freqs.reset.action') }))
    await waitFor(() => expect(last(seen)).toEqual({ js8Frequencies: null }))
    expect(confirmDialog).toHaveBeenCalledTimes(1)
    vi.mocked(confirmDialog).mockResolvedValueOnce(false)
    cleanup()
    const declined = await mount({ js8Frequencies: [{ mhz: 7.095, description: 'mine' }] })
    fireEvent.click(screen.getByRole('button', { name: t('settings.js8.freqs.reset.action') }))
    await act(async () => {
      await Promise.resolve()
    })
    expect(declined, 'declined: nothing changes').toEqual([])
  })

  it('Export saves the list to Downloads as the file Import reads back', async () => {
    const { saveTextToDownloads } = await import('../api')
    const rows = [{ mhz: 7.095, description: 'mine' }]
    const seen = await mount({ js8Frequencies: rows })
    fireEvent.click(screen.getByRole('button', { name: t('settings.js8.freqs.export.action') }))
    await waitFor(() => expect(saveTextToDownloads).toHaveBeenCalledTimes(1))
    const [name, text] = vi.mocked(saveTextToDownloads).mock.calls[0]
    expect(name).toMatch(/^nexus-js8-frequencies-\d{4}-\d{2}-\d{2}\.json$/)
    expect(JSON.parse(text)).toEqual({ nexusJs8Frequencies: 1, frequencies: rows })

    cleanup()
    const fresh = await mount({ js8Frequencies: [{ mhz: 3.578, description: '' }] })
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File([text], name, { type: 'application/json' })
    fireEvent.change(input, { target: { files: [file] } })
    await waitFor(() => expect(last(fresh)).toEqual({ js8Frequencies: rows }))
    expect(seen).toEqual([])
  })

  it('an unreadable file changes nothing and says so', async () => {
    const { pushToast } = await import('../toast')
    const seen = await mount({ js8Frequencies: [{ mhz: 3.578, description: '' }] })
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    fireEvent.change(input, { target: { files: [new File(['{"frequencies":[{"mhz":"x"}]}'], 'f.json')] } })
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t('settings.js8.freqs.import.bad'), 'error'))
    expect(seen).toEqual([])
  })
})

describe('return to the last used frequency and the band schedule', () => {
  it('both are switches, off by default, and the schedule says it changes the frequency by itself', async () => {
    const seen = await mount()
    const last = screen.getByRole('switch', { name: t('settings.js8.returnToLastDial.label') })
    const hop = screen.getByRole('switch', { name: t('settings.js8.schedule.label') })
    expect([last.getAttribute('aria-checked'), hop.getAttribute('aria-checked')]).toEqual(['false', 'false'])
    expect(screen.getByText(t('settings.js8.schedule.hint'))).toBeTruthy()
    fireEvent.click(last)
    fireEvent.click(hop)
    expect(seen).toEqual([{ js8ReturnToLastDial: true }, { js8BandHop: true }])
  })

  it('a schedule row is a dial and a UTC window, edited in place', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(Date.UTC(2026, 9, 15, 13, 5))
    const seen = await mount()
    fireEvent.change(screen.getByLabelText(t('settings.js8.schedule.add.mhz.aria')), { target: { value: '14.078' } })
    fireEvent.click(screen.getByRole('button', { name: t('settings.js8.schedule.add.action') }))
    expect(last(seen)).toEqual({
      js8Schedule: [{ mhz: 14.078, fromMin: 13 * 60 + 5, untilMin: 14 * 60 + 5, description: '' }],
    })
    fireEvent.change(screen.getByLabelText(t('settings.js8.schedule.until.aria', { n: 1 })), {
      target: { value: '02:30' },
    })
    expect(last(seen)).toEqual({
      js8Schedule: [{ mhz: 14.078, fromMin: 13 * 60 + 5, untilMin: 150, description: '' }],
    })
    vi.restoreAllMocks()
  })
})
