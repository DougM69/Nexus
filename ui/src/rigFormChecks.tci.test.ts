// The TCI connection, as the Settings form checks it before a save.
//
// TCI has no serial port and no Hamlib model: the SDR program's server is the radio. So none of the
// serial checks may speak for it (a port that is not plugged in, a dial-in device, CAT keying with
// no model), and the two facts it does need, an address and a port, are the ones a save is refused
// without, because without either Nexus has nowhere to connect.
import { describe, it, expect } from 'vitest'
import { blocks, checkRigForm } from './rigFormChecks'
import { TCI_RIG_MODEL } from './tci'

const tci = (over: Partial<Parameters<typeof checkRigForm>[0]> = {}) => ({
  serialPort: '',
  rigConn: 'tci',
  pttMethod: 'cat',
  rigModel: TCI_RIG_MODEL,
  tciHost: '127.0.0.1',
  tciPort: 50001,
  ...over,
})

describe('the pre-save checks on the TCI connection', () => {
  it('pass a complete TCI radio, and ask for no serial port and no model', () => {
    expect(checkRigForm(tci(), [], [0, 1, 2], undefined, undefined, null)).toEqual([])
    // A serial port left over from before, unplugged, and a portless list that does not name TCI's
    // model: neither is TCI's business.
    expect(checkRigForm(tci({ serialPort: '/dev/tty.usbserial' }), [], [0, 1, 2])).toEqual([])
    // The control: the same leftovers on Serial are refused, so the checks above could have spoken.
    const serial = checkRigForm(tci({ rigConn: 'serial', serialPort: '/dev/tty.usbserial' }), [], [0, 1, 2])
    expect(blocks(serial)).toBe(true)
  })

  it('read an absent address or port as the default the form shows', () => {
    expect(checkRigForm(tci({ tciHost: undefined, tciPort: undefined }), [], [])).toEqual([])
  })

  it('refuse a cleared address, or one with a scheme, a path or a space', () => {
    for (const host of ['', '   ', 'ws://127.0.0.1', '127.0.0.1/', 'sdr pc']) {
      const checks = checkRigForm(tci({ tciHost: host }), [], [])
      expect(blocks(checks), JSON.stringify(host)).toBe(true)
      expect(checks[0].message).toContain('127.0.0.1')
    }
    // A host name and an IPv6 address are addresses.
    for (const host of ['shack-pc.local', '192.168.1.30', '::1']) {
      expect(checkRigForm(tci({ tciHost: host }), [], []), host).toEqual([])
    }
  })

  it('refuse a port outside 1–65535', () => {
    for (const port of [0, -1, 65536, 1.5, Number.NaN]) {
      const checks = checkRigForm(tci({ tciPort: port }), [], [])
      expect(blocks(checks), String(port)).toBe(true)
      expect(checks[0].message).toContain('50001')
    }
    expect(checkRigForm(tci({ tciPort: 40001 }), [], [])).toEqual([])
    expect(checkRigForm(tci({ tciPort: 65535 }), [], [])).toEqual([])
  })
})
