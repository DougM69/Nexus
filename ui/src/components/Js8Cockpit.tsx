// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), for ONE reason:
// the TX-on-air pill's tooltip, which states what Stop TX does to a frame in flight. That is
// transmit-path wording and moves with the JS8 TX batch (B7), with the stop-line sweeps re-run.
// Everything else operator-visible is in the catalog under `js8.*`. What is NOT prose and stays
// in the code is the mode's own vocabulary (js8Vocab.ts): JS8, HB, CQ, @ALLCALL, the speed
// names and their ALL.TXT letters, the 32 directed-command texts, callsigns, grids, offsets in
// Hz, SNR in dB, UTC stamps and the s/m/h age units.
import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { useStationCapability, useStationControl, useStationData } from '../stationAccess'
import { useEscStop } from '../useEscStop'
import { useJs8Context } from '../remote-web/useJs8Context'
import { useDecoderSettings } from '../remote-web/useDecoderSettings'
import { useReceiverSettings } from '../remote-web/useReceiverSettings'
import { js8DisplayNow } from '../remote-web/js8'
import { RemoteRecallEntry } from '../remote-web/RemoteRecall'
import type { AppSnapshot, BandChannel, Js8InboxEntry, Js8InboxState, Js8Origin, Js8Speed, Js8State, Js8Switch, Js8Unarmed } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { RegionColumnSeams } from './panes/RegionColumnSeams'
import { PaneSeam } from './PaneSeam'
import { regionColsStyle } from '../features/paneColumns'
import { WATERFALL_SPLIT_MAX, WATERFALL_SPLIT_MIN } from '../features/paneSeam'
import { PanelsMenu } from './PanelsMenu'
import { ArrangePanes } from './panes/ArrangePanes'
import { PaneDropLayer, gridTargets, usePaneDrag, type DropArea } from './panes/PaneDrag'
import { CockpitBox, boxLabels, foldedRailBoxes, pickForBox, useBoxSelection, type BoxSource } from './panes/CockpitBox'
import { panelHost } from '../features/panelHost'
import { BOX_IDS, JS8_PANEL_IDS, JS8_PANELS, boxEntries, isBoxId, type BoxId, type Js8PanelId, type PanelLayoutApi } from '../features/panelState'
import { regionGroups, type PaneDrop } from '../features/panelPlace'
import { FrequencyControl } from './FrequencyControl'
import { LogEntry } from './LogEntry'
import { RotorStrip } from './RotorStrip'
import { rotorPointAt } from './rotorPointAt'
import { Waterfall } from './Waterfall'
import { RfScopePane } from './RfScopePane'
import { useRegionCols, type RegionCols } from '../useRegionCols'
import { Js8RxPane } from './Js8RxPane'
import { Js8BandActivity } from './Js8BandActivity'
import { Js8CallList } from './Js8CallList'
import { Js8InboxPanel } from './Js8InboxPanel'
import { Js8AddStation } from './Js8AddStation'
import { Js8ListMenu } from './Js8ListMenu'
import { js8BandRowEntries, js8BandViewEntries, js8CallRowEntries, js8CallViewEntries, js8ClearEntries, type Js8Jump } from './js8ListMenus'
import { confirmDialog } from '../confirm'
import {
  highlightOf,
  js8BandRows,
  js8CallRows,
  js8GroupRows,
  js8InboxFor,
  js8ReplyText,
  js8RxLines,
  onCallList,
  sortBandRows,
  sortCallRows,
  JS8_REPLY_PLACEHOLDER,
  type Js8BandRow,
  type Js8CallRow,
} from '../features/js8Lists'
import {
  clearJs8Lists,
  loadJs8ListMarks,
  loadJs8ViewPrefs,
  saveJs8ListMarks,
  saveJs8ViewPrefs,
  type Js8ListMarks,
  type Js8ViewPrefs,
} from '../features/js8View'
import { Js8SoundWatch, js8SoundsToPlay, playJs8Sound, type Js8SoundEvent } from '../features/js8Sounds'
import {
  atuTune,
  getJs8State,
  getLicensedBandPlan,
  haltTx,
  js8Arm,
  js8CallCq,
  js8AnswerReply,
  js8Composer,
  js8CqRepeat,
  js8DropQueue,
  js8Enter,
  js8InboxDelete,
  js8InboxMark,
  js8LocatorRefusal,
  js8Send,
  js8SendCommand,
  js8SetSpeed,
  setRxOffset,
  setTune,
  setTxLevel,
  setTxOffset,
} from '../api'
import { bandLabelForMhz } from '../band'
import { failureToShow, useLatestLogAnswer, useLogStatus } from '../features/logSource'
import { loadJs8Pins, saveJs8Pins, sortPinnedFirst, toggleJs8Pin } from '../features/js8Pins'
import { useUnits } from '../units'
import { pushToast, withErrorToast } from '../toast'
import { pollSingleFlight } from '../singleFlight'
import { usePinnedScroll } from '../usePinnedScroll'
import { t } from '../i18n'
import {
  ALLCALL,
  AUTOREPLY,
  CQ,
  HB,
  HB_ACK,
  JS8,
  JS8_COMMANDS,
  JS8_CQS,
  JS8_GRID,
  JS8_QUICK_QUERIES,
  JS8_SPEEDS,
  JS8_SPEED_LIST,
  RELAY,
  RX_PLATE,
  TX_PLATE,
  js8BaseCall,
  js8UnreadFrom,
  countBits,
  estimateFrames,
  fmtSnr,
  js8ListedStations,
  utcClock,
} from '../js8Vocab'

interface Props {
  /** Open the Logbook filtered to a callsign — handed to the log strip's recall card. */
  onOpenLogbook?: (call: string) => void
  /** The call in this cockpit's log strip, for the Rotor box beside it (LogEntry `onEntryCall`). */
  onEntryCall?: (call: string) => void
  /** Live snapshot — may be absent while the app is still connecting. */
  snap?: AppSnapshot | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when JS8 is the visible view. The cockpit stays MOUNTED in its keep-alive host;
   * this pauses the display poll while hidden and drives the view-entry `js8_enter`
   * (rising edge). */
  active?: boolean
  /** QSY to a band-plan channel / a typed dial — a QSY, never TX. */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
  /** Arm/disarm TX (WSJT-X "Enable Tx") — the header pill becomes the arm control, since the
   * TopBar's cluster is hidden with the digital chrome in this view. NOT a stop control in a
   * slotted mode (the Operate ruling): a frame in flight completes. */
  onSetTxEnabled?: (on: boolean) => void
  theme?: string
  wheelSensitivity?: number
  /** JS8Call's callsign aging from Settings, in minutes (0 = off): the Stations list leaves out
   *  a call not heard for this long (`js8ListedStations`). */
  callsignAgingMin?: number
  /** JS8Call's band-activity aging, in minutes (Settings ▸ JS8; 0, the default here, is off):
   *  App hands down the station's setting. */
  activityAgingMin?: number
  /** Panel visibility record — host-owned (App) so it survives remounts. */
  panels?: PanelLayoutApi<Js8PanelId>
  /** Open Settings at a section id: the rotor strip's "configured but not answering" chip
   *  opens the Rotator section through it, as it does in the Phone, CW and FT cockpits. */
  onOpenSettings?: (target: string) => void
  /** What the window lends this cockpit's BOXES (2026-10-07) — exactly what it lends the dashboard
   *  rail. Absent ⇒ no box is drawn or offered, whatever the record says (Phone's rule). */
  boxes?: BoxSource
  /** The lists' own settings (Settings ▸ JS8) and the call list's two writes. Absent: JS8Call's
   *  defaults, and the writes are not offered. */
  lists?: Js8ListSettings
}

/** What Settings ▸ JS8 holds for the lists (JS8Call 3.0.3's, at its defaults when absent), and the
 *  call list's writes, each one field, never the settings form (App). */
export interface Js8ListSettings {
  rxBlocklist: readonly string[]
  highlightPrimary: readonly string[]
  highlightSecondary: readonly string[]
  eot: string
  mfi: string
  /** The events that play a sound (features/js8Sounds). */
  sounds: readonly string[]
  /** The joined @GROUPs. */
  groups: readonly string[]
  /** "Block this station": the RX block list as it should be. */
  setRxBlocklist?: (calls: string[]) => Promise<void>
  /** "Add New Station or Group…" with a @GROUP and "Remove This Group": the groups as they should be. */
  setGroups?: (groups: string[]) => Promise<void>
}

/** How long the new-message notice stays up: JS8Call's "New Message Received" box closes itself
 *  after 300 seconds (`SelfDestructMessageBox(300, …)`, processCommandActivity.cpp:809-818). */
const JS8_NEW_MESSAGE_MS = 300_000

/** JS8Call's defaults for the lists, where no settings are handed down. */
const JS8_LIST_DEFAULTS: Js8ListSettings = {
  rxBlocklist: [],
  highlightPrimary: [],
  highlightSecondary: [],
  eot: '\u2662',
  mfi: '\u2026\u2026',
  sounds: [],
  groups: [],
}

/** The ⊞ menu's entries: the cockpit's own panes. A box comes and goes through ⊞ Arrange's "+ Add a
 *  box" and its own ✕, never a tick (Phone's rule). */
const JS8_MENU = JS8_PANEL_IDS.filter((id) => !isBoxId(id))

/** Display labels for the JS8 removable panels — resolved when the menu is BUILT. */
const js8PanelLabels = (): Record<Exclude<Js8PanelId, BoxId>, string> => ({
  scope: t('js8.panel.scope'),
  rfScope: t('rfScope.title'),
  activity: t('js8.panel.activity'),
  offsets: t('js8.panel.offsets'),
  stations: t('js8.panel.stations'),
  inbox: t('js8.panel.inbox'),
  log: t('js8.panel.log'),
})

/**
 * JS8 operating cockpit (Digital rail: FT · Tempo · RTTY · PSK · SSTV · APRS · JS8) — the
 * JS8Call-compatible keyboard mode on FT8's physical layer. RX is engine-owned and starts on
 * view entry (`js8_enter` = set_tier(JS8) + the JS8 watering hole for the band, decoding every
 * speed the operator has enabled); nothing here keys. Every send asks the engine, which
 * re-checks the session TX latch, privileges, identity and — in B7 — the two-act arm for
 * automatic origins, and answers with a reason when it refuses.
 *
 * THE STOP LINE census here (outside every ⊞-removable pane; mirrored in stop-line.test.tsx's
 * JS8 case): Stop TX (header → halt_tx, never disabled), Tune (header; the carrier it starts),
 * and Esc (keyboard-only, census-only — bound while this is the visible view). The TX-enable
 * latch is NOT a stop in a slotted mode; "Drop queue" is a SENDER-class control, and so are
 * the CQ/HB repeat toggles — switching one OFF cancels the SCHEDULE, never an over in
 * flight, so neither may enter the stop-line sweep.
 *
 * Mounted in a keep-alive host (like RTTY/PSK/SSTV/APRS) so the activity stream keeps its
 * scroll position and selection while the operator is on another section.
 */
export function Js8Cockpit({
  snap,
  onSnap,
  active = true,
  onSetFrequency,
  onSetTxEnabled,
  theme = 'dark',
  wheelSensitivity,
  callsignAgingMin = 0,
  activityAgingMin = 0,
  onOpenLogbook,
  onEntryCall,
  panels,
  onOpenSettings,
  boxes,
  lists = JS8_LIST_DEFAULTS,
}: Props) {
  const canControl = useStationControl(), dataAvailable = useStationData()
  const rotatorControl = useStationCapability('rotator')
  const decoderSettings = useDecoderSettings(snap, 'JS8')
  const receiverSettings = useReceiverSettings(snap, 'JS8')
  // THE BOXES (2026-10-07): what each box on screen shows — none where the window lends them nothing
  // (Phone's rule), and none while this keep-alive cockpit is not the one on screen: a box's body polls
  // the Conditions feeds while it is mounted, and a hidden JS8 must not keep them asking.
  // Once per screen across the cockpit and the dashboard rail beside it: a box gives way to the rail (App).
  const entries = panels && boxes && active ? boxEntries(JS8_PANELS, panels.layout, panels.stateOf, boxes.rail?.shows) : {}
  const labels = { ...js8PanelLabels(), ...boxLabels(entries) }
  const host = panels
    ? panelHost(panels, {
        menu: JS8_MENU,
        side: ['stations', 'inbox'],
        main: 'activity',
        labels,
        shipsHidden: JS8_PANELS.defaultRemoved,
      })
    : null
  // No panel record (a render without a host): every pane at its vocabulary's default — shown,
  // except the RF scope pane, which ships hidden (`defaultRemoved`).
  const shown = (id: Js8PanelId) => (host ? host.shown(id) : !JS8_PANELS.defaultRemoved?.includes(id))
  // The pane's own ✕ — now routed through panelHost like every other cockpit's, so the five
  // hand-rolled `setPanelState(..., 'removed')` calls this cockpit shipped with cannot drift
  // from the ⊞ tick. Same act, same record; `{}` with no panel host.
  const closeProps = (id: Js8PanelId) => (host ? host.closeProps(id) : {})

  // Live state — polled at 2 Hz while this is the visible view (the PSK pattern; no Tauri
  // events). The backend keeps decoding while we're hidden; the first tick on re-activation
  // catches the display up.
  const [js8, setJs8] = useState<Js8State | null>(null)
  const stationCalls = (js8?.stations ?? []).map((h) => h.call).sort().join(' ')
  const context = useJs8Context(active, stationCalls)
  const remote = context.remote
  useEffect(() => {
    if (!active || (remote && !dataAvailable)) { if (remote) setJs8(null); return }
    // Single-flight (#335): `get_js8_state` takes the engine mutex, so a tick skips while the last
    // read is still out instead of stacking another waiter behind a CAT stall.
    return pollSingleFlight('js8 state', 500, (owns) =>
      getJs8State()
        .then((s) => {
          if (owns()) setJs8(s)
          if (!remote) return composerSync.current()
        })
        .catch(() => { if (owns() && remote) setJs8(null) }),
    )
  }, [active, remote, dataAvailable])

  // Rising-edge toast for the idle-watchdog trip: the automatic origins just stood down
  // with no click behind it, so the operator is told ONCE per trip — not on every poll.
  const idleTrippedRef = useRef(false)
  useEffect(() => {
    const tripped = js8?.idleTripped ?? false
    if (tripped && !idleTrippedRef.current) {
      pushToast(t('js8.toast.idleTripped', { min: js8?.idleLimitMin ?? 60 }), 'info', 8000)
    }
    idleTrippedRef.current = tripped
  }, [js8?.idleTripped, js8?.idleLimitMin])

  // THE INBOX'S STATION FILTER (JS8Call's MessagePanel `setCall`): null shows every station. The
  // call list's "Show message inbox" and the new-message notice pick one, and bring the pane back
  // if it was hidden.
  const [inboxCall, setInboxCall] = useState<string | null>(null)
  const showInbox = (call: string | null) => {
    setInboxCall(call)
    if (host && !host.shown('inbox')) panels?.setPanelState('inbox', 'docked')
  }

  // JS8Call's notification sounds (features/js8Sounds), the enabled events only, each once a poll.
  // What a poll brings is read against the one before; coming back to this view starts afresh, so
  // what was heard meanwhile does not sound all at once. The Remote page plays none.
  const soundWatch = useRef(new Js8SoundWatch())
  const playSounds = (events: readonly Js8SoundEvent[]) => {
    if (remote) return
    for (const ev of js8SoundsToPlay(events, lists.sounds)) playJs8Sound(ev)
  }
  useEffect(() => {
    if (!active) soundWatch.current = new Js8SoundWatch()
  }, [active])
  useEffect(() => {
    if (!js8) return
    playSounds(soundWatch.current.frames(js8.activity, snapRef.current?.mycall ?? '', lists.groups))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [js8?.activity])

  // A MSG to me announces itself, as JS8Call's "New Message Received" box does
  // (processCommandActivity.cpp:806-820): ONCE per message, and it stays up as long as that box
  // (300 s) unless dismissed, with a way to the message. What the first poll finds is taken as seen,
  // so mail filed before this view opened (or restored from the journal) does not announce itself.
  const seenInboxRef = useRef<Set<number> | null>(null)
  useEffect(() => {
    const inbox = js8?.inbox
    if (!inbox) return
    const seen = seenInboxRef.current
    if (seen === null) {
      seenInboxRef.current = new Set(inbox.map((e) => e.id))
      return
    }
    let filed = false
    for (const e of inbox) {
      if (seen.has(e.id)) continue
      seen.add(e.id)
      if (e.state === 'unread') {
        filed = true
        pushToast(t('js8.inbox.new', { from: e.from, time: utcClock(e.atMs) }), 'info', JS8_NEW_MESSAGE_MS, {
          action: () => showInbox(e.from),
          actionLabel: t('js8.inbox.new.open'),
        })
      }
    }
    if (filed) playSounds(['inbox'])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [js8?.inbox])

  // ENTER the mode on the rising edge of `active` (works unconfigured, spec §Works unconfigured):
  // `js8_enter` sets the tier and the dial. ⚠️ RX ONLY, and the ENGINE guarantees it — the call
  // confers neither TX-enable nor any automatic-origin arm.
  const entered = useRef(false)
  useEffect(() => {
    if (!active) {
      entered.current = false
      return
    }
    if (remote || !canControl || entered.current) return
    entered.current = true
    void js8Enter()
      .then((s) => setJs8(s))
      .catch(() => {})
  }, [active, canControl, remote])

  // JS8 watering holes (JS8Call's FrequencyList), license-filtered.
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    if (remote) return
    void getLicensedBandPlan('js8').then(setPlan).catch(() => {})
  }, [remote])

  // THE LOGBOOK JOIN behind the roster's ✓ / Name / Comment columns is asked of `LogSource`
  // (`logDetail`, below): the same `callHistory` rule the log strip and the Operate card answer
  // "have I worked this call" with, and not a second one. Asked only while this is the visible
  // view: a hidden roster has no business paying for the join on every logged contact.

  // PINS (a row menu's "Pin to the top") — an operator hold on a roster that re-sorts. Held in state so a write
  // that localStorage refuses still applies for the session (features/js8Pins).
  const [pins, setPins] = useState<string[]>(loadJs8Pins)
  const togglePin = (call: string) => {
    const next = toggleJs8Pin(pins, call)
    setPins(next)
    saveJs8Pins(next)
  }

  const commitDial = (mhz: number) => {
    if (!canControl) return
    onSetFrequency?.(mhz, bandLabelForMhz(mhz), snap?.radio.sideband || 'USB')
  }

  // STOP TX → halt_tx: the universal stop (unkeys, arms slot_tx_abort, and — from B7 — empties
  // the JS8 queue, the HB schedule and the pending auto-reply).
  const stop = () => {
    if (!canControl) return
    void haltTx()
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  // Esc stops from anywhere in the cockpit — bound only while this is the VISIBLE view (the
  // cockpit stays mounted in the keep-alive host, so an unconditional listener would fire
  // Stop TX from inside another section). The stop rides the shared capture listener
  // (useEscStop, operator 2026-10-01), so no control on the screen can swallow it; this listener
  // still cancels the key's default, as it always did.
  useEscStop(active && canControl, stop)
  useEffect(() => {
    if (!active || !canControl) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, canControl])

  // --- The dock's addressee + composer. The ENGINE is the authority on every send. ---
  const [toCall, setToCall] = useState('')
  const [text, setText] = useState('')
  const [cqIdx, setCqIdx] = useState(0)
  /** The directed command the composer sends, or null for a plain message / MSG. */
  const [cmdId, setCmdId] = useState<number | null>(null)
  const snapRef = useRef(snap)
  snapRef.current = snap
  // THE COMPOSE BOX, BOTH WAYS (JS8Call's extFreeTextMsgEdit). With AUTO off the station puts a
  // reply here for the operator to send, as JS8Call's processTxQueue types it into its box
  // (mainwindow.cpp:9671): taken only into an EMPTY box, never over what the operator typed,
  // and named back so the station knows the box holds it. The station is also told whether the
  // box holds text. Native only: the Remote observes and has no compose box to fill.
  const textRef = useRef(text)
  textRef.current = text
  /** The compose box, for the inbox's Reply to fill and focus. */
  const composeRef = useRef<HTMLInputElement>(null)
  const takenRef = useRef<number | null>(null)
  const composerSync = useRef(async () => {})
  composerSync.current = async () => {
    if (!canControl) return
    const taken = takenRef.current
    const offered = await js8Composer(textRef.current.trim() !== '', taken).catch(() => undefined)
    if (offered === undefined) return
    if (takenRef.current === taken) takenRef.current = null
    if (offered && typeof offered.text === 'string' && typeof offered.id === 'number' && textRef.current.trim() === '') {
      takenRef.current = offered.id
      textRef.current = offered.text
      setToCall('')
      setCmdId(null)
      setText(offered.text)
    }
  }
  const composing = text.trim() !== ''
  useEffect(() => {
    if (!active || remote) return
    void composerSync.current()
  }, [composing, active, remote])
  const selectStation = (call: string) => setToCall(call.toUpperCase())
  /** A RECEIVE move only — the offset table's double-click, JS8Call's own behaviour on
   *  tableWidgetRXAll. The TX offset is untouched; nothing here keys. */
  const tuneRx = (hz: number) => {
    if (!canControl) { receiverSettings.tuneRx(hz); return }
    void setRxOffset(hz)
      .then((sn) => onSnap?.(sn))
      .catch(() => {})
  }

  /** The two refusals worth a toast BEFORE the round trip; the engine re-checks both. */
  const refuseIfUnready = (): boolean => {
    const mycall = snapRef.current?.mycall?.trim() ?? ''
    if (!mycall) {
      pushToast(t('js8.toast.noCallsign'), 'info', 3500)
      return true
    }
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast(snapRef.current.radio.txRefusal ?? t('js8.toast.txLocked'), 'info', 3500)
      return true
    }
    return false
  }
  const send = () => {
    if (!canControl) return
    const body = text.trim()
    const to = toCall.trim().toUpperCase()
    if (cmdId !== null && !to) {
      pushToast(t('js8.toast.noAddressee'), 'info', 3000)
      return
    }
    if (cmdId === null && !body) return
    if (refuseIfUnready()) return
    const call = cmdId !== null ? () => js8SendCommand(to, cmdId, body) : () => js8Send(to || null, body)
    void withErrorToast(call, t('js8.toast.send.failed')).then((s) => {
      if (s) {
        setJs8(s)
        setText('')
      }
    })
  }
  const toggleSwitch = (which: Js8Switch, on: boolean) => {
    if (!canControl) return
    void withErrorToast(() => js8Arm(which, !on), t('js8.toast.arm.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  // JS8Call's AutoreplyConfirmation box (mainwindow.cpp:5209-5230): the answer names the reply it
  // was shown for, so a Yes can never reach a different one.
  const answerPending = (yes: boolean) => {
    const p = js8?.pendingReply
    if (!canControl || !p) return
    void withErrorToast(() => js8AnswerReply(yes, p.display, p.firesAtMs), t('js8.toast.answer.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const dropQueue = () => {
    if (!canControl) return
    void withErrorToast(() => js8DropQueue(), t('js8.toast.drop.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const callCq = () => {
    if (!canControl) return
    if (refuseIfUnready()) return
    void withErrorToast(() => js8CallCq(cqIdx), t('js8.toast.cq.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  // HB is the SESSION-ONLY schedule (never persisted): the second act for heartbeat frames.
  // Turning it on never keys by itself — the session TX latch is the first act (B7).
  const toggleHb = () => {
    if (!canControl) return
    void withErrorToast(() => js8Arm('hb', js8?.hbOn !== true), t('js8.toast.arm.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  // JS8Call's CHECKABLE CQ button (mainwindow.cpp:6353): with a repeat interval set it arms
  // a schedule instead of sending once. Session-only and never persisted; keys nothing on
  // its own — the TX latch is the first act and the engine re-reads it every slot.
  const toggleCqRepeat = () => {
    if (!canControl) return
    void withErrorToast(() => js8CqRepeat(js8?.cqOn !== true, cqIdx), t('js8.toast.arm.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const quickQuery = (call: string, cmd: number) => {
    if (!canControl) return
    if (refuseIfUnready()) return
    void withErrorToast(() => js8SendCommand(call, cmd, ''), t('js8.toast.command.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  // JS8Call's query menu sends a station your locator in one click, `<call> GRID <my_grid()>`
  // (mainwindow.cpp:6656-6668), and is disabled while none is set (:6657). The button is disabled
  // whenever the JS8 gate refuses the locator (`locatorRefusal`), and the engine refuses such a
  // send whatever the button says.
  const sendMyGrid = (call: string) => {
    if (!canControl) return
    if (refuseIfUnready()) return
    const grid = (snapRef.current?.mygrid ?? '').trim().toUpperCase()
    void withErrorToast(() => js8SendCommand(call, JS8_GRID.id, grid), t('js8.toast.command.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const setSpeed = (idx: number) => {
    if (!canControl) {
      if (js8 && JS8_SPEEDS[js8.speed].idx !== idx) decoderSettings.change({ action: 'decoder.js8Speed', expectedSpeed: JS8_SPEEDS[js8.speed].idx, speed: idx })
      return
    }
    void withErrorToast(() => js8SetSpeed(idx), t('js8.header.speed.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const markInbox = (id: number, state: Js8InboxState) => {
    if (!canControl) return
    void withErrorToast(() => js8InboxMark(id, state), t('js8.inbox.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  /** Delete messages, several at once as JS8Call's Delete takes the rows picked (MessagePanel.cpp:
   *  194-232): one engine verb each, in turn. */
  const deleteInbox = (ids: readonly number[]) => {
    if (!canControl) return
    void (async () => {
      for (const id of ids) {
        const s = await withErrorToast(() => js8InboxDelete(id), t('js8.inbox.failed'))
        if (s) setJs8(s)
      }
    })()
  }

  // REGION TIERS (CW's rule): three tracks only when the activity column, the aux column
  // (stations / inbox) AND the log column all have something to hold; an empty track is the
  // "band of empty black" rebuilt. useRegionCols owns data-cols/data-flow.
  // The main track carries BOTH decode surfaces (the transcript and the offset table), so it
  // is present while either is: hiding only `activity` must not strand `offsets` in no column.
  //
  // WHERE EACH PANE STANDS (layout L3, ⊞ Panels ▸ Arrange): the columns are the record's placement,
  // or the stock grouping (features/panelPlace), and "has something to hold" is counted on them. The
  // log pane needs a snapshot to render, so that is part of whether it is on screen.
  const place = panels?.layout.place
  const paneShown = (id: Js8PanelId): boolean => (isBoxId(id) ? entries[id] != null : shown(id) && (id !== 'log' || snap != null))
  const placed3 = regionGroups(JS8_PANELS.arrange!, place, 3, paneShown)
  // The DASHBOARD RAIL'S boxes, on a window too small for the rail (the operator's "They move into the
  // columns"), stand at the foot of the leading column, where JS8's own boxes stand until placed.
  const folded = boxes?.rail?.folded?.boxes ?? []
  const populated = placed3.filter((g, i) => g.ids.length > 0 || (i === 0 && folded.length > 0)).length
  const { ref: panesRef, cols, flow } = useRegionCols<HTMLDivElement>(Math.max(1, populated) as RegionCols)
  // The boxes' selection: the cockpit's own, shared by its boxes and never the window's (CockpitBox).
  const boxSel = useBoxSelection()
  // The columns, for the dividers between them to measure (panes/RegionColumnSeams).
  const mainColRef = useRef<HTMLDivElement>(null)
  const auxColRef = useRef<HTMLDivElement>(null)
  const logColRef = useRef<HTMLDivElement>(null)
  // Two tracks with the log hidden are activity | stations + inbox: the second track is the
  // stations column, and the width divider on its left edge says so.
  const auxInLogTrack = cols === 2 && placed3[2].ids.length === 0
  // The columns as this tier renders them: a | b | log at three; at two, a | b when the log column
  // is empty and a + b | log otherwise; at one, a + b over the log.
  const rendered: Js8PanelId[][] =
    cols === 3 || auxInLogTrack
      ? placed3.map((g) => g.ids)
      : [[...placed3[0].ids, ...placed3[1].ids], placed3[2].ids]
  // ⊞ ARRANGE BY DRAG (2026-10-08): Phone's (PhoneCockpit, the same words): a pane is picked up by its title
  // and dropped onto a column or between two panes, the drop the arrows' moves with their own arguments; the
  // log stays in its column, as its arrows keep it (pinned). The dock with its stop controls is no place.
  const cockpitRef = useRef<HTMLElement>(null)
  const dropPane = panels?.dropPane != null ? (id: Js8PanelId, drop: PaneDrop<Js8PanelId>) => panels.dropPane?.(id, drop, paneShown) : undefined
  const holds = (area: DropArea) => placed3.some((g) => g.col === area && g.ids.length > 0)
  const paneDrag = usePaneDrag<Js8PanelId>({
    root: cockpitRef,
    region: panesRef,
    enabled: dropPane != null,
    spec: JS8_PANELS.arrange!,
    arrangement: { place },
    shown: paneShown,
    sideShows: false,
    stacked: flow === 'stack',
    // The columns as this tier draws them (`rendered`): a | b | log, or a | b with the log column empty, or a
    // + b | log.
    targets: () =>
      gridTargets(
        cols === 3 || auxInLogTrack
          ? [
              { el: mainColRef.current, areas: ['a'] },
              { el: auxColRef.current, areas: ['b'] },
              { el: cols === 3 ? logColRef.current : null, areas: ['log'] },
            ]
          : [
              { el: mainColRef.current, areas: ['a', 'b'] },
              { el: logColRef.current, areas: ['log'] },
            ],
        holds,
      ),
    labels,
    names: { a: t('panels.arrange.column.a'), b: t('panels.arrange.column.b'), log: t('panels.arrange.column.log') },
    onDrop: (id, drop) => dropPane?.(id, drop),
  })
  /** Whether `below` sits directly under `above` in one rendered column — what makes them a pair. */
  const adjacent = (above: Js8PanelId, below: Js8PanelId) =>
    rendered.some((ids) => {
      const i = ids.indexOf(above)
      return i >= 0 && ids[i + 1] === below
    })

  // THE DIVIDERS BETWEEN PANES IN A COLUMN (layout L2): Activity | Band activity (the two decode
  // surfaces) and Stations | Inbox. Each pair is adjacent in its column at every tier, and a
  // pair only while both are shown and the region is bounded — in the stacking flow every pane is
  // content height and a share moves nothing. While it is a pair, each pane carries the operator's
  // share and its floor follows it (CockpitPaneFrame `split`). Activity weighs 2 and Band activity
  // 1, so their divider paints grows of share × 1.5: the pair keeps its stock total of 3, and at
  // two columns, where Stations and Inbox share the column, they stay exactly where they were.
  // The waterfall strip, for the divider under it (its height; layout L2).
  const wfRef = useRef<HTMLDivElement>(null)
  const activityFrameRef = useRef<HTMLElement>(null)
  const offsetsFrameRef = useRef<HTMLElement>(null)
  const stationsFrameRef = useRef<HTMLElement>(null)
  const inboxFrameRef = useRef<HTMLElement>(null)
  const decodePair = panels != null && flow === 'fill' && shown('activity') && shown('offsets') && adjacent('activity', 'offsets')
  const heardPair = panels != null && flow === 'fill' && shown('stations') && shown('inbox') && adjacent('stations', 'inbox')
  /** The grow a share of 1 stands for in the decode pair: the mean of its stock weights 2 and 1. */
  const DECODE_SPLIT = 1.5
  const pairedShare = (paired: boolean, id: Js8PanelId, k: number) => {
    const s = panels?.layout.share[id]
    return paired && s != null ? s * k : undefined
  }

  const activityPin = usePinnedScroll<HTMLDivElement>()
  const units = useUnits()
  const myGrid = snap?.mygrid ?? ''
  const ownGrid = myGrid.trim().toUpperCase()
  // JS8Call's Settings refuse a malformed locator (Configuration.cpp:2443-2446), so its GRID menu
  // item only ever meets a good one or none, and is disabled for none (mainwindow.cpp:6657).
  // Nexus's Settings can hold a malformed one, which the JS8 gate refuses. "Send my grid" asks
  // that gate, its one rule, whenever the locator changes: disabled while it refuses, with its
  // reason as the tooltip, and until it has answered. The Remote never asks (it cannot send).
  const [locatorRefusal, setLocatorRefusal] = useState<string | null | undefined>(undefined)
  useEffect(() => {
    if (!active || !canControl) return
    let live = true
    void js8LocatorRefusal()
      .then((reason) => {
        if (live) setLocatorRefusal(reason)
      })
      .catch(() => {
        if (live) setLocatorRefusal(undefined)
      })
    return () => {
      live = false
    }
  }, [active, canControl, myGrid])

  const sending = js8?.sending === true
  const rxCount = countBits(js8?.rxSpeeds ?? 0)
  const selectedCall = toCall.trim().toUpperCase()
  const selected = js8?.stations.find((h) => h.call === selectedCall) ?? null
  const now = js8DisplayNow(js8)
  // The list only: the station keeps every call it heard, so the log strip, the rotator and the
  // To box still know an aged one, as JS8Call's call activity does.
  const listedStations = js8
    ? js8ListedStations(js8.stations, js8.inbox, {
        agingMin: callsignAgingMin,
        nowMs: now,
        selectedCall,
        myCall: snap?.mycall ?? '',
      })
    : []

  // ⚑ and the lift to the top for a station with an unread message to me (js8Vocab.js8UnreadFrom).
  const unreadFrom = js8 ? js8UnreadFrom(js8.inbox, snap?.mycall ?? '') : new Set<string>()

  // THE LISTS' VIEW (features/js8View): the columns, the orders, the labels and the heartbeat toggle
  // the operator picked, and what was cleared, removed or added — this browser's, at JS8Call's
  // defaults. Held in state so a value storage refuses still applies for the session.
  const [view, setView] = useState<Js8ViewPrefs>(loadJs8ViewPrefs)
  const [marks, setMarks] = useState<Js8ListMarks>(loadJs8ListMarks)
  const changeView = (next: Js8ViewPrefs) => {
    setView(next)
    saveJs8ViewPrefs(next)
  }
  const changeMarks = (fn: (m: Js8ListMarks) => Js8ListMarks) => {
    const next = fn(marks)
    setMarks(next)
    saveJs8ListMarks(next)
  }
  const myCall = snap?.mycall ?? ''
  // The RX pane, Band activity and the call list, as JS8Call draws them (features/js8Lists).
  const rxLines = js8RxLines(js8?.activity ?? [], {
    rxOffsetHz: snap?.radio.rxOffsetHz ?? 1500,
    myCall,
    groups: lists.groups,
    blocklist: lists.rxBlocklist,
    eot: lists.eot,
    mfi: lists.mfi,
    nowMs: now,
    clearedAtMs: marks.cleared.rx,
  })
  const bandRows = sortBandRows(
    js8BandRows(js8?.activity ?? [], {
      nowMs: now,
      agingMin: activityAgingMin,
      selectedHz: snap?.radio.rxOffsetHz ?? null,
      showHb: view.showHb,
      eot: lists.eot,
      mfi: lists.mfi,
      myCall,
      primary: lists.highlightPrimary,
      secondary: lists.highlightSecondary,
      clearedAtMs: marks.cleared.band,
      removed: marks.removedOffsets,
    }),
    view.band.sort,
  )
  const callRows = js8CallRows(listedStations, {
    nowMs: now,
    myCall,
    blocklist: lists.rxBlocklist,
    unreadFrom,
    activity: js8?.activity ?? [],
    inbox: js8?.inbox ?? [],
    added: marks.added,
    removed: marks.removedCalls,
    clearedAtMs: marks.cleared.call,
  })
  // Every call the list shows, the ones added by hand and the ones heard through a relay included.
  const listCalls = [...new Set([...stationCalls.split(' ').filter(Boolean), ...callRows.map((r) => r.call)])].sort().join(' ')

  /** Per-listed-call log detail: worked-before, the name and comment of the most recent QSO,
   *  and that QSO's grid as a fallback when the station has not sent one (JS8Call does the
   *  same, mainwindow.cpp:10325-10345). Keyed on the CALL SET, not the stations array — that
   *  array is a fresh object on every 500 ms poll, and re-scanning the whole log twice a
   *  second per station is not a thing a roster may cost. */
  // JS8Call's own scope for this column is hasWorkedBefore(call, "") — worked ANYWHERE, any band,
  // any mode (features/callHistory `callsSummary`). The band/mode dupe scope belongs to the log
  // strip, not here.
  //
  // Each newly heard station asks a new question, and until its answer lands the rows keep the
  // latest answer for the calls it covered: no row loses its marks for the round trip. A call that
  // answer did not cover shows "—" (below), as the Remote path does when it cannot tell.
  const summaryQuestion = { kind: 'callsSummary', calls: listCalls.split(' ').filter(Boolean) } as const
  const reading = active && !remote ? summaryQuestion : null
  const summary = useLatestLogAnswer(reading, snap?.logTick)
  const summaryFailed = failureToShow(useLogStatus(reading))
  const logDetail = useMemo(() => {
    if (remote) return new Map(Object.entries(context.value?.history ?? {}).filter(([,h]) => h.count > 0))
    return new Map(Object.entries(summary?.answer ?? {}))
  }, [summary, remote, context.value])
  /** The heard calls the log has answered about. */
  const answeredCalls = useMemo(() => new Set(summary?.question.calls), [summary])
  const unansweredTitle = summaryFailed !== null ? t('logbook.readFailed', { reason: summaryFailed }) : t('logbook.reading')
  /** Why a call's ✓ is not known yet ("—"), or null once it is. */
  const logPending = (call: string): string | null =>
    remote
      ? context.value?.history[call.trim().toUpperCase()]
        ? null
        : t('remote.collectionUnavailable')
      : reading && !answeredCalls.has(call)
        ? unansweredTitle
        : null
  // A call heard for the first time sounds once the log has said whether it is worked.
  useEffect(() => {
    if (!js8 || remote) return
    playSounds(soundWatch.current.newCalls(js8.stations, (call) => (answeredCalls.has(call) ? logDetail.has(call) : null)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [js8?.stations, answeredCalls, logDetail])
  // Each listed call's grid: its own, else the log's (JS8Call's fallback), for the distance and
  // azimuth orders.
  const callGrids = new Map(
    callRows.map((r) => [r.call, (r.heard?.grid ?? '').trim() || logDetail.get(r.call)?.grid || ''] as [string, string]),
  )
  const sortedCallRows = sortPinnedFirst(
    sortCallRows(callRows, view.call.sort, { myGrid: snap?.mygrid ?? '', grids: callGrids, unreadFrom }),
    pins,
  )
  const groupRows = js8GroupRows(lists.groups, js8?.inbox ?? [], true)

  /** What a switch that is on but not armed says: the station's reason (`notArmed`). The idle
   *  watchdog's sentence is shared, TX off keeps the switch's own "on, but TX is off" title
   *  (`txOff`), and the rest are that switch's own words (`own`), literal keys at each call site
   *  so the orphan guard sees each referenced. An older station on the Remote page sends no
   *  reason, and its chip keeps the title this page always gave it. */
  const notArmedTitle = (
    why: Js8Unarmed | null | undefined,
    txOff: string,
    own: Partial<Record<Js8Unarmed, string>> = {},
  ): string =>
    why === 'idle' ? t('js8.dock.notArmed.idle', { min: js8?.idleLimitMin ?? 60 }) : (why && own[why]) || txOff

  const hbTitle =
    js8?.hbOn && js8.armed.hb
      ? t('js8.dock.hb.title.armed')
      : js8?.hbOn
        ? notArmedTitle(js8.notArmed?.hb, t('js8.dock.hb.title.on'), {
            unscheduled: t('js8.dock.hb.notArmed.unscheduled'),
          })
        : t('js8.dock.hb.title.off')

  /** JS8Call renders the countdown IN the button — `CQ (12)`, `HB (42)`, `HB (now)`
   *  (`updateRepeatButtonDisplay`, mainwindow.cpp:7800). Whole seconds, truncated, exactly
   *  as `QDateTime::secsTo` gives them; a deadline already passed reads "now". `null` when
   *  nothing is scheduled, and the button shows its bare token. */
  const repeatCountdown = (on: boolean | undefined, nextAtMs: number | null | undefined): string | null => {
    if (on !== true || nextAtMs == null) return null
    const secs = Math.floor((nextAtMs - now) / 1000)
    return secs > 0 ? String(secs) : t('js8.dock.repeat.now')
  }
  const hbCount = repeatCountdown(js8?.hbOn, js8?.hbNextAtMs)
  const cqCount = repeatCountdown(js8?.cqOn, js8?.cqNextAtMs)
  /** A repeat interval of 0 is JS8Call's "on demand": the CQ button stays the one-shot it
   *  has always been. Above 0 it becomes the checkable auto-repeat. */
  const cqRepeats = (js8?.cqIntervalMin ?? 0) > 0
  const cqRepeatTitle =
    js8?.cqOn && js8.armed.cq
      ? t('js8.dock.cqRepeat.title.armed', { min: js8?.cqIntervalMin ?? 0 })
      : js8?.cqOn
        ? notArmedTitle(js8.notArmed?.cq, t('js8.dock.cqRepeat.title.on'), {
            unscheduled: t('js8.dock.cqRepeat.notArmed.unscheduled'),
          })
        : t('js8.dock.cqRepeat.title.off', { min: js8?.cqIntervalMin ?? 0 })

  // THE ESTIMATE beside Send — a hint, not a gate (js8Vocab.estimateFrames). The engine is
  // the authority and refuses over the §97.119 cap; the `over` face and the disabled Send
  // just save the round trip.
  const speedInfo = JS8_SPEEDS[js8?.speed ?? 'normal']
  const frames = estimateFrames(toCall, cmdId, text, snap?.mycall ?? '', speedInfo.key)
  const overCap = frames > speedInfo.maxFrames
  const estimateText =
    frames === 0
      ? ''
      : overCap
        ? t('js8.dock.estimate.over', { count: frames, max: speedInfo.maxFrames })
        : t('js8.dock.estimate', { count: frames, secs: frames * speedInfo.periodS })
  const canSend = !overCap && (cmdId !== null ? toCall.trim() !== '' : text.trim() !== '')
  const pendingSecs = js8?.pendingReply ? Math.max(0, Math.ceil((js8.pendingReply.firesAtMs - now) / 1000)) : 0

  /** Literal keys per origin, so the orphan guard sees each referenced. */
  const originLabel = (o: Js8Origin): string => {
    switch (o) {
      case 'operator':
        return t('js8.dock.origin.operator')
      case 'heartbeat':
        return t('js8.dock.origin.heartbeat')
      case 'hbAck':
        return t('js8.dock.origin.hbAck')
      case 'autoReply':
        return t('js8.dock.origin.autoReply')
      case 'relay':
        return t('js8.dock.origin.relay')
      case 'cqRepeat':
        return t('js8.dock.origin.cqRepeat')
    }
  }
  /** One second-act chip with its three faces: off · on but not armed (`notArmedTitle` says
   *  why) · ARMED. */
  const armChip = (
    which: Js8Switch,
    cls: string,
    label: string,
    on: boolean,
    armed: boolean,
    titles: [off: string, notArmed: string, armed: string],
  ) => (
    <button
      type="button"
      className={`cw-macro rtty-arm js8-arm ${cls}${on ? ' on' : ''}${on && armed ? ' armed' : ''}`}
      aria-pressed={on}
      disabled={!canControl}
      onClick={() => toggleSwitch(which, on)}
      // Every face carries the two-act note: an armed chip that has not fired reads as a bug
      // to a JS8Call operator, and the sentence that stops it is the one naming the latch.
      title={`${on && armed ? titles[2] : on ? titles[1] : titles[0]} ${t('js8.dock.arm.differs')}`}
    >
      <span className="cw-macro-label">{label}</span>
    </button>
  )

  // ---- panes ----
  // THE RF SCOPE PANE — the radio's own panadapter, hidden until ticked (RF_SCOPE_PANEL_ID). Placed
  // like any region pane (it heads the leading column in the stock grouping); display only, it hosts
  // no stop control and no sender (THE STOP LINE).
  const rfScopePane = shown('rfScope') && (
    <RfScopePane
      closeProps={closeProps('rfScope')}
      dialMhz={snap?.radio.dialMhz ?? 0}
      keyed={sending || (snap?.radio.tuning ?? false)}
      theme={theme}
      active={active}
      privilegeMode={snap?.radio.operatingMode}
    />
  )

  // ---- THE LISTS' MENUS (js8ListMenus): JS8Call's View and header menus, each pane's ⋯, and its
  // right-click menus, each row's ⋯ or a right-click on the row. Nothing in them keys. ----
  const clearList = (which: 'rx' | 'band' | 'call') => changeMarks((m) => clearJs8Lists(m, which, js8DisplayNow(js8)))
  /** "Clear All Lists", after JS8Call's question (UI_Constructor.cpp:783-795). */
  const clearAll = () => {
    void confirmDialog({
      title: t('js8.lists.clearAll.confirm.title'),
      body: t('js8.lists.clearAll.confirm.body'),
      confirmLabel: t('js8.lists.clearAll.confirm.label'),
      danger: true,
    }).then((yes) => {
      if (yes) changeMarks((m) => clearJs8Lists(m, 'all', js8DisplayNow(js8)))
    })
  }
  const clears = (which: 'rx' | 'band' | 'call') => js8ClearEntries(() => clearList(which), clearAll)
  /** "Jump to …Hz": JS8Call's one offset is where it listens and sends, so both cursors move, as a
   *  Ctrl-click on the waterfall moves them. The Remote observer moves RX alone, as its waterfall does. */
  const jumpTo = (hz: number) => {
    if (!canControl) {
      receiverSettings.tuneRx(hz)
      return
    }
    void setRxOffset(hz)
      .then((sn) => onSnap?.(sn))
      .then(() => setTxOffset(hz))
      .then((sn) => onSnap?.(sn))
      .catch(() => {})
  }
  const jump = (hz: number | null, speed: Js8Speed | null): Js8Jump => ({
    hz,
    speed,
    current: js8?.speed ?? null,
    offsetLocked: sending || (js8?.queue.length ?? 0) > 0,
    canJump: canControl || receiverSettings.rxAllowed,
    canChangeSpeed: canControl || decoderSettings.allowed,
    onJump: jumpTo,
    onSpeed: (sp) => setSpeed(JS8_SPEEDS[sp].idx),
  })
  const [adding, setAdding] = useState(false)
  /** "Remove This Station" / "Remove This Group" (UI_Constructor.cpp:1099-1117): a station leaves the
   *  list until it is heard again; a group is left. (@ALLCALL's is the setting that stops taking
   *  part in it.) */
  const removeFromList = (call: string) => {
    if (call.startsWith('@')) {
      if (call !== ALLCALL) void lists.setGroups?.(lists.groups.filter((g) => g.trim().toUpperCase() !== call))
      return
    }
    changeMarks((m) => {
      const added = { ...m.added }
      delete added[call]
      return { ...m, added, removedCalls: { ...m.removedCalls, [call]: js8DisplayNow(js8) } }
    })
  }
  /** "Add New Station or Group…" (UI_Constructor.cpp:1059-1097). */
  const addToList = (what: { kind: 'allcall' | 'group' | 'call'; name: string }) => {
    if (what.kind === 'group') {
      if (!lists.groups.some((g) => g.trim().toUpperCase() === what.name)) void lists.setGroups?.([...lists.groups, what.name])
      return
    }
    if (what.kind === 'call') {
      changeMarks((m) => {
        const removedCalls = { ...m.removedCalls }
        delete removedCalls[what.name]
        return { ...m, removedCalls, added: { ...m.added, [what.name]: js8DisplayNow(js8) } }
      })
    }
  }
  /** "Block This Station" (UI_Constructor.cpp:1119-1129), and its undoing. */
  const block = (call: string, on: boolean) => {
    const kept = lists.rxBlocklist.filter((c) => {
      const e = c.trim().toUpperCase()
      return e !== call && e !== js8BaseCall(call)
    })
    void lists.setRxBlocklist?.(on ? [...kept, call] : kept)
  }
  const callRowMenu = (call: string, row: Js8CallRow | null) => {
    const group = call.startsWith('@')
    const blocked = !group && onCallList(lists.rxBlocklist, call)
    const h = row?.heard ?? null
    return js8CallRowEntries({
      group,
      jump: h ? jump(Math.trunc(h.freqHz), h.speed) : null,
      hasMessages: js8InboxFor(js8?.inbox ?? [], call).length > 0,
      onShowInbox: () => showInbox(call),
      pinned: pins.includes(call),
      onPin: () => togglePin(call),
      onAdd: () => setAdding(true),
      canRemove: !group || (call !== ALLCALL && lists.setGroups != null && canControl),
      onRemove: () => removeFromList(call),
      blocked,
      canBlock: lists.setRxBlocklist != null && canControl,
      onBlock: () => block(call, !blocked),
      clears: clears('call'),
    })
  }
  const bandRowMenu = (r: Js8BandRow) =>
    js8BandRowEntries(
      jump(r.offsetHz, r.speed),
      () => changeMarks((m) => ({ ...m, removedOffsets: { ...m.removedOffsets, [String(r.offsetHz)]: js8DisplayNow(js8) } })),
      clears('band'),
    )
  const menuLabel = (pane: string) => t('js8.view.menu.aria', { pane })
  const rxMenu = <Js8ListMenu label={menuLabel(t('js8.panel.activity'))} entries={() => clears('rx')} />
  const bandMenu = <Js8ListMenu label={menuLabel(t('js8.panel.offsets'))} entries={() => js8BandViewEntries(view, changeView, clears('band'))} />
  const callMenu = (
    <Js8ListMenu label={menuLabel(t('js8.panel.stations'))} entries={() => js8CallViewEntries(view, changeView, () => setAdding(true), clears('call'))} />
  )
  /** Reply (MessagePanel.cpp:336-350, mainwindow.cpp:145-150): JS8Call's reply text in the compose
   *  box, the To box and the command cleared, `[MESSAGE]` picked so typing replaces it. Sends nothing. */
  const replyTo = (e: Js8InboxEntry) => {
    if (!canControl) return
    const reply = js8ReplyText(e)
    setToCall('')
    setCmdId(null)
    textRef.current = reply
    setText(reply)
    requestAnimationFrame(() => {
      const el = composeRef.current
      if (!el) return
      el.focus({ preventScroll: true })
      const at = reply.indexOf(JS8_REPLY_PLACEHOLDER)
      el.setSelectionRange(at, at + JS8_REPLY_PLACEHOLDER.length)
    })
  }

  // THE RX PANE (JS8Call's): my offset plus what is addressed to me, a line per message, the end
  // mark after it. Every frame of every offset is in Band activity. A display only.
  const activityPane = shown('activity') && (
    <CockpitPaneFrame
      title={t('js8.panel.activity')}
      paneId="activity"
      weight={2}
      split={decodePair ? DECODE_SPLIT : undefined}
      share={pairedShare(decodePair, 'activity', DECODE_SPLIT)}
      paneRef={activityFrameRef}
      actions={rxMenu}
      {...closeProps('activity')}
    >
      <Js8RxPane lines={rxLines} scrollRef={activityPin.ref} onScroll={activityPin.onScroll} onSelectCall={selectStation} />
    </CockpitPaneFrame>
  )

  // BAND ACTIVITY — JS8Call's tableWidgetRXAll (mainwindow.ui:989): one row per frequency offset
  // holding every frame heard there. A fill pane with a weight: a table of rows can use surplus
  // height (the role question). It renders no sender and no stop — a double-click moves the RX
  // cursor, a receive control, and its menu's jump moves the offsets — so it is ⊞-hideable.
  const offsetsPane = shown('offsets') && (
    <CockpitPaneFrame
      title={t('js8.panel.offsets')}
      paneId="offsets"
      weight={1}
      split={decodePair ? DECODE_SPLIT : undefined}
      share={pairedShare(decodePair, 'offsets', DECODE_SPLIT)}
      paneRef={offsetsFrameRef}
      actions={bandMenu}
      {...closeProps('offsets')}
    >
      <Js8BandActivity
        rows={bandRows}
        columns={view.band.columns}
        labels={view.labels}
        nowMs={now}
        selectedHz={snap?.radio.rxOffsetHz ?? null}
        onDoubleClick={tuneRx}
        rowMenu={bandRowMenu}
      />
    </CockpitPaneFrame>
  )

  /** A station row's own last cell: the HB chip, what I hold for it, and the one-click asks. */
  const stationActions = (r: Js8CallRow) => {
    const h = r.heard
    return (
      <>
        {h?.lastHb && <span className="js8-chip">{HB}</span>}
        {h != null && h.storedMsgs > 0 && (
          <span className="js8-chip" title={t('js8.station.stored', { count: h.storedMsgs })}>
            ✉ {h.storedMsgs}
          </span>
        )}
        {JS8_QUICK_QUERIES.map((q) => (
          <button
            key={q.id}
            type="button"
            className="cw-macro js8-query"
            disabled={!canControl}
            onClick={() => quickQuery(r.call, q.id)}
            title={t('js8.station.query.title', { cmd: q.label, call: r.call })}
          >
            {q.label}
          </button>
        ))}
        <button
          type="button"
          className="cw-macro js8-query js8-send-grid"
          disabled={!canControl || locatorRefusal !== null}
          onClick={() => sendMyGrid(r.call)}
          title={locatorRefusal ?? (ownGrid ? t('js8.station.sendGrid.title', { grid: ownGrid, call: r.call }) : undefined)}
        >
          {ownGrid ? `${JS8_GRID.label} ${ownGrid}` : JS8_GRID.label}
        </button>
      </>
    )
  }

  const stationsPane = shown('stations') && (
    <CockpitPaneFrame
      title={t('js8.panel.stations')}
      paneId="stations"
      split={heardPair ? 1 : undefined}
      share={pairedShare(heardPair, 'stations', 1)}
      paneRef={stationsFrameRef}
      actions={callMenu}
      {...closeProps('stations')}
    >
      <Js8CallList
        header={
          remote && (
            <div className="js8-history-status" role="status">
              {!context.value && <span>{context.loading ? t('remote.collectionLoading') : t('remote.collectionUnavailable')}</span>}
              <button type="button" className="cw-macro" onClick={context.refresh} disabled={!dataAvailable || context.loading}>
                {t('remote.refreshCollection')}
              </button>
            </div>
          )
        }
        groups={groupRows}
        rows={sortedCallRows}
        columns={view.call.columns}
        labels={view.labels}
        nowMs={now}
        selectedCall={selectedCall}
        pins={pins}
        myGrid={myGrid}
        units={units}
        logDetail={logDetail}
        logPending={logPending}
        highlight={(call) => highlightOf(new Set([call]), lists.highlightPrimary, lists.highlightSecondary)}
        onSelect={selectStation}
        rowMenu={callRowMenu}
        actions={stationActions}
      />
      <Js8AddStation open={adding} onOpenChange={setAdding} onAdd={addToList} />
    </CockpitPaneFrame>
  )

  const inboxPane = shown('inbox') && (
    <CockpitPaneFrame
      title={t('js8.panel.inbox')}
      paneId="inbox"
      split={heardPair ? 1 : undefined}
      share={pairedShare(heardPair, 'inbox', 1)}
      paneRef={inboxFrameRef}
      {...closeProps('inbox')}
    >
      <Js8InboxPanel
        inbox={js8?.inbox ?? []}
        call={inboxCall}
        onCall={setInboxCall}
        canControl={canControl}
        onRead={(id) => markInbox(id, 'read')}
        onDelete={deleteInbox}
        onReply={replyTo}
      />
    </CockpitPaneFrame>
  )

  // THE LOG STRIP — CW's shape (a LogEntry in the log column), prefilled from the selected
  // station through the live machine-fill channel (no focus steal — see PskCockpit for why).
  const logPane = snap && shown('log') && (
    <CockpitPaneFrame
      title={t('js8.panel.log')}
      paneId="log"
      weight={1.5}
      {...closeProps('log')}
    >
      {!canControl ? <RemoteRecallEntry snap={snap} mode={JS8} selectedCall={selectedCall} onOpenLog={onOpenLogbook}/> : <LogEntry
        onOpenLogbook={onOpenLogbook}
        onEntryCall={onEntryCall}
        snap={snap}
        // The ADIF token: written as MODE=MFSK SUBMODE=JS8 by the logbook (B5).
        mode={JS8}
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        cwLive={
          selected
            ? { call: selected.call, rst: fmtSnr(selected.snrDb), name: null, confirmed: true }
            : null
        }
        onSnap={onSnap}
        fieldDay={snap.fieldDay ?? null}
        active={active}
        fdMode="DIG"
        fdSubmode={JS8}
      />}
    </CockpitPaneFrame>
  )

  // The two dividers between panes (see decodePair / heardPair above). Each sits between its pair
  // in whichever column holds them, as its own slot, so its coming and going moves no pane.
  const decodeSeam = decodePair && panels && (
    <PaneSeam
      above={activityFrameRef}
      below={offsetsFrameRef}
      varName="--pane-share"
      scale={DECODE_SPLIT}
      onCommit={(a, b) => panels.setShares({ activity: a, offsets: b })}
      onReset={() => panels.setShares({ activity: null, offsets: null })}
      label={t('js8.seam.activityOffsets.label')}
      className="in-column"
    />
  )
  const heardSeam = heardPair && panels && (
    <PaneSeam
      above={stationsFrameRef}
      below={inboxFrameRef}
      varName="--pane-share"
      onCommit={(a, b) => panels.setShares({ stations: a, inbox: b })}
      onReset={() => panels.setShares({ stations: null, inbox: null })}
      label={t('js8.seam.stationsInbox.label')}
      className="in-column"
    />
  )

  // Each placed pane by its id, KEYED by it (layout L3), with a pair's divider right after the pane
  // above it: a move within a column is a React move, not a remount, and the pinned log never
  // changes column.
  // THE BOXES (2026-10-07), as Phone's: each one on screen, in the cockpit's frame with its entry's role.
  // JS8 shows no shared board as a pane of its own, so only the other boxes and the dashboard rail beside it
  // are "on screen elsewhere".
  const onScreen = new Set<string>([...Object.values(entries), ...(boxes?.rail?.shows ?? [])])
  const boxEls = {} as Record<BoxId, React.ReactNode>
  for (const b of BOX_IDS) {
    const entry = entries[b]
    boxEls[b] =
      entry != null && boxes && panels ? (
        <CockpitBox
          box={b}
          entry={entry}
          source={boxes}
          selection={boxSel}
          onScreen={(e) => onScreen.has(e)}
          onPick={(e) => pickForBox(panels.setBox, boxes, b, e, entry)}
          onRemove={() => panels.setPanelState(b, 'removed')}
          stacked={flow === 'stack'}
        />
      ) : null
  }
  const paneEls: Record<Js8PanelId, React.ReactNode> = {
    scope: null,
    rfScope: rfScopePane,
    activity: activityPane,
    offsets: offsetsPane,
    stations: stationsPane,
    inbox: inboxPane,
    log: logPane,
    ...boxEls,
  }
  const slots = (ids: readonly Js8PanelId[]) =>
    ids.flatMap((id, i) => [
      <Fragment key={id}>{paneEls[id]}</Fragment>,
      ...(id === 'activity' && ids[i + 1] === 'offsets' && decodeSeam ? [<Fragment key="seam-decode">{decodeSeam}</Fragment>] : []),
      ...(id === 'stations' && ids[i + 1] === 'inbox' && heardSeam ? [<Fragment key="seam-heard">{heardSeam}</Fragment>] : []),
    ])

  return (
    <main className="layout single js8-cockpit" ref={cockpitRef} data-pane-drag={dropPane ? '' : undefined}>
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          remoteWorkspace="js8"
          // TX DRIVE, the FT8 header's control: a configuration control on the transmit
          // path, not a transmit control.
          power={{
            value: snap.radio.txLevel,
            unit: 'drive',
            onChange: (v: number) => {
              if (!canControl) return
              void setTxLevel(v)
                .then((s) => onSnap?.(s))
                .catch(() => {})
            },
            label: t('js8.header.power.label'),
            title: t('js8.header.power.title'),
          }}
          modeIndicator={
            <>
              <span className="cw-mode-badge" title={t('js8.header.speed.title')}>
                {JS8}
              </span>
              {/* The TRANSMIT speed — the slot clock follows it. Names are the mode's own. */}
              <span className="js8-speeds" role="group" aria-label={t('js8.header.speed.aria')}>
                {JS8_SPEED_LIST.map((s) => (
                  <button
                    key={s.key}
                    type="button"
                    className={`rtty-arm js8-speed-chip${js8?.speed === s.key ? ' on' : ''}`}
                    aria-pressed={js8?.speed === s.key}
                    disabled={!canControl && (!decoderSettings.allowed || !js8)}
                    onClick={() => setSpeed(s.idx)}
                    title={t('js8.header.speed.chip.title', { speed: s.label, period: s.periodS })}
                  >
                    {s.label}
                  </button>
                ))}
              </span>
              {/* MULTI-DECODE: how many of the four speeds the receiver is decoding right now.
                  Which ones is a Settings choice (Settings ▸ Digital ▸ JS8). */}
              <span className="rtty-afc-pill js8-multi" title={t('js8.header.rx.title', { n: rxCount })}>
                {RX_PLATE} {rxCount}/4
              </span>
              {sending && (
                // ⚠️ NOT MIGRATED — the transmit-path deferral: this tooltip states what Stop TX
                // does to a frame in flight. It moves in the JS8 TX batch with the sweeps re-run.
                <span className="rtty-tx-pill" title="JS8 frame on the air (Stop TX aborts it)">
                  {TX_PLATE}
                </span>
              )}
            </>
          }
          bandControl={
            onSetFrequency ? (
              <FrequencyControl
                channels={remote ? context.value?.plan ?? [] : plan}
                dialMhz={snap.radio.dialMhz}
                band={snap.radio.band}
                mode={snap.radio.sideband}
                variant="compact"
                showReadout={false}
                showModeToggle={false}
                onSet={(...args) => { if (canControl) onSetFrequency(...args) }}
              />
            ) : (
              <span className="cockpit-ph-pill" title={t('js8.header.band.title')}>
                {bandLabelForMhz(snap.radio.dialMhz) || '— band —'}
              </span>
            )
          }
          onCommitDial={onSetFrequency ? commitDial : undefined}
          digitTune={onSetFrequency != null}
          wheelSensitivity={wheelSensitivity}
          actions={
            host && panels ? (
              <PanelsMenu
                items={host.menuItems}
                onToggle={(id, show) => panels.setPanelState(id as Js8PanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
                // ⊞ Arrange (layout L3): where each region pane stands. Undo and Reset above cover it.
                lead={
                  panels.movePane ? (
                    <ArrangePanes
                      spec={JS8_PANELS.arrange!}
                      layout={panels.layout}
                      shown={paneShown}
                      labels={labels}
                      onMove={(id, move) => panels.movePane!(id, move, paneShown)}
                      onDrop={dropPane}
                      // "+ Add a box" only where the window lends them (never the hosted Remote page).
                      onAddBox={boxes && panels.addBox ? (area) => panels.addBox?.(area, undefined, boxes.rail?.shows) : undefined}
                      boxesFull={BOX_IDS.every((b) => shown(b))}
                    />
                  ) : undefined
                }
              />
            ) : undefined
          }
        >
          {/* THE ROTOR STRIP, in the header's own control cluster as in the keyboard cockpits.
              → CALL points at the SELECTED station, the heard one the log strip is prefilled
              from, and not at the To box, which also takes group addresses (@ALLCALL) and calls
              nobody has heard. `active`, because this cockpit stays mounted while hidden. Its ■
              stops the rotator, never a transmission. */}
          {canControl || rotatorControl ? <RotorStrip
            active={active}
            onOpenSettings={onOpenSettings}
            targetCall={selected?.call ?? null}
            onPointAt={rotorPointAt(canControl)}
          /> : <span className="dim" role="status" aria-label={t('remote.rotatorUnavailable')} title={t('remote.rotatorUnavailable')}>{t('rotor.strip.aria')} —</span>}
        </CockpitHeader>
      )}

      {/* THE BAND WATERFALL — ⊞-hideable (SCOPE_PANEL_ID). The RX/TX cursors are the engine's
          audio offsets: a click sets RX, right-click/Shift TX, Ctrl/Command both.
          It hosts no stop control and no sender. Its divider (layout L2) sits under it, a shell
          child like Phone's and CW's scope dividers, and goes with it when the strip is hidden:
          the stored height stays, so ticking the waterfall back brings back the height set. */}
      {shown('scope') && (
        <>
        <Waterfall
          stripRef={wfRef}
          {...closeProps('scope')}
          paneTitle={js8PanelLabels().scope}
          theme={theme}
          active={active}
          transmitting={snap?.radio.transmitting ?? false}
          // #230: JS8 paints no dark band either, so its held picture says so — including
          // under a tune carrier, which holds it the same way (see RttyCockpit's note).
          keyed={sending || (snap?.radio.tuning ?? false)}
          rxOffsetHz={snap?.radio.rxOffsetHz ?? 1500}
          txOffsetHz={snap?.radio.txOffsetHz ?? 1500}
          onTune={(hz, target) => {
            if (!canControl) {
              if (target === 'rx') receiverSettings.tuneRx(hz)
              return
            }
            if (target !== 'tx')
              void setRxOffset(hz)
                .then((s) => onSnap?.(s))
                .catch(() => {})
            if (target !== 'rx')
              void setTxOffset(hz)
                .then((s) => onSnap?.(s))
                .catch(() => {})
          }}
        />
        <PaneSeam
          axis="y"
          varName="--js8-wf-h"
          strip={wfRef}
          storageKey="nexus.split.js8.waterfall"
          min={WATERFALL_SPLIT_MIN}
          max={WATERFALL_SPLIT_MAX}
          defaultPct={25}
          label={t('js8.waterfall.splitter.label')}
        />
        </>
      )}

      {/* THE TX STRIP — FT's cluster under the waterfall (2026-10-01): the TX-enable
          latch, Tune, the rig's ATU and Stop TX, sticky so they never leave the window. A shell
          child with no ⊞ id, so hiding the waterfall leaves it directly under the header.
          TUNE — a steady carrier; also a stop control (it stops the carrier it started), so it
          is on this cockpit's stop-line census and its sweep. */}
      {snap && (
        <CockpitTxStrip
          radio={snap.radio}
          onSnap={onSnap}
          onStopTx={stop}
          onSetTxEnabled={onSetTxEnabled ? (on) => { if (canControl) onSetTxEnabled(on) } : undefined}
          onTune={(on) => { if (canControl) void setTune(on).then((s) => onSnap?.(s)) }}
          onAtuTune={() =>
            canControl && void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
        />
      )}

      {js8?.lastError && (
        <div className="cw-keyer-warn" role="alert">
          ⚠ {js8.lastError}
        </div>
      )}

      {/* THE PANE REGION — CW's keyed columns: the log column keeps its key across a 2↔3 flip so
          the LogEntry never remounts mid-entry (the fix-round D1 rule). The column dividers
          (layout L2) ride the region after its columns and move only the boundaries between
          them — Phone's twin, which says why. */}
      <div className="cockpit-panes" ref={panesRef} style={regionColsStyle(panels?.layout.cols)}>
        {/* The columns the placement gives at this tier (`rendered`, above), each ONE flat keyed
            list — a pane that stays in its column keeps its fiber across a tier flip — with the two
            pair dividers riding directly under the pane above them, wherever the pair is adjacent
            (layout L3). The keys and refs of the columns are unchanged: main, aux, log. */}
        {cols === 3 || auxInLogTrack ? (
          <>
            <div className="cockpit-col" key="main" ref={mainColRef}>
              {slots(rendered[0])}
              {foldedRailBoxes(boxes, boxSel, (e) => onScreen.has(e), flow === 'stack')}
            </div>
            <div className="cockpit-col" key="aux" ref={auxColRef}>
              {slots(rendered[1])}
            </div>
            {cols === 3 && (
              <div className="cockpit-col" key="log" ref={logColRef}>
                {slots(rendered[2])}
              </div>
            )}
          </>
        ) : (
          <>
            {(rendered[0].length > 0 || folded.length > 0) && (
              <div className="cockpit-col" key="main" ref={mainColRef}>
                {slots(rendered[0])}
                {foldedRailBoxes(boxes, boxSel, (e) => onScreen.has(e), flow === 'stack')}
              </div>
            )}
            {rendered[1].length > 0 && (
              <div className="cockpit-col" key="log" ref={logColRef}>
                {slots(rendered[1])}
              </div>
            )}
          </>
        )}
        {panels?.setCols && (
          <RegionColumnSeams
            region={panesRef}
            cols={cols}
            tracks={cols === 3 ? [mainColRef, auxColRef, logColRef] : [mainColRef, auxInLogTrack ? auxColRef : logColRef]}
            stored={panels.layout.cols}
            setCols={panels.setCols}
            splitLabel={t('js8.seam.columns.label')}
            widthLabel={auxInLogTrack ? t('js8.seam.auxWidth.label') : t('pane.seam.logWidth.label')}
          />
        )}
        {/* A drag in flight, drawn inside the region (panes/PaneDrag); nothing at all otherwise. */}
        <PaneDropLayer drag={paneDrag} host={panesRef} />
      </div>

      {/* TX DOCK — every transmit control, pinned OUTSIDE the pane region. None has a ⊞ id.
          Stop TX and Tune are up in the TX strip: THE STOP LINE. Everything down here is a
          SENDER (Send, CQ), a second-act arm (HB / AUTOREPLY / RELAY / HB ACK — each is only
          the SECOND act; the session TX latch in the TX strip is the first, and the chip shows
          "armed" only when both agree), a cancel for a reply that has not fired, or Drop
          queue — a SENDER-class control (it empties the queue; a frame already keyed
          finishes) that must never enter the stop-line sweep. */}
      <div className={`cockpit-txdock${canControl ? '' : ' remote-observer-dock'}`}>
        {remote && <div role="status" className="js8-observation-status">{js8 ? t('remote.applicationObserver') : t('remote.collectionUnavailable')}</div>}
        <div className="js8-dock-row js8-compose-row" role="group" aria-label={t('js8.dock.aria')}>
          <input
            className="settings-input rtty-hiscall js8-to"
            list="js8-to-list"
            value={toCall}
            onChange={(e) => setToCall(e.target.value.toUpperCase())}
            placeholder={t('js8.dock.to.placeholder')}
            aria-label={t('js8.dock.to.aria')}
            autoComplete="off"
            spellCheck={false}
          />
          <datalist id="js8-to-list">
            {[ALLCALL, ...(js8?.stations.map((h) => h.call) ?? [])].map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
          {/* THE 32-COMMAND PALETTE: ids are the wire values; labels are the trimmed wire
              texts (invariant tokens). Freetext (31) is a bare space on the wire, so its row
              gets a word. */}
          <select
            className="settings-input js8-cmd-select"
            value={cmdId === null ? '' : String(cmdId)}
            onChange={(e) => setCmdId(e.target.value === '' ? null : Number(e.target.value))}
            aria-label={t('js8.dock.cmd.aria')}
          >
            <option value="">{t('js8.dock.cmd.none')}</option>
            {JS8_COMMANDS.map((c) => (
              <option key={c.id} value={String(c.id)}>
                {c.id === 31 ? t('js8.dock.cmd.freetext') : c.label}
              </option>
            ))}
          </select>
          <input
            ref={composeRef}
            className="settings-input cw-type js8-compose"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return
              e.preventDefault()
              if (canSend) send()
            }}
            placeholder={t('js8.dock.compose.placeholder')}
            aria-label={t('js8.dock.compose.aria')}
            autoComplete="off"
            spellCheck={false}
          />
          <span className={`js8-estimate${overCap ? ' over' : ''}`} title={t('js8.dock.estimate.title')}>
            {estimateText}
          </span>
          <button type="button" className="cw-send-btn js8-send" onClick={send} disabled={!canControl || !canSend}>
            {t('js8.dock.send.label')}
          </button>
        </div>

        <div className="js8-dock-row js8-beacon-row">
          <select
            className="settings-input js8-cq-select"
            value={cqIdx}
            onChange={(e) => setCqIdx(Number(e.target.value))}
            aria-label={t('js8.dock.cq.aria')}
          >
            {JS8_CQS.map((c, i) => (
              <option key={c} value={i}>
                {c}
              </option>
            ))}
          </select>
          {/* CQ — one button, two behaviours, exactly as JS8Call's `cqMacroButton`: a plain
              one-shot at interval 0, and the checkable auto-repeat above it, carrying the
              live countdown in its own label. Turning the repeat off cancels the SCHEDULE;
              it is a sender, not a stop. */}
          {cqRepeats ? (
            <button
              type="button"
              className={`cw-macro rtty-arm js8-arm js8-cq js8-cq-repeat${js8?.cqOn ? ' on' : ''}${js8?.cqOn && js8.armed.cq ? ' armed' : ''}`}
              aria-pressed={js8?.cqOn === true}
              disabled={!canControl} onClick={toggleCqRepeat}
              title={cqRepeatTitle}
            >
              <span className="cw-macro-label">{cqCount ? `${CQ} (${cqCount})` : CQ}</span>
            </button>
          ) : (
            <button type="button" className="cw-macro js8-cq" disabled={!canControl} onClick={callCq} title={t('js8.dock.cq.title')}>
              <span className="cw-macro-label">{CQ}</span>
            </button>
          )}
          <button
            type="button"
            className={`cw-macro rtty-arm js8-arm js8-hb${js8?.hbOn ? ' on' : ''}${js8?.hbOn && js8.armed.hb ? ' armed' : ''}`}
            aria-pressed={js8?.hbOn === true}
            disabled={!canControl}
            onClick={toggleHb}
            title={hbTitle}
          >
            <span className="cw-macro-label">{hbCount ? `${HB} (${hbCount})` : HB}</span>
          </button>
          {armChip('autoreply', 'js8-autoreply', AUTOREPLY, js8?.autoreply === true, js8?.armed.autoreply === true, [
            t('js8.dock.autoreply.title.off'),
            notArmedTitle(js8?.notArmed?.autoreply, t('js8.dock.autoreply.title.on')),
            t('js8.dock.autoreply.title.armed'),
          ])}
          {armChip('relay', 'js8-relay', RELAY, js8?.relay === true, js8?.armed.relay === true, [
            t('js8.dock.relay.title.off'),
            notArmedTitle(js8?.notArmed?.relay, t('js8.dock.relay.title.on'), {
              needsAuto: t('js8.dock.relay.notArmed.needsAuto'),
            }),
            t('js8.dock.relay.title.armed'),
          ])}
          {armChip('hback', 'js8-hback', HB_ACK, js8?.hbAck === true, js8?.armed.hbAck === true, [
            t('js8.dock.hbAck.title.off'),
            notArmedTitle(js8?.notArmed?.hbAck, t('js8.dock.hbAck.title.on'), {
              needsHb: t('js8.dock.hbAck.notArmed.needsHb'),
              needsAuto: t('js8.dock.hbAck.notArmed.needsAuto'),
              needsHbAndAuto: t('js8.dock.hbAck.notArmed.needsHbAndAuto'),
            }),
            t('js8.dock.hbAck.title.armed'),
          ])}
          {/* JS8Call's idle watchdog (60 min default, floor 5, 0 = off): trips HB / AUTOREPLY /
              RELAY off and leaves the TX latch alone. An operator verb restarts it. */}
          {js8 &&
            (js8.idleTripped ? (
              <span className="js8-chip js8-idle tripped" role="alert">
                {t('js8.dock.idle.tripped')}
              </span>
            ) : js8.idleLimitMin === 0 ? (
              <span className="js8-chip js8-idle">{t('js8.dock.idle.off')}</span>
            ) : (
              <span className="js8-chip js8-idle">
                {t('js8.dock.idle', { min: js8.idleMinutes, limit: js8.idleLimitMin })}
              </span>
            ))}
        </div>

        {/* THE AUTOMATIC REPLY THAT ASKS FIRST (JS8Call's AutoreplyConfirmation, on by default):
            Yes queues it for the next period; No, or no answer before the count runs out, sends
            nothing. With TX off it says so: a Yes then keys nothing. With the confirmation off a
            reply never asks: it is in the queue below, and keys in the next period. */}
        {js8?.pendingReply && (
          <div className="js8-dock-row js8-pending-row js8-confirm-row" role="status">
            <span className="js8-pending-text">
              {js8.txEnabled
                ? t('js8.dock.confirm', { text: js8.pendingReply.display })
                : t('js8.dock.pending.txOff', { to: js8.pendingReply.to, text: js8.pendingReply.display })}
            </span>
            <button type="button" className="cw-macro js8-confirm-yes" disabled={!canControl} onClick={() => answerPending(true)} title={t('js8.dock.confirm.yes.title')}>
              {t('js8.dock.confirm.yes.label')}
            </button>
            <button type="button" className="cw-macro js8-confirm-no" disabled={!canControl} onClick={() => answerPending(false)} title={t('js8.dock.confirm.no.title')}>
              {t('js8.dock.confirm.no.label', { secs: pendingSecs })}
            </button>
          </div>
        )}

        {/* THE QUEUE — one frame leaves per period once TX is on. F/L are the i3 First/Last
            flags (tokens). Drop queue is NOT a stop; Stop TX is in the TX strip. */}
        {js8 && js8.queue.length > 0 && (
          <div className="js8-dock-row js8-queue-row" title={t('js8.dock.queue.title')}>
            {js8.queue.map((r, i) => (
              <span key={`${i}-${r.display}`} className={`js8-queue-item origin-${r.origin}`}>
                <span className="js8-chip">{originLabel(r.origin)}</span>
                <span className="js8-queue-text">{r.display}</span>
                {r.first && <span className="js8-chip">F</span>}
                {r.last && <span className="js8-chip">L</span>}
              </span>
            ))}
            <button type="button" className="cw-macro js8-drop" disabled={!canControl} onClick={dropQueue} title={t('js8.dock.queue.drop.title')}>
              {t('js8.dock.queue.drop.label')}
            </button>
          </div>
        )}
      </div>
    </main>
  )
}
