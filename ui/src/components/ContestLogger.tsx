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
// and the contest screen (band board, score, log) scrolls below. A− / A+ in the bar over the
// entry sizes the entry and the last contacts together, and this computer remembers it for this
// window. The sizes are styles.css's, scoped to the entry: the cockpits' log lines are untouched.
//
// ⭐ THE OPERATOR BOX IS ON THE ENTRY'S TITLE LINE, beside Sending and I moved (the operator,
// 2026-10-12: "I want to move the field to change operators up near the top"), in both windows, and
// the screen below draws none, so there is one. Ctrl+O from the log line lands in it. It writes the
// operator through the host's narrow save, never the whole-settings one, and its Enter is its own:
// it logs nothing.
//
// ⭐ AND THE SAME ENTRY TOPS THE MAIN WINDOW'S CONTEST SCREEN (the operator's pick, 2026-10-11: "The
// Logger window's large Call / RST / County boxes, dupe warning and last contacts at the top of the
// Contest screen"). It is one component, `ContestLargeEntry`, mounted by this window and by App: there
// it is the main window's contest strip, so it shares the contact as the cockpits' strips do, Esc there
// is the main window's stop, and it transmits nothing of its own either.
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
import type { FdRulesetDto } from '../api'
import { isFieldDay } from '../fdEvent'
import { BOX_SCALE_MAX, BOX_SCALE_MIN, BOX_SCALE_STEP, boxScaleValue } from '../features/panelState'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
import type { AppSnapshot, FieldDayStatus } from '../types'
import { useContestSwitch } from '../contestSwitch'
import { useOperatorBoxAsk } from '../features/contestKeys'
import { contestLogMode, stationMode, usePskName } from '../features/contestMode'
import { annotate, ContestLogHead, ContestLogRow, ContestView, OperatorBox } from './ContestView'
import { NoCreditChip } from './FdAdvisories'
import { LogEntry } from './LogEntry'

/** Where the entry keeps its text size. Per surface (`windowScope`): each window its own, on this
 *  computer — the main window's Contest screen under the bare key, this window under its own, which
 *  starts from the main window's until its first press here. */
const TEXT_SCALE_KEY = 'nexus.contestLogger.textScale'

/** How many of the newest contacts the entry lists under its boxes. */
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
        <ContestLogHead fieldDayColumns={fieldDayColumns} isWfd={fieldDay.event === 'wfd'} slotColumns={slotColumns} role={fieldDay.role} />
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

/**
 * ⭐ THE LARGE LOG LINE — the bar (its name and A− / A+), the contest strip at a size of its own with
 * the dupe warning right under the boxes, and the last contacts. The logger window's top, and the top
 * of the main window's Contest screen: one component, so the two cannot drift.
 *
 * It logs under the station's operating section (`contestLogMode`), as the cockpit there logs, and
 * says so when that is a mode the contest does not permit: "FT8 · no credit" on its top row, right
 * after the mode chip, with the header chip's own sentence on hover, for the same mode
 * (`stationMode`). A chip and not a band over the boxes, the operator's pick (2026-10-12). Logging
 * stays allowed; the contact is the sponsor's to score.
 *
 * ⛔ NOTHING HERE TRANSMITS: the strip gets no `esm`, and A− / A+ only size the text. `sharedEntry`
 * is the logger window's (`LogEntry`): there Esc clears the entry. Absent, in the main window, Esc
 * stays the window's stop (App's), and the strip takes part in the shared contact as the cockpits'
 * strips do.
 */
export function ContestLargeEntry({
  snap,
  fieldDay,
  fdActive,
  fdRuleset,
  title,
  sharedEntry,
  operator,
  onSaveOperator,
  operatorAsk,
  onOperatorFocused,
}: {
  snap: AppSnapshot
  fieldDay: FieldDayStatus
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
  /** The bar's name for the entry: the window's own in the logger window, the screen's in the main. */
  title: string
  sharedEntry?: 'logger'
  /** The operator at the key as this window's settings hold it (`fdOperator`), or null until they
   *  are read. The box on the title line shows it. */
  operator: string | null
  /** The window's narrow operator write (`setFdOperator`), never the whole-settings save. */
  onSaveOperator: (call: string) => void | Promise<unknown>
  /** Ctrl+O: the asks the operator box has not answered (`features/contestKeys.ts`). */
  operatorAsk?: number
  onOperatorFocused?: () => void
}) {
  const section = snap.radio.operatingMode
  const pskName = usePskName(section)
  const m = contestLogMode(section, pskName)
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
      <section className="contest-logger-entry" aria-label={t('contestLogger.entry.aria')}>
        <div className="contest-logger-bar">
          <span className="contest-logger-title">{title}</span>
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
            fieldDay={fieldDay}
            fdMode={m.fdMode}
            fdSubmode={m.fdSubmode}
            sharedEntry={sharedEntry}
            noCreditChip={
              <NoCreditChip fdActive={fdActive} ruleset={fdRuleset} activeMode={stationMode(section, snap.link.tier, pskName)} />
            }
            operatorBox={
              <OperatorBox
                onLogLine
                value={operator}
                onSave={onSaveOperator}
                operatorAsk={operatorAsk}
                onOperatorFocused={onOperatorFocused}
              />
            }
          />
        </div>
      </section>
      <LastContacts fieldDay={fieldDay} style={scaled} />
    </>
  )
}

export function ContestLogger({
  snap,
  fdActive,
  fdRuleset,
  operator,
  onSaveOperator,
}: {
  snap: AppSnapshot
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
  /** This window's operator at the key and its narrow write (DetachedPanel's): the entry's box. */
  operator: string | null
  onSaveOperator: (call: string) => void | Promise<unknown>
}) {
  // THE F-KEYS DO NOTHING HERE. In the main window they send messages; in this one they are not
  // bound to anything, and the webview's own F-key actions (F5 reloads the page) are held too.
  // With Alt, Ctrl or Cmd held a key is the system's (Alt+F4 still closes the window), as on the
  // main window's Contest screen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key) && !e.altKey && !e.ctrlKey && !e.metaKey) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const fd = snap.fieldDay ?? null
  // CTRL+O FROM THE LOG LINE (`features/contestKeys.ts`): the operator box is this window's own, on
  // the entry's title line while a contest runs, else in its copy of the Contest screen below.
  const [operatorAsk, setOperatorAsk] = useState(0)
  useOperatorBoxAsk(() => setOperatorAsk((n) => n + 1))
  // A contest switched by a pick saved in the main window: this copy of the Contest screen says
  // so too.
  const contestSwitch = useContestSwitch(fd)
  return (
    <>
      {fd && (
        <ContestLargeEntry
          snap={snap}
          fieldDay={fd}
          fdActive={fdActive}
          fdRuleset={fdRuleset}
          title={t('contestLogger.title')}
          sharedEntry="logger"
          operator={operator}
          onSaveOperator={onSaveOperator}
          operatorAsk={operatorAsk}
          onOperatorFocused={() => setOperatorAsk(0)}
        />
      )}
      <main className="layout single">
        <ContestView
          fieldDay={fd}
          fdActive={fdActive}
          fdRuleset={fdRuleset}
          tier={snap.link.tier}
          section={snap.radio.operatingMode}
          logger
          contestSwitch={contestSwitch}
          operatorAsk={operatorAsk}
          onOperatorFocused={() => setOperatorAsk(0)}
          operatorBoxAbove={fd != null}
        />
      </main>
    </>
  )
}
