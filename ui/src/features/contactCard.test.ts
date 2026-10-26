import { describe, expect, it } from 'vitest'
import { cardDirty, cardSave, draftOf, fieldChanges, fieldsOf, storedEdit, type CardDraft, type CardField } from './contactCard'
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

describe('fields kept as written: only what the operator touched is sent', () => {
  // Imported with values that are empty or blank: the ADIF reader keeps `<QSLMSG:0>` as written.
  const IMPORTED: LoggedQso = { ...Q, extra: [['APP_NEXUS_X', ''], ['APP_X_NOTE', ' '], ['CNTY', 'CT,HARTFORD'], ['QSLMSG', '']] }

  it('an untouched empty or blank field is not removed, and the card does not open changed', () => {
    const start = draftOf(IMPORTED)
    const fields = fieldsOf(IMPORTED)
    expect(fieldChanges(fields)).toEqual([])
    expect(cardDirty(start, start, fields)).toBe(false)
    expect(cardSave(IMPORTED, start, { ...start, name: 'Hiram' }, fields)).toEqual({
      ok: true,
      edit: { ...storedEdit(IMPORTED), name: 'Hiram' },
      fields: [],
    })
  })

  it('a field the operator emptied or removed goes as a removal', () => {
    const fields = fieldsOf(IMPORTED).map((f) =>
      f.tag === 'CNTY' ? { ...f, value: '  ' } : f.tag === 'QSLMSG' ? { ...f, removed: true } : f,
    )
    expect(fieldChanges(fields)).toEqual([
      { tag: 'CNTY', old: 'CT,HARTFORD', new: null },
      { tag: 'QSLMSG', old: '', new: null },
    ])
  })

  it('a typed value is trimmed, and one that trims to what it held is no change', () => {
    const fields: CardField[] = [
      { tag: 'CNTY', orig: 'CT,HARTFORD', value: ' CT,TOLLAND ', removed: false },
      { tag: 'CQZ', orig: '5', value: '5 ', removed: false },
      { tag: 'ITUZ', orig: null, value: ' 8\t', removed: false },
    ]
    expect(fieldChanges(fields)).toEqual([
      { tag: 'CNTY', old: 'CT,HARTFORD', new: 'CT,TOLLAND' },
      { tag: 'ITUZ', old: null, new: '8' },
    ])
  })
})

describe('a merged contest contact', () => {
  // The exchange holds the county and zone it was worked in; the record still carries a stale CNTY.
  const C: LoggedQso = { ...Q, contest: { contestId: 'OH-QSO-PARTY', adif: [['CNTY', 'OH,FRANKLIN'], ['CQZ', '5']] } }

  it('a field its contest exchange holds is refused, changed or added, and says why', () => {
    const start = draftOf(C)
    const changed = fieldsOf(C).map((f) => (f.tag === 'CNTY' ? { ...f, value: 'OH,DELAWARE' } : f))
    expect(cardSave(C, start, start, changed)).toEqual({ ok: false, error: { code: 'contest', tag: 'CNTY' } })
    const added = [...fieldsOf(C), { tag: 'CQZ', orig: null, value: '4', removed: false }]
    expect(cardSave(C, start, start, added)).toEqual({ ok: false, error: { code: 'contest', tag: 'CQZ' } })
    const other = [...fieldsOf(C), { tag: 'ITUZ', orig: null, value: '8', removed: false }]
    expect(cardSave(C, start, start, other)).toEqual({ ok: true, edit: storedEdit(C), fields: [{ tag: 'ITUZ', old: null, new: '8' }] })
  })
})
