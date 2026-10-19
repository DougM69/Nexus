//! Enrolling a computer on this network by its own sign-in: the account's road onto the LAN, beside
//! the pairing window, with no code and no press at the shack.
//!
//! A computer that signed in to the station's account (as its owner, or as someone the station
//! invited to operate) holds the issuer's ID token for exactly its keys: its proof. On its first
//! contact with this shack it opens TLS with the key it made for this shack (the LAN key its proof
//! names), and its first message is `{"type":"enrol", …, "kind", "publicKey", "proof"}`. That
//! connection reaches nothing but this check: the proof must hold for the key this very TLS session
//! proved and the other key it names (`publicKey`, its key for the internet road), under the
//! station's owner or an invitation, signed within the offline age (`tempo_client::account`). All
//! of it is read from memory, so it holds with the internet down: the owner, the issuer's keys and
//! the people invited, as the station kept them ([`Enrolment::arm`]). Then the key is kept for the
//! LAN's own device cache and paired in the book, the computer is told its device id, and the
//! connection ends; the computer comes back with a hello, as after a pairing.
//!
//! A computer invited only to listen is refused (`listenNative`): every computer in the book holds
//! station control while the LAN is on. A refused proof counts against its address at the gate, as
//! a wrong pairing code does.
//!
//! Nothing here writes a log line, and no proof is kept: the cache holds the key's pin, the role
//! and the proof's signing time.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tempo_client::account::{self, CachedDevice, DeviceCache, Invite, KeySet, Owner, Role, Tenant};
use tokio::sync::watch;

use super::super::vault::AccountVault;
use super::book::{valid_name, Book};
use super::channel::Socket;
use super::pairing::{refuse, say, Desk, Outcome};
use super::tls;

/// The longest proof taken: an ID token is a little over a kilobyte.
const PROOF_CHARS: usize = 4096;

/// What a station bound to its account checks an enrolment against, as it kept it: no token.
pub(crate) struct Ready {
    pub(crate) tenant: &'static Tenant,
    pub(crate) owner: Owner,
    pub(crate) keys: KeySet,
    pub(crate) invites: Vec<Invite>,
    /// Where the LAN's device cache is kept.
    pub(crate) vault: Arc<dyn AccountVault>,
    /// The clock proofs are read by: the station's own.
    pub(crate) now_s: fn() -> u64,
}

/// Whether, and against what, the LAN takes enrolments now. Unarmed (the station is not bound, or
/// its owner or the issuer's keys are not kept) it takes none, and TLS takes no unknown key for one.
#[derive(Default)]
pub(crate) struct Enrolment {
    ready: Mutex<Option<Arc<Ready>>>,
}

impl Enrolment {
    /// Armed with `ready`, or disarmed with `None`.
    pub(crate) fn arm(&self, ready: Option<Ready>) {
        if let Ok(mut held) = self.ready.lock() {
            *held = ready.map(Arc::new);
        }
    }

    /// Whether an enrolment can be checked now: what TLS asks of an unknown key.
    pub(crate) fn open(&self) -> bool {
        self.ready.lock().is_ok_and(|ready| ready.is_some())
    }

    /// What TLS asks of a key no paired computer holds: whether `pairing` (the window) is open, or
    /// this enrolment is.
    pub(super) fn or_pairing(self: &Arc<Self>, pairing: tls::Pairing) -> tls::Pairing {
        let enrolment = self.clone();
        Arc::new(move || pairing() || enrolment.open())
    }

    fn ready(&self) -> Option<Arc<Ready>> {
        self.ready.lock().ok()?.clone()
    }

    /// The time now as proofs are read, while armed.
    fn now_s(&self) -> Option<u64> {
        self.ready().map(|ready| (ready.now_s)())
    }
}

/// What an enrolment's first message asked, with the key its TLS session proved.
pub(super) struct Asked {
    name: String,
    kind: String,
    /// Its other key (for the internet road), SPKI, as the message named it.
    public_key: Vec<u8>,
    proof: String,
    /// The key this session proved, SPKI: the LAN key its proof must name.
    key: Vec<u8>,
}

impl Asked {
    /// `None` for anything out of shape: a name, a kind that is not a computer or a phone, a key
    /// that is not a P-256 key, or a proof empty or too long.
    pub(super) fn new(
        name: &str,
        kind: &str,
        public_key: &str,
        proof: &str,
        key: Vec<u8>,
    ) -> Option<Self> {
        if !valid_name(name)
            || !matches!(kind, "computer" | "phone")
            || !tempo_stream::protocol::device_key(public_key)
            || proof.is_empty()
            || proof.len() > PROOF_CHARS
        {
            return None;
        }
        Some(Self {
            name: name.trim().to_string(),
            kind: kind.to_string(),
            public_key: tempo_stream::protocol::hex_bytes(public_key)?,
            proof: proof.to_string(),
            key,
        })
    }
}

/// Admits the computer `asked` names, under the LAN station id `under`, at `now_s`: its proof
/// checked (see the header), its key kept in the device cache and then paired in the book. Its
/// device id. Blocks on the credential store.
pub(super) fn admit(
    book: &Book,
    enrolment: &Enrolment,
    under: &str,
    asked: &Asked,
    now_s: u64,
) -> Result<String, &'static str> {
    let ready = enrolment.ready().ok_or("enrolClosed")?;
    let vouched = account::vouched(
        &asked.proof,
        &asked.kind,
        &[&asked.public_key, &asked.key],
        &ready.keys,
        ready.tenant,
        &ready.owner,
        &ready.invites,
        now_s,
    )?;
    if vouched.role != Role::Operate {
        return Err("listenNative");
    }
    let pin = tls::pin(&asked.key);
    // The cache first: a key the cache does not hold is never taken out of the book by a list.
    let station_id = &ready.owner.station_id;
    let mut cache = ready
        .vault
        .device_cache()?
        .filter(|cache| cache.station_id == *station_id)
        .unwrap_or_else(|| DeviceCache::new(station_id));
    cache.keep(CachedDevice {
        pin: tempo_client::hex(&pin),
        role: vouched.role,
        guest: vouched.guest,
        issued_at: vouched.issued_at,
        // Not yet heard from the service.
        confirmed_at: 0,
    })?;
    ready.vault.save_device_cache(&cache)?;
    book.add(pin, &asked.name, under)
}

/// Whether a refusal is the computer's own doing (its proof), which counts against its address.
fn counted(reason: &str) -> bool {
    reason.starts_with("token")
        || matches!(
            reason,
            "proofNotYours" | "ownerIssuer" | "deviceKind" | "listenNative"
        )
}

/// One enrol-only connection, from its first message to its close, inside `deadline`.
pub(super) async fn session<S>(
    mut socket: Socket<S>,
    asked: Asked,
    desk: Desk,
    deadline: Duration,
    mut stop: watch::Receiver<bool>,
) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let outcome = tokio::select! {
        biased;
        _ = stop.changed() => Outcome::Turned,
        done = tokio::time::timeout(deadline, enrol(&mut socket, asked, &desk)) => {
            done.unwrap_or(Outcome::Failed)
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    outcome
}

async fn enrol<S>(socket: &mut Socket<S>, asked: Asked, desk: &Desk) -> Outcome
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (book, enrolment, under) = (
        desk.book.clone(),
        desk.enrolment.clone(),
        desk.station_id.clone(),
    );
    // The store can block: off this connection's thread.
    let admitted = tokio::task::spawn_blocking(move || {
        let now_s = enrolment.now_s().ok_or("enrolClosed")?;
        admit(&book, &enrolment, &under, &asked, now_s)
    })
    .await;
    match admitted {
        Ok(Ok(device)) => {
            say(
                socket,
                json!({"type":"enrolled","deviceId":device,"stationId":desk.station_id}),
            )
            .await;
            Outcome::Paired
        }
        Ok(Err(reason)) if counted(reason) => refuse(socket, reason, Outcome::Failed).await,
        Ok(Err(reason)) => refuse(socket, reason, Outcome::Turned).await,
        Err(_) => refuse(socket, "unavailable", Outcome::Turned).await,
    }
}
