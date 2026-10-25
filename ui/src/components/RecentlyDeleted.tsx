// RECENTLY DELETED — the contacts taken out of the log and not put back, newest first, each with
// Restore. A restore puts the contact back whole, under its own id, with every field and upload
// mark it had, so nothing goes to a service again. The list is the log's own history, so it
// survives a restart — on the logbook database. On the fallback path (the log kept in log.adi
// when its database could not be opened) the history lives in memory and starts empty each launch.
// A deleted contact's copy is kept for the days Settings names (90 by default), then removed.
// A restore is refused when the contact was logged again after it was deleted (the same station,
// band and mode within 5 minutes); the row then offers Restore anyway, for two different contacts.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Calls, bands and modes
// are invariant tokens, shown as they are.
import { useCallback, useEffect, useState } from 'react'
import { recentlyDeletedQsos, restoreDeletedQso, type DeletedQso } from '../api'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { utcDate } from '../features/utcLog'
import { Dialog } from './ui/Dialog'
import { sayUndone, sourceWord, utcMs } from './QsoHistory'

export function RecentlyDeleted({
  open,
  onClose,
  onRestored,
}: {
  open: boolean
  onClose: () => void
  onRestored: () => void
}) {
  const [rows, setRows] = useState<DeletedQso[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  // The rows whose restore was refused because the contact was logged again since.
  const [blocked, setBlocked] = useState<ReadonlySet<string>>(new Set())

  const load = useCallback(async () => {
    try {
      setRows(await recentlyDeletedQsos())
      setFailed(false)
    } catch {
      setFailed(true)
    }
  }, [])

  useEffect(() => {
    if (open) void load()
    else { setRows(null); setBlocked(new Set()) }
  }, [open, load])

  const changed = () => {
    void load()
    onRestored()
  }

  const restore = async (r: DeletedQso, anyway: boolean) => {
    setBusy(r.id)
    const answer = await withErrorToast(() => restoreDeletedQso(r.id, anyway), t('logbook.deleted.failed'))
    setBusy(null)
    if (!answer) return
    if (answer.changed === 0 && answer.duplicate > 0) setBlocked((b) => new Set(b).add(r.id))
    sayUndone(answer, t('logbook.deleted.restored', { call: r.call }), changed)
    changed()
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('logbook.deleted.title')}
      description={t('logbook.deleted.intro')}
      className="recently-deleted-dialog"
    >
      {failed && <p className="qso-detail-bare">{t('logbook.deleted.readFailed')}</p>}
      {!failed && rows === null && <p className="qso-detail-bare">{t('logbook.deleted.reading')}</p>}
      {!failed && rows !== null && rows.length === 0 && (
        <p className="qso-detail-bare">{t('logbook.deleted.empty')}</p>
      )}
      {!failed && rows !== null && rows.length > 0 && (
        <ul className="recently-deleted-list">
          {rows.map((r) => (
            <li key={r.id} className="recently-deleted-row">
              <span className="mono recently-deleted-call">{r.call}</span>
              <span className="mono">{utcDate(r.whenUnix)}</span>
              <span className="mono">{`${r.band} ${r.mode}`}</span>
              <span className="recently-deleted-when">
                {t('logbook.deleted.when', { when: utcMs(r.deletedAtMs), source: sourceWord(r.source) })}
              </span>
              <button
                type="button"
                className="export-btn"
                disabled={busy !== null}
                title={blocked.has(r.id) ? t('logbook.deleted.restoreAnywayHint') : undefined}
                onClick={() => void restore(r, blocked.has(r.id))}
              >
                {blocked.has(r.id) ? t('logbook.deleted.restoreAnyway') : t('logbook.deleted.restore')}
              </button>
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  )
}
