// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every word comes from the
// catalog; the modes below are wire values (ADIF mode names), never translated, and `A−` / `A+` are
// the text size control's glyphs, as on a Connect box's menu.
//
// THE CONTEST LOGGER WINDOW (`?panel=contestlog`) — a log line for a second person logging at a
// second monitor and keyboard while the operator works the radio in the main window (two keyboards
// and two mice on one computer, through MouseMux), with the whole contest screen under it.
//
// ⭐ THE ENTRY FIRST, AND LARGE (the operator's layout, 2026-10-11). For the person at this window
// the call and the exchange are the job, so they are the top of the window, in large text, with the
// dupe warning right under the boxes; the last contacts come next, newest first, at the same size;
// and the contest screen (operator, band board, score) scrolls below. A− / A+ in the bar over the
// entry sizes the entry and the last contacts together, and this computer remembers it for this
// window. The sizes are styles.css's, scoped to this window: the main window's log line is untouched.
//
// ⭐ ONE CONTACT, TWO WINDOWS. The log line here and the operator's own contest strip share the
// contact in progress through the engine (`features/contestEntryShare`): what is typed in either
// shows in both as it is typed, every hint follows it in both, and Enter in either logs it once.
//
// ⛔ NOTHING IN THIS WINDOW TRANSMITS. No PTT, no F-key messages, no Tune, no Enter Sends Message:
// the log line here gets no `esm`; the screen's contest switch, Running / S&P and scoring are
// read-only (`ContestView`'s `logger`); the F-keys do nothing (the webview's own as well, so F5
// does not reload the logger mid-contact); Esc clears the entry (`LogEntry`'s `sharedEntry`); and
// A− / A+ only size the text.
// Every TX and stop control stays on the operator's screen, whose hotkeys are bound in the main
// window's document only. This is a separate window with a document of its own, so a keystroke
// here never reaches them, and one there never reaches this.
//
// It logs as THIS computer's own position — the same engine, contest log and club sync, so the
// club wire carries nothing new — and under the station's operating mode, read live from the
// engine, so a contact logged here is credited as the operator's own would be. A Remote viewer
// never sees this window: the stream is the main window's picture.

import { useEffect, useId, useMemo, useState, type CSSProperties } from 'react'
import { getPskState, type FdRulesetDto } from '../api'
import { isFieldDay } from '../fdEvent'
import { BOX_SCALE_MAX, BOX_SCALE_MIN, BOX_SCALE_STEP, boxScaleValue } from '../features/panelState'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
import { PSK_MODE_BY_SLUG } from '../pskModes'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { useContestSwitch } from '../contestSwitch'
import { useOperatorBoxAsk } from '../features/contestKeys'
import { annotate, ContestLogHead, ContestLogRow, ContestView } from './ContestView'
import { LogEntry } from './LogEntry'

/** Where the window keeps its text size. Per surface (`windowScope`): this window's alone, on this
 *  computer. */
const TEXT_SCALE_KEY = 'nexus.contestLogger.textScale'

/** How many of the newest contacts the window lists under the entry. */
export const LAST_CONTACTS = 5

/** The text size as stored, clamped on load into the range A− / A+ writes — a Connect box's, 80–160 %
 *  in steps of 10 — and 100 % for anything that is not a size (nothing stored, a hand edit). */
export function loggerTextScale(stored: string | null): number {
  return boxScaleValue(Number(stored)) ?? 1
}

/** The newest contacts of the running contest, newest first, in the contest screen's own columns. */
function LastContacts({ fieldDay, style }: { fieldDay: FieldDayStatus; style: CSSProperties }) {
  const titleId = useId()
  const fieldDayColumns = isFieldDay(fieldDay.event)
  const slotColumns = fieldDayColumns ? [] : (fieldDay.receives ?? [])
  // Marked over the whole log, as the screen marks it (a new section is new in log order), then the
  // newest few, each keyed by its place in the log.
  const rows = useMemo(
    () =>
      annotate(fieldDay.log)
        .map((row, at) => ({ row, at }))
        .slice(-LAST_CONTACTS)
        .reverse(),
    [fieldDay.log],
  )
  return (
    <section className="contest-logger-recent" aria-labelledby={titleId} style={style}>
      <div className="contest-logger-recent-body">
        <h2 id={titleId} className="contest-logger-recent-title">
          {t('contestLogger.recent.title')}
        </h2>
        <ContestLogHead fieldDayColumns={fieldDayColumns} isWfd={fieldDay.event === 'wfd'} slotColumns={slotColumns} />
        {rows.length === 0 && <p className="empty">{t('fieldDay.log.empty')}</p>}
        {rows.map(({ row, at }) => (
          <ContestLogRow
            key={`${row.qso.call}-${at}`}
            row={row}
            fieldDayColumns={fieldDayColumns}
            slotColumns={slotColumns}
          />
        ))}
      </div>
    </section>
  )
}

/** What this window logs under: the station's operating section (`snap.radio.operatingMode`, the
 *  engine's, which follows the operator from cockpit to cockpit) as the cockpit there logs it —
 *  Phone as PH, CW as CW, RTTY and the keyboard modes as DIG with the mode on the air, and the FT
 *  modes as DIG, whose mode the engine stamps from the tier it runs. A station too old to say
 *  logs as Phone, the voice logging this window exists for. */
export function loggerMode(
  section: string | undefined,
  pskName: string | null,
): { fdMode: 'CW' | 'PH' | 'DIG'; fdSubmode?: string; mode: string; defaultRst: string } {
  switch (section) {
    case 'cw':
      return { fdMode: 'CW', mode: 'CW', defaultRst: '599' }
    case 'rtty':
      return { fdMode: 'DIG', fdSubmode: 'RTTY', mode: 'RTTY', defaultRst: '599' }
    case 'keyboard': {
      // The PSK cockpit's own fallback when its state has not arrived: PSK31.
      const name = pskName ?? PSK_MODE_BY_SLUG.psk31.name
      return { fdMode: 'DIG', fdSubmode: name, mode: name, defaultRst: '599' }
    }
    case 'digital':
      return { fdMode: 'DIG', mode: 'FT8', defaultRst: '599' }
    default:
      return { fdMode: 'PH', mode: 'SSB', defaultRst: '59' }
  }
}

export function ContestLogger({
  snap,
  fdActive,
  fdRuleset,
}: {
  snap: AppSnapshot
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
}) {
  const section = snap.radio.operatingMode
  // The keyboard modes' sub-mode is the PSK cockpit's state, not the snapshot's: read it the way
  // that cockpit does, when the station is on that section.
  const [pskName, setPskName] = useState<string | null>(null)
  useEffect(() => {
    if (section !== 'keyboard') return
    let live = true
    getPskState()
      .then((s) => live && setPskName(PSK_MODE_BY_SLUG[s.mode ?? 'psk31']?.name ?? null))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [section])
  // THE F-KEYS DO NOTHING HERE. In the main window they send messages; in this one they are not
  // bound to anything, and the webview's own F-key actions (F5 reloads the page) are held too.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key)) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const fd = snap.fieldDay ?? null
  // CTRL+O FROM THE LOG LINE (`features/contestKeys.ts`): the operator box is this window's own, in
  // its copy of the Contest screen below.
  const [operatorAsk, setOperatorAsk] = useState(0)
  useOperatorBoxAsk(() => setOperatorAsk((n) => n + 1))
  // A contest switched by a pick saved in the main window: this copy of the Contest screen says
  // so too.
  const contestSwitch = useContestSwitch(fd)
  const m = loggerMode(section, pskName)
  // THE TEXT SIZE (A− / A+): a factor on the app's Text size for the entry and the last contacts,
  // remembered on this computer for this window and clamped as it is read back.
  const [textScale, setTextScale] = useState(() => loggerTextScale(surfaceGet(TEXT_SCALE_KEY)))
  const stepText = (dir: 1 | -1) => {
    const next = boxScaleValue(Math.round((textScale + dir * BOX_SCALE_STEP) * 100) / 100) ?? 1
    setTextScale(next)
    surfaceSet(TEXT_SCALE_KEY, String(next))
  }
  const scaled = { '--logger-text-scale': textScale } as CSSProperties
  return (
    <>
      {fd && (
        <section className="contest-logger-entry" aria-label={t('contestLogger.entry.aria')}>
          <div className="contest-logger-bar">
            <span className="contest-logger-title">{t('contestLogger.title')}</span>
            <span className="contest-logger-size">
              {t('contestLogger.size.label', { pct: Math.round(textScale * 100) })}
            </span>
            {/* A press sizes the text and takes no focus, so the caret stays in the box it was in. */}
            <button
              type="button"
              className="contest-logger-step"
              aria-label={t('contestLogger.size.smaller')}
              title={t('contestLogger.size.smaller')}
              disabled={textScale <= BOX_SCALE_MIN}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => stepText(-1)}
            >
              A−
            </button>
            <button
              type="button"
              className="contest-logger-step"
              aria-label={t('contestLogger.size.larger')}
              title={t('contestLogger.size.larger')}
              disabled={textScale >= BOX_SCALE_MAX}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => stepText(1)}
            >
              A+
            </button>
          </div>
          <div className="contest-logger-scaled" style={scaled}>
            <LogEntry
              snap={snap}
              mode={m.mode}
              defaultRst={m.defaultRst}
              exchange="terrestrial"
              titled={false}
              fieldDay={fd}
              fdMode={m.fdMode}
              fdSubmode={m.fdSubmode}
              sharedEntry="logger"
            />
          </div>
        </section>
      )}
      {fd && <LastContacts fieldDay={fd} style={scaled} />}
      <main className="layout single">
        <ContestView
          fieldDay={fd}
          fdActive={fdActive}
          fdRuleset={fdRuleset}
          tier={snap.link.tier}
          logger
          contestSwitch={contestSwitch}
          operatorAsk={operatorAsk}
          onOperatorFocused={() => setOperatorAsk(0)}
        />
      </main>
    </>
  )
}
