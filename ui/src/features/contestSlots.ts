// ---------------------------------------------------------------------------
// WHAT AN EXCHANGE SLOT IS CALLED ON SCREEN.
//
// A slot id (`CLASS`, `ZN`, `QTH`) is the rules file's INVARIANT TOKEN — never translated.
// Its caption is PROSE and comes from the catalog, for the slots whose id is not a word an
// operator reads. Everything else shows its id, which is a token and correctly untranslated,
// rather than an invented English word. The entry strip and the contest log table read the
// same table, so a box and its column cannot be called two different things.
//
// …AND WHAT A BOX MEANS, AND WHETHER IT CAN BE LOGGED (`fdSlotValue`, `fdFieldOk`, `fdVerdict`):
// the entry strip and the edit of a logged contact read the same three, so an edit cannot write
// a value the strip would refuse to log.
// ---------------------------------------------------------------------------

import { t, type MessageKey } from '../i18n'
import type { ContestFieldSpec } from '../types'
import { inDomain, resolveDomainValue } from './contestDomains'
import { isCountyLine, readCountyLine, resolveCountyLine } from './countyLine'

/** The catalog keys for one slot's caption and tooltip. Field Day's two slots keep the
 *  captions and tooltips they shipped with. */
export const SLOT_TEXT: Record<string, { labelKey: MessageKey; titleKey: MessageKey }> = {
  CLASS: { labelKey: 'logEntry.fd.class.label', titleKey: 'logEntry.fd.class.title' },
  SECTION: { labelKey: 'logEntry.fd.section.label', titleKey: 'logEntry.fd.section.title' },
  // CQ WW's zone slot. `RST` and `QTH` are tokens hams read as they are, and keep their ids.
  ZN: { labelKey: 'logEntry.fd.zone.label', titleKey: 'logEntry.fd.zone.title' },
}

/** The operator-facing caption for a slot. */
export function slotCaption(key: string): string {
  const m = SLOT_TEXT[key]
  return m ? t(m.labelKey) : key
}

/** The tooltip for a slot's box, when the catalog has one. */
export function slotTitle(key: string): string | undefined {
  const m = SLOT_TEXT[key]
  return m ? t(m.titleKey) : undefined
}

/** Why this slot is not loggable yet — Field Day's own two sentences for Field Day's
 *  two slots, and a generic pair naming the field for anything else. */
export function fdVerdict(spec: ContestFieldSpec, raw: string, line?: string): string {
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
    return t('logEntry.fd.badNumber', { field: slotCaption(spec.key), min: spec.min, max: spec.max })
  }
  return v === ''
    ? t('logEntry.fd.needField', { field: slotCaption(spec.key) })
    : t('logEntry.fd.badField', { field: slotCaption(spec.key), value: v })
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
