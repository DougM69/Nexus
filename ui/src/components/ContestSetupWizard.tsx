// ---------------------------------------------------------------------------
// ⭐ THE CONTEST SETUP — the one way into a contest: pick it, fill in what it sends, declare the
// entry its Cabrillo log writes, check the radio, and Start. Built like the first-run setup wizard
// (a dialog with steps); "Change setup…" on the Contest screen reopens it at the exchange.
//
// It changes nothing the engine does. It writes the SAME settings Settings ▸ Contesting writes,
// through the patch seam (`patchSettings`, read-then-patch, never a whole-form save), in ONE save
// with Contest mode on — the save that enters the contest in S&P, as the Contest screen's switch
// does. Cancel, Esc or Back saves nothing: until Start, every answer is a draft, and the preview of
// it (`preview_fd_ruleset`) is read-only.
//
// ⭐ STARTED MEANS ENTERED. After the save it reads the snapshot that save returned: the engine in
// the contest, on the picked one. If the engine did not enter, it says why in the engine's own words
// and turns Contest mode off again, so the setup can never leave "on, but not running".
//
// ⛔ IT NEVER TRANSMITS. Start always enters S&P; Running stays the operator's own press. Nothing here
// calls `set_mode`, and a banned mode on the station is a warning with a way to CW or Phone, never a
// refusal (the advisories are warn-only by operator ruling).
//
// ⚠️ A CONTEST ALREADY RUNNING ends when a different one starts (`save_switches_contest`); its
// contacts stay in its own journal. The pick step says so and asks for a tick before it goes on.
// ---------------------------------------------------------------------------
import { useEffect, useRef, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import type { AppSnapshot, Settings } from '../types'
import { contestWindows, getFdRuleset, getSnapshot, getSettings, previewFdRuleset, type ContestWindow, type FdRulesetDto } from '../api'
import { patchSettings } from '../settings/patch'
import { CONTESTS, contestName, contestRoleWords } from '../fdEvent'
import { blockerText, startBlocker } from '../features/contestStart'
import { locationWarningText } from '../features/contestLocation'
import { stationMode } from '../features/contestMode'
import { askMainToWorkIn, type WorkInView } from '../features/contestWorkIn'
import { upperTyped } from '../typedInput'

/** Raised on this window after the setup saves, for App to read the settings and snapshot again (the
 *  Contests pane hosts the setup and has no line to App of its own). */
export const CONTEST_SETUP_SAVED_EVENT = 'nexus:contest-setup-saved'

const DAY = 86_400
/** How far ahead a contest counts as "this week": first in the pick list, and the quiet line. */
export const SOON_SECS = 7 * DAY
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/** A UTC time as a contest calendar writes it: "Oct 25 1400Z". Tokens, never translated. */
export function utcStamp(unix: number): string {
  const d = new Date(unix * 1000)
  const hhmm = `${String(d.getUTCHours()).padStart(2, '0')}${String(d.getUTCMinutes()).padStart(2, '0')}Z`
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()} ${hhmm}`
}

/** The contest this week, for the quiet line: the soonest one on the picker that STARTS within seven
 *  days of `now` — not one already running, nor one further off. Null when there is none. */
export function contestSoon(windows: readonly ContestWindow[], now: number): ContestWindow | null {
  const offered = new Set(CONTESTS.map((c) => c.id))
  const soon = windows
    .filter((w) => offered.has(w.id) && w.startUnix > now && w.startUnix - now <= SOON_SECS)
    .sort((a, b) => a.startUnix - b.startUnix)
  return soon[0] ?? null
}

/** ARRL Field Day's power, asked once: the Cabrillo power category and the score's multiplier both
 *  follow from it. ×5 needs QRP AND battery or other natural power; QRP on mains is ×2. */
export type FdPower = 'QRP' | 'LOW' | 'HIGH'
export function fdPowerAnswer(power: FdPower, battery: boolean): Pick<Settings, 'contestCategoryPower' | 'fdPowerMult'> {
  const fdPowerMult = power === 'HIGH' ? 1 : power === 'QRP' && battery ? 5 : 2
  return { contestCategoryPower: power, fdPowerMult }
}
/** The answer the stored settings already give: the declared category, else the multiplier's. */
export function fdPowerFrom(s: Partial<Settings>): { power: FdPower; battery: boolean } {
  const declared = (s.contestCategoryPower ?? '').trim().toUpperCase()
  const mult = s.fdPowerMult ?? 1
  const power: FdPower =
    declared === 'QRP' || declared === 'LOW' || declared === 'HIGH' ? declared : mult === 5 ? 'QRP' : mult === 2 ? 'LOW' : 'HIGH'
  return { power, battery: mult === 5 }
}

/** Did the save enter the picked contest? The snapshot the save returned is the engine's answer. */
export function enteredContest(snap: AppSnapshot | null | undefined, event: string): boolean {
  return snap?.mode === 'fieldDay' && (snap.fieldDay?.event || 'arrlfd') === event
}

/** Every setting this setup can write: what a host may hand it as unsaved answers. */
export const SETUP_KEYS: readonly (keyof Settings)[] = [
  'fdEvent',
  'fdClass',
  'fdSection',
  'contestQthCounty',
  'contestQthState',
  'contestCheck',
  'contestCqZone',
  'contestItuZone',
  'contestPower',
  'mycall',
  'mygrid',
  'contestCategoryOperator',
  'contestCategoryPower',
  'contestCategoryAssisted',
  'contestCategoryStation',
  'contestCategoryTransmitter',
  'contestCategoryBand',
  'contestCategoryOverlay',
  'contestEntryClass',
  'contestClub',
  'contestOperators',
  'contestEmail',
  'fdPowerMult',
]

type Step = 0 | 1 | 2 | 3 | 4
// One key per step, read by the dot and the progress label alike, as the first-run setup does.
const STEPS: { titleKey: MessageKey }[] = [
  { titleKey: 'contestSetup.steps.pick' },
  { titleKey: 'contestSetup.steps.sends' },
  { titleKey: 'contestSetup.steps.entry' },
  { titleKey: 'contestSetup.steps.ready' },
  { titleKey: 'contestSetup.steps.start' },
]

/** A sent setting by its Rust name: the field it is and the box that fills it. */
type SendBox = { key: keyof Settings; label: () => string; zone?: boolean }
function sendBox(name: string, event: string): SendBox | null {
  switch (name) {
    case 'fd_class':
      return {
        key: 'fdClass',
        label: () => (event === 'wfd' ? t('settings.fieldDay.category.label') : t('settings.fieldDay.class.label')),
      }
    case 'fd_section':
      return { key: 'fdSection', label: () => t('settings.fieldDay.section.label') }
    case 'contest_qth_county':
      return { key: 'contestQthCounty', label: () => t('settings.contestStation.county.label') }
    case 'contest_qth_state':
      return { key: 'contestQthState', label: () => t('settings.contestStation.state.label') }
    case 'contest_check':
      return { key: 'contestCheck', label: () => t('settings.contestStation.check.label') }
    case 'contest_cq_zone':
      return { key: 'contestCqZone', label: () => t('settings.contestStation.cqZone.label'), zone: true }
    case 'contest_itu_zone':
      return { key: 'contestItuZone', label: () => t('settings.contestStation.ituZone.label'), zone: true }
    case 'contest_power':
      return { key: 'contestPower', label: () => t('settings.contestStation.power.label') }
    case 'mycall':
      return { key: 'mycall', label: () => t('settings.fdWho.call.label') }
    case 'mygrid':
      return { key: 'mygrid', label: () => t('setup.station.grid.label') }
    default:
      return null
  }
}

/** An entry declaration (a Cabrillo CATEGORY-* line): its field, its label and the sponsor's tokens.
 *  `''` is "not set". The tokens are invariant: they go in the file verbatim. */
type Axis = { key: keyof Settings; label: () => string; tokens: readonly string[] }
const OPERATOR_AXIS: Axis = {
  key: 'contestCategoryOperator',
  label: () => t('settings.contestPick.category.label'),
  tokens: ['SINGLE-OP', 'MULTI-OP', 'CHECKLOG'],
}
const CATEGORY_AXES: Record<string, Axis> = {
  contest_category_power: { key: 'contestCategoryPower', label: () => t('settings.contestPick.power.label'), tokens: ['', 'HIGH', 'LOW', 'QRP'] },
  contest_category_assisted: {
    key: 'contestCategoryAssisted',
    label: () => t('settings.contestPick.assisted.label'),
    tokens: ['', 'NON-ASSISTED', 'ASSISTED'],
  },
  contest_category_station: { key: 'contestCategoryStation', label: () => t('settings.contestPick.station.label'), tokens: ['', 'SCHOOL'] },
}
/** The header each category axis writes, for "does this contest's log carry it". */
const AXIS_HEADER: Record<string, string> = {
  contest_category_power: 'CATEGORY-POWER',
  contest_category_assisted: 'CATEGORY-ASSISTED',
  contest_category_station: 'CATEGORY-STATION',
}

function AxisSelect({
  axis,
  value,
  onChange,
  tokens,
}: {
  axis: Axis
  value: string
  onChange: (v: string) => void
  tokens?: readonly string[]
}) {
  const list = tokens ?? axis.tokens
  return (
    <label className="wizard-field">
      <span>{axis.label()}</span>
      <select value={list.includes(value) ? value : ''} onChange={(e) => onChange(e.target.value)}>
        {(list.includes('') ? list : ['', ...list]).map((c) => (
          <option key={c || 'unset'} value={c}>
            {c || t('contestSetup.entry.unset')}
          </option>
        ))}
      </select>
    </label>
  )
}

export function ContestSetupWizard({
  onClose,
  startAt = 'pick',
  initial,
  onSaved,
  onWorkIn,
}: {
  onClose: () => void
  /** Answers to start from over the saved settings: the quiet line's contest, or what Settings' form
   *  holds and has not saved (only the fields this setup asks, `SETUP_KEYS`). */
  initial?: Partial<Settings>
  /** `sends` for "Change setup…" on a running contest: the exchange first, the contest as it is. */
  startAt?: 'pick' | 'sends'
  /** After a save, with the fields it wrote, for a host that holds its own copy (Settings' form). */
  onSaved?: (written: Partial<Settings>) => void
  /** The main window's own switch to a cockpit; elsewhere a press asks the main window. */
  onWorkIn?: (view: WorkInView) => void
}) {
  const [base, setBase] = useState<Settings | null>(null)
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  const [windows, setWindows] = useState<ContestWindow[] | null>(null)
  const [draft, setDraft] = useState<Partial<Settings>>(() => {
    const out: Record<string, unknown> = {}
    for (const k of SETUP_KEYS) if (initial && k in initial) out[k] = initial[k]
    return out as Partial<Settings>
  })
  const [preview, setPreview] = useState<FdRulesetDto | null>(null)
  const [previewing, setPreviewing] = useState(true)
  const [step, setStep] = useState<Step>(startAt === 'sends' ? 1 : 0)
  const [switchOk, setSwitchOk] = useState(false)
  const [busy, setBusy] = useState(false)
  const [refused, setRefused] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [started, setStarted] = useState<{ event: string; running: boolean } | null>(null)

  useEffect(() => {
    let live = true
    getSettings()
      .then((s) => live && setBase(s))
      .catch((e) => live && setError(String(e)))
    getSnapshot()
      .then((s) => live && setSnap(s ?? null))
      .catch(() => {})
    contestWindows()
      .then((w) => live && setWindows(Array.isArray(w) ? w : []))
      .catch(() => live && setWindows([]))
    return () => {
      live = false
    }
  }, [])

  // The preview of the answers so far: read-only, the newest asked wins.
  const asked = useRef(0)
  const draftKey = JSON.stringify(draft)
  useEffect(() => {
    if (!base) return
    const n = ++asked.current
    setPreviewing(true)
    previewFdRuleset(draft)
      .then((p) => n === asked.current && setPreview(p ?? null))
      .catch(() => n === asked.current && setPreview(null))
      .finally(() => n === asked.current && setPreviewing(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [base, draftKey])

  const merged: Partial<Settings> = { ...(base ?? {}), ...draft }
  const event = (merged.fdEvent ?? '').trim() || 'arrlfd'
  const name = contestName(event)
  const session = snap?.fieldDay ?? null
  const runningEvent = session ? session.event || 'arrlfd' : null
  const switching = runningEvent !== null && runningEvent !== event
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => setDraft((d) => ({ ...d, [key]: value }))
  const str = (key: keyof Settings): string => String(merged[key] ?? '')

  // What keeps the contest from starting, in the engine's words, for the answers so far. Field Day's
  // blanks are known before the preview answers; any other contest waits for it.
  const blocker = base
    ? startBlocker({ fdEvent: event, fdClass: str('fdClass'), fdSection: str('fdSection') }, preview?.event === event ? preview : null)
    : null
  const exchangeHeld = !base || previewing || blocker !== null
  const canLeave = (i: number) => (i >= 1 && switching && !switchOk ? false : i >= 2 ? !exchangeHeld : true)

  const sent = preview?.event === event ? (preview.sentSettings ?? []) : []
  const sendBoxes = sent.map((n) => sendBox(n, event)).filter((b): b is SendBox => b !== null)
  const sentAxes = sent.filter((n) => n in CATEGORY_AXES)
  const headers = preview?.cabrilloHeaders ?? []
  const multiOp = str('contestCategoryOperator') === 'MULTI-OP'
  const arrlFd = event === 'arrlfd'
  const fdPower = fdPowerFrom(merged)
  const entryAxes = Object.keys(CATEGORY_AXES).filter(
    (n) =>
      !sentAxes.includes(n) &&
      !(arrlFd && n === 'contest_category_power') &&
      (headers.includes(AXIS_HEADER[n]) || (n === 'contest_category_power' && (preview?.powerCategories?.length ?? 0) > 0)),
  )

  // Step 4: the station as it is now. Read-only; nothing here is a refusal.
  const mode = snap ? stationMode(snap.radio.operatingMode, snap.link?.tier, null) : ''
  const banned = !!mode && (preview?.bannedModes ?? []).some((m) => m.toUpperCase() === mode.toUpperCase())
  const catOk = snap?.radio.catOk
  const workIn = (view: WorkInView) => (onWorkIn ?? askMainToWorkIn)(view)

  const start = async () => {
    setBusy(true)
    setError(null)
    setRefused(null)
    const written: Partial<Settings> = { ...draft, fdActive: true }
    try {
      const after = await patchSettings(() => written)
      onSaved?.(written)
      window.dispatchEvent(new CustomEvent(CONTEST_SETUP_SAVED_EVENT))
      if (enteredContest(after, event)) {
        setStarted({ event, running: after.fieldDay?.running === true })
        return
      }
      // The engine did not enter. Never leave the mode saved on with nothing running: turn it off
      // again, and say why in the engine's own words, read against what is saved now.
      const why = await getFdRuleset().catch(() => null)
      const saved = await getSettings().catch(() => null)
      const b = startBlocker(saved, why)
      await patchSettings(() => ({ fdActive: false }))
      onSaved?.({ fdActive: false })
      window.dispatchEvent(new CustomEvent(CONTEST_SETUP_SAVED_EVENT))
      setRefused(
        b
          ? t('contestSetup.start.refused', { contest: name, reason: blockerText(b, name) })
          : t('contestSetup.start.refusedUnknown', { contest: name }),
      )
      setStep(1)
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const title = startAt === 'sends' ? t('contestSetup.titleChange') : t('contestSetup.title')
  const now = Date.now() / 1000
  const windowOf = (id: string) => windows?.find((w) => w.id === id)
  const soonFirst = [...CONTESTS].sort((a, b) => (windowOf(a.id)?.startUnix ?? Infinity) - (windowOf(b.id)?.startUnix ?? Infinity))
  const isSoon = (id: string) => {
    const w = windowOf(id)
    return !!w && w.startUnix - now <= SOON_SECS
  }
  const groups = [
    { key: 'soon', label: t('contestSetup.pick.soon'), list: soonFirst.filter((c) => isSoon(c.id)) },
    { key: 'later', label: t('contestSetup.pick.later'), list: soonFirst.filter((c) => !isSoon(c.id)) },
  ]

  if (started) {
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()} title={title} className="contest-setup">
        <div className="contest-setup-done" role="status">
          <p className="contest-setup-on">
            {started.running
              ? t('contestSetup.start.onRunning', { contest: contestName(started.event) })
              : t('contestSetup.start.on', { contest: contestName(started.event) })}
          </p>
          {!started.running && <p className="wizard-sub">{t('contestSetup.start.onHint')}</p>}
        </div>
        <div className="wizard-actions">
          <div className="wizard-actions-right">
            <button type="button" className="wizard-go" onClick={onClose} autoFocus>
              {t('contestSetup.nav.close')}
            </button>
          </div>
        </div>
      </Dialog>
    )
  }

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o && !busy) onClose()
      }}
      title={title}
      className="contest-setup"
    >
      <div
        className="wizard-dots"
        aria-label={t('contestSetup.steps.aria', { n: step + 1, total: STEPS.length, title: t(STEPS[step].titleKey) })}
      >
        {STEPS.map(({ titleKey: k }, i) => (
          <button
            key={k}
            type="button"
            className={`wizard-dot${i === step ? ' cur' : ''}${i < step ? ' done' : ''}`}
            disabled={busy || (i > step && !STEPS.slice(0, i).every((_, j) => j === 0 || canLeave(j)))}
            onClick={() => setStep(i as Step)}
          >
            <span className="wizard-dot-n">{i + 1}</span> {t(k)}
          </button>
        ))}
      </div>

      {step === 0 && (
        <>
          <h2 className="wizard-title">{t('contestSetup.pick.title')}</h2>
          <p className="wizard-sub">{t('contestSetup.pick.sub')}</p>
          {groups.map(
            (g) =>
              g.list.length > 0 && (
                <div key={g.key} className="contest-setup-group" role="radiogroup" aria-label={g.label}>
                  <h3 className="wizard-modes-title">{g.label}</h3>
                  {g.list.map((c) => {
                    const w = windowOf(c.id)
                    const on = c.id === event
                    return (
                      <button
                        key={c.id}
                        type="button"
                        role="radio"
                        aria-checked={on}
                        className={`contest-setup-pick${on ? ' sel' : ''}`}
                        onClick={() => {
                          set('fdEvent', c.id)
                          setSwitchOk(false)
                        }}
                      >
                        <span className="contest-setup-pick-name">{c.name}</span>
                        {w && (
                          <span className="contest-setup-pick-when">
                            {w.startUnix <= now
                              ? t('contestSetup.pick.running', { end: utcStamp(w.endUnix) })
                              : t('contestSetup.pick.window', { start: utcStamp(w.startUnix), end: utcStamp(w.endUnix) })}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </div>
              ),
          )}
          {switching && runningEvent && (
            <div className="contest-setup-warn" role="status">
              <p>{t('contestSetup.pick.switch', { running: contestName(runningEvent), contest: name })}</p>
              <label className="contest-setup-tick">
                <input type="checkbox" checked={switchOk} onChange={() => setSwitchOk(!switchOk)} />
                <span>{t('contestSetup.pick.switch.confirm', { running: contestName(runningEvent), contest: name })}</span>
              </label>
            </div>
          )}
        </>
      )}

      {step === 1 && (
        <>
          <h2 className="wizard-title">{t('contestSetup.sends.title', { contest: name })}</h2>
          <p className="wizard-sub">{t('contestSetup.sends.sub')}</p>
          {refused && (
            <p className="contest-setup-bad" role="alert">
              {refused}
            </p>
          )}
          <div className="wizard-fields">
            {sendBoxes.map((b) => (
              <label key={b.key} className="wizard-field">
                <span>{b.label()}</span>
                {b.zone ? (
                  <input
                    type="number"
                    min={0}
                    max={b.key === 'contestCqZone' ? 40 : 90}
                    value={Number(merged[b.key] ?? 0) || ''}
                    onChange={(e) => set(b.key, (Number(e.target.value) || 0) as never)}
                  />
                ) : (
                  <input
                    type="text"
                    className="mono"
                    value={str(b.key)}
                    onChange={(e) => set(b.key, upperTyped(e.target) as never)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                )}
              </label>
            ))}
            {sentAxes.map((n) => {
              const axis = CATEGORY_AXES[n]
              return <AxisSelect key={n} axis={axis} value={str(axis.key)} onChange={(v) => set(axis.key, v as never)} />
            })}
            {sentAxes.length > 0 && (
              <AxisSelect
                axis={OPERATOR_AXIS}
                value={str('contestCategoryOperator') || 'SINGLE-OP'}
                onChange={(v) => set('contestCategoryOperator', v)}
              />
            )}
          </div>
          {!previewing && sendBoxes.length === 0 && sentAxes.length === 0 && (
            <p className="wizard-sub">{t('contestSetup.sends.none')}</p>
          )}
          {previewing ? (
            <p className="wizard-field-hint">{t('contestSetup.sends.checking')}</p>
          ) : blocker ? (
            <p className="contest-setup-bad" role="alert" data-blocker>
              {blockerText(blocker, name)}
            </p>
          ) : preview?.exchange?.length ? (
            <p className="contest-setup-exchange">
              {preview.role ? `${t('settings.contestPick.role.value', { role: contestRoleWords(preview.role) })} ` : ''}
              <span className="mono">{t('contestSetup.sends.exchange', { exchange: preview.exchange.join(' ') })}</span>
            </p>
          ) : null}
          {!previewing && preview?.locationWarning && (
            <p className="contest-setup-warn" role="status">
              {locationWarningText(preview.locationWarning)}
            </p>
          )}
        </>
      )}

      {step === 2 && (
        <>
          <h2 className="wizard-title">{t('contestSetup.entry.title')}</h2>
          <p className="wizard-sub">{t('contestSetup.entry.sub', { contest: name })}</p>
          {arrlFd && (
            <div className="contest-setup-group" role="radiogroup" aria-label={t('contestSetup.fdPower.aria')}>
              <h3 className="wizard-modes-title">{t('contestSetup.fdPower.label')}</h3>
              {(['QRP', 'LOW', 'HIGH'] as const).map((p) => (
                <button
                  key={p}
                  type="button"
                  role="radio"
                  aria-checked={fdPower.power === p}
                  className={`contest-setup-pick${fdPower.power === p ? ' sel' : ''}`}
                  onClick={() => setDraft((d) => ({ ...d, ...fdPowerAnswer(p, fdPower.battery) }))}
                >
                  {p === 'QRP' ? t('contestSetup.fdPower.qrp') : p === 'LOW' ? t('contestSetup.fdPower.low') : t('contestSetup.fdPower.high')}
                </button>
              ))}
              {fdPower.power === 'QRP' && (
                <>
                  <label className="contest-setup-tick">
                    <input
                      type="checkbox"
                      checked={fdPower.battery}
                      onChange={() => setDraft((d) => ({ ...d, ...fdPowerAnswer('QRP', !fdPower.battery) }))}
                    />
                    <span>{t('contestSetup.fdPower.battery')}</span>
                  </label>
                  {!fdPower.battery && <p className="wizard-field-hint">{t('contestSetup.fdPower.noBattery')}</p>}
                </>
              )}
            </div>
          )}
          <div className="wizard-fields">
            {sentAxes.length === 0 && (
              <AxisSelect
                axis={OPERATOR_AXIS}
                value={str('contestCategoryOperator') || 'SINGLE-OP'}
                onChange={(v) => set('contestCategoryOperator', v)}
              />
            )}
            {entryAxes.map((n) => {
              const axis = CATEGORY_AXES[n]
              const only = n === 'contest_category_power' && preview?.powerCategories?.length ? preview.powerCategories : undefined
              return <AxisSelect key={n} axis={axis} tokens={only} value={str(axis.key)} onChange={(v) => set(axis.key, v as never)} />
            })}
            {multiOp && (preview?.transmitterCategories?.length ?? 0) > 0 && (
              <AxisSelect
                axis={{ key: 'contestCategoryTransmitter', label: () => t('settings.contestPick.transmitter.label'), tokens: preview!.transmitterCategories! }}
                value={str('contestCategoryTransmitter')}
                onChange={(v) => set('contestCategoryTransmitter', v)}
              />
            )}
            {(preview?.bandCategories?.length ?? 0) > 0 && (
              <AxisSelect
                axis={{ key: 'contestCategoryBand', label: () => t('settings.contestPick.band.label'), tokens: preview!.bandCategories! }}
                value={str('contestCategoryBand')}
                onChange={(v) => set('contestCategoryBand', v)}
              />
            )}
            {(preview?.overlayCategories?.length ?? 0) > 0 && (
              <AxisSelect
                axis={{ key: 'contestCategoryOverlay', label: () => t('settings.contestPick.overlay.label'), tokens: preview!.overlayCategories! }}
                value={str('contestCategoryOverlay')}
                onChange={(v) => set('contestCategoryOverlay', v)}
              />
            )}
            {(preview?.entryClasses?.length ?? 0) > 0 && (
              <AxisSelect
                axis={{ key: 'contestEntryClass', label: () => t('settings.contestPick.entryClass.label'), tokens: preview!.entryClasses! }}
                value={str('contestEntryClass')}
                onChange={(v) => set('contestEntryClass', v)}
              />
            )}
            {headers.includes('CLUB') && (
              <label className="wizard-field">
                <span>{t('settings.contestPick.club.label')}</span>
                <input type="text" value={str('contestClub')} onChange={(e) => set('contestClub', e.target.value)} autoComplete="off" />
              </label>
            )}
            {(headers.includes('OPERATORS') || (multiOp && (preview?.transmitterCategories?.length ?? 0) > 0)) && (
              <label className="wizard-field">
                <span>{t('settings.contestPick.operators.label')}</span>
                <input
                  type="text"
                  className="mono"
                  value={str('contestOperators')}
                  onChange={(e) => set('contestOperators', upperTyped(e.target))}
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
            )}
            {headers.includes('EMAIL') && (
              <label className="wizard-field">
                <span>{t('settings.contestPick.email.label')}</span>
                <input type="email" value={str('contestEmail')} onChange={(e) => set('contestEmail', e.target.value)} autoComplete="off" />
              </label>
            )}
          </div>
          {session && <p className="wizard-field-hint">{t('contestSetup.entry.readAtStart')}</p>}
        </>
      )}

      {step === 3 && (
        <>
          <h2 className="wizard-title">{t('contestSetup.ready.title')}</h2>
          <p className="wizard-sub">{t('contestSetup.ready.sub')}</p>
          <ul className="contest-setup-checks">
            <li className={catOk === false ? 'warn' : undefined}>
              {catOk === true ? t('contestSetup.ready.catOk') : catOk === false ? t('contestSetup.ready.catBad') : t('contestSetup.ready.catNone')}
            </li>
            {mode && <li>{t('contestSetup.ready.mode', { mode })}</li>}
            {banned && (
              <li className="warn" data-banned>
                {t('contestSetup.ready.banned', { contest: name, mode })}{' '}
                <span className="contest-setup-workin">
                  <button type="button" className="settings-linkbtn" onClick={() => workIn('cw')}>
                    {t('fieldDay.workIn.cw')}
                  </button>{' '}
                  <button type="button" className="settings-linkbtn" onClick={() => workIn('phone')}>
                    {t('fieldDay.workIn.phone')}
                  </button>
                </span>
              </li>
            )}
            <li>{t('contestSetup.ready.sp')}</li>
          </ul>
        </>
      )}

      {step === 4 && (
        <>
          <h2 className="wizard-title">{t('contestSetup.start.title', { contest: name })}</h2>
          <p className="wizard-sub">{t('contestSetup.start.sub')}</p>
          {preview?.exchange?.length ? (
            <p className="contest-setup-exchange mono">{t('contestSetup.sends.exchange', { exchange: preview.exchange.join(' ') })}</p>
          ) : null}
          {switching && runningEvent && (
            <p className="contest-setup-warn">{t('contestSetup.pick.switch', { running: contestName(runningEvent), contest: name })}</p>
          )}
        </>
      )}

      {error && (
        <p className="contest-setup-bad" role="alert">
          {error}
        </p>
      )}

      <div className="wizard-actions">
        <div className="wizard-actions-right">
          {step > 0 && (
            <button type="button" className="wizard-skip" disabled={busy} onClick={() => setStep((step - 1) as Step)}>
              {t('contestSetup.nav.back')}
            </button>
          )}
          <button type="button" className="wizard-skip" disabled={busy} onClick={onClose}>
            {t('contestSetup.nav.cancel')}
          </button>
          {step < 4 ? (
            <button
              type="button"
              className="wizard-go"
              disabled={!canLeave(step + 1)}
              onClick={() => setStep((step + 1) as Step)}
            >
              {t('contestSetup.nav.next')}
            </button>
          ) : (
            <button type="button" className="wizard-go" disabled={busy || exchangeHeld || (switching && !switchOk)} onClick={() => void start()}>
              {busy
                ? t('contestSetup.start.busy')
                : runningEvent === event
                  ? t('contestSetup.start.save')
                  : t('contestSetup.start.button', { contest: name })}
            </button>
          )}
        </div>
      </div>
    </Dialog>
  )
}

/** ⭐ THE QUIET LINE: "<contest> this weekend — Set up…", when a contest on the picker starts within
 *  seven days and none is running. No pop-up: it is a line where contests are shown, and its button
 *  opens the setup. */
export function ContestSoonLine({ onWorkIn }: { onWorkIn?: (view: WorkInView) => void }) {
  const [soon, setSoon] = useState<ContestWindow | null>(null)
  const [open, setOpen] = useState(false)
  useEffect(() => {
    let live = true
    // A passive line: whatever the station cannot answer (an older one, a host that lends no bridge),
    // the screen it sits on goes on without it.
    try {
      Promise.all([contestWindows(), getSnapshot().catch(() => null)])
        .then(([w, s]) => live && setSoon(s?.fieldDay || !Array.isArray(w) ? null : contestSoon(w, Date.now() / 1000)))
        .catch(() => {})
    } catch {
      // no line
    }
    return () => {
      live = false
    }
  }, [])
  if (!soon) return null
  return (
    <>
      <p className="contest-soon" role="status">
        {t('contestSetup.soon', { contest: contestName(soon.id), when: utcStamp(soon.startUnix) })}{' '}
        <button type="button" className="settings-linkbtn" onClick={() => setOpen(true)}>
          {t('contestSetup.soon.action')}
        </button>
      </p>
      {open && <ContestSetupWizard initial={{ fdEvent: soon.id }} onClose={() => setOpen(false)} onWorkIn={onWorkIn} />}
    </>
  )
}
