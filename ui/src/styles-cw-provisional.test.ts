// THE AI CW DECODER'S PROVISIONAL LETTERS. The unconfirmed tail of the copy is drawn in
// --cw-unconfirmed until a later pass confirms it, then in --text like the rest. Resolved through
// the cascade in every theme and palette, so a theme that forgot the token, or one where the
// provisional ink is unreadable or reads the same as confirmed copy, goes red.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { MODES, contrast, expandWith, parseHex, parseRules, rootTokensFrom, toRgb } from './cssCascade'

const CSS = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  (m) => m.replace(/[^\n]/g, ' '),
)
const RULES = parseRules(CSS)

describe('provisional AI CW letters', () => {
  it('paint in the provisional token', () => {
    const rule = [...RULES].reverse().find((r) => r.selector === '.cw-decode-unconfirmed')
    expect(rule?.decls.find((d) => d.prop === 'color')?.value.trim()).toBe('var(--cw-unconfirmed)')
  })

  for (const mode of MODES) {
    it(`${mode}: readable on the panel, and plainly dimmer than confirmed copy`, () => {
      const tokens = rootTokensFrom(RULES, mode)
      expect(tokens.has('--cw-unconfirmed'), 'the theme defines the token').toBe(true)
      const page = parseHex(expandWith(tokens, 'var(--bg)'))!
      const rgb = (v: string) => toRgb(expandWith(tokens, v), page)!
      const panel = rgb('var(--panel)')
      const provisional = rgb('var(--cw-unconfirmed)')
      const confirmed = rgb('var(--text)')
      expect(contrast(provisional, panel)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(confirmed, panel) - contrast(provisional, panel)).toBeGreaterThan(3)
    })
  }
})
