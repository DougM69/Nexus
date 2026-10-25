import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { describe, it, expect } from 'vitest'
import { t } from '../i18n'
import { IMPORT_MAX_BYTES, droppedCount, importTooLarge, reportText, type ImportReport } from './logImport'

const REPORT: ImportReport = {
  format: 'cabrillo',
  template: 'ARRL-SS-CW',
  records: 2,
  contacts: 2,
  added: 1,
  duplicates: 1,
  updated: 0,
  newOnly: true,
  fields: [
    { field: 'BAND', occurrences: 2, stored: 2, normalized: 0, passthrough: 0, dropped: 0, reasons: [], examples: [] },
    { field: 'CALL', occurrences: 2, stored: 2, normalized: 0, passthrough: 0, dropped: 0, reasons: [], examples: [] },
    {
      field: 'FREQ', occurrences: 2, stored: 1, normalized: 0, passthrough: 0, dropped: 1,
      reasons: [{ reason: 'invalid', count: 1 }], examples: ['abc'],
    },
    { field: 'APP_X_Y', occurrences: 2, stored: 0, normalized: 0, passthrough: 2, dropped: 0, reasons: [], examples: [] },
  ],
  unmapped: [{ what: 'PREC', side: 'rcvd', count: 2, reason: 'exchangeText' }],
  notes: [{ kind: 'excludedQso', count: 1, examples: [{ kind: 'excludedQso', at: 7, text: 'K8MM 2024-11-02 2100' }] }],
  accounted: true,
}

describe('the no-loss report as a text file', () => {
  const text = reportText(REPORT, 'ss.log', new Date(Date.UTC(2026, 9, 25, 12, 0, 0)))

  it('names every field of the file, with each of its four counts', () => {
    for (const f of REPORT.fields) {
      expect(text).toContain(
        `${f.field}: ${f.occurrences} = ${f.stored} ${t('logbook.import.report.col.stored')} + ${f.normalized} ${t(
          'logbook.import.report.col.normalized',
        )} + ${f.passthrough} ${t('logbook.import.report.col.passthrough')} + ${f.dropped} ${t('logbook.import.report.col.dropped')}`,
      )
    }
  })

  it('says why a value was dropped, and shows it', () => {
    expect(text).toContain(`${t('logbook.import.report.col.dropped')} 1: ${t('logbook.import.reason.invalid')}`)
    expect(text).toContain('    abc')
  })

  it('lists what has no field of a contact, and the problems, by line', () => {
    expect(text).toContain(`${t('logbook.import.side.rcvd', { what: 'PREC' })} ×2: ${t('logbook.import.unmapped.exchangeText')}`)
    expect(text).toContain(`${t('logbook.import.at.line', { n: 7 })}: K8MM 2024-11-02 2100`)
  })

  it('carries the file, the time, the template and the new-only note', () => {
    expect(text).toContain(`${t('logbook.import.report.file')}: ss.log`)
    expect(text).toContain('2026-10-25 12:00:00Z')
    expect(text).toContain(`${t('logbook.import.report.template')}: ARRL-SS-CW`)
    expect(text).toContain(t('logbook.import.report.newOnly'))
    expect(text).toContain(t('logbook.import.report.accounted'))
    expect(droppedCount(REPORT)).toBe(1)
  })
})

describe('a file too large to import', () => {
  it('is refused with its size and the limit, before it is read', () => {
    expect(importTooLarge(IMPORT_MAX_BYTES)).toBeNull()
    const why = importTooLarge(IMPORT_MAX_BYTES + 500_001)
    expect(why).toBe(t('logbook.import.tooLarge', { size: '200.5', max: 200 }))
    expect(why).toContain('200.5')
  })

  it('is the engine’s own limit', () => {
    const rust = readFileSync(
      resolve(dirname(fileURLToPath(import.meta.url)), '../../../crates/tempo-core/src/logbook/import.rs'),
      'utf8',
    )
    const engine = rust.match(/pub const MAX_BYTES: usize = ([0-9_]+);/)
    expect(engine?.[1].replace(/_/g, '')).toBe(String(IMPORT_MAX_BYTES))
  })
})
