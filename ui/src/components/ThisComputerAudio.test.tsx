// @vitest-environment jsdom
// This computer ▸ Audio: the headset a driving computer hears the station on and talks into. It
// shows only on a computer driving a station; it lists this computer's devices with the system's
// default first, keeps a pick here through `set_client_audio`, keeps a device that is not plugged
// in visible rather than silently replacing it, keeps how loud the station is heard here with the
// devices, shows the microphone's level and plays a test.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, renderHook, screen } from '@testing-library/react'
import type { ClientAudio, ClientStatus } from '../api'
import { EN } from '../i18n/en'

let desktop = true
const mockStatus = vi.fn<() => Promise<ClientStatus | null>>()
const mockAudio = vi.fn<() => Promise<ClientAudio>>()
const mockSet = vi.fn<(output: string, input: string, volumeDb?: number) => Promise<ClientAudio>>()
const mockLevel = vi.fn<() => Promise<{ level: number; open: boolean; trouble: string | null }>>()
const mockTest = vi.fn<() => Promise<{ played: boolean; trouble?: string | null }>>()
vi.mock('../api', () => ({
  isTauri: () => desktop,
  clientStatus: () => mockStatus(),
  clientAudio: () => mockAudio(),
  setClientAudio: (output: string, input: string, volumeDb?: number) => mockSet(output, input, volumeDb),
  clientAudioLevel: () => mockLevel(),
  clientAudioTest: () => mockTest(),
}))
import { METER_EVERY_MS, ThisComputerAudio, useDriving } from './ThisComputerAudio'

const VIEW: ClientAudio = {
  output: '',
  input: 'USB Headset Mic',
  volumeDb: 24,
  outputs: [
    { name: 'USB Headset', label: 'USB Headset' },
    { name: 'Speakers', label: 'Speakers (Realtek)' },
  ],
  inputs: [{ name: 'USB Headset Mic', label: 'USB Headset Microphone' }],
  available: true,
  trouble: { output: null, input: null },
}

beforeEach(() => {
  desktop = true
  vi.useFakeTimers()
  mockStatus.mockReset()
  mockAudio.mockReset().mockResolvedValue(VIEW)
  mockSet
    .mockReset()
    .mockImplementation(async (output, input, volumeDb) => ({ ...VIEW, output, input, volumeDb: volumeDb ?? VIEW.volumeDb }))
  mockLevel.mockReset().mockResolvedValue({ level: 0.42, open: true, trouble: null })
  mockTest.mockReset().mockResolvedValue({ played: true })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

async function settled() {
  await act(async () => {
    await Promise.resolve()
  })
}

describe('whether the section shows', () => {
  it('only on a computer driving a station, asked once', async () => {
    mockStatus.mockResolvedValue({ station: '192.168.1.20:42075', phase: 'live', reason: null })
    const driving = renderHook(() => useDriving())
    await settled()
    expect(driving.result.current).toBe(true)
    mockStatus.mockResolvedValue(null)
    const own = renderHook(() => useDriving())
    await settled()
    expect(own.result.current).toBe(false)
    desktop = false
    const hosted = renderHook(() => useDriving())
    await settled()
    expect(hosted.result.current).toBe(false)
    expect(mockStatus).toHaveBeenCalledTimes(2)
  })
})

describe('This computer ▸ Audio', () => {
  it('lists this computer’s devices, the system default first, and keeps a pick here', async () => {
    render(<ThisComputerAudio />)
    await settled()
    const output = screen.getByRole('combobox', {
      name: EN['settings.thisComputer.audio.output.label'],
    }) as HTMLSelectElement
    expect([...output.options].map((o) => o.textContent)).toEqual([
      EN['settings.thisComputer.audio.default'],
      'USB Headset',
      'Speakers (Realtek)',
    ])
    expect(output.value).toBe('')
    fireEvent.change(output, { target: { value: 'USB Headset' } })
    await settled()
    expect(mockSet).toHaveBeenCalledWith('USB Headset', 'USB Headset Mic', 24)
    expect(output.value).toBe('USB Headset')
  })

  it('keeps how loud the station is heard here with the devices: the page’s 0 to 42 dB, read as gain', async () => {
    mockAudio.mockResolvedValue({ ...VIEW, volumeDb: 30 })
    render(<ThisComputerAudio />)
    await settled()
    const volume = screen.getByRole('slider', {
      name: EN['settings.thisComputer.audio.volume.label'],
    }) as HTMLInputElement
    expect([volume.min, volume.max, volume.step, volume.value]).toEqual(['0', '42', '1', '30'])
    expect(volume.getAttribute('aria-valuetext')).toBe('+30 dB')
    fireEvent.change(volume, { target: { value: '36' } })
    await settled()
    expect(mockSet).toHaveBeenCalledWith('', 'USB Headset Mic', 36)
    expect(volume.value).toBe('36')
    expect(volume.getAttribute('aria-valuetext')).toBe('+36 dB')
  })

  it('keeps a chosen device that is not plugged in visible, never another in its place', async () => {
    mockAudio.mockResolvedValue({ ...VIEW, input: 'Old Mic' })
    render(<ThisComputerAudio />)
    await settled()
    const input = screen.getByRole('combobox', {
      name: EN['settings.thisComputer.audio.input.label'],
    }) as HTMLSelectElement
    expect(input.value).toBe('Old Mic')
    expect(input.selectedOptions[0].textContent).toBe('Old Mic (not connected)')
  })

  it('shows the microphone’s level while it is open, and plays a test', async () => {
    render(<ThisComputerAudio />)
    await settled()
    await act(async () => {
      vi.advanceTimersByTime(METER_EVERY_MS)
      await Promise.resolve()
    })
    const meter = screen.getByLabelText(EN['settings.thisComputer.audio.meter.label']) as HTMLMeterElement
    expect(meter.value).toBeCloseTo(0.42)
    fireEvent.click(screen.getByRole('button', { name: EN['settings.thisComputer.audio.test'] }))
    await settled()
    expect(mockTest).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('says why a device would not open, and when a test could not play', async () => {
    mockTest.mockResolvedValue({ played: false, trouble: 'device unplugged' })
    mockLevel.mockResolvedValue({ level: 0, open: false, trouble: 'no such microphone' })
    render(<ThisComputerAudio />)
    await settled()
    await act(async () => {
      vi.advanceTimersByTime(METER_EVERY_MS)
      await Promise.resolve()
    })
    fireEvent.click(screen.getByRole('button', { name: EN['settings.thisComputer.audio.test'] }))
    await settled()
    const alerts = screen.getAllByRole('alert').map((a) => a.textContent)
    expect(alerts).toContain(
      EN['settings.thisComputer.audio.output.trouble'].replace('{{why}}', 'device unplugged'),
    )
    expect(alerts).toContain(
      EN['settings.thisComputer.audio.input.trouble'].replace('{{why}}', 'no such microphone'),
    )
  })

  it('says so when this computer’s audio did not start, and asks nothing more', async () => {
    mockAudio.mockResolvedValue({ ...VIEW, available: false })
    render(<ThisComputerAudio />)
    await settled()
    expect(screen.getByRole('status').textContent).toBe(EN['settings.thisComputer.audio.unavailable'])
    await act(async () => {
      vi.advanceTimersByTime(METER_EVERY_MS * 5)
    })
    expect(mockLevel).not.toHaveBeenCalled()
  })
})
