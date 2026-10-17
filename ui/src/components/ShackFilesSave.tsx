// "Save to this computer" for one of the station's shared folders — what a computer driving the
// station from afar shows where the station's own screen shows Reveal (which opens the station's
// file manager, on the station's screen, and is refused from afar). ALL.TXT is one file, so its
// button saves it; the recordings and SSTV pictures open a list of the station's files, each with
// its own Save. Every byte goes through features/shackFiles.ts.
import { useState } from 'react'
import { shackFileList, type ShackFile, type ShackFolder } from '../api'
import { saveStationFile } from '../features/shackFiles'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import { Dialog } from './ui/Dialog'

const ALL_TXT = 'ALL.TXT'

function kb(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} kB`
}

/** Save one station file, with its progress on `setShare`, and say where it landed. */
async function save(folder: ShackFolder, name: string, setShare: (share: number | null) => void) {
  setShare(0)
  await withErrorToast(async () => {
    const path = await saveStationFile(folder, name, setShare)
    pushToast(t('remote.stationFiles.save.done', { path }), 'info')
    return path
  }, t('remote.stationFiles.save.failed')).finally(() => setShare(null))
}

export function ShackFilesSave({ folder, className }: { folder: ShackFolder; className: string }) {
  const [open, setOpen] = useState(false)
  const [files, setFiles] = useState<ShackFile[] | null>(null)
  const [saving, setSaving] = useState<{ name: string; share: number } | null>(null)

  const saveOne = (name: string) =>
    save(folder, name, (share) => setSaving(share === null ? null : { name, share }))

  if (folder === 'allTxt') {
    return (
      <button
        type="button"
        className={className}
        title={t('remote.stationFiles.save.title')}
        disabled={saving !== null}
        onClick={() => void saveOne(ALL_TXT)}
      >
        {saving ? t('remote.stationFiles.saving', { pct: Math.round(saving.share * 100) }) : t('remote.stationFiles.save.label')}
      </button>
    )
  }

  const show = () => {
    setOpen(true)
    setFiles(null)
    void withErrorToast(async () => {
      const listed = await shackFileList(folder)
      setFiles(listed)
      return listed
    }, t('remote.stationFiles.list.failed'))
  }

  return (
    <>
      <button type="button" className={className} title={t('remote.stationFiles.save.title')} onClick={show}>
        {t('remote.stationFiles.list.label')}
      </button>
      <Dialog
        open={open}
        onOpenChange={setOpen}
        title={folder === 'sstv' ? t('remote.stationFiles.list.title.sstv') : t('remote.stationFiles.list.title.recordings')}
      >
        {files === null ? (
          <p className="dim">{t('remote.stationFiles.list.loading')}</p>
        ) : files.length === 0 ? (
          <p className="dim">{t('remote.stationFiles.list.empty')}</p>
        ) : (
          <ul className="shack-files-list">
            {files.map((f) => (
              <li key={f.name} className="shack-files-row">
                <span className="shack-files-name">{f.name}</span>
                <span className="dim">{kb(f.bytes)}</span>
                <button
                  type="button"
                  className="settings-linkbtn"
                  disabled={saving !== null}
                  aria-label={t('remote.stationFiles.save.one', { name: f.name })}
                  onClick={() => void saveOne(f.name)}
                >
                  {saving?.name === f.name
                    ? t('remote.stationFiles.saving', { pct: Math.round(saving.share * 100) })
                    : t('remote.stationFiles.save.label')}
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="confirm-actions">
          <button type="button" className="settings-refresh" onClick={() => setOpen(false)}>
            {t('remote.stationFiles.close')}
          </button>
        </div>
      </Dialog>
    </>
  )
}
