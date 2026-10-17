//! Station-owned authority for the lease, Stop and the FT gestures.
//! Cloud admission routes an approved browser; only a local grant (given at the
//! shack, or restored for a still-approved browser) permits a lease. FT8/FT4 CQ
//! and TX On/Off require a separate local transmit grant, and no grant arms TX;
//! Stop has independent admission. A held FT contact's log append already begun
//! cannot be rolled back on disconnect. Its bounded receipt remains queryable by
//! the same device while locally permitted.
use super::transport::identifier;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tempo_app::remote_control::{transmit::TransmitAuthority, Completion, Outcome, Revocation};

mod logging;
mod station;
mod transmit_stop;

/// A fresh random id in the UUID shape: the station's boot, a lease, a command window.
fn snapshot_id() -> Result<String, &'static str> {
    let s = super::transport::random_secret()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &s[..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    ))
}

const LEASE: Duration = Duration::from_secs(5);
const WINDOW: Duration = Duration::from_secs(2);
const RESULT_AGE: Duration = Duration::from_secs(600);
const MAX_COUNTER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    State {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    Acquire {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
    },
    Heartbeat {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
    Release {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
    StopTransmit {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
    },
    Result {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    StationControl {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        #[serde(rename = "commandWindowId")]
        command_window_id: String,
        #[serde(rename = "clientSequence")]
        client_sequence: u64,
        context: station::Context,
        action: Box<station::Action>,
    },
}
impl Request {
    pub fn id(&self) -> &str {
        match self {
            Self::State { request_id }
            | Self::Acquire { request_id, .. }
            | Self::Heartbeat { request_id, .. }
            | Self::Release { request_id, .. }
            | Self::StopTransmit { request_id, .. }
            | Self::Result { request_id, .. }
            | Self::StationControl { request_id, .. } => request_id,
        }
    }
}
struct Window {
    id: String,
    at: Instant,
}
struct Lease {
    id: String,
    session: String,
    device: String,
    until: Instant,
    sequence: u64,
    /// The LAN connection that took it, or `None` for the relay's. The lease is bound to that
    /// connection: it ends with it, and nothing on the other road ends it (the operator's ruling of
    /// 2026-10-04, "both at once").
    lan: Option<u64>,
}
struct Receipt {
    id: String,
    session: String,
    device: String,
    fingerprint: Vec<u8>,
    at: Instant,
    /// `None` while the write is still in flight (a control receipt's answer is its completion
    /// and never lives here). See `value`.
    value: Option<Value>,
    control: Option<Completion>,
    // The result payload can require a newer decoder than the Result request.
    // Never send v4 QSO evidence to a browser which only understands v2/v3.
    result_version: u8,
}
impl Receipt {
    /// What a replay or a Result request is answered with. A receipt whose write has not come
    /// back — its file sync or network post runs with Core released — answers `remoteBusy`,
    /// exactly what the browser was told while that work held the lock, so it waits rather than
    /// appending or posting twice.
    fn value(&self) -> Result<Value, &'static str> {
        match (&self.control, &self.value) {
            (Some(control), _) => Ok(control_value(&self.id, control.outcome())),
            (None, Some(value)) => Ok(value.clone()),
            (None, None) => Err("remoteBusy"),
        }
    }
    fn pending(&self) -> bool {
        match &self.control {
            Some(control) => control.outcome() == Outcome::Pending,
            None => self.value.is_none(),
        }
    }
}
fn control_value(id: &str, outcome: Outcome) -> Value {
    let mut value = serde_json::to_value(outcome)
        .unwrap_or_else(|_| json!({"outcome":"unknown","reason":"hardwareUnconfirmed"}));
    value["operation"] = json!("stationControl");
    value["operationId"] = json!(id);
    value
}
/// A control's outcome has settled: what the transport needs to tell the browser unprompted
/// (operation v5). Carries nothing the browser could not poll for — it is the poll, arriving
/// early — so a notice that is dropped costs a round trip and never an outcome.
#[derive(Clone, Debug)]
pub struct CompletionNotice {
    pub session: String,
    pub device: String,
    pub operation_id: String,
    pub version: u8,
    pub connection: u64,
}
struct Core {
    epoch: u64,
    connection: u64,
    lease_epoch: u64,
    boot: Option<String>,
    grants: BTreeSet<String>,
    control_grants: BTreeSet<String>,
    transmit_grants: BTreeSet<String>,
    lease: Option<Lease>,
    revision: u64,
    context: Option<Vec<u8>>,
    windows: VecDeque<Window>,
    receipts: VecDeque<Receipt>,
}
impl Core {
    /// Record a receipt and spend the command window it was admitted under.
    fn open(&mut self, receipt: Receipt) {
        self.receipts.push_back(receipt);
        while self.receipts.len() > 1024 {
            self.receipts.pop_front();
        }
        self.windows.clear();
    }
}
impl Default for Core {
    fn default() -> Self {
        Self {
            epoch: 0,
            connection: 0,
            lease_epoch: 0,
            boot: snapshot_id().ok(),
            grants: BTreeSet::new(),
            control_grants: BTreeSet::new(),
            transmit_grants: BTreeSet::new(),
            lease: None,
            revision: 0,
            context: None,
            windows: VecDeque::new(),
            receipts: VecDeque::new(),
        }
    }
}
/// Grants remembered from before a restart or a Turn on, per browser: remote logging, station
/// control and FT8/FT4 transmit. See `Authority::restore`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DurableGrants {
    pub logging: Vec<String>,
    pub control: Vec<String>,
    pub transmit: Vec<String>,
}
/// A grant a local revoke removes without waiting for Core (#318). Transmit has its own queue.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Grant {
    Logging,
    Station,
}
#[derive(Default)]
pub struct Authority {
    epoch: AtomicU64,
    connection: AtomicU64,
    lease_epoch: AtomicU64,
    core: Mutex<Core>,
    hardware: Revocation,
    transmit: TransmitAuthority,
    /// A streamed session's transmit presence (see `stream_presence`). Its own authority, not
    /// `transmit`'s: the engine's stand-down (`halt_tx`) moves `transmit`, and a Stop ends a
    /// transmission, not the session that is watching it. Revoked with every other execution
    /// authority when a lease ends, which stops everything at the station on the next tick.
    presence: TransmitAuthority,
    stop_owner: Mutex<Option<transmit_stop::Owner>>,
    transmit_revocations: Mutex<BTreeSet<String>>,
    /// Logging and station-control revokes, queued as `transmit_revocations` is; see
    /// `revoke_grant_device`.
    grant_revocations: Mutex<BTreeSet<(Grant, String)>>,
    /// Where a settled control's notice goes (see `CompletionNotice`): the live transport's
    /// bounded queue, set per socket. None, or a full queue, means the browser polls as it did
    /// before operation v5 — this is never load-bearing.
    completions: Mutex<Option<tokio::sync::mpsc::Sender<CompletionNotice>>>,
    /// The LAN road's open connections. Its own lock, never held while another is taken, so
    /// asking whether a connection is open can wait on nothing.
    lan: Mutex<LanRoad>,
    /// The LAN connection that holds the lease, or 0: a mirror of the lease, kept under Core, for
    /// the relay's departures, which revoke without Core and must leave a LAN controller's
    /// transmission alone. Without a LAN connection it is always 0, and they revoke as before.
    lan_lease: AtomicU64,
    /// One stream at a time for the whole station, whichever road it came by: a stream's end
    /// is the station's (its presence, the held PTT, the window's reset), so a second may not start
    /// while the first is still ending. See `claim_stream`.
    streaming: std::sync::atomic::AtomicBool,
    /// The LAN road's paired computers, who hold station control while LAN is on (see
    /// `hold_lan_devices`). Unset in a build of the service without a LAN road.
    lan_devices: std::sync::OnceLock<LanDevices>,
    /// The computers approved for the room's tunnel, who hold station control while hosted Remote
    /// is on (see `hold_tunnel_devices`). Unset in a build of the service without the tunnel.
    tunnel_devices: std::sync::OnceLock<LanDevices>,
    #[cfg(test)]
    before_sync: Option<Box<dyn Fn() + Send + Sync>>,
}
/// The device ids of the LAN road's paired computers while LAN is on, and none while it is off.
/// Called under Core: it may take no lock that is ever held while Core is taken.
pub type LanDevices = Box<dyn Fn() -> Vec<String> + Send + Sync>;
impl Authority {
    /// The authority's lock, held for as long as a test keeps what this returns: every request
    /// that comes meanwhile is answered `remoteBusy`, as it is while another request holds it.
    #[cfg(test)]
    pub(crate) fn busy_for_a_test(&self) -> impl Sized + '_ {
        self.core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The transmit authority's revocation, for the ENGINE to hold so that a local stand-down
    /// (`Engine::halt_tx`) retires remote transmit the way Remote's own Stop does. Shares the
    /// counter a browser is shown as `transmitEpoch`.
    pub fn transmit_revocation(&self) -> tempo_app::remote_control::Revocation {
        self.transmit.revocation()
    }
    fn revoke_execution(&self) {
        self.hardware.revoke();
        self.transmit.revoke();
        self.presence.revoke();
    }
    /// The relay's road moved (its socket replaced or gone, or a browser on it left): what was
    /// issued under its lease ends at once, before Core is free, as it always has. A lease held on
    /// the LAN is not the relay's, and what it keeps on the air goes on. With no LAN
    /// connection there is no LAN lease, and this is `revoke_execution`.
    fn revoke_relay_execution(&self) {
        if self.lan_lease.load(Ordering::SeqCst) == 0 {
            self.revoke_execution();
        }
    }
    /// The LAN road's paired computers hold station control while LAN is on, whatever clears
    /// grants: `reconcile` reads `devices` each time and gives each one it names its grant back,
    /// so a local decision that clears every grant (`invalidate`: Turn Remote on or off, take over)
    /// leaves them theirs before any next request is answered, as `restore`
    /// does for hosted browsers once the service lists them. It only ever gives: a computer
    /// removed at the shack, or LAN going off, is first dropped from `devices` and then revoked
    /// (`permit_station`), and the revoke stands. Set once, by the LAN road at its start; with no
    /// LAN road, or nobody paired, or LAN off, `reconcile` is as it was.
    pub fn hold_lan_devices(&self, devices: LanDevices) {
        let _ = self.lan_devices.set(devices);
    }
    /// The computers approved here for the room's tunnel hold station control as the LAN's paired
    /// computers do (operator decision 2026-10-10, one rule for every device): `reconcile` reads
    /// `devices` each time and gives each name it lists its grant back, so Turn Remote on, or a
    /// revoke of someone else, leaves them theirs. It only ever gives: a computer removed or no
    /// longer approved is first dropped from `devices` and then revoked (`permit_station`), and the
    /// revoke stands; with hosted Remote off `devices` lists nobody, so the clear that turning it off
    /// makes (`invalidate`) stands too. Names are the shack's own (`lan::tunnel_device_id`), never a
    /// service id. Set once, by the service at its start; `devices` takes only locks of its own,
    /// since it runs under Core.
    pub fn hold_tunnel_devices(&self, devices: LanDevices) {
        let _ = self.tunnel_devices.set(devices);
    }
    /// Is `device` one of the computers that hold station control here (the LAN's paired ones
    /// while LAN is on, the tunnel's approved ones while hosted Remote is on)? A computer reads
    /// only over its own pinned link, so the relay's lane answers no read in its name.
    pub fn is_computer(&self, device: &str) -> bool {
        [&self.lan_devices, &self.tunnel_devices]
            .into_iter()
            .filter_map(|devices| devices.get())
            .any(|devices| devices().iter().any(|d| d == device))
    }
    /// Synchronous invalidation does not wait for an in-flight file operation.
    /// Its epoch is reconciled before any next request or local grant.
    pub fn invalidate(&self) {
        self.revoke_execution();
        let _ = self
            .epoch
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1));
    }
    pub fn start_connection(&self) -> u64 {
        self.revoke_relay_execution();
        self.connection
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_or(u64::MAX, |n| n + 1)
    }
    pub fn retire_connection(&self, id: u64) {
        if self
            .connection
            .compare_exchange(id, id.saturating_add(1), Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.revoke_relay_execution();
        }
    }
    /// A computer's connection on the LAN road opened. It disturbs nothing: not the relay's
    /// connection, nor a lease held on either road. Its id carries [`LAN_ROAD`].
    pub fn start_lan_connection(&self) -> u64 {
        let Ok(mut lan) = self.lan.lock() else {
            return u64::MAX;
        };
        if lan.next >= LAN_ROAD - 1 {
            return u64::MAX;
        }
        lan.next += 1;
        let id = LAN_ROAD | lan.next;
        lan.open.insert(id);
        id
    }
    /// That connection ended. If it held the lease, what the lease kept on the air ends at once,
    /// before Core is free, and the lease itself at the next `reconcile`. Any other connection's
    /// lease, on either road, is untouched.
    pub fn retire_lan_connection(&self, id: u64) {
        if let Ok(mut lan) = self.lan.lock() {
            lan.open.remove(&id);
        }
        if self
            .lan_lease
            .compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.revoke_execution();
        }
    }
    fn lan_open(&self, id: u64) -> bool {
        self.lan.lock().is_ok_and(|lan| lan.open.contains(&id))
    }
    /// May a request on `connection` be answered: the relay's current socket, or a LAN connection
    /// still open?
    fn current(&self, connection: u64) -> bool {
        match lan_road(connection) {
            Some(id) => self.lan_open(id),
            None => connection == self.connection.load(Ordering::SeqCst),
        }
    }
    /// Hold the station's one stream slot (see `streaming`), or `None` while a stream on either
    /// road still holds it. Given back when the claim is dropped, however its stream ends.
    pub fn claim_stream(self: &std::sync::Arc<Self>) -> Option<StreamClaim> {
        self.streaming
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then(|| StreamClaim(self.clone()))
    }
    fn reconcile(&self, c: &mut Core, now: Instant) -> Result<(), &'static str> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch == u64::MAX {
            return Err("authorityUnavailable");
        }
        let connection = self.connection.load(Ordering::SeqCst);
        let lease_epoch = self.lease_epoch.load(Ordering::SeqCst);
        if connection == u64::MAX || lease_epoch == u64::MAX {
            return Err("authorityUnavailable");
        }
        if c.connection != connection || c.lease_epoch != lease_epoch {
            c.lease_epoch = lease_epoch;
            c.connection = connection;
            // The relay's road moved. A lease the LAN holds is not the relay's to end.
            if c.lease.as_ref().is_none_or(|l| l.lan.is_none()) {
                c.lease = None;
                c.windows.clear();
                self.advance(c)?;
            }
        }
        // A lease taken on the LAN ends with its connection. `retire_lan_connection` has
        // already revoked what it kept on the air, unless it was taken while the connection ended.
        if c.lease
            .as_ref()
            .and_then(|l| l.lan)
            .is_some_and(|id| !self.lan_open(id))
        {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        if c.epoch != epoch {
            c.epoch = epoch;
            c.grants.clear();
            c.control_grants.clear();
            c.transmit_grants.clear();
            c.lease = None;
            c.windows.clear();
            c.context = None;
            self.advance(c)?;
        }
        // The LAN road's paired computers keep station control while LAN is on (see
        // `hold_lan_devices`). Before the queued revocations below, so a revoke wins.
        if let Some(devices) = self.lan_devices.get() {
            c.control_grants
                .extend(devices().into_iter().filter(|d| identifier(d)));
        }
        // The tunnel's approved computers the same way, while hosted Remote is on (see
        // `hold_tunnel_devices`).
        if let Some(devices) = self.tunnel_devices.get() {
            c.control_grants
                .extend(devices().into_iter().filter(|d| identifier(d)));
        }
        // A local revoke cannot wait behind a durable append. Retire its grant
        // before any later heartbeat, state or command can consume Core again.
        let revoked = std::mem::take(
            &mut *self
                .transmit_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?,
        );
        for device in revoked {
            c.transmit_grants.remove(&device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.transmit.revoke();
            }
        }
        // Logging and station control the same way (#318). A revoked controller loses its
        // lease here, as it always has when Core was free.
        let grants_revoked = std::mem::take(
            &mut *self
                .grant_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?,
        );
        let mut controller_revoked = false;
        for (grant, device) in grants_revoked {
            match grant {
                Grant::Logging => {
                    c.grants.remove(&device);
                }
                Grant::Station => {
                    c.control_grants.remove(&device);
                    c.transmit_grants.remove(&device);
                }
            }
            controller_revoked |= c.lease.as_ref().is_some_and(|l| l.device == device);
        }
        if controller_revoked {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        if c.lease.as_ref().is_some_and(|l| now >= l.until) {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        while c
            .windows
            .front()
            .is_some_and(|w| now.saturating_duration_since(w.at) >= WINDOW)
        {
            c.windows.pop_front();
        }
        while c
            .receipts
            .front()
            .is_some_and(|r| now.saturating_duration_since(r.at) >= RESULT_AGE)
        {
            c.receipts.pop_front();
        }
        self.sync_stop_owner(c, now);
        Ok(())
    }
    fn advance(&self, c: &mut Core) -> Result<(), &'static str> {
        if c.revision >= MAX_COUNTER {
            self.revoke_execution();
            c.windows.clear();
            c.lease = None;
            return Err("authorityUnavailable");
        }
        c.revision += 1;
        Ok(())
    }
    /// The transport's queue for settled controls. One socket at a time holds the station, so
    /// the newest simply replaces the last; a notice for an older connection is refused by
    /// `completion_event` anyway.
    pub fn watch_completions(&self, sink: tokio::sync::mpsc::Sender<CompletionNotice>) {
        if let Ok(mut current) = self.completions.lock() {
            *current = Some(sink);
        }
    }
    /// Arrange for `notice` to reach the transport when `completion` settles — only for a
    /// browser that negotiated v5, because an older page has no parser for the event and its
    /// socket would close on it. `try_send`, never a wait: the queue is bounded and a full one
    /// drops the notice, which the browser's own polling covers.
    fn notify_on_finish(&self, completion: &Completion, notice: CompletionNotice) {
        if notice.version < 5 {
            return;
        }
        let Some(sink) = self.completions.lock().ok().and_then(|s| s.clone()) else {
            return;
        };
        completion.on_finish(std::sync::Arc::new(move || {
            let _ = sink.try_send(notice.clone());
        }));
    }
    /// The unprompted `operationEvent` for a settled control: its outcome and the state a
    /// `State` request would return now — computed under the same locks, in the same order, as
    /// `handle_version`, so the browser can install it exactly as it installs a reply. None when
    /// there is nothing to say (the receipt is gone, the socket has changed) and the browser is
    /// left to its poll.
    ///
    /// Waits for both locks rather than answering `stationBusy`: the radio loop that settled
    /// the outcome still holds Engine when the notice arrives, and this runs on a blocking
    /// thread the transport does not wait on. Engine is taken first, as `handle_version` takes
    /// Core then tries Engine and every other Core user only tries, so nothing can wait on
    /// this thread while it waits.
    pub fn completion_event(
        &self,
        notice: &CompletionNotice,
        engine: &crate::SharedEngine,
    ) -> Option<String> {
        let now = Instant::now();
        let engine = tempo_app::engine::engine_lock_result(engine).ok()?;
        let mut c = self.core.lock().ok()?;
        self.reconcile(&mut c, now).ok()?;
        if !self.current(notice.connection) {
            return None;
        }
        let value = c
            .receipts
            .iter()
            .find(|r| {
                r.id == notice.operation_id
                    && r.session == notice.session
                    && r.device == notice.device
            })?
            .value()
            .ok()?;
        self.context(&mut c, &engine).ok()?;
        let control = Some((
            notice.version,
            station::Context::capture(&engine),
            engine.remote_ft_available(),
            engine.remote_ft_tx_owned(),
        ));
        let state = self
            .state(&mut c, &notice.session, &notice.device, now, control)
            .ok()?;
        Some(
            json!({"type":"operationEvent","sessionId":notice.session,"operationId":notice.operation_id,"value":value,"state":state})
                .to_string(),
        )
    }
    /// The epoch every local grant belongs to. Any local decision that clears permissions
    /// (Turn off Remote, take over, turning Remote on again) moves it. Revoking one browser does not:
    /// it takes that browser's grants alone (`permit`, `permit_station`, `permit_transmit`).
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    /// Put back grants remembered for still-approved browsers when Remote comes on (at launch, or
    /// Turn on Remote).
    ///
    /// `epoch` is the epoch captured when Remote came on. If it has moved, a local decision has been
    /// made since and its answer stands: nothing is restored. The grants land in a fresh revision
    /// with no command window and no lease, so nothing issued before the restore can be used with
    /// them.
    ///
    /// FT8/FT4 transmit is restored only for a browser that also holds station control, exactly as
    /// `permit_transmit` requires. ⛔ Restoring it arms nothing: the TX-enable latch is untouched,
    /// the lease is dropped, and keying still needs the browser's own TX On under a fresh lease.
    pub fn restore(&self, epoch: u64, grants: &DurableGrants) -> Result<bool, &'static str> {
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, Instant::now())?;
        if c.epoch != epoch || self.epoch.load(Ordering::SeqCst) != epoch {
            return Ok(false);
        }
        for device in grants.logging.iter().filter(|d| identifier(d)) {
            if c.grants.len() < 16 {
                c.grants.insert(device.clone());
            }
        }
        for device in grants.control.iter().filter(|d| identifier(d)) {
            if c.control_grants.len() < 16 {
                c.control_grants.insert(device.clone());
            }
        }
        for device in &grants.transmit {
            if c.control_grants.contains(device) {
                c.transmit_grants.insert(device.clone());
            }
        }
        if c.lease.take().is_some() {
            self.revoke_execution();
        }
        c.windows.clear();
        self.advance(&mut c)?;
        self.sync_stop_owner(&c, Instant::now());
        Ok(true)
    }
    /// A revoke never waits for Core (#318): it is queued for `reconcile`, which applies it
    /// before anything else consumes Core, as `permit_transmit` does. A grant may still be
    /// refused `remoteBusy`.
    pub fn permit(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        if !allow {
            self.revoke_grant_device(Grant::Logging, device)?;
        }
        let mut c = match self.core.try_lock() {
            Ok(c) => c,
            Err(std::sync::TryLockError::WouldBlock) if !allow => return Ok(()),
            Err(_) => return Err("remoteBusy"),
        };
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if c.grants.len() >= 16 && !c.grants.contains(device) {
                return Err("remoteBusy");
            }
            c.grants.insert(device.to_string());
        }
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// Revoking station control revokes transmit with it; like `permit`, a revoke never waits.
    pub fn permit_station(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        if !allow {
            self.revoke_grant_device(Grant::Station, device)?;
        }
        let mut c = match self.core.try_lock() {
            Ok(c) => c,
            Err(std::sync::TryLockError::WouldBlock) if !allow => return Ok(()),
            Err(_) => return Err("remoteBusy"),
        };
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if c.control_grants.len() >= 16 && !c.control_grants.contains(device) {
                return Err("remoteBusy");
            }
            c.control_grants.insert(device.to_string());
        }
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// Local permission, never implied by a receiver grant, and it arms nothing.
    /// Removing it revokes the current TX permit without waiting for Engine.
    pub fn permit_transmit(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        if !allow {
            self.revoke_transmit_device(device)?;
        }
        let mut c = match self.core.try_lock() {
            Ok(c) => c,
            Err(std::sync::TryLockError::WouldBlock) if !allow => return Ok(()),
            Err(_) => return Err("remoteBusy"),
        };
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if !c.control_grants.contains(device) {
                return Err("localPermissionRequired");
            }
            c.transmit_grants.insert(device.to_string());
        } else {
            c.transmit_grants.remove(device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.transmit.revoke();
            }
        }
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// May this browser listen to the station's receive audio right now?
    ///
    /// Two things, and both are the operator's: the local **control** grant, and this
    /// browser's own live lease. A logging-only browser holds `grants` and a perfectly
    /// valid lease and is refused here — hearing a shack is not a consequence of being
    /// allowed to write to its log, and the two permissions are given separately at the
    /// radio precisely so they can be withheld separately.
    ///
    /// Nothing here can key anything. It is a read of permission state, and the caller
    /// it serves holds no transmit authority at all.
    ///
    /// `remoteBusy` means the authority lock was held, not that the browser was refused.
    /// The audio lane re-asks on a cadence, and treating contention as a refusal would
    /// drop a listener every time an operation happened to be in flight.
    pub fn audio_admitted(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        if !identifier(session) || !identifier(device) || !identifier(lease_id) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, now)?;
        if !c.control_grants.contains(device) {
            return Err("notController");
        }
        let lease = c.lease.as_ref().ok_or("notController")?;
        if lease.id != lease_id || lease.session != session || lease.device != device {
            return Err("notController");
        }
        Ok(())
    }
    /// May this browser stream the station (security acceptance test A3)? Exactly the audio lane's
    /// rule, deliberately one definition: the operator's local control grant plus this browser's
    /// own live lease. Asked before any WebRTC state exists, and again on a slow cadence while a
    /// stream runs, so a lapsed lease or a revoked device ends it.
    pub fn stream_admitted(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        self.audio_admitted(session, device, lease_id, now)
    }

    /// S8: the streamed session's transmit presence, minted under the lease that admitted it. The
    /// same checks as admission; a deadline of five seconds from now, and never past the lease.
    ///
    /// ⛔ IT GRANTS NOTHING. It is not the FT permit (`permit_generation`): nothing is armed or
    /// keyed with it and no browser gesture is bound to it, so there is no displayed generation to
    /// check it against. Held by the engine, its only power is to stop every transmission at the
    /// station when it lapses (`engine/remote_stream.rs`).
    pub fn stream_presence(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<tempo_app::remote_control::transmit::TransmitPermit, &'static str> {
        self.presence_until(session, device, lease_id, now)
            .map(|(permit, _)| permit)
    }

    /// [`Self::stream_presence`], with the deadline it was given: what Stop's second road tells the
    /// computer whose heartbeat renewed it.
    pub(crate) fn presence_until(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(tempo_app::remote_control::transmit::TransmitPermit, Instant), &'static str> {
        if !identifier(session) || !identifier(device) || !identifier(lease_id) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, now)?;
        if !c.control_grants.contains(device) {
            return Err("notController");
        }
        let lease = c.lease.as_ref().ok_or("notController")?;
        if lease.id != lease_id || lease.session != session || lease.device != device {
            return Err("notController");
        }
        let deadline = (now + LEASE).min(lease.until);
        self.presence
            .permit(deadline)
            .map(|permit| (permit, deadline))
            .ok_or("authorityUnavailable")
    }

    /// The authority's lock, held by a test as a slow request would hold it, until the guard goes.
    #[cfg(test)]
    pub(crate) fn held(&self) -> impl Sized + '_ {
        self.core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Does the LAN connection `connection` hold the station's lease now, for `device`, which holds
    /// station control? What the command lane asks before it admits any call but a Stop, and before
    /// it pushes a feed or an event. Read under Core after a reconcile, so a lease that ran out or
    /// went with a revoke reads as gone even when nothing else has asked since. Off the radio's
    /// path: Core is never held across anything slow, and a Stop never asks.
    pub fn lan_controls(&self, connection: u64, device: &str, now: Instant) -> bool {
        let Ok(mut c) = self.core.lock() else {
            return false;
        };
        if self.reconcile(&mut c, now).is_err() {
            return false;
        }
        c.control_grants.contains(device)
            && c.lease
                .as_ref()
                .is_some_and(|lease| lease.lan == Some(connection) && lease.device == device)
    }

    /// The stream ended: its presence ends with it, and the engine halts on its next tick if it
    /// held any. Ending a session that never connected stops nothing, because it held none.
    pub fn end_stream_presence(&self) {
        self.presence.revoke();
    }

    /// Any admitted browser departure ends the shared logging lease. This is
    /// deliberately conservative and cannot wait behind a file append. Grants
    /// survive; a controller must explicitly acquire a fresh lease afterward.
    pub fn disconnect_session(&self, session: &str) {
        if identifier(session) {
            self.revoke_relay_execution();
            let _ = self
                .lease_epoch
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1));
        }
    }
    pub fn local_status(&self) -> Value {
        let Ok(mut c) = self.core.try_lock() else {
            return json!({"devices":[],"controller":null});
        };
        if self.reconcile(&mut c, Instant::now()).is_err() {
            return json!({"devices":[],"controller":null});
        }
        json!({"devices":c.grants,"controlDevices":c.control_grants,"transmitDevices":c.transmit_grants,"controller":c.lease.as_ref().map(|l|l.device.as_str())})
    }
    fn context(
        &self,
        c: &mut Core,
        engine: &tempo_app::engine::Engine,
    ) -> Result<(), &'static str> {
        let s = engine.settings();
        let value = json!([
            engine.remote_log_context_generation(),
            engine.remote_receiver_context_generation(),
            engine.remote_actuation_context_generation(),
            s.active_radio,
            s.mycall,
            s.mygrid,
            s.fd_operator,
            s.fd_active,
            s.dial_mhz,
            s.band,
            s.operating_mode
        ]);
        let next = digest(
            &SHA256,
            &serde_json::to_vec(&value).map_err(|_| "authorityUnavailable")?,
        )
        .as_ref()
        .to_vec();
        if c.context.as_ref() != Some(&next) {
            c.context = Some(next);
            c.windows.clear();
            self.advance(c)?;
        }
        Ok(())
    }
    fn state(
        &self,
        c: &mut Core,
        session: &str,
        device: &str,
        now: Instant,
        control: Option<(u8, station::Context, bool, bool)>,
    ) -> Result<Value, &'static str> {
        self.sync_stop_owner(c, now);
        let allowed =
            c.grants.contains(device) || (control.is_some() && c.control_grants.contains(device));
        let owned = allowed
            && c.lease
                .as_ref()
                .is_some_and(|l| l.session == session && l.device == device);
        if owned
            && c.windows
                .back()
                .is_none_or(|w| now.saturating_duration_since(w.at) >= Duration::from_millis(500))
        {
            c.windows.push_back(Window {
                id: snapshot_id()?,
                at: now,
            });
            while c.windows.len() > 4 {
                c.windows.pop_front();
            }
        }
        let mut value = json!({"stationBootId":c.boot.as_deref().ok_or("authorityUnavailable")?,"allowed":allowed,
            "phase":if owned{"controlling"}else if c.lease.is_some(){"occupied"}else if allowed{"available"}else{"localPermissionRequired"},
            "leaseId":if owned{c.lease.as_ref().map(|l|l.id.as_str())}else{None},"revision":c.revision,
            "commandWindowId":if owned{c.windows.back().map(|w|w.id.as_str())}else{None},
            "nextSequence":if owned{c.lease.as_ref().map(|l|l.sequence+1)}else{None},
            "leaseRemainingMs":if owned{c.lease.as_ref().map(|l|l.until.saturating_duration_since(now).as_millis() as u64)}else{None},
            // `actions` names no action any more, and stays: every shipped page requires the field.
            "actions":Vec::<&str>::new(),"txArmed":false});
        if control
            .as_ref()
            .is_some_and(|(version, _, _, _)| *version >= 4)
        {
            // The stop token goes to any controlling browser (stop anything, 2026-09-14), and stays
            // with one whose lease has since run out (expired-lease stop, 2026-09-15) — that is what
            // `holds_stop_token` is, and for a live controller it is exactly the old
            // `owned && control_grants.contains(device)`. It starts nothing: every FT action also
            // checks the live lease, the transmit grant and its own capability.
            value["transmitEpoch"] = if self.holds_stop_token(session, device) {
                json!(format!("{:016x}", self.transmit.generation()))
            } else {
                Value::Null
            };
        }
        if let Some((version, context, ft_available, tx_owned)) = control {
            let mut capabilities = vec![];
            if version >= 4 {
                if c.grants.contains(device) {
                    capabilities.push("qsoLogging");
                }
                if c.control_grants.contains(device) {
                    // Listening to the station's receive audio. The hint is what makes
                    // the negotiation work in BOTH directions: a browser that has never
                    // heard of it never offers a listen control, and a station built
                    // without the audio lane never advertises it, so nobody is ever
                    // asked for something the other end cannot do. It is under the
                    // CONTROL grant, not the logging one — see `audio_admitted`.
                    #[cfg(feature = "radio")]
                    capabilities.push("audioListen");
                    // Operation v5: this station tells the browser a control's outcome the
                    // moment it settles (`operationEvent`), so the page may stop re-reading
                    // after each command. NEGOTIATED, never assumed from the page's own
                    // version: `version` is the one the relay agreed for this request, so a
                    // v5 page on an older station never sees the hint and keeps its immediate
                    // re-read. A hint like `audioListen` - it names no action.
                    if version >= 5 {
                        capabilities.push("outcomePush");
                    }
                }
                value["txArmed"] = json!(owned && tx_owned);
                if ft_available
                    && c.control_grants.contains(device)
                    && c.transmit_grants.contains(device)
                {
                    capabilities.extend([
                        "ftOperate",
                        "ftCall",
                        "ftExchange",
                        "ftMessages",
                        "ftSettings",
                        "ftRuntime",
                    ]);
                }
            }
            value["controls"] = json!({"context":context,"capabilities":capabilities});
        }
        Ok(value)
    }
    /// The serialized size and fingerprint a replay is matched on.
    ///
    /// ⚠️ **This must match exactly the receipt-bearing arm in [`Self::handle_version`]**: a
    /// request that arm takes with no fingerprint is refused `invalidRequest`.
    ///
    /// `None` for every other request, deliberately: one whose arm keeps no receipt must not
    /// acquire a lookup it never had. The size is returned rather than bounded here so the
    /// 4 KiB refusal keeps its old position inside the arm.
    fn replay_fingerprint(request: &Request) -> Result<Option<(usize, Vec<u8>)>, &'static str> {
        if !matches!(request, Request::StationControl { .. }) {
            return Ok(None);
        }
        let bytes = serde_json::to_vec(request).map_err(|_| "invalidRequest")?;
        let fingerprint = digest(&SHA256, &bytes).as_ref().to_vec();
        Ok(Some((bytes.len(), fingerprint)))
    }

    pub fn handle(
        &self,
        connection: u64,
        session: &str,
        device: &str,
        request: &Request,
        engine: &crate::SharedEngine,
        now: Instant,
    ) -> Result<Value, &'static str> {
        self.handle_version((connection, 1), session, device, request, engine, now)
    }
    pub fn handle_version(
        &self,
        (connection, version): (u64, u8),
        session: &str,
        device: &str,
        request: &Request,
        engine: &crate::SharedEngine,
        now: Instant,
    ) -> Result<Value, &'static str> {
        if matches!(request, Request::StopTransmit { .. }) {
            if version != 4 {
                return Err("stationUnsupported");
            }
            self.stop_transmit(connection, session, device, request, now)?;
            transmit_stop::stop_station(engine);
            return Ok(json!({"stop":"accepted"}));
        }
        if !matches!(version, 1..=5)
            || matches!(request, Request::StationControl { action, .. } if version < action.minimum_version())
        {
            return Err("stationUnsupported");
        }
        if !self.current(connection) {
            return Err("staleConnection");
        }
        if !identifier(session) || !identifier(device) || !identifier(request.id()) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, now)?;
        // A permission refusal never depends on Engine contention: a device that
        // lost its local permission is told so even while Engine is busy. Each
        // request arm below repeats its own check, so nothing more is allowed.
        if !permitted(&c, version, device, request) {
            return Err("localPermissionRequired");
        }
        // ⚠️ A REPLAY IS ANSWERED BEFORE THE ENGINE LOCK, AND THAT ORDER IS THE WHOLE FIX.
        //
        // A duplicate request's entire answer is a stored [`Receipt`]: Core guards `receipts`
        // and the Engine is not consulted for one. The `try_lock` below fails the WHOLE call
        // whenever the station's own radio loop holds the Engine for a tick, so while this
        // lookup sat 160 lines downstream a replay was refused `stationBusy` for a lock it
        // never needed — and a browser could not retrieve the receipt for an operation that
        // had ALREADY applied. It reached CI as a flaky `logManual` replay
        // (`remote/test/native.test.mjs:782`), which is a race against a ~20 ms tick.
        //
        // That the refusal was never intended is recorded one screen up in [`Receipt::value`]:
        // a replay whose write is still in flight answers `remoteBusy`, "so it waits rather
        // than appending or posting twice". The design already has a considered answer for a
        // contended replay, and `stationBusy` is not it.
        //
        // Only the fingerprint moves. The size bound and every other check stay where they
        // were in the arm, so no error keeps a different precedence than before.
        //
        // ⚠️ WHAT BOTH EARLY ANSWERS BELOW DO SKIP is `self.context(&mut c, &engine)`, which
        // runs after the lock and, when the station context has changed, clears the command
        // windows and advances the revision. That is unavoidable — it needs the very Engine
        // these two do not take — and it is safe, because it is housekeeping a READ cannot
        // make unsafe: the next request that actually writes takes the lock, runs `context`
        // itself, and refuses on `staleContext` / `windowExpired` exactly as before. Receipt
        // ageing is unaffected; `reconcile` above already ran.
        let replay = Self::replay_fingerprint(request)?;
        if let Some((_, fingerprint)) = replay.as_ref() {
            if let Some(r) = c.receipts.iter().find(|r| r.id == request.id()) {
                return if r.session == session
                    && r.device == device
                    && r.fingerprint == *fingerprint
                {
                    r.value()
                } else {
                    Err("requestConflict")
                };
            }
        }
        // …and so is a `Result` request, for the identical reason: it is a pure receipt READ.
        // It reads `c.receipts`, `c.grants`/`c.control_grants` and `device` — Core and nothing
        // else — so the Engine lock below was refusing `stationBusy` to a browser that only
        // wanted the answer already on file. No failure was ever attributed to this one; it is
        // the same defect as the replay above, one arm over, and swept with it rather than
        // left to be rediscovered with a smaller window.
        if let Request::Result { operation_id, .. } = request {
            if !identifier(operation_id)
                || !(c.grants.contains(device) || version >= 2 && c.control_grants.contains(device))
            {
                return Err("localPermissionRequired");
            }
            return c
                .receipts
                .iter()
                .find(|r| {
                    r.id == *operation_id
                        && r.device == device
                        && (version >= 2 || r.control.is_none())
                })
                .ok_or("resultExpired")
                .and_then(|receipt| {
                    if version < receipt.result_version {
                        Err("stationUnsupported")
                    } else {
                        receipt.value()
                    }
                });
        }
        // Capture context and execute under the same engine lock. There is no
        // queue whose work could migrate into a later radio/profile context.
        let shared_engine = engine;
        let mut engine =
            tempo_app::engine::engine_try_lock(shared_engine).map_err(|_| "stationBusy")?;
        self.context(&mut c, &engine)?;
        let control = (version >= 2).then(|| {
            (
                version,
                station::Context::capture(&engine),
                engine.remote_ft_available(),
                engine.remote_ft_tx_owned(),
            )
        });
        match request {
            Request::StopTransmit { .. } => unreachable!("handled before ordinary operations"),
            Request::State { .. } => self.state(&mut c, session, device, now, control),
            Request::Acquire {
                station_boot_id, ..
            } => {
                if !(c.grants.contains(device) || version >= 2 && c.control_grants.contains(device))
                {
                    return Err("localPermissionRequired");
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                if let Some(l) = &c.lease {
                    // One controller for the whole station, on either road.
                    if l.session != session || l.device != device || l.lan != lan_road(connection) {
                        return Err("controllerBusy");
                    }
                } else {
                    c.lease = Some(Lease {
                        id: snapshot_id()?,
                        session: session.into(),
                        device: device.into(),
                        until: now + LEASE,
                        sequence: 0,
                        lan: lan_road(connection),
                    });
                    self.advance(&mut c)?;
                }
                self.state(&mut c, session, device, now, control)
            }
            Request::Heartbeat { lease_id, .. } => {
                let l = c.lease.as_mut().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                l.until = now + LEASE;
                let until = l.until;
                if c.transmit_grants.contains(device) {
                    if let Some(permit) = self.transmit.permit(until) {
                        // An accepted heartbeat extends only an already-owned,
                        // unexpired transmission. It cannot arm or take over TX.
                        engine.renew_remote_transmit(permit, now);
                    }
                }
                engine.poll_remote_transmit(now);
                self.state(&mut c, session, device, now, control)
            }
            Request::Release { lease_id, .. } => {
                if c.lease.as_ref().is_some_and(|l| {
                    l.id == *lease_id && l.session == session && l.device == device
                }) {
                    self.revoke_execution();
                    c.lease = None;
                    c.windows.clear();
                    self.advance(&mut c)?;
                }
                self.state(&mut c, session, device, now, control)
            }
            Request::Result { .. } => unreachable!("handled before the Engine lock"),
            Request::StationControl {
                station_boot_id,
                lease_id,
                expected_revision,
                command_window_id,
                client_sequence,
                ..
            } => {
                // The same local grant `permitted` checked above, read again under this lock.
                if !permitted(&c, version, device, request) {
                    return Err("localPermissionRequired");
                }
                // Both computed by `replay_fingerprint` before the Engine lock, where the
                // receipt lookup that used to live here now runs — see the note there. The
                // size bound stays in this position so its precedence is unchanged, and
                // reaching this line means no receipt carried this request id.
                let Some((size, fingerprint)) = replay else {
                    return Err("invalidRequest");
                };
                if size > 4096 {
                    return Err("invalidRequest");
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                let l = c.lease.as_ref().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                if *client_sequence == 0 || *client_sequence > MAX_COUNTER {
                    return Err("invalidRequest");
                }
                if *client_sequence <= l.sequence {
                    return Err("resultExpired");
                }
                if *client_sequence != l.sequence + 1 {
                    return Err("sequenceConflict");
                }
                if *expected_revision != c.revision {
                    return Err("staleContext");
                }
                let window = c
                    .windows
                    .iter()
                    .find(|w| w.id == *command_window_id)
                    .ok_or("windowExpired")?;
                let current = now.max(Instant::now());
                if current.saturating_duration_since(window.at) >= WINDOW
                    || current >= l.until
                    || self.epoch.load(Ordering::SeqCst) != c.epoch
                    || !self.current(connection)
                    || c.lease_epoch != self.lease_epoch.load(Ordering::SeqCst)
                {
                    return Err("windowExpired");
                }
                if c.receipts.iter().any(Receipt::pending) {
                    return Err("remoteBusy");
                }
                if let Request::StationControl {
                    action, context, ..
                } = request
                {
                    if action.is_logging() {
                        self.advance(&mut c)?;
                        c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                        let prepared = logging::prepare(&mut engine, action);
                        drop(engine);
                        // The hardware receipt's own shape: pending under Core, finished once
                        // the file sync comes back with Core released (see the log change above).
                        let completion = Completion::default();
                        c.open(Receipt {
                            id: request.id().into(),
                            session: session.into(),
                            device: device.into(),
                            fingerprint,
                            at: current,
                            value: None,
                            control: Some(completion.clone()),
                            result_version: 4,
                        });
                        drop(c);
                        self.notify_on_finish(
                            &completion,
                            CompletionNotice {
                                session: session.into(),
                                device: device.into(),
                                operation_id: request.id().into(),
                                version,
                                connection,
                            },
                        );
                        #[cfg(test)]
                        if let Some(probe) = &self.before_sync {
                            probe();
                        }
                        let outcome = match prepared {
                            Ok(work) => work.finish(shared_engine),
                            Err(reason) => Outcome::Rejected { reason },
                        };
                        completion.finish(outcome.clone());
                        return Ok(control_value(request.id(), outcome));
                    }
                    let deadline = (current + Duration::from_secs(5)).min(l.until);
                    let transmit_permit = if let Some(epoch) = action.transmit_epoch() {
                        if !c.transmit_grants.contains(device) {
                            return Err("localPermissionRequired");
                        }
                        if epoch.len() != 16
                            || !epoch
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        {
                            return Err("invalidRequest");
                        }
                        let expected =
                            u64::from_str_radix(epoch, 16).map_err(|_| "invalidRequest")?;
                        Some(
                            self.transmit
                                .permit_generation(expected, deadline)
                                .ok_or("staleContext")?,
                        )
                    } else {
                        None
                    };
                    // Taken for an FT gesture as it always was, and held to the end of this arm.
                    let _permit = self
                        .hardware
                        .permit(deadline)
                        .ok_or("authorityUnavailable")?;
                    self.advance(&mut c)?;
                    c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                    let executed = if let Some(transmit_permit) = transmit_permit {
                        station::execute_transmit(&mut engine, context, action, transmit_permit)
                    } else {
                        // Every action but the logging ones is an FT gesture, which carries its
                        // transmit epoch.
                        Err(tempo_app::remote_control::Reason::UnsupportedAction)
                    };
                    let completion = match executed {
                        Ok(result) => result,
                        Err(reason) => {
                            let result = Completion::default();
                            result.finish(Outcome::Rejected { reason });
                            result
                        }
                    };
                    let value = control_value(request.id(), completion.outcome());
                    c.open(Receipt {
                        id: request.id().into(),
                        session: session.into(),
                        device: device.into(),
                        fingerprint,
                        at: current,
                        value: None,
                        control: Some(completion.clone()),
                        result_version: 2,
                    });
                    // After the receipt is in, so a settlement that races this registration
                    // (the radio loop can be that fast on a rejected target) finds the receipt
                    // the event is built from. `on_finish` tells at once if it already settled.
                    self.notify_on_finish(
                        &completion,
                        CompletionNotice {
                            session: session.into(),
                            device: device.into(),
                            operation_id: request.id().into(),
                            version,
                            connection,
                        },
                    );
                    return Ok(value);
                }
                Err("invalidRequest")
            }
        }
    }
}

/// The local grant a request needs, read before Engine is locked. Stop and the
/// lease-only requests need none here; their arms keep their own rules.
fn permitted(c: &Core, version: u8, device: &str, request: &Request) -> bool {
    match request {
        Request::Acquire { .. } | Request::Result { .. } => {
            c.grants.contains(device) || version >= 2 && c.control_grants.contains(device)
        }
        Request::StationControl { action, .. } => {
            if action.is_logging() {
                c.grants.contains(device)
            } else {
                c.control_grants.contains(device)
            }
        }
        _ => true,
    }
}

/// A LAN connection's id carries this bit, so no relay connection's id is ever a LAN one's.
/// The relay's ids count up from 1, with `u64::MAX` its exhausted mark, so neither ever has this
/// bit alone at the top.
pub const LAN_ROAD: u64 = 1 << 62;

/// The LAN connection `connection` names, or `None` for the relay's.
fn lan_road(connection: u64) -> Option<u64> {
    (connection >> 62 == 1).then_some(connection)
}

/// The LAN road's connections: the ones open now, and the count their ids are made from.
#[derive(Default)]
struct LanRoad {
    next: u64,
    open: BTreeSet<u64>,
}

/// The station's one stream slot, held by a stream from its offer until it has finished ending.
pub struct StreamClaim(std::sync::Arc<Authority>);
impl Drop for StreamClaim {
    fn drop(&mut self) {
        self.0.streaming.store(false, Ordering::SeqCst);
    }
}

/// One computer's connection on the LAN road. Retired when dropped, however it ends.
pub struct LanConnection {
    pub authority: std::sync::Arc<Authority>,
    pub id: u64,
}
impl LanConnection {
    pub fn new(authority: std::sync::Arc<Authority>) -> Self {
        let id = authority.start_lan_connection();
        Self { authority, id }
    }
}
impl Drop for LanConnection {
    fn drop(&mut self) {
        self.authority.retire_lan_connection(self.id)
    }
}

pub(crate) use transmit_stop::RoadStop;

/// Presence for a computer on Stop's second road (`lan::stop_road`): minted under that computer's
/// own lease, exactly as a stream's is ([`Authority::stream_presence`]), and handed to the engine,
/// which halts every transmission at the station when it lapses. Neither the authority nor the
/// Engine is waited for: the authority busy is a heartbeat that renewed nothing, and the next asks
/// again; the Engine busy leaves the minted permit with the road, which hands it over at its next
/// look, as a stream's waits for its next tick. The road's thread holds this, never the Engine
/// itself.
///
/// ⛔ IT GRANTS NOTHING, as a stream's presence grants nothing: holding it arms and keys nothing,
/// and its only power is to stop the station when it lapses.
pub(crate) struct RoadPresence {
    authority: std::sync::Arc<Authority>,
    engine: crate::SharedEngine,
}

impl RoadPresence {
    pub(crate) fn new(authority: std::sync::Arc<Authority>, engine: crate::SharedEngine) -> Self {
        Self { authority, engine }
    }

    /// Mint presence for `device`'s `lease` on `session`, now: the permit and its deadline, or
    /// `None` when it is not this computer's lease, or the authority was busy.
    pub(crate) fn mint(
        &self,
        session: &str,
        device: &str,
        lease: &str,
        now: Instant,
    ) -> Option<(tempo_app::remote_control::transmit::TransmitPermit, Instant)> {
        self.authority
            .presence_until(session, device, lease, now)
            .ok()
    }

    /// Hand a minted permit to the engine, whose lock is only tried: `None` while the Engine is
    /// busy, else whether the engine holds presence after.
    pub(crate) fn install(
        &self,
        permit: &tempo_app::remote_control::transmit::TransmitPermit,
        now: Instant,
    ) -> Option<bool> {
        let mut engine = match tempo_app::engine::engine_try_lock(&self.engine) {
            Ok(engine) => engine,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(engine.hold_remote_presence(permit.clone(), now))
    }
}

/// Retire the lease on every socket exit, including malformed input and timeout.
/// Local device permission survives a network interruption; a lease never does.
pub struct Connection {
    pub authority: std::sync::Arc<Authority>,
    pub id: u64,
}
impl Connection {
    pub fn new(authority: std::sync::Arc<Authority>) -> Self {
        let id = authority.start_connection();
        Self { authority, id }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.authority.retire_connection(self.id)
    }
}

#[cfg(test)]
pub(crate) mod tests;
