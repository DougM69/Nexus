// A TX On the sign-in hold refused, the UI half. While a newly signed-in device that is not yet
// approved controls the station, the station starts no transmission and refuses TX On too
// (crates/tempo-app/src/engine.rs, `set_tx_enabled`). The latch just stays dark, so the status lane
// says why, for as long as the hold stands: at the shack, where to approve the device; on a
// computer driving the station, that this device must be approved there. Display only.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a TX On the sign-in hold refused, or null. `client` is whether this
 *  computer drives another station. */
export function txOnHeldLane(held: boolean | undefined, client: boolean): Omit<StatusItem, 'id'> | null {
  if (!held) return null
  return {
    tier: 'warning',
    message: t('shell.lane.txOnHeld.message'),
    detail: client ? t('client.ptt.refusal.onHold') : t('shell.lane.txOnHeld.detail'),
  }
}
