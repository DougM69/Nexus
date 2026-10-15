// The Logbook's country picker, the part that is not a screen: which DXCC entities a search finds,
// and the DXCC numbers a logged contact carries. Pure, so the picker's rules are tested without a
// station.
//
// The entities are the station's (`getDxccEntities`): every number a record can name, current and
// deleted. Their names and numbers are data, the same in every language; only the words around
// them come from the catalog.

import type { DxccEntityOption, LoggedQso } from '../types'

/** How many entities the picker lists at once. The list scrolls, and a search narrows it; the
 *  rest are counted, never silently dropped. */
export const PICK_LIMIT = 60

/** A name as a search compares it: lower case, accents aside ("Curaçao" finds "Curacao"). */
function folded(s: string): string {
  return s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()
}

/** The entities a search finds, and how many there are in all (`total` counts past `limit`).
 *
 *  Digits find by NUMBER: the numbers that begin with them, in number order, so the entity with
 *  exactly that number comes first ("18" → 18, 180…189). Anything else finds by NAME: every word
 *  of the search somewhere in the name — "german dem" finds the GERMAN DEMOCRATIC REPUBLIC, "fed
 *  rep" the Fed. Rep. of Germany — the names that BEGIN with the search first, then the rest, each
 *  by name. Deleted entities are found like current ones: an old log is where they live. An empty
 *  search lists every entity, by name. */
export function findEntities(
  all: readonly DxccEntityOption[],
  search: string,
  limit = PICK_LIMIT,
): { found: DxccEntityOption[]; total: number } {
  const q = folded(search.trim())
  let hits: DxccEntityOption[]
  if (/^\d+$/.test(q)) {
    hits = all.filter((e) => String(e.code).startsWith(q)).sort((a, b) => a.code - b.code)
  } else {
    const words = q.split(/\s+/).filter(Boolean)
    const byName = (a: DxccEntityOption, b: DxccEntityOption) => a.name.localeCompare(b.name)
    const named = all.filter((e) => {
      const name = folded(e.name)
      return words.every((w) => name.includes(w))
    })
    const starts = named.filter((e) => folded(e.name).startsWith(q)).sort(byName)
    const rest = named.filter((e) => !folded(e.name).startsWith(q)).sort(byName)
    hits = [...starts, ...rest]
  }
  return { found: hits.slice(0, limit), total: hits.length }
}

/** The ADIF field the station keeps LoTW's DXCC number in, where LoTW confirmed a contact as
 *  another entity than the log's own (tempo-core's `LOTW_DXCC_FIELD`). */
const LOTW_DXCC_FIELD = 'APP_NEXUS_LOTW_DXCC'

/** LoTW's DXCC number for a contact, where the log keeps one beside its own — the number the
 *  awards count the contact by, whatever the log's own says. */
export function lotwDxccOf(q: Pick<LoggedQso, 'extra'>): number | null {
  const kept = q.extra?.find(([name]) => name === LOTW_DXCC_FIELD)?.[1]?.trim()
  return kept && /^\d+$/.test(kept) ? Number(kept) : null
}
