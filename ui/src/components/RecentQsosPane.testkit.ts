// The Recent contacts pane in a cockpit's structure test: a small log served through the window's log
// source (as the engine serves it), and the pane's row for a call.
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type AnswerTo, type LogQuestion } from '../features/logAnswers'
import { __resetLogSourceForTests, setLogSource } from '../features/logSource'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const qso = (id: string, call: string, minute: number) =>
  ({ id, call, band: '20m', freqMhz: 14.2, mode: 'SSB', whenUnix: 1_700_000_000 + minute * 60 }) as unknown as LoggedQso

/** Two contacts with one call, a minute apart: only the id tells them apart. */
export const RECENT_LOG: readonly LoggedQso[] = [qso('id-older', 'W1AW', 1), qso('id-newer', 'W1AW', 2)]

/** Serve `RECENT_LOG` to every log question until `resetRecentLog`. */
export function installRecentLog(): void {
  setLogSource(createAskingLogSource(async <Q extends LogQuestion>(q: Q) => answerFrom([...RECENT_LOG], q, 1) as AnswerTo<Q>))
}

export const resetRecentLog = __resetLogSourceForTests

/** The pane's row buttons, newest first, once the list has landed. */
export function recentRows(): HTMLButtonElement[] {
  const name = t('logbook.row.edit', { call: 'W1AW' })
  return [...document.querySelectorAll<HTMLButtonElement>('[data-pane="recent"] button')].filter((b) => b.getAttribute('aria-label') === name)
}
