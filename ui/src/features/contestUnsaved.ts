// ---------------------------------------------------------------------------
// THE CONTEST JOURNAL COULD NOT BE WRITTEN, in words.
//
// The engine keeps how many of the contest journal's writes failed since its last good one, and
// when that one landed (`FieldDayStatus.unsaved`). The contacts since then are in memory only, and
// a power cut loses them. One wording for the two places that show it (the Contest screen and the
// contest log strip), so they cannot drift apart. A later good write clears it.
// ---------------------------------------------------------------------------

import { t } from '../i18n'
import type { FieldDayStatus } from '../types'

/** The red line as the operator reads it: since when nothing is saved, and what to do. */
export function contestUnsavedText(u: NonNullable<FieldDayStatus['unsaved']>): string {
  return u.lastSavedUnix != null
    ? t('fieldDay.unsaved.since', { time: new Date(u.lastSavedUnix * 1000).toISOString().slice(11, 16) })
    : t('fieldDay.unsaved.session')
}
