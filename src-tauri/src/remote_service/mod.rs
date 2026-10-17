//! Local approval and the Remote background task. Engine access is restricted to
//! bounded observation, the stream, the lease and Stop, and the separately approved FT gestures.
/// Receive audio for a listening browser. Gated with the radio feature because the
/// encoder lives in tempo-audio, which a build without it does not have at all.
#[cfg(feature = "radio")]
mod audio;
/// Remote over this network: the shack's own listener, for a paired computer on the same network.
pub(crate) mod lan;
/// The relay's older lanes, held to the browser's own key (security review S1-M1).
mod lanes;
#[cfg(test)]
pub(crate) mod log_tests;
pub(crate) mod operations;
/// The shack PC's two identifiers, hashed, sent when approving a station may start a trial.
mod pc_identity;
/// The station's own signing key, which signs its stream answers (security review S3-M1).
mod station_key;
#[cfg(test)]
pub(crate) mod stored_log_tests;
/// Remote as a stream: the `streamSignal` lane and the session thread (see its header).
pub(crate) mod stream;
#[cfg(test)]
mod tests;
mod transport;
/// The tunnel through the room: Nexus on another computer driving this station over the internet.
mod tunnel;
pub(crate) mod vault;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};
/// The one Remote origin: the service this station talks to, and the page the Remote stations
/// window shows (`remote_window`).
pub(crate) use transport::REMOTE_ORIGIN;
use transport::{credential, empty, identifier, station_path, Client};
use vault::{Binding, SystemVault, Vault};

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    phase: String,
    origin: String,
    station_id: Option<String>,
    account_id: Option<String>,
    pairing_id: Option<String>,
    pairing_code: Option<String>,
    expires_at: Option<u64>,
    devices: Vec<Device>,
    error: Option<&'static str>,
    observation_generation: Option<String>,
    logging_permissions: Vec<String>,
    station_permissions: Vec<String>,
    transmit_permissions: Vec<String>,
    logging_controller: Option<String>,
    /// A5: the browsers whose listed device key is the one pinned at the radio, the ones that can
    /// stream. An approved browser listed with a key and missing here is approved again to stream.
    pinned_devices: Vec<String>,
    /// S3-L1: SHA-256 of this station's own signing key, lowercase hex, worked out as a page works out
    /// the key the service lists: shown as "This station's key" for the operator to compare with the
    /// key the page kept. Its public half only. `None` while there is no pairing or no key.
    station_key: Option<String>,
    /// S3-M1: the service holds another signing key for this station, so browsers refuse its stream
    /// until it is paired again. Its own field, because it lasts past any one request's error.
    key_refused: bool,
    /// Remote over this network (`lan`): its switch, its paired computers and its pairing window.
    /// Absent only from a build of the service without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    lan: Option<lan::LanStatus>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Device {
    id: String,
    name: String,
    approved: u8,
    expires_at: u64,
    /// The approval generation. Listed only by a service with the browser approval lifetime, and only
    /// to a Nexus that asks (`transport::Client::post` does).
    #[serde(default)]
    generation: Option<u64>,
    /// The end that use cannot move, ninety days after the shack approved it. None for an approval that
    /// never renews.
    #[serde(default)]
    renews_until: Option<u64>,
    /// The browser's device key (A5): its SPKI as lowercase hex. Listed only to a Nexus that asks
    /// (`transport::Client::post` does), and never shown: the desktop is given `key`.
    #[serde(default, skip_serializing)]
    public_key: Option<String>,
    /// SHA-256 of that key as lowercase hex: shown, shortened, beside the browser's name, and what
    /// approving it pins. Worked out here from the listed key, never taken from the service.
    #[serde(skip_deserializing)]
    key: Option<String>,
    /// What it is. Listed only to a Nexus that asks (`transport::Client::post` does); a service
    /// without kinds lists browsers alone, so a device it names no kind for is a browser.
    #[serde(default)]
    kind: Kind,
    /// The platform a native device runs on, one its kind may name ([`Kind::fits`]); none for a
    /// browser.
    #[serde(default)]
    platform: Option<Platform>,
    /// 1 for an approval that lasts until it is removed (the operator's decision of 2026-10-10: every
    /// approval since), whose `expires_at` is then an end no clock reaches. Listed only to a Nexus that
    /// asks for kinds; 0 from a service before that rule, whose approvals keep their listed end. Shown,
    /// never decided on: every list is still held to `expires_at`.
    #[serde(default)]
    until_removed: u8,
}
/// A device's kind, held to the service's own values: a browser, or Nexus itself on another
/// computer or a phone (the native client). Approving a native device grants nothing on the
/// relay's lanes: it pins its key, which the tunnel through the room checks, and that is all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Browser,
    Computer,
    Phone,
}
/// The platforms a native device may name, held to the service's own values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Platform {
    Windows,
    Macos,
    Linux,
    Ios,
    Android,
}
impl Kind {
    /// May a device of this kind name this platform? A browser names none, a computer Windows,
    /// macOS or Linux, and a phone iOS or Android, as the service registers them.
    fn fits(self, platform: Option<Platform>) -> bool {
        matches!(
            (self, platform),
            (Kind::Browser, None)
                | (
                    Kind::Computer,
                    Some(Platform::Windows | Platform::Macos | Platform::Linux)
                )
                | (Kind::Phone, Some(Platform::Ios | Platform::Android))
        )
    }
}
/// The station's key (S3-M1), and the same key as TLS presents it for the room's tunnel: both, or
/// neither, but for a key TLS would not take, which then signs answers alone.
type StationKeys = (
    Option<Arc<station_key::Signer>>,
    Option<Arc<lan::tls::Identity>>,
);
/// The station's key as the room's tunnel proves it, for the pairing's station.
fn tunnel_identity(pkcs8: &str, binding: &Binding) -> Option<Arc<lan::tls::Identity>> {
    lan::tls::Identity::new(pkcs8, binding.station_id.clone()).map(Arc::new)
}
/// SHA-256 of a device key's SPKI, lowercase hex in and out: its pin (A5). The station's own key is
/// shown the same way (S3-L1). `None` for anything that is not the shape of a P-256 key.
fn fingerprint(public_key: &str) -> Option<String> {
    if !tempo_stream::protocol::device_key(public_key) {
        return None;
    }
    let spki = tempo_stream::protocol::hex_bytes(public_key)?;
    Some(
        ring::digest::digest(&ring::digest::SHA256, &spki)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// A pin as stored: 32 bytes of SHA-256 as lowercase hex.
fn pin_shape(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
/// What a computer's (or phone's) pin begins with.
const NATIVE_PIN_LABEL: &[u8] = b"nexus-native-pin/1";
/// The pin approving a computer or a phone makes: SHA-256 over [`NATIVE_PIN_LABEL`], the approval's
/// generation and its key's fingerprint (`key`, lowercase hex), as lowercase hex. Never a browser's
/// pin, which is the fingerprint itself, so a browser's approval admits nothing through the room's
/// tunnel and a computer's admits nothing on the relay's lanes; and never another approval's, so a
/// computer the service lists as approved again, under an approval the shack did not give, is not
/// taken on the pin made for the one before. A renewal on use keeps the generation, and the pin.
fn native_pin(key: &str, generation: u64) -> Option<String> {
    let fingerprint = tempo_stream::protocol::hex_bytes(key).filter(|f| f.len() == 32)?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    hash.update(NATIVE_PIN_LABEL);
    hash.update(&generation.to_be_bytes());
    hash.update(&fingerprint);
    Some(
        hash.finish()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// An id the service made for a device: the shape every id has, at version 4 (the service makes them
/// with `crypto.randomUUID`). The shack's own names for computers are version 8 (`lan::device_id`,
/// `lan::tunnel_device_id`), so no device the service lists can stand for one of them in the
/// authority they share, or take its grants.
fn hosted(id: &str) -> bool {
    identifier(id) && id.as_bytes()[14] == b'4'
}
impl Device {
    fn approval(&self) -> Approval {
        Approval {
            expires_at: self.expires_at,
            generation: self.generation,
        }
    }
    /// The pin approving it as it is listed makes: a browser's key's fingerprint, or a computer's or
    /// phone's [`native_pin`]. `None` with no key, and for a computer or phone with no approval
    /// generation, which pins nothing.
    fn pin(&self) -> Option<String> {
        let key = self.key.as_deref()?;
        match self.kind {
            Kind::Browser => Some(key.to_string()),
            Kind::Computer | Kind::Phone => native_pin(key, self.generation?),
        }
    }
}
/// One browser approval as the service lists it.
#[derive(Clone, Copy, PartialEq)]
struct Approval {
    expires_at: u64,
    generation: Option<u64>,
}
impl Approval {
    /// Was `grant` given against this approval? By generation when both sides know it: a renewal on
    /// use moves the expiry and keeps the generation, while approving again or revoking moves it.
    /// Otherwise by expiry, which every approval and revocation rewrites: a record 1.12.0 wrote, or a
    /// service without the approval lifetime, which never renews.
    fn holds(&self, grant: &vault::Grant) -> bool {
        match (grant.generation, self.generation) {
            (Some(held), Some(listed)) => held == listed,
            _ => grant.expires_at == self.expires_at,
        }
    }
    /// Rebind a grant this approval holds to the approval as listed now, so a record written before
    /// the service reported a generation carries one from here on. Grants nothing.
    fn bind(&self, grant: &mut vault::Grant) {
        if self.generation.is_some() {
            grant.generation = self.generation;
            grant.expires_at = self.expires_at;
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
// Empty struct variants preserve deny_unknown_fields; Serde unit variants ignore extra fields.
pub enum Action {
    LoggingPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    StationPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    TransmitPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    TakeOverLogging {},
    Begin {
        name: String,
    },
    Refresh {},
    Cancel {},
    Approve {
        #[serde(rename = "enrollmentId")]
        enrollment_id: String,
        #[serde(rename = "accountId")]
        account_id: String,
        /// "Also allow FT8/FT4 transmit" for the browser that did the pairing.
        #[serde(default)]
        transmit: bool,
    },
    Enable {},
    Disable {},
    Forget {},
    Device {
        #[serde(rename = "deviceId")]
        device_id: String,
        approve: bool,
        /// "Also allow FT8/FT4 transmit" on this approval. Ignored when revoking.
        #[serde(default)]
        transmit: bool,
        /// A5: the device key the operator was shown beside this browser (its SHA-256, the device
        /// list's `key`). Pinned only if the service still lists that key once it has approved.
        #[serde(default)]
        key: Option<String>,
        /// The kind the operator was shown it as: a browser, a computer or a phone. Approving grants
        /// and pins only for a device the service still lists as that kind once it has approved; with
        /// none shown, it grants and pins nothing. Ignored when revoking.
        #[serde(default)]
        kind: Option<Kind>,
    },
    /// Remote over this network on, at the shack (`lan`): on `address`, this computer's private
    /// IPv4 address the operator picked, or with none as `lan` chooses; on `port`, or as it was.
    LanOn {
        #[serde(default)]
        address: Option<String>,
        #[serde(default)]
        port: Option<u16>,
    },
    /// Remote over this network off, at the shack: the port closes and every LAN session ends.
    LanOff {},
    /// Where Remote over this network listens, picked at the shack: one of this computer's private
    /// IPv4 addresses, or with none as `lan` chooses. Kept, on or off.
    LanAddress {
        #[serde(default)]
        address: Option<String>,
    },
    /// Pair a computer over this network, at the shack: the pairing window opens with a new code,
    /// and this press is the approval (as ruled on 2026-10-04, "One press").
    LanPair {},
    /// Close the pairing window early.
    LanCancel {},
    /// Remove a paired computer, at the shack: out at once, its station control and its session
    /// with it.
    LanRevoke {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    /// Reset this station's network identity, at the shack: a new LAN key, and every paired
    /// computer removed.
    LanReset {},
}
type Reply = oneshot::Sender<Result<(), &'static str>>;
/// The most browsers a restart remembers grants for. The service lists at most eight per station,
/// and the record has to fit the smallest OS credential blob (Windows).
const MAX_REMEMBERED: usize = 8;
/// A decision a restart must remember. Operator decisions 2026-09-13: Remote remembers being on;
/// approving a browser grants it station control and logging, plus FT8/FT4 transmit when the
/// approval ticks it, and each is kept for as long as that approval stands, until revoked.
///
/// Every decision is applied to `Control::remembered` under the `Control` lock at the moment it is
/// made, so the record always holds the operator's decisions in the order they were made, and Turn
/// on reads exactly what has been decided. Writing it happens on the vault writer's own thread,
/// never under a lock and never on the path that revokes transmission, because an OS credential
/// store can block (a locked keychain can prompt).
enum Persist {
    /// The controller loaded or created this pairing; later decisions apply to it.
    Bound(Binding, Option<vault::State>, Option<Box<vault::Pins>>),
    /// Remote was turned on and is connecting.
    Enabled,
    /// Remote went off for a reason that ends access: Revoke station access, a cancelled pairing,
    /// or the connection refusing itself. The permissions it cleared are remembered as cleared.
    Off,
    /// Turn off Remote. It only pauses (operator decision 2026-09-14): every browser is
    /// disconnected and every live permission cleared, but the remembered grants stay, so Turn on
    /// or a restart restores them for browsers still approved at the same approval.
    Paused,
    /// A local decision cleared every browser permission: a browser's removal that could not be
    /// applied to that browser alone (see `Control::revoke_browser`).
    ClearGrants,
    /// One browser's grants changed. `approval` is the approval the change was made against: a grant
    /// is only ever created against a known approval, and a grant against a different approval (not
    /// merely a renewed one) starts that browser afresh. A `None` field is left as it was.
    Browser {
        device_id: String,
        approval: Option<Approval>,
        logging: Option<bool>,
        control: Option<bool>,
        transmit: Option<bool>,
    },
    /// The approvals the service lists right now. A grant remembered against any other approval
    /// (a browser revoked, re-approved, expired or gone) is forgotten; the rest are rebound to the
    /// approval as listed. So is a pin for a browser no longer approved.
    Approvals(Vec<(String, Approval)>),
    /// The pairing was removed.
    Removed,
    /// A5: the operator approved this browser at the radio with its device key shown (`Some`: the
    /// key's SHA-256, now pinned; for a computer or phone, its [`native_pin`]), or the approval
    /// pinned nothing, or the browser was revoked (`None`: no pin).
    Pin {
        device_id: String,
        key: Option<String>,
    },
    /// The browsers the service lists as approved right now: a pin for any other is forgotten.
    Listed(Vec<String>),
}
/// What the vault must hold next. Each write is a whole record, so only the latest of each matters.
enum Write {
    Save(vault::State),
    Remove,
    /// The pins' own entry (A5).
    SavePins(vault::Pins),
    RemovePins,
}
/// The record for the current pairing, kept in step with every decision.
#[derive(Default)]
struct Remembered {
    binding: Option<Binding>,
    state: Option<vault::State>,
    /// A5: browser id to the SHA-256 of the device key the operator pinned for it (a computer's or
    /// phone's: its [`native_pin`]), kept in the vault's pins entry. A pin is an approval's, not a
    /// permission's: the switches, take over and Turn off leave it; approving at the radio sets it
    /// and revoking the browser removes it.
    pins: BTreeMap<String, String>,
}
impl Remembered {
    /// Apply one decision. Returns the writes it calls for: at most one for each entry, and none
    /// when nothing changed.
    fn apply(&mut self, decision: Persist) -> Vec<Write> {
        let pins = self.apply_pins(&decision);
        self.apply_state(decision).into_iter().chain(pins).collect()
    }
    fn apply_pins(&mut self, decision: &Persist) -> Option<Write> {
        let before = self.pins.clone();
        match decision {
            Persist::Bound(bound, _, pins) => {
                self.pins = pins
                    .as_ref()
                    .filter(|p| p.binding == *bound)
                    .map(|p| p.keys.clone())
                    .unwrap_or_default();
                return None;
            }
            Persist::Removed => {
                self.pins.clear();
                return Some(Write::RemovePins);
            }
            Persist::Off => self.pins.clear(),
            Persist::Pin { device_id, key } => match key {
                Some(key) => {
                    self.pins.insert(device_id.clone(), key.clone());
                }
                None => {
                    self.pins.remove(device_id);
                }
            },
            Persist::Listed(approved) => self.pins.retain(|id, _| approved.contains(id)),
            Persist::Approvals(approvals) => self
                .pins
                .retain(|id, _| approvals.iter().any(|(listed, _)| listed == id)),
            _ => return None,
        }
        if self.pins == before {
            return None;
        }
        let binding = self.binding.clone()?;
        Some(if self.pins.is_empty() {
            Write::RemovePins
        } else {
            Write::SavePins(vault::Pins {
                binding,
                keys: self.pins.clone(),
            })
        })
    }
    fn apply_state(&mut self, decision: Persist) -> Option<Write> {
        match decision {
            Persist::Bound(bound, state, _) => {
                self.state = state.filter(|s| s.binding == bound);
                self.binding = Some(bound);
                return None;
            }
            Persist::Removed => {
                self.binding = None;
                self.state = None;
                return Some(Write::Remove);
            }
            Persist::Pin { .. } | Persist::Listed(_) => return None,
            _ => {}
        }
        let bound = self.binding.clone()?;
        let mut state = self.state.clone().unwrap_or(vault::State {
            binding: bound,
            enabled: false,
            grants: Vec::new(),
        });
        match decision {
            Persist::Enabled => state.enabled = true,
            Persist::Off => {
                state.enabled = false;
                state.grants.clear();
            }
            Persist::Paused => state.enabled = false,
            Persist::ClearGrants => state.grants.clear(),
            Persist::Browser {
                device_id,
                approval,
                logging,
                control,
                transmit,
            } => {
                let at = state.grants.iter().position(|g| g.device_id == device_id);
                let mut grant = match (at.map(|i| state.grants[i].clone()), approval) {
                    (Some(mut grant), Some(approval)) if approval.holds(&grant) => {
                        approval.bind(&mut grant);
                        grant
                    }
                    (_, Some(approval)) => vault::Grant {
                        device_id,
                        expires_at: approval.expires_at,
                        generation: approval.generation,
                        logging: false,
                        control: false,
                        transmit: false,
                    },
                    (Some(grant), None) => grant,
                    // Nothing remembered for this browser, and no approval to bind a grant to.
                    (None, None) => return None,
                };
                grant.logging = logging.unwrap_or(grant.logging);
                grant.control = control.unwrap_or(grant.control);
                // FT8/FT4 transmit is only ever held alongside station control.
                grant.transmit = transmit.unwrap_or(grant.transmit) && grant.control;
                let keep = grant.logging || grant.control;
                match at {
                    Some(i) if keep => state.grants[i] = grant,
                    Some(i) => {
                        state.grants.remove(i);
                    }
                    None if keep && state.grants.len() < MAX_REMEMBERED => state.grants.push(grant),
                    None => {}
                }
            }
            Persist::Approvals(approvals) => state.grants.retain_mut(|g| {
                let listed = approvals
                    .iter()
                    .find(|(id, approval)| *id == g.device_id && approval.holds(g));
                if let Some((_, approval)) = listed {
                    approval.bind(g);
                }
                listed.is_some()
            }),
            Persist::Bound(..) | Persist::Removed | Persist::Pin { .. } | Persist::Listed(_) => {
                return None
            }
        }
        if self.state.as_ref() == Some(&state) {
            return None;
        }
        self.state = Some(state.clone());
        Some(Write::Save(state))
    }
    fn grants(&self) -> &[vault::Grant] {
        self.state.as_ref().map_or(&[], |s| s.grants.as_slice())
    }
    /// The device key pinned for this browser (A5), as the SHA-256 a stream offer's key must have.
    fn pinned(&self, device_id: &str) -> Option<[u8; 32]> {
        tempo_stream::protocol::hex_bytes(self.pins.get(device_id)?)?
            .try_into()
            .ok()
    }
}
#[derive(Default)]
struct Control {
    operations: Arc<operations::Authority>,
    enabled: bool,
    cancel: Option<watch::Sender<bool>>,
    generation: u64,
    vault_writer: Option<mpsc::UnboundedSender<Write>>,
    remembered: Remembered,
    /// This pairing's signing key (S3-M1), read from the vault or made there once. `None` while there
    /// is no pairing, or when the credential store would not give one up: the station's answers then
    /// go unsigned, and pages refuse them.
    station_key: Option<Arc<station_key::Signer>>,
    /// The service holds another key for this station (`stationKeyPinned`): pages will refuse its
    /// answers until it is paired again. Said at the shack.
    key_refused: bool,
    /// Remote over this network (`lan`), beside the relay's road and sharing its authority.
    /// Taken out of the lock to be used. `None` in a test build of the service.
    lan: Option<Arc<lan::Lan>>,
    /// The same key as `station_key`, as TLS presents it: what the room's tunnel proves itself
    /// with. Made from the same vault entry at the same moments.
    station_identity: Option<Arc<lan::tls::Identity>>,
    /// The computers approved here for the room's tunnel, kept in step with the pins and the
    /// service's list ([`Control::sync_approved`]).
    approved: Arc<tunnel::Approved>,
    /// The computers (and phones) the service lists as approved and not yet ended, by device id:
    /// what a pin is compared with.
    natives: BTreeMap<String, Native>,
    /// What the room's tunnel serves its computers with: the LAN road's authority, feeds and
    /// command lane. `None` in a build of the service without the LAN road.
    tunnel: Option<lan::TunnelDeps>,
    /// Hosted Remote is on: `enabled`, mirrored where the operations authority can read it under its
    /// own lock, for the tunnel's computers' station control (`Authority::hold_tunnel_devices`).
    /// Cleared before every stop's invalidation, set only by [`Control::start`].
    hosted: Arc<std::sync::atomic::AtomicBool>,
}
impl Control {
    fn stop(&mut self) {
        // Before the invalidation, so the clear it makes is never undone by the tunnel's holder.
        self.hosted
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.operations.invalidate();
        self.enabled = false;
        self.generation = self.generation.wrapping_add(1);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }
    /// Turn hosted Remote on: a fresh authority (every permission in memory cleared), then on. The
    /// tunnel's approved computers hold station control again from here.
    fn start(&mut self) {
        self.stop();
        self.enabled = true;
        self.hosted.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Removing one device's approval at the shack ends that device's grants at once (a lease it
    /// holds with them), remembered and live, and its pin; every other device keeps its own
    /// (operator decision 2026-10-14). A revoke never waits for Core. Should one not be taken, the
    /// whole authority is cleared instead, as it was before: fail closed.
    fn revoke_browser(&mut self, device_id: &str) {
        let operations = self.operations.clone();
        let taken = operations.permit(device_id, false).is_ok()
            && operations.permit_station(device_id, false).is_ok()
            && operations.permit_transmit(device_id, false).is_ok();
        if taken {
            self.remember(Persist::Browser {
                device_id: device_id.to_string(),
                approval: None,
                logging: Some(false),
                control: Some(false),
                transmit: Some(false),
            });
        } else {
            operations.invalidate();
            self.remember(Persist::ClearGrants);
        }
        // A5: revoking ends the approval, and its pin with it.
        self.remember(Persist::Pin {
            device_id: device_id.to_string(),
            key: None,
        });
    }
    /// Hand a decision to the vault writer. Never called from `stop`: `stop` also runs when Nexus
    /// exits, and an exit must not be remembered as the operator turning Remote off.
    fn remember(&mut self, decision: Persist) {
        for write in self.remembered.apply(decision) {
            if let Some(writer) = &self.vault_writer {
                let _ = writer.send(write);
            }
        }
        self.sync_approved();
    }
    /// The computers the service lists as approved and not yet ended (`devices`): who the room's
    /// tunnel takes, with the pins.
    fn listed(&mut self, devices: &[Device]) {
        let now = now_ms();
        self.natives = devices
            .iter()
            .filter(|d| d.kind != Kind::Browser && d.approved == 1 && d.expires_at > now)
            .filter_map(|d| {
                let native = Native {
                    pin: d.pin()?,
                    key: d.key.clone()?,
                    until: d.expires_at,
                };
                Some((d.id.clone(), native))
            })
            .collect();
        self.sync_approved();
    }
    /// Who the room's tunnel takes, until when: each computer the service lists as approved whose
    /// pin here is the one approving it as listed makes ([`Device::pin`]: its key, under this same
    /// approval, approved as a computer). Never a browser, whatever its pin. Each is named from the
    /// key pinned for it (`lan::tunnel_device_id`), never by the service's id, so it can stand for
    /// no other device in the authority it shares with the relay's road and the LAN's.
    fn sync_approved(&self) {
        let keys = self
            .natives
            .iter()
            .filter(|(id, native)| self.remembered.pins.get(*id) == Some(&native.pin))
            .filter_map(|(_, native)| {
                let pin: [u8; 32] = tempo_stream::protocol::hex_bytes(&native.key)?
                    .try_into()
                    .ok()?;
                Some((pin, (lan::tunnel_device_id(&pin), native.until)))
            })
            .collect();
        // A computer no longer on it loses station control at once, after it is off the holder.
        for name in self.approved.set(keys) {
            let _ = self.operations.permit_station(&name, false);
        }
    }
}
/// A computer (or phone) the service lists as approved: the pin approving it as listed makes, the
/// fingerprint of the key listed for it, and when its approval ends.
struct Native {
    pin: String,
    key: String,
    until: u64,
}
/// A command, the Remote generation it was issued under, the authority epoch at that moment (so a
/// local decision made after it wins), and where to reply.
type Command = (Action, u64, u64, Reply);
pub struct Service {
    commands: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
}
// A superseded connection may finish an I/O operation after a newer local
// decision. Its status update must not undo that decision in the desktop UI.
#[derive(Clone)]
struct SessionStatus {
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    generation: u64,
}
impl SessionStatus {
    fn set(&self, value: &str, error: Option<&'static str>) {
        if let Ok(mut control) = self.control.lock() {
            if !control.enabled || control.generation != self.generation {
                return;
            }
            if value == "disabled" {
                control.stop();
                // The service refused this station (access denied) or its credential is unusable.
                // Remote is off now, so the next launch must not turn it back on. A malformed
                // service reply is deliberately NOT this: it reconnects (see `supervise`), because
                // an off that is remembered can only be undone at the shack.
                control.remember(Persist::Off);
            }
            phase(&self.status, value, error);
        }
    }
}
pub(super) fn phase(status: &Arc<Mutex<Status>>, value: &str, error: Option<&'static str>) {
    if let Ok(mut status) = status.lock() {
        status.phase = value.into();
        status.error = error;
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}
impl Service {
    pub fn new(
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        // `audio`: the station's bounded receive-audio copy, for a listening browser.
        // `None` leaves the audio lane unadvertised, so nothing is ever offered it.
        #[cfg(feature = "radio")] audio: Option<Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
        // What a streamed session needs from the application: the window its input goes to.
        stream: stream::Host,
        // How a paired computer's calls run at the shack, and the events that cross to it.
        commands: lan::commands::Commands,
    ) -> Self {
        tempo_app::engine::engine_lock(&engine)
            .configure_remote_settings_store(crate::settings_path());
        let feeds = transport::Feeds {
            monitor: publisher,
            // One encoder for the station, whoever listens: the Listen lane and every stream.
            #[cfg(feature = "radio")]
            audio: audio.map(audio::ReceiveFanout::new),
            stream,
        };
        let service = Self::start(
            REMOTE_ORIGIN.to_string(),
            Box::new(SystemVault),
            engine.clone(),
            feeds.clone(),
        );
        // Its LAN key and paired computers are in the OS credential store, read on its own thread.
        service.with_lan(
            crate::settings_path().with_file_name("remote-lan.json"),
            engine,
            feeds,
            Arc::new(lan::Book::new(Arc::new(SystemVault))),
            Arc::new(tempo_stream::lan::look),
            commands,
        )
    }
    /// Remote over this network, beside the relay's road and sharing its one authority: its switch
    /// kept at `path`, its key and paired computers in `book`, and a paired computer's calls run
    /// with `commands`.
    fn with_lan(
        self,
        path: std::path::PathBuf,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
        book: Arc<lan::Book>,
        resolve: lan::Resolve,
        commands: lan::commands::Commands,
    ) -> Self {
        let authority = self.control.lock().ok().map(|c| c.operations.clone());
        if let Some(authority) = authority {
            let dispatcher =
                crate::command_dispatch::Dispatcher::new(crate::command_dispatch::ShackStores {
                    engine: engine.clone(),
                });
            // The room's tunnel serves its computers with the same authority, feeds and command
            // lane: one lease for the station, and one revision of each store, whichever road a
            // call comes by. Its computers are named by their own keys (`Control::sync_approved`).
            let tunnel = lan::TunnelDeps {
                authority: authority.clone(),
                engine: engine.clone(),
                feeds: feeds.clone(),
                dispatcher: dispatcher.clone(),
                commands: commands.clone(),
            };
            let lan = Arc::new(lan::Lan::start(
                path,
                lan::Deps {
                    authority,
                    engine,
                    feeds,
                    book,
                    resolve,
                    advertise: lan::advertise(),
                    firewall: lan::firewall_says(),
                    dispatcher,
                    commands,
                },
            ));
            if let Ok(mut control) = self.control.lock() {
                control.lan = Some(lan);
                control.tunnel = Some(tunnel);
            }
        }
        self
    }
    #[cfg(test)]
    fn configured(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
    ) -> Self {
        Self::start(
            origin,
            vault,
            engine,
            transport::Feeds {
                monitor: publisher,
                #[cfg(feature = "radio")]
                audio: None,
                stream: stream::Host::default(),
            },
        )
    }
    /// `feeds` arrives whole rather than as four parameters. They were four until the audio
    /// lane made it five, and a five-feed argument list is the shape that gets called with two
    /// of them transposed — they already travel together into the controller.
    fn start(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
    ) -> Self {
        let vault: Arc<dyn Vault> = Arc::from(vault);
        let status = Arc::new(Mutex::new(Status {
            phase: "unpaired".into(),
            origin: origin.clone(),
            ..Default::default()
        }));
        // The vault writer runs on a thread of its own, so remembering a decision never waits on
        // the OS credential store while a lock is held or while the controller is mid-request. If
        // it cannot start, nothing is remembered, and the next launch leaves Remote off.
        let (writer, decisions) = mpsc::unbounded_channel();
        let writer_vault = vault.clone();
        let vault_writer = std::thread::Builder::new()
            .name("nexus-remote-vault".into())
            .spawn(move || write_remembered(writer_vault, decisions))
            .ok()
            .map(|_| writer);
        let operations = Arc::new(operations::Authority::default());
        // A stand-down at the shack retires remote transmit authority. Handing the engine this
        // revocation is what makes `Engine::halt_tx` — the verb every local stop funnels through,
        // including WSJT-X's UDP HaltTx — move the generation a browser carries as its
        // `transmitEpoch`, so a gesture held across the operator's own Stop TX is refused instead
        // of arming the rig a second later. See `Engine::halt_tx`.
        tempo_app::engine::engine_lock(&engine)
            .set_remote_transmit_revocation(operations.transmit_revocation());
        // A streamed operator's held PTT: the stream records the holds, the engine arms and
        // releases the microphone over on its own radio-loop tick (`engine/remote_stream.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_ptt_hold(feeds.stream.ptt.clone());
        // …and the page's microphone, which the stream decodes and the engine's microphone over
        // plays, only while that press has armed it (`engine/remote_mic.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_mic_feed(feeds.stream.mic.clone());
        let control = Arc::new(Mutex::new(Control {
            operations,
            vault_writer,
            ..Default::default()
        }));
        // The tunnel's approved computers hold station control while hosted Remote is on, as the
        // LAN's paired computers do while LAN is on: read from the tunnel's roster and the mirror of
        // "on", each behind a lock of its own (never Control's, which is held while Core is taken).
        if let Ok(control) = control.lock() {
            let (approved, hosted) = (control.approved.clone(), control.hosted.clone());
            control.operations.hold_tunnel_devices(Box::new(move || {
                if hosted.load(std::sync::atomic::Ordering::SeqCst) {
                    approved.names()
                } else {
                    Vec::new()
                }
            }));
        }
        let (commands, receiver) = mpsc::channel(1);
        let task_status = status.clone();
        let task_control = control.clone();
        let thread = std::thread::Builder::new()
            .name("nexus-remote".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let client = Client::new(&origin);
                match (runtime, client) {
                    (Ok(runtime), Ok(client)) => runtime.block_on(
                        Controller {
                            client,
                            vault,
                            binding: None,
                            pending: None,
                            status: task_status,
                            control: task_control,
                            engine,
                            feeds,
                            restore: None,
                        }
                        .run(receiver),
                    ),
                    _ => phase(&task_status, "unpaired", Some("serviceUnavailable")),
                }
            });
        if thread.is_err() {
            phase(&status, "unpaired", Some("serviceUnavailable"));
        }
        Self {
            commands,
            status,
            control,
        }
    }
    pub fn status(&self) -> Result<Status, &'static str> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .clone();
        let control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        let enabled = control.enabled;
        let operations = control.operations.local_status();
        status.logging_permissions =
            serde_json::from_value(operations["devices"].clone()).unwrap_or_default();
        status.logging_controller = operations["controller"].as_str().map(str::to_string);
        status.station_permissions =
            serde_json::from_value(operations["controlDevices"].clone()).unwrap_or_default();
        status.transmit_permissions =
            serde_json::from_value(operations["transmitDevices"].clone()).unwrap_or_default();
        status.pinned_devices = status
            .devices
            .iter()
            .filter(|d| {
                d.pin()
                    .is_some_and(|pin| control.remembered.pins.get(&d.id) == Some(&pin))
            })
            .map(|d| d.id.clone())
            .collect();
        status.station_key = control
            .station_key
            .as_ref()
            .and_then(|key| fingerprint(key.public_key()));
        status.observation_generation = enabled.then(|| control.generation.to_string());
        status.key_refused = control.key_refused;
        status.lan = control.lan.as_deref().map(lan::Lan::status);
        if !enabled && ["connected", "connecting", "reconnecting"].contains(&status.phase.as_str())
        {
            status.phase = "disabled".into();
        }
        Ok(status)
    }
    /// Remote over this network, out of the lock.
    fn lan(&self) -> Result<Option<Arc<lan::Lan>>, &'static str> {
        Ok(self
            .control
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .lan
            .clone())
    }
    pub async fn action(&self, action: Action) -> Result<Status, &'static str> {
        match &action {
            Action::LoggingPermission { device_id, allow }
            | Action::StationPermission { device_id, allow }
            | Action::TransmitPermission { device_id, allow } => {
                // Keep admission and grant under the same local control lock
                // as Disable/Take over. A prior status snapshot must not install
                // a new grant after that local authority has been stopped.
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                // The approval being granted against. A restart checks it (by generation where the
                // service reports one) before giving the grant back, so a revoked or re-approved
                // browser gets nothing, and a renewed one keeps what it had.
                let mut approval = None;
                if *allow {
                    let status = self.status.lock().map_err(|_| "serviceUnavailable")?;
                    // A browser's alone: a computer holds nothing on the relay's lanes to allow.
                    let Some(device) = status
                        .devices
                        .iter()
                        .find(|d| d.id == *device_id && d.approved == 1 && d.kind == Kind::Browser)
                        .filter(|_| control.enabled)
                    else {
                        return Err("accessDenied");
                    };
                    approval = Some(device.approval());
                }
                // These are the restrict switches: each allows or revokes one permission for one
                // browser, live first, and the record follows only what took effect. Revoking
                // station control revokes transmit with it.
                let operations = control.operations.clone();
                let (logging, station, transmit) = match &action {
                    Action::StationPermission { .. } => {
                        operations.permit_station(device_id, *allow)?;
                        (None, Some(*allow), (!*allow).then_some(false))
                    }
                    Action::TransmitPermission { .. } => {
                        operations.permit_transmit(device_id, *allow)?;
                        (None, None, Some(*allow))
                    }
                    _ => {
                        operations.permit(device_id, *allow)?;
                        (Some(*allow), None, None)
                    }
                };
                control.remember(Persist::Browser {
                    device_id: device_id.clone(),
                    approval,
                    logging,
                    control: station,
                    transmit,
                });
                drop(control);
                return self.status();
            }
            Action::TakeOverLogging {} => {
                // "End remote control" (the operator's decision of 2026-10-10, one rule for every
                // device): it drops everyone and holds until Remote is turned on again. Hosted Remote
                // goes off as Turn off takes it, here and now, never behind the command queue: every
                // permission in memory cleared, the station's socket closed and with it every browser
                // and every tunnel session, and off remembered across a restart. What each approval
                // gave is remembered (`Paused`, not cleared), so turning Remote on again gives every
                // approved device back what it had, with no approval again.
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                control.stop();
                control.remember(Persist::Paused);
                let lan = control.lan.clone();
                drop(control);
                // It ends Remote over this network too: its port closes and every LAN session ends,
                // and the shack says why until it is turned on again.
                if let Some(lan) = lan {
                    lan.end_at_shack();
                }
                return self.status();
            }
            Action::LanOn { address, port } => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                lan.turn_on(address.as_deref(), *port)?;
                return self.status();
            }
            Action::LanOff {} => {
                if let Some(lan) = self.lan()? {
                    lan.turn_off();
                }
                return self.status();
            }
            Action::LanAddress { address } => {
                self.lan()?
                    .ok_or("serviceUnavailable")?
                    .pick(address.as_deref())?;
                return self.status();
            }
            Action::LanPair {} => {
                self.lan()?.ok_or("serviceUnavailable")?.pair()?;
                return self.status();
            }
            Action::LanCancel {} => {
                if let Some(lan) = self.lan()? {
                    lan.cancel_pairing();
                }
                return self.status();
            }
            Action::LanRevoke { device_id } => {
                if !identifier(device_id) {
                    return Err("invalidRequest");
                }
                let (lan, device) = (self.lan()?.ok_or("serviceUnavailable")?, device_id.clone());
                // Off this thread, since the credential store can block. Inside, the computer is
                // out and its control revoked before the store is written.
                tokio::task::spawn_blocking(move || lan.revoke(&device))
                    .await
                    .map_err(|_| "serviceUnavailable")??;
                return self.status();
            }
            Action::LanReset {} => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                tokio::task::spawn_blocking(move || lan.reset())
                    .await
                    .map_err(|_| "serviceUnavailable")??;
                return self.status();
            }
            _ => {}
        }
        let (reply, response) = oneshot::channel();
        {
            let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
            if let Action::Device {
                approve: false,
                device_id,
                ..
            } = &action
            {
                control.revoke_browser(device_id);
            }
            if matches!(
                action,
                Action::Disable {} | Action::Forget {} | Action::Cancel {}
            ) {
                control.stop();
                // Remembered here rather than in the controller: this stop is immediate even when
                // the command queue is full, and what a restart remembers must match it. The stop
                // clears every live permission either way; only Turn off keeps the remembered ones.
                control.remember(if matches!(action, Action::Disable {}) {
                    Persist::Paused
                } else {
                    Persist::Off
                });
            }
            // A refused Enable must not leave enabled authority behind. Stop
            // actions remain immediate even when the command queue is full.
            let permit = self.commands.try_reserve().map_err(|_| "remoteBusy")?;
            if matches!(action, Action::Enable {}) {
                control.start();
                // Turning on starts a fresh authority: every permission in memory is cleared. The
                // grants remembered for still-approved browsers come back once the service lists
                // them (see `Controller::apply_restore`). "On" itself is remembered by the
                // controller, once the connection has actually started.
            }
            permit.send((
                action,
                control.generation,
                control.operations.epoch(),
                reply,
            ));
        }
        response.await.map_err(|_| "serviceUnavailable")??;
        self.status()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
}

struct Pending {
    id: String,
    proof: String,
    code: String,
    expires_at: u64,
    account_id: Option<String>,
    station_credential: Option<String>,
}
struct Controller {
    client: Client,
    vault: Arc<dyn Vault>,
    binding: Option<Binding>,
    pending: Option<Pending>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    engine: crate::SharedEngine,
    feeds: transport::Feeds,
    restore: Option<Restore>,
}
/// Remote came on with grants remembered; they wait for the service's browser list.
struct Restore {
    /// The authority epoch when Remote came on. Any local decision since then (Turn off, take over)
    /// moves it, and that decision wins. A browser revoked since is read out of the record.
    epoch: u64,
    at: tokio::time::Instant,
    attempts: u32,
}
impl Controller {
    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        match self.vault.binding() {
            Ok(Some(binding))
                if binding.origin == self.client.origin()
                    && identifier(&binding.station_id)
                    && identifier(&binding.account_id) =>
            {
                self.binding = Some(binding.clone());
                self.resume(binding).await;
            }
            Ok(None) => {}
            _ => phase(&self.status, "unpaired", Some("credentialStoreUnavailable")),
        }
        loop {
            let retry = self.restore.as_ref().map(|r| r.at);
            tokio::select! {
                next = receiver.recv() => {
                    let Some((action, generation, epoch, reply)) = next else { break };
                    let result = self.handle(action, generation, epoch).await;
                    if let Err(error) = result {
                        if let Ok(mut status) = self.status.lock() {
                            status.error = Some(error);
                        }
                    }
                    let _ = reply.send(result);
                }
                _ = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.retry_restore().await,
            }
        }
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
    /// Remote remembers being on. If it was on when Nexus last exited, turn it on again exactly as
    /// Turn on Remote does: a fresh local authority (every in-memory permission cleared, transmit
    /// included), a fresh generation, and the remembered grants restored for still-approved
    /// browsers. A locked or unreadable store, a record written for another pairing, or a record
    /// that says off all leave it off.
    async fn resume(&mut self, binding: Binding) {
        let remembered = match self.vault.state() {
            Ok(state) => state.filter(|s| {
                s.binding == binding
                    && s.grants.len() <= MAX_REMEMBERED
                    && s.grants.iter().all(|g| identifier(&g.device_id))
            }),
            Err(error) => {
                if let Ok(mut control) = self.control.lock() {
                    control.remember(Persist::Bound(binding, None, None));
                }
                self.reflect("disabled");
                phase(&self.status, "disabled", Some(error));
                return;
            }
        };
        // A5: the pins, from their own entry. One that is locked, unreadable or out of shape pins
        // nothing: every browser is then approved again at the radio before it streams.
        let pins = self.vault.pins().ok().flatten().filter(|p| {
            p.binding == binding
                && p.keys.len() <= MAX_REMEMBERED
                && p.keys
                    .iter()
                    .all(|(id, key)| identifier(id) && pin_shape(key))
        });
        // S3-M1: this pairing's signing key, made here once for a pairing from before it existed.
        let (key, identity) = self.station_key(&binding);
        // Decide under the lock BEFORE the pairing becomes visible, so a Turn off that arrives
        // afterwards is ordered after this decision and wins.
        let resumed = {
            let Ok(mut control) = self.control.lock() else {
                return;
            };
            let enabled = remembered.as_ref().is_some_and(|s| s.enabled);
            control.station_key = key;
            control.station_identity = identity;
            control.key_refused = false;
            control.remember(Persist::Bound(binding, remembered, pins.map(Box::new)));
            if enabled {
                control.start();
                Some(control.generation)
            } else {
                None
            }
        };
        self.reflect("disabled");
        let Some(generation) = resumed else {
            return;
        };
        if let Err(error) = self.enable(generation) {
            if let Ok(mut status) = self.status.lock() {
                status.error = Some(error);
            }
        }
    }
    /// Nobody opens Settings after an unattended restart, so the station fetches its own browser
    /// list to restore grants, backing off while the service is unreachable.
    async fn retry_restore(&mut self) {
        let current = self.restore.as_ref().is_some_and(|restore| {
            self.control
                .lock()
                .is_ok_and(|c| c.enabled && c.operations.epoch() == restore.epoch)
        });
        if !current {
            self.restore = None;
            return;
        }
        match self.devices().await {
            Ok(devices) => {
                self.apply_restore(&devices);
                self.listed(&devices);
                if let Ok(mut status) = self.status.lock() {
                    status.devices = devices;
                }
            }
            Err(_) => {
                if let Some(restore) = &mut self.restore {
                    restore.attempts = restore.attempts.saturating_add(1).min(5);
                    restore.at = tokio::time::Instant::now()
                        + std::time::Duration::from_secs(1 << restore.attempts);
                }
            }
        }
    }
    /// Put back remembered grants for each browser the service still lists as approved at the SAME
    /// approval (`Approval::holds`: the same generation, so a renewal on use still matches), under the
    /// epoch captured when Remote came on. The grants are read from the record now, under the lock, so
    /// a revocation made while the list was on its way is honoured. FT8/FT4 transmit comes back only
    /// where it was granted, only with station control, and arms nothing.
    fn apply_restore(&mut self, devices: &[Device]) {
        let Some(restore) = self.restore.take() else {
            return;
        };
        let now = now_ms();
        let approvals: Vec<(String, Approval)> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| (d.id.clone(), d.approval()))
            .collect();
        let Ok(mut control) = self.control.lock() else {
            return;
        };
        if !control.enabled || control.operations.epoch() != restore.epoch {
            return;
        }
        let kept: Vec<vault::Grant> = control
            .remembered
            .grants()
            .iter()
            .filter(|g| {
                approvals
                    .iter()
                    .any(|(id, approval)| *id == g.device_id && approval.holds(g))
            })
            .cloned()
            .collect();
        let ids = |held: fn(&vault::Grant) -> bool| {
            kept.iter()
                .filter(|g| held(g))
                .map(|g| g.device_id.clone())
                .collect()
        };
        let grants = operations::DurableGrants {
            logging: ids(|g| g.logging),
            control: ids(|g| g.control),
            transmit: ids(|g| g.control && g.transmit),
        };
        let operations = control.operations.clone();
        match operations.restore(restore.epoch, &grants) {
            // Browsers no longer approved at the same approval are forgotten.
            Ok(true) => control.remember(Persist::Approvals(approvals)),
            Ok(false) => {}
            Err(_) => {
                drop(control);
                self.restore = Some(Restore {
                    at: tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                    ..restore
                });
            }
        }
    }
    /// The service's list, as the room's tunnel reads it (`Control::listed`).
    fn listed(&self, devices: &[Device]) {
        if let Ok(mut control) = self.control.lock() {
            control.listed(devices);
        }
    }
    async fn devices(&self) -> Result<Vec<Device>, &'static str> {
        let (binding, token) = self.bound()?;
        let value = self
            .client
            .post(&station_path(&binding, "devices"), Some(&token), empty())
            .await?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Devices {
            devices: Vec<Device>,
        }
        let mut value: Devices = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
        // One id names one device, and only ever an id the service made: a press, a pin and a grant
        // are each made for an id, so a second device under it, or one under a name the shack gives
        // its own computers, refuses the list as a malformed id does.
        let ids: std::collections::BTreeSet<&str> =
            value.devices.iter().map(|d| d.id.as_str()).collect();
        if value.devices.len() > 8
            || ids.len() != value.devices.len()
            || value.devices.iter().any(|d| {
                !hosted(&d.id) || !valid_name(&d.name) || d.approved > 1 || !d.kind.fits(d.platform)
            })
        {
            return Err("invalidResponse");
        }
        // A5: a key that is not the shape of a P-256 key is no key. It shows nothing and pins
        // nothing, and the rest of the list (approvals, grants) stands.
        for device in &mut value.devices {
            device.key = device.public_key.as_deref().and_then(fingerprint);
        }
        Ok(value.devices)
    }
    /// This pairing's signing key (S3-M1), made once: the one the vault holds for it, or a new one
    /// saved there first. A store that will not answer (locked) makes nothing, because the key may
    /// be in it: the station's answers go unsigned until it does.
    fn station_key(&self, binding: &Binding) -> StationKeys {
        match self.vault.station_key() {
            Ok(Some(kept)) if kept.binding == *binding => {
                if let Some(signer) = station_key::Signer::restore(&kept.pkcs8) {
                    return (
                        Some(Arc::new(signer)),
                        tunnel_identity(&kept.pkcs8, binding),
                    );
                }
            }
            Ok(_) => {}
            Err(_) => return (None, None),
        }
        self.new_station_key(binding)
    }
    /// A new signing key for `binding`, saved before it is used: a key the vault did not keep would
    /// be gone at the next start, and the service would refuse the one made then.
    fn new_station_key(&self, binding: &Binding) -> StationKeys {
        let Some((signer, pkcs8)) = station_key::Signer::generate() else {
            return (None, None);
        };
        let identity = tunnel_identity(&pkcs8, binding);
        if self
            .vault
            .save_station_key(&vault::StationKey {
                binding: binding.clone(),
                pkcs8,
            })
            .is_err()
        {
            return (None, None);
        }
        (Some(Arc::new(signer)), identity)
    }
    fn reflect(&self, value: &str) {
        if let Ok(mut status) = self.status.lock() {
            status.phase = value.into();
            status.error = None;
            status.station_id = self.binding.as_ref().map(|b| b.station_id.clone());
            status.account_id = self
                .binding
                .as_ref()
                .map(|b| b.account_id.clone())
                .or_else(|| self.pending.as_ref().and_then(|p| p.account_id.clone()));
            status.pairing_id = self.pending.as_ref().map(|p| p.id.clone());
            status.pairing_code = self.pending.as_ref().map(|p| p.code.clone());
            status.expires_at = self.pending.as_ref().map(|p| p.expires_at);
            if self.binding.is_none() {
                status.devices.clear();
            }
        }
    }
    fn bound(&self) -> Result<(Binding, String), &'static str> {
        let binding = self.binding.as_ref().ok_or("stationNotPaired")?.clone();
        let token = self.vault.credential(&binding)?;
        if !credential(&token) {
            return Err("credentialStoreUnavailable");
        }
        Ok((binding, token))
    }
    /// ONE APPROVAL (operator decision 2026-09-13): approving a browser grants it station control
    /// and logging, and FT8/FT4 transmit when the approval ticked it. With Remote on they take
    /// effect at once; with Remote off they wait in the record for Turn on. A local decision made
    /// after the approval was asked for (Turn off or on, take over) wins, and nothing is granted;
    /// revoking this device does too (the revoke is taken again after this, `Controller::handle`). ⛔ Granting transmit arms nothing: the browser still has to press TX On.
    ///
    /// A5: the device key the operator was shown (`shown`) is pinned if the service still lists that
    /// key for the browser now that it has approved it. A key that changed in between pins nothing,
    /// and that browser is approved again before it streams.
    ///
    /// The approval is of the device as it was shown (`kind`): a device the service now lists as
    /// another kind is granted nothing and pinned for nothing, and is asked about again in its own
    /// words. A computer's pin is its own ([`Device::pin`]), made for this approval.
    fn grant_approved(
        &self,
        devices: &[Device],
        device_id: String,
        transmit: bool,
        shown: Option<String>,
        kind: Option<Kind>,
        (generation, epoch): (u64, u64),
    ) -> Result<(), &'static str> {
        let now = now_ms();
        let device = devices
            .iter()
            .find(|d| d.id == device_id && d.approved == 1 && d.expires_at > now)
            .ok_or("invalidResponse")?;
        let approval = device.approval();
        let as_shown = kind == Some(device.kind);
        let pin = shown
            .filter(|shown| as_shown && device.key.as_ref() == Some(shown))
            .and_then(|_| device.pin());
        let approved: Vec<String> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| d.id.clone())
            .collect();
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(());
        }
        // A computer (the native client) is granted nothing on the relay's lanes, whatever the
        // approval said of transmit: it never reaches them. Its approval is its pin, below, which
        // the tunnel through the room checks. A browser is granted only if it was shown as one.
        let browser = as_shown && device.kind == Kind::Browser;
        if control.enabled && browser {
            let operations = control.operations.clone();
            operations.permit(&device_id, true)?;
            operations.permit_station(&device_id, true)?;
            // A fresh approval without the tick holds no transmit, even if an earlier one did.
            operations.permit_transmit(&device_id, transmit)?;
        }
        if browser {
            control.remember(Persist::Browser {
                device_id: device_id.clone(),
                approval: Some(approval),
                logging: Some(true),
                control: Some(true),
                transmit: Some(transmit),
            });
        }
        control.remember(Persist::Listed(approved));
        control.remember(Persist::Pin {
            device_id,
            key: pin,
        });
        Ok(())
    }
    /// Connect under `generation`, which the caller has already made enabled with a fresh
    /// authority (Turn on Remote, a launch that remembers being on, or approving the pairing).
    fn enable(&mut self, generation: u64) -> Result<(), &'static str> {
        let bound = self.bound();
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if !control.enabled || control.generation != generation {
            return Ok(());
        }
        let (binding, token) = match bound {
            Ok(value) => value,
            Err(error) => {
                control.stop();
                return Err(error);
            }
        };
        if let Some(cancel) = control.cancel.take() {
            let _ = cancel.send(true);
        }
        let (cancel, receiver) = watch::channel(false);
        control.cancel = Some(cancel);
        phase(&self.status, "connecting", None);
        tokio::spawn(transport::supervise(
            self.client.clone(),
            binding,
            token,
            receiver,
            self.engine.clone(),
            self.feeds.clone(),
            SessionStatus {
                status: self.status.clone(),
                control: self.control.clone(),
                generation,
            },
        ));
        // Remote is on and connecting: the next launch turns it back on.
        control.remember(Persist::Enabled);
        // Remembered grants come back once the service confirms each browser is still approved at
        // the same approval. The epoch is read here, under the lock: a later take over or Turn off
        // moves it, and that decision wins; a later revocation is read out of the record.
        if !control.remembered.grants().is_empty() {
            self.restore = Some(Restore {
                epoch: control.operations.epoch(),
                at: tokio::time::Instant::now(),
                attempts: 0,
            });
        }
        Ok(())
    }
    async fn handle(
        &mut self,
        action: Action,
        generation: u64,
        epoch: u64,
    ) -> Result<(), &'static str> {
        match action {
            Action::LoggingPermission { .. }
            | Action::StationPermission { .. }
            | Action::TransmitPermission { .. }
            | Action::TakeOverLogging {}
            | Action::LanOn { .. }
            | Action::LanOff {}
            | Action::LanAddress { .. }
            | Action::LanPair {}
            | Action::LanCancel {}
            | Action::LanRevoke { .. }
            | Action::LanReset {} => return Err("invalidRequest"),
            Action::Begin { name } => {
                if self.binding.is_some() || self.pending.is_some() || !valid_name(&name) {
                    return Err("invalidRequest");
                }
                let value = self
                    .client
                    .post("enroll", None, json!({ "name": name.trim() }))
                    .await?;
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Enrollment {
                    id: String,
                    proof: String,
                    code: String,
                    expires_at: u64,
                }
                let e: Enrollment = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                if !identifier(&e.id)
                    || !credential(&e.proof)
                    || e.code.len() != 16
                    || !e.code.bytes().all(|b| b.is_ascii_hexdigit())
                    || e.expires_at <= now_ms()
                    || e.expires_at > now_ms() + 660000
                {
                    return Err("invalidResponse");
                }
                self.pending = Some(Pending {
                    id: e.id,
                    proof: e.proof,
                    code: e.code,
                    expires_at: e.expires_at,
                    account_id: None,
                    station_credential: None,
                });
                self.reflect("pairing");
            }
            Action::Cancel {} => {
                self.pending = None;
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Refresh {} => {
                if let Some(pending) = &mut self.pending {
                    if pending.expires_at <= now_ms() {
                        self.pending = None;
                        self.reflect("unpaired");
                        return Err("pairingExpired");
                    }
                    let value = self
                        .client
                        .post(
                            "enroll/check",
                            None,
                            json!({ "id": pending.id, "proof": pending.proof }),
                        )
                        .await?;
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Check {
                        account_id: Option<String>,
                        approved: bool,
                    }
                    let check: Check =
                        serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                    if check
                        .account_id
                        .as_deref()
                        .is_some_and(|value| !identifier(value))
                    {
                        return Err("invalidResponse");
                    }
                    // A successful approve whose response was lost may be retried
                    // with the same in-memory credential; never issue a new one.
                    if check.approved && pending.station_credential.is_none() {
                        return Err("invalidResponse");
                    }
                    pending.account_id = check.account_id;
                    self.reflect(
                        if self
                            .pending
                            .as_ref()
                            .is_some_and(|p| p.account_id.is_some())
                        {
                            "approval"
                        } else {
                            "pairing"
                        },
                    );
                } else if self.binding.is_some() {
                    let devices = self.devices().await?;
                    // A fresh list is also what a restore after a restart is waiting for.
                    self.apply_restore(&devices);
                    self.listed(&devices);
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                        status.error = None;
                    }
                }
            }
            Action::Approve {
                enrollment_id,
                account_id,
                transmit,
            } => {
                let pending = self.pending.as_mut().ok_or("pairingExpired")?;
                if pending.id != enrollment_id
                    || pending.account_id.as_deref() != Some(&account_id)
                    || pending.expires_at <= now_ms()
                {
                    return Err("pairingExpired");
                }
                let token = pending
                    .station_credential
                    .get_or_insert(transport::random_secret()?)
                    .clone();
                let binding = Binding {
                    origin: self.client.origin().into(),
                    station_id: pending.id.clone(),
                    account_id: account_id.clone(),
                };
                // Confirm the OS vault works before creating cloud station authority.
                self.vault.stage(&binding, &token)?;
                // One trial per callsign and per shack PC: the station callsign configured here (sent
                // as it is, blank included, for the service to judge) and the PC's two hashed
                // identifiers (`pc_identity`), read only now, when approving may start a trial.
                let callsign = tempo_app::engine::engine_lock(&self.engine)
                    .settings()
                    .mycall
                    .trim()
                    .to_string();
                let result = self
                    .client
                    .post(
                        "enroll/approve",
                        None,
                        json!({ "id": pending.id, "proof": pending.proof, "credential": token,
                            "callsign": callsign, "pc": pc_identity::pc() }),
                    )
                    .await;
                let value = match result {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = self.vault.remove(&binding);
                        return Err(error);
                    }
                };
                if value.get("stationId").and_then(|v| v.as_str()) != Some(&binding.station_id)
                    || value.get("accountId").and_then(|v| v.as_str()) != Some(&binding.account_id)
                {
                    let _ = self.vault.remove(&binding);
                    return Err("invalidResponse");
                }
                self.vault.save(&binding, &token)?;
                // S3-M1: a new pairing is a new station, with a signing key of its own.
                let (key, identity) = self.new_station_key(&binding);
                // One approval: the service approved the browser that confirmed this pairing along
                // with the station. It gets what any approved browser gets, waiting in the record
                // for Turn on. A service that reports no browser pairs exactly as it used to.
                let paired = value
                    .get("device")
                    .and_then(|d| {
                        Some((
                            d.get("id")?.as_str()?.to_string(),
                            Approval {
                                expires_at: d.get("expiresAt")?.as_u64()?,
                                generation: d.get("generation").and_then(|g| g.as_u64()),
                            },
                            // The device key the confirming browser brought (A5), if any.
                            d.get("publicKey")
                                .and_then(|k| k.as_str())
                                .and_then(fingerprint),
                        ))
                    })
                    .filter(|(id, approval, _)| identifier(id) && approval.expires_at > now_ms());
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = key;
                    control.station_identity = identity;
                    control.key_refused = false;
                    // A new pairing starts with nothing remembered: off, no grants, no pins.
                    control.remember(Persist::Bound(binding.clone(), None, None));
                    if let Some((device_id, approval, key)) = paired {
                        control.remember(Persist::Browser {
                            device_id: device_id.clone(),
                            approval: Some(approval),
                            logging: Some(true),
                            control: Some(true),
                            transmit: Some(transmit),
                        });
                        // A5, operator ruling D4 (2026-09-28): that browser's key is pinned on first
                        // use, the trust the pairing already places in the service. Both ends show
                        // its fingerprint straight after, and revoking the browser is one click.
                        control.remember(Persist::Pin { device_id, key });
                    }
                }
                self.binding = Some(binding);
                self.pending = None;
                self.reflect("disabled");
                // Approving the pairing also turns Remote on (operator decision 2026-09-14), exactly
                // as Turn on Remote does: a fresh authority, and the TX-enable latch untouched. A
                // Turn off or Cancel made while the approval was on its way moved the generation,
                // and wins.
                let turned_on = self.control.lock().ok().and_then(|mut control| {
                    (control.generation == generation).then(|| {
                        control.start();
                        control.generation
                    })
                });
                if let Some(generation) = turned_on {
                    self.enable(generation)?;
                }
            }
            Action::Enable {} => self.enable(generation)?,
            Action::Disable {} => {
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Forget {} => {
                let (binding, token) = self.bound()?;
                self.client
                    .post(&station_path(&binding, "revoke"), Some(&token), empty())
                    .await?;
                self.vault.remove(&binding)?;
                // The pairing's key goes with it: a new pairing makes its own.
                let _ = self.vault.remove_station_key();
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = None;
                    control.station_identity = None;
                    control.key_refused = false;
                    control.remember(Persist::Removed);
                }
                self.binding = None;
                self.pending = None;
                self.reflect("unpaired");
            }
            Action::Device {
                device_id,
                approve,
                transmit,
                key,
                kind,
            } => {
                if !identifier(&device_id) || key.as_deref().is_some_and(|key| !pin_shape(key)) {
                    return Err("invalidRequest");
                }
                if !approve {
                    // Taken when the press was made (`Service::action`); taken again here, after
                    // any approval of this device that was already on its way when it was made,
                    // so that approval cannot hand the grants back.
                    if let Ok(mut control) = self.control.lock() {
                        control.revoke_browser(&device_id);
                    }
                }
                let (binding, token) = self.bound()?;
                self.client
                    .post(
                        &station_path(
                            &binding,
                            if approve {
                                "approve-device"
                            } else {
                                "revoke-device"
                            },
                        ),
                        Some(&token),
                        json!({ "deviceId": device_id }),
                    )
                    .await?;
                if let Ok(mut status) = self.status.lock() {
                    status.devices.clear();
                }
                if approve {
                    // The list names the approval the service just wrote; the grant binds to it.
                    let devices = self.devices().await?;
                    let granted = self.grant_approved(
                        &devices,
                        device_id,
                        transmit,
                        key,
                        kind,
                        (generation, epoch),
                    );
                    self.listed(&devices);
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                    }
                    granted?;
                }
            }
        }
        Ok(())
    }
}

/// The vault writer. Each write is a whole record for the current pairing, produced in the order
/// the decisions were made, so a backlog collapses to the latest write of each entry. A failed save
/// is followed by deleting the record: a record that could not be updated must not survive to be
/// restored. No state record means Remote stays off at the next launch; no pins record means every
/// browser is approved again at the radio before it streams.
fn write_remembered(vault: Arc<dyn Vault>, mut writes: mpsc::UnboundedReceiver<Write>) {
    while let Some(first) = writes.blocking_recv() {
        let (mut state, mut pins) = (None, None);
        let mut next = Some(first);
        while let Some(write) = next {
            match write {
                Write::Save(_) | Write::Remove => state = Some(write),
                Write::SavePins(_) | Write::RemovePins => pins = Some(write),
            }
            next = writes.try_recv().ok();
        }
        for write in [state, pins].into_iter().flatten() {
            match write {
                Write::Save(state) => {
                    if vault.save_state(&state).is_err() {
                        let _ = vault.remove_state();
                    }
                }
                Write::Remove => {
                    let _ = vault.remove_state();
                }
                Write::SavePins(pins) => {
                    if vault.save_pins(&pins).is_err() {
                        let _ = vault.remove_pins();
                    }
                }
                Write::RemovePins => {
                    let _ = vault.remove_pins();
                }
            }
        }
    }
}

#[tauri::command]
pub fn get_remote_station_status(
    service: tauri::State<'_, Service>,
) -> Result<Status, &'static str> {
    service.status()
}
#[tauri::command]
pub async fn remote_station_action(
    service: tauri::State<'_, Service>,
    action: Action,
) -> Result<Status, &'static str> {
    service.action(action).await
}
