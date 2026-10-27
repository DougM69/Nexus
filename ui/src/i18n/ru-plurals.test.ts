// Russian plurals: one / few / many, and why `one` must show the number.
//
// Russian picks a noun form by the count's last digits, not by "is it 1": 1, 21, 31, 101 take
// the singular (`one`: «21 связь»), 2–4, 22–24 the genitive singular (`few`: «22 связи») and
// 0, 5–20, 25–30, 111 the genitive plural (`many`: «25 связей»). `Intl.PluralRules('ru')` already
// selects those categories and `selectPluralForm` already reads them, so nothing in the runtime is
// Russian-specific. What can go wrong is the CATALOG:
//   • a Russian plural with only `one` and `other` reads «2 связей» (2–4 fall to `other`), and
//   • a `one` sentence written without {{count}} («Скрыт один спот») is shown for 21 and 101,
//     which then reads as ONE.
// So every Russian plural carries all three integer forms, and its `one` shows {{count}}
// wherever the English plural shows the count at all.

import { afterEach, describe, expect, it } from 'vitest'
import { EN, installCatalog, selectPluralForm, setLocale, t, type MessageKey } from './index'
import { RU } from './ru'
import type { PluralForms } from './types'

afterEach(() => setLocale('en'))

const forms: PluralForms = { one: 'one', few: 'few', many: 'many', other: 'other' }

describe('Russian plural selection', () => {
  it.each([
    [1, 'one'],
    [2, 'few'],
    [5, 'many'],
    [11, 'many'],
    [21, 'one'],
    [22, 'few'],
    [25, 'many'],
    [101, 'one'],
    [0, 'many'],
    [111, 'many'],
    [1.5, 'other'],
  ])('%s selects %s', (n, want) => {
    expect(selectPluralForm('ru', n, forms)).toBe(want)
  })

  it('leaves English and the shipped languages exactly as they were', () => {
    // English, German, Spanish and French have one + other only; Japanese has other only. The
    // same counts that split three ways in Russian must still split one / other there.
    for (const loc of ['en', 'de', 'es', 'fr'])
      for (const n of [1, 2, 5, 11, 21, 22, 25, 101])
        expect(selectPluralForm(loc, n, forms), `${loc} ${n}`).toBe(n === 1 ? 'one' : 'other')
    for (const n of [1, 2, 21]) expect(selectPluralForm('ja', n, forms)).toBe('other')
  })

  it('renders a real Russian catalog entry correctly for 1, 2, 5, 11, 21, 22, 25 and 101', () => {
    installCatalog('ru', RU)
    setLocale('ru')
    // The sentence that read «1 спот скрыт» for 21 hidden spots before the plural forms were whole.
    const key = 'spots.empty.worked' as MessageKey
    const want: Record<number, string> = {}
    const entry = RU[key] as PluralForms
    for (const [n, f] of [
      [1, 'one'],
      [2, 'few'],
      [5, 'many'],
      [11, 'many'],
      [21, 'one'],
      [22, 'few'],
      [25, 'many'],
      [101, 'one'],
    ] as const)
      want[n] = entry[f]!.replace('{{count}}', String(n))
    for (const n of [1, 2, 5, 11, 21, 22, 25, 101]) expect(t(key, { count: n })).toBe(want[n])
    expect(t(key, { count: 21 })).toMatch(/^21 спот скрыт/)
    expect(t(key, { count: 22 })).toMatch(/^22 спота скрыто/)
    expect(t(key, { count: 25 })).toMatch(/^25 спотов скрыто/)
    // Control: the three forms really are three different sentences, so the test above could fail.
    expect(new Set([entry.one, entry.few, entry.many]).size).toBe(3)
  })
})

/** What is wrong with a Russian catalog's plurals, as messages. */
function pluralProblems(cat: Record<string, unknown>): string[] {
  const out: string[] = []
  for (const [key, en] of Object.entries(EN)) {
    if (typeof en === 'string') continue
    const ru = cat[key]
    if (ru === undefined) continue // completeness is placeholders.test.ts's job
    if (typeof ru === 'string') {
      out.push(`${key}: a plain string where English has plural forms`)
      continue
    }
    const r = ru as PluralForms
    for (const f of ['one', 'few', 'many', 'other'] as const)
      if (!r[f]?.trim()) out.push(`${key}: no ${f} form`)
    if ((en as PluralForms).other.includes('{{count}}') && r.one && !r.one.includes('{{count}}'))
      out.push(`${key}: one form without {{count}} — it is also shown for 21, 31 and 101`)
  }
  return out
}

describe('the Russian catalog carries every plural form it needs', () => {
  it('every plural has one, few, many and other, and one shows the count', () => {
    expect(pluralProblems(RU as Record<string, unknown>)).toEqual([])
  })

  it('the check fires (positive control)', () => {
    const key = Object.keys(EN).find((k) => {
      const v = EN[k as MessageKey]
      return typeof v !== 'string' && v.other.includes('{{count}}')
    })!
    expect(key, 'English has a counted plural to break').toBeTruthy()
    const twoForms = { [key]: { one: '{{count}} x', other: '{{count}} y' } }
    expect(pluralProblems(twoForms)).toEqual([`${key}: no few form`, `${key}: no many form`])
    const noCount = { [key]: { one: 'один', few: '{{count}} b', many: '{{count}} c', other: '{{count}} c' } }
    expect(pluralProblems(noCount)[0]).toContain('one form without {{count}}')
  })
})
