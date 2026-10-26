// @vitest-environment jsdom
//
// ⭐ #344 IN THE FT STRIP: "after running ATU maybe have it change to ATU ON (and then you could
// click to turn it off)". Operate draws its own ATU button (`OperateQsoStrip`), so the rule
// `CockpitTxStrip.atu.test.tsx` pins for the other screens is pinned here too: the label follows
// the radio's tuner, and with a switch-out wired the click on ATU ON takes the tuner out and runs
// no tune-up.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled: true, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true, ...over,
  }) as unknown as RadioStatus

function strip(over: Partial<RadioStatus>, onAtuTune: () => void, onAtuBypass: () => void) {
  const noop = vi.fn()
  render(
    <OperateQsoStrip
      qso={null}
      radio={radio(over)}
      onSetMode={noop}
      onCallCq={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onAtuTune={onAtuTune}
      onAtuBypass={onAtuBypass}
      onHaltTx={noop}
      onSetHoldTxFreq={noop}
    />,
  )
}

describe("Operate's ATU follows the radio's tuner (#344)", () => {
  it('ATU ON while the tuner is in line: the click switches it out and tunes nothing', () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: true }, tune, out)
    const on = screen.getByRole('button', { name: 'ATU ON' })
    expect(on.getAttribute('aria-pressed')).toBe('true')
    on.click()
    expect([out.mock.calls.length, tune.mock.calls.length]).toEqual([1, 0])
  })

  it('CONTROL: ATU while the tuner is out, and the click runs the tune-up', () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: false }, tune, out)
    const atu = screen.getByRole('button', { name: 'ATU' })
    expect(atu.getAttribute('aria-pressed')).toBe('false')
    atu.click()
    expect([out.mock.calls.length, tune.mock.calls.length]).toEqual([0, 1])
  })
})
