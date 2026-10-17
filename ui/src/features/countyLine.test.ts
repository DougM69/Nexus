// The county-line reader, tested at the seam the entry strip calls, so the strip's own tests
// assert against a checked answer. Every verdict is paired with the case that must NOT give it.
import { describe, it, expect } from 'vitest'
import {
  countyLineDomain,
  isCountyLine,
  lastCountyPart,
  readCountyLine,
  resolveCountyLine,
  withLastCountyPart,
} from './countyLine'
import { ILQP_COUNTIES } from './ilqpQth'

describe('which box takes a county line', () => {
  it('is the Illinois county list, and no other list this build carries', () => {
    expect(countyLineDomain(['il_counties', 'il_mults'])).toBe('il_counties')
    expect(countyLineDomain(['il_counties'])).toBe('il_counties')
    // New York's counties keep one county per contact: their rules have not been checked
    // for this, and an unchecked rule is not a rule.
    expect(countyLineDomain(['ny_counties', 'ny_mults'])).toBeUndefined()
    expect(countyLineDomain(['fd_sections'])).toBeUndefined()
    expect(countyLineDomain(undefined)).toBeUndefined()
  })

  it('is a box with a slash in it, and only that', () => {
    expect(isCountyLine('COOK/DUPG')).toBe(true)
    expect(isCountyLine('COOK/')).toBe(true)
    expect(isCountyLine('COOK')).toBe(false)
    expect(isCountyLine('St. Clair')).toBe(false)
  })
})

describe('reading a county line', () => {
  it('gives the counties, in order, from codes or full names', () => {
    expect(readCountyLine('il_counties', 'COOK/DUPG')).toEqual({ ok: true, counties: ['COOK', 'DUPG'] })
    expect(readCountyLine('il_counties', ' cook / DuPage / st clair / LEE ')).toEqual({
      ok: true,
      counties: ['COOK', 'DUPG', 'SCLA', 'LEE'],
    })
  })

  it('names the first part that is not a county, and never completes a prefix', () => {
    expect(readCountyLine('il_counties', 'COOK/DUP')).toEqual({ ok: false, why: 'unknown', part: 'DUP' })
    expect(readCountyLine('il_counties', 'COOK/IN')).toEqual({ ok: false, why: 'unknown', part: 'IN' })
    // "Ma" could be seven counties, so it is none of them.
    expect(readCountyLine('il_counties', 'MA/COOK')).toEqual({ ok: false, why: 'unknown', part: 'MA' })
  })

  it('refuses an empty part, a repeated county and a fifth county', () => {
    expect(readCountyLine('il_counties', 'COOK/')).toEqual({ ok: false, why: 'incomplete', max: 4 })
    expect(readCountyLine('il_counties', 'COOK//DUPG')).toEqual({ ok: false, why: 'incomplete', max: 4 })
    // A name and its code are the same county.
    expect(readCountyLine('il_counties', 'COOK/Cook')).toEqual({ ok: false, why: 'repeated', part: 'COOK' })
    expect(readCountyLine('il_counties', 'COOK/DUPG/KANE/WILL/LAKE')).toEqual({ ok: false, why: 'tooMany', max: 4 })
    // …and four is allowed.
    expect(readCountyLine('il_counties', 'COOK/DUPG/KANE/WILL').ok).toBe(true)
  })
})

describe('editing a county line', () => {
  it('writes each county as its code and keeps what is not a county as typed', () => {
    expect(resolveCountyLine('il_counties', 'Cook/DuPage')).toBe('COOK/DUPG')
    expect(resolveCountyLine('il_counties', 'cook/xyz/')).toBe('COOK/XYZ/')
  })

  it('completes the part after the last slash', () => {
    expect(lastCountyPart('COOK/DU')).toBe('DU')
    expect(lastCountyPart('COOK/')).toBe('')
    expect(withLastCountyPart('COOK/DU', 'DUPG')).toBe('COOK/DUPG')
    expect(withLastCountyPart('COOK/DUPG/KA', 'KANE')).toBe('COOK/DUPG/KANE')
  })
})

// `+` joins a county line exactly as `/` does: some loggers take `COOK+WILL`, so an operator
// used to one types that. Each verdict is asserted BY VALUE for both marks, so the two cannot
// agree by both being wrong.
describe('a county line joined by +', () => {
  it('is a county line, and no county on the chart has either mark in its code or name', () => {
    expect(isCountyLine('COOK+WILL')).toBe(true)
    expect(isCountyLine('COOK+')).toBe(true)
    expect(ILQP_COUNTIES.filter((c) => /[/+]/.test(c.code + c.name))).toEqual([])
  })

  it('reads to the counties the slash gives: spaces, lower case, names, three or four, both marks', () => {
    for (const [typed, counties] of [
      ['COOK+WILL', ['COOK', 'WILL']],
      ['COOK/WILL', ['COOK', 'WILL']],
      [' cook + will ', ['COOK', 'WILL']],
      ['Cook+DuPage+St Clair', ['COOK', 'DUPG', 'SCLA']],
      ['COOK/DUPG+KANE+WILL', ['COOK', 'DUPG', 'KANE', 'WILL']],
    ] as const) {
      expect(readCountyLine('il_counties', typed), typed).toEqual({ ok: true, counties })
    }
  })

  it('refuses a stray +, a repeat, a fifth county and a part that is not a county, as the slash does', () => {
    for (const [plus, slash, verdict] of [
      ['COOK+', 'COOK/', { ok: false, why: 'incomplete', max: 4 }],
      ['+COOK', '/COOK', { ok: false, why: 'incomplete', max: 4 }],
      ['COOK++WILL', 'COOK//WILL', { ok: false, why: 'incomplete', max: 4 }],
      ['COOK+Cook', 'COOK/Cook', { ok: false, why: 'repeated', part: 'COOK' }],
      ['COOK+DUPG+KANE+WILL+LAKE', 'COOK/DUPG/KANE/WILL/LAKE', { ok: false, why: 'tooMany', max: 4 }],
      ['COOK+XYZ', 'COOK/XYZ', { ok: false, why: 'unknown', part: 'XYZ' }],
    ] as const) {
      expect(readCountyLine('il_counties', plus), plus).toEqual(verdict)
      expect(readCountyLine('il_counties', slash), slash).toEqual(verdict)
    }
  })

  it('is written with / once resolved, and completes the part after the last mark', () => {
    expect(resolveCountyLine('il_counties', 'Cook+DuPage')).toBe('COOK/DUPG')
    expect(resolveCountyLine('il_counties', 'cook+xyz+')).toBe('COOK/XYZ/')
    expect(lastCountyPart('COOK+DU')).toBe('DU')
    expect(lastCountyPart('COOK+DUPG/')).toBe('')
    expect(withLastCountyPart('COOK+DU', 'DUPG')).toBe('COOK+DUPG')
    expect(withLastCountyPart('COOK/DUPG+KA', 'KANE')).toBe('COOK/DUPG+KANE')
  })
})
