//! JS8's internet links — the engine half of JS8Call 3.0.3's spotting: stations heard on JS8 go to
//! PSK Reporter and JS8NET, commands to a group go to JS8NET, the `@APRSIS` gateway's lines go to
//! APRS-IS, and a never-spot list keeps calls off all three. A CHILD module of `engine` (declared
//! with `#[path]` there, like `js8`), so it reads the engine's private fields with no visibility
//! widening.
//!
//! WHO DOES WHAT. JS8Call: `logCallActivity(…, spot = true)` → `processSpots` → `spotReport`
//! (JS8NET `RX.SPOT`) and `pskLogReport` (PSK Reporter, mode "JS8"); `spotCmd` (JS8NET
//! `RX.DIRECTED`, a command to any `@` group); `spotAprsGrid` / `spotAprsCmd` (APRS-IS). Here,
//! [`Engine::js8_links_on_event`] runs for every reassembler
//! event AFTER the station has seen it — so the heard list holds the grid JS8Call's call activity
//! would — and fills three bounded queues. The radio loop drains the PSK Reporter spots into its
//! own upload ([`Engine::take_js8_psk_spots`]: the same reporter, switch and flush as FT8's); a
//! network thread in the app drains JS8NET once a minute ([`Engine::take_js8net`]); the APRS-IS
//! bridge drains the gateway's lines onto the one APRS-IS session ([`Engine::take_js8_aprs_uplink`])
//! and asks [`Engine::js8_aprs_link`] whether that session must be up for JS8. Nothing here
//! touches a socket, a clock or a thread.
//!
//! GATES, JS8Call's: every link needs spotting on — Nexus's PSK Reporter switch as it uploads
//! (`pskreporter_uploading`, so a contest that allows spots only over RF stops all three) — and a
//! real callsign, as the iGate needs. The gateway also needs `js8_spot_aprs` (JS8Call's
//! `SpotToAPRS`, on by default). A station on `js8_spot_never` (JS8Call's `SpotBlacklist`), as
//! heard or by its base call, is sent to no network.

use std::collections::{HashMap, VecDeque};

use ::js8::proto::callsign::{is_base_call, is_compound_call, split_portable};
use ::js8::proto::reassembly::Checksum;
use ::js8::{Command, Frame, MessageEvent, Speed};
use tempo_core::aprs::js8gate;
use tempo_net::js8net::{By, Record};
use tempo_net::pskreporter::Spot;

use super::{now_unix_secs, Engine};
use crate::dto::Tier;

/// PSK Reporter's per-call-per-band cache (JS8Call `PSKReporter.cpp` `CACHE_TIMEOUT`): a call
/// re-heard on a band within the hour updates its queued spot instead of adding one.
const PSK_CACHE_SECS: u64 = 3_600;
/// Spots queued for the radio loop's next PSK Reporter flush, at most.
const PSK_QUEUE_CAP: usize = 1_000;
/// JS8NET records waiting for the minute's batch, at most (the oldest go first).
const JS8NET_QUEUE_CAP: usize = 512;
/// APRS-IS lines waiting for the session, at most.
const APRS_QUEUE_CAP: usize = 64;
/// JS8Call drops an APRS-IS line queued longer than this (`PACKET_TIMEOUT_SECONDS`).
const APRS_LINE_TTL_SECS: u64 = 300;
/// The session stays up this long after JS8 last handed it a line, so a line handed over is
/// written before the session goes.
const APRS_LINGER_SECS: u64 = 60;

/// The engine's JS8-links state: three queues and the caches that shape them. Session-only.
#[derive(Debug, Default)]
pub struct Js8Links {
    psk: Vec<Spot>,
    psk_cache: HashMap<String, u64>,
    js8net: VecDeque<(i64, Record)>,
    js8net_local: Option<(String, String, String)>,
    js8net_batches: u64,
    js8net_dial: u64,
    aprs: VecDeque<(u64, String)>,
    aprs_handoff_secs: Option<u64>,
}

/// What the APRS-IS session must do for JS8 now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Js8AprsLink {
    /// The gateway is on while JS8 runs: the login carries the passcode, as the iGate's does.
    pub armed: bool,
    /// The session must be up: a line waits, or one was just handed over.
    pub want: bool,
}

/// A callsign the app treats as real, as `is_real_call` in the app does for the iGate and the
/// cluster: 3–10 characters, a letter and a digit, nothing but letters, digits and `/`.
fn real_call(call: &str) -> bool {
    let n = call.chars().count();
    (3..=10).contains(&n)
        && call.chars().any(|c| c.is_ascii_digit())
        && call.chars().any(|c| c.is_ascii_alphabetic())
        && call.chars().all(|c| c.is_ascii_alphanumeric() || c == '/')
}

/// JS8Call's submode number for a speed (`Varicode::SubmodeType`): what JS8NET's `SPEED` carries.
fn js8call_submode(speed: Speed) -> i32 {
    match speed {
        Speed::Normal => 0,
        Speed::Fast => 1,
        Speed::Turbo => 2,
        Speed::Slow => 4,
    }
}

/// JS8Call's `Varicode::parseGrids` (`grid_pattern`, Varicode.cpp:33-34): every run of two letters
/// A–X and two digits, then any letter pairs each optionally followed by a digit pair, wherever it
/// starts; `RR73` is not a grid.
fn parse_grids(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let letters =
        |i: usize| b.len() >= i + 2 && b[i..i + 2].iter().all(|c| (b'A'..=b'X').contains(c));
    let digits = |i: usize| b.len() >= i + 2 && b[i..i + 2].iter().all(u8::is_ascii_digit);
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !(letters(i) && digits(i + 2)) {
            i += 1;
            continue;
        }
        let mut end = i + 4;
        while letters(end) {
            end += 2;
            if digits(end) {
                end += 2;
            }
        }
        let grid = &text[i..end];
        if grid != "RR73" {
            out.push(grid.to_string());
        }
        i = end;
    }
    out
}

/// The sender of a standard message's first frame, `CALL: …` (processRxActivity.cpp:85-101): the
/// text starts with a callsign followed straight by a colon.
fn first_frame_sender(text: &str) -> Option<String> {
    let (call, _) = text.split_once(':')?;
    let ok = !call.is_empty()
        && !call.contains(char::is_whitespace)
        && (is_base_call(split_portable(call).0) || is_compound_call(call))
        && parse_grids(call).first().is_none_or(|g| g != call);
    ok.then(|| call.to_string())
}

impl Engine {
    /// Spotting is on as JS8Call's SPOT is on: the PSK Reporter switch as it uploads, and a call.
    fn js8_spotting(&self) -> bool {
        self.pskreporter_uploading() && real_call(self.settings.mycall.trim())
    }

    /// The @APRSIS gateway is on (JS8Call's `spot_to_aprs()`).
    fn js8_aprs_gateway(&self) -> bool {
        self.js8_spotting() && self.settings.js8_spot_aprs
    }

    /// On the never-spot list, as heard or by its base call (JS8Call's
    /// `spot_blacklist().contains(call) || contains(Radio::base_callsign(call))`).
    fn js8_never_spot(&self, call: &str) -> bool {
        let call = call.trim().to_ascii_uppercase();
        let base = js8gate::base_callsign(&call);
        self.settings.js8_spot_never.iter().any(|c| {
            let c = c.trim();
            c.eq_ignore_ascii_case(&call) || c.eq_ignore_ascii_case(&base)
        })
    }

    /// This station as JS8NET's `BY` names it.
    fn js8net_by(&self) -> By {
        By {
            call: self.settings.mycall.trim().to_ascii_uppercase(),
            grid: self.settings.mygrid.trim().to_ascii_uppercase(),
        }
    }

    fn js8net_push(&mut self, at_ms: u64, rec: Record) {
        let q = &mut self.js8_links.js8net;
        q.push_back((at_ms as i64, rec));
        while q.len() > JS8NET_QUEUE_CAP {
            q.pop_front();
        }
    }

    fn js8_aprs_push(&mut self, line: String) {
        let q = &mut self.js8_links.aprs;
        q.push_back((now_unix_secs(), line));
        while q.len() > APRS_QUEUE_CAP {
            q.pop_front();
        }
    }

    /// One reassembler event → the links, after the station has seen it. See the module header.
    pub(crate) fn js8_links_on_event(&mut self, ev: &MessageEvent, now_ms: u64) {
        if !self.js8_spotting() {
            // JS8Call's `processSpots` empties its queue while SPOT is off: nothing heard then
            // goes out later.
            self.js8_links.psk.clear();
            return;
        }
        match ev {
            MessageEvent::Frame(rx) => {
                if let (Frame::Data { text, .. }, true) = (&rx.frame, rx.i3.first) {
                    if let Some(call) = first_frame_sender(text) {
                        self.js8_spot_heard(&call, None, rx.freq_hz, rx.snr_db, rx.speed, rx.at_ms);
                    }
                }
            }
            MessageEvent::Message(m) => {
                // A buffer whose checksum failed never becomes a command in JS8Call.
                if m.checksum == Checksum::Bad {
                    return;
                }
                let from = m.from.trim().to_ascii_uppercase();
                if from.is_empty() || from.contains('>') || from.contains("<....>") {
                    return;
                }
                let at = m.last_ms.max(now_ms);
                // Every command is the sender heard (processCommandActivity.cpp:56-69); a CQ is
                // logged twice (:69 and :750), so JS8NET hears it twice, as JS8Call sends it.
                let heard = |e: &mut Engine| {
                    e.js8_spot_heard(&from, m.grid.clone(), m.freq_hz, m.snr_db, m.speed, at)
                };
                heard(self);
                if m.cq.is_some() {
                    heard(self);
                }
                let to_aprsis = m.to_text.eq_ignore_ascii_case("@APRSIS");
                // A GRID's locators: each is the sender heard there, and to @APRSIS each is an
                // APRS-IS position too (:87-115).
                if m.cmd == Some(Command::Grid) {
                    for grid in parse_grids(&m.text) {
                        if to_aprsis {
                            self.js8_aprs_position(&from, &grid, m.freq_hz, m.snr_db);
                        }
                        self.js8_spot_heard(&from, Some(grid), m.freq_hz, m.snr_db, m.speed, at);
                    }
                }
                // A command to any group goes to JS8NET (:117-120).
                if m.to_text.starts_with('@') {
                    self.js8net_directed(m, &from, at);
                }
                // A CMD to @APRSIS is a raw APRS packet (:122-125).
                if to_aprsis && m.cmd == Some(Command::Cmd) {
                    self.js8_aprs_raw(&from, &m.text);
                }
            }
        }
    }

    /// A station heard (JS8Call's `logCallActivity` → `processSpots`): PSK Reporter and JS8NET,
    /// with the grid it was heard with, or the one heard from it before.
    fn js8_spot_heard(
        &mut self,
        call: &str,
        grid: Option<String>,
        offset_hz: f32,
        snr: i32,
        speed: Speed,
        at_ms: u64,
    ) {
        if self.js8_never_spot(call) {
            return;
        }
        let grid = grid.filter(|g| !g.is_empty()).or_else(|| {
            let key = split_portable(call).0;
            self.js8_station
                .heard()
                .iter()
                .find(|h| h.call.eq_ignore_ascii_case(key))
                .and_then(|h| h.grid.clone())
        });
        let grid = grid.unwrap_or_default();
        let dial = self.settings.dial_hz();
        // JS8Call's offset is the decoder's float truncated to whole hertz (`frequencyOffset`).
        let offset = offset_hz.max(0.0) as u64;
        self.js8_queue_psk(Spot {
            call: call.to_string(),
            freq_hz: dial + offset,
            snr,
            mode: "JS8".into(),
            grid: grid.clone(),
            time_secs: (at_ms / 1000) as u32,
        });
        let by = self.js8net_by();
        self.js8net_push(
            at_ms,
            Record::Spot {
                by,
                call: call.to_string(),
                grid,
                dial: dial as i64,
                offset: offset as i64,
                snr,
                speed: js8call_submode(speed),
            },
        );
    }

    /// PSK Reporter's queue with JS8Call's call-and-band cache (`addRemoteStation`): a call new on
    /// the band, or not queued for an hour, is a new spot; inside the hour it replaces the one
    /// still queued, or is dropped when that one has gone. (Its ±6 h eclipse exemption for HamSCI
    /// is not here.)
    fn js8_queue_psk(&mut self, spot: Spot) {
        let band = |hz: u64| crate::bandplan::band_for_dial(hz as f64 / 1e6).unwrap_or("");
        let now = now_unix_secs();
        let key = format!("{}_{}", spot.call, band(spot.freq_hz));
        let links = &mut self.js8_links;
        let fresh = links
            .psk_cache
            .get(&key)
            .is_none_or(|&t| t < now.saturating_sub(PSK_CACHE_SECS));
        if fresh {
            links.psk_cache.insert(key, now);
            links.psk.push(spot);
            if links.psk.len() > PSK_QUEUE_CAP {
                links.psk.remove(0);
            }
        } else if let Some(i) = links
            .psk
            .iter()
            .rposition(|s| s.call == spot.call && band(s.freq_hz) == band(spot.freq_hz))
        {
            links.psk[i] = spot;
            links.psk_cache.insert(key, now);
        }
        links
            .psk_cache
            .retain(|_, t| now.saturating_sub(*t) <= 2 * PSK_CACHE_SECS);
    }

    /// A command heard addressed to a group, as JS8NET's `RX.DIRECTED` (`spotCmd`).
    fn js8net_directed(&mut self, m: &::js8::proto::reassembly::Message, from: &str, at_ms: u64) {
        if self.js8_never_spot(from) {
            return;
        }
        // JS8Call's command text, left-stripped unless it is the bare-space free text: a heartbeat
        // and a CQ are the faux commands " HEARTBEAT" and " CQ".
        let cmd = match (m.cq, m.cmd) {
            (Some(_), _) => "CQ",
            (None, None) => "HEARTBEAT",
            (None, Some(c)) if c.text().trim().is_empty() => c.text(),
            (None, Some(c)) => c.text().trim_start(),
        };
        // A CQ carries its whole frame as text (`decodedtext.message()`); a heartbeat none.
        let text = match m.cq {
            Some(idx) => Frame::Heartbeat {
                call: m.from.clone(),
                grid: m.grid.clone(),
                is_cq: true,
                idx,
            }
            .render(),
            None => m.text.clone(),
        };
        let extra = match (m.cmd, m.num) {
            (Some(c), Some(n)) if c.carries_snr() => ::js8::proto::frame::format_snr(i32::from(n)),
            (_, Some(n)) => n.to_string(),
            _ => String::new(),
        };
        let dial = self.settings.dial_hz();
        let by = self.js8net_by();
        self.js8net_push(
            at_ms,
            Record::Directed {
                by,
                cmd: cmd.to_string(),
                from: from.to_string(),
                to: m.to_text.clone(),
                path: String::new(),
                text,
                grid: m.grid.clone().unwrap_or_default(),
                extra,
                dial: dial as i64,
                offset: m.freq_hz.max(0.0) as i64,
                snr: m.snr_db,
                speed: js8call_submode(m.speed),
            },
        );
    }

    /// `@APRSIS GRID` → an APRS-IS position (`spotAprsGrid`).
    fn js8_aprs_position(&mut self, from: &str, grid: &str, offset_hz: f32, snr: i32) {
        if !self.js8_aprs_gateway() || self.js8_never_spot(from) {
            return;
        }
        let freq = self.settings.dial_hz() + offset_hz.max(0.0) as u64;
        let mycall = self.settings.mycall.clone();
        if let Some(line) = js8gate::position_line(from, &mycall, grid, freq, snr) {
            self.js8_aprs_push(line);
        }
    }

    /// `@APRSIS CMD` → the raw APRS packet its text is (`spotAprsCmd`).
    fn js8_aprs_raw(&mut self, from: &str, text: &str) {
        if !self.js8_aprs_gateway() || self.js8_never_spot(from) {
            return;
        }
        let mycall = self.settings.mycall.clone();
        if let Some(line) = js8gate::raw_line(from, &mycall, text) {
            self.js8_aprs_push(line);
        }
    }

    /// The spots heard on JS8 since the last call, for the radio loop's PSK Reporter upload.
    pub fn take_js8_psk_spots(&mut self) -> Vec<Spot> {
        std::mem::take(&mut self.js8_links.psk)
    }

    /// The minute's JS8NET batch, serialised: `RX.LOCAL` first when this station has changed (or
    /// it has not gone since JS8 started, or on JS8Call's every-fifteenth-minute dial change), then
    /// every record queued. Empty, and the queue dropped, unless JS8 runs with spotting on.
    /// `version` is the app's name and version, `Nexus 1.19.0`. Call once a minute (JS8Call's
    /// `SEND_INTERVAL`).
    pub fn take_js8net(&mut self, version: &str) -> Vec<String> {
        if self.tier() != Tier::Js8 || !self.js8_spotting() {
            self.js8_links.js8net.clear();
            self.js8_links.js8net_local = None;
            return Vec::new();
        }
        let now_ms = now_unix_secs() * 1000;
        let by = self.js8net_by();
        let local = (
            by.call.clone(),
            by.grid.clone(),
            self.js8_station.info_text(),
        );
        let dial = self.settings.dial_hz();
        let links = &mut self.js8_links;
        let mut out = Vec::new();
        let changed = links.js8net_local.as_ref() != Some(&local);
        let dial_moved = links.js8net_dial != dial && links.js8net_batches.is_multiple_of(15);
        if changed || dial_moved {
            out.push(
                Record::Local {
                    call: local.0.clone(),
                    grid: local.1.clone(),
                    info: local.2.clone(),
                    version: version.to_string(),
                }
                .to_json(now_ms as i64),
            );
            links.js8net_local = Some(local);
        }
        links.js8net_dial = dial;
        links.js8net_batches += 1;
        out.extend(links.js8net.drain(..).map(|(at, r)| r.to_json(at)));
        out
    }

    /// The gateway's lines for the APRS-IS session, oldest first; any queued longer than five
    /// minutes is dropped, as JS8Call drops it. Call only with a VERIFIED login.
    pub fn take_js8_aprs_uplink(&mut self, now_secs: u64) -> Vec<String> {
        let links = &mut self.js8_links;
        let out: Vec<String> = links
            .aprs
            .drain(..)
            .filter(|(at, _)| now_secs.saturating_sub(*at) <= APRS_LINE_TTL_SECS)
            .map(|(_, line)| line)
            .collect();
        if !out.is_empty() {
            links.aprs_handoff_secs = Some(now_secs);
        }
        out
    }

    /// What the APRS-IS session must do for JS8 now (see [`Js8AprsLink`]). The gateway is armed
    /// only while JS8 runs; waiting lines expire after five minutes, so a session nobody can
    /// verify is not kept up for them.
    pub fn js8_aprs_link(&mut self, now_secs: u64) -> Js8AprsLink {
        let armed = self.tier() == Tier::Js8 && self.js8_aprs_gateway();
        if !armed {
            self.js8_links.aprs.clear();
            return Js8AprsLink::default();
        }
        self.js8_links
            .aprs
            .retain(|(at, _)| now_secs.saturating_sub(*at) <= APRS_LINE_TTL_SECS);
        let lingering = self
            .js8_links
            .aprs_handoff_secs
            .is_some_and(|t| now_secs.saturating_sub(t) < APRS_LINGER_SECS);
        Js8AprsLink {
            armed,
            want: lingering || !self.js8_links.aprs.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::js8::proto::compose::frames;
    use ::js8::proto::frame::encode_frame;
    use ::js8::I3;
    use modes::Js8Speed;

    /// A decode row exactly as `Js8Mode::decode_frame` emits one for `frame`.
    fn row(frame: &Frame, i3: I3, freq: f32, snr: i32) -> modes::Decode {
        let word = encode_frame(frame, i3, Js8Speed::Normal).expect("packable");
        modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr,
            dt: 0.1,
            freq,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 {
                speed: Js8Speed::Normal,
            }),
            raw: Some(*word.as_bytes()),
        }
    }

    /// Every frame of `text` sent by `from`, as rows at `freq`.
    fn sent(from: &str, text: &str, freq: f32, snr: i32) -> Vec<modes::Decode> {
        frames(from, None, text, Js8Speed::Normal)
            .expect("composable")
            .iter()
            .map(|(f, i3)| row(f, *i3, freq, snr))
            .collect()
    }

    fn beacon(call: &str, grid: &str, cq: bool, freq: f32) -> modes::Decode {
        let f = Frame::Heartbeat {
            call: call.into(),
            grid: Some(grid.into()),
            is_cq: cq,
            idx: 0,
        };
        let whole = I3 {
            first: true,
            last: true,
            data: false,
        };
        row(&f, whole, freq, -7)
    }

    /// `K1ABC/P: KD9TAW SNR?` — one directed frame whose sender is portable.
    fn portable_query(freq: f32) -> modes::Decode {
        let f = Frame::Directed {
            from: ::js8::proto::callsign::CallRef::Base("K1ABC".into()),
            to: ::js8::proto::callsign::CallRef::Base("KD9TAW".into()),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: true,
            portable_to: false,
        };
        let whole = I3 {
            first: true,
            last: true,
            data: false,
        };
        row(&f, whole, freq, -3)
    }

    /// JS8 on 20 m at 14.078 MHz, KD9TAW in EN52, every switch at its default (PSK Reporter on).
    fn links_engine() -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.set_frequency(14.078, "20m", "USB");
        e
    }

    /// The traffic the tests hear, in order: W1AW's heartbeat from FN31; K1ABC's CQ from EM73; a
    /// directed SNR? from N0XYZ to me; VE3QQQ's plain message; W1AW's GRID to @ALLCALL.
    fn hear_the_band(e: &mut Engine) {
        let mut rows = vec![
            beacon("W1AW", "FN31", false, 1500.0),
            beacon("K1ABC", "EM73", true, 1200.0),
        ];
        rows.extend(sent("N0XYZ", "KD9TAW SNR?", 900.0, -12));
        rows.extend(sent("VE3QQQ", "HELLO WORLD", 1800.0, 2));
        rows.extend(sent("W1AW", "@ALLCALL GRID FN31PR", 1500.0, -7));
        e.js8_ingest(&rows, 1);
    }

    /// (call, grid, freq, snr, mode) of each spot.
    fn psk_view(spots: &[Spot]) -> Vec<(String, String, u64, i32, String)> {
        spots
            .iter()
            .map(|s| {
                (
                    s.call.clone(),
                    s.grid.clone(),
                    s.freq_hz,
                    s.snr,
                    s.mode.clone(),
                )
            })
            .collect()
    }

    fn view(call: &str, grid: &str, freq: u64, snr: i32) -> (String, String, u64, i32, String) {
        (call.into(), grid.into(), freq, snr, "JS8".into())
    }

    /// The sender records of a PSK Reporter datagram as the server reads them (template 0x50E3:
    /// call, frequency, SNR, mode, locator, information source, time).
    fn uploaded(dgram: &[u8]) -> Vec<(String, u32, i8, String, String, u8)> {
        let u16at = |o: usize| u16::from_be_bytes([dgram[o], dgram[o + 1]]) as usize;
        let mut off = 16;
        while off + 4 <= dgram.len() {
            let (id, len) = (u16at(off), u16at(off + 2));
            if id == 0x50E3 {
                let body = &dgram[off + 4..off + len];
                let mut i = 0;
                let mut out = Vec::new();
                let s = |i: &mut usize| {
                    let n = body[*i] as usize;
                    let v = String::from_utf8_lossy(&body[*i + 1..*i + 1 + n]).into_owned();
                    *i += 1 + n;
                    v
                };
                while i < body.len() && body[i] != 0 {
                    let call = s(&mut i);
                    let freq = u32::from_be_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
                    let snr = body[i + 4] as i8;
                    i += 5;
                    let mode = s(&mut i);
                    let grid = s(&mut i);
                    let source = body[i];
                    i += 1 + 4;
                    out.push((call, freq, snr, mode, grid, source));
                }
                return out;
            }
            off += len;
        }
        panic!("no sender records");
    }

    #[test]
    fn heard_js8_stations_reach_psk_reporter_as_js8_spots_with_their_grids() {
        let mut e = links_engine();
        hear_the_band(&mut e);
        let spots = e.take_js8_psk_spots();
        assert_eq!(
            psk_view(&spots),
            vec![
                // W1AW's heartbeat spot, replaced in the queue by its GRID's 6-character locator
                // (the call-and-band cache: the same call on the same band within the hour).
                view("W1AW", "FN31PR", 14_079_500, -7),
                view("K1ABC", "EM73", 14_079_200, -7),
                // A directed command carries no grid, and none was heard from N0XYZ.
                view("N0XYZ", "", 14_078_900, -12),
                // The first frame of a plain message, `VE3QQQ: …`, is its sender heard.
                view("VE3QQQ", "", 14_079_800, 2),
            ]
        );
        assert!(e.take_js8_psk_spots().is_empty(), "taken once");

        // …and they leave on the wire so: the radio loop's reporter, a loopback listener.
        let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let r = tempo_net::pskreporter::PskReporter::with_target(
            &listener.local_addr().unwrap().to_string(),
        );
        r.send_spots("KD9TAW", "EN52", "Nexus", &spots).unwrap();
        let mut buf = [0u8; 2048];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        assert_eq!(
            uploaded(&buf[..n]),
            vec![
                (
                    "W1AW".into(),
                    14_079_500,
                    -7,
                    "JS8".into(),
                    "FN31PR".into(),
                    1
                ),
                (
                    "K1ABC".into(),
                    14_079_200,
                    -7,
                    "JS8".into(),
                    "EM73".into(),
                    1
                ),
                (
                    "N0XYZ".into(),
                    14_078_900,
                    -12,
                    "JS8".into(),
                    String::new(),
                    1
                ),
                (
                    "VE3QQQ".into(),
                    14_079_800,
                    2,
                    "JS8".into(),
                    String::new(),
                    1
                ),
            ]
        );
    }

    #[test]
    fn a_call_heard_again_within_the_hour_updates_its_queued_spot_and_another_band_is_new() {
        let mut e = links_engine();
        e.js8_ingest(&[beacon("W1AW", "FN31", false, 1500.0)], 1);
        let mut louder = beacon("W1AW", "FN31", false, 1510.0);
        louder.snr = 4;
        e.js8_ingest(&[louder], 2);
        e.set_frequency(7.078, "40m", "USB");
        e.js8_ingest(&[beacon("W1AW", "FN31", false, 1500.0)], 3);
        assert_eq!(
            psk_view(&e.take_js8_psk_spots()),
            vec![
                view("W1AW", "FN31", 14_079_510, 4),
                view("W1AW", "FN31", 7_079_500, -7),
            ]
        );
        // Once its spot has gone, the same call on the same band inside the hour is not
        // spotted again, as JS8Call's cache drops it.
        e.js8_ingest(&[beacon("W1AW", "FN31", false, 1500.0)], 4);
        assert!(e.take_js8_psk_spots().is_empty());
    }

    #[test]
    fn nothing_is_spotted_without_psk_reporter_a_real_call_or_for_a_never_spot_call() {
        let mut off = links_engine();
        off.settings.pskreporter = false;
        hear_the_band(&mut off);
        assert!(off.take_js8_psk_spots().is_empty());
        assert!(off.take_js8net("Nexus").is_empty());

        let mut nocall = links_engine();
        nocall.settings.mycall = String::new();
        hear_the_band(&mut nocall);
        assert!(nocall.take_js8_psk_spots().is_empty());

        // The never-spot list holds a call as heard and by its base call.
        let mut never = links_engine();
        never.settings.js8_spot_never = vec!["w1aw".into(), "VE3QQQ".into()];
        hear_the_band(&mut never);
        never.js8_ingest(&[portable_query(600.0)], 2);
        let calls: Vec<String> = never
            .take_js8_psk_spots()
            .into_iter()
            .map(|s| s.call)
            .collect();
        assert_eq!(calls, vec!["K1ABC", "N0XYZ", "K1ABC/P"]);
        let mut base = links_engine();
        base.settings.js8_spot_never = vec!["K1ABC".into()];
        base.js8_ingest(&[portable_query(600.0)], 1);
        assert!(
            base.take_js8_psk_spots().is_empty(),
            "K1ABC/P by its base call"
        );
        assert!(
            base.take_js8net("Nexus")
                .iter()
                .all(|d| !d.contains("K1ABC")),
            "nor to JS8NET"
        );
    }

    /// One JS8NET datagram as (type, its params without `_ID`), and the `_ID` checked to be the
    /// string of a whole number.
    fn js8net_view(datagrams: &[String]) -> Vec<(String, serde_json::Value)> {
        datagrams
            .iter()
            .map(|d| {
                let mut v: serde_json::Value = serde_json::from_str(d).expect("JSON");
                assert_eq!(v["value"], "", "{d}");
                let id = v["params"]["_ID"]
                    .as_str()
                    .expect("_ID is a string")
                    .to_string();
                assert!(id.parse::<i64>().is_ok_and(|n| n > 0), "{id}");
                v["params"].as_object_mut().unwrap().remove("_ID");
                (v["type"].as_str().unwrap().to_string(), v["params"].clone())
            })
            .collect()
    }

    #[test]
    fn js8net_gets_this_station_each_station_heard_and_each_group_command_as_js8call_sends_them() {
        let mut e = links_engine();
        hear_the_band(&mut e);
        let by = serde_json::json!({"CALLSIGN": "KD9TAW", "GRID": "EN52"});
        let spot = |call: &str, grid: &str, offset: i64, snr: i32| {
            (
                "RX.SPOT".to_string(),
                serde_json::json!({"BY": by, "CALLSIGN": call, "GRID": grid, "DIAL": 14_078_000,
                    "OFFSET": offset, "FREQ": 14_078_000 + offset, "SNR": snr, "SPEED": 0}),
            )
        };
        let directed = |cmd: &str, from: &str, to: &str, text: &str, grid: &str, offset: i64| {
            (
                "RX.DIRECTED".to_string(),
                serde_json::json!({"BY": by, "CMD": cmd, "FROM": from, "TO": to, "PATH": "",
                    "TEXT": text, "GRID": grid, "EXTRA": "", "DIAL": 14_078_000, "OFFSET": offset,
                    "FREQ": 14_078_000 + offset, "SNR": -7, "SPEED": 0}),
            )
        };
        assert_eq!(
            js8net_view(&e.take_js8net("Nexus 1.19.0")),
            vec![
                (
                    "RX.LOCAL".to_string(),
                    serde_json::json!({"CALLSIGN": "KD9TAW", "GRID": "EN52", "INFO": "",
                        "VERSION": "Nexus 1.19.0"})
                ),
                spot("W1AW", "FN31", 1500, -7),
                directed("HEARTBEAT", "W1AW", "@HB", "", "FN31", 1500),
                // A CQ is the sender heard twice in JS8Call, and its whole frame is the text.
                spot("K1ABC", "EM73", 1200, -7),
                spot("K1ABC", "EM73", 1200, -7),
                directed(
                    "CQ",
                    "K1ABC",
                    "@ALLCALL",
                    "K1ABC: @ALLCALL CQ CQ CQ EM73 ",
                    "EM73",
                    1200
                ),
                // To me, not to a group: heard, but no RX.DIRECTED.
                spot("N0XYZ", "", 900, -12),
                spot("VE3QQQ", "", 1800, 2),
                spot("W1AW", "FN31", 1500, -7),
                spot("W1AW", "FN31PR", 1500, -7),
                directed("GRID", "W1AW", "@ALLCALL", "FN31PR", "", 1500),
            ]
        );
        // The next minute: nothing new, and this station unchanged, so nothing at all…
        assert!(e.take_js8net("Nexus 1.19.0").is_empty());
        // …until it changes.
        e.settings.js8_info = "QRP <MYGRID4>".into();
        e.js8_apply_station_config();
        assert_eq!(
            js8net_view(&e.take_js8net("Nexus 1.19.0")),
            vec![(
                "RX.LOCAL".to_string(),
                serde_json::json!({"CALLSIGN": "KD9TAW", "GRID": "EN52", "INFO": "QRP EN52",
                    "VERSION": "Nexus 1.19.0"})
            )]
        );
        // Off JS8 nothing goes, and the queue is dropped; back on JS8 the station is announced.
        e.js8_ingest(&[beacon("W1AW", "FN31", false, 1500.0)], 9);
        e.set_tier(Tier::Ft8);
        assert!(e.take_js8net("Nexus 1.19.0").is_empty());
        e.js8_enter();
        let back = js8net_view(&e.take_js8net("Nexus 1.19.0"));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].0, "RX.LOCAL");
    }

    #[test]
    fn a_heard_aprsis_grid_and_cmd_become_the_gateways_aprs_is_lines() {
        let mut e = links_engine();
        let mut rows = sent("K1ABC", "@APRSIS GRID FN04TV53", 1500.0, -7);
        rows.extend(sent(
            "K1ABC",
            "@APRSIS CMD :SMSGTE :@5551212 TEST",
            900.0,
            -3,
        ));
        e.js8_ingest(&rows, 1);
        let now = now_unix_secs();
        assert_eq!(
            e.js8_aprs_link(now),
            Js8AprsLink {
                armed: true,
                want: true,
            },
            "lines wait: the session must come up, with the passcode"
        );
        assert_eq!(
            e.take_js8_aprs_uplink(now),
            vec![
                "K1ABC>APZNEX,qAS,KD9TAW:=4453.25N/07822.50WG#JS8 14.079500MHz -07dB".to_string(),
                "K1ABC>APZNEX,qAS,KD9TAW::SMSGTE :@5551212 TEST".to_string(),
            ]
        );
        // Handed over: the session stays a minute, so the lines are written, then may go.
        assert!(e.js8_aprs_link(now + 59).want);
        assert!(!e.js8_aprs_link(now + 60).want);
        assert!(e.js8_aprs_link(now + 60).armed, "armed while JS8 runs");
        // A GRID to anyone else, or a CMD to anyone else, is no APRS line.
        e.js8_ingest(&sent("K1ABC", "@ALLCALL GRID FN04TV53", 1500.0, -7), 2);
        e.js8_ingest(&sent("K1ABC", "W1AW CMD :SMSGTE :HI", 1500.0, -7), 3);
        assert!(e.take_js8_aprs_uplink(now).is_empty());
    }

    #[test]
    fn the_gateway_sends_nothing_switched_off_for_a_never_spot_call_or_off_js8_and_lines_expire() {
        let rows = sent("K1ABC", "@APRSIS GRID FN04", 1500.0, -7);
        let now = now_unix_secs();
        let mut off = links_engine();
        off.settings.js8_spot_aprs = false;
        off.js8_ingest(&rows, 1);
        assert!(off.take_js8_aprs_uplink(now).is_empty());
        assert_eq!(off.js8_aprs_link(now), Js8AprsLink::default());
        let mut never = links_engine();
        never.settings.js8_spot_never = vec!["K1ABC".into()];
        never.js8_ingest(&rows, 1);
        assert!(never.take_js8_aprs_uplink(now).is_empty());
        let mut nopsk = links_engine();
        nopsk.settings.pskreporter = false;
        nopsk.js8_ingest(&rows, 1);
        assert!(nopsk.take_js8_aprs_uplink(now).is_empty());
        // A line not handed over within five minutes is dropped, as JS8Call drops it.
        let mut late = links_engine();
        late.js8_ingest(&rows, 1);
        assert!(late
            .take_js8_aprs_uplink(now + APRS_LINE_TTL_SECS + 1)
            .is_empty());
        // Leaving JS8 disarms the gateway and drops what waits.
        let mut left = links_engine();
        left.js8_ingest(&rows, 1);
        left.set_tier(Tier::Ft8);
        assert_eq!(left.js8_aprs_link(now), Js8AprsLink::default());
        left.js8_enter();
        assert!(left.take_js8_aprs_uplink(now).is_empty());
    }
}
