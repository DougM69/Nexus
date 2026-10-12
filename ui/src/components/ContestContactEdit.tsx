// ⭐ THE EDIT OF A LOGGED CONTEST CONTACT, IN PLACE — the contest screen's Edit (in the row it
// edits) and the strip's Ctrl+Y (in place of its entry boxes): the call, one box per received
// slot, the band and the mode, with Save and Cancel. Enter saves; Esc leaves it as it was.
// What a box may hold is the strip's own rule (`features/contestEdit`), and the engine names the
// contact by its call and time, so a contact that changed meanwhile is refused, never mis-edited.
//
// ⛔ NOTHING HERE TRANSMITS, AND NOTHING HERE STOPS A KEY ON ITS WAY. No handler calls
// stopPropagation: Esc still reaches the shared capture listener that stops TX, and Space's keyup
// still reaches Phone's push-to-talk release. Spaces are dropped from the call, as the strip
// drops them, because the space bar is the phone position's push-to-talk.

import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { contestEdit } from '../api'
import type { ContestFieldSpec, FieldDayQso } from '../types'
import { t } from '../i18n'
import { isStreamInput } from '../remote-native/stream-input'
import { bandOptions, editForm, editOf, editText, modeOptions } from '../features/contestEdit'
import { contactLabel } from '../features/contestRemoval'
import { slotCaption } from '../features/contestSlots'

/** The line a host shows once an edit is over. */
export interface EditNote {
  text: string
  alert: boolean
}

export function ContestContactEdit({
  rows,
  receives,
  bands,
  onDone,
}: {
  /** The contact: one row, or one per county for a county line. */
  rows: FieldDayQso[]
  /** The session's received slots, in receive order. */
  receives: readonly ContestFieldSpec[]
  /** The contest's bands. */
  bands?: readonly string[]
  /** The edit is over: what to say, or `null` when it was left as it was. */
  onDone: (note: EditNote | null) => void
}) {
  const [form, setForm] = useState(() => editForm(rows, receives))
  const [why, setWhy] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const callRef = useRef<HTMLInputElement>(null)
  // The keyboard goes to the call, where an edit almost always starts, its caret at the end.
  useEffect(() => {
    const el = callRef.current
    if (!el) return
    el.focus({ preventScroll: true })
    el.scrollIntoView?.({ block: 'nearest' })
    el.setSelectionRange(el.value.length, el.value.length)
  }, [])
  const first = rows[0]
  const save = () => {
    if (busy || !first) return
    // Only at the station: a press that comes through the Remote stream edits nothing.
    if (isStreamInput()) {
      onDone({ text: t('contest.edit.remote'), alert: true })
      return
    }
    const made = editOf(form, rows, receives)
    if ('why' in made) {
      setWhy(made.why)
      return
    }
    setBusy(true)
    contestEdit(first.call, first.whenUnix ?? 0, made.edit).then(
      (answer) => onDone({ text: editText(answer), alert: answer.outcome === 'refused' }),
      () => onDone({ text: t('contest.edit.failed'), alert: true }),
    )
  }
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      save()
    } else if (e.key === 'Escape') {
      onDone(null)
    }
  }
  return (
    <div
      className="contest-edit"
      role="group"
      aria-label={t('contest.edit.aria', { contact: contactLabel(rows) })}
    >
      <label className="contest-edit-field contest-edit-call">
        <span className="contest-edit-cap">{t('logEntry.fd.call.label')}</span>
        <input
          ref={callRef}
          className="settings-input mono"
          value={form.call}
          onChange={(e) => {
            const call = e.target.value.replace(/\s+/g, '').toUpperCase()
            setForm((f) => ({ ...f, call }))
          }}
          onKeyDown={onKey}
          autoComplete="off"
          spellCheck={false}
        />
      </label>
      {receives.map((f) => (
        <label className="contest-edit-field" key={f.key}>
          <span className="contest-edit-cap">{slotCaption(f.key)}</span>
          <input
            className="settings-input mono"
            value={form.fields[f.key] ?? ''}
            onChange={(e) => {
              const value = e.target.value.toUpperCase()
              setForm((s) => ({ ...s, fields: { ...s.fields, [f.key]: value } }))
            }}
            onKeyDown={onKey}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
      ))}
      <label className="contest-edit-field">
        <span className="contest-edit-cap">{t('fieldDay.log.column.band')}</span>
        {/* A satellite contact keeps the band its pass gave it. */}
        <select
          className="settings-input"
          value={form.band}
          disabled={!!first?.sat}
          onChange={(e) => {
            const band = e.target.value
            setForm((s) => ({ ...s, band }))
          }}
          onKeyDown={onKey}
        >
          {bandOptions(first, bands).map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
      </label>
      <label className="contest-edit-field">
        <span className="contest-edit-cap">{t('fieldDay.log.column.mode')}</span>
        <select
          className="settings-input"
          value={form.mode}
          onChange={(e) => {
            const mode = e.target.value
            setForm((s) => ({ ...s, mode }))
          }}
          onKeyDown={onKey}
        >
          {modeOptions(first).map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </label>
      <button type="button" className="le-log-btn contest-edit-save" onClick={save} disabled={busy}>
        {t('contest.edit.save')}
      </button>
      <button type="button" className="le-qrz" onClick={() => onDone(null)}>
        {t('contest.edit.cancel')}
      </button>
      {why && (
        <p className="contest-edit-why" role="alert">
          {why}
        </p>
      )}
    </div>
  )
}
