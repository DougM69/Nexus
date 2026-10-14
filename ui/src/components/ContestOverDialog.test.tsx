// @vitest-environment jsdom
//
// THE CONTEST-OVER CHECKLIST — what it offers, by value, and that it does nothing unasked.
//
// The offer lists only what the contest changed, each as it is now and as it was before, and a
// blank callsign is never offered. Nothing is ticked to begin with. "Not now" does nothing at all.
// "Turn off" does exactly what was ticked, in the order that matters: the merge and the export
// while the session still exists, the export before anything is put back, Unassisted through its
// own switch, and only then the ONE save that turns the mode off with the rest put back. A step
// that fails stops the rest and leaves the mode on.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestOverDialog, contestOverOffer, hasOffer, type ContestOverOffer } from './ContestOverDialog'
import type { ContestBefore, Settings } from '../types'

const calls: string[] = []
vi.mock('../api', () => ({
  fdMergeToGeneral: vi.fn(async () => {
    calls.push('merge')
    return { added: 212, already: 0, refused: 0, queued: false }
  }),
  exportLog: vi.fn(async (format: string) => {
    calls.push(`export:${format}`)
    return 'START-OF-LOG: 3.0'
  }),
  saveTextToDownloads: vi.fn(async (name: string) => {
    calls.push(`save:${name}`)
    return `/home/op/Downloads/${name}`
  }),
  setUnassistedMode: vi.fn(async (on: boolean) => {
    calls.push(`unassisted:${on}`)
    return {}
  }),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
const stream = { input: false }
vi.mock('../remote-native/stream-input', () => ({ isStreamInput: () => stream.input }))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** The station on its everyday settings, as the engine's note holds them. */
const EVERYDAY: ContestBefore = {
  mycall: 'K9ABC',
  fdOperator: '',
  fdPositionName: '',
  contestCategoryOperator: '',
  contestEntryClass: '',
  contestCategoryPower: '',
  unassistedMode: false,
}

/** The same station at the end of a club weekend. */
const clubNow = (over: Partial<Settings> = {}): Settings =>
  ({
    mycall: 'W9CLB',
    fdOperator: 'W9OP',
    fdPositionName: 'CW tent',
    contestCategoryOperator: 'MULTI-OP',
    contestEntryClass: '',
    contestCategoryPower: 'LOW',
    unassistedMode: true,
    contestBefore: EVERYDAY,
    ...over,
  }) as unknown as Settings

beforeEach(() => {
  calls.length = 0
  stream.input = false
  for (const f of Object.values(api)) f.mockClear()
})
afterEach(cleanup)

describe('what the checklist offers', () => {
  it('lists only what the contest changed, each as it is now and as it was before, in order', () => {
    const offer = contestOverOffer(clubNow(), 212, 'ilqp')
    expect(offer.contacts).toBe(212)
    expect(offer.putBack).toEqual([
      { key: 'mycall', now: 'W9CLB', before: 'K9ABC' },
      { key: 'fdOperator', now: 'W9OP', before: '' },
      { key: 'fdPositionName', now: 'CW tent', before: '' },
      { key: 'contestCategoryOperator', now: 'MULTI-OP', before: '' },
      { key: 'contestCategoryPower', now: 'LOW', before: '' },
      { key: 'unassistedMode', now: true, before: false },
    ])
  })

  it('offers nothing to put back when the engine holds no note', () => {
    expect(contestOverOffer(clubNow({ contestBefore: null }), 5, 'ilqp').putBack).toEqual([])
  })

  it('never offers a blank callsign back', () => {
    const offer = contestOverOffer(clubNow({ contestBefore: { ...EVERYDAY, mycall: '' } }), 0, 'ilqp')
    expect(offer.putBack.map((r) => r.key)).not.toContain('mycall')
  })

  it('has something to offer with contacts or with a change, and nothing with neither', () => {
    expect(hasOffer({ contacts: 0, event: 'ilqp', putBack: [], clubQueued: 0 })).toBe(false)
    expect(hasOffer({ contacts: 1, event: 'ilqp', putBack: [], clubQueued: 0 })).toBe(true)
    expect(hasOffer(contestOverOffer(clubNow(), 0, 'ilqp'))).toBe(true)
  })

  it('carries the club chip\'s count, and has that to say with nothing else to offer', () => {
    expect(contestOverOffer(clubNow(), 5, 'ilqp').clubQueued).toBe(0)
    const owed = contestOverOffer(clubNow({ contestBefore: null }), 0, 'ilqp', 2)
    expect([owed.contacts, owed.putBack.length, owed.clubQueued]).toEqual([0, 0, 2])
    expect(hasOffer(owed)).toBe(true)
    // CONTROL: the same station with the club holding everything.
    expect(hasOffer(contestOverOffer(clubNow({ contestBefore: null }), 0, 'ilqp', 0))).toBe(false)
  })
})

/** Open the dialog on an offer, with spies for both answers. */
function open(offer: ContestOverOffer = contestOverOffer(clubNow(), 212, 'ilqp'), turnOff = vi.fn(async () => {
  calls.push('turnOff')
})) {
  const onNotNow = vi.fn()
  render(<ContestOverDialog offer={offer} onNotNow={onNotNow} onTurnOff={turnOff} />)
  return { onNotNow, turnOff }
}
const box = (name: RegExp) => screen.getByRole('checkbox', { name }) as HTMLInputElement
const press = async (name: RegExp) => {
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name }))
  })
}

describe('the checklist', () => {
  it('shows the merge with its count, the export, and each change now and before, none of them ticked', () => {
    open()
    expect(box(/Merge 212 contacts into my logbook/).checked).toBe(false)
    expect(box(/Export the Cabrillo log/).checked).toBe(false)
    const rows = [...document.querySelectorAll('.contest-over-row')].map((r) => [
      r.querySelector('.contest-over-setting')!.textContent,
      r.querySelector('.contest-over-now')!.textContent,
      r.querySelector('.contest-over-before')!.textContent,
      (r.querySelector('input') as HTMLInputElement).checked,
    ])
    expect(rows).toEqual([
      ['Callsign on the air', 'W9CLB', 'K9ABC', false],
      ['Operator at the key', 'W9OP', 'your callsign', false],
      ['Position name', 'CW tent', 'none', false],
      ['Entry category', 'MULTI-OP', 'SINGLE-OP', false],
      ['Power category', 'LOW', 'not declared', false],
      ['Unassisted entry', 'On', 'Off', false],
    ])
  })

  it('Not now does nothing at all', async () => {
    const { onNotNow, turnOff } = open()
    fireEvent.click(box(/Merge 212/))
    fireEvent.click(box(/Callsign on the air/))
    await press(/Not now/)
    expect(onNotNow).toHaveBeenCalledTimes(1)
    expect(turnOff).not.toHaveBeenCalled()
    expect(calls).toEqual([])
  })

  it('Turn off with nothing ticked only turns the mode off, putting nothing back', async () => {
    const { turnOff } = open()
    await press(/Turn off/)
    expect(calls).toEqual(['turnOff'])
    expect(turnOff).toHaveBeenCalledWith({})
  })

  it('does what is ticked in the order that matters, and only that', async () => {
    const { turnOff } = open()
    fireEvent.click(box(/Merge 212/))
    fireEvent.click(box(/Export the Cabrillo log/))
    fireEvent.click(box(/Callsign on the air/))
    fireEvent.click(box(/Unassisted entry/))
    await press(/Turn off/)
    const stamp = new Date().toISOString().slice(0, 10)
    expect(calls).toEqual([
      'merge',
      'export:cabrillo',
      `save:ilqp-log-${stamp}.cbr`,
      'unassisted:false',
      'turnOff',
    ])
    // ONE save turns the mode off, with exactly the ticked settings put back.
    expect(turnOff).toHaveBeenCalledTimes(1)
    expect(turnOff).toHaveBeenCalledWith({ mycall: 'K9ABC' })
  })

  it('puts back each ticked setting to its before, blank included', async () => {
    const { turnOff } = open()
    fireEvent.click(box(/Operator at the key/))
    fireEvent.click(box(/Position name/))
    fireEvent.click(box(/Entry category/))
    fireEvent.click(box(/Power category/))
    await press(/Turn off/)
    expect(turnOff).toHaveBeenCalledWith({
      fdOperator: '',
      fdPositionName: '',
      contestCategoryOperator: '',
      contestCategoryPower: '',
    })
    expect(api.setUnassistedMode).not.toHaveBeenCalled()
  })

  it('stops at a failed merge: no export, nothing put back, the mode left on, the reason shown', async () => {
    api.fdMergeToGeneral.mockImplementationOnce(async () => {
      calls.push('merge')
      throw 'the logbook is busy'
    })
    const { turnOff } = open()
    fireEvent.click(box(/Merge 212/))
    fireEvent.click(box(/Export the Cabrillo log/))
    fireEvent.click(box(/Callsign on the air/))
    await press(/Turn off/)
    expect(calls).toEqual(['merge'])
    expect(turnOff).not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toBe('the logbook is busy')
  })

  it('refuses the merge and the export through the Remote stream, in the Contest screen’s words', async () => {
    stream.input = true
    const { turnOff } = open()
    fireEvent.click(box(/Merge 212/))
    await press(/Turn off/)
    expect(calls).toEqual([])
    expect(turnOff).not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toBe(
      'Only at the station: a press through Remote merges no contact into the logbook.',
    )
  })

  it('at a club, says how many of this laptop\'s contacts the club\'s log does not have yet', () => {
    open(contestOverOffer(clubNow(), 212, 'ilqp', 2))
    expect(document.querySelector('.contest-over-club')?.textContent).toBe(
      "2 contacts from this laptop are not in the club's log yet. They still go to the host when this laptop is back with it, Contest mode on or off. Leave Nexus open, with club sync on, until then.",
    )
    cleanup()
    open(contestOverOffer(clubNow(), 212, 'ilqp', 1))
    expect(document.querySelector('.contest-over-club')?.textContent).toBe(
      "1 contact from this laptop is not in the club's log yet. It still goes to the host when this laptop is back with it, Contest mode on or off. Leave Nexus open, with club sync on, until then.",
    )
    // CONTROL: the club has them all, and nothing is said.
    cleanup()
    open(contestOverOffer(clubNow(), 212, 'ilqp', 0))
    expect(document.querySelector('.contest-over-club')).toBeNull()
  })

  it('with only the club\'s count to say, Turn off turns the mode off and puts nothing back', async () => {
    const { turnOff } = open(contestOverOffer(clubNow({ contestBefore: null }), 0, 'ilqp', 2))
    expect(screen.queryAllByRole('checkbox')).toEqual([])
    await press(/Turn off/)
    expect(calls).toEqual(['turnOff'])
    expect(turnOff).toHaveBeenCalledWith({})
  })

  it('offers no merge or export when the session holds no contacts', () => {
    open(contestOverOffer(clubNow(), 0, 'ilqp'))
    expect(screen.queryByRole('checkbox', { name: /Merge/ })).toBeNull()
    expect(screen.queryByRole('checkbox', { name: /Export/ })).toBeNull()
    expect(document.querySelectorAll('.contest-over-row')).toHaveLength(6)
  })
})
