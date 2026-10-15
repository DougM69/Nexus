import { describe, expect, it } from 'vitest'
import fixture from './__fixtures__/js8.json'
import { parseJs8Sample } from './js8'

/** A note of a message a cap took out of the inbox, as the station sends it. */
const note = (why: string) => ({ atMs: fixture.capturedAtMs, why, entry: structuredClone(fixture.state.inbox[0]) })

describe('JS8 observation: the Inbox notes of what a cap dropped', () => {
  it('takes a station that sends notes, and one that sends none', () => {
    const value = structuredClone(fixture) as typeof fixture & { state: { inboxDropped?: unknown } }
    value.state.inboxDropped = [note('count'), note('bytes')]
    expect(parseJs8Sample(value, 0).inboxDropped?.map((d) => d.why)).toEqual(['count', 'bytes'])
    expect(parseJs8Sample(structuredClone(fixture), 0).inboxDropped).toBeUndefined()
  })
  it('refuses a malformed note', () => {
    for (const notes of [
      [note('expired')],
      Array.from({ length: 11 }, () => note('count')),
      [{ ...note('count'), extra: true }],
      [{ ...note('count'), entry: { ...fixture.state.inbox[0], state: 'lost' } }],
      [{ ...note('count'), atMs: -1 }],
      'none',
    ]) {
      const value = structuredClone(fixture) as typeof fixture & { state: { inboxDropped?: unknown } }
      value.state.inboxDropped = notes
      expect(() => parseJs8Sample(value, 0)).toThrow()
    }
  })
})
