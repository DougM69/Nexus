// @vitest-environment jsdom
//
// The band picker's 🔒 chip names the rule when the phone MODE is the whole reason for the lock:
// LSB, AM or FM on 60 m, where a USB pick would key (`phoneModeRefused`). The frequency is fine
// there, so "This frequency/mode is outside your license privileges" pointed the operator at the
// wrong remedy. Every other lock keeps its words, split included.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandPicker } from './BandPicker'
import type { AppSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getLicensedBandPlan: vi.fn(async () => []),
}))

afterEach(cleanup)

const snap = (radio: Record<string, unknown>) =>
  ({ radio: { dialMhz: 5.36, band: '60m', txAllowed: false, ...radio } }) as unknown as AppSnapshot

const chipTitle = (radio: Record<string, unknown>) => {
  render(<BandPicker snap={snap(radio)} mode="phone" />)
  return document.querySelector('.tx-lock')?.getAttribute('title')
}

describe('the band picker lock chip on 60 m', () => {
  it('names the rule when the mode is the whole reason', () => {
    expect(chipTitle({ phoneModeRefused: true, txEmissionMhz: 5.36 })).toBe(
      '60 m allows only USB phone — transmit is blocked. Pick USB or AUTO.',
    )
  })

  it('keeps the frequency sentence for a frequency refusal', () => {
    expect(chipTitle({ dialMhz: 5.34, txEmissionMhz: 5.34 })).toBe(
      'This frequency/mode is outside your license privileges — transmit is blocked. Pick a band above, or change your license class in Settings.',
    )
  })

  it('keeps the split sentence for a split transmit frequency outside the privileges', () => {
    expect(chipTitle({ dialMhz: 5.36, txEmissionMhz: 5.34 })).toBe(
      'TX locked — your split transmit frequency, 5.3400 MHz, is outside your license privileges. Receiving on 5.3600 MHz is fine.',
    )
  })

  it('shows no chip while transmit is allowed', () => {
    expect(chipTitle({ txAllowed: true, phoneUsbOnly: true })).toBeUndefined()
  })
})
