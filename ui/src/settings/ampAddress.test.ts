import { describe, expect, it } from 'vitest'
import { ampPortIsNetwork, joinAmpAddress, KPA_NET_PORT, splitAmpAddress } from './ampAddress'

describe('amplifier address', () => {
  // The same cases as `a_kpa1500_network_address_is_told_apart_from_a_serial_device` in
  // crates/tempo-audio/src/amplifier.rs: the UI and the poller must read one string the same way.
  it('reads host:port as the network and every serial spelling as serial', () => {
    for (const net of ['192.168.1.50:1500', ' kpa.lan:1501 ', '[fe80::1]:1500', ':1500']) {
      expect(ampPortIsNetwork(net), net).toBe(true)
    }
    for (const dev of [
      'COM8',
      'COM3:',
      '\\\\.\\COM10',
      '/dev/ttyUSB2',
      '/dev/cu.usbserial-1410',
      '/dev/serial/by-id/usb-FTDI_FT231X-if00:1',
      '',
      'kpa.lan:0',
      'kpa.lan:65536',
      'kpa.lan:15x0',
    ]) {
      expect(ampPortIsNetwork(dev), dev).toBe(false)
    }
  })

  it('round-trips the two fields through one address', () => {
    expect(KPA_NET_PORT).toBe(1500)
    expect(joinAmpAddress('192.168.1.50', 1500)).toBe('192.168.1.50:1500')
    expect(splitAmpAddress('192.168.1.50:1501')).toEqual({ host: '192.168.1.50', port: 1501 })
    expect(joinAmpAddress('fe80::1', 1500)).toBe('[fe80::1]:1500')
    expect(splitAmpAddress('[fe80::1]:1500')).toEqual({ host: 'fe80::1', port: 1500 })
    // Network picked, nothing typed yet: still the network, so the fields stay on screen.
    expect(joinAmpAddress('', KPA_NET_PORT)).toBe(':1500')
    expect(ampPortIsNetwork(joinAmpAddress('', KPA_NET_PORT))).toBe(true)
    // A pasted URL or a cleared port cannot flip the address to a serial device.
    expect(joinAmpAddress('http://10.0.0.7', Number(''))).toBe('[http:10.0.0.7]:1500')
    expect(ampPortIsNetwork(joinAmpAddress('http://10.0.0.7', 0))).toBe(true)
  })
})
