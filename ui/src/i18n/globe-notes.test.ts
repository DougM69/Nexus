// THE 3-D GLOBES' NOTES. Where a computer cannot make the WebGL2 context a globe is drawn with, or a globe fails
// once drawn, its place shows a note (components/GlobeGuard.tsx): the Logbook's band says the log is not
// affected, and Connect's map cell sends the operator to the maps that draw everywhere. Each note is in every
// catalog, WebGL2 stays the product name it is, Connect's names the map picker's own choices in each language,
// and the pt-BR kit's rows have the English the app shows.
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
const KEYS = ['logbook.globe.noWebgl2', 'logbook.globe.failed', 'globe.failed'] as const

describe("the 3-D globes' notes", () => {
  it.each(Object.entries(CATALOGS).flatMap(([lang, cat]) => KEYS.map((key) => [lang, key, cat] as const)))(
    '%s %s: is there',
    (_lang, key, cat) => {
      expect(typeof cat[key] === 'string' && (cat[key] as string).trim().length > 0, String(cat[key])).toBe(true)
    },
  )

  it.each(Object.entries(CATALOGS))('%s: the Logbook note names WebGL2 as it is', (_lang, cat) => {
    expect(String(cat['logbook.globe.noWebgl2'])).toContain('WebGL2')
  })

  it.each(Object.entries(CATALOGS))("%s: Connect's note names the map picker's own choices", (_lang, cat) => {
    const note = String(cat['globe.failed'])
    for (const choice of ['map.projection.globe.label', 'map.projection.world.label', 'map.projection.beam.label'])
      expect(note, choice).toContain(String(cat[choice]))
  })

  it.each(KEYS)("the pt-BR kit's %s row has the English the app shows", (key) => {
    const csv = readFileSync(
      fileURLToPath(new URL('../../../translations/pt-BR/nexus-ptbr-translation.csv', import.meta.url)),
      'utf8',
    )
    const english = String(CATALOGS.en[key]).replace(/"/g, '""')
    expect(csv).toContain(`"${key}","${english}"`)
  })
})
