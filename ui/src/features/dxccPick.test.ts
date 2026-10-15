// What the Logbook's country picker finds. The names and numbers are the station's own
// (`propagation::dxcc::entities`: cty.dat's for a current entity, TrustedQSL's for a deleted one).
import { describe, expect, it } from 'vitest'
import type { DxccEntityOption } from '../types'
import { findEntities, lotwDxccOf } from './dxccPick'

// Not in number order, as no list a search reads need be.
const ENTITIES: DxccEntityOption[] = [
  { code: 185, name: 'Niue', deleted: false },
  { code: 1, name: 'Canada', deleted: false },
  { code: 18, name: 'Chile', deleted: false },
  { code: 182, name: 'Navassa Island', deleted: false },
  { code: 229, name: 'GERMAN DEMOCRATIC REPUBLIC', deleted: true },
  { code: 230, name: 'Fed. Rep. of Germany', deleted: false },
  { code: 291, name: 'United States', deleted: false },
  { code: 517, name: 'Curacao', deleted: false },
  { code: 85, name: 'BONAIRE,CURACAO (NETH ANTILLES)', deleted: true },
  { code: 168, name: 'Marshall Islands', deleted: false },
]

const names = (search: string) => findEntities(ENTITIES, search).found.map((e) => e.name)

describe('the country picker finds an entity', () => {
  it('finds a deleted entity by its name, case and word order of the search aside', () => {
    expect(findEntities(ENTITIES, 'german dem').found).toEqual([
      { code: 229, name: 'GERMAN DEMOCRATIC REPUBLIC', deleted: true },
    ])
    expect(names('democratic GERMAN')).toEqual(['GERMAN DEMOCRATIC REPUBLIC'])
    // Every word must be in the name: a word it lacks finds nothing.
    expect(names('german kingdom')).toEqual([])
  })

  it('puts the names that begin with the search first, then the rest, each by name', () => {
    // "Curacao" begins with it; the deleted Netherlands Antilles only contain it.
    expect(names('curacao')).toEqual(['Curacao', 'BONAIRE,CURACAO (NETH ANTILLES)'])
    expect(names('isl')).toEqual(['Marshall Islands', 'Navassa Island'])
    expect(names('germ')).toEqual(['GERMAN DEMOCRATIC REPUBLIC', 'Fed. Rep. of Germany'])
  })

  it('finds a name typed with its accent', () => {
    expect(names('Curaçao')).toEqual(['Curacao', 'BONAIRE,CURACAO (NETH ANTILLES)'])
  })

  it('finds by number: that number first, then the numbers that begin with it', () => {
    expect(findEntities(ENTITIES, '182').found.map((e) => e.code)).toEqual([182])
    expect(findEntities(ENTITIES, '18').found.map((e) => e.code)).toEqual([18, 182, 185])
    expect(findEntities(ENTITIES, ' 229 ').found.map((e) => e.name)).toEqual(['GERMAN DEMOCRATIC REPUBLIC'])
  })

  it('lists every entity by name for an empty search, and counts what the limit leaves out', () => {
    const all = findEntities(ENTITIES, '  ')
    expect(all.total).toBe(ENTITIES.length)
    expect(all.found.map((e) => e.name)).toEqual([...ENTITIES.map((e) => e.name)].sort((a, b) => a.localeCompare(b)))
    const some = findEntities(ENTITIES, '', 3)
    expect([some.found.length, some.total]).toEqual([3, ENTITIES.length])
  })
})

describe("LoTW's number for a contact", () => {
  it('is read from the field the station keeps it in, and only from that field', () => {
    expect(lotwDxccOf({ extra: [['APP_NEXUS_LOTW_DXCC', '182']] })).toBe(182)
    expect(lotwDxccOf({ extra: [['CQZ', '8'], ['APP_NEXUS_LOTW_DXCC', ' 20 ']] })).toBe(20)
    expect(lotwDxccOf({ extra: [['DXCC', '182']] })).toBeNull()
    expect(lotwDxccOf({ extra: [['APP_NEXUS_LOTW_DXCC', 'x']] })).toBeNull()
    expect(lotwDxccOf({})).toBeNull()
  })
})
