// The banner a computer shows while it drives a paired station (client mode): which station, how
// the link stands, and Leave.
//
// It renders nothing on a computer running its own station: the backend answers `null`, once, and
// the banner never asks again. Nor anywhere but the desktop app itself: the hosted page and the TV
// page mount the same App over transports that have no such question. While driving a station it
// asks every half second, because the link's state changes under it: connecting, live, not live
// (the panes keep the station's last state while Nexus connects again by itself), another device
// in control, or refused, with why.
//
// An in-flow strip, a child of `.app` like the other banners: it takes its own height from the
// shell below it and covers nothing, so no control of any cockpit, Stop included, is ever under it.
//
// It also says what the station last said of this computer's over (Stop's road): the holes on
// the air in it so far, in amber, why it ended, or that the station has gone quiet while a press
// is held. The short tone that goes with an end is played by Nexus itself, in this computer's
// headset.
//
// In a test build it carries the road switch too (`src-tauri/src/bench_road_switch.rs`): Normal,
// Block the command link, Block the Stop road, one click each, and a mark while a road is blocked.
// Only a test build's status names the switch, so a shipped build draws none of it.

import { useEffect, useState } from 'react'
import {
  benchRoadSwitch,
  clientStatus,
  isTauri,
  leaveClientMode,
  type BenchPosition,
  type ClientOver,
  type ClientStatus,
} from '../api'
import { t } from '../i18n'
import { lanReachLine } from '../remote-native/lanReach'

/** How often the banner asks after the link while this computer drives a station. */
export const CLIENT_STATUS_EVERY_MS = 500

/** Why the station did not take this computer, in a sentence. Literal calls, so the catalog
 *  guard can see each key; a reason this build cannot name is said as such. */
function reasonLine(reason: string | null): string {
  switch (reason) {
    case 'unreachable': return t('lanWindow.reason.unreachable')
    case 'otherNetwork':
    case 'refused':
    case 'noAnswer': return lanReachLine(reason)
    case 'keyChanged': return t('lanWindow.reason.keyChanged')
    case 'notThisStation': return t('lanWindow.reason.notThisStation')
    case 'notPaired': return t('lanWindow.reason.notPaired')
    case 'notStation': return t('lanWindow.reason.notStation')
    case 'updateStation': return t('lanWindow.reason.updateStation')
    case 'updateComputer': return t('lanWindow.reason.updateComputer')
    case 'storeUnavailable': return t('lanWindow.reason.storeUnavailable')
    case 'unknownStation': return t('lanWindow.reason.unknownStation')
    case 'otherBuild': return t('client.reason.otherBuild')
    case 'localPermissionRequired': return t('client.reason.localPermissionRequired')
    default: return t('client.reason.unknown')
  }
}

/** Why an over ended, in a sentence. Literal calls, so the catalog guard can see each key. */
function endedLine(why: Extract<ClientOver, { phase: 'ended' }>['why']): string {
  switch (why) {
    case 'audioGap': return t('client.over.ended.audioGap')
    case 'pttGap': return t('client.over.ended.pttGap')
    case 'stopped': return t('client.over.ended.stopped')
    case 'presence': return t('client.over.ended.presence')
    case 'ceiling': return t('client.over.ended.ceiling')
    case 'watchdog': return t('client.over.ended.watchdog')
    case 'routeChanged': return t('client.over.ended.routeChanged')
    case 'refused': return t('client.over.ended.refused')
  }
}

/** This computer's over, in a sentence, when there is anything to say of it: an over on the air
 *  with no holes says nothing. The seconds are invariant, as every number on screen is. */
export function overLine(over: ClientOver | null | undefined): string | null {
  if (!over) return null
  switch (over.phase) {
    case 'onAir':
      return over.holes > 0
        ? t('client.over.gaps', { count: over.holes, seconds: (over.holeMs / 1000).toFixed(1) })
        : null
    case 'noWord': return t('client.over.noWord')
    case 'ended': return endedLine(over.why)
  }
}

function line(status: ClientStatus): string {
  const station = status.station
  switch (status.phase) {
    case 'connecting': return t('client.banner.connecting', { station })
    case 'live': return t('client.banner.live', { station })
    case 'notLive': return t('client.banner.notLive', { station })
    case 'busy': return t('client.banner.busy', { station })
    case 'refused':
      return t('client.banner.refused', { station, reason: reasonLine(status.reason) })
  }
}

/** The road switch of a test build. Each move draws the switch's own answer at once; a move it
 *  refuses changes nothing, and the next ask after the link says where it is. A road can be
 *  blocked only while the other is open, as the switch itself allows. */
function RoadSwitch({
  position,
  moved,
}: {
  position: BenchPosition
  moved: (now: BenchPosition) => void
}) {
  const move = (to: BenchPosition) => {
    void benchRoadSwitch(to).then(moved, () => undefined)
  }
  return (
    <span className="client-banner-bench" role="group" aria-label={t('client.bench.label')}>
      <span className="client-banner-bench-label" aria-hidden="true">
        {t('client.bench.label')}
      </span>
      <button type="button" aria-pressed={position === 'normal'} onClick={() => move('normal')}>
        {t('client.bench.normal')}
      </button>
      <button
        type="button"
        aria-pressed={position === 'blockLink'}
        disabled={position === 'blockStopRoad'}
        onClick={() => move('blockLink')}
      >
        {t('client.bench.blockLink')}
      </button>
      <button
        type="button"
        aria-pressed={position === 'blockStopRoad'}
        disabled={position === 'blockLink'}
        onClick={() => move('blockStopRoad')}
      >
        {t('client.bench.blockStopRoad')}
      </button>
      {position === 'blockLink' && (
        <span className="client-banner-bench-mark">{t('client.bench.blocked.link')}</span>
      )}
      {position === 'blockStopRoad' && (
        <span className="client-banner-bench-mark">{t('client.bench.blocked.stopRoad')}</span>
      )}
    </span>
  )
}

export function ClientBanner() {
  const [status, setStatus] = useState<ClientStatus | null>(null)
  const [leaving, setLeaving] = useState(false)

  useEffect(() => {
    if (!isTauri()) return
    let alive = true
    let timer: ReturnType<typeof setTimeout> | null = null
    const ask = async () => {
      const now = await clientStatus().catch(() => null)
      if (!alive) return
      setStatus(now)
      // A computer running its own station answers null: asked once, never again.
      if (now) timer = setTimeout(() => void ask(), CLIENT_STATUS_EVERY_MS)
    }
    void ask()
    return () => {
      alive = false
      if (timer) clearTimeout(timer)
    }
  }, [])

  if (!status) return null
  const over = overLine(status.over)
  const leave = () => {
    setLeaving(true)
    void leaveClientMode().catch(() => setLeaving(false))
  }
  const bench = status.bench
  return (
    <div
      className="client-banner"
      data-phase={status.phase}
      data-bench={bench}
      role="status"
      aria-live="polite"
    >
      <span className="client-banner-line">{line(status)}</span>
      {over && (
        <span className="client-banner-over" data-over={status.over?.phase}>
          {over}
        </span>
      )}
      {bench && (
        <RoadSwitch
          position={bench}
          moved={(now) => setStatus((was) => (was ? { ...was, bench: now } : was))}
        />
      )}
      <button
        type="button"
        className="client-banner-leave"
        onClick={leave}
        disabled={leaving}
        title={t('client.banner.leave.title')}
      >
        {t('client.banner.leave')}
      </button>
    </div>
  )
}
