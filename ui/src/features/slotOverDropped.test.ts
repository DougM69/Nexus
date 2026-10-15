// The status lane for an FT8 or FT4 over the radio keyed only after its period had ended: the station
// dropped that over and left TX on, and the lane says so, when and how late, in a lane of its own; after
// that many in a row TX stopped, and the lane says that instead, until TX is on again.
import { describe, expect, it } from 'vitest'
import { slotOverDroppedLane } from './slotOverDropped'
import { EN } from '../i18n/en'

describe('the status lane after a dropped FT8/FT4 over', () => {
  // 2026-10-15 14:32:05 UTC
  const AT = Date.UTC(2026, 9, 15, 14, 32, 5) / 1000

  it('says nothing while no over was dropped', () => {
    expect(slotOverDroppedLane(null)).toBeNull()
    expect(slotOverDroppedLane(undefined)).toBeNull()
  })

  it('says the over was dropped, when and how late, and that TX stayed on: a warning, not a stop', () => {
    const lane = slotOverDroppedLane({ at: AT, late: '1.2 s', count: 1 })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('warning')
    expect(lane!.message).toBe(EN['shell.lane.slotOverDropped.message'])
    expect(lane!.message).not.toContain('STOPPED')
    expect(lane!.detail).toBe(
      EN['shell.lane.slotOverDropped.detail'].replace('{{time}}', '14:32:05').replace('{{late}}', '1.2 s'),
    )
    expect(lane!.detail).toContain('1.2 s after your period had ended (slow CAT)')
    expect(lane!.detail).toContain('left TX on')
    expect(lane!.detail).not.toMatch(/\{\{/)
  })

  it('reads the same for the second in a row: still a warning while TX is on', () => {
    const lane = slotOverDroppedLane({ at: AT, late: '0.4 s', count: 2 })
    expect(lane!.tier).toBe('warning')
    expect(lane!.detail).toContain('0.4 s after your period had ended')
  })

  it('says TX stopped after that many in a row, how many, when the last was and how late', () => {
    const lane = slotOverDroppedLane({ at: AT, late: '0.3 s', count: 3, txStopped: true })
    expect(lane!.tier).toBe('critical')
    expect(lane!.message).toBe(EN['shell.lane.slotOverDropped.stopped.message'])
    expect(lane!.detail).toBe(
      EN['shell.lane.slotOverDropped.stopped.detail'].other
        .replace('{{count}}', '3')
        .replace('{{time}}', '14:32:05')
        .replace('{{late}}', '0.3 s'),
    )
    expect(lane!.detail).toContain('3 overs in a row')
    expect(lane!.detail).toContain('turn TX on again')
    expect(lane!.detail).not.toMatch(/\{\{/)
  })

  it('says it of one over when one was enough to stop TX', () => {
    const lane = slotOverDroppedLane({ at: AT, late: '1.0 s', count: 1, txStopped: true })
    expect(lane!.tier).toBe('critical')
    expect(lane!.detail).toBe(
      EN['shell.lane.slotOverDropped.stopped.detail'].one
        .replace('{{time}}', '14:32:05')
        .replace('{{late}}', '1.0 s'),
    )
  })
})
