// Enter Sends Message's words, by value: a name for every step, and a sentence for every reason
// the step table gives — one to one, no two reasons sharing a sentence — with a step's refusal
// naming the step and its key.
import { describe, expect, it } from 'vitest'
import { ESM_ROLES, type EsmInert, type EsmRefusal } from './esm'
import { esmInertText, esmRefusalText, esmStepName } from './esmWords'

// Every reason, spelled out. `satisfies` makes the compiler refuse this file when the step table
// gains a reason nobody worded, or loses one this table still names.
const REFUSALS = {
  dupe: { why: 'dupe' },
  b4Sent: { why: 'b4Sent' },
  txOff: { why: 'txOff' },
  txLocked: { why: 'txLocked' },
  clockRepair: { why: 'clockRepair' },
  recording: { why: 'recording' },
  pttHeld: { why: 'pttHeld' },
  unmapped: { why: 'unmapped', role: 'tu' },
  empty: { why: 'empty', role: 'callExch', key: 'F2' },
  oneSlot: { why: 'oneSlot', role: 'exch' },
} satisfies { [W in EsmRefusal['why']]: Extract<EsmRefusal, { why: W }> }
const INERT = { noKeyer: true, auto: true, continuousTx: true, noRoles: true } satisfies Record<EsmInert, true>
const inert = Object.keys(INERT) as EsmInert[]

describe('Enter Sends Message — its words', () => {
  it('names CQ, TU, AGN and QSO B4 by the on-air words, and the other steps in words', () => {
    expect(Object.fromEntries(ESM_ROLES.map((role) => [role, esmStepName(role)]))).toEqual({
      cq: 'CQ',
      callExch: 'His call and your exchange',
      tu: 'TU',
      myCall: 'Your call',
      exch: 'Your S&P exchange',
      again: 'AGN',
      b4: 'QSO B4',
    })
  })

  it('says why Enter sent nothing, for every refusal, naming the step and the key where there is one', () => {
    expect(Object.fromEntries(Object.entries(REFUSALS).map(([why, r]) => [why, esmRefusalText(r)]))).toEqual({
      dupe: 'A dupe of your own log: Enter sends nothing and logs nothing.',
      b4Sent: 'QSO B4 went to this call. Enter logs it as a dupe once his exchange is copied.',
      txOff: 'TX is off, and Enter never turns it on. Turn TX on yourself, then press Enter.',
      txLocked:
        'TX is locked here: the dial is outside your license privileges, or this connection does not allow transmitting.',
      clockRepair: 'A clock repair is holding transmit, so Enter sends nothing.',
      recording: 'The voice keyer is recording, so Enter plays nothing.',
      pttHeld: 'You are holding PTT, so Enter plays nothing.',
      unmapped: 'TU: no key is mapped, so Enter sends nothing and logs nothing at that step.',
      empty: 'His call and your exchange: F2 is empty, so Enter sends nothing and logs nothing at that step.',
      oneSlot: 'Your S&P exchange: mapped to two recordings, and the keyer plays one per press.',
    })
  })

  it('says why ESM stepped aside, for every reason, and that Enter then logs as with ESM off', () => {
    expect(Object.fromEntries(inert.map((why) => [why, esmInertText(why)]))).toEqual({
      noKeyer: 'ESM steps aside while the voice keyer is hidden: Enter logs as it does with ESM off.',
      auto: 'ESM steps aside while the RTTY auto sequence runs: Enter logs as it does with ESM off.',
      continuousTx: 'ESM steps aside while Continuous TX is latched: Enter logs as it does with ESM off.',
      noRoles: 'This set has no step mapped, so ESM steps aside: Enter logs as it does with ESM off.',
    })
  })

  it('maps reasons to sentences one to one', () => {
    const texts = [...Object.values(REFUSALS).map(esmRefusalText), ...inert.map(esmInertText)]
    expect(texts).toHaveLength(14)
    expect(new Set(texts).size).toBe(texts.length)
  })
})
