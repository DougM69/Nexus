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

import { useEffect, useState } from 'react'
import { clientStatus, isTauri, leaveClientMode, type ClientStatus } from '../api'
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
  const leave = () => {
    setLeaving(true)
    void leaveClientMode().catch(() => setLeaving(false))
  }
  return (
    <div className="client-banner" data-phase={status.phase} role="status" aria-live="polite">
      <span className="client-banner-line">{line(status)}</span>
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
