import { describe, expect, it } from 'vitest'
import { cardSave, draftOf, fieldsOf, storedEdit, type CardDraft } from './contactCard'
import type { LoggedQso } from '../types'

const Q: LoggedQso = {
  id: 'a', call: 'W1AW', grid: null, band: '20m', freqMhz: 14.0251, mode: 'CW', rstSent: '599', rstRcvd: '579',
  whenUnix: Date.UTC(2026, 9, 26, 23, 58, 0) / 1000, confirmed: false, awardConfirmed: false,
  ota: { theirProgram: 'POTA', theirRef: 'US-0001', myProgram: null, myRef: null, iota: null },
  extra: [['CNTY', 'CT,HARTFORD']],
}
const saved = (over: Partial<CardDraft>, q: LoggedQso = Q) => {
  const start = draftOf(q)
  return cardSave(q, start, { ...start, ...over }, fieldsOf(q))
}

describe('the contact card’s save', () => {
  it('left alone, is the stored contact exactly, with no field change', () => {
    expect(saved({})).toEqual({ ok: true, edit: storedEdit(Q), fields: [] })
  })

  it('an end time earlier in the day than the start is the next day (the rollover)', () => {
    const r = saved({ timeOff: '00:03' })
    expect(r.ok && r.edit.timeOffUnix).toBe(Q.whenUnix + 5 * 60)
  })

  it('a date-only contact keeps its instant when only another box changes', () => {
    const q = { ...Q, timeKnown: false, whenUnix: Date.UTC(2020, 0, 2) / 1000 }
    expect(draftOf(q).timeOn).toBe('')
    const r = saved({ name: 'Hiram' }, q)
    expect(r.ok && r.edit.whenUnix).toBe(q.whenUnix)
  })

  it('a changed programme rides with the stored reference', () => {
    const r = saved({ theirProgram: 'WWFF' })
    expect(r.ok && r.edit.ota).toEqual({ theirProgram: 'WWFF', theirRef: 'US-0001', myProgram: null, myRef: null, iota: null })
  })

  it('emptying both park boxes is refused: an empty park is "leave it" to the log', () => {
    expect(saved({ theirRef: '' })).toEqual({ ok: false, error: { code: 'cannotClear', field: 'parks' } })
  })

  it('a frequency or a time that is not one is refused, never guessed', () => {
    expect(saved({ freq: '14.0x' })).toEqual({ ok: false, error: { code: 'freq' } })
    expect(saved({ timeOn: '25:00' })).toEqual({ ok: false, error: { code: 'when' } })
  })

  it('an added field the contact already holds is refused', () => {
    const start = draftOf(Q)
    const fields = [...fieldsOf(Q), { tag: 'cnty', orig: null, value: 'x', removed: false }]
    expect(cardSave(Q, start, start, fields)).toEqual({ ok: false, error: { code: 'twice', tag: 'CNTY' } })
  })
})
