//! Remote over this network, on the computer an operator works FROM: the other end of the shack's
//! own listener (`remote_service::lan`), carried in a Nexus window of its own (the operator's
//! rulings of 2026-10-04: "Window only in v1", so never a browser; "Both at once", beside the hosted
//! road; "Refuse, say which" for versions). No relay, no sign-in and no internet.
//!
//! ## What it holds, and what it never does
//!
//! - **Its own key for each station** ([`ComputerKey`]): P-256, made with ring when this computer
//!   pairs with that station, and kept with the station's record in the OS credential store, and
//!   nowhere else. This is the design's second key, approved on 2026-10-04 ("Approve as designed"),
//!   and the only secret this module keeps. It proves this computer twice over, in the TLS
//!   handshake and on every stream offer (A5), and the two signatures cover different labels, so
//!   neither passes for the other. The private half is never logged, shown or sent: the page never
//!   holds it, and nothing that carries it has `Debug`, `Clone` or `Serialize`.
//! - **Each station's record** ([`Stations`]): the station's key as it presented it while pairing,
//!   pinned from then on; its LAN station id; the device id it gave this computer; and the
//!   addresses it answered at, the last one that worked first; and, once Operate opened there, the
//!   address or host name the operator typed for it, which Operate tries first. One credential entry for each
//!   station, within the smallest one Windows keeps (2560 bytes of UTF-16), and the list of them
//!   in one more.
//! - **No engine, no radio and no operations authority.** This computer's own station, if it has
//!   one, is out of reach: nothing here names the engine, and the window's page reaches no Nexus
//!   command (the loopback origin, below).
//! - **This computer's headset, in client mode** ([`audio_road`]): the station's audio out and the
//!   operator's microphone in, on this computer's own devices, opened only while wanted. It plays
//!   and captures; nothing here keys anything.
//!
//! ## Pairing ([`pairing`])
//!
//! The computer's half of the shack's ceremony, with the shack's own functions
//! (`remote_service::lan::pairing`). The operator types the code the shack shows: sixteen
//! hexadecimal characters, either case, spaces anywhere ([`code`]). This computer makes a key,
//! connects with no station key pinned (that one connection only), checks the station's proof
//! BEFORE it sends its own, and keeps the record when the station says `paired`. Each refusal
//! comes back as a code the page says in a sentence.
//!
//! ## The road ([`road`])
//!
//! TLS 1.3 with this computer's key and the station's key pinned: any other key is refused before
//! this computer sends its own. Then the hello: the versions must be the station's own, or the page
//! is told which side to update. After the welcome, the page's operation requests (state, acquire,
//! heartbeat, release and Stop) and its stream signals go to the station, and the station's
//! answers, stream state and status line come back. Two things happen here, in Rust, so the key
//! never enters the page: every offer is signed with this computer's key for this station and
//! session (A5), and every answer is checked against the station's pinned key before the page
//! sees it (S3-M1). The page checks the answer again with its own code.
//!
//! ## Found by name (the operator's ruling of 2026-10-04, "By name, or typed")
//!
//! The pairing dialog's address field offers the stations Windows' own DNS-SD finds on this
//! computer's networks (`tempo_stream::lan::dnssd::find`, through the page's socket), and a typed
//! address still works. A paired station is tried where it answered before, the last address that
//! worked first, and only when none of those answers, or another key answers there, where a look
//! by name finds it: an advert whose key tag starts the key pinned for it. A station found by name is a hint, never an
//! identity, since the road takes only the pinned key.
//!
//! ## The loopback origin ([`origin`])
//!
//! The window's page is served from `http://127.0.0.1:<port>`, which Tauri treats as a remote
//! origin: the page reaches NO Nexus command, as the hosted page in the Remote stations window
//! reaches none (`remote_window`'s tests hold both). It talks to this module over one WebSocket on
//! the same origin. The listener binds the loopback address only, lives while the window does,
//! and answers a request only through all three of its gates ([`origin::Gates`]), each of which
//! refuses on its own:
//!
//! 1. **The launch secret**: 32 random bytes made each time the window opens, the first segment of
//!    every path. The window's address carries it; nothing else knows it.
//! 2. **The `Host` header**: exactly `127.0.0.1:<port>`, so a page that points a name of its own
//!    at this address (DNS rebinding) is refused.
//! 3. **The `Origin` header**: the WebSocket only from `http://127.0.0.1:<port>`, the window's own
//!    page, and any other request that names another origin is refused too.
//!
//! ## Control taken back by the hosted road
//!
//! The station ends this computer's lease as a side effect of decisions about the hosted road (Turn
//! Remote on or off, revoking a browser), and gives the grant back at once. The page acquires
//! again after `notController` or `leaseExpired`, when nobody else holds control; nothing at the
//! station changes for it.
/// The audio road, beside the command link, and this computer's headset (the client core's).
pub(crate) use tempo_client::audio_road;
/// The command link, protocol 2, which client mode drives (the client core's).
pub(crate) use tempo_client::link;
/// The internet road: adding it to a paired station, and opening it again.
pub(crate) mod internet;
mod origin;
pub(crate) mod pairing;
/// The road to a paired station (the client core's).
pub(crate) use tempo_client::road;
/// Stop's second road, beside the command link (the client core's).
pub(crate) use tempo_client::stop_road;
#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) use origin::PREFERRED_PORT;
pub(crate) use origin::{Origin, Reach};

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tempo_client::hex;
/// This computer's key for a station, and a paired station as the page shows it (the client
/// core's).
pub(crate) use tempo_client::{ComputerKey, StationView};
use tempo_stream::lan::Typed;
use tempo_stream::protocol;

use crate::remote_service::lan::{valid_name, NAME_CHARS};
use crate::remote_service::vault::{InternetRoad, PairedStation, PairedStations, StationVault};

/// The most stations this computer keeps a pairing with.
pub(crate) const MAX_STATIONS: usize = 8;
/// The most addresses one station's record keeps.
pub(crate) const MAX_ADDRESSES: usize = 4;
/// The smallest OS credential blob, Windows': 2560 bytes of UTF-16.
const CREDENTIAL_BYTES: usize = 2560;

/// The code as the operator typed it: sixteen hexadecimal characters, either case, with spaces
/// anywhere (the shack shows it in groups of four). `None` for anything else.
pub(crate) fn code(typed: &str) -> Option<[u8; 8]> {
    let hex: String = typed
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    protocol::hex_bytes(&hex)?.try_into().ok()
}

/// This computer's name as the operating system gives it, for the station to show beside its key:
/// at most [`NAME_CHARS`] characters, or empty when there is none to give, and the operator types
/// one.
pub(crate) fn computer_name() -> String {
    let given = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let name: String = given
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CHARS)
        .collect();
    if valid_name(&name) {
        name.trim().to_string()
    } else {
        String::new()
    }
}

/// Does this record fit the smallest credential blob?
fn fits<T: Serialize>(record: &T) -> bool {
    serde_json::to_string(record)
        .is_ok_and(|value| value.encode_utf16().count() * 2 <= CREDENTIAL_BYTES)
}

/// Is this a record this module could have written: ids that are ids, a station key that is a
/// P-256 SPKI, a key that is one, and addresses that are private? A record that is not reads as
/// none: that station is paired again.
fn sound(record: &PairedStation) -> bool {
    protocol::identifier(&record.station_id)
        && protocol::identifier(&record.device_id)
        && protocol::device_key(&record.station_key)
        && !record.addresses.is_empty()
        && record.addresses.len() <= MAX_ADDRESSES
        && record
            .addresses
            .iter()
            .all(|a| tempo_stream::lan::typed(a).is_some())
        && ComputerKey::restore(&record.pkcs8).is_some()
}

/// Is this an internet road this module could have written: the service's ids, a station key that
/// is a P-256 SPKI, and a key that is one? One that is not reads as none: the road is added again.
fn sound_road(road: &InternetRoad) -> bool {
    protocol::identifier(&road.station_id)
        && protocol::identifier(&road.device_id)
        && protocol::device_key(&road.station_key)
        && ComputerKey::restore(&road.pkcs8).is_some()
}

/// The stations this computer is paired with, in the OS credential store. The store can block, so
/// a caller off any thread that must not wait makes these calls.
pub(crate) struct Stations {
    vault: Arc<dyn StationVault>,
    /// One writer at a time, so two presses never interleave their writes to the list.
    writing: Mutex<()>,
}

impl Stations {
    pub(crate) fn new(vault: Arc<dyn StationVault>) -> Self {
        Self {
            vault,
            writing: Mutex::new(()),
        }
    }

    fn ids(&self) -> Result<Vec<String>, &'static str> {
        Ok(self
            .vault
            .paired_stations()
            .map_err(|_| "storeUnavailable")?
            .map(|list| list.stations)
            .unwrap_or_default()
            .into_iter()
            .filter(|id| protocol::identifier(id))
            .collect())
    }

    /// Every paired station the store holds a sound record for, in the order they were paired.
    pub(crate) fn list(&self) -> Result<Vec<StationView>, &'static str> {
        Ok(self.records()?.iter().filter_map(StationView::of).collect())
    }

    /// The same stations' records, this computer's key for each included: what pairing with one of
    /// them again takes that key from ([`pairing::pair`]).
    pub(crate) fn records(&self) -> Result<Vec<PairedStation>, &'static str> {
        let mut records = Vec::new();
        for id in self.ids()? {
            records.extend(self.get(&id)?);
        }
        Ok(records)
    }

    /// The record for station `id`, if the store holds a sound one.
    pub(crate) fn get(&self, id: &str) -> Result<Option<PairedStation>, &'static str> {
        if !protocol::identifier(id) {
            return Ok(None);
        }
        Ok(self
            .vault
            .paired_station(id)
            .map_err(|_| "storeUnavailable")?
            .filter(|record| record.station_id == id && sound(record)))
    }

    /// Keep a station's record: a new pairing, or the same station paired again (its record is
    /// replaced). The record first, then the list, so a crash between them leaves a record nobody
    /// lists, never a listed station with no record. `stationsFull` with [`MAX_STATIONS`] paired
    /// already, or a record that would not fit one entry.
    pub(crate) fn keep(&self, record: &PairedStation) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !sound(record) {
            return Err("notStation");
        }
        if !fits(record) {
            return Err("stationsFull");
        }
        let mut ids = self.ids()?;
        ids.retain(|id| *id != record.station_id);
        if ids.len() >= MAX_STATIONS {
            return Err("stationsFull");
        }
        ids.push(record.station_id.clone());
        let list = PairedStations { stations: ids };
        if !fits(&list) {
            return Err("stationsFull");
        }
        self.vault
            .save_paired_station(record)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .save_paired_stations(&list)
            .map_err(|_| "storeUnavailable")
    }

    /// Station `id` answered at `at`: that address goes first, so it is the first tried next time.
    pub(crate) fn worked(&self, id: &str, at: SocketAddrV4) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let Some(mut record) = self.get(id)? else {
            return Err("unknownStation");
        };
        let at = at.to_string();
        if record.addresses.first() == Some(&at) {
            return Ok(());
        }
        record.addresses.retain(|a| *a != at);
        record.addresses.insert(0, at);
        record.addresses.truncate(MAX_ADDRESSES);
        self.vault
            .save_paired_station(&record)
            .map_err(|_| "storeUnavailable")
    }

    /// Keep `typed`, an address or a host name, as the one Operate tries first for station `id`,
    /// or none. A record with an address it cannot read keeps its pairing and reads as none.
    pub(crate) fn keep_typed(&self, id: &str, typed: Option<&Typed>) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let Some(mut record) = self.get(id)? else {
            return Err("unknownStation");
        };
        let typed = typed.map(Typed::to_string);
        if record.typed_address == typed {
            return Ok(());
        }
        record.typed_address = typed;
        self.vault
            .save_paired_station(&record)
            .map_err(|_| "storeUnavailable")
    }

    /// The internet road kept for station `id`, if the store holds a sound one.
    pub(crate) fn internet(&self, id: &str) -> Result<Option<InternetRoad>, &'static str> {
        if !protocol::identifier(id) {
            return Ok(None);
        }
        Ok(self
            .vault
            .internet_road(id)
            .map_err(|_| "storeUnavailable")?
            .filter(sound_road))
    }

    /// Keep the internet road to station `id`, paired here: a new one replaces any before it.
    pub(crate) fn keep_internet(&self, id: &str, road: &InternetRoad) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if self.get(id)?.is_none() {
            return Err("unknownStation");
        }
        if !sound_road(road) {
            return Err("notStation");
        }
        if !fits(road) {
            return Err("stationsFull");
        }
        self.vault
            .save_internet_road(id, road)
            .map_err(|_| "storeUnavailable")
    }

    /// Forget station `id` on this computer: its internet road and its record, with this
    /// computer's keys for it, first, then the list. A list that still names it after a failure
    /// names a record that is gone, which reads as no pairing.
    pub(crate) fn forget(&self, id: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !protocol::identifier(id) {
            return Err("unknownStation");
        }
        self.vault
            .remove_internet_road(id)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .remove_paired_station(id)
            .map_err(|_| "storeUnavailable")?;
        let mut ids = self.ids()?;
        ids.retain(|kept| kept != id);
        self.vault
            .save_paired_stations(&PairedStations { stations: ids })
            .map_err(|_| "storeUnavailable")
    }
}
