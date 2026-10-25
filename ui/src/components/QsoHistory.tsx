// A CONTACT'S HISTORY — every change the log recorded for it, oldest first, and the undo of each.
//
// Shown as the last section of the QSO detail view. Each entry says when, what kind of change,
// who made it (the operator, an import, a download, the fill job, an undo) and the fields it
// changed, old → new. An edit, a merge and a fill each carry Undo: the backend puts back only the
// fields that still hold what the change wrote, and says which it kept. An undo is itself an
// entry, so undoing it is the redo; the toast after an undo offers exactly that.
//
// Nothing here sends anything to a service: an undo never queues an upload (the backend's rule,
// `tempo_app::loghistory`).
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). ADIF tags, field values
// and service names are invariant tokens, shown as they are.
import { useCallback, useEffect, useState } from 'react'
import { qsoHistory, undoHistoryRun, undoQsoChange, type QsoHistoryEntry, type UndoAnswer } from '../api'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'

/** The services' own names, which are tokens, not prose. */
const SERVICE: Readonly<Record<string, string>> = { lotw: 'LoTW', eqsl: 'eQSL', qrz: 'QRZ', pota: 'POTA' }

/** Who made a change, in words. */
export function sourceWord(source: string): string {
  if (SERVICE[source]) return SERVICE[source]
  switch (source) {
    case 'operator': return t('qso.history.source.operator')
    case 'import': return t('qso.history.source.import')
    case 'fill': return t('qso.history.source.fill')
    case 'contest': return t('qso.history.source.contest')
    case 'remote': return t('qso.history.source.remote')
    case 'file': return t('qso.history.source.file')
    case 'undo': return t('qso.history.source.undo')
    default: return source
  }
}

function kindWord(kind: QsoHistoryEntry['kind']): string {
  switch (kind) {
    case 'create': return t('qso.history.kind.create')
    case 'delete': return t('qso.history.kind.delete')
    case 'merge': return t('qso.history.kind.merge')
    case 'fill': return t('qso.history.kind.fill')
    default: return t('qso.history.kind.edit')
  }
}

/** UTC, to the second — as every other surface states a time. */
export function utcMs(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ` +
    `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}Z`
}

/** What an undo made, said in a toast — with Redo where it changed something. */
export function sayUndone(answer: UndoAnswer, done: string, onChanged: () => void): void {
  if (answer.changed === 0) {
    pushToast(
      answer.alreadyThere > 0 ? t('qso.history.undo.alreadyThere')
        : answer.gone > 0 ? t('qso.history.undo.gone')
        : t('qso.history.undo.nothing'),
      'info',
      8000,
    )
    return
  }
  const kept = answer.kept.length ? t('qso.history.undo.kept', { fields: answer.kept.join(', ') }) : ''
  const run = answer.run
  pushToast(`${done}${kept}`, 'success', 10_000, run ? {
    actionLabel: t('qso.history.redo'),
    action: () => {
      void withErrorToast(() => undoHistoryRun(run), t('qso.history.undo.failed')).then((a) => {
        if (a) { sayUndone(a, t('qso.history.redone'), onChanged); onChanged() }
      })
    },
  } : {})
}

export function QsoHistory({ id, onChanged }: { id: string; onChanged?: () => void }) {
  const [entries, setEntries] = useState<QsoHistoryEntry[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setEntries(await qsoHistory(id))
      setFailed(false)
    } catch {
      setFailed(true)
    }
  }, [id])

  useEffect(() => {
    void load()
  }, [load])

  const changed = () => {
    void load()
    onChanged?.()
  }

  const undo = async (e: QsoHistoryEntry) => {
    setBusy(true)
    const answer = await withErrorToast(() => undoQsoChange(e.seq), t('qso.history.undo.failed'))
    setBusy(false)
    if (!answer) return
    sayUndone(answer, t('qso.history.undone'), changed)
    changed()
  }

  const none = t('qso.history.noValue')
  return (
    <section className="qso-detail-section qso-history">
      <h3 className="qso-detail-heading">{t('qso.history.title')}</h3>
      {failed && <p className="qso-detail-bare">{t('qso.history.failed')}</p>}
      {!failed && entries !== null && entries.length === 0 && (
        <p className="qso-detail-bare">{t('qso.history.empty')}</p>
      )}
      {!failed && entries !== null && entries.length > 0 && (
        <ol className="qso-history-list">
          {[...entries].reverse().map((e) => (
            <li key={e.seq} className="qso-history-entry">
              <div className="qso-history-head">
                <span className="mono qso-history-when">{utcMs(e.atMs)}</span>
                <span className="qso-history-what">
                  {t('qso.history.what', { kind: kindWord(e.kind), source: sourceWord(e.source) })}
                </span>
                {e.undoneBy !== null ? (
                  <span className="qso-history-undone">{t('qso.history.wasUndone')}</span>
                ) : (e.kind === 'edit' || e.kind === 'merge' || e.kind === 'fill') && (
                  <button
                    type="button"
                    className="qso-history-undo"
                    disabled={busy}
                    onClick={() => void undo(e)}
                  >
                    {t('qso.history.undo')}
                  </button>
                )}
              </div>
              {e.changes.length > 0 && (
                <dl className="qso-detail-fields qso-history-changes">
                  {e.changes.map((c) => (
                    <div key={c.tag} className="qso-detail-field">
                      <dt className="mono">{c.tag}</dt>
                      <dd className="mono">{`${c.old ?? none} → ${c.new ?? none}`}</dd>
                    </div>
                  ))}
                </dl>
              )}
            </li>
          ))}
        </ol>
      )}
    </section>
  )
}
