// THE STATIONS ON THIS NETWORK WINDOW NAMES WHAT IS ON ITS SCREEN. Its line after pairing said to press
// Stream, but a paired station's row leads with Operate from this computer and offers the stream only
// "instead", so every catalog's line names that button by the label the catalog gives it. And the pt-BR
// kit's rows carry the English the app shows: the line, the button it names, and the two strings that
// show this computer's key for the internet road (the key the station asks the operator to compare).
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

describe('the line after pairing', () => {
  it.each(Object.entries(CATALOGS))('%s: names the button a paired station’s row shows', (_lang, cat) => {
    const operate = String(cat['lanWindow.operate'])
    expect(operate.length).toBeGreaterThan(3)
    expect(String(cat['lanWindow.pairedNow'])).toContain(operate)
  })
})

describe('the pt-BR kit', () => {
  it.each(['lanWindow.pairedNow', 'lanWindow.operate', 'lanWindow.internet.waiting', 'lanWindow.internet.computerKey'])(
    "%s: its row has the English the app shows",
    (key) => {
      const csv = readFileSync(
        fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
        'utf8',
      )
      const english = String(CATALOGS.en[key]).replace(/"/g, '""')
      expect(english).not.toBe('undefined')
      expect(csv).toContain(`"${key}","${english}"`)
    },
  )
})
