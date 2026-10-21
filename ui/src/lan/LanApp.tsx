// The Stations on this network window's page (src-tauri/src/remote_window.rs, `open_lan_stations_window`):
// the stations on this computer, adding one by signing in with its Nexus Remote account (or, for a
// station whose owner has not signed in there, pairing with its code), and the stream of the one the
// operator opens (the operator's ruling of 2026-10-04, "Window only in v1"). A station signed in to
// from elsewhere (a laptop away from home, a guest who lives elsewhere) is listed apart,
// by its name: Operate drives it over the internet until a sign-in on its row, made on its network,
// adds it there. Served from this computer's own loopback origin, so it reaches no Nexus command;
// everything it does goes on its one socket to this computer's Nexus, which signs in, holds the keys
// and the station records and does the cryptography (src-tauri/src/lan_client.rs).
import { useCallback, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { StreamView } from '../remote-web/StreamView'
import type { StreamEnvironment } from '../remote-web/stream-link'
// A key as the shack shows one (its approval question, Remote access, Remote over this network), so the
// operator compares the two screens by eye.
import { shortFingerprint } from '../remote-web/stream-protocol'
import { LanConnection, lanStream } from './connection'
import { codeShaped, readTold, socketUrl, type ClosedReason, type ConnectReason, type LanAway, type LanChoice, type LanFound, type LanInternet, type LanSigned, type LanStation, type PairReason, type SignInReason, type WhereReason } from './protocol'
import { lanFindLine, lanReachLine, lanUnreached } from '../remote-native/lanReach'
import '../remote-monitor/monitor.css'
import '../remote-web/remote.css'
import '../remote-web/remote-site.css'
import nexusMark from '../remote-web/nexus-mark.svg'

// Invariant tokens: the product name, and an address as a station shows one.
const BRAND = 'Nexus'
const EXAMPLE_ADDRESS = '192.168.1.20:42075'
/** A station computer's name, for the hint where an address or a name is typed. */
const EXAMPLE_NAME = 'shack-pc.local'
/** The longest address or host name with its port (`tempo_stream::lan::Typed`). */
const TYPED_CHARS = 259
// After control was taken back by the station (a decision about the hosted road there), the page
// acquires again once; not again within this long, so a station that keeps refusing is never asked
// in a loop.
const RESUME_EVERY_MS = 30_000
// And only if control shows free within this long of the stream's end: the lease's own heartbeat
// says so within a second or two.
const RESUME_WITHIN_MS = 10_000

/** The page's socket, as the browser gives it: what this page uses of it. */
export type PageSocket = {
  send: (text: string) => void
  close: () => void
  onopen: (() => void) | null
  onmessage: ((event: { data: unknown }) => void) | null
  onclose: (() => void) | null
}

/** Every reason this computer's Nexus can give, in a sentence. Literal calls, so the catalog
 *  guard can see each key. */
function sentence(reason: PairReason | ConnectReason | ClosedReason): string {
  switch (reason) {
    case 'badAddress': return t('lanWindow.reason.badAddress')
    case 'badCode': return t('lanWindow.reason.badCode')
    case 'badName': return t('lanWindow.reason.badName')
    case 'unreachable': return t('lanWindow.reason.unreachable')
    case 'pairingClosed': return t('lanWindow.reason.pairingClosed')
    case 'wrongCode': return t('lanWindow.reason.wrongCode')
    case 'stationProofFailed': return t('lanWindow.reason.stationProofFailed')
    case 'pairingFull': return t('lanWindow.reason.pairingFull')
    case 'stationUnavailable': return t('lanWindow.reason.stationUnavailable')
    case 'updateStation': return t('lanWindow.reason.updateStation')
    case 'updateComputer': return t('lanWindow.reason.updateComputer')
    case 'stationsFull': return t('lanWindow.reason.stationsFull')
    case 'storeUnavailable': return t('lanWindow.reason.storeUnavailable')
    case 'notStation': return t('lanWindow.reason.notStation')
    case 'unavailable': return t('lanWindow.reason.unavailable')
    case 'keyChanged': return t('lanWindow.reason.keyChanged')
    case 'notThisStation': return t('lanWindow.reason.notThisStation')
    case 'notPaired': return t('lanWindow.reason.notPaired')
    case 'unknownStation': return t('lanWindow.reason.unknownStation')
    case 'nameUnresolved': return t('lanWindow.reason.nameUnresolved')
    case 'stationLeft': return t('lanWindow.reason.stationLeft')
    case 'connectionLost': return t('lanWindow.reason.connectionLost')
    case 'disconnected': return t('lanWindow.disconnected')
    // Nothing answered at the address: the card's own words for why (`tempo_stream::lan::unreached`).
    case 'otherNetwork':
    case 'refused':
    case 'noAnswer': return lanReachLine(reason)
  }
}

/** Why a sign-in added nothing, in a sentence. Literal calls, so the catalog guard can see each key. */
function signInSentence(reason: SignInReason): string {
  switch (reason) {
    case 'signInPortBusy': return t('lanWindow.internet.reason.signInPortBusy')
    case 'signInTimedOut': return t('lanWindow.internet.reason.signInTimedOut')
    case 'signInRefused': return t('lanWindow.internet.reason.signInRefused')
    case 'signInUnderWay': return t('lanWindow.internet.reason.signInUnderWay')
    case 'accountNotReady':
    case 'internetNotReady': return t('lanWindow.internet.reason.internetNotReady')
    case 'internetNotIncluded': return t('lanWindow.internet.reason.internetNotIncluded')
    case 'serviceUnavailable':
    case 'issuerUnavailable': return t('lanWindow.internet.reason.serviceUnavailable')
    case 'tryLater':
    case 'rateLimited': return t('lanWindow.internet.reason.tryLater')
    case 'accessDenied':
    case 'notApprovedThere':
    case 'waitingApproval':
    case 'trialDisabled': return t('lanWindow.internet.reason.accessDenied')
    case 'deviceLimit': return t('lanWindow.internet.reason.deviceLimit')
    case 'stationUnavailable': return t('lanWindow.internet.reason.stationUnavailable')
    case 'stationKeyMissing': return t('lanWindow.internet.reason.updateStation')
    case 'stationNotChosen': return t('lanWindow.internet.reason.stationNotChosen')
    case 'stationsFull': return t('lanWindow.reason.stationsFull')
    case 'storeUnavailable': return t('lanWindow.reason.storeUnavailable')
    case 'unknownStation': return t('lanWindow.reason.unknownStation')
    case 'alreadyHere': return t('lanWindow.signIn.reason.alreadyHere')
    case 'listenOnly': return t('lanWindow.signIn.reason.listenOnly')
    case 'stationNotProven': return t('lanWindow.signIn.reason.stationNotProven')
    case 'stationProofFailed': return t('lanWindow.signIn.reason.stationProofFailed')
    case 'notOnThisNetwork': return t('lanWindow.signIn.reason.notOnThisNetwork')
    case 'stationNotSignedIn': return t('lanWindow.signIn.reason.stationNotSignedIn')
    case 'proofNotYours': return t('lanWindow.signIn.reason.proofNotYours')
    case 'proofRefused': return t('lanWindow.signIn.reason.proofRefused')
    case 'stationCouldNotKeep': return t('lanWindow.signIn.reason.stationCouldNotKeep')
    case 'pairingFull': return t('lanWindow.reason.pairingFull')
    case 'updateStation': return t('lanWindow.reason.updateStation')
    case 'updateComputer': return t('lanWindow.reason.updateComputer')
    case 'keyChanged':
    case 'notThisStation': return t('lanWindow.signIn.reason.keyChanged')
    case 'notStation':
    case 'signInFailed': return t('lanWindow.internet.reason.signInFailed')
  }
}

/** Why the address last tried did not take this computer, while a sign-in asks where the station
 *  is. Literal calls, so the catalog guard can see each key. */
function whereSentence(reason: WhereReason): string {
  switch (reason) {
    case 'keyChanged': return t('lanWindow.signIn.reason.keyChanged')
    case 'badAddress': return t('lanWindow.reason.badAddress')
    case 'nameUnresolved': return t('lanWindow.reason.nameUnresolved')
    case 'unreachable': return t('lanWindow.reason.unreachable')
    case 'otherNetwork':
    case 'refused':
    case 'noAnswer': return lanReachLine(reason)
  }
}

/** A day as an operator reads it anywhere: the ISO date, in UTC, never in a locale's format. */
function day(seconds: number): string {
  return new Date(seconds * 1000).toISOString().slice(0, 10)
}

function openSocket(url: string): PageSocket {
  return new WebSocket(url) as unknown as PageSocket
}

type Road = { connection: LanConnection; station: LanStation }

export function LanApp({ open = openSocket, page = typeof location === 'undefined' ? '' : location.href, environment = lanStream }: {
  /** Where the page's socket comes from; a test's own. */
  open?: (url: string) => PageSocket
  /** This page's address: its socket is beside it. */
  page?: string
  /** The stream's browser; a test's own. */
  environment?: () => StreamEnvironment
}) {
  useViewport(1, true)
  const socket = useRef<PageSocket | null>(null)
  const [phase, setPhase] = useState<'opening' | 'open' | 'lost'>('opening')
  const [stations, setStations] = useState<LanStation[]>([])
  const [internet, setInternet] = useState<LanInternet[]>([])
  const [account, setAccount] = useState<LanSigned[]>([])
  // The stations signed in to from elsewhere: kept by their service id until a sign-in here, on
  // their network, adds them there.
  const [away, setAway] = useState<LanAway[]>([])
  const awayIds = useRef<Set<string>>(new Set())
  awayIds.current = new Set(away.map(station => station.id))
  const [computer, setComputer] = useState('')
  const [storeFailed, setStoreFailed] = useState(false)
  const [pairing, setPairing] = useState(false)
  const [address, setAddress] = useState('')
  const [found, setFound] = useState<{ shacks: LanFound[]; available: boolean } | null>(null)
  const [code, setCode] = useState('')
  const [name, setName] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [said, setSaid] = useState<{ text: string; alert: boolean } | null>(null)
  const [road, setRoad] = useState<Road | null>(null)
  const current = useRef<Road | null>(null)
  const [forgetting, setForgetting] = useState<string | null>(null)
  // A sign-in under way: for a new station (null), or for the station on this computer this id names.
  const signingFor = useRef<string | null>(null)
  // The stations added with the account, as last told: a sign-in for one of them is a sign-in again.
  const renewing = useRef<Set<string>>(new Set())
  renewing.current = new Set(account.map(signed => signed.stationId))
  // Signing in for an account with more than one station: which one.
  const [choosing, setChoosing] = useState<LanChoice[] | null>(null)
  // Signing in for a station found nowhere on this network: where it is.
  const [where, setWhere] = useState<{ reason: WhereReason | null; address: string } | null>(null)
  // Operate from this computer, asked first: it closes this Nexus and starts it again driving the
  // station, with this computer's own station off until Leave.
  const [operating, setOperating] = useState<string | null>(null)
  // The address typed for the station after nothing answered, when Operate was asked from there.
  const [operateAt, setOperateAt] = useState<string | null>(null)
  // The address or name Operate tries first for a station, being changed on its row.
  const [changing, setChanging] = useState<{ id: string; address: string } | null>(null)
  const [another, setAnother] = useState<{ id: string; address: string } | null>(null)
  const [resume, setResume] = useState(0)
  const connecting = useRef<string | null>(null)

  const say = useCallback((value: Record<string, unknown>) => {
    try { socket.current?.send(JSON.stringify(value)) } catch { setPhase('lost') }
  }, [])

  useEffect(() => {
    const ws = open(socketUrl(page))
    socket.current = ws
    const leave = () => {
      current.current?.connection.dispose()
      current.current = null
      setRoad(null)
    }
    ws.onopen = () => { setPhase('open'); ws.send(JSON.stringify({ type: 'stations' })) }
    ws.onclose = () => { setPhase('lost'); setBusy(null); setChoosing(null); setWhere(null); leave() }
    ws.onmessage = event => {
      if (typeof event.data !== 'string') return
      let told
      try { told = readTold(JSON.parse(event.data)) } catch { return }
      switch (told.type) {
        case 'stations':
          setStations(told.stations)
          setInternet(told.internet)
          setAccount(told.account)
          setAway(told.away)
          setComputer(told.computer)
          setStoreFailed(told.error !== null)
          if (told.forgetUnreached !== null) setSaid({ text: t('lanWindow.internet.forgetUnreached'), alert: true })
          return
        case 'found':
          setFound({ shacks: told.shacks, available: told.available })
          return
        case 'paired':
          setBusy(null); setPairing(false); setCode('')
          setSaid({ text: t('lanWindow.pairedNow'), alert: false })
          say({ type: 'stations' })
          return
        case 'pairRefused':
          setBusy(null)
          setSaid({ text: sentence(told.reason), alert: true })
          return
        case 'connected': {
          const station = { id: told.road.stationId, addresses: [told.road.address], key: '' }
          const connection = new LanConnection(text => ws.send(text), told.road, environment())
          current.current = { connection, station }
          setBusy(null); setSaid(null); setAnother(null); setResume(0)
          setRoad(current.current)
          return
        }
        case 'connectRefused':
          setBusy(null)
          setSaid({ text: sentence(told.reason), alert: true })
          // Nothing answered where the station was: it may have moved, so offer to type where it is.
          if ((told.reason === 'unreachable' || lanUnreached(told.reason)) && connecting.current) setAnother({ id: connecting.current, address: '' })
          return
        case 'closed':
          leave()
          setSaid({ text: sentence(told.reason), alert: told.reason !== 'disconnected' })
          say({ type: 'stations' })
          return
        case 'answerRefused':
          current.current?.connection.answerRefused(told.reason)
          return
        case 'operating':
          setSaid({ text: t('lanWindow.operating'), alert: false })
          return
        case 'signingIn':
          setSaid({ text: t('lanWindow.internet.signingIn'), alert: false })
          return
        case 'chooseStation':
          setChoosing(told.stations)
          setSaid(null)
          return
        case 'stationAddress':
          setWhere({ reason: told.reason, address: '' })
          setSaid(null)
          return
        case 'signedIn': {
          // Signed in again for a station added with the account; or added (on this network, or
          // over the internet when found nowhere here), or linked.
          const again = signingFor.current !== null && renewing.current.has(signingFor.current)
          setBusy(null); setChoosing(null); setWhere(null)
          setSaid({ text: told.waiting ? t('lanWindow.internet.waiting', { key: shortFingerprint(told.key) }) : told.away ? t('lanWindow.away.added')
            : again ? t('lanWindow.signIn.renewed') : t('lanWindow.signIn.done'), alert: false })
          say({ type: 'stations' })
          return
        }
        case 'signInRefused': {
          // A station signed in to from elsewhere, still not found on this network, stays as it was.
          const stillAway = told.reason === 'notOnThisNetwork' && signingFor.current !== null && awayIds.current.has(signingFor.current)
          setBusy(null); setChoosing(null); setWhere(null)
          setSaid({ text: stillAway ? t('lanWindow.away.stillAway') : signInSentence(told.reason), alert: true })
          return
        }
        case 'station':
          current.current?.connection.receive(told.message, new TextEncoder().encode(event.data).length)
      }
    }
    return () => { leave(); ws.onopen = ws.onclose = ws.onmessage = null; ws.close() }
  }, [open, page, say, environment])

  // Control taken back by the station as a side effect of a decision about the hosted road (Turn
  // Remote on or off there, revoking a browser), which gives this computer its grant back at once.
  // The stream ends `notController`, the lease's next heartbeat is refused, and the next state
  // shows control free: then the view starts again by itself, once, and acquires. Control held by
  // another device, or not allowed, is never fought for, and any other end stays an end until the
  // operator asks, as on the hosted road.
  const lastResume = useRef(-Infinity)
  useEffect(() => {
    if (!road) return
    const { stream: link, operations } = road.connection
    let wasRunning = false
    let pending: number | null = null
    const check = () => {
      if (pending === null) return
      const ops = operations.getSnapshot()
      if (performance.now() - pending > RESUME_WITHIN_MS) { pending = null; return }
      if (!ops.connected || !ops.fresh || !ops.state || ops.state.phase === 'controlling') return
      if (ops.state.phase === 'available' && ops.state.allowed) setResume(n => n + 1)
      pending = null
    }
    const offLink = link.subscribe(() => {
      const view = link.getSnapshot()
      if (view.phase === 'connecting' || view.phase === 'live' || view.phase === 'stalled') { wasRunning = true; return }
      if (view.phase !== 'ended' || !wasRunning) return
      wasRunning = false
      if (view.reason !== 'notController' || performance.now() - lastResume.current < RESUME_EVERY_MS) return
      lastResume.current = pending = performance.now()
      check()
    })
    const offOps = operations.subscribe(check)
    return () => { offLink(); offOps() }
  }, [road])

  // The pairing dialog's address field offers the stations a look by name finds (the operator's
  // ruling of 2026-10-04, "By name, or typed"); typing an address always works, found or not.
  const look = () => {
    setFound(null)
    say({ type: 'find' })
  }

  const signIn = (id: string | null) => {
    signingFor.current = id
    setBusy('signIn'); setSaid(null); setPairing(false)
    say({ type: 'signIn', stationId: id })
  }

  const connect = (id: string, typed?: string) => {
    connecting.current = id
    setBusy(`connect:${id}`)
    setSaid(null)
    say(typed ? { type: 'connect', stationId: id, address: typed } : { type: 'connect', stationId: id })
  }

  if (road) {
    return <StreamView key={resume} connection={road.connection} autostart
      station={t('lanWindow.stationAt', { address: road.station.addresses[0] })}
      disconnect={() => say({ type: 'disconnect' })} />
  }

  const shownName = name ?? computer
  const pairReady = !busy && phase === 'open' && address.trim() !== '' && codeShaped(code) && shownName.trim() !== ''
  return <div className="app remote-monitor-app remote-service-app remote-site">
    <header className="rm-header"><span className="remote-site-wordmark">
      <img className="remote-site-mark" src={nexusMark} alt="" width={28} height={28} />
      <strong>{BRAND}</strong>
    </span></header>
    <main className="rm-scroll" aria-label={t('lanWindow.title')}><div className="rm-content remote-account">
      <h1>{t('lanWindow.title')}</h1>
      <p className="remote-site-lead">{t('lanWindow.intro')}</p>
      {phase === 'opening' && <p className="remote-site-status" role="status">{t('lanWindow.opening')}</p>}
      {phase === 'lost' && <p className="rm-warning" role="alert">{t('lanWindow.lost')}</p>}
      {storeFailed && <p className="rm-warning" role="alert">{t('lanWindow.reason.storeUnavailable')}</p>}
      {said && <p className={said.alert ? 'rm-warning' : 'remote-site-status'} role={said.alert ? 'alert' : 'status'}>{said.text}</p>}
      {choosing && <section className="rm-card remote-section remote-site-card--action" aria-label={t('lanWindow.internet.chooseTitle')}>
        <h2>{t('lanWindow.internet.chooseTitle')}</h2>
        <p>{signingFor.current === null ? t('lanWindow.signIn.choose') : t('lanWindow.internet.choose')}</p>
        <div className="remote-actions">{choosing.map(choice =>
          <button type="button" key={choice.id} className="remote-button"
            onClick={() => { setChoosing(null); say({ type: 'chooseStation', stationId: choice.id }) }}>{choice.name || choice.id}</button>)}
          <button type="button" className="remote-button remote-button--quiet"
            onClick={() => { setChoosing(null); say({ type: 'chooseStation', stationId: null }) }}>{t('lanWindow.cancel')}</button>
        </div>
      </section>}
      {where && <section className="rm-card remote-section remote-site-card--action" aria-label={t('lanWindow.signIn.whereTitle')}>
        <h2>{t('lanWindow.signIn.whereTitle')}</h2>
        <p>{t('lanWindow.signIn.where')}</p>
        {where.reason && <p className="rm-warning" role="alert">{whereSentence(where.reason)}</p>}
        {/* Guarded on the submit too: Enter in a field submits a form whose button is disabled. */}
        <form onSubmit={event => {
          event.preventDefault()
          if (!where.address.trim()) return
          setWhere(null)
          say({ type: 'stationAddress', address: where.address.trim() })
        }}>
          <label>{t('lanWindow.address')}<input autoComplete="off" spellCheck={false} value={where.address} maxLength={TYPED_CHARS}
            onChange={event => setWhere({ ...where, address: event.target.value })} /></label>
          <span className="settings-hint">{t('lanWindow.otherAddressHint', { example: EXAMPLE_ADDRESS, name: EXAMPLE_NAME })}</span>
          <div className="remote-actions">
            <button className="remote-button remote-button--primary" disabled={!where.address.trim()}>{t('lanWindow.signIn.tryAddress')}</button>
            {/* For a station being added, or one signed in to from elsewhere, the other answer is that it
                is not here: it is then kept, or stays, over the internet. */}
            <button type="button" className="remote-button remote-button--quiet"
              onClick={() => { setWhere(null); say({ type: 'stationAddress', address: null }) }}>
              {signingFor.current === null || awayIds.current.has(signingFor.current) ? t('lanWindow.signIn.notHere') : t('lanWindow.cancel')}</button>
          </div>
        </form>
      </section>}
      <section className="rm-card remote-section">
        <h2>{t('lanWindow.paired')}</h2>
        {stations.length === 0 && away.length === 0 && <p>{t('lanWindow.none')}</p>}
        {stations.map(station => { const internetKey = internet.find(added => added.id === station.id)?.key
          return <div key={station.id} className="remote-station lan-station">
          <p><strong>{t('lanWindow.stationAt', { address: station.addresses[0] })}</strong></p>
          <p className="remote-site-status">{t('lanWindow.stationKey', { key: shortFingerprint(station.key) })}</p>
          {forgetting === station.id ? <>
            <p className="remote-warning">{t('lanWindow.forgetConfirm')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={!!busy} onClick={() => { setForgetting(null); say({ type: 'forget', stationId: station.id }) }}>{t('lanWindow.forgetYes')}</button>
              <button type="button" className="remote-button remote-button--quiet" onClick={() => setForgetting(null)}>{t('lanWindow.keep')}</button>
            </div>
          </> : operating === station.id ? <>
            <p className="remote-warning">{t('lanWindow.operateConfirm')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={!!busy || phase !== 'open'} onClick={() => {
                setOperating(null); setBusy(`operate:${station.id}`); setSaid(null)
                say(operateAt ? { type: 'operate', stationId: station.id, address: operateAt } : { type: 'operate', stationId: station.id })
              }}>{t('lanWindow.operateYes')}</button>
              <button type="button" className="remote-button remote-button--quiet" onClick={() => setOperating(null)}>{t('lanWindow.operateNo')}</button>
            </div>
          </> : <>
            {/* Operating is the main action: Nexus runs here and the radio takes this computer's clicks and
                pop-outs. The stream, a picture of the station's screen, is only the fallback. */}
            <div className="remote-actions">
              <button type="button" className="remote-button remote-button--primary" disabled={!!busy || phase !== 'open'} onClick={() => { setOperateAt(null); setOperating(station.id) }}>{t('lanWindow.operate')}</button>
              <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'} onClick={() => connect(station.id)}>
                {busy === `connect:${station.id}` ? t('lanWindow.connecting') : t('lanWindow.streamInstead')}</button>
              <button type="button" className="remote-button remote-button--quiet" disabled={!!busy} onClick={() => setForgetting(station.id)}>{t('lanWindow.forget')}</button>
            </div>
            <p className="settings-hint">{t('lanWindow.choiceHint')}</p>
          </>}
          {(() => {
            const signed = account.find(entry => entry.stationId === station.id)
            const settled = forgetting !== station.id && operating !== station.id
            // With the road added, this computer's key for it: the station shows the same one when it
            // asks to approve this computer, and under Remote access ▸ Your computers.
            const computerKey = internetKey !== undefined
              && <p className="remote-site-status">{t('lanWindow.internet.computerKey', { key: shortFingerprint(internetKey) })}</p>
            // Added with the account: until when the station takes this computer on its network, and
            // in the last week before that, signing in again.
            if (signed) return signed.renew
              ? <>
                <p className="remote-warning">{t('lanWindow.signIn.renew', { date: day(signed.until) })}</p>
                {computerKey}
                {settled && <div className="remote-actions">
                  <button type="button" className="remote-button" disabled={!!busy || phase !== 'open'} onClick={() => signIn(station.id)}>
                    {busy === 'signIn' && signingFor.current === station.id ? t('lanWindow.internet.adding') : t('remote.account.signInAgain')}</button>
                </div>}
              </>
              : <>
                <p className="remote-site-status">{t('lanWindow.signIn.until', { date: day(signed.until) })}</p>
                {computerKey}
              </>
            // Paired with a code: signing in links it to the account, or adds its internet road.
            return <>
              {internetKey !== undefined && <p className="remote-site-status">{t('lanWindow.internet.added')}</p>}
              {computerKey}
              {settled && <div className="remote-actions">
                <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'} title={t('lanWindow.internet.hint')}
                  onClick={() => signIn(station.id)}>
                  {busy === 'signIn' && signingFor.current === station.id ? t('lanWindow.internet.adding') : t('lanWindow.signIn')}</button>
              </div>}
            </>
          })()}
          {/* Kept once Operate opened there, and tried first by Operate from then on: changed or cleared here. */}
          {station.typedAddress !== undefined && forgetting !== station.id && operating !== station.id && (changing?.id === station.id
            ? <form onSubmit={event => {
              event.preventDefault()
              if (changing.address.trim()) { setChanging(null); setSaid(null); say({ type: 'keepAddress', stationId: station.id, address: changing.address.trim() }) }
            }}>
              <label>{t('lanWindow.typedLabel')}<input autoComplete="off" spellCheck={false} value={changing.address} maxLength={TYPED_CHARS}
                onChange={event => setChanging({ id: station.id, address: event.target.value })} /></label>
              <span className="settings-hint">{t('lanWindow.otherAddressHint', { example: EXAMPLE_ADDRESS, name: EXAMPLE_NAME })}</span>
              <div className="remote-actions">
                <button className="remote-button" disabled={!!busy || phase !== 'open' || !changing.address.trim()}>{t('lanWindow.typedSave')}</button>
                <button type="button" className="remote-button remote-button--quiet" onClick={() => setChanging(null)}>{t('lanWindow.cancel')}</button>
              </div>
            </form>
            : <>
              <p className="remote-site-status">{t('lanWindow.typedFirst', { address: station.typedAddress })}</p>
              <div className="remote-actions">
                <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'}
                  onClick={() => setChanging({ id: station.id, address: station.typedAddress ?? '' })}>{t('lanWindow.typedChange')}</button>
                <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'}
                  onClick={() => { setSaid(null); say({ type: 'keepAddress', stationId: station.id, address: null }) }}>{t('lanWindow.typedClear')}</button>
              </div>
            </>)}
          {/* Typed where the station is now: Operate there, as from the card, or the stream there instead. */}
          {another?.id === station.id && <form onSubmit={event => {
            event.preventDefault()
            if (another.address.trim()) { setOperateAt(another.address.trim()); setOperating(station.id) }
          }}>
            <label>{t('lanWindow.otherAddress')}<input autoComplete="off" spellCheck={false} value={another.address} maxLength={TYPED_CHARS}
              onChange={event => setAnother({ id: station.id, address: event.target.value })} /></label>
            <span className="settings-hint">{t('lanWindow.otherAddressHint', { example: EXAMPLE_ADDRESS, name: EXAMPLE_NAME })}</span>
            <div className="remote-actions">
              <button className="remote-button" disabled={!!busy || phase !== 'open' || !another.address.trim()}>{t('lanWindow.operateAt')}</button>
              <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || !another.address.trim()}
                onClick={() => connect(station.id, another.address.trim())}>{t('lanWindow.stream')}</button>
            </div>
          </form>}
        </div> })}
        {/* Signed in to from elsewhere: by its name, Operate over the internet, and Sign in on its row
            once on its network to add it there. No stream: that is this network's road alone. */}
        {away.map(station => <div key={station.id} className="remote-station lan-station">
          {station.name && <p><strong>{station.name}</strong></p>}
          <p className="remote-site-status">{t('lanWindow.stationKey', { key: shortFingerprint(station.key) })}</p>
          {forgetting === station.id ? <>
            <p className="remote-warning">{t('lanWindow.forgetConfirm')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={!!busy} onClick={() => { setForgetting(null); say({ type: 'forget', stationId: station.id }) }}>{t('lanWindow.forgetYes')}</button>
              <button type="button" className="remote-button remote-button--quiet" onClick={() => setForgetting(null)}>{t('lanWindow.keep')}</button>
            </div>
          </> : operating === station.id ? <>
            <p className="remote-warning">{t('lanWindow.away.operateConfirm')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={!!busy || phase !== 'open'} onClick={() => {
                setOperating(null); setBusy(`operate:${station.id}`); setSaid(null)
                say({ type: 'operate', stationId: station.id })
              }}>{t('lanWindow.operateYes')}</button>
              <button type="button" className="remote-button remote-button--quiet" onClick={() => setOperating(null)}>{t('lanWindow.operateNo')}</button>
            </div>
          </> : <>
            <p className="remote-site-status">{t('lanWindow.away.status')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button remote-button--primary" disabled={!!busy || phase !== 'open'} onClick={() => setOperating(station.id)}>{t('lanWindow.operate')}</button>
              <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'} onClick={() => signIn(station.id)}>
                {busy === 'signIn' && signingFor.current === station.id ? t('lanWindow.internet.adding') : t('lanWindow.signIn')}</button>
              <button type="button" className="remote-button remote-button--quiet" disabled={!!busy} onClick={() => setForgetting(station.id)}>{t('lanWindow.forget')}</button>
            </div>
          </>}
        </div>)}
        {/* Signing in is how a station is added. A code stays for a station whose owner has not signed
            in to Nexus Remote at the station, until every station has. */}
        <div className="remote-actions">
          <button type="button" className="remote-button remote-button--primary" disabled={!!busy || phase !== 'open'} onClick={() => signIn(null)}>
            {busy === 'signIn' && signingFor.current === null ? t('lanWindow.internet.adding') : t('lanWindow.signIn')}</button>
        </div>
        <p className="settings-hint">{t('lanWindow.signInHint')}</p>
        {!pairing && <>
          <div className="remote-actions">
            <button type="button" className="remote-button remote-button--quiet" disabled={!!busy || phase !== 'open'} onClick={() => { setPairing(true); setSaid(null); look() }}>{t('lanWindow.pair')}</button>
          </div>
          <p className="settings-hint">{t('lanWindow.pairCodeHint')}</p>
        </>}
      </section>
      {pairing && <section className="rm-card remote-section remote-site-card--action" aria-label={t('lanWindow.pairTitle')}>
        <h2>{t('lanWindow.pairTitle')}</h2>
        <p>{t('lanWindow.pairHint')}</p>
        {/* Guarded on the submit too: Enter in a field submits a form whose button is disabled. */}
        <form onSubmit={event => {
          event.preventDefault()
          if (!pairReady) return
          setBusy('pair'); setSaid(null)
          say({ type: 'pair', address: address.trim(), code, name: shownName.trim() })
        }}>
          <label>{t('lanWindow.address')}<input autoComplete="off" spellCheck={false} value={address} maxLength={21} required
            onChange={event => setAddress(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.addressHint', { example: EXAMPLE_ADDRESS })}</span>
          {found === null
            ? <p className="remote-site-status" role="status">{t('lanWindow.finding')}</p>
            : found.shacks.length === 0
              ? <p className="settings-hint">{lanFindLine(found.available)}</p>
              : <div role="group" aria-labelledby="lan-found">
                <span id="lan-found" className="settings-hint">{t('lanWindow.found')}</span>
                <div className="remote-actions">{found.shacks.map(shack =>
                  <button type="button" key={shack.name} className="remote-button remote-button--quiet"
                    aria-pressed={address.trim() === shack.address} onClick={() => setAddress(shack.address)}>
                    {t('lanWindow.foundAt', { name: shack.name, address: shack.address })}</button>)}</div>
              </div>}
          {found?.available && <div className="remote-actions">
            <button type="button" className="remote-button remote-button--quiet" onClick={look}>{t('lanWindow.findAgain')}</button>
          </div>}
          <label>{t('lanWindow.code')}<input className="remote-site-code-input" autoComplete="off" spellCheck={false} value={code} maxLength={24} required
            onChange={event => setCode(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.codeHint')}</span>
          <label>{t('lanWindow.name')}<input autoComplete="off" value={shownName} maxLength={32} required
            onChange={event => setName(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.nameHint')}</span>
          <div className="remote-actions">
            <button className="remote-button remote-button--primary" disabled={!pairReady}>{busy === 'pair' ? t('lanWindow.pairing') : t('lanWindow.pairSubmit')}</button>
            <button type="button" className="remote-button remote-button--quiet" disabled={busy === 'pair'} onClick={() => setPairing(false)}>{t('lanWindow.cancel')}</button>
          </div>
        </form>
      </section>}
    </div></main>
  </div>
}
