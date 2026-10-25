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
import { contestEdit, resolveEntity } from '../api'
import type { ContestFieldSpec, FieldDayQso } from '../types'
import { t } from '../i18n'
import { isStreamInput } from '../remote-native/stream-input'
import { bandOptions, editForm, editOf, editText, modeOptions } from '../features/contestEdit'
import { contactLabel } from '../features/contestRemoval'
import { receivedCaption } from '../features/contestSlots'

// ONE EDIT OF A CONTACT AT A TIME IN A WINDOW. The last contacts' Edit, the contest screen's and the
// strip's Ctrl+Y can each open one, and two open at once would race: the engine refuses the second
// save as changed, and the losing edit's typing is gone. So a host that has none open asks here
// first, and while one is open it opens nothing and the keyboard goes back to the open one as typed.
let openEdit: HTMLElement | null = null

/** Is another contact's edit open in this window? If so it gets the keyboard back, and the caller
 *  opens nothing. A host asks only while it has no edit of its own open. */
export function focusOpenContestEdit(): boolean {
  const el = openEdit
  if (!el?.isConnected) return false
  el.querySelector<HTMLInputElement>('input')?.focus({ preventScroll: true })
  el.scrollIntoView?.({ block: 'nearest' })
  return true
}

/** The line a host shows once an edit is over. */
export interface EditNote {
  text: string
  alert: boolean
}

export function ContestContactEdit({
  rows,
  receives,
  role,
  bands,
  onDone,
}: {
  /** The contact: one row, or one per county for a county line. */
  rows: FieldDayQso[]
  /** The session's received slots, in receive order. */
  receives: readonly ContestFieldSpec[]
  /** The session's role, which each box's caption depends on as the strip's does. */
  role?: string
  /** The contest's bands. */
  bands?: readonly string[]
  /** The edit is over: what to say, or `null` when it was left as it was. */
  onDone: (note: EditNote | null) => void
}) {
  const [form, setForm] = useState(() => editForm(rows, receives))
  const [why, setWhy] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const callRef = useRef<HTMLInputElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = boxRef.current
    openEdit = el
    return () => {
      if (openEdit === el) openEdit = null
    }
  }, [])
  // The country of the call in the form, as the strip resolves its own: from a station in the USA
  // or Canada a QSO party's QTH box takes only what is on the contest's lists (`editOf`).
  const [entity, setEntity] = useState<string | null>(null)
  useEffect(() => {
    const call = form.call.trim()
    if (!call) {
      setEntity(null)
      return
    }
    let stale = false
    void resolveEntity(call)
      .then((e) => {
        if (!stale) setEntity(e)
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [form.call])
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
    const made = editOf(form, rows, receives, role, entity)
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
      ref={boxRef}
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
          <span className="contest-edit-cap">{receivedCaption(f, role)}</span>
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
