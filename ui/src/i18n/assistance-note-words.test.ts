// THE CONTEST-ASSISTANCE NOTE NAMES THE CW DECODER, AND CALLS NO DECODES EXEMPT.
//
// Its source lines said "AI CW decoder", but an unassisted entry stops the classic decoder too, and
// CQ WW's rule VIII.2 names "a CW decoder": the source is the CW decoder, on whenever any CW
// decoding runs. And its line on the station's own decodes said they are not assistance under
// either ruleset, while both rulesets count any technology that identifies a callsign. That line now
// names the digital decodes that keep feeding the Needed board in an unassisted entry.
//
// Read off all five catalogs and the pt-BR kit's rows: a language left on the AI-only name or on the
// old line, or a kit row left on the old English, fails here.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { EN } from './en'
import { DE } from './de'
import { ES } from './es'
import { FR } from './fr'
import { JA } from './ja'

type Catalog = Readonly<Record<string, unknown>>
/** Each language's name for the CW decoder, and the AI-only name it replaces. */
const LOCALES: Record<string, { cat: Catalog; decoder: string; aiOnly: RegExp }> = {
  en: { cat: EN, decoder: 'CW decoder', aiOnly: /AI CW decoder/ },
  de: { cat: DE, decoder: 'CW-Dekoder', aiOnly: /KI-CW/ },
  es: { cat: ES, decoder: 'decodificador de CW', aiOnly: /con IA/ },
  fr: { cat: FR, decoder: 'décodeur CW', aiOnly: /par IA/ },
  ja: { cat: JA, decoder: 'CWデコーダー', aiOnly: /AI ?CW/ },
}
const SOURCES = ['assist.sources.off', 'assist.sources.offSince', 'assist.sources.on', 'assist.sources.onSince'] as const
const OWN = 'assist.why.ownDecodes'

describe('the contest-assistance note', () => {
  it.each(Object.entries(LOCALES))('%s: the four source lines name the CW decoder, not the AI one alone', (_lang, l) => {
    for (const key of SOURCES) {
      const line = String(l.cat[key])
      expect(line, key).toContain(l.decoder)
      expect(line, key).not.toMatch(l.aiOnly)
    }
  })

  it.each(Object.entries(LOCALES))('%s: the own-decodes line names the digital modes it is about', (_lang, l) => {
    // FT8 and FT4 are invariant tokens, so every language names them; the old line named no mode.
    const line = String(l.cat[OWN])
    expect(line).toContain('FT8')
    expect(line).toContain('FT4')
  })

  it("the pt-BR kit's rows have the English as it now reads", () => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    for (const key of [...SOURCES, OWN] as const) {
      expect(csv, key).toContain(`"${key}","${String(EN[key]).replace(/"/g, '""')}"`)
    }
  })
})
