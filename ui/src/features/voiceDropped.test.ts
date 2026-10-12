// The status-lane item for a voice-keyer message Nexus's own Flex client dropped without keying the
// radio: it says so, when, and why, by value, for each of the two causes.
import { describe, expect, it } from 'vitest'
import { voiceDroppedLane } from './voiceDropped'
import { EN } from '../i18n/en'

describe('the status lane after a voice-keyer message the Flex client dropped', () => {
  // 2026-10-12 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 12, 14, 32, 5) / 1000

  it('says nothing while no message was dropped', () => {
    expect(voiceDroppedLane(null)).toBeNull()
    expect(voiceDroppedLane(undefined)).toBeNull()
  })

  it('says the radio did not switch to DAX in time, and when', () => {
    expect(voiceDroppedLane({ at: AT, cause: 'noDax' })).toEqual({
      tier: 'warning',
      message: EN['shell.lane.voiceDropped.message'],
      detail: EN['shell.lane.voiceDropped.noDax'].replace('{{time}}', '14:32:05'),
    })
  })

  it('says something else keyed the radio first, and when', () => {
    expect(voiceDroppedLane({ at: AT, cause: 'keyed' })).toEqual({
      tier: 'warning',
      message: EN['shell.lane.voiceDropped.message'],
      detail: EN['shell.lane.voiceDropped.keyed'].replace('{{time}}', '14:32:05'),
    })
  })
})
