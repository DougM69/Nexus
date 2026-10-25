// RECENTLY DELETED — the contacts taken out of the log and not put back, newest first, each with
// Restore. A restore puts the contact back whole, under its own id, with every field and upload
// mark it had, so nothing goes to a service again. The list is the log's own history, so it
// survives a restart — on the logbook database. On the fallback path (the log kept in log.adi
// when its database could not be opened) the history lives in memory and starts empty each launch.
// A deleted contact's copy is kept for the days Settings names (90 by default), then removed.
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
    else setRows(null)
  }, [open, load])

  const changed = () => {
    void load()
    onRestored()
  }

  const restore = async (r: DeletedQso) => {
    setBusy(r.id)
    const answer = await withErrorToast(() => restoreDeletedQso(r.id), t('logbook.deleted.failed'))
    setBusy(null)
    if (!answer) return
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
                onClick={() => void restore(r)}
              >
                {t('logbook.deleted.restore')}
              </button>
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  )
}
