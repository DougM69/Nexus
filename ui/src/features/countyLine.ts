// ---------------------------------------------------------------------------
// ⭐ THE COUNTY LINE — one contact with a station on the border of two to four counties, which
// the sponsor counts as one contact PER COUNTY. The Illinois QSO Party's 2026 rules:
// "Contacts with/by stations at the border of 2/3/4 counties count as 2/3/4 counties and 2/3/4
// QSOs."
//
// The station on the line sends its counties joined by `/` (`COOK/DUPG`), and that is how N1MM
// Logger+ takes them in its exchange box, so the entry strip takes them the same way and logs
// one row per county (`contestLogManualRows`): each an ordinary contact with its own county,
// all at one time, band and mode. N3FJP's loggers take the line joined by `+` (`COOK+DUPG`), so
// an operator coming from one types that, and the strip reads it as the same line.
//
// ⚠️ EVERY PART IS JUDGED AS A SINGLE COUNTY BOX JUDGES ONE, and against the county list alone.
// A part is a county code or a county's full name (`resolveDomainValue`), never a prefix of
// one, so `DUP` left unpicked is not DuPage and nothing is guessed onto the air. A state or a
// country is not half of a county line. One bad part refuses the whole line: logging the good
// half of `COOK/XYZ` would credit a contact the operator did not mean to log alone.
//
// ZERO IPC, like the rest of the strip's while-typing verdict: the county list is the mirror in
// `ilqpQth.ts`, read through `contestDomains.ts`.
// ---------------------------------------------------------------------------

import { getLocale } from '../i18n'
import { resolveDomainValue } from './contestDomains'

/** The county lists whose sponsor counts a county-line contact once per county, and the most
 *  counties one contact may name there. ILQP is the one party that has been checked: its 2026
 *  rules say "2/3/4 counties". A list missing here keeps one county per contact. */
const COUNTY_LINE_MAX: Record<string, number> = {
  il_counties: 4,
}

/** The county list a slot drawing on `domains` takes a county line from, or `undefined` when the
 *  slot takes one value per contact. */
export function countyLineDomain(domains: string[] | undefined): string | undefined {
  return domains?.find((d) => d in COUNTY_LINE_MAX)
}

/** The most counties one contact may name in `domain`'s county line. */
export function countyLineMax(domain: string): number {
  return COUNTY_LINE_MAX[domain] ?? 1
}

/** What joins a line's counties: `/` or `+`, alike and in any mix. */
const JOIN = /[/+]/

/** Does this box hold a county line rather than one value? A `/` or a `+` is the whole test: no
 *  county code and no county name contains either. */
export function isCountyLine(raw: string): boolean {
  return JOIN.test(raw)
}

const parts = (raw: string): string[] => raw.split(JOIN).map((p) => p.trim().toUpperCase())

/** What a county line says, part by part — the counties to log, or the first reason it cannot
 *  be logged. The reasons come in the order an operator fixes them: an empty part, a part that
 *  is not a county, a county named twice, and only then too many counties. */
export type CountyLine =
  | { ok: true; counties: string[] }
  | { ok: false; why: 'incomplete'; max: number }
  | { ok: false; why: 'unknown'; part: string }
  | { ok: false; why: 'repeated'; part: string }
  | { ok: false; why: 'tooMany'; max: number }

/** Read the county line in `raw` against `domain`'s county list. */
export function readCountyLine(domain: string, raw: string): CountyLine {
  const max = countyLineMax(domain)
  const typed = parts(raw)
  if (typed.some((p) => p === '')) return { ok: false, why: 'incomplete', max }
  const counties: string[] = []
  for (const p of typed) {
    const code = resolveDomainValue([domain], p)
    if (code === undefined) return { ok: false, why: 'unknown', part: p }
    if (counties.includes(code)) return { ok: false, why: 'repeated', part: code }
    counties.push(code)
  }
  if (counties.length > max) return { ok: false, why: 'tooMany', max }
  return { ok: true, counties }
}

/** The box with every part that names a county written as its code — `Cook/DuPage` →
 *  `COOK/DUPG` — and every other part kept as typed, so the verdict can name it. The parts are
 *  joined by `/` whichever mark joined them as typed: `Cook+DuPage` is `COOK/DUPG` too. */
export function resolveCountyLine(domain: string, raw: string): string {
  return parts(raw)
    .map((p) => resolveDomainValue([domain], p) ?? p)
    .join('/')
}

/** The part the type-ahead completes: the text after the last `/` or `+`. */
export function lastCountyPart(raw: string): string {
  return raw.split(JOIN).pop() ?? ''
}

/** The box with its last part replaced by `code`, the county picked from the list. */
export function withLastCountyPart(raw: string, code: string): string {
  return raw.slice(0, raw.length - lastCountyPart(raw).length) + code
}

/** `Intl.ListFormat`, where the runtime has it (every webview Nexus ships in does). */
const ListFormat = (
  Intl as unknown as {
    ListFormat?: new (locale: string, o: { style: string; type: string }) => { format(items: string[]): string }
  }
).ListFormat

/** Counties as a sentence names them, `COOK and WILL`, joined as the interface's language joins
 *  a list. The codes are invariant tokens. */
export function countyList(codes: string[]): string {
  return ListFormat ? new ListFormat(getLocale(), { style: 'long', type: 'conjunction' }).format(codes) : codes.join(', ')
}

/** Codes offered as a choice, `WASH or WA`, joined as the interface's language joins one. Never
 *  with `/`, which in a county box joins a county line. */
export function codeChoice(codes: string[]): string {
  return ListFormat ? new ListFormat(getLocale(), { style: 'long', type: 'disjunction' }).format(codes) : codes.join(', ')
}
