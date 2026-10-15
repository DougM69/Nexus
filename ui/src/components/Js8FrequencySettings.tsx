// Settings ▸ Digital ▸ JS8 ▸ Frequencies — JS8Call 3.0.3's Settings ▸ Frequencies tab:
//   · "Return to the last used frequency" (JS8Call's "Monitor returns to last used frequency"),
//     off by default as JS8Call ships it;
//   · the operator's own frequency list, with the descriptions 3.0 added, Reset (back to JS8Call's
//     table), Import and Export. While no list of the operator's own is kept, JS8Call's table
//     shows here and the first edit makes it theirs;
//   · the band schedule (JS8Call's Frequency Schedule): it changes the rig's frequency BY ITSELF,
//     with nobody at the radio, so it is off by default and its hint says so.
// Everything here is a form field: Save sends it with the rest of Settings. The dials are DATA
// (`toFixed`), never prose.
import { useEffect, useRef, useState } from 'react'
import type { Js8Freq, Js8Hop, Settings } from '../types'
import { js8DefaultFrequencies, saveTextToDownloads } from '../api'
import { confirmDialog } from '../confirm'
import { pushToast, withErrorToast } from '../toast'
import { t } from '../i18n'
import {
  JS8_DESCRIPTION_MAX,
  hhmmToMinutes,
  js8FrequencyFile,
  minutesToHhmm,
  newHop,
  parseJs8FrequencyFile,
  parseMhz,
} from '../features/js8Frequencies'

/** The dial the refusal shows as an example: a frequency, an invariant token. */
const MHZ_EXAMPLE = '7.078'
/** The time fields' placeholder: the format, an invariant token. */
const HHMM_PLACEHOLDER = 'HH:MM'

export type Js8FrequencyPatch = Partial<
  Pick<Settings, 'js8Frequencies' | 'js8ReturnToLastDial' | 'js8BandHop' | 'js8Schedule'>
>

interface Props {
  form: Settings
  onChange: (patch: Js8FrequencyPatch) => void
  disabled?: boolean
}

function Toggle({
  label,
  hint,
  on,
  disabled,
  onToggle,
}: {
  label: string
  hint: string
  on: boolean
  disabled?: boolean
  onToggle: () => void
}) {
  return (
    <div className="settings-field">
      <label className="settings-toggle">
        <span className="settings-label">{label}</span>
        <button
          disabled={disabled}
          type="button"
          role="switch"
          aria-checked={on}
          className={`toggle${on ? ' on' : ''}`}
          onClick={onToggle}
        >
          <span className="toggle-knob" />
        </button>
      </label>
      <span className="settings-hint">{hint}</span>
    </div>
  )
}

/** A dial field that commits on blur or Enter: a half-typed "7." is never stored. */
function MhzInput({
  value,
  ariaLabel,
  disabled,
  onCommit,
}: {
  value: number
  ariaLabel: string
  disabled?: boolean
  onCommit: (mhz: number) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    const mhz = parseMhz(draft)
    if (mhz === null) pushToast(t('settings.js8.freqs.badMhz', { example: MHZ_EXAMPLE }), 'info', 3500)
    else if (mhz !== value) onCommit(mhz)
    setDraft(null)
  }
  return (
    <input
      disabled={disabled}
      className="settings-input"
      type="text"
      inputMode="decimal"
      aria-label={ariaLabel}
      value={draft ?? value.toFixed(6)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit()
      }}
      autoComplete="off"
    />
  )
}

/** A UTC time of day as 24-hour HH:MM text, committed on blur or Enter; text that is not a time
 *  puts the stored one back. Plain text, not `type="time"`: the native control draws the computer's
 *  12-hour clock in some locales, and these are UTC schedule times (JS8Call shows hh:mm). */
function HhmmInput({
  value,
  ariaLabel,
  title,
  disabled,
  onCommit,
}: {
  value: number
  ariaLabel: string
  title?: string
  disabled?: boolean
  onCommit: (min: number) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    const min = hhmmToMinutes(draft)
    if (min !== null && min !== value) onCommit(min)
    setDraft(null)
  }
  return (
    <input
      disabled={disabled}
      className="settings-input js8-hhmm"
      type="text"
      inputMode="numeric"
      aria-label={ariaLabel}
      title={title}
      placeholder={HHMM_PLACEHOLDER}
      maxLength={5}
      value={draft ?? minutesToHhmm(value)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit()
      }}
      autoComplete="off"
    />
  )
}

export function Js8FrequencySettings({ form, onChange, disabled }: Props) {
  // JS8Call's own table, from the station: shown while the operator keeps no list of their own.
  const [defaults, setDefaults] = useState<Js8Freq[]>([])
  useEffect(() => {
    let live = true
    void js8DefaultFrequencies()
      .then((rows) => {
        // A list, or nothing to show: an answer of any other shape is not JS8Call's table.
        if (live && Array.isArray(rows)) setDefaults(rows)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  const own = form.js8Frequencies ?? null
  const rows = own ?? defaults
  const setRows = (next: Js8Freq[]) => onChange({ js8Frequencies: next })
  const editRow = (i: number, patch: Partial<Js8Freq>) =>
    setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)))

  const [addMhz, setAddMhz] = useState('')
  const [addDesc, setAddDesc] = useState('')
  const add = () => {
    const mhz = parseMhz(addMhz)
    if (mhz === null) {
      pushToast(t('settings.js8.freqs.badMhz', { example: MHZ_EXAMPLE }), 'info', 3500)
      return
    }
    setRows([...rows, { mhz, description: addDesc.trim() }])
    setAddMhz('')
    setAddDesc('')
  }

  const reset = async () => {
    if (
      own !== null &&
      !(await confirmDialog({
        title: t('settings.js8.freqs.reset.confirm.title'),
        body: t('settings.js8.freqs.reset.confirm.body'),
        confirmLabel: t('settings.js8.freqs.reset.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    onChange({ js8Frequencies: null })
  }

  const exportRows = () =>
    withErrorToast(async () => {
      const stamp = new Date().toISOString().slice(0, 10)
      // The FILE NAME is invariant: a translated word in it would reduce a non-Latin locale's
      // file to `nexus-js8-frequencies-.json`.
      const path = await saveTextToDownloads(`nexus-js8-frequencies-${stamp}.json`, js8FrequencyFile(rows))
      pushToast(t('settings.js8.freqs.export.done', { path }), 'success')
    }, t('settings.js8.freqs.export.failed'))

  const fileRef = useRef<HTMLInputElement>(null)
  const importFile = async (file: File) => {
    const parsed = parseJs8FrequencyFile(await file.text())
    if (parsed === null) {
      pushToast(t('settings.js8.freqs.import.bad'), 'error')
      return
    }
    // JS8Call's Load asks before it replaces a list that holds anything (Configuration.cpp:4088-4099).
    if (
      own !== null &&
      own.length > 0 &&
      !(await confirmDialog({
        title: t('settings.js8.freqs.import.confirm.title'),
        body: t('settings.js8.freqs.import.confirm.body'),
        confirmLabel: t('settings.js8.freqs.import.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    setRows(parsed)
    pushToast(t('settings.js8.freqs.import.done', { count: parsed.length }), 'info')
  }

  const schedule = form.js8Schedule ?? []
  const setSchedule = (next: Js8Hop[]) => onChange({ js8Schedule: next })
  const editHop = (i: number, patch: Partial<Js8Hop>) =>
    setSchedule(schedule.map((r, j) => (j === i ? { ...r, ...patch } : r)))
  const [hopMhz, setHopMhz] = useState('')
  const addHop = () => {
    const mhz = parseMhz(hopMhz)
    if (mhz === null) {
      pushToast(t('settings.js8.freqs.badMhz', { example: MHZ_EXAMPLE }), 'info', 3500)
      return
    }
    setSchedule([...schedule, newHop(mhz, Date.now())])
    setHopMhz('')
  }
  const hopOn = form.js8BandHop === true

  return (
    <div className="settings-featgroup js8-freq-settings">
      <span className="settings-featgroup-title">{t('settings.js8.freq.title')}</span>

      <Toggle
        label={t('settings.js8.returnToLastDial.label')}
        hint={t('settings.js8.returnToLastDial.hint')}
        on={form.js8ReturnToLastDial === true}
        disabled={disabled}
        onToggle={() => onChange({ js8ReturnToLastDial: form.js8ReturnToLastDial !== true })}
      />

      <div className="settings-field">
        <span className="settings-label">{t('settings.js8.freqs.label')}</span>
        {rows.map((r, i) => (
          <div className="js8-freq-row" key={i}>
            <MhzInput
              value={r.mhz}
              ariaLabel={t('settings.js8.freqs.mhz.aria', { n: i + 1 })}
              disabled={disabled}
              onCommit={(mhz) => editRow(i, { mhz })}
            />
            <input
              disabled={disabled}
              className="settings-input"
              type="text"
              aria-label={t('settings.js8.freqs.description.aria', { n: i + 1 })}
              placeholder={t('settings.js8.freqs.description.placeholder')}
              value={r.description}
              maxLength={JS8_DESCRIPTION_MAX}
              onChange={(e) => editRow(i, { description: e.target.value })}
              autoComplete="off"
            />
            <button
              disabled={disabled}
              type="button"
              className="settings-refresh"
              title={t('settings.js8.freqs.remove.title')}
              aria-label={t('settings.js8.freqs.remove.aria', { mhz: r.mhz.toFixed(6) })}
              onClick={() => setRows(rows.filter((_, j) => j !== i))}
            >
              ✕
            </button>
          </div>
        ))}
        <div className="js8-freq-row">
          <input
            disabled={disabled}
            className="settings-input"
            type="text"
            inputMode="decimal"
            aria-label={t('settings.js8.freqs.add.mhz.aria')}
            placeholder={t('settings.js8.freqs.add.mhz.placeholder')}
            value={addMhz}
            onChange={(e) => setAddMhz(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') add()
            }}
            autoComplete="off"
          />
          <input
            disabled={disabled}
            className="settings-input"
            type="text"
            aria-label={t('settings.js8.freqs.add.description.aria')}
            placeholder={t('settings.js8.freqs.description.placeholder')}
            value={addDesc}
            maxLength={JS8_DESCRIPTION_MAX}
            onChange={(e) => setAddDesc(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') add()
            }}
            autoComplete="off"
          />
          <button disabled={disabled} type="button" className="settings-refresh" onClick={add}>
            {t('settings.js8.freqs.add.action')}
          </button>
        </div>
        <div className="rig-share-row">
          <button disabled={disabled} type="button" className="settings-linkbtn" onClick={() => void reset()}>
            {t('settings.js8.freqs.reset.action')}
          </button>
          <button disabled={disabled} type="button" className="settings-linkbtn" onClick={() => void exportRows()}>
            {t('settings.js8.freqs.export.action')}
          </button>
          <input
            disabled={disabled}
            ref={fileRef}
            type="file"
            accept=".json"
            style={{ display: 'none' }}
            onChange={(e) => {
              const f = e.target.files?.[0]
              e.target.value = ''
              if (f) void importFile(f)
            }}
          />
          <button
            disabled={disabled}
            type="button"
            className="settings-linkbtn"
            onClick={() => fileRef.current?.click()}
          >
            {t('settings.js8.freqs.import.action')}
          </button>
        </div>
        <span className="settings-hint">
          {own === null ? t('settings.js8.freqs.hint.stock') : t('settings.js8.freqs.hint.own')}
        </span>
      </div>

      <Toggle
        label={t('settings.js8.schedule.label')}
        hint={t('settings.js8.schedule.hint')}
        on={hopOn}
        disabled={disabled}
        onToggle={() => onChange({ js8BandHop: !hopOn })}
      />
      <div className="settings-field">
        <span className="settings-label">{t('settings.js8.schedule.list.label')}</span>
        {schedule.length === 0 && <span className="settings-hint">{t('settings.js8.schedule.empty')}</span>}
        {schedule.map((r, i) => (
          <div className="js8-hop-row" key={i}>
            <MhzInput
              value={r.mhz}
              ariaLabel={t('settings.js8.schedule.mhz.aria', { n: i + 1 })}
              disabled={disabled}
              onCommit={(mhz) => editHop(i, { mhz })}
            />
            <HhmmInput
              value={r.fromMin}
              ariaLabel={t('settings.js8.schedule.from.aria', { n: i + 1 })}
              disabled={disabled}
              onCommit={(fromMin) => editHop(i, { fromMin })}
            />
            <HhmmInput
              value={r.untilMin}
              ariaLabel={t('settings.js8.schedule.until.aria', { n: i + 1 })}
              title={t('settings.js8.schedule.until.title')}
              disabled={disabled}
              onCommit={(untilMin) => editHop(i, { untilMin })}
            />
            <input
              disabled={disabled}
              className="settings-input"
              type="text"
              aria-label={t('settings.js8.schedule.description.aria', { n: i + 1 })}
              placeholder={t('settings.js8.freqs.description.placeholder')}
              value={r.description}
              maxLength={JS8_DESCRIPTION_MAX}
              onChange={(e) => editHop(i, { description: e.target.value })}
              autoComplete="off"
            />
            <button
              disabled={disabled}
              type="button"
              className="settings-refresh"
              title={t('settings.js8.schedule.remove.title')}
              aria-label={t('settings.js8.schedule.remove.aria', { n: i + 1 })}
              onClick={() => setSchedule(schedule.filter((_, j) => j !== i))}
            >
              ✕
            </button>
          </div>
        ))}
        <div className="js8-hop-add">
          <input
            disabled={disabled}
            className="settings-input"
            type="text"
            inputMode="decimal"
            aria-label={t('settings.js8.schedule.add.mhz.aria')}
            placeholder={t('settings.js8.freqs.add.mhz.placeholder')}
            value={hopMhz}
            onChange={(e) => setHopMhz(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') addHop()
            }}
            autoComplete="off"
          />
          <button disabled={disabled} type="button" className="settings-refresh" onClick={addHop}>
            {t('settings.js8.schedule.add.action')}
          </button>
        </div>
        <span className="settings-hint">{t('settings.js8.schedule.list.hint')}</span>
      </div>
    </div>
  )
}
