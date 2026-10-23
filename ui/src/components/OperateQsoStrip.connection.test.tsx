// @vitest-environment jsdom
//
// A CONNECTION THAT TRANSMITS NOTHING SAYS SO ON THE FT8/FT4 SCREEN TOO. The Icom network
// connection is receive and control only in its Beta: the engine refuses every transmission on it
// and names the connection (`radio.txRefusal`). The other screens' TX strip reads "No TX on this
// connection" with that reason on hover (CockpitTxStrip.connection.test.tsx); this strip read
// "▼ Receiving" over a Call CQ that keyed nothing, with no words saying why. The control: with no
// refusal the strip reads as it always has.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'

import { OperateQsoStrip } from './OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(cleanup)

const REFUSED = 'Transmit is off on the Icom network connection (Beta: receive and control only)'

const qso = { running: false, state: 'Idle', dxcall: null, txNow: null } as unknown as QsoStatus

const radioWith = (over: Record<string, unknown> = {}) =>
  ({
    dialMhz: 14.074,
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    holdTxFreq: false,
    txAllowed: true,
    ...over,
  }) as unknown as RadioStatus

function strip(radio: RadioStatus) {
  return render(
    <OperateQsoStrip
      qso={qso}
      radio={radio}
      onSetMode={vi.fn()}
      onCallCq={vi.fn()}
      onResend={vi.fn()}
      onFreetext={vi.fn()}
      onLog={vi.fn()}
      onSetTxEnabled={vi.fn()}
      onSetTune={vi.fn()}
      onHaltTx={vi.fn()}
      onSetHoldTxFreq={vi.fn()}
    />,
  )
}

describe('the FT8/FT4 strip on a connection that transmits nothing', () => {
  it('names the connection, with its reason on hover', () => {
    strip(radioWith({ txAllowed: false, txRefusal: REFUSED }))
    const state = screen.getByText('No TX on this connection')
    expect(state.getAttribute('title')).toBe(REFUSED)
    expect(screen.queryByText('▼ Receiving')).toBeNull()
  })

  it('names the connection with TX off as well', () => {
    strip(radioWith({ txEnabled: false, txAllowed: false, txRefusal: REFUSED }))
    expect(screen.getByText('No TX on this connection').getAttribute('title')).toBe(REFUSED)
    expect(screen.queryByText('■ TX off')).toBeNull()
  })

  it('reads as it always has with no refusal', () => {
    strip(radioWith())
    expect(screen.queryByText('No TX on this connection')).toBeNull()
    const state = screen.getByText('▼ Receiving')
    expect(state.getAttribute('title')).toBeNull()
  })

  it('still shows what the radio itself puts on the air', () => {
    strip(radioWith({ txAllowed: false, txRefusal: REFUSED, rigKeyed: true }))
    expect(screen.queryByText('No TX on this connection')).toBeNull()
    expect(screen.getByText('▲ TRANSMITTING').getAttribute('title')).toBeNull()
  })
})
