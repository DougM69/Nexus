// ---------------------------------------------------------------------------
// WHAT AN EXCHANGE SLOT IS CALLED ON SCREEN.
//
// A slot id (`CLASS`, `ZN`, `QTH`) is the rules file's INVARIANT TOKEN — never translated, and
// not what a RECEIVED box is called when anything says more: an operator rehearsing the Illinois
// QSO Party could not find the county box because it read "QTH". A received box is captioned by
// what it holds for this station's role (`receivedCaption`), in the catalog's words. The entry
// strip, the contest log table and the edit of a logged contact read that one function, so a
// box, its column and its edit cannot be called different things.
//
// …AND WHAT A BOX MEANS, AND WHETHER IT CAN BE LOGGED (`fdSlotValue`, `fdFieldOk`, `fdVerdict`):
// the entry strip and the edit of a logged contact read the same three, so an edit cannot write
// a value the strip would refuse to log.
// ---------------------------------------------------------------------------

import { t, type MessageKey } from '../i18n'
import type { ContestFieldSpec } from '../types'
import { inDomain, resolveDomainValue } from './contestDomains'
import { isCountyLine, readCountyLine, resolveCountyLine } from './countyLine'

/** The catalog keys for one slot's caption and tooltip, for the slots only an id can name. Field
 *  Day's two slots keep the captions and tooltips they shipped with. */
export const SLOT_TEXT: Record<string, { labelKey: MessageKey; titleKey: MessageKey }> = {
  CLASS: { labelKey: 'logEntry.fd.class.label', titleKey: 'logEntry.fd.class.title' },
  SECTION: { labelKey: 'logEntry.fd.section.label', titleKey: 'logEntry.fd.section.title' },
  // CQ WW's zone slot.
  ZN: { labelKey: 'logEntry.fd.zone.label', titleKey: 'logEntry.fd.zone.title' },
  // Sweepstakes' precedence letter and check: the rules give each a pattern and no list, so
  // nothing but the id says what the box holds.
  PREC: { labelKey: 'logEntry.fd.precedence.label', titleKey: 'logEntry.fd.precedence.title' },
  CK: { labelKey: 'logEntry.fd.check.label', titleKey: 'logEntry.fd.check.title' },
}

/** The operator-facing caption for a slot, by id alone: a SENT slot's box, and the slots in
 *  `SLOT_TEXT`. A received box is captioned by `receivedCaption`. */
export function slotCaption(key: string): string {
  const m = SLOT_TEXT[key]
  return m ? t(m.labelKey) : key
}

/** ⭐ A county list, by rules-file domain id: the state whose party it is, and the example an empty
 *  box shows (the state's most populous county, the one most likely worked). LISTED rather than
 *  read off the id's spelling, as `COUNTY_LINE_MAX` is: a list missing here leaves its box
 *  captioned by slot id, which `LogEntry.exchangeLabels.test.tsx` refuses for every seeded
 *  contest. */
const COUNTY_LISTS: Record<string, { state: string; example: string }> = {
  ca_counties: { state: 'CA', example: 'LANG' },
  il_counties: { state: 'IL', example: 'COOK' },
  ny_counties: { state: 'NY', example: 'KIN' },
  oh_counties: { state: 'OH', example: 'FRAN' },
  tn_counties: { state: 'TN', example: 'SHEL' },
  tx_counties: { state: 'TX', example: 'HARR' },
}

/** The lists of US states and Canadian provinces, and the ARRL/RAC section lists. */
const STATE_LISTS: ReadonlySet<string> = new Set(['cqp_states', 'cqww_rtty_qth', 'il_mults', 'ny_mults', 'oh_mults'])
const SECTION_LISTS: ReadonlySet<string> = new Set(['arrl_sections', 'fd_sections', 'ss_sections'])

/** The example an empty state or section box shows: a code in both lists, and Field Day's own. */
const STATE_OR_SECTION_EXAMPLE = 'WI'

/** The roles of a station OUTSIDE the state whose QSO party it works (`contestRoleWords`' ids).
 *  It works only that state's stations, so a county box holds that state's counties alone. */
const OUTSIDE_ROLES: ReadonlySet<string> = new Set(['out_of_state', 'w_ve', 'dx'])

const countyList = (spec: ContestFieldSpec): string | undefined => spec.domains?.find((d) => d in COUNTY_LISTS)

/** ⭐ **What a RECEIVED box is called, for this station's role** — by what it holds, first match:
 *
 *  1. the rules file's own label, when it declares one (the on-air word that introduces the
 *     field, `NAME` in "NAME ALEX"); no shipped slot has one;
 *  2. the catalog's words for the slots only an id can name (`SLOT_TEXT`);
 *  3. a county box: "IL county" for a station outside the state, since those are all it can
 *     work; inside it "County / State" when the box takes more than the county list (the states,
 *     provinces and countries everyone else sends, as a list or as free text), else "County";
 *  4. a state list, a section list; a serial, a grid, a call;
 *  5. otherwise the slot id, a token: `RST`, which hams read as it is. */
export function receivedCaption(spec: ContestFieldSpec, role: string | undefined): string {
  const own = spec.label?.trim()
  if (own) return own
  if (SLOT_TEXT[spec.key]) return slotCaption(spec.key)
  const county = countyList(spec)
  if (county) {
    if (role !== undefined && OUTSIDE_ROLES.has(role)) {
      return t('logEntry.fd.countyIn.label', { state: COUNTY_LISTS[county].state })
    }
    return spec.kind === 'oneOf' ? t('logEntry.fd.countyOrState.label') : t('logEntry.fd.county.label')
  }
  if (spec.domains?.some((d) => STATE_LISTS.has(d))) return t('logEntry.fd.state.label')
  if (spec.domains?.some((d) => SECTION_LISTS.has(d))) return t('logEntry.fd.section.label')
  if (spec.kind === 'serial') return t('logEntry.fd.serial.label')
  if (spec.kind === 'grid') return t('logEntry.fd.grid.label')
  if (spec.kind === 'call') return t('logEntry.fd.call.label')
  return spec.key
}

/** The example an EMPTY received box shows: a code from the list it draws on. An INVARIANT
 *  TOKEN, never a catalog string (the strip's `LOG_EXAMPLES` rule). `undefined` for a box with
 *  no list, whose caption says what goes there — the zone box among them, whose placeholder is
 *  the country file's hint for the call typed. */
export function slotExample(spec: ContestFieldSpec): string | undefined {
  const county = countyList(spec)
  if (county) return COUNTY_LISTS[county].example
  if (spec.domains?.some((d) => STATE_LISTS.has(d) || SECTION_LISTS.has(d))) return STATE_OR_SECTION_EXAMPLE
  return undefined
}

/** The tooltip for a slot's box, when the catalog has one. */
export function slotTitle(key: string): string | undefined {
  const m = SLOT_TEXT[key]
  return m ? t(m.titleKey) : undefined
}

/** Why this slot is not loggable yet — Field Day's own two sentences for Field Day's
 *  two slots, and a generic pair naming the field for anything else, by its box's caption for
 *  this station's `role`. */
export function fdVerdict(spec: ContestFieldSpec, raw: string, line?: string, role?: string): string {
  const v = raw.trim()
  // A COUNTY LINE says which part stops it, because one bad part refuses the whole line.
  if (line && isCountyLine(v)) {
    const read = readCountyLine(line, v)
    if (!read.ok) {
      if (read.why === 'unknown') return t('logEntry.countyLine.unknown', { value: read.part })
      if (read.why === 'repeated') return t('logEntry.countyLine.repeated', { value: read.part })
      if (read.why === 'tooMany') return t('logEntry.countyLine.tooMany', { max: read.max })
      return t('logEntry.countyLine.incomplete', { max: read.max })
    }
  }
  // ⚠️ Field Day's two sentences, byte for byte, INCLUDING the em-dash placeholder a
  // blank Section has always shown. "Section \"—\" isn't a known ARRL/RAC section" reads
  // oddly and is what shipped; a strip that started saying something better here would
  // be a change to what a Field Day operator reads, which this batch does not make.
  if (spec.key === 'CLASS' && v === '') return t('logEntry.fd.needClass')
  if (spec.key === 'SECTION') return t('logEntry.fd.badSection', { section: v || '—' })
  // A bounded number with a value in it: say the range, which is what fixes it.
  if (spec.kind === 'number' && v !== '' && spec.min != null && spec.max != null) {
    return t('logEntry.fd.badNumber', { field: receivedCaption(spec, role), min: spec.min, max: spec.max })
  }
  return v === ''
    ? t('logEntry.fd.needField', { field: receivedCaption(spec, role) })
    : t('logEntry.fd.badField', { field: receivedCaption(spec, role), value: v })
}

/** Is this slot's typed value good enough to log?
 *
 *  Byte-for-byte Field Day's shipped rule, generalised by KIND and by nothing else:
 *  an `enum` slot must be a member of its domain, anything else required must be
 *  non-blank. A kind this build has no matcher for gets the non-blank test and no more
 *  — never an approximated verdict (the `Pattern` rule).
 *
 *  An OPTIONAL slot may be blank, whatever its kind: CQ WW RTTY's QTH is sent by W/VE
 *  stations only, and a DX contact has none to type. A value that IS typed still has to
 *  be legal. A `number` slot carrying its bounds (a CQ zone, 1–40) must be a whole number
 *  inside them. */
/** ⭐ **What a box actually means** — the code, whether the operator typed the code or the
 *  NAME beside it in the contest's own list ("Cook" → COOK, "St. Clair" → SCLA).
 *
 *  A county exchange is four letters an operator has to know and the name is the part they
 *  do know, so both are accepted; a name that could mean several counties resolves to
 *  nothing and the box keeps exactly what was typed. It is ONE function because the
 *  verdict below, the suggestion list and the value that is logged must agree — a strip
 *  that shows a value good and logs a different one is the screen lying about what was
 *  written.
 *
 *  `line` is the county list this box takes a COUNTY LINE from (`features/countyLine`), when it
 *  takes one: a box holding `Cook/DuPage` then means `COOK/DUPG`, part by part. */
export function fdSlotValue(spec: ContestFieldSpec, raw: string, line?: string): string {
  const v = raw.trim().toUpperCase()
  if (line && isCountyLine(v)) return resolveCountyLine(line, v)
  return resolveDomainValue(spec.domains, v) ?? v
}

export function fdFieldOk(spec: ContestFieldSpec, raw: string, line?: string): boolean {
  const v = fdSlotValue(spec, raw, line)
  if (v === '') return !spec.required
  if (line && isCountyLine(v)) return readCountyLine(line, v).ok
  if (spec.kind === 'enum') return inDomain(spec.domain, v)
  if (spec.kind === 'number' && spec.min != null && spec.max != null) {
    return /^\d{1,3}$/.test(v) && Number(v) >= spec.min && Number(v) <= spec.max
  }
  return true
}
