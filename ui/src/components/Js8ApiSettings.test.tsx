// @vitest-environment jsdom
//
// Settings ▸ Digital ▸ JS8 ▸ JS8Call API: both switches off and JS8Call's ports when the settings
// predate them, each switch and port saved through the form's own setters, a port outside 1–65535
// never saved, and the four keys carried by a saved profile like the WSJT-X UDP API they sit beside.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { Js8ApiSettings, parsePort } from './Js8ApiSettings'
import { mergeProfile } from '../profiles'
import { EN } from '../i18n/en'
import type { Settings } from '../types'

afterEach(cleanup)

const form = (over: Partial<Settings> = {}) => ({ mycall: 'KD9TAW', ...over }) as Settings

function setup(over: Partial<Settings> = {}) {
  const onSwitch = vi.fn()
  const onPort = vi.fn()
  render(<Js8ApiSettings form={form(over)} onSwitch={onSwitch} onPort={onPort} />)
  const tcp = screen.getByRole('switch', { name: EN['settings.js8.api.tcp.label'] })
  const udp = screen.getByRole('switch', { name: EN['settings.js8.api.udp.label'] })
  const tcpPort = screen.getByLabelText(EN['settings.js8.api.tcpPort.label'], { exact: false }) as HTMLInputElement
  const udpPort = screen.getByLabelText(EN['settings.js8.api.udpPort.label'], { exact: false }) as HTMLInputElement
  return { onSwitch, onPort, tcp, udp, tcpPort, udpPort }
}

describe('JS8Call API settings', () => {
  it("reads off, on JS8Call's ports, when the settings predate them", () => {
    const { tcp, udp, tcpPort, udpPort } = setup()
    expect(tcp.getAttribute('aria-checked')).toBe('false')
    expect(udp.getAttribute('aria-checked')).toBe('false')
    expect(tcpPort.value).toBe('2442')
    expect(udpPort.value).toBe('2242')
    expect(screen.getByText(EN['settings.js8.api.title'])).toBeTruthy()
  })

  it('shows what is saved and saves each switch and port through the form', () => {
    const { onSwitch, onPort, tcp, udp, tcpPort, udpPort } = setup({
      js8ApiTcp: true,
      js8ApiTcpPort: 2500,
      js8ApiUdpPort: 2600,
    })
    expect(tcp.getAttribute('aria-checked')).toBe('true')
    expect(tcpPort.value).toBe('2500')
    expect(udpPort.value).toBe('2600')
    fireEvent.click(tcp)
    fireEvent.click(udp)
    expect(onSwitch.mock.calls).toEqual([
      ['js8ApiTcp', false],
      ['js8ApiUdp', true],
    ])
    fireEvent.change(tcpPort, { target: { value: '2443' } })
    fireEvent.change(udpPort, { target: { value: '65535' } })
    expect(onPort.mock.calls).toEqual([
      ['js8ApiTcpPort', 2443],
      ['js8ApiUdpPort', 65535],
    ])
  })

  it('never saves a port outside 1–65535', () => {
    const { onPort, tcpPort } = setup()
    for (const value of ['0', '65536', '-1', '24.5', 'abc', '']) {
      fireEvent.change(tcpPort, { target: { value } })
    }
    expect(onPort).not.toHaveBeenCalled()
    expect([parsePort('1'), parsePort(' 2442 '), parsePort('1e3')]).toEqual([1, 2442, 1000])
    expect([parsePort('0'), parsePort('65536'), parsePort(''), parsePort('2442x')]).toEqual([
      null,
      null,
      null,
      null,
    ])
  })

  // A feed other programs listen to, as the WSJT-X UDP API is: it travels with a saved profile.
  it('comes from a saved profile, every key', () => {
    const current = form({ js8ApiTcp: false, js8ApiTcpPort: 2442, js8ApiUdp: false, js8ApiUdpPort: 2242 })
    const saved = { js8ApiTcp: true, js8ApiTcpPort: 2443, js8ApiUdp: true, js8ApiUdpPort: 2243 } as Settings
    const merged = mergeProfile(current, saved)
    expect([merged.js8ApiTcp, merged.js8ApiTcpPort, merged.js8ApiUdp, merged.js8ApiUdpPort]).toEqual([
      true,
      2443,
      true,
      2243,
    ])
  })
})
