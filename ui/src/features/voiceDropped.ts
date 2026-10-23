// A voice-keyer message Nexus's own Flex client dropped without keying the radio, the UI half. With
// native DAX audio on, Phone at the shack keeps the radio's mic, and each message waits for the
// radio to take its transmit audio from DAX (operator ruling, 2026-10-12, "Change it now"); one the
// radio did not switch for within 2 seconds, or that something else keyed before, is dropped, never
// played later (crates/tempo-app/src/engine.rs, `drop_flex_voice`). This is the operator's sentence
// for it, on the status lane, since the keyer's own toast answered the press before the drop. The
// next message the keyer accepts clears it.

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts) from birth. Every key is
// written literally.

import type { VoiceDropped } from '../types'
import type { StatusItem } from '../status'
import { t } from '../i18n'

/** The status-lane item after a voice-keyer message the Flex client dropped, or null. */
export function voiceDroppedLane(d: VoiceDropped | null | undefined): Omit<StatusItem, 'id'> | null {
  if (!d) return null
  // UTC, as every time an operator logs is.
  const time = new Date(d.at * 1000).toISOString().slice(11, 19)
  return {
    tier: 'warning',
    message: t('shell.lane.voiceDropped.message'),
    detail:
      d.cause === 'noDax'
        ? t('shell.lane.voiceDropped.noDax', { time })
        : t('shell.lane.voiceDropped.keyed', { time }),
  }
}
