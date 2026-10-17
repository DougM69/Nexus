// Same-origin browser API, the native client's API, and outbound station admission. No radio command router.
import { account, access, body, bodyBytes, browserOrigin, caller, CHALLENGE_MS, cookie, device, deviceCookie, deviceKeys, digest,
  deviceKinds, fieldsOf, id, keepApprovals, label, lifetime, native, nativeAccount, nativeDevice, nativeKind, proof, publicKey, rate, Refusal, requireAdmin, requireEligible,
  requireTrial, requireUnspentIdentity, requireValue, secret, signedBy, station, trial, TRIAL_MS, UNTIL_REMOVED, untilRemoved, uuid } from './authority'
import type { DeviceRow, RemoteEnv, StationRow } from './authority'
import { observerDeadline } from '../../ui/src/remote-monitor/relay'
import type { Identity } from '../../ui/src/remote-monitor/relay'
import { advertisedOperationVersion } from '../../ui/src/remote-web/operation-version'
export { StationRoom } from './room'

/** How long a station's connection is admitted before it has to connect again. A stream is a lane of
 *  one station connection and ends with it, so this is also the longest a stream can last. */
const STATION_ADMISSION_MS = 86400000

function json(value: unknown, status = 200, headers?: HeadersInit): Response {
  const result = new Response(JSON.stringify(value), { status, headers })
  result.headers.set('content-type', 'application/json')
  result.headers.set('cache-control', 'no-store')
  result.headers.set('x-content-type-options', 'nosniff')
  result.headers.set('referrer-policy', 'no-referrer')
  return result
}
async function room(env: RemoteEnv, stationId: string, path: string, value?: unknown): Promise<Response> {
  // Construct fresh internal headers. Public requests cannot supply an admission.
  const upgrade = path === 'station' || path === 'browser' || path === 'client-pipe' || path === 'station-pipe'
  return env.STATIONS.get(env.STATIONS.idFromName(stationId)).fetch(new Request(`https://station.internal/${path}`, {
    method: upgrade ? 'GET' : 'POST',
    headers: upgrade ? { upgrade: 'websocket', 'x-nexus-admission': JSON.stringify(value) } : { 'content-type': 'application/json' },
    body: upgrade ? undefined : JSON.stringify(value ?? {}),
  }))
}
async function sync(env: RemoteEnv, row: StationRow, now: number): Promise<void> {
  const response = await room(env, row.id, 'policy', { access: await access(env, row, now) })
  requireValue(response.ok, 'serviceUnavailable', 503)
}

async function api(request: Request, env: RemoteEnv): Promise<Response> {
  const url = new URL(request.url), now = Date.now()
  requireValue(url.origin === env.PUBLIC_REMOTE_ORIGIN && !url.search, 'originDenied')
  const path = url.pathname.slice('/api/remote/'.length)
  if (request.method === 'GET' && path === 'config') return json({
    issuer: env.AUTH0_ISSUER, audience: env.AUTH0_AUDIENCE, clientId: env.AUTH0_CLIENT_ID,
    // The Native application Nexus signs in to for the internet road, or null while there is none:
    // never the page's own, which no native route takes.
    nativeClientId: nativeClientId(env),
    ready: env.AUTH0_CLIENT_ID !== 'unconfigured',
    revision: env.REMOTE_BUILD_REVISION ?? 'local',
    operationVersion: 2,
    operationMaxVersion: 3,
    operationFtVersion: 1,
    // Operation v5: the station pushes a settled control's outcome. A page that cannot see this
    // number negotiates v4 and polls exactly as before.
    operationPushVersion: 1,
    // A service rolled back past the audio lane reports nothing here, and a browser
    // that cannot see this number never offers listening at all.
    audioVersion: 1,
    // The stream's signalling lane. A page that cannot see this number never offers a stream.
    streamVersion: 1,
  })
  const match = /^stations\/([0-9a-f-]{36})\/(.+)$/.exec(path)
  // The native client's routes (Nexus on another computer), on its own token and its device key's
  // signature, with no Origin and no cookie. No other route takes that caller, and it takes no other.
  if (path.startsWith('client/') || match?.[2].startsWith('client/')) return client(request, env, path, match, now)
  if (request.method === 'GET' && match && ['connect', 'observe', 'pipe'].includes(match[2])) {
    const stationId = id(match[1])
    requireValue(request.headers.get('upgrade')?.toLowerCase() === 'websocket', 'webSocketRequired', 426)
    // The station's end of a native client's pipe (room.ts, pipe.ts): a second socket of its own,
    // opened when its main socket was asked to, for the pipe that message named.
    if (match[2] === 'pipe') {
      const row = await native(request, env, stationId)
      await rate(env, `station-pipe:${stationId}`, now, 60)
      return room(env, stationId, 'station-pipe', { access: await access(env, row, now), pipeId: id(request.headers.get('x-nexus-pipe-id')),
        identity: { stationId, accountId: row.account_id, generation: row.generation, expiresAt: now + STATION_ADMISSION_MS } })
    }
    if (match[2] === 'connect') {
      const row = await native(request, env, stationId)
      await rate(env, `connect:${stationId}`, now, 20)
      // Approvals given under the thirty/ninety-day rule last until removed from here on.
      await keepApprovals(env, stationId, now)
      return room(env, stationId, 'station', { access: await access(env, row, now),
        operationVersion: advertisedOperationVersion(
          ['1','2'].includes(request.headers.get('x-nexus-operation-version')??'') ? Number(request.headers.get('x-nexus-operation-version')) : 0,
          request.headers.get('x-nexus-operation-max-version') === '3' ? 3 : 0,
          request.headers.get('x-nexus-operation-ft-version') === '1' ? 1 : 0,
          request.headers.get('x-nexus-operation-push-version') === '1' ? 1 : 0,
        ),
        // Receive audio. Its own advertisement, independent of every other version:
        // a station that does not send this header never gets handed an `audioListen`,
        // whose unknown fields its message parser would refuse - taking the whole
        // control socket down with it.
        audioVersion: request.headers.get('x-nexus-audio-version') === '1' ? 1 : 0,
        // The stream's signalling lane, on the same terms: a station that does not send this header
        // is never handed a `streamSignal`.
        streamVersion: request.headers.get('x-nexus-stream-version') === '1' ? 1 : 0,
        // The browser's proof on the operation and audio lanes (S1-M1): handed on only to a station that
        // takes it, because any other refuses a field it does not know by closing its socket.
        laneVersion: request.headers.get('x-nexus-lane-signature-version') === '1' ? 1 : 0,
        // The native client's pipe, on the same terms: only a station that sends this header is ever
        // asked to open one (`pipeOpen`), a message any other would close its socket on.
        pipeVersion: request.headers.get('x-nexus-pipe-version') === '1' ? 1 : 0,
        identity: { stationId, accountId: row.account_id, generation: row.generation, expiresAt: now + STATION_ADMISSION_MS } })
    }
    browserOrigin(request, env)
    const protocols = (request.headers.get('sec-websocket-protocol') ?? '').split(',').map(s => s.trim())
    requireValue(protocols.length === 2 && protocols[0] === 'nexus-observe-v1' && /^ticket\.[0-9a-f]{64}$/.test(protocols[1]), 'invalidTicket', 401)
    // DELETE RETURNING makes replay refusal atomic across concurrent Worker instances.
    const ticket = await env.DB.prepare(`DELETE FROM tickets WHERE digest=? AND station_id=? AND expires_at>? RETURNING *`)
      .bind(await digest(protocols[1].slice(7)), stationId, now).first<{
        account_id: string; device_id: string; device_generation: number; identity_until: number
      }>()
    requireValue(ticket, 'invalidTicket', 401)
    const row = await station(env, stationId)
    return room(env, stationId, 'browser', { access: await access(env, row, now), sessionId: uuid(),
      identity: { accountId: ticket.account_id, deviceId: ticket.device_id,
        deviceGeneration: ticket.device_generation, expiresAt: ticket.identity_until },
      entitlement: await trial(env, ticket.account_id, now) })
  }
  requireValue(request.method === 'POST', 'methodNotAllowed', 405)

  if (path === 'enroll') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    // PER-CALLER FIRST. `rate()` increments whenever a bucket is under its OWN limit, so checking
    // the shared bucket first let one refused caller spend a global slot on every rejected request:
    // ~1000 requests from a single host exhausted enroll-global and no operator anywhere could
    // pair a station until the fixed ten-minute window rolled. A caller already over its own
    // limit must never be able to touch the shared budget.
    //
    // WHY THE SHARED BUDGET CANNOT BE STARVED FROM ONE NETWORK. A caller is an IPv4 address or an
    // IPv6 /64 (see caller()), and every IPv6 caller is also capped per /48, in that order, before
    // anything reaches enroll-global. So one /64 can spend at most 5 shared slots and one /48 at
    // most 20, out of 1000: draining the pool takes 50 separate /48s or 200 separate IPv4
    // addresses, which is a distributed attacker and a job for an edge rule, not for this counter.
    // Keyed on the full address instead, one /64 spent the whole pool (security review H1).
    // The /48 cap still sits AFTER the /64 one, so a single flooding host cannot use up its
    // neighbours' share either. It also bounds rate_limits growth: at most one row per /64 the
    // caller holds, where a full-address key minted one per address.
    const { caller: from, network } = caller(request.headers.get('cf-connecting-ip'))
    await rate(env, `enroll:${from}`, now, 5, 600000)
    if (network) await rate(env, `enroll-network:${network}`, now, 20, 600000)
    // The sweep runs BEFORE the shared gate, and it is the service's only garbage collection:
    // enrollments, tickets and rate_limits are reaped nowhere else and there is no cron trigger.
    // Behind the shared gate, exhausting that bucket also stopped all cleanup - the denial and
    // the unbounded growth were the same bug, and the growth outlived the window.
    await env.DB.batch([
      env.DB.prepare('DELETE FROM enrollments WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM rate_limits WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
    ])
    await rate(env, 'enroll-global', now, 1000, 600000)
    const input = await body(request, ['name']), stationId = uuid(), credential = secret(), code = secret().slice(0, 16)
    await env.DB.prepare('INSERT INTO enrollments(id,name,proof_hash,code_hash,expires_at) VALUES(?,?,?,?,?)')
      .bind(stationId, label(input.name), await digest(credential), await digest(code), now + 600000).run()
    return json({ id: stationId, proof: credential, code, expiresAt: now + 600000 })
  }
  if (path === 'enroll/check' || path === 'enroll/approve') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    const input = await body(request, path.endsWith('approve') ? ['id', 'proof', 'credential'] : ['id', 'proof'])
    const stationId = id(input.id), hash = await digest(proof(input.proof))
    // Bounded by CALLER before it is bounded by the id the caller chose. The per-station bucket
    // is keyed on attacker-supplied input, and rate() writes one rate_limits row per distinct
    // key, so without this a stranger could mint unbounded rows by sending fresh random UUIDs -
    // each refused 410, each leaving a row behind. The caller is an IPv4 address or an IPv6 /64,
    // with a /48 cap behind it, for the reason given at `enroll`: keyed on the full address, one
    // /64 rotating its addresses minted those rows without limit again.
    const { caller: from, network } = caller(request.headers.get('cf-connecting-ip'))
    await rate(env, `enrollcheck:${from}`, now, 60, 600000)
    if (network) await rate(env, `enrollcheck-network:${network}`, now, 240, 600000)
    await rate(env, `enrollment:${stationId}`, now, 30)
    const pending = await env.DB.prepare('SELECT account_id,approved,confirmed FROM enrollments WHERE id=? AND proof_hash=? AND expires_at>?')
      .bind(stationId, hash, now).first<{ account_id: string | null; approved: number; confirmed: number }>()
    requireValue(pending, 'pairingExpired', 410)
    // Exactly these two fields, and NEVER a third. The shack parses this with deny_unknown_fields
    // (src-tauri remote_service/mod.rs, `struct Check`), so an added field fails every check on every
    // Nexus build that predates it - pairing silently stops working. `confirmed` was added here for a
    // day and did precisely that. The browser learns it from session.pending; the shack learns it as
    // an `awaitingConfirmation` refusal from approve, which it can already parse.
    if (path.endsWith('check')) return json({ accountId: pending.account_id, approved: pending.approved === 1 })
    requireValue(pending.account_id, 'accountNotClaimed', 409)
    // THE GATE. Typing a code is not agreement to attach a station: a code the operator RECEIVED
    // from somebody else reaches exactly this point, and approval happens at the sender's own
    // shack, so the two-act protection cannot see it. Everything permanent is below this line -
    // the station row, its credential, and the trial clock that can never return to 'none'.
    requireValue(pending.confirmed === 1, 'awaitingConfirmation', 409)
    requireEligible(await trial(env, pending.account_id, now))
    await requireUnspentIdentity(env, pending.account_id, now)
    const credentialHash = await digest(proof(input.credential))
    // Approval at the shack is the moment the trial starts, so the clock is written in the same
    // batch as the station. Station insert first on purpose: a trial with no station is
    // unrecoverable by the operator, whereas a station with no trial is repaired by the native
    // client's existing retry, which reuses the same credential. ON CONFLICT DO NOTHING against
    // the account_id primary key IS the concurrency argument, and it is also "one trial, ever"
    // written in SQL - two approvals racing cannot produce two clocks.
    await env.DB.batch([
      env.DB.prepare(`INSERT OR IGNORE INTO stations(id,account_id,name,credential_hash)
        SELECT id,account_id,name,? FROM enrollments e WHERE id=? AND proof_hash=? AND expires_at>?
        AND account_id IS NOT NULL AND approved=0 AND (SELECT COUNT(*) FROM stations s WHERE s.account_id=e.account_id AND s.enabled=1)<2`)
        .bind(credentialHash, stationId, hash, now),
      env.DB.prepare(`UPDATE enrollments SET approved=1 WHERE id=? AND proof_hash=?
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)`)
        .bind(stationId, hash, stationId, credentialHash),
      // The browser that confirmed this pairing is approved with it (one approval at the shack).
      // Bound to the station row carrying THIS credential, like the trial below, and skipped when a
      // device already holds the digest, so a retried approve adds nothing. The name is a label the
      // shack shows beside the browser code; the browser never asked to be named.
      // It lasts until removed (authority.ts, `UNTIL_REMOVED`). The approval time is recorded only for a
      // Nexus that binds its grants to the generation (authority.ts, `lifetime`). The device key (A5) is
      // the one the confirming browser brought, and none from a page that sent none.
      env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,approved,approved_at,expires_at,public_key)
        SELECT ?,e.id,e.account_id,'Paired browser',e.device_hash,1,?,?,e.public_key FROM enrollments e
        WHERE e.id=? AND e.proof_hash=? AND e.device_hash IS NOT NULL
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.station_id=e.id AND d.credential_hash=e.device_hash)`)
        .bind(uuid(), lifetime(request) ? now : null, UNTIL_REMOVED, stationId, hash, stationId, credentialHash),
      // ONE TRIAL PER MAILBOX, DECIDED IN SQL. `ON CONFLICT(account_id)` is the concurrency
      // argument for one ACCOUNT racing itself, and it always was; it is not one for one PERSON,
      // because two sign-ins by the same human are two account rows and conflict with nothing.
      // requireUnspentIdentity above is a check-then-write: two approvals in flight together both
      // read "no sibling trial" before either wrote one, and both got a fourteen-day clock. The
      // NOT EXISTS below closes that, and it closes it in the same statement as the write, so
      // SQLite's single-writer serialization is what makes it atomic rather than a claim about
      // where the await points fall. It is deliberately the same sibling test requireUnspentIdentity
      // makes - mailbox hash, or an address hash recorded before that column existed - so the two
      // cannot drift into disagreeing about who is the same person. An account with no verified
      // address matches no sibling and is unaffected, which is the documented existing behaviour.
      env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
        SELECT ?,1,?,?,'trial' WHERE EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        AND NOT EXISTS(SELECT 1 FROM trials t JOIN accounts a ON a.id=t.account_id, accounts self
          WHERE self.id=? AND t.account_id<>self.id
          AND (a.mailbox_hash=self.mailbox_hash OR a.email_hash IN (self.email_hash, self.mailbox_hash)))
        ON CONFLICT(account_id) DO NOTHING`)
        .bind(pending.account_id, now + TRIAL_MS, now, stationId, credentialHash, pending.account_id),
    ])
    // Read both rows back before answering 200. That makes "the station and the clock landed
    // together" an observable property of the response rather than a claim about D1's rollback
    // semantics, which are unproven here: every test runs against local Miniflare D1.
    const accepted = await env.DB.prepare('SELECT id FROM stations WHERE id=? AND credential_hash=? AND enabled=1')
      .bind(stationId, credentialHash).first()
    requireValue(accepted, 'stationLimit', 409)
    const started = await trial(env, pending.account_id, now)
    // Losing the race above is not a service failure. The NOT EXISTS guard is what makes one
    // trial per person true under concurrency, and the loser deserves the same named refusal a
    // sequential caller would have been given - trialActiveElsewhere - rather than a bare 503
    // that reads as "try again", which is exactly what it must not do.
    if (started.state !== 'active') await requireUnspentIdentity(env, pending.account_id, now)
    requireValue(started.state === 'active', 'serviceUnavailable', 503)
    // The approved pairing browser, so the shack can give it station control and logging without
    // waiting for a browser list. Additive: the shack reads stationId and accountId by name and has
    // always ignored anything else here. `null` when the confirm came from a browser of an older era.
    // `generation` is additive here too: the shack reads `device.id` and `device.expiresAt` by name.
    // So is `publicKey` (A5), the key a Nexus that pins keys pins for this browser with the pairing.
    const paired = await env.DB.prepare(`SELECT d.id, d.expires_at AS expiresAt, d.generation, d.public_key AS publicKey FROM devices d
      JOIN enrollments e ON d.station_id=e.id AND d.credential_hash=e.device_hash
      WHERE e.id=? AND e.proof_hash=? AND d.approved=1 AND d.expires_at>?`)
      .bind(stationId, hash, now).first<{ id: string; expiresAt: number; generation: number; publicKey: string | null }>()
    return json({ stationId, accountId: pending.account_id, entitlement: started, device: paired ?? null })
  }

  if (match?.[2].startsWith('native/')) {
    const verb = match[2].slice(7)
    const row = await native(request, env, match[1], verb === 'revoke')
    await rate(env, `native:${row.id}`, now, 60)
    if (verb === 'devices') {
      await body(request, [])
      // The generation (and `renewsUntil`, null now that no approval renews: each lasts until removed) go
      // ONLY to a Nexus that asks. 1.12.0 parses this list with deny_unknown_fields, so a field added for
      // everyone would fail every refresh it makes. The device key (A5) likewise, on its own header.
      // The native client's devices go only to a Nexus that asks for kinds, and with their kind and
      // platform, for its approval pop-up; a browser is listed there as kind 'browser' on no platform.
      // That Nexus is also told, by name, which approvals last until removed (`untilRemoved`, 1 or 0).
      await keepApprovals(env, row.id, now)
      const kinds = deviceKinds(request)
      const key = (deviceKeys(request) ? ',public_key AS publicKey' : '') +
        (kinds ? ",COALESCE(kind,'browser') AS kind,platform,CASE WHEN approved=1 AND expires_at>=? THEN 1 ELSE 0 END AS untilRemoved" : '')
      const forever = kinds ? [UNTIL_REMOVED] : []
      const only = kinds ? '' : ' AND kind IS NULL'
      const devices = lifetime(request)
        ? await env.DB.prepare(`SELECT id,name,approved,expires_at AS expiresAt,generation,NULL AS renewsUntil${key}
            FROM devices WHERE station_id=? AND expires_at>?${only} ORDER BY id LIMIT 8`).bind(...forever, row.id, now).all()
        : await env.DB.prepare(`SELECT id,name,approved,expires_at AS expiresAt${key} FROM devices WHERE station_id=? AND expires_at>?${only} ORDER BY id LIMIT 8`)
          .bind(...forever, row.id, now).all()
      return json({ devices: devices.results })
    }
    if (verb === 'approve-device' || verb === 'revoke-device') {
      const input = await body(request, ['deviceId']), deviceId = id(input.deviceId)
      const found = await env.DB.prepare('SELECT id FROM devices WHERE id=? AND station_id=? AND expires_at>?')
        .bind(deviceId, row.id, now).first()
      requireValue(found, 'deviceUnavailable', 404)
      const approve = verb === 'approve-device'
      // Every approval, including approving again, is a new generation, and lasts until removed. Revoking
      // (Remove) ends it now and clears the time.
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=?,generation=generation+1,expires_at=?,approved_at=? WHERE id=? AND station_id=?')
          .bind(approve ? 1 : 0, approve ? UNTIL_REMOVED : now, approve && lifetime(request) ? now : null, deviceId, row.id),
        env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
      ])
      await sync(env, await station(env, row.id), now)
      return json({ ok: true })
    }
    if (verb === 'revoke') {
      await body(request, [])
      await revokeStation(env, row.id, now)
      return json({ ok: true })
    }
    if (verb === 'key') {
      // The station's own signing key (security review S3-M1), recorded once in its pairing record.
      // The first key a station sends is kept, the same one again is answered as kept, and any other is
      // refused by name: a key changes only by pairing again, which makes a new station. Its pages
      // check the station's stream answers against this key, read from here, never through the relay.
      const input = await body(request, ['publicKey']), key = await publicKey(input.publicKey)
      await env.DB.prepare('UPDATE stations SET public_key=? WHERE id=? AND public_key IS NULL').bind(key, row.id).run()
      const kept = await env.DB.prepare('SELECT public_key FROM stations WHERE id=?').bind(row.id).first<{ public_key: string | null }>()
      requireValue(kept?.public_key === key, 'stationKeyPinned', 409)
      return json({ ok: true })
    }
    throw new Refusal('notFound', 404)
  }

  // Every remaining operation is account-authenticated and subject to exact Origin.
  // Destructured off deliberately: `identity` is spread into the browser identity below and on
  // into relay socket state, and the provider subject has no business travelling with it.
  // `verified` is destructured off for the same reason `subject` is: `identity` is spread into
  // the browser identity and on into relay socket state, and how this account was identified is
  // not something the wire needs to carry.
  const { subject, verified, ...identity } = await account(request, env, now)
  await rate(env, `account:${identity.accountId}`, now, 120)
  const entitlement = await trial(env, identity.accountId, now)
  // The only privileged write in the service. It exists because every trial in a closed beta is
  // granted by hand, and the alternative was a wrangler command the operator can only run from a
  // laptop. Deliberately ONE action: each further admin verb is another way for a mistake here to
  // become an entitlement bypass.
  if (path === 'admin/grant-trial') {
    requireAdmin(env, subject)
    const input = await body(request, ['accountId', 'days'])
    const target = id(input.accountId)
    requireValue(Number.isInteger(input.days) && (input.days as number) >= 1 && (input.days as number) <= 365,
      'invalidRequest', 400)
    const expiresAt = now + (input.days as number) * 86400000
    // Selecting FROM accounts means an id that does not exist grants nothing rather than leaving
    // an orphan row. UPDATE on conflict, never DELETE: the trials row is the durable proof an
    // account consumed its trial, and removing it re-opens the reinstall and re-pair abuse the
    // one-trial-ever rule exists to refuse.
    //
    // The first grant's `started_at` is KEPT, and a self-serve trial that is later extended by
    // hand records BOTH as 'trial+manual'. Overwriting them destroyed exactly the distinction
    // 0002_trial.sql and grant-trial.mjs both exist to preserve - that a hand grant and a
    // self-serve trial stay tellable apart forever. After an extension the honest answer to "did
    // this account ever earn a trial on its own?" is yes, and the row should still say so.
    await env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
      SELECT id, 1, ?, ?, 'manual' FROM accounts WHERE id=?
      ON CONFLICT(account_id) DO UPDATE SET enabled=1, expires_at=excluded.expires_at,
        started_at=COALESCE(trials.started_at, excluded.started_at),
        source=CASE WHEN trials.source='trial' THEN 'trial+manual' ELSE 'manual' END`)
      .bind(expiresAt, now, target).run()
    const granted = await trial(env, target, now)
    requireValue(granted.state === 'active', 'stationUnavailable', 404)
    return json({ accountId: target, entitlement: granted })
  }
  if (path === 'session') {
    await body(request, [])
    const rows = await env.DB.prepare('SELECT id,name,enabled,public_key FROM stations WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2')
      .bind(identity.accountId).all<{ id: string; name: string; enabled: number; public_key: string | null }>()
    // The browser's own fields as they have always been (`renewsUntil` now always null: no approval
    // renews), and `untilRemoved` for an approval that lasts until removed. The approval time itself stays
    // in the Worker.
    const stations = await Promise.all(rows.results.map(async row => {
      const current = await device(request, env, row.id, identity.accountId, now)
      // `stationKey` (S3-M1): the station's own signing key, which its stream answers must carry a
      // signature from; null for a station that has not sent one, whose streams the page refuses.
      return { id: row.id, name: row.name, stationKey: row.public_key, device: current && { id: current.id, name: current.name, generation: current.generation,
        approved: current.approved, expires_at: current.expires_at, renewsUntil: null, untilRemoved: untilRemoved(current),
        publicKey: current.public_key } }
    }))
    // A claim is durable on the enrollment row, but nothing put it on the wire, so the browser
    // could only remember "waiting for approval" in component state. A reload lost it, the
    // operator saw the pairing form again, re-typed the code and was refused - the claim UPDATE
    // requires account_id IS NULL - and concluded pairing had failed when it had actually worked.
    // SELECT the columns the browser can show and NOTHING ELSE: code_hash and proof_hash are the
    // credentials this table exists to protect and must never leave the Worker. `name` is the
    // string the operator typed at their own shack and pair/claim already returns it.
    const claimed = await env.DB.prepare(`SELECT id,name,expires_at,confirmed FROM enrollments
      WHERE account_id=? AND approved=0 AND expires_at>? ORDER BY expires_at DESC LIMIT 1`)
      .bind(identity.accountId, now).first<{ id: string; name: string; expires_at: number; confirmed: number }>()
    const pending = claimed ? { id: claimed.id, name: claimed.name, expiresAt: claimed.expires_at, confirmed: claimed.confirmed === 1 } : null
    // `identityVerified` is the signal that the one-trial-per-person check is actually LIVE. It
    // is false whenever the token carried no verified address - which is exactly the state in which
    // that check silently protects nothing. Without it there is no way to tell the two apart from
    // outside, and a provider that never sends the claim looks identical to one that does.
    return json({ accountId: identity.accountId, entitlement, stations, pending,
      identityVerified: verified, serverNow: now })
  }
  if (path === 'pair/claim') {
    requireEligible(entitlement)
    await requireUnspentIdentity(env, identity.accountId, now)
    await rate(env, `claim:${identity.accountId}`, now, 5, 600000)
    const input = await body(request, ['code'])
    requireValue(typeof input.code === 'string' && /^[0-9a-f]{16}$/.test(input.code), 'invalidPairingCode', 400)
    const row = await env.DB.prepare(`UPDATE enrollments SET account_id=? WHERE code_hash=? AND expires_at>?
      AND account_id IS NULL AND approved=0 RETURNING id,name`)
      .bind(identity.accountId, await digest(input.code), now).first()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId })
  }
  if (path === 'pair/confirm') {
    requireEligible(entitlement)
    await requireUnspentIdentity(env, identity.accountId, now)
    await rate(env, `confirm:${identity.accountId}`, now, 10, 600000)
    const input = await body(request, ['id'], ['publicKey'])
    requireValue(typeof input.id === 'string' && /^[0-9a-f-]{36}$/.test(input.id), 'invalidPairingCode', 400)
    // The confirming browser's device key (A5), which the approval gives the device it creates.
    const key = 'publicKey' in input ? await publicKey(input.publicKey) : null
    // Scoped to this account's own unapproved claim. An id alone is not authority: another account
    // holding the same id confirms nothing, and a claim already approved is past the point this
    // gate protects.
    //
    // ONE APPROVAL (operator decision 2026-09-13): the browser that confirms is the browser that
    // did the pairing, and approving at the shack approves it too. It cannot have a device row yet
    // (a device references a station, which does not exist until approval), so it gets its device
    // credential now, as the same cookie `device` sets, and the enrollment keeps only the digest.
    // Confirming again from another browser moves that approval to the other browser.
    const credential = secret()
    const row = await env.DB.prepare(`UPDATE enrollments SET confirmed=1, device_hash=?, public_key=?
      WHERE id=? AND account_id=? AND approved=0 AND expires_at>? RETURNING id,name`)
      .bind(await digest(credential), key, input.id, identity.accountId, now).first<{ id: string; name: string }>()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2]
  requireValue(row.account_id === identity.accountId && row.enabled === 1, 'stationUnavailable', 404)
  // Above requireTrial, with revoke and forget-device: renaming is housekeeping on your own
  // account, and an operator whose service access has lapsed should still be able to tidy it.
  // `name` is not part of StationAccess, so no policy version moves and no room needs syncing -
  // this changes a label, not an authority.
  if (verb === 'rename') {
    const input = await body(request, ['name'])
    const name = label(input.name)
    await env.DB.prepare('UPDATE stations SET name=? WHERE id=? AND account_id=?')
      .bind(name, row.id, identity.accountId).run()
    return json({ id: row.id, name })
  }
  if (verb === 'revoke') {
    await body(request, [])
    await revokeStation(env, row.id, now)
    return json({ ok: true }, 200, { 'set-cookie': cookie(row.id, '', 0) })
  }
  if (verb === 'forget-device') {
    await body(request, [])
    const current = await device(request, env, row.id, identity.accountId, now)
    if (current) {
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=0,generation=generation+1,expires_at=? WHERE id=?').bind(now, current.id),
        env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
      ])
      await sync(env, await station(env, row.id), now)
    }
    return json({ ok: true }, 200, { 'set-cookie': cookie(row.id, '', 0) })
  }
  requireTrial(entitlement, now)
  if (verb === 'device') {
    const input = await body(request, ['name'], ['publicKey']), name = label(input.name)
    // The browser's device key (A5), when the page sends one.
    const key = 'publicKey' in input ? await publicKey(input.publicKey) : null
    const current = await device(request, env, row.id, identity.accountId, now)
    if (current) {
      // A browser approved before it had a key registers one here, and one that lost its stored key
      // brings a new one. The approval is left exactly as it is: the station's pin, not this row,
      // decides whether a browser can stream, and a key the station has not pinned is approved again
      // at the radio. Only this browser's own credential reaches this row.
      if (key !== null && key !== current.public_key) {
        await env.DB.prepare('UPDATE devices SET public_key=? WHERE id=? AND station_id=?').bind(key, current.id, row.id).run()
      }
      return json({ deviceId: current.id, approved: current.approved === 1 })
    }
    const credential = secret(), deviceId = uuid()
    const inserted = await env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,expires_at,public_key)
      SELECT ?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM devices WHERE station_id=? AND expires_at>?)<8 RETURNING id`)
      .bind(deviceId, row.id, identity.accountId, name, await digest(credential), now + 600000, key, row.id, now).first()
    requireValue(inserted, 'deviceLimit', 409)
    return json({ deviceId, approved: false }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  if (verb === 'ticket' || verb === 'renew' || verb === 'turn') {
    const input = await body(request, verb === 'renew' ? ['sessionId'] : [])
    const current = await device(request, env, row.id, identity.accountId, now)
    requireValue(current?.approved === 1, 'deviceNotApproved')
    const browser = { ...identity, deviceId: current.id, deviceGeneration: current.generation,
      expiresAt: Math.min(identity.expiresAt, current.expires_at) }
    const policy = await access(env, row, now)
    observerDeadline(policy, browser, entitlement, now)
    // The stream's relay, for exactly the browser a ticket is for, past exactly the same checks.
    // It renews no approval: a page asks for a relay as a stream starts, and that is not use. The
    // credential lasts the longest session, and never past the entitlement that a stream needs.
    if (verb === 'turn') {
      await rate(env, `turn:${row.id}:${current.id}`, now, 12)
      return json({ iceServers: await relayServers(env, Math.min(STATION_ADMISSION_MS, entitlement.expiresAt - now)) })
    }
    if (verb === 'renew') {
      const response = await room(env, row.id, 'renew', { access: policy,
        sessionId: id(input.sessionId), identity: browser, entitlement })
      requireValue(response.ok, 'sessionNotApproved')
      // Use, once the live session this browser holds has been confirmed.
      return json({ ok: true, serverNow: now }, 200, await used(request, env, row.id, current, now))
    }
    await rate(env, `ticket:${row.id}:${current.id}`, now, 12)
    const ticket = secret()
    await env.DB.batch([
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO tickets(digest,station_id,account_id,device_id,device_generation,identity_until,expires_at) VALUES(?,?,?,?,?,?,?)')
        .bind(await digest(ticket), row.id, identity.accountId, current.id, current.generation, browser.expiresAt, now + 15000),
    ])
    return json({ ticket, serverNow: now }, 200, await used(request, env, row.id, current, now))
  }
  throw new Refusal('notFound', 404)
}

async function revokeStation(env: RemoteEnv, stationId: string, now: number): Promise<void> {
  await env.DB.batch([
    env.DB.prepare('UPDATE stations SET enabled=0,generation=generation+1,policy_version=policy_version+1 WHERE id=?').bind(stationId),
    env.DB.prepare('DELETE FROM tickets WHERE station_id=?').bind(stationId),
  ])
  await sync(env, await station(env, stationId), now)
}

// THE NATIVE CLIENT. Nexus on another computer (later a phone app) driving its station over the
// internet: signed in through the system browser to the Native application (authority.ts,
// `nativeAccount`), with no Origin and no cookie, and acting as a device of that station, approved at
// the shack as a browser is and on the same terms: until removed, eight devices a station, removed at
// the shack or by itself. Its credential is its key: every request that
// acts as the device is signed by it over a one-use challenge (`signedBy`), and the shack pins that
// same key when it approves the device, for the pipe's own TLS to prove.
//
//   POST client/challenge              a challenge for the next signed request, good for a minute
//   POST client/session                the account's stations, each with its station key
//   POST stations/:id/client/device    this device's key registered (signed by that key), or found again
//   POST stations/:id/client/challenge a challenge for an approved device reopening, with no token
//   POST stations/:id/client/turn      TURN credentials for its internet roads
//   GET  stations/:id/client/pipe      its pipe to the station, through the room (pipe.ts)
//   POST stations/:id/client/renew     the pipe's lease renewed, as a browser renews its session
//   POST stations/:id/client/forget    its own approval ended, as a browser forgets itself
//
// REOPENING. A request with no account token is an approved device opening its station again
// (`reopener`): the device it names, approved at the shack and not removed, is its caller, under its
// station's account, and each of its verbs is signed by its key as any other. So Nexus keeps no
// account credential on the computer, and signs in only to register a device. Registering, the
// account's stations and a challenge without a station all still need the token.
async function client(request: Request, env: RemoteEnv, path: string, match: RegExpExecArray | null, now: number): Promise<Response> {
  const reopening = !request.headers.has('authorization')
  const caller = reopening ? await reopener(request, env, match, now) : await nativeAccount(request, env, now)
  await rate(env, `client:${caller.accountId}`, now, 120)
  const entitlement = await trial(env, caller.accountId, now)
  if (path === 'client/challenge') {
    requireValue(request.method === 'POST', 'methodNotAllowed', 405)
    await body(request, [])
    const challenge = secret(), expiresAt = now + CHALLENGE_MS
    await env.DB.batch([
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO challenges(digest,account_id,expires_at) VALUES(?,?,?)').bind(await digest(challenge), caller.accountId, expiresAt),
    ])
    return json({ challenge, expiresAt })
  }
  if (path === 'client/session') {
    requireValue(request.method === 'POST', 'methodNotAllowed', 405)
    await body(request, [])
    // The station key (S3-M1) over the signed-in API, never through the relay: the key the station's
    // end of the pipe must prove, which the computer keeps at first sight.
    const rows = await env.DB.prepare('SELECT id,name,public_key FROM stations WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2')
      .bind(caller.accountId).all<{ id: string; name: string; public_key: string | null }>()
    return json({ accountId: caller.accountId, entitlement, identityVerified: caller.verified, serverNow: now,
      stations: rows.results.map(row => ({ id: row.id, name: row.name, stationKey: row.public_key })) })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2].slice('client/'.length)
  requireValue(row.account_id === caller.accountId && row.enabled === 1, 'stationUnavailable', 404)
  if (verb === 'challenge') {
    // Only for reopening: a signed-in computer asks `client/challenge`.
    requireValue(reopening && request.method === 'POST', 'notFound', 404)
    await body(request, [])
    const challenge = secret(), expiresAt = now + CHALLENGE_MS
    await env.DB.batch([
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO challenges(digest,account_id,expires_at) VALUES(?,?,?)').bind(await digest(challenge), caller.accountId, expiresAt),
    ])
    return json({ challenge, expiresAt })
  }
  // Every verb but `device` is this device's own: the device the request names, of this account and
  // station, within its approval or its wait for one, and the request signed by its key.
  const signer = async (bytes: Uint8Array): Promise<DeviceRow> => {
    const current = await nativeDevice(request, env, row.id, caller.accountId, now)
    requireValue(current?.public_key, 'deviceNotApproved')
    await signedBy(request, env, caller.accountId, current.public_key, bytes, now)
    return current
  }
  const identity = (current: DeviceRow) => ({ accountId: caller.accountId, deviceId: current.id,
    deviceGeneration: current.generation, expiresAt: Math.min(caller.expiresAt, current.expires_at) })
  if (verb === 'pipe') {
    requireValue(request.method === 'GET' && request.headers.get('upgrade')?.toLowerCase() === 'websocket', 'webSocketRequired', 426)
    requireTrial(entitlement, now)
    const current = await signer(new Uint8Array())
    requireValue(current.approved === 1, 'deviceNotApproved')
    await rate(env, `pipe:${row.id}:${current.id}`, now, 12)
    const response = await room(env, row.id, 'client-pipe', { access: await access(env, row, now), identity: identity(current), entitlement })
    // Opening the station from this computer is use, as a browser's ticket is.
    if (response.status === 101) await keepApprovals(env, row.id, now, current.id)
    return response
  }
  requireValue(request.method === 'POST', 'methodNotAllowed', 405)
  const bytes = await bodyBytes(request)
  // Above the trial, as a browser's own is: an operator whose access lapsed may still tidy up.
  if (verb === 'forget') {
    fieldsOf(bytes, [])
    const current = await signer(bytes)
    await env.DB.batch([
      env.DB.prepare('UPDATE devices SET approved=0,generation=generation+1,expires_at=? WHERE id=?').bind(now, current.id),
      env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
    ])
    await sync(env, await station(env, row.id), now)
    return json({ ok: true })
  }
  requireTrial(entitlement, now)
  if (verb === 'device') {
    // A new device is the account's to add: signed in, never reopening.
    requireValue(!reopening, 'signInRequired', 401)
    const input = fieldsOf(bytes, ['name', 'kind', 'platform', 'publicKey'])
    const name = label(input.name), key = await publicKey(input.publicKey), { kind, platform } = nativeKind(input.kind, input.platform)
    // Signed by the key it registers, so whoever sends it holds that key.
    await signedBy(request, env, caller.accountId, key, bytes, now)
    const current = await env.DB.prepare(`SELECT id,name,generation,approved,approved_at,expires_at,public_key FROM devices
      WHERE station_id=? AND account_id=? AND public_key=? AND kind IS NOT NULL AND expires_at>? ORDER BY expires_at DESC LIMIT 1`)
      .bind(row.id, caller.accountId, key, now).first<DeviceRow>()
    if (current) return json({ deviceId: current.id, approved: current.approved === 1, expiresAt: current.expires_at, renewsUntil: null })
    // Ten minutes to be approved at the shack, as a browser has. The credential digest is of a value
    // nobody holds: this device's credential is its key, and no cookie can ever match its row.
    const deviceId = uuid(), expiresAt = now + 600000
    const inserted = await env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,expires_at,public_key,kind,platform)
      SELECT ?,?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM devices WHERE station_id=? AND expires_at>?)<8 RETURNING id`)
      .bind(deviceId, row.id, caller.accountId, name, await digest(secret()), expiresAt, key, kind, platform, row.id, now).first()
    requireValue(inserted, 'deviceLimit', 409)
    return json({ deviceId, approved: false, expiresAt, renewsUntil: null })
  }
  fieldsOf(bytes, [])
  const current = await signer(bytes)
  requireValue(current.approved === 1, 'deviceNotApproved')
  const policy = await access(env, row, now)
  // Past exactly the checks a browser's ticket is.
  observerDeadline(policy, identity(current), entitlement, now)
  if (verb === 'turn') {
    // The relay a browser's stream has, for this device's two internet roads. Each entry is a username
    // and a password, which is what the client core's TURN client takes.
    await rate(env, `turn:${row.id}:${current.id}`, now, 12)
    return json({ iceServers: await relayServers(env, Math.min(STATION_ADMISSION_MS, entitlement.expiresAt - now)) })
  }
  if (verb === 'renew') {
    const response = await room(env, row.id, 'pipe-renew', { access: policy, identity: identity(current), entitlement })
    requireValue(response.ok, 'pipeClosed', 404)
    await keepApprovals(env, row.id, now, current.id)
    return json({ ok: true, serverNow: now })
  }
  throw new Refusal('notFound', 404)
}

// The caller of a request with no account token: the native device it names (`x-nexus-device-id`), of
// the station the path names, approved there and not removed, acting under that station's account for
// at most an hour, as a token would. What it may do is still each verb's own, signed by the device's
// key. No Origin and no cookie, as for any native caller. Refused alike, and before anything is read,
// for a path with no station or a device that is not one approved there: no token, no reopening.
async function reopener(request: Request, env: RemoteEnv, match: RegExpExecArray | null, now: number): Promise<Identity & { subject: string; verified: boolean }> {
  requireValue(!request.headers.has('origin') && !request.headers.has('cookie'), 'originDenied')
  requireValue(nativeClientId(env), 'serviceNotConfigured', 503)
  requireValue(match, 'signInRequired', 401)
  const deviceId = request.headers.get('x-nexus-device-id') ?? ''
  requireValue(/^[0-9a-f-]{36}$/.test(deviceId), 'signInRequired', 401)
  await rate(env, `reopen:${match[1]}:${deviceId}`, now, 60)
  const row = await station(env, match[1]).catch(() => null)
  requireValue(row, 'signInRequired', 401)
  const current = await nativeDevice(request, env, row.id, row.account_id, now)
  requireValue(current?.public_key && current.approved === 1, 'signInRequired', 401)
  return { accountId: row.account_id, expiresAt: Math.min(current.expires_at, now + 3600000), subject: '', verified: false }
}

// The Native application's client id, or null while the service has none: unset, or the page's own.
function nativeClientId(env: RemoteEnv): string | null {
  const id = env.AUTH0_NATIVE_CLIENT_ID
  return id && id !== 'unconfigured' && id !== env.AUTH0_CLIENT_ID ? id : null
}

// A browser's use of its approval (operator decision 2026-10-10: an approval lasts until removed).
// Reached only from `ticket` and `renew`, after the request has been admitted: the account's token, THIS
// browser's own credential, an approved device, a live trial, and for `renew` the live session. An
// approval given under the thirty/ninety-day rule becomes one until removed (`keepApprovals`), and the
// browser's credential is re-issued, unchanged, for as long as a browser keeps one, so a browser in use
// never loses it. It moves no generation and grants nothing. Returns the response headers.
async function used(request: Request, env: RemoteEnv, stationId: string, current: DeviceRow, now: number): Promise<HeadersInit | undefined> {
  await keepApprovals(env, stationId, now, current.id)
  const credential = deviceCookie(request, stationId)
  return credential ? { 'set-cookie': cookie(stationId, credential) } : undefined
}

// Every response sent over TLS tells the browser to keep to https:// here for a year, so after one
// visit an address typed or linked as http:// never leaves the browser in the clear. No
// includeSubDomains: it would reach only hosts under this one, and there are none. No preload: that
// binds the whole registrable domain. Never over plain HTTP, where a browser ignores it and the
// standard forbids it. Not on a WebSocket's 101 either: the page that opens a socket has already
// been told, and the relay's handshake stays exactly what it was.
function hsts(request: Request, response: Response): Response {
  if (!request.url.startsWith('https:') || response.status === 101) return response
  // A room's response has immutable headers; a copy of it does not.
  const result = new Response(response.body, response)
  result.headers.set('strict-transport-security', 'max-age=31536000')
  return result
}

// THE STREAM'S RELAY. Cloudflare Realtime TURN credentials, minted with the service's own key for
// one stream, lasting `ttlMs`. The page adds them beside its STUN server, and ICE prefers a direct path,
// so the relay carries a stream only where nothing direct works. The key's id and token are Worker
// secrets and go nowhere but the provider; the page is handed the TURN entries alone. Without the key
// this refuses `relayNotConfigured`; a provider that fails, is slow or answers anything else,
// `relayUnavailable`. Either way the page streams direct, as it did before the relay.
const RELAY_API = 'https://rtc.live.cloudflare.com/v1/turn/keys/'
/** The provider's answer within this, or no relay. Below the page's own wait for the route. */
const RELAY_DEADLINE_MS = 3000
const RELAY_URL = /^turns?:[0-9a-z.-]{1,253}:(\d{1,5})(\?transport=(udp|tcp))?$/i
async function relayServers(env: RemoteEnv, ttlMs: number): Promise<{ urls: string[]; username: string; credential: string }[]> {
  const keyId = env.TURN_KEY_ID ?? '', token = env.TURN_KEY_TOKEN ?? ''
  requireValue(/^[0-9A-Za-z_-]{1,128}$/.test(keyId) && token.length > 0, 'relayNotConfigured', 503)
  let answer: unknown
  try {
    const response = await fetch(`${RELAY_API}${keyId}/credentials/generate-ice-servers`, {
      method: 'POST', headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ ttl: Math.ceil(ttlMs / 1000) }), signal: AbortSignal.timeout(RELAY_DEADLINE_MS),
    })
    if (!response.ok) throw new Error('relayUnavailable')
    answer = await response.json()
  } catch { throw new Refusal('relayUnavailable', 503) }
  // The TURN entries and nothing else: the page has its own STUN server. A URL on port 53 is left out:
  // browsers may refuse TURN on the DNS port, and the provider offers the same transports on others.
  const servers = (answer as { iceServers?: unknown } | null)?.iceServers
  const relay = (Array.isArray(servers) ? servers.slice(0, 4) : []).flatMap(server => {
    const { urls, username, credential } = (server ?? {}) as Record<string, unknown>
    const turn = (Array.isArray(urls) ? urls : [urls]).filter((url): url is string => {
      const port = typeof url === 'string' ? RELAY_URL.exec(url)?.[1] : undefined
      return port !== undefined && Number(port) !== 53
    }).slice(0, 8)
    return turn.length > 0 && typeof username === 'string' && typeof credential === 'string' && username.length > 0
      && credential.length > 0 && username.length <= 1024 && credential.length <= 1024 ? [{ urls: turn, username, credential }] : []
  })
  requireValue(relay.length > 0, 'relayUnavailable', 503)
  return relay
}

export default {
  async fetch(request: Request, env: RemoteEnv): Promise<Response> {
    try {
      const url = new URL(request.url)
      // Plain HTTP reaches no route, no database and no sign-in. A read is sent to the same path and
      // query on the service's https:// origin. Anything else is refused as the API refuses a wrong
      // origin: its body and credentials have already crossed in the clear, and a 308 would resend
      // them as if all were well. A local http:// service (wrangler dev, the tests) has nowhere to send it.
      if (url.protocol === 'http:' && env.PUBLIC_REMOTE_ORIGIN.startsWith('https:')) {
        requireValue(request.method === 'GET' || request.method === 'HEAD', 'originDenied')
        return Response.redirect(`${env.PUBLIC_REMOTE_ORIGIN}${url.pathname}${url.search}`, 301)
      }
      if (url.pathname.startsWith('/api/')) return hsts(request, await api(request, env))
      // HTML normalization is disabled so /index.html cannot redirect around
      // these response headers. Resolve the home page explicitly, retaining
      // the browser's original callback URL for the SDK's PKCE completion.
      if (url.pathname === '/') url.pathname = '/index.html'
      url.search = ''
      const asset = await env.ASSETS.fetch(new Request(url, request))
      const response = new Response(asset.body, asset)
      const issuer = new URL(env.AUTH0_ISSUER).origin
      const socketOrigin = env.PUBLIC_REMOTE_ORIGIN.replace(/^http/, 'ws')
      response.headers.set('content-security-policy', [
        "default-src 'none'", "script-src 'self'", "style-src 'self' 'unsafe-inline'",
        // Received SSTV files and the local composer use validated Blob images.
        // This permission is image-only; scripts and objects retain their policy.
        "img-src 'self' data: blob:", `connect-src 'self' ${socketOrigin} ${issuer}`,
        `frame-src ${issuer}`, "worker-src 'self'", "form-action 'self'",
        // The web app manifest (the page's own, so a phone can put it on its home screen): `default-src 'none'`
        // refuses a manifest.
        "manifest-src 'self'",
        "frame-ancestors 'none'", "base-uri 'none'", "object-src 'none'",
      ].join('; '))
      response.headers.set('referrer-policy', 'no-referrer')
      response.headers.set('x-content-type-options', 'nosniff')
      // The stream's microphone over is this page's own: its origin may ask, and no frame or other
      // site can. The browser still asks the operator. The camera and location stay off.
      response.headers.set('permissions-policy', 'camera=(), microphone=(self), geolocation=()')
      response.headers.set('cache-control', 'no-store')
      return hsts(request, response)
    } catch (error) {
      // Never serialize exception details, provider responses, URLs or headers.
      return hsts(request, json({ error: error instanceof Refusal ? error.code : 'serviceUnavailable' }, error instanceof Refusal ? error.status : 503))
    }
  },
}
