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
//! **Its proof's age.** A computer enrolled this way is admitted on the LAN for
//! `tempo_client::account::PROOF_MAX_AGE_S` from its proof's signing, owner's and guest's alike,
//! internet up or down, and then signs in again (it enrols afresh, and its new proof replaces the
//! old). The station hears the service only when it reads its list, so between lists nothing else
//! vouches for it. [`Enrolment::bounded`] holds every admission to that age: TLS, the hello, and the
//! check each session makes at every status tick, so one that passes it mid-session ends; and one
//! past it is taken out of the book at launch and after each list (`super::Lan::expire`). A computer
//! paired by code is not enrolled and is not held to it, and neither is one enrolled before and
//! then paired again with a code at the shack: the pairing takes it out of the device cache
//! ([`Enrolment::paired_by_code`]), so a code pairing at the shack outranks a sign-in's age. It then
//! stays until it is removed at the shack, and a removal on the web no longer reaches it here: the
//! service's list reaches the LAN only through that cache.
//!
//! **One writer at a time.** The device cache has writers here and beside: an enrolment
//! ([`admit`]), an expiry ([`Enrolment::forget`]), a code pairing ([`Enrolment::paired_by_code`]),
//! and each list the station hears (`remote_service::account::sweep`, and the controller keeping a
//! device it vouched for); and the copy in memory follows it ([`Enrolment::reread`]). Each holds
//! the cache (`DevicesHeld`) from its read to its save, so none saves over another's change. An
//! enrolment holds it until its computer is in the book, so no list takes its entry out between the
//! two; that is the one place the book's lock is taken inside it, and nothing holding the book's
//! takes it (an expiry and a code pairing write the book first and let its lock go). An expiry
//! takes out only an entry still past its age once it holds the cache: a computer enrolled again
//! since it was found expired is in the book again, held to its new proof.
//!
//! **The sign-in transmit hold is WAIVED here (the 2026-10-23 sign-in decision).** A computer that signs in over the
//! INTERNET and is auto-granted control carries a 15-minute transmit hold
//! (`super::super::account::GRANTS_ON_SIGN_IN`). A computer enrolling on the station's OWN network
//! is different: it is on the shack's LAN, which stands in for the shack tap the hold is waived by,
//! so [`admit`] records no hold — it never touches `operations::Authority::hold_transmit_until`, and
//! the engine's arm gate holds a LAN-enrolled computer not at all.
//!
//! Nothing here writes a log line, and no proof is kept: the cache holds the key's pin, the role
//! and the proof's signing time.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tempo_client::account::{self, CachedDevice, DeviceCache, Invite, KeySet, Owner, Role, Tenant};
use tokio::sync::watch;

use super::super::vault::{AccountVault, DevicesHeld};
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
/// And the computers enrolled already, which every admission holds to their proofs' age.
#[derive(Default)]
pub(crate) struct Enrolment {
    ready: Mutex<Option<Arc<Ready>>>,
    /// Where the LAN's device cache is kept. `None` only in a test with no account: then nothing is
    /// enrolled, and nothing is held to an age.
    kept: Option<Kept>,
    /// The devices that cache holds, as last read: `None` until it has been read, and then nobody
    /// is admitted, as the book admits nobody until it has read its own.
    enrolled: Mutex<Option<Vec<CachedDevice>>>,
}

/// Where the LAN's device cache is kept, and the clock proofs are read by.
struct Kept {
    vault: Arc<dyn AccountVault>,
    now_s: fn() -> u64,
}

impl Enrolment {
    /// The enrolment for the station whose LAN device cache `vault` keeps, its proofs read by `now_s`.
    pub(crate) fn kept_in(vault: Arc<dyn AccountVault>, now_s: fn() -> u64) -> Self {
        Self {
            kept: Some(Kept { vault, now_s }),
            ..Self::default()
        }
    }

    /// Reads the device cache, if it has not been read yet: before anything listens.
    pub(super) fn load(&self) {
        if self.enrolled.lock().is_ok_and(|held| held.is_none()) {
            self.reread();
        }
    }

    /// Reads the device cache again, after the station changed it. One that does not read leaves
    /// what was read before.
    pub(crate) fn reread(&self) {
        let Some(Kept { vault, .. }) = &self.kept else {
            return;
        };
        self.read(&**vault, &DevicesHeld::take(&**vault));
    }

    /// [`Enrolment::reread`], the cache held: so the copy in memory follows the store in the order
    /// its writers saved it, and never goes back to a read older than a save.
    fn read(&self, vault: &dyn AccountVault, _held: &DevicesHeld<'_>) {
        let read = match vault.device_cache() {
            Ok(cache) => cache.map(|cache| cache.devices).unwrap_or_default(),
            Err(_) => {
                // Never read yet, nobody is admitted (`admits`): the cause of a refusal to say.
                let first = self.enrolled.lock().is_ok_and(|held| held.is_none());
                tempo_core::applog::warn(
                    "remote",
                    if first {
                        "on this network: the signed-in computers' list did not read; until it \
                         does, no paired computer is taken"
                    } else {
                        "on this network: the signed-in computers' list did not read again; the \
                         last one read stays"
                    },
                );
                return;
            }
        };
        self.hold(read);
    }

    fn hold(&self, devices: Vec<CachedDevice>) {
        if let Ok(mut held) = self.enrolled.lock() {
            *held = Some(devices);
        }
    }

    /// Whether the LAN admits the computer holding the key with `pin` now, as far as its enrolment
    /// goes: one the cache does not hold was paired by code, and the book alone decides; one it holds
    /// is admitted for [`account::PROOF_MAX_AGE_S`] from its proof's signing (see the header).
    pub(crate) fn admits(&self, pin: &[u8; 32]) -> bool {
        let Some(Kept { now_s, .. }) = &self.kept else {
            return true;
        };
        let Ok(held) = self.enrolled.lock() else {
            return false;
        };
        held.as_ref().is_some_and(|devices| {
            let pin = tempo_client::hex(pin);
            devices
                .iter()
                .find(|device| device.pin == pin)
                .is_none_or(|device| device.admits(now_s(), false))
        })
    }

    /// The pins of the enrolled computers whose proofs are past their age now, as last read.
    pub(super) fn expired(&self) -> Vec<[u8; 32]> {
        let Some(Kept { now_s, .. }) = &self.kept else {
            return Vec::new();
        };
        let now_s = now_s();
        let Ok(held) = self.enrolled.lock() else {
            return Vec::new();
        };
        held.iter()
            .flatten()
            .filter(|device| !device.admits(now_s, false))
            .filter_map(|device| super::pairing::hex32(&device.pin))
            .collect()
    }

    /// Takes the computers with `pins` out of the device cache, once they are out of the book: each
    /// only while its proof is still past its age, since one enrolled again meanwhile is in the book
    /// again, held to its new proof (see the header). A store that will not keep it leaves them
    /// there, still refused.
    pub(super) fn forget(&self, pins: &[[u8; 32]]) {
        let Some(Kept { vault, now_s }) = &self.kept else {
            return;
        };
        let held = DevicesHeld::take(&**vault);
        if let Ok(Some(mut cache)) = vault.device_cache() {
            let now_s = now_s();
            for pin in pins {
                let pin = tempo_client::hex(pin);
                let within_age = cache.find(&pin).is_some_and(|d| d.admits(now_s, false));
                if !within_age {
                    cache.forget(&pin);
                }
            }
            let _ = vault.save_device_cache(&held, &cache);
        }
        self.read(&**vault, &held);
    }

    /// The computer with `pin` was just paired with a code at the shack, and the book keeps that
    /// pairing: it is taken out of the device cache, so no sign-in's age holds it from now on, as
    /// for every computer paired by code (see the header). A cache that cannot be read, or will not
    /// keep the change, leaves it held to its proof's age: `unavailable`, which the pairing says.
    pub(super) fn paired_by_code(&self, pin: &[u8; 32]) -> Result<(), &'static str> {
        let Some(Kept { vault, .. }) = &self.kept else {
            return Ok(());
        };
        let pin = tempo_client::hex(pin);
        let held = DevicesHeld::take(&**vault);
        let taken = vault.device_cache().and_then(|cache| match cache {
            Some(mut cache) if cache.find(&pin).is_some() => {
                cache.forget(&pin);
                vault.save_device_cache(&held, &cache)
            }
            _ => Ok(()),
        });
        self.read(&**vault, &held);
        taken.map_err(|_| "unavailable")
    }

    /// `paired` (who the book admits), held to [`Enrolment::admits`]: what the LAN's TLS and its
    /// sessions ask.
    pub(super) fn bounded(self: &Arc<Self>, paired: tls::Paired) -> tls::Paired {
        let enrolment = self.clone();
        Arc::new(move |pin: &[u8; 32]| paired(pin).filter(|_| enrolment.admits(pin)))
    }

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

    /// The people it takes, while armed: for a test to read.
    #[cfg(test)]
    pub(crate) fn invites(&self) -> Option<Vec<Invite>> {
        self.ready().map(|ready| ready.invites.clone())
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
/// checked (see the header), its key kept in the device cache and then paired in the book, the
/// cache held from its read until the book keeps the pairing. Its device id. Blocks on the
/// credential store.
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
    // The cache first: a key the cache does not hold is never taken out of the book by a list. Held
    // until the book keeps the pairing, so no list's sweep lands between the two.
    let station_id = &ready.owner.station_id;
    let held = DevicesHeld::take(&*ready.vault);
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
    ready.vault.save_device_cache(&held, &cache)?;
    enrolment.hold(cache.devices);
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
