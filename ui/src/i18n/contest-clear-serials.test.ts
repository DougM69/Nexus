// CLEAR TEST CONTACTS STARTS THE SERIAL NUMBERS AGAIN AT 001, and in a contest that sends a serial the
// Contest screen says so where the operator decides and where it answers: the button's question before
// the second click, the answer after it, and the button's tooltip. A contest that sends none (Field
// Day, a QSO party sending a county) has its own forms of the three, which never mention a serial.
//
// Read off all five catalogs and the pt-BR kit's rows: every serial form says 001, no other form does,
// none still says the number is not used again, each translation is its own language's, and the kit
// carries the catalog's English, with 001 kept as it is where it is said.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
const CATALOGS: Record<string, Catalog> = { en: EN, de: DE, es: ES, fr: FR, ja: JA }
const KEYS = ['contest.clear.confirm.serial', 'contest.clear.done.serial', 'contest.clear.title.serial'] as const
/** The same three for a contest that sends no serial. */
const PLAIN = ['contest.clear.confirm', 'contest.clear.done', 'contest.clear.title'] as const

/** What each catalog said while a cleared contact's serial stayed spent. */
const SPENT: Record<string, string> = {
  en: 'not used again',
  de: 'nicht wieder vergeben',
  es: 'no se vuelve a usar',
  fr: 'pas réutilisé',
  ja: '再使用されません',
}

/** A key's text in every form it has, as the kit names each: the string, or each plural form. */
const forms = (key: string, value: unknown): [string, string][] =>
  typeof value === 'string'
    ? [[key, value]]
    : Object.entries(value as Record<string, string>).map(([form, text]) => [`${key}::${form}`, text])

describe('Clear test contacts says the serial numbers start again at 001', () => {
  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => KEYS.map((key) => [lang, key, cat] as const)))(
    '%s %s',
    (lang, key, cat) => {
      const english = forms(key, EN[key]).map(([, text]) => text)
      for (const [form, text] of forms(key, cat[key])) {
        expect(text, form).toContain('001')
        expect(text, form).not.toContain(SPENT[lang])
        if (lang !== 'en') expect(english, `${lang} ${form} is translated`).not.toContain(text)
      }
    },
  )

  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => PLAIN.map((key) => [lang, key, cat] as const)))(
    '%s %s, for a contest that sends no serial, says nothing of one',
    (lang, key, cat) => {
      const english = forms(key, EN[key]).map(([, text]) => text)
      for (const [form, text] of forms(key, cat[key])) {
        expect(text, form).not.toContain('001')
        expect(text, form).not.toContain(SPENT[lang])
        if (lang !== 'en') expect(english, `${lang} ${form} is translated`).not.toContain(text)
      }
    },
  )

  it.each(PLAIN)('the pt-BR kit has the English of %s, with no 001 to keep', (key) => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    for (const [form, text] of forms(key, EN[key])) {
      const head = `"${form}","${text}","","`
      const row = csv.split('\n').find((l) => l.includes(head))
      expect(row, form).toBeDefined()
      const keep = (row ?? '').slice((row ?? '').indexOf(head) + head.length).split('"')[0]
      expect(keep.split(' | '), form).not.toContain('001')
    }
  })

  it.each(KEYS)('the pt-BR kit has the new English of %s, with 001 kept as it is', (key) => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    for (const [form, text] of forms(key, EN[key])) {
      const head = `"${form}","${text}","","`
      const row = csv.split('\n').find((l) => l.includes(head))
      expect(row, form).toBeDefined()
      const keep = (row ?? '').slice((row ?? '').indexOf(head) + head.length).split('"')[0]
      expect(keep.split(' | '), form).toContain('001')
    }
  })
})
