// ONE FILE, TWO READERS. The operator's call-history file fills the contest strip's boxes
// (`historyFill`, here) and predicts the band map's multipliers (`exchanges_for`,
// crates/tempo-app/src/call_history.rs). A station the strip would fill as COOK must be one the band
// map predicts as COOK, so both are held to one file and one table of what it says, which live with
// the Rust crate (crates/tempo-app/tests/fixtures/call-history.txt and call-history-expected.json)
// and are read from there — not copied — by this test and by
// `the_shared_call_history_file_reads_as_its_table_says`. The strip never reads the text: the
// station parses it and sends the entries, and that test holds the parse to the table's entries.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import type { CallHistoryFile, ContestFieldSpec } from '../types'
import { historyFill } from './callHistoryFill'

interface Case {
  event: string
  call: string
  fill: Record<string, string>
  predict: string[][]
  differs?: string
}

const table = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../../crates/tempo-app/tests/fixtures/call-history-expected.json'),
    'utf8',
  ),
) as { entries: CallHistoryFile['entries']; cases: Case[] }

// The receive slots as the engine really sends them (`FdFieldDto`), per contest.
const SLOTS: Record<string, ContestFieldSpec[]> = {
  ilqp: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  arrlfd: [
    { key: 'CLASS', kind: 'pattern', required: true, adif: 'CLASS' },
    { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections', domains: ['fd_sections'], adif: 'ARRL_SECT' },
  ],
  cqww_cw: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
  ],
}

describe('the strip fills what the shared call-history table says', () => {
  it('the shared table is there, with both answers in every contest', () => {
    expect(Object.keys(table.entries).length).toBeGreaterThanOrEqual(10)
    for (const event of Object.keys(SLOTS))
      for (const fills of [true, false])
        expect(
          table.cases.some((c) => c.event === event && (Object.keys(c.fill).length > 0) === fills),
          `${event} ${fills}`,
        ).toBe(true)
  })

  it('the strip agrees with the table on every case', () => {
    const wrong = table.cases
      .map((c) => {
        const file: CallHistoryFile = { contest: c.event, fileName: 'call-history.txt', entries: table.entries }
        const got = historyFill(c.call, SLOTS[c.event], file, c.event)
        return JSON.stringify(got) === JSON.stringify(c.fill) ? '' : `${c.event} ${c.call}: ${JSON.stringify(got)}`
      })
      .filter((w) => w !== '')
    expect(wrong).toEqual([])
  })

  it('the fill and the prediction differ only where the table says why', () => {
    for (const c of table.cases) {
      const asPredict = Object.keys(c.fill).length ? [Object.entries(c.fill).map(([k, v]) => `${k}=${v}`)] : []
      expect(JSON.stringify(asPredict) !== JSON.stringify(c.predict), `${c.event} ${c.call}`).toBe(c.differs !== undefined)
    }
  })
})
