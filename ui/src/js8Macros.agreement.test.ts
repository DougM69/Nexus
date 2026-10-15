// ONE MACRO TABLE, TWO LANGUAGES. The compose box fills and sends with JS8Call's macros replaced
// here (js8Macros.ts); the station replaces them in the CQ and in its INFO? and STATUS? replies
// (crates/js8/src/proto/macros.rs). A saved message and the CQ the station keys must read alike,
// so both are held to one table of cases that lives with the Rust crate
// (crates/js8/tests/fixtures/js8-macros.json) and is read from there — not copied — by this test and
// by `the_macros_agree_with_the_desktop_on_every_case_in_the_shared_table`.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { JS8_MACRO_TOKENS, js8MacroValues, replaceJs8Macros, type Js8MacroInput } from './js8Macros'

interface Case {
  name: string
  input: Js8MacroInput
  text: string
  prune: boolean
  expect: string
}

const table = JSON.parse(
  readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../crates/js8/tests/fixtures/js8-macros.json'), 'utf8'),
) as { cases: Case[] }

describe('the compose box and the station replace JS8Call macros alike', () => {
  it('the shared table is there, and names every token both ways', () => {
    expect(table.cases.length).toBeGreaterThanOrEqual(30)
    for (const token of JS8_MACRO_TOKENS) {
      expect(table.cases.some((c) => c.text.toUpperCase().includes(token)), token).toBe(true)
    }
    expect(table.cases.some((c) => c.prune)).toBe(true)
    expect(table.cases.some((c) => !c.prune)).toBe(true)
  })

  it('the desktop agrees with the table on every case', () => {
    for (const c of table.cases) {
      expect(replaceJs8Macros(c.text, js8MacroValues(c.input), c.prune), c.name).toBe(c.expect)
    }
  })
})
