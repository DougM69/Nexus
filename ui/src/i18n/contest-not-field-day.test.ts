// "FIELD DAY" STOOD FOR THE CONTEST IN TWO MORE PLACES THAN THE RAIL'S MODE BADGE, and both show with
// any contest. The torn-off scoreboard window, with no contest running, said "Field Day isn't
// active."; the Remote page's logging hint said to log "Field Day contacts" at the shack, which is
// true of every contest's (an entry made there goes to the station's general logbook).
//
// Read off all five catalogs and the pt-BR kit's rows: neither string names Field Day in any language,
// each names the contest by that language's own word for it (the one the rail's Contest item uses),
// and the kit carries the catalog's English with no Field Day left to keep untranslated.
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
const KEYS = ['detached.fieldDay.inactive', 'remote.loggingHint'] as const

describe('the contest is not called Field Day', () => {
  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => KEYS.map((key) => [lang, key, cat] as const)))(
    '%s %s: names the contest, not Field Day',
    (_lang, key, cat) => {
      const text = String(cat[key])
      expect(text).not.toMatch(/field.?day|フィールド/i)
      // The language's word for a contest: the rail item's label, as a sentence writes it.
      const word = String(cat['nav.fieldDay.label']).toLowerCase()
      expect(text.toLowerCase(), word).toContain(word)
    },
  )

  it.each(KEYS)('the pt-BR kit has the new English of %s, and no Field Day to keep', (key) => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    const row = csv.split('\n').find((l) => l.includes(`,"${key}",`))
    expect(row, key).toContain(`"${key}","${EN[key]}","",`)
    expect(row).not.toMatch(/Field Day/)
  })
})
