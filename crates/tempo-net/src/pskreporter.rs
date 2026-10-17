//! PSK Reporter spot upload — the UDP datagram WSJT-X sends to
//! `report.pskreporter.info:4739`.
//!
//! PSK Reporter ingests an **IPFIX-like** binary format (it predates and loosely
//! follows RFC 5101 / IPFIX). Each datagram is:
//!
//! ```text
//! Header (16 bytes, big-endian):
//!   u16 version          = 0x000A
//!   u16 length           = total datagram byte length
//!   u32 export_time      = Unix seconds
//!   u32 sequence_number  = the spots this reporter sent before this datagram
//!   u32 observation_id   = a random-ish "random identifier" for this sender
//!
//! Then a sequence of "sets", each: u16 set_id, u16 set_length, padded to a
//! 4-byte boundary. Two kinds appear:
//!
//!   * Template/Options-Template sets (set_id 2 / 3) declaring the field layout
//!     of the records that follow. WSJT-X embeds two well-known templates with
//!     fixed ids 0x50E2 (receiver info) and 0x50E3 (sender/spot info).
//!   * Data sets, whose set_id equals a previously declared template id.
//!
//! In practice WSJT-X sends, in one datagram:
//!   1. the two template definitions (so the server is self-describing),
//!   2. a *receiver* data record (our call / grid / software),
//!   3. one or more *sender* data records (each a spotted station).
//!
//! ## Assumptions / notes
//! - Strings are length-prefixed by a single byte (variable-length IE encoding
//!   < 255 bytes), then the UTF-8 bytes; the whole record is padded with NULs to
//!   a 4-byte boundary, as WSJT-X does.
//! - The two template ids `0x50E2`/`0x50E3` and their enterprise-numbered
//!   information elements are the de-facto values from the WSJT-X source
//!   (`PSKReporter.cpp`). The server keys on the *template field list*, so as
//!   long as the declared templates match the data records they parse. We
//!   reproduce the minimal field set WSJT-X uses.
//! - The sequence number counts the spots this reporter has sent before the datagram, from 0, as
//!   WSJT-X 3.0.2's does (Network/PSKReporter.cpp:63, 324; IPFIX's count of data records), so a
//!   flush split in several datagrams reads to the server as several, not one sent again.
//! - A flush is split as WSJT-X 3.0.2 splits a report (`PSKReporterIPFIX::buildPackets`,
//!   PSKReporterIPFIX.cpp:234-292, called from PSKReporter.cpp:302-325): datagrams of at most
//!   [`MAX_DATAGRAM_BYTES`], 952 bytes, with every string cut to WSJT-X's limits
//!   (PSKReporterIPFIX.cpp:16-19, `boundedUtf8` :34-49), so a datagram can never outgrow its
//!   u16 length. One datagram per flush, whatever its size, lost a busy band's whole flush: past
//!   65,507 bytes UDP refuses it. Unlike WSJT-X, which sends its templates in the first three
//!   reports after it connects and hourly after that, every datagram here carries both templates
//!   and the receiver record, so one lost on the way orphans no other.
//! - [`SpotCache`] is WSJT-X's one-spot-per-call rule (PSKReporter.cpp:382-397, 414-419), for the
//!   caller to apply to the decodes it spots as WSJT-X does; JS8's spots keep JS8Call's own.
//! - Spot `time` is Unix seconds (the "flowStartSeconds" IE).
//! - The sender record carries the spotted station's locator and an information source, as
//!   WSJT-X's and JS8Call 3.0.3's do (`appendSIDTo`: senderLocator, then informationSource 1,
//!   "automatically extracted"). A spot with no known locator sends it empty, which is what
//!   WSJT-X sends for every decode that is not a CQ. The frequency stays 4 bytes (theirs is 5):
//!   the server reads the width the template declares, and 4 bytes reach 4.29 GHz.
//!
//! - The datagram leaves from the reporter's own thread, which looks the server up and sends it.
//!   Whoever calls [`PskReporter::send_spots`] is the radio loop, the loop that also keys and
//!   unkeys the radio, so a DNS server slow to answer must never be waited for there.
//!
//! Everything here builds bytes; tests assert structure and bind loopback only.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default PSK Reporter ingest endpoint.
pub const DEFAULT_TARGET: &str = "report.pskreporter.info:4739";

/// The most bytes one datagram carries: WSJT-X 3.0.2's `maxUdpIpfixPayloadBytes()`
/// (PSKReporterIPFIX.cpp:9-12, 224-227), a 1,000-byte packet less a 40-byte IPv6 header and an
/// 8-byte UDP header, so no datagram is fragmented on its way.
pub const MAX_DATAGRAM_BYTES: usize = 1000 - 40 - 8;

/// The longest each string may be, in bytes, as WSJT-X cuts them (PSKReporterIPFIX.cpp:16-19).
const CALLSIGN_LIMIT: usize = 32;
const LOCATOR_LIMIT: usize = 16;
const MODE_LIMIT: usize = 16;
const PROGRAM_INFO_LIMIT: usize = 80;

/// IPFIX version word.
const IPFIX_VERSION: u16 = 0x000A;
/// Template-set id (a set whose records define a template).
const SET_ID_TEMPLATE: u16 = 0x0002;
/// Options-template-set id.
const SET_ID_OPTIONS_TEMPLATE: u16 = 0x0003;
/// Template id WSJT-X uses for the receiver-information record.
const TEMPLATE_RX: u16 = 0x50E2;
/// Template id WSJT-X uses for each sender (spot) record.
const TEMPLATE_TX: u16 = 0x50E3;

/// One spotted station to report.
#[derive(Debug, Clone)]
pub struct Spot {
    /// The call sign we copied.
    pub call: String,
    /// Frequency in Hz the station was heard on.
    pub freq_hz: u64,
    /// Signal report (dB), as decoded.
    pub snr: i32,
    /// Mode string, e.g. "FT8".
    pub mode: String,
    /// The spotted station's Maidenhead locator; empty when none was heard.
    pub grid: String,
    /// Reception time, Unix seconds (UTC).
    pub time_secs: u32,
}

/// Looks a `host:port` up. [`system_lookup`] unless a test stands in a slow one.
pub type Lookup = fn(&str) -> std::io::Result<SocketAddr>;

/// The system's own lookup: the first address `target` resolves to.
pub fn system_lookup(target: &str) -> std::io::Result<SocketAddr> {
    target
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))
}

/// Builds and sends PSK Reporter datagrams.
pub struct PskReporter {
    target: String,
    /// Per-sender random identifier echoed in every datagram header.
    observation_id: u32,
    /// The spots this reporter has sent: the next datagram's sequence number.
    sequence: AtomicU32,
    /// The sender thread's queue, one flush's datagrams at a time. The thread ends when the
    /// reporter is dropped.
    outbox: std::sync::mpsc::Sender<Vec<Vec<u8>>>,
}

impl PskReporter {
    /// New reporter sending to [`DEFAULT_TARGET`].
    pub fn new() -> Self {
        Self::with_target(DEFAULT_TARGET)
    }

    /// New reporter sending to a custom `host:port`.
    pub fn with_target(target: &str) -> Self {
        Self::with_lookup(target, system_lookup)
    }

    /// New reporter sending to `target`, looked up with `lookup` on the reporter's own thread.
    pub fn with_lookup(target: &str, lookup: Lookup) -> Self {
        let (outbox, queue) = std::sync::mpsc::channel::<Vec<Vec<u8>>>();
        let to = target.to_string();
        // A thread that cannot start leaves the queue with no reader: every send then reports an
        // error and nothing goes, as when the network is down.
        let _ = std::thread::Builder::new()
            .name("pskreporter".into())
            .spawn(move || {
                for flush in queue {
                    let _ = send_to(&to, lookup, &flush);
                }
            });
        Self {
            target: target.to_string(),
            observation_id: derive_observation_id(),
            sequence: AtomicU32::new(0),
            outbox,
        }
    }

    /// The configured target address string.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Build the datagrams for a batch of spots without sending: as many as it takes to keep each
    /// within [`MAX_DATAGRAM_BYTES`], the spots in order, each datagram numbered by the spots this
    /// reporter has sent before it. Exposed so the structure can be unit-tested and so callers
    /// can inspect/log it.
    ///
    /// Returns none if there are no spots (PSK Reporter expects at least one sender record).
    pub fn build_datagrams(
        &self,
        rx_call: &str,
        rx_grid: &str,
        software: &str,
        spots: &[Spot],
        export_time: u32,
    ) -> Vec<Vec<u8>> {
        // Every datagram describes itself: both templates, then the receiver record.
        let mut head = rx_template_set();
        head.extend_from_slice(&tx_template_set());
        head.extend_from_slice(&rx_data_set(rx_call, rx_grid, software));

        let mut sequence = self.sequence.load(Ordering::Relaxed);
        let (mut dgrams, mut records, mut count) = (Vec::new(), Vec::new(), 0u32);
        for spot in spots {
            let record = spot_record(spot);
            // A full datagram goes as it is; a spot always fits an empty one, every field
            // being cut to WSJT-X's limits.
            if count > 0
                && 16 + head.len() + align4(4 + records.len() + record.len()) > MAX_DATAGRAM_BYTES
            {
                dgrams.push(self.datagram(&head, &records, sequence, export_time));
                sequence = sequence.wrapping_add(count);
                records.clear();
                count = 0;
            }
            records.extend_from_slice(&record);
            count += 1;
        }
        if count > 0 {
            dgrams.push(self.datagram(&head, &records, sequence, export_time));
        }
        dgrams
    }

    /// One datagram: the header, then `head`'s sets and `records` as one sender data set.
    fn datagram(&self, head: &[u8], records: &[u8], sequence: u32, export_time: u32) -> Vec<u8> {
        let sender_set = data_set(TEMPLATE_TX, records);
        let total_len = 16 + head.len() + sender_set.len();
        debug_assert!(total_len <= MAX_DATAGRAM_BYTES, "{total_len} bytes");
        let mut dgram = Vec::with_capacity(total_len);
        dgram.extend_from_slice(&IPFIX_VERSION.to_be_bytes());
        dgram.extend_from_slice(&(total_len as u16).to_be_bytes());
        dgram.extend_from_slice(&export_time.to_be_bytes());
        dgram.extend_from_slice(&sequence.to_be_bytes());
        dgram.extend_from_slice(&self.observation_id.to_be_bytes());
        dgram.extend_from_slice(head);
        dgram.extend_from_slice(&sender_set);
        dgram
    }

    /// Build the datagrams for `spots` and hand them to the sender thread, which looks the server
    /// up once and sends them over UDP. Never waits on DNS or the network.
    ///
    /// Uses the current system time as the export time. Returns the datagrams' total length once
    /// queued, or an error if the sender thread has gone. Does nothing (returns `Ok(0)`) if
    /// `spots` is empty.
    pub fn send_spots(
        &self,
        rx_call: &str,
        rx_grid: &str,
        software: &str,
        spots: &[Spot],
    ) -> std::io::Result<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        let dgrams = self.build_datagrams(rx_call, rx_grid, software, spots, now);
        if dgrams.is_empty() {
            return Ok(0);
        }
        let n = dgrams.iter().map(Vec::len).sum();
        self.outbox.send(dgrams).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the PSK Reporter sender has stopped",
            )
        })?;
        self.sequence
            .fetch_add(spots.len() as u32, Ordering::Relaxed);
        Ok(n)
    }
}

/// WSJT-X's one-spot-per-call cache (`PSKReporter::addRemoteStation`, Network/PSKReporter.cpp:
/// 382-397 and its sweep at 414-419, WSJT-X 3.0.2): a call is spotted again only once more than
/// five minutes have passed since it last was, except above 49 MHz, where every decode is
/// spotted. Keyed on the call alone, as WSJT-X's is, so a QSY does not reset it. WSJT-X also lets
/// every spot through for six hours either side of a solar eclipse, from a date list it ships
/// (`eclipse.txt`, for HamSCI); that exemption is not here.
#[derive(Debug, Default)]
pub struct SpotCache {
    /// When each call was last spotted, Unix seconds.
    last: HashMap<String, u64>,
}

impl SpotCache {
    /// A call spotted this many seconds ago or less is not spotted again (`CACHE_TIMEOUT`).
    const REPEAT_SECS: u64 = 300;
    /// Above this frequency, in hertz, every decode is spotted.
    const EVERY_DECODE_ABOVE_HZ: u64 = 49_000_000;
    /// A call not spotted for longer than this is forgotten.
    const FORGET_SECS: u64 = 600;

    /// Whether a spot of `call` heard on `freq_hz` at `now` (Unix seconds) goes to PSK Reporter.
    /// When it goes, its time is noted.
    pub fn admit(&mut self, call: &str, freq_hz: u64, now: u64) -> bool {
        let due = self
            .last
            .get(call)
            .is_none_or(|&t| now.saturating_sub(t) > Self::REPEAT_SECS);
        let admit = due || freq_hz > Self::EVERY_DECODE_ABOVE_HZ;
        if admit {
            self.last.insert(call.to_string(), now);
        }
        self.last
            .retain(|_, &mut t| now.saturating_sub(t) <= Self::FORGET_SECS);
        admit
    }
}

impl Default for PskReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Look `target` up and send a flush's datagrams there, from one port of the address's own family.
/// The sender thread's work; the address is looked up once a flush, as a send to a host name was.
fn send_to(target: &str, lookup: Lookup, flush: &[Vec<u8>]) -> std::io::Result<()> {
    let addr = lookup(target)?;
    let bind = if addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind)?;
    for dgram in flush {
        let _ = socket.send_to(dgram, addr);
    }
    Ok(())
}

/// Round `n` up to the next multiple of 4 (IPFIX sets/records are 4-byte aligned).
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Pad `buf` with NULs to a 4-byte boundary.
fn pad4(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

/// Append a single-byte-length-prefixed string (PSK Reporter variable-length IE), cut to `limit`
/// bytes and never inside a character, as WSJT-X's `boundedUtf8` cuts it.
fn push_str(buf: &mut Vec<u8>, s: &str, limit: usize) {
    let mut len = s.len().min(limit);
    while !s.is_char_boundary(len) {
        len -= 1;
    }
    buf.push(len as u8);
    buf.extend_from_slice(&s.as_bytes()[..len]);
}

/// Wrap record `body` in a data set with `set_id`, padding to 4 bytes.
fn data_set(set_id: u16, body: &[u8]) -> Vec<u8> {
    let set_len = align4(4 + body.len());
    let mut s = Vec::with_capacity(set_len);
    s.extend_from_slice(&set_id.to_be_bytes());
    s.extend_from_slice(&(set_len as u16).to_be_bytes());
    s.extend_from_slice(body);
    pad4(&mut s);
    s
}

/// The receiver-information template definition (a Template/Options set).
///
/// Declares: receiverCallsign, receiverLocator, decodingSoftware — the three
/// strings in the receiver data record. Field ids use PSK Reporter's enterprise
/// number (30351). The exact IE ids mirror WSJT-X; the server keys on the field
/// *list*, so the declaration and data record stay in lockstep.
fn rx_template_set() -> Vec<u8> {
    const ENTERPRISE: u32 = 30351;
    // (information-element id, length=0xFFFF means variable-length string)
    let fields: [(u16, u16); 3] = [
        (0x8002, 0xFFFF), // receiverCallsign
        (0x8004, 0xFFFF), // receiverLocator
        (0x8008, 0xFFFF), // decodingSoftware
    ];
    options_template_set(TEMPLATE_RX, ENTERPRISE, &fields, /*scope*/ 0)
}

/// The sender/spot template definition.
///
/// Declares: senderCallsign, frequency, snr, mode, senderLocator, informationSource,
/// flowStartSeconds — WSJT-X's and JS8Call's field list and order.
fn tx_template_set() -> Vec<u8> {
    const ENTERPRISE: u32 = 30351;
    // For standard IEs (no enterprise bit) we still emit enterprise ids to keep
    // a single code path; PSK Reporter accepts the WSJT-X layout.
    let fields: [(u16, u16); 7] = [
        (0x8001, 0xFFFF), // senderCallsign (variable string)
        (0x8005, 4),      // frequency (u32 Hz; WSJT-X uses up to 8, see note)
        (0x8006, 1),      // snr (i8)
        (0x800A, 0xFFFF), // mode (variable string; 0x8007 is iMD, NOT mode)
        (0x8003, 0xFFFF), // senderLocator (variable string, empty when unknown)
        (0x800B, 1),      // informationSource (u8)
        (150, 4),         // flowStartSeconds (standard IE 150, u32)
    ];
    plain_template_set(TEMPLATE_TX, ENTERPRISE, &fields)
}

/// Encode one template field. A field with an enterprise number sets the high
/// bit of the IE id and appends the 4-byte enterprise number.
fn push_template_field(buf: &mut Vec<u8>, ie_id: u16, len: u16, enterprise: u32) {
    if ie_id & 0x8000 != 0 {
        buf.extend_from_slice(&ie_id.to_be_bytes());
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(&enterprise.to_be_bytes());
    } else {
        buf.extend_from_slice(&ie_id.to_be_bytes());
        buf.extend_from_slice(&len.to_be_bytes());
    }
}

/// Build a plain Template set (set id 2) declaring `fields` under `template_id`.
fn plain_template_set(template_id: u16, enterprise: u32, fields: &[(u16, u16)]) -> Vec<u8> {
    let mut rec = Vec::new();
    rec.extend_from_slice(&template_id.to_be_bytes());
    rec.extend_from_slice(&(fields.len() as u16).to_be_bytes());
    for &(ie, len) in fields {
        push_template_field(&mut rec, ie, len, enterprise);
    }
    finish_set(SET_ID_TEMPLATE, rec)
}

/// Build an Options-Template set (set id 3) declaring `fields` under
/// `template_id`, with `scope` leading scope fields.
fn options_template_set(
    template_id: u16,
    enterprise: u32,
    fields: &[(u16, u16)],
    scope: u16,
) -> Vec<u8> {
    let mut rec = Vec::new();
    rec.extend_from_slice(&template_id.to_be_bytes());
    rec.extend_from_slice(&(fields.len() as u16).to_be_bytes());
    rec.extend_from_slice(&scope.to_be_bytes());
    for &(ie, len) in fields {
        push_template_field(&mut rec, ie, len, enterprise);
    }
    finish_set(SET_ID_OPTIONS_TEMPLATE, rec)
}

/// Wrap a template-record body in its set header (set id + length) and pad.
fn finish_set(set_id: u16, body: Vec<u8>) -> Vec<u8> {
    let set_len = align4(4 + body.len());
    let mut s = Vec::with_capacity(set_len);
    s.extend_from_slice(&set_id.to_be_bytes());
    s.extend_from_slice(&(set_len as u16).to_be_bytes());
    s.extend_from_slice(&body);
    pad4(&mut s);
    s
}

/// The receiver data record (our call/grid/software), as a data set keyed to
/// the receiver template id.
fn rx_data_set(rx_call: &str, rx_grid: &str, software: &str) -> Vec<u8> {
    let mut rec = Vec::new();
    push_str(&mut rec, rx_call, CALLSIGN_LIMIT);
    push_str(&mut rec, rx_grid, LOCATOR_LIMIT);
    push_str(&mut rec, software, PROGRAM_INFO_LIMIT);
    data_set(TEMPLATE_RX, &rec)
}

/// `informationSource` 1: "automatically extracted" from a decode (`REPORTER_SOURCE_AUTOMATIC`),
/// the value WSJT-X and JS8Call send for every spot.
const SOURCE_AUTOMATIC: u8 = 1;

/// One spot's record, in the sender template's field order; a datagram's records go under one
/// data set keyed to the sender template id.
fn spot_record(spot: &Spot) -> Vec<u8> {
    let mut rec = Vec::new();
    push_str(&mut rec, &spot.call, CALLSIGN_LIMIT);
    rec.extend_from_slice(&(spot.freq_hz as u32).to_be_bytes());
    rec.push(clamp_i8(spot.snr) as u8);
    push_str(&mut rec, &spot.mode, MODE_LIMIT);
    push_str(&mut rec, &spot.grid, LOCATOR_LIMIT);
    rec.push(SOURCE_AUTOMATIC);
    rec.extend_from_slice(&spot.time_secs.to_be_bytes());
    rec
}

/// Clamp an SNR into the i8 range PSK Reporter expects.
fn clamp_i8(v: i32) -> i8 {
    v.clamp(i8::MIN as i32, i8::MAX as i32) as i8
}

/// Derive a stable-ish per-process random identifier from the wall clock. PSK
/// Reporter only needs it to be reasonably unique per sender; any 32-bit value
/// works.
fn derive_observation_id() -> u32 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mix the low/high words so we don't just get the seconds.
    (n as u32) ^ ((n >> 32) as u32) | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spots() -> Vec<Spot> {
        vec![
            Spot {
                call: "W1AW".to_string(),
                freq_hz: 14_074_000,
                snr: -7,
                mode: "FT8".to_string(),
                grid: String::new(),
                time_secs: 1_577_836_800,
            },
            Spot {
                call: "VE3ABC".to_string(),
                freq_hz: 14_074_500,
                snr: -15,
                mode: "JS8".to_string(),
                grid: "FN04TV".to_string(),
                time_secs: 1_577_836_815,
            },
        ]
    }

    fn read_u16(b: &[u8], off: usize) -> u16 {
        u16::from_be_bytes([b[off], b[off + 1]])
    }
    fn read_u32(b: &[u8], off: usize) -> u32 {
        u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
    }

    #[test]
    fn tx_template_declares_the_pskreporter_mode_field() {
        // Field ids per PSK Reporter's registry (enterprise 30351), as used by
        // WSJT-X's PSKReporter.cpp: 1=senderCallsign, 5=frequency, 6=sNR,
        // 10=mode, 3=senderLocator, 11=informationSource, plus standard IE
        // 150=flowStartSeconds. Field 7 is iMD — a PSK31 distortion metric;
        // declaring the mode string there leaves the report modeless and PSK
        // Reporter displays its default (PSK31).
        let set = tx_template_set();
        assert_eq!(read_u16(&set, 0), SET_ID_TEMPLATE);
        assert_eq!(read_u16(&set, 4), TEMPLATE_TX);
        let n = read_u16(&set, 6) as usize;
        let mut ids = Vec::new();
        let mut off = 8;
        for _ in 0..n {
            let ie = read_u16(&set, off);
            ids.push(ie);
            off += 4; // ie id + length
            if ie & 0x8000 != 0 {
                off += 4; // enterprise number
            }
        }
        assert_eq!(
            ids,
            vec![0x8001, 0x8005, 0x8006, 0x800A, 0x8003, 0x800B, 150]
        );
    }

    #[test]
    fn empty_spots_produce_no_datagram() {
        let r = PskReporter::new();
        assert!(r
            .build_datagrams("KD9TAW", "EN52", "Tempo 0.1", &[], 0)
            .is_empty());
    }

    #[test]
    fn header_is_well_formed() {
        let r = PskReporter::with_target("127.0.0.1:4739");
        let dgram = r
            .build_datagrams(
                "KD9TAW",
                "EN52",
                "Tempo 0.1",
                &sample_spots(),
                1_577_836_900,
            )
            .remove(0);

        assert_eq!(read_u16(&dgram, 0), IPFIX_VERSION);
        // Length field equals the actual datagram length.
        assert_eq!(read_u16(&dgram, 2) as usize, dgram.len());
        // Export time we passed in.
        assert_eq!(read_u32(&dgram, 4), 1_577_836_900);
        // The spots this reporter sent before it: none yet, WSJT-X's count starting at 0.
        assert_eq!(read_u32(&dgram, 8), 0);
        // Observation id is non-zero (derive_observation_id ORs in 1).
        assert_ne!(read_u32(&dgram, 12), 0);
        // Whole datagram is 4-byte aligned.
        assert_eq!(dgram.len() % 4, 0);
    }

    #[test]
    fn contains_both_templates_and_two_data_sets() {
        let r = PskReporter::new();
        let dgram = r
            .build_datagrams("KD9TAW", "EN52", "Tempo", &sample_spots(), 100)
            .remove(0);

        // Walk the sets after the 16-byte header, collecting their ids.
        let mut off = 16;
        let mut set_ids = Vec::new();
        while off + 4 <= dgram.len() {
            let set_id = read_u16(&dgram, off);
            let set_len = read_u16(&dgram, off + 2) as usize;
            assert!(set_len >= 4, "set length must include the 4-byte header");
            assert_eq!(set_len % 4, 0, "every set is 4-byte aligned");
            set_ids.push(set_id);
            off += set_len;
        }
        assert_eq!(off, dgram.len(), "sets exactly tile the datagram");

        // Two template-definition sets (ids 2 and 3) then two data sets keyed to
        // the receiver and sender template ids.
        assert!(set_ids.contains(&SET_ID_OPTIONS_TEMPLATE)); // rx template
        assert!(set_ids.contains(&SET_ID_TEMPLATE)); // tx template
        assert!(set_ids.contains(&TEMPLATE_RX)); // rx data set
        assert!(set_ids.contains(&TEMPLATE_TX)); // tx data set
    }

    #[test]
    fn rx_data_set_carries_call_grid_software() {
        let r = PskReporter::new();
        let dgram = r
            .build_datagrams("KD9TAW", "EN52", "Tempo 0.1", &sample_spots(), 100)
            .remove(0);
        // The receiver data set is keyed to TEMPLATE_RX; find it and check the
        // length-prefixed strings appear in order.
        let bytes = find_data_set(&dgram, TEMPLATE_RX);
        let body = &bytes[4..]; // strip set header
        let mut off = 0;
        assert_eq!(read_str(body, &mut off), "KD9TAW");
        assert_eq!(read_str(body, &mut off), "EN52");
        assert_eq!(read_str(body, &mut off), "Tempo 0.1");
    }

    #[test]
    fn tx_data_set_carries_each_spot() {
        let r = PskReporter::new();
        let spots = sample_spots();
        let dgram = r
            .build_datagrams("KD9TAW", "EN52", "Tempo", &spots, 100)
            .remove(0);
        let bytes = find_data_set(&dgram, TEMPLATE_TX);
        let body = &bytes[4..];
        let mut off = 0;
        // First spot: no locator heard, so an empty string, as WSJT-X sends one.
        assert_eq!(read_str(body, &mut off), "W1AW");
        assert_eq!(read_u32(body, off), 14_074_000);
        off += 4;
        assert_eq!(body[off] as i8, -7);
        off += 1;
        assert_eq!(read_str(body, &mut off), "FT8");
        assert_eq!(read_str(body, &mut off), "");
        assert_eq!(body[off], 1, "informationSource: automatically extracted");
        off += 1;
        assert_eq!(read_u32(body, off), 1_577_836_800);
        off += 4;
        // Second spot, with its locator.
        assert_eq!(read_str(body, &mut off), "VE3ABC");
        assert_eq!(read_u32(body, off), 14_074_500);
        off += 4;
        assert_eq!(body[off] as i8, -15);
        off += 1;
        assert_eq!(read_str(body, &mut off), "JS8");
        assert_eq!(read_str(body, &mut off), "FN04TV");
        assert_eq!(body[off], 1);
        off += 1;
        assert_eq!(read_u32(body, off), 1_577_836_815);
        off += 4;
        // Nothing after the last record but the set's NUL padding.
        assert!(body[off..].iter().all(|&b| b == 0));
    }

    #[test]
    fn send_spots_over_loopback_succeeds() {
        // Bind a loopback listener and send to it (no real network egress).
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let r = PskReporter::with_target(&addr.to_string());
        let n = r
            .send_spots("KD9TAW", "EN52", "Tempo", &sample_spots())
            .unwrap();
        assert!(n > 16);

        let mut buf = [0u8; 4096];
        let (recv_n, _) = listener.recv_from(&mut buf).unwrap();
        assert_eq!(recv_n, n);
        assert_eq!(read_u16(&buf, 0), IPFIX_VERSION);
        assert_eq!(read_u16(&buf, 2) as usize, recv_n);
    }

    #[test]
    fn send_spots_empty_is_noop() {
        let r = PskReporter::with_target("127.0.0.1:1"); // unused
        assert_eq!(r.send_spots("KD9TAW", "EN52", "Tempo", &[]).unwrap(), 0);
    }

    /// A busy band's flush reaches the server whole. Each five-minute flush went as ONE datagram:
    /// ~80 decodes a period over the 40 FT4 periods of a flush is 3,200 spots, near 90 KB, past
    /// the 65,507 bytes a UDP datagram can carry (and its u16 length field wrapped), so the whole
    /// flush was lost. WSJT-X 3.0.2 splits a report into datagrams of at most
    /// `maxUdpIpfixPayloadBytes()`, 952 bytes, each numbered by the spots sent before it
    /// (Network/PSKReporter.cpp:302-325; PSKReporterIPFIX.cpp:9-12, 234-292).
    #[test]
    fn a_busy_bands_flush_reaches_the_server_whole_in_wsjtx_sized_datagrams() {
        static LOOKUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn counted(target: &str) -> std::io::Result<SocketAddr> {
            LOOKUPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            system_lookup(target)
        }
        let suffix = |n: usize| -> String {
            [n / 676 % 26, n / 26 % 26, n % 26]
                .iter()
                .map(|&c| (b'A' + c as u8) as char)
                .collect()
        };
        let spots: Vec<Spot> = (0..3200)
            .map(|i| Spot {
                call: format!("K{}{}", i % 10, suffix(i / 10)),
                freq_hz: 14_080_000 + (i % 3000) as u64,
                snr: -10,
                mode: "FT4".into(),
                grid: "FN42".into(),
                time_secs: 1_700_000_000,
            })
            .collect();
        let socket =
            socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None).unwrap();
        // Room for the whole burst, so the test measures what was sent, not the kernel's queue.
        socket.set_recv_buffer_size(4 << 20).unwrap();
        socket
            .bind(&SocketAddr::from(([127, 0, 0, 1], 0)).into())
            .unwrap();
        let listener: UdpSocket = socket.into();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let r = PskReporter::with_lookup(&listener.local_addr().unwrap().to_string(), counted);
        let receive = |n: usize| {
            let (mut got, mut bytes, mut buf) = (Vec::new(), 0, vec![0u8; 65_536]);
            while bytes < n {
                match listener.recv_from(&mut buf) {
                    Ok((k, _)) => {
                        bytes += k;
                        got.push(buf[..k].to_vec());
                    }
                    Err(_) => break,
                }
            }
            got
        };
        let first = receive(r.send_spots("KD9TAW", "EN52", "Nexus", &spots).unwrap());
        let (mut heard, mut faults) = (Vec::new(), Vec::new());
        for (k, d) in first.iter().enumerate() {
            if read_u16(d, 0) != IPFIX_VERSION || read_u16(d, 2) as usize != d.len() {
                faults.push(format!(
                    "{k}: header length {} on {} bytes",
                    read_u16(d, 2),
                    d.len()
                ));
            }
            if d.len() > 952 || !d.len().is_multiple_of(4) {
                faults.push(format!("{k}: {} bytes", d.len()));
            }
            if read_u32(d, 8) as usize != heard.len() {
                faults.push(format!(
                    "{k}: sequence {} after {} spots",
                    read_u32(d, 8),
                    heard.len()
                ));
            }
            // Each datagram describes itself, so one lost on the way orphans no other.
            let sets = set_ids(d);
            if sets
                != [
                    SET_ID_OPTIONS_TEMPLATE,
                    SET_ID_TEMPLATE,
                    TEMPLATE_RX,
                    TEMPLATE_TX,
                ]
            {
                faults.push(format!("{k}: sets {sets:x?}"));
            }
            heard.extend(sender_records(d).into_iter().map(|(call, ..)| call));
        }
        assert_eq!(faults, Vec::<String>::new());
        assert_eq!(heard.len(), spots.len(), "spots received");
        let first_difference = heard
            .iter()
            .zip(&spots)
            .position(|(call, spot)| *call != spot.call);
        assert_eq!(first_difference, None, "each spot once, in order");
        assert_eq!(
            LOOKUPS.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "lookups for the flush"
        );
        // The next flush carries on the count.
        let next = receive(
            r.send_spots("KD9TAW", "EN52", "Nexus", &spots[..2])
                .unwrap(),
        );
        let sequences: Vec<u32> = next.iter().map(|d| read_u32(d, 8)).collect();
        assert_eq!(sequences, [3200]);
    }

    /// A field longer than WSJT-X sends is cut where WSJT-X cuts it (PSKReporterIPFIX.cpp:16-19,
    /// `boundedUtf8` :34-49): a callsign at 32 bytes, a locator and a mode at 16, the software's
    /// name at 80, and never inside a UTF-8 character. So no field can push a datagram past 952
    /// bytes.
    #[test]
    fn a_field_longer_than_wsjtx_sends_is_cut_where_wsjtx_cuts_it() {
        let spot = Spot {
            call: "W".repeat(40),
            freq_hz: 14_074_000,
            snr: -7,
            mode: "M".repeat(20),
            grid: "G".repeat(20),
            time_secs: 1_700_000_000,
        };
        // Seventy-nine bytes, then a two-byte é across the 80-byte limit.
        let software = format!("{}é", "N".repeat(79));
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let r = PskReporter::with_target(&listener.local_addr().unwrap().to_string());
        r.send_spots(&"R".repeat(40), &"L".repeat(20), &software, &[spot])
            .unwrap();
        let mut buf = [0u8; 2048];
        let (k, _) = listener.recv_from(&mut buf).unwrap();
        let rx = find_data_set(&buf[..k], TEMPLATE_RX);
        let mut off = 0;
        let receiver = [
            read_str(&rx[4..], &mut off),
            read_str(&rx[4..], &mut off),
            read_str(&rx[4..], &mut off),
        ];
        assert_eq!(receiver, ["R".repeat(32), "L".repeat(16), "N".repeat(79)]);
        assert_eq!(
            sender_records(&buf[..k]),
            [("W".repeat(32), "M".repeat(16), "G".repeat(16))]
        );
    }

    /// The spot cache forgets a call not spotted for over ten minutes, as WSJT-X's sweep does
    /// (Network/PSKReporter.cpp:414-419), so it holds no more than ten minutes of calls.
    #[test]
    fn the_spot_cache_forgets_a_call_after_ten_minutes() {
        let mut cache = SpotCache::default();
        let held = |c: &SpotCache| {
            let mut calls: Vec<String> = c.last.keys().cloned().collect();
            calls.sort();
            calls
        };
        assert!(cache.admit("W1AW", 14_074_000, 1_000));
        assert!(cache.admit("K1ABC", 14_074_000, 1_300));
        assert!(cache.admit("K2BBB", 14_074_000, 1_600));
        assert_eq!(
            held(&cache),
            ["K1ABC", "K2BBB", "W1AW"],
            "600 s on, W1AW is held"
        );
        assert!(cache.admit("K3CCC", 14_074_000, 1_601));
        assert_eq!(
            held(&cache),
            ["K1ABC", "K2BBB", "K3CCC"],
            "601 s on, it is gone"
        );
    }

    // --- test helpers ---

    /// The ids of the sets in `dgram`, in order; empty unless they tile it exactly.
    fn set_ids(dgram: &[u8]) -> Vec<u16> {
        let (mut off, mut ids) = (16, Vec::new());
        while off + 4 <= dgram.len() {
            let len = read_u16(dgram, off + 2) as usize;
            if len < 4 {
                return Vec::new();
            }
            ids.push(read_u16(dgram, off));
            off += len;
        }
        if off == dgram.len() {
            ids
        } else {
            Vec::new()
        }
    }

    /// The sender records of `dgram`, each (call, mode, locator), read by the template's layout.
    fn sender_records(dgram: &[u8]) -> Vec<(String, String, String)> {
        let set = find_data_set(dgram, TEMPLATE_TX);
        let body = &set[4..];
        let (mut off, mut out) = (0, Vec::new());
        while off < body.len() && body[off] != 0 {
            let call = read_str(body, &mut off);
            off += 4 + 1; // frequency, SNR
            let mode = read_str(body, &mut off);
            let locator = read_str(body, &mut off);
            off += 1 + 4; // information source, time
            out.push((call, mode, locator));
        }
        out
    }

    /// Find the data set whose set id is `template_id`, returning its bytes
    /// (including the 4-byte set header).
    fn find_data_set(dgram: &[u8], template_id: u16) -> Vec<u8> {
        let mut off = 16;
        while off + 4 <= dgram.len() {
            let set_id = read_u16(dgram, off);
            let set_len = read_u16(dgram, off + 2) as usize;
            if set_id == template_id {
                return dgram[off..off + set_len].to_vec();
            }
            off += set_len;
        }
        panic!("data set {template_id:#06x} not found");
    }

    /// Read a single-byte-length-prefixed string at `*off`, advancing it.
    fn read_str(body: &[u8], off: &mut usize) -> String {
        let len = body[*off] as usize;
        *off += 1;
        let s = String::from_utf8_lossy(&body[*off..*off + len]).into_owned();
        *off += len;
        s
    }

    /// THE NAME EVERY OTHER OPERATOR SEES. PSKReporter publishes the reporting software on
    /// pskreporter.info, so this string is the app's most public identifier — and it is the one
    /// the rename missed. Nexus users showed up as "Tempo" to the whole network until an
    /// operator noticed (2026-08-26).
    ///
    /// The datagram is checked here rather than the call site because this is where the bytes
    /// are built: a caller passing the wrong name is the bug that happened, and a caller passing
    /// the RIGHT name into a builder that mangled it would look identical from outside.
    #[test]
    fn the_reporting_software_name_reaches_the_wire_intact() {
        let r = PskReporter::new();
        let dgram = r
            .build_datagrams("KD9TAW", "EN52", "Nexus", &sample_spots(), 100)
            .into_iter()
            .next()
            .expect("a datagram is built");
        // The name must appear in the RX data set, byte for byte.
        let needle = b"Nexus";
        assert!(
            dgram.windows(needle.len()).any(|w| w == needle),
            "the software name is not in the datagram at all"
        );
        // …and the old one must not survive anywhere in it.
        let stale = b"Tempo";
        assert!(
            !dgram.windows(stale.len()).any(|w| w == stale),
            "the pre-rename name is still on the wire"
        );
    }
}
