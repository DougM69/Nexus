// The station keeps a JS8 call as heard, `/P` included, as JS8Call's call activity keys it: W1AW and
// W1AW/P are two rows of the heard list (`stations`), a MSG from W1AW/P is filed from W1AW/P, and the
// roster's history is asked for by the call as heard. The page's parser takes all of it already (a
// call is any text of up to 32 bytes, each row's call once), so a station that sends them needs
// nothing new of the page. The control: the same call twice is still refused, so the parser does
// read the calls.
import { describe, expect, it } from 'vitest'
import fixture from './__fixtures__/js8.json'
import { parseJs8Context, parseJs8Sample } from './js8'
import type { QueryPage } from './application-query-protocol'

describe('JS8 calls kept as heard', () => {
  it('takes W1AW and W1AW/P as two stations, and a message from W1AW/P', () => {
    const value = structuredClone(fixture)
    value.state.stations[1] = { ...value.state.stations[0], call: 'W1AW/P', snrDb: -15 }
    value.state.inbox[0].from = 'W1AW/P'
    const state = parseJs8Sample(value, 0)
    expect(state.stations.map((h) => [h.call, h.snrDb])).toEqual([
      ['W1AW', -8],
      ['W1AW/P', -15],
    ])
    expect(state.inbox[0].from).toBe('W1AW/P')
    const twice = structuredClone(value)
    twice.state.stations[1].call = 'W1AW'
    expect(() => parseJs8Sample(twice, 0), 'control: one call twice').toThrow('invalidJs8')
  })

  it('takes the roster history of a call as heard', () => {
    const page = {
      collection: 'js8Context',
      offset: 0,
      total: 0,
      retained: 0,
      rows: [],
      nextCursor: null,
      meta: {
        capturedAgeMs: 0,
        source: { plan: [], history: { 'W1AW/P': { count: 1, lastUnix: 1700000000, grid: 'FN30', name: '', comment: '' } } },
      },
    } as unknown as QueryPage
    expect(parseJs8Context(page).history['W1AW/P']).toEqual({ count: 1, lastUnix: 1700000000, grid: 'FN30', name: '', comment: '' })
  })
})
