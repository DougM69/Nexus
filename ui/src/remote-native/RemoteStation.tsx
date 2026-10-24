import { useEffect, useRef, useState } from 'react'
import { answerRemoteAutostartOffer, getRemoteStationStatus, getSettings, remoteStationAction, setLaunchAtLogin } from '../api'
import { t } from '../i18n'
import { isNative, PLATFORM_NAMES, waitsForApprove, type AccountRole, type RemoteStationAction, type RemoteStationStatus } from './types'
import { shortFingerprint } from '../remote-web/stream-protocol'
import '../remote-web/remote.css'

// Browser approval lifetime: the shack warns this long before the end that use cannot move.
const APPROVAL_WARNING_MS = 7 * 86400000
// A UTC calendar date, never locale formatting: a date an operator plans around must not read
// differently from one country to the next (the browser page does the same).
const utcDate = (ms: number) => new Date(ms).toISOString().slice(0, 10)

export function RemoteStation() {
  const [status, setStatus] = useState<RemoteStationStatus | null>(null)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [offer, setOffer] = useState(false)
  const [offerFailed, setOfferFailed] = useState(false)
  // People: the address and role of the next invitation.
  const [inviteEmail, setInviteEmail] = useState('')
  const [inviteRole, setInviteRole] = useState<AccountRole>('operate')
  const pending = useRef(false), mounted = useRef(true), requestEpoch = useRef(0)
  useEffect(() => {
    mounted.current = true
    const initial = ++requestEpoch.current
    void (async () => { try { const next = await getRemoteStationStatus(); if (mounted.current && initial === requestEpoch.current) setStatus(next) } catch { if (mounted.current && initial === requestEpoch.current) setError('serviceUnavailable') } })()
    const timer = setInterval(() => {
      if (pending.current) return
      pending.current = true
      const epoch = ++requestEpoch.current
      void (async () => {
        try { const next = await remoteStationAction({ type: 'refresh' }); if (mounted.current && epoch === requestEpoch.current) setStatus(next) }
        catch (error) { if (mounted.current && epoch === requestEpoch.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
        finally { if (epoch === requestEpoch.current) pending.current = false }
      })()
    }, 5000)
    return () => { mounted.current = false; requestEpoch.current++; pending.current = false; clearInterval(timer) }
  }, [])
  // The sign-in / pairing buttons are dead until a station name is typed, and the field starts
  // empty — so on arrival there is nothing to do and no hint that there should be. Prefill the
  // field with the operator's callsign (the first-hour setting every station already has) so the
  // buttons are live at once; the operator can still edit it, and if there is no callsign the
  // inline hint in the unpaired block asks for a name. Done once, and only while the field is
  // still untouched, so it never overwrites an edit.
  const prefilled = useRef(false)
  useEffect(() => {
    void (async () => {
      try {
        const settings = await getSettings()
        if (!mounted.current || prefilled.current) return
        prefilled.current = true
        const call = settings.mycall?.trim()
        if (call) setName(prev => (prev === '' ? call : prev))
      } catch { /* no settings: the hint covers an empty field */ }
    })()
  }, [])
  async function act(action: RemoteStationAction) {
    // Disable cancels the native socket even while an HTTP request is pending.
    if (pending.current && action.type !== 'disable' && action.type !== 'takeOverLogging') return
    const epoch = ++requestEpoch.current
    pending.current = true; setBusy(true); setError(null)
    try {
      const next = await remoteStationAction(action)
      if (mounted.current && epoch === requestEpoch.current) setStatus(next)
      // Approving the pairing turns Remote on too, so it is the same moment to offer.
      if (action.type === 'enable' || (action.type === 'approve' && ['connected', 'connecting', 'reconnecting'].includes(next.phase))) void offerStartAtSignIn()
    }
    catch (error) { if (mounted.current && epoch === requestEpoch.current) setError(typeof error === 'string' ? error : 'serviceUnavailable') }
    finally { if (epoch === requestEpoch.current) { pending.current = false; if (mounted.current) setBusy(false) } }
  }
  // Remote comes back after a restart only if Nexus itself starts. So when the operator turns Remote
  // on (or approves the pairing, which turns it on), and Nexus is not set to start at sign-in, offer
  // it: once, only on that click (never on a
  // launch that turned Remote back on by itself), never switched on without a click, and never
  // asked again once answered either way.
  async function offerStartAtSignIn() {
    try {
      const settings = await getSettings()
      if (mounted.current && !settings.launchAtLogin && !settings.remoteAutostartOfferAnswered) setOffer(true)
    } catch { /* no settings, no offer: the switch in Settings is always there */ }
  }
  async function answerOffer(accept: boolean) {
    setOffer(false); setOfferFailed(false)
    try { await answerRemoteAutostartOffer() } catch { /* closed for this session either way */ }
    if (!accept) return
    try { await setLaunchAtLogin(true) } catch { if (mounted.current) setOfferFailed(true) }
  }
  const connected = status && ['connected', 'connecting', 'reconnecting'].includes(status.phase)
  const issue = error ?? status?.error, phase = status?.phase
  // The browsers, and the computers running Nexus (the native client) apart from them: a computer's
  // approval pins its key and grants nothing on the relay's lanes, so it has no switches, here or
  // under Advanced.
  // Read on every render, so a status with no list at all is a station with no devices, never a crash
  // that takes the whole of Settings down with this card.
  const browsers = status?.devices?.filter(device => !isNative(device)) ?? []
  const computers = status?.devices?.filter(isNative) ?? []
  const account = status?.account
  const roleName = (role: AccountRole) => role === 'listen' ? t('remote.account.role.listen') : t('remote.account.role.operate')
  // What the station knows of a device beyond its approval: added by its own sign-in, a proof the station
  // could not confirm (never granted), or a removal the service has not heard yet. A device added by its
  // sign-in that waits for Approve here was confirmed, so it says only how it was added.
  const accountNotes = (device: RemoteStationStatus['devices'][number]) => <>
    {device.addedBy === 'account' && <span>{t('remote.account.addedByAccount')}</span>}
    {account?.unconfirmed?.[device.id] && !waitsForApprove(status, device.id)
      && <span className="remote-warning">{t('remote.account.unconfirmed')}</span>}
    {account?.removals?.includes(device.id) && <span>{t('remote.account.removalWaiting')}</span>}
  </>
  // One card (the operator, 2026-10-02: "Settings ▸ Remote access becomes one card"): where this
  // station stands with the Remote service and the one thing to do next, then the browsers with their
  // keys. Everything else the panel had folds under Advanced, unchanged: the same state, the same
  // actions, the same defaults. Nothing here grants anything the old panel did not.
  return <div className="remote-section remote-native">
    <p role="status">{phase === 'unpaired' ? t('remote.unpaired') : phase === 'pairing' ? t('remote.pairing')
      : phase === 'approval' ? t('remote.localApproval') : phase === 'disabled' ? t('remote.disabled')
      : phase === 'connected' ? t('remote.connected') : phase === 'reconnecting' ? t('remote.reconnecting') : t('monitor.connecting')}</p>
    {/* Every refusal except the vault one used to render "the request could not be completed",
        which points at the network. A station whose access was revoked from a browser, or whose
        service access has run out, has not got a network problem and cannot fix one. */}
    {issue && <p role="alert">{
      issue === 'credentialStoreUnavailable' ? t('remote.vaultFailed')
      : issue === 'stationRevoked' ? t('remote.nativeRevoked')
      // The service returns these two only from approval, so the station was never attached. They
      // need different next steps: an ended trial is not an account that was switched off.
      : issue === 'trialEnded' ? t('remote.nativeTrialEnded')
      : issue === 'trialDisabled' ? t('remote.nativeTrialDisabled')
      // One trial per callsign and per shack PC: approval refused for the callsign this Nexus sent, or
      // for this PC, each with what the operator can do about it here. And a service that needs a newer
      // Nexus to approve at all.
      : issue === 'callsignInvalid' ? t('remote.nativeCallsignInvalid')
      : issue === 'callsignNotLicensed' ? t('remote.nativeCallsignNotLicensed')
      : issue === 'trialUsedByCallsign' ? t('remote.nativeTrialUsedByCallsign')
      : issue === 'trialUsedOnThisPc' ? t('remote.nativeTrialUsedOnThisPc')
      : issue === 'updateRequired' ? t('remote.nativeUpdateRequired')
      // Approval refused because this mailbox already has a trial under another sign-in. It used to
      // read as access denied, which does not say that the sign-in is what needs changing.
      : issue === 'trialActiveElsewhere' ? t('remote.nativeTrialActiveElsewhere')
      // Approving at the shack before the browser agreed to attach the station. Falling through to
      // requestFailed would point an operator standing at the radio at their network.
      : issue === 'awaitingConfirmation' ? t('remote.nativeAwaitingConfirmation')
      // Signing in at the shack, and inviting people.
      : issue === 'accountNotReady' ? t('remote.account.notReady')
      : issue === 'signInPortBusy' ? t('remote.account.signInPortBusy')
      : issue === 'signInFailed' || issue === 'proofInvalid' || issue.startsWith('token') ? t('remote.account.signInFailed')
      : issue === 'proofNotYours' ? t('remote.account.notOwner')
      : issue === 'stationLimit' ? t('remote.account.stationLimit')
      : issue === 'inviteLimit' ? t('remote.account.inviteLimit')
      : issue === 'noKey' ? t('remote.vaultFailed')
      : t('remote.requestFailed')}</p>}
    {/* S3-M1: browsers check this station's stream against the key the service recorded for it at
        pairing. Another one there is not fixed by waiting, so it gets its own line until it is. */}
    {status?.keyRefused && <p role="alert">{t('remote.nativeStationKeyPinned')}</p>}
    {/* The notice: a device the station granted on its own proof, until the operator has seen it. Inline,
        never a modal: nothing here may stand between the operator and Stop TX. */}
    {account?.added?.map(added => { const what = added.kind === 'browser' ? t('remote.account.aBrowser')
      : t('remote.native.computerPlatform', { platform: added.platform ? PLATFORM_NAMES[added.platform] : '—' })
    const key = added.key ? shortFingerprint(added.key) : '—'
    return <div key={added.deviceId} role="status" className="remote-native-browser">
      <span>{added.guest ? t('remote.account.addedGuest', { name: added.name, what, key, role: roleName(added.role) })
        : t('remote.account.added', { name: added.name, what, key })}</span>
      <span className="remote-actions">
        <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'device', deviceId: added.deviceId, approve: false })}>{t('remote.native.remove')}</button>
        <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'dismissAdded', deviceId: added.deviceId })}>{t('remote.account.gotIt')}</button>
      </span>
    </div> })}
    {account?.reproof && <p role="alert">{t('remote.account.reproof')}</p>}
    {/* Signing in again, for a station bound already: the same sign-in, the station otherwise as it was. */}
    {phase !== 'unpaired' && (account?.signingIn ? <>
      <p role="status">{t('remote.account.signingIn')}</p>
      <button type="button" className="remote-button" onClick={() => void act({ type: 'cancelSignInAgain' })}>{t('remote.account.cancelSignIn')}</button>
    </> : account?.reproof && <div className="remote-actions">
      <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'signInAgain' })}>{t('remote.account.signInAgain')}</button>
    </div>)}
    {/* Linking the station to the account IS the sign-in, so its steps stay in the card while it is
        not linked. */}
    {phase === 'unpaired' && <>
      <label>{t('remote.stationName')}<input value={name} maxLength={48} onChange={event => setName(event.target.value)} /></label>
      {/* The buttons below need a name; prefill usually supplies one, but when the station has no
          callsign yet the field is empty, so say what to do rather than leave a dead button. */}
      {!name.trim() && <p className="settings-hint">{t('remote.stationNameHint')}</p>}
      {/* Signing in binds the station with no code; pairing with a code stays beside it. */}
      {account?.signingIn ? <>
        <p role="status">{t('remote.account.signingIn')}</p>
        <button type="button" className="remote-button" onClick={() => void act({ type: 'cancel' })}>{t('remote.account.cancelSignIn')}</button>
      </> : <>
        <p>{t('remote.account.signInHint')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'signIn', name })}>{t('remote.account.signIn')}</button>
          <button type="button" className="remote-button" disabled={busy || !name.trim()} onClick={() => void act({ type: 'begin', name })}>{t('remote.pairStation')}</button>
        </div>
      </>}
    </>}
    {status?.pairingCode && <>
      <p>{t('remote.openService')} <a href={status.origin} target="_blank" rel="noreferrer">{status.origin}</a></p>
      <p>{t('remote.pairingCode')} <code>{status.pairingCode.match(/.{1,4}/g)?.join(' ')}</code></p>
      <p>{t('remote.codeExpires')}</p>
      {status.accountId && <p>{t('remote.accountMatch')} <code>{status.accountId}</code></p>}
      <div className="remote-actions">
        {phase === 'approval' && status.accountId && status.pairingId && <button type="button" className="remote-button" disabled={busy}
          onClick={() => void act({ type: 'approve', accountId: status.accountId!, enrollmentId: status.pairingId! })}>{t('remote.approvePairing')}</button>}
        <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'cancel' })}>{t('remote.cancelPairing')}</button>
      </div>
    </>}
    {status?.stationId && <>
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={!connected && busy} onClick={() => void act({ type: connected ? 'disable' : 'enable' })}>
          {connected ? t('remote.disable') : t('remote.enable')}</button>
      </div>
      {offer && <div role="group" aria-label={t('settings.launchAtLogin.legend')}>
        <p>{t('remote.autostartOffer')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" onClick={() => void answerOffer(true)}>{t('remote.autostartOfferAccept')}</button>
          <button type="button" className="remote-button" onClick={() => void answerOffer(false)}>{t('remote.autostartOfferDismiss')}</button>
        </div>
      </div>}
      {offerFailed && <p role="alert">{t('remote.autostartOfferFailed')}</p>}
      {/* S3-L1: this station's own key, as the Remote page shows the key it kept for the station: the
          operator compares the two, group by group. */}
      {status.stationKey && <p>{t('remote.thisStationKey', { key: shortFingerprint(status.stationKey) })}</p>}
      {/* The browsers, each with its key and the one thing to do next: the same actions as the
          per-browser block under Advanced. */}
      <h3>{t('remote.native.browsers')}</h3>
      {browsers.length === 0 ? <p>{t('remote.noBrowsers')}</p> : <ul className="remote-native-browsers">
        {browsers.map(device => { const key = device.key ?? undefined
          const unpinned = device.approved === 1 && !!key && !status.pinnedDevices?.includes(device.id)
          return <li key={device.id} className="remote-native-browser">
            <span className="remote-native-browser-name">{device.name}</span>
            {key && <span className="remote-native-browser-key">{t('remote.browserKey', { key: shortFingerprint(key) })}</span>}
            <span className="remote-native-browser-state">{device.approved !== 1 || waitsForApprove(status, device.id) ? t('remote.native.asks')
              : unpinned ? t('remote.browserKeyNotPinned') : device.untilRemoved === 1 ? t('remote.browserApprovedUntilRemoved')
              : t('remote.browserApprovedUntil', { until: utcDate(device.expiresAt) })}</span>
            {accountNotes(device)}
            <span className="remote-actions">
              {(device.approved !== 1 || unpinned) && <button type="button" className="remote-button" disabled={busy}
                onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, key, kind: device.kind ?? 'browser' })}>{t('remote.approve')}</button>}
              {device.approved === 1 && <button type="button" className="remote-button" disabled={busy}
                onClick={() => void act({ type: 'device', deviceId: device.id, approve: false })}>{t('remote.native.remove')}</button>}
            </span>
          </li> })}
      </ul>}
      {/* The computers running Nexus, each by its name, its platform and its key in eight groups. Approve
          is the pop-up's own: the key shown pinned. */}
      {computers.length > 0 && <>
        <h3>{t('remote.native.computers')}</h3>
        <ul className="remote-native-browsers">
          {computers.map(device => { const key = device.key ?? undefined
            const unpinned = device.approved === 1 && !!key && !status.pinnedDevices?.includes(device.id)
            return <li key={device.id} className="remote-native-browser">
              <span className="remote-native-browser-name">{device.name}</span>
              {device.platform && <span>{t('remote.native.computerPlatform', { platform: PLATFORM_NAMES[device.platform] })}</span>}
              {key && <span className="remote-native-browser-key">{t('remote.browserKey', { key: shortFingerprint(key) })}</span>}
              <span className="remote-native-browser-state">{device.approved !== 1 || waitsForApprove(status, device.id) ? t('remote.native.computerAsks')
                : unpinned ? t('remote.native.computerKeyNotPinned') : device.untilRemoved === 1 ? t('remote.browserApprovedUntilRemoved')
                : t('remote.browserApprovedUntil', { until: utcDate(device.expiresAt) })}</span>
              {accountNotes(device)}
              <span className="remote-actions">
                {(device.approved !== 1 || unpinned) && <button type="button" className="remote-button" disabled={busy}
                  onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, key, kind: device.kind ?? 'browser' })}>{t('remote.approve')}</button>}
                {device.approved === 1 && <button type="button" className="remote-button" disabled={busy}
                  onClick={() => void act({ type: 'device', deviceId: device.id, approve: false })}>{t('remote.native.remove')}</button>}
              </span>
            </li> })}
        </ul>
      </>}
      {/* People: invited by the address they sign in with, to operate or only to listen. Their devices are
          added when they sign in; the station checks each one's proof against this list itself. */}
      <h3>{t('remote.account.people')}</h3>
      <p>{t('remote.account.peopleHint')}</p>
      {(account?.people?.length ?? 0) === 0 ? <p>{t('remote.account.noPeople')}</p> : <ul className="remote-native-browsers">
        {account!.people.map(person => <li key={person.mailbox} className="remote-native-browser">
          <span className="remote-native-browser-name">{person.mailbox}</span>
          <span>{roleName(person.role)}</span>
          <span className="remote-actions">
            <button type="button" className="remote-button" disabled={busy}
              onClick={() => void act({ type: 'uninvite', email: person.mailbox })}>{t('remote.account.uninvite')}</button>
          </span>
        </li>)}
      </ul>}
      <label>{t('remote.account.email')}<input type="email" value={inviteEmail} maxLength={254} onChange={event => setInviteEmail(event.target.value)} /></label>
      <label>{t('remote.account.role')}<select value={inviteRole} onChange={event => setInviteRole(event.target.value === 'listen' ? 'listen' : 'operate')}>
        <option value="operate">{t('remote.account.role.operate')}</option>
        <option value="listen">{t('remote.account.role.listen')}</option>
      </select></label>
      {/* Someone invited to operate holds what the owner's own other computer holds: said where Operate is offered,
          so the owner invites to operate only someone they would hand the shack to. */}
      {inviteRole === 'operate' && <p className="remote-warning">{t('remote.account.operateIsYours')}</p>}
      <div className="remote-actions">
        <button type="button" className="remote-button" disabled={busy || !inviteEmail.trim()}
          onClick={() => void act({ type: 'invite', email: inviteEmail.trim(), role: inviteRole })}>{t('remote.account.invite')}</button>
      </div>
    </>}
    {/* Advanced: the old panel's options, kept as they were (the operator, 2026-10-02: "The old
        pairing and permission options fold under "Advanced" (kept, not deleted)"). */}
    <details className="remote-native-advanced"><summary>{t('remote.native.advanced')}</summary>
      <p>{t('remote.nativeIntro')}</p>
      {status?.stationId && <>
        <p>{t('remote.restartHint')}</p>
        <div className="remote-actions">
          <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'refresh' })}>{t('remote.refreshDevices')}</button>
        </div>
        {status.stationPermissions && <><p>{t('remote.controlLocalHint')}</p><button type="button" className="remote-button" onClick={()=>void act({type:'takeOverLogging'})}>{t('remote.controlTakeOver')}</button></>}
        <h3>{t('remote.browserApprovals')}</h3><p>{t('remote.browserMatch')}</p>
        {browsers.map(device => { const key = device.key ?? undefined
          // A5: approved, with a key the service lists, and that key not the one pinned here (approved
          // before keys, or the browser came back with a new one): it cannot stream until approved again.
          const unpinned = device.approved === 1 && !!key && !status.pinnedDevices?.includes(device.id)
          const ending = device.approved === 1 && device.untilRemoved !== 1 && (device.renewsUntil ?? device.expiresAt) - Date.now() <= APPROVAL_WARNING_MS
          return <div key={device.id}>
          {/* The browser's key beside its name: the page shows the same one, for the operator to compare. */}
          <p>{device.name} <code>{device.id.slice(-6)}</code>{key && <> <span>{t('remote.browserKey', { key: shortFingerprint(key) })}</span></>}</p>
          {/* One approval: approving grants station controls. The switch below then only
              restricts (or gives back) what an approved browser holds. */}
          {device.approved === 1
            ? <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'device', deviceId: device.id, approve: false })}>{t('remote.revokeBrowser')}</button>
            : <button type="button" className="remote-button" disabled={busy}
                onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, key, kind: device.kind ?? 'browser' })}>{t('remote.approveBrowser')}</button>}
          {device.approved === 1 && <p>{device.untilRemoved === 1 ? t('remote.browserApprovedUntilRemoved') : device.renewsUntil
            ? t('remote.browserRenewsUntil', { until: utcDate(device.expiresAt), limit: utcDate(device.renewsUntil) })
            : t('remote.browserApprovedUntil', { until: utcDate(device.expiresAt) })}</p>}
          {/* The last week before the end that use cannot move, or a key not pinned here. Approving again
              is a NEW approval: it grants station controls again, and pins the key shown
              above. */}
          {(ending || unpinned) && <>
            {ending && <p className="remote-warning">{t('remote.browserApprovalEnding', { until: utcDate(device.renewsUntil ?? device.expiresAt) })}</p>}
            {unpinned && <p className="remote-warning">{t('remote.browserKeyNotPinned')}</p>}
            <button type="button" className="remote-button" disabled={busy}
              onClick={() => void act({ type: 'device', deviceId: device.id, approve: true, key, kind: device.kind ?? 'browser' })}>{t('remote.approveAgain')}</button>
          </>}
          {device.approved===1&&status.stationPermissions&&<button type="button" className="remote-button" disabled={busy||!connected} onClick={()=>void act({type:'stationPermission',deviceId:device.id,allow:!status.stationPermissions!.includes(device.id)})}>{status.stationPermissions.includes(device.id)?t('remote.controlRevoke'):t('remote.controlAllow')}</button>}
          {status.controller===device.id&&<p role="status">{t('remote.controlActive')}</p>}
        </div> })}
        <details><summary>{t('remote.stationAccess')}</summary><p>{t('remote.revokeHint')}</p>
          <button type="button" className="remote-button" disabled={busy} onClick={() => void act({ type: 'forget' })}>{t('remote.revokeStation')}</button>
        </details>
      </>}
    </details>
  </div>
}
