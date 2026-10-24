// The status lane after a TX On the sign-in hold refused: it says why the latch did not light, in
// the words for where the operator is, for as long as the hold stands, and nothing otherwise.
import { describe, expect, it } from 'vitest'
import { txOnHeldLane } from './txOnHeld'
import { EN } from '../i18n/en'

describe('the status lane after a TX On the sign-in hold refused', () => {
  it('says nothing while no TX On is held', () => {
    expect(txOnHeldLane(false, false)).toBeNull()
    expect(txOnHeldLane(undefined, false)).toBeNull()
    expect(txOnHeldLane(false, true)).toBeNull()
  })

  it('at the shack, says a newly signed-in device waits for approval, and where', () => {
    expect(txOnHeldLane(true, false)).toEqual({
      tier: 'warning',
      message: EN['shell.lane.txOnHeld.message'],
      detail: EN['shell.lane.txOnHeld.detail'],
    })
  })

  it('on a computer driving the station, says this device must be approved there', () => {
    expect(txOnHeldLane(true, true)).toEqual({
      tier: 'warning',
      message: EN['shell.lane.txOnHeld.message'],
      detail: EN['client.ptt.refusal.onHold'],
    })
  })
})
