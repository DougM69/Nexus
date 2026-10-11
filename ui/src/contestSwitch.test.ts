// @vitest-environment jsdom
//
// A CONTEST SWITCHED BY A SAVE, as the station's snapshots show it (`contestSwitch.ts`). The
// Contest screen says "Now running …" from this alone, so each case holds it to what the
// snapshots did, by value: a switch is one contest followed straight by another, and nothing else
// is one — not off-and-on, not a window opening on a contest, not the blank id ARRL Field Day is.
import { describe, expect, it } from 'vitest'
import { renderHook } from '@testing-library/react'
import { followContest, runningContest, useContestSwitch, type ContestSeen } from './contestSwitch'
import type { FieldDayStatus } from './types'

/** The tracker over a run of snapshots' contests, from a window opened on the first. */
const walk = (contests: (string | null)[]): ContestSeen =>
  contests.slice(1).reduce(followContest, { last: contests[0], switched: null })

const session = (event: string) => ({ event }) as FieldDayStatus

describe('the tracker, snapshot by snapshot', () => {
  it('a save that switches: one contest straight to another, both named', () => {
    expect(walk(['arrlfd', 'arrlfd', 'ilqp']).switched).toEqual({ from: 'arrlfd', to: 'ilqp' })
  })

  it('off and on is no switch: a snapshot outside a contest forgets the one before', () => {
    expect(walk(['arrlfd', null, 'ilqp']).switched).toBeNull()
  })

  it('a window that opens on a contest has seen none before it', () => {
    expect(walk(['ilqp']).switched).toBeNull()
    expect(walk([null, 'ilqp']).switched).toBeNull()
  })

  it('said while that contest runs, and gone with it', () => {
    expect(walk(['arrlfd', 'ilqp', 'ilqp', 'ilqp']).switched).toEqual({ from: 'arrlfd', to: 'ilqp' })
    expect(walk(['arrlfd', 'ilqp', null]).switched).toBeNull()
  })

  it('a second switch replaces the first', () => {
    expect(walk(['arrlfd', 'ilqp', 'cqww_cw']).switched).toEqual({ from: 'ilqp', to: 'cqww_cw' })
  })

  it('the blank id is ARRL Field Day, and no contest is null', () => {
    expect([runningContest(session('')), runningContest(session('wfd')), runningContest(null)]).toEqual([
      'arrlfd',
      'wfd',
      null,
    ])
  })
})

describe('the hook, render by render', () => {
  it('says the switch for the session running, and only for it', () => {
    const { result, rerender } = renderHook(({ fd }) => useContestSwitch(fd), {
      initialProps: { fd: session('') as FieldDayStatus | null },
    })
    expect(result.current).toBeNull()
    rerender({ fd: session('arrlfd') })
    expect(result.current, 'blank to arrlfd is the same contest').toBeNull()
    rerender({ fd: session('ilqp') })
    expect(result.current).toEqual({ from: 'arrlfd', to: 'ilqp' })
    rerender({ fd: null })
    expect(result.current).toBeNull()
    rerender({ fd: session('arrlfd') })
    expect(result.current, 'off and on').toBeNull()
  })

  it('no render ever names a contest other than the one running, a second switch included', () => {
    const said: [string | null, unknown][] = []
    const { rerender } = renderHook(
      ({ fd }) => {
        const s = useContestSwitch(fd)
        said.push([runningContest(fd), s])
        return s
      },
      { initialProps: { fd: session('arrlfd') as FieldDayStatus | null } },
    )
    rerender({ fd: session('ilqp') })
    rerender({ fd: session('cqww_cw') })
    const wrong = said.filter(([now, s]) => s !== null && (s as { to: string }).to !== now)
    expect(wrong).toEqual([])
    expect(said[said.length - 1]).toEqual(['cqww_cw', { from: 'ilqp', to: 'cqww_cw' }])
  })
})
