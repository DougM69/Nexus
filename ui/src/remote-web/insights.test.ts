import { expect, it } from 'vitest'
import { parseInsights } from './insights'
import { queryPage, type InsightCollection, type QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/insights.json'

export function insightPage(kind: InsightCollection = 'statistics'): QueryPage {
  return { type: 'applicationPage', collection: kind, requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
    meta: { capturedAgeMs: 0, source: { logCount: 2301, ...structuredClone(fixture) } } }
}
it('accepts complete summaries beyond the log window and keeps the existing chart ordering', () => {
  for (const kind of ['awards', 'statistics'] as const) {
    const page = insightPage(kind)
    expect(queryPage(page, 6).collection).toBe(kind)
    for (const version of [3, 4, 5]) expect(() => queryPage(page, version)).toThrow()
    const result = parseInsights(page, kind)
    expect(result.logCount).toBe(2301)
    if (result.kind === 'statistics') expect(result.statistics).toEqual(fixture.statistics)
  }
})
it('refuses mismatched, incomplete, malformed and expired insight pages', () => {
  const base = insightPage()
  for (const patch of [{ total: 2301 }, { retained: 1 }, { nextCursor: `${base.snapshotId}:1` },
    { offset: 1 }, { rows: [{}] }, { collection: 'awards' }]) {
    expect(() => parseInsights({ ...base, ...patch } as QueryPage, 'statistics')).toThrow()
  }
  for (const mutate of [
    (v: typeof fixture) => { v.statistics.total = 2000 },
    (v: typeof fixture) => { v.statistics.hourUtc = [] },
    (v: typeof fixture) => { v.statistics.byBand[0].count = 2302 },
    (v: typeof fixture) => { v.statistics.byBand.push(v.statistics.byBand[0]) },
    (v: typeof fixture) => { v.geography.domestic = -1 },
  ]) {
    const values = structuredClone(fixture); mutate(values)
    expect(() => parseInsights({ ...base, meta: { capturedAgeMs: 0, source: { logCount: 2301, ...values } } }, 'statistics')).toThrow()
  }
  expect(() => parseInsights({ ...base, meta: { capturedAgeMs: 60000, source: { logCount: 2301, ...fixture } } }, 'statistics')).toThrow()
  const broken = insightPage('awards')
  ;(broken.meta as { source: { awards: { vucc: unknown } } }).source.awards.vucc = null
  expect(() => parseInsights(broken, 'awards')).toThrow()
})
it('reads the DXCC totals and LoTW disagreements when a station sends them, and a station without them', () => {
  const page = insightPage('awards')
  const result = parseInsights(page, 'awards')
  if (result.kind !== 'awards') throw new Error('awards')
  expect(result.awards.dxccConfirmedTotal).toBe(3)
  expect(result.awards.dxccDisagreements?.[0]).toMatchObject({ call: 'K1N', loggedDxcc: 291, lotwDxcc: 182 })
  // A station older than the count by each record's own number sends none of the three.
  const older = insightPage('awards')
  const awards = (older.meta as { source: { awards: Record<string, unknown> } }).source.awards
  delete awards.dxccWorkedTotal
  delete awards.dxccConfirmedTotal
  delete awards.dxccDisagreements
  expect(parseInsights(older, 'awards').kind).toBe('awards')
  // An entity the station's table does not know arrives as null, and is accepted.
  const unnamed = insightPage('awards')
  ;(unnamed.meta as { source: { awards: { dxccDisagreements: { lotwEntity: unknown }[] } } }).source.awards.dxccDisagreements[0].lotwEntity = null
  expect(parseInsights(unnamed, 'awards').kind).toBe('awards')
  for (const mutate of [
    (a: Record<string, unknown>) => { a.dxccConfirmedTotal = -1 },
    (a: Record<string, unknown>) => { a.dxccDisagreements = [{ call: 'K1N' }] },
    (a: Record<string, unknown>) => { a.dxccDisagreements = [{ ...(a.dxccDisagreements as object[])[0], lotwDxcc: '182' }] },
    (a: Record<string, unknown>) => { a.dxccDisagreements = [{ ...(a.dxccDisagreements as object[])[0], loggedEntity: 7 }] },
  ]) {
    const broken = insightPage('awards')
    mutate((broken.meta as { source: { awards: Record<string, unknown> } }).source.awards)
    expect(() => parseInsights(broken, 'awards')).toThrow()
  }
})
it('reads the needs per band when a station sends them, and refuses a malformed list', () => {
  const result = parseInsights(insightPage('awards'), 'awards')
  if (result.kind !== 'awards') throw new Error('awards')
  expect(result.awards.bandNeeds?.dxcc[0]).toEqual({ band: '20m', unworked: ['Albania', 'Bouvet', 'Crozet Island'], unconfirmed: ['Japan'] })
  // A station older than the Stats pane sends none.
  const older = insightPage('awards')
  delete (older.meta as { source: { awards: Record<string, unknown> } }).source.awards.bandNeeds
  expect(parseInsights(older, 'awards').kind).toBe('awards')
  for (const mutate of [
    (n: Record<string, unknown>) => { delete n.waz },
    (n: Record<string, unknown>) => { n.dxcc = [{ band: '20m', unworked: 'Albania', unconfirmed: [] }] },
    (n: Record<string, unknown>) => { n.was = [{ band: 20, unworked: [], unconfirmed: [] }] },
    (n: Record<string, unknown>) => { n.waz = [{ band: '20m', unworked: [1], unconfirmed: [] }] },
  ]) {
    const broken = insightPage('awards')
    mutate((broken.meta as { source: { awards: { bandNeeds: Record<string, unknown> } } }).source.awards.bandNeeds)
    expect(() => parseInsights(broken, 'awards')).toThrow()
  }
})
