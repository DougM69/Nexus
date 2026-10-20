//! Nexus as a TCI server, the app's side (TCI server design, task S6): the engine's answers to the
//! programs on this computer ([`EngineTci`]), the manager that hot-applies the switch, the state
//! programs are told, the receive audio they are sent, the transmit-audio requests (TX_CHRONO), the
//! connection log, and the server's line in the snapshot. The protocol, the listener and its guards
//! (loopback only, no web pages, the bounds) are `tempo_net::tci::server`'s; this file never touches
//! a socket, and nothing here can make the server listen anywhere but `127.0.0.1` and `::1`.
//!
//! WHAT A PROGRAM MAY DO, AND WHAT IT MAY NOT (design §2.4, §3):
//! - **Nothing a program sends transmits yet.** ⛔ Every key, Tune and CW is refused here
//!   ([`transmit_verdict`]): the engine's TCI rules (the program as an owner of the transmitter,
//!   its verdict, the passband judgement, the TCI over) are task S3's, and until they exist the
//!   answer is closed. A program is told as TCI tells any key that did not take (`trx:0,false;`),
//!   and its `tx_enable` reads false, from the same function. Its transmit audio is dropped.
//! - **Every stop a program sends is honoured, ungated.** `trx:0,false` from a program that keyed
//!   nothing stops whatever is on the air, as WSJT-X's UDP Halt Tx does (design Q8 A); `tune:0,false`
//!   ends Tune whoever started it; `cw_macros_stop` ends CW from any origin.
//! - **The dial and the mode go through the CAT broker's own rules** (`shared_retune`,
//!   `shared_mode_change` in `lib.rs`): refused while anything is on the air (Q9 A), and the dial
//!   for 200 ms after a change no program made (the spec's §3.5: the radio program's own change
//!   holds the parameter that long).
//! - **Power only down** (Q10 A), through `set_rf_power`, so the operator's ceiling still holds.
//!
//! THREADS AND LOCKS:
//! - The manager (`tci-server`) reads the switch, starts and stops the server, and while it serves
//!   builds the station's state AND publishes it under the engine lock, 20 times a second. That is
//!   tempo-net's contract: a snapshot built before a command and published after its answer would
//!   undo the answer's echo. It stops the server with the engine lock released, because
//!   `Server::stop` ends each program's transmissions through this backend, which takes that lock.
//! - The pump (`tci-rx-audio`) sends receive audio, from the server's own copy of the capture, and
//!   TX_CHRONO. It never takes the engine lock, so a CAT stall at the radio loop never stalls a
//!   program's audio.
//! - The server's reader threads call [`EngineTci`], which takes the engine lock briefly, as the
//!   CAT broker's backend does.
//! - The operator's Disconnect ([`disconnect`]) never holds the engine lock either.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::dto::{TciProgramStatus, TciServerStatus};
use tempo_app::engine::{engine_lock, Engine, MeterFeed, TxOwner};
use tempo_audio::receive_audio::{ReceiveAudioFeed, ReceiveAudioReader};
use tempo_audio::tci_audio::{Format, Stream, ToProgram};
use tempo_net::tci::frame::{self, BlockHeader, BlockKind, Chrono, LengthMeaning};
use tempo_net::tci::server::{
    AudioFormat, ClientId, Closed, Config, Cw, Event, Handle, HandshakeRefusal, KeyKind, Limits,
    Program, Refusal, Server, Snapshot, TciBackend, DEFAULT_PORT, UNKNOWN_LIMITS,
};
use tempo_net::tci::wire::{Hz, Word, MAX_HZ};
use tempo_net::tci::Trx;

use crate::{conn_log, SharedEngine};

/// The connection log's lane.
const LANE: &str = "TCI server";
/// How often the manager publishes the station while the server runs: 20 times a second, the most
/// the design allows (§2.1).
const TICK: Duration = Duration::from_millis(50);
/// …and how often it reads the switch while the server is not running.
const IDLE_TICK: Duration = Duration::from_millis(1000);
/// A port whose bind failed is tried again after this; a different port at once.
const BIND_RETRY: Duration = Duration::from_secs(30);
/// How often the pump looks for receive audio to send and transmit audio to ask for.
const PUMP_TICK: Duration = Duration::from_millis(10);
/// A program's dial write is refused this long after a change no program made (the spec's §3.5).
const OPERATOR_HOLD: Duration = Duration::from_millis(200);
/// Nexus's own transmit buffering, told as `tx_stream_audio_buffering`: the TCI over's lead-in
/// (60 ms) and look-ahead (40 ms), the Remote microphone's numbers (design §3.3).
const TX_BUFFERING_MS: u32 = 100;
/// What an armed over asks for ahead of real time: the same buffering.
const LEAD: Duration = Duration::from_millis(TX_BUFFERING_MS as u64);

/// ⚠️ THE SWITCH IS NOT ON THIS BRANCH YET. `Settings::tci_server` and `tci_server_port` are task
/// S4's, on another branch. Until they land here the switch reads as this constant, off, and the
/// server never starts, whatever else is built.
const TCI_SERVER_SWITCH: bool = false;

/// ⚠️ "Programs on TCI may transmit" (`Settings::tci_server_tx`, task S4) is not on this branch
/// either: it reads as off, its default.
const PROGRAMS_MAY_TRANSMIT: bool = false;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The port the operator asked the server to serve on, or `None` while it is switched off. With
/// S4's settings this reads `tci_server` and `tci_server_port`.
fn wanted(_e: &Engine) -> Option<u16> {
    TCI_SERVER_SWITCH.then_some(DEFAULT_PORT)
}

/// The verdict for anything a program asks to transmit: a key, Tune, CW (design §3.2), and the
/// `tx_enable` each program is told, so the two can never disagree.
///
/// ⛔ CLOSED UNTIL TASK S3. The design's first gate here is the station's switch (§3.2 item 2),
/// which reads off. Past it, the engine's own TCI verdict (`Engine::tci_tx_verdict`: every Nexus
/// gate, then the program's ownership of the transmitter and the passband) does not exist yet, so
/// a key that passed the switch would still be refused as not offered. Never `Ok` before S3 lands
/// that verdict and the TCI over that would carry the program's audio.
fn transmit_verdict(_e: &Engine) -> Result<(), Refusal> {
    if !PROGRAMS_MAY_TRANSMIT {
        return Err(Refusal::Switch);
    }
    Err(Refusal::Tier)
}

/// The program whose TCI over is armed: the one the pump asks for transmit audio. ⛔ None until
/// task S3, since no program's key is ever admitted here ([`transmit_verdict`]).
fn armed_over(_e: &Engine) -> Option<ClientId> {
    None
}

/// The dial in Hz.
fn dial_hz(e: &Engine) -> i64 {
    (e.settings().dial_mhz * 1_000_000.0).round() as i64
}

fn hz(v: i64) -> Hz {
    Hz::new(v.clamp(-MAX_HZ, MAX_HZ)).unwrap_or_else(|| unreachable!("clamped to the range"))
}

/// A 0.0–1.0 power as TCI's 0–100.
fn percent(frac: f32) -> u32 {
    (frac * 100.0).round().clamp(0.0, 100.0) as u32
}

/// The power the station is on, as `drive`: commanded, else read back from the radio; 0 while
/// Nexus knows neither (TCI has no word for "unknown").
fn drive(e: &Engine) -> u32 {
    e.effective_rf_power().map(percent).unwrap_or(0)
}

/// The power a Tune keys at, as `tune_drive`: the Tune power, never above the operator's own level
/// (`Settings::tune_power_pct`, applied as the lower of the two), or that level when it is unset.
fn tune_drive(e: &Engine) -> u32 {
    let tune = e.settings().tune_power_pct.map(u32::from);
    match (tune, e.effective_rf_power().map(percent)) {
        (Some(t), Some(d)) => t.min(d),
        (Some(t), None) => t.min(100),
        (None, Some(d)) => d,
        (None, None) => 0,
    }
}

/// The mode the radio was commanded, as a TCI word ([`tempo_net::tci::server::MODULATIONS`]). The
/// CAT broker's reading of the same mode (`EngineRig::mode`), with its fallback to the sideband.
fn tci_mode(e: &Engine) -> Word {
    let commanded = e.rig_mode_effective();
    let commanded = if commanded.trim().is_empty() {
        e.settings().sideband.clone()
    } else {
        commanded
    };
    use crate::CatFamily as F;
    let word = match crate::parse_cat_mode(&commanded) {
        Some(m) => match (m.family, m.lsb) {
            (F::Ssb, Some(true)) => "lsb",
            (F::Ssb, _) => "usb",
            (F::Data, Some(true)) => "digl",
            (F::Data, _) => "digu",
            (F::Cw, _) => "cw",
            // TCI has no RTTY: Nexus's AFSK keys on the lower sideband, and FSK's mark is the dial.
            (F::Rtty, _) => "digl",
            (F::Am, _) => "am",
            (F::Fm | F::DataFm, _) => "nfm",
        },
        None => "usb",
    };
    Word::new(word).unwrap_or_else(|| unreachable!("a TCI mode word"))
}

/// The receive level in dBm from the S-meter's dB over S9: S9 is −73 dBm below 30 MHz and −93 dBm
/// at and above (IARU Region 1 Technical Recommendation R.1), as the radios' own meters are scaled.
fn rx_dbm(dial: i64, over_s9_db: i32) -> f64 {
    let s9 = if dial < 30_000_000 { -73.0 } else { -93.0 };
    s9 + f64::from(over_s9_db)
}

/// The station as programs are told it, read from the engine; the caller holds the engine lock.
fn station(e: &Engine, programs: &[Program], smeter_db: Option<i32>) -> Snapshot {
    let dial = dial_hz(e);
    let verdict = transmit_verdict(e).is_ok();
    Snapshot {
        radio: u64::from(e.settings().active_radio),
        // Nexus has the radio's own range only as a yes or no per frequency (`rig_covers_mhz`,
        // which a program's dial write asks), so programs are told the wide one.
        vfo_limits: (hz(UNKNOWN_LIMITS.0), hz(UNKNOWN_LIMITS.1)),
        receive_only: e.connection_tx_refusal().is_some(),
        dial: hz(dial),
        mode: tci_mode(e),
        drive: drive(e),
        tune_drive: tune_drive(e),
        // S3: or a program's key admitted and its over armed (tempo-net's contract 2).
        trx: e.on_air(),
        tune: e.tuning(),
        // S3: the program whose key owns the transmitter (contract 3).
        owner: None,
        tx_enable: programs.iter().map(|p| (p.id, verdict)).collect(),
        cw_speed: e.cw_wpm(),
        tx_buffering_ms: TX_BUFFERING_MS,
        rx_dbm: smeter_db.map(|db| rx_dbm(dial, db)),
        // Nexus reads its power and SWR only while keyed, has no engine accessor for them on this
        // branch, and reads no microphone level, which `tx_sensors` needs: S3, beside the over.
        tx: None,
    }
}

/// The 200 ms rule's memory (the spec's §3.5): the dial as last seen, and when it last moved
/// without a program moving it, which is the operator's change (or Nexus's own: Doppler, a channel
/// park), and holds the dial against programs for [`OPERATOR_HOLD`].
#[derive(Default)]
struct DialWatch {
    seen: Option<i64>,
    moved_at: Option<Instant>,
}

impl DialWatch {
    /// Look at the dial: the manager every tick, and a program's write before it decides, so a
    /// change is seen by whichever looks first.
    fn look(&mut self, dial: i64, now: Instant) {
        if self.seen.is_some_and(|seen| seen != dial) {
            self.moved_at = Some(now);
        }
        self.seen = Some(dial);
    }

    /// A program moved it.
    fn moved_by_program(&mut self, dial: i64) {
        self.seen = Some(dial);
    }

    fn holds(&self, now: Instant) -> bool {
        self.moved_at
            .is_some_and(|at| now.saturating_duration_since(at) < OPERATOR_HOLD)
    }
}

/// The server's backend: every question a program's command asks, answered from the engine. See
/// the module header for what it admits; the trait's own header (tempo-net) for when it is called.
pub(crate) struct EngineTci {
    engine: SharedEngine,
    /// The server's copy of the receive audio: `audio_start` is answered from whether it has a
    /// source.
    feed: Arc<ReceiveAudioFeed>,
    dial: Mutex<DialWatch>,
    /// The last thing each connected program was refused, in the log's words, for the status.
    refused: Mutex<HashMap<ClientId, &'static str>>,
    /// The last line this lane wrote, so a line is never written twice in a row (the broker's
    /// rule, `refusal_is_new`).
    last_line: Mutex<Option<String>>,
    /// Transmit-audio frames dropped because no over of the sender's is armed.
    tx_dropped: AtomicU64,
}

impl EngineTci {
    fn new(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>) -> EngineTci {
        EngineTci {
            engine,
            feed,
            dial: Mutex::new(DialWatch::default()),
            refused: Mutex::new(HashMap::new()),
            last_line: Mutex::new(None),
            tx_dropped: AtomicU64::new(0),
        }
    }

    fn log(&self, level: &'static str, line: String) {
        if crate::refusal_is_new(&self.last_line, &line) {
            conn_log(LANE, level, line);
        }
    }
}

/// A connection-log line for what the server reports, and its level.
fn describe(event: &Event) -> (&'static str, String) {
    match event {
        Event::Connected { client, peer } => ("info", format!("{client} connected from {peer}")),
        Event::Closed { client, why } => {
            let (level, text) = match why {
                Closed::Program => ("info", "it closed the connection"),
                Closed::Stopped => ("info", "the server stopped"),
                Closed::Disconnected => ("info", "the operator disconnected it"),
                Closed::Lost => ("error", "the connection failed"),
                Closed::Silent => ("error", "it stopped answering"),
                Closed::Flood => ("error", "it sent commands too fast for too long"),
                Closed::TooBig => ("error", "it sent a message too large"),
                Closed::Garbage => ("error", "it sent messages that are not TCI"),
                Closed::Protocol => ("error", "it broke the WebSocket protocol"),
                Closed::Backlog => ("error", "it stopped reading what Nexus sent"),
            };
            (level, format!("{client} disconnected: {text}"))
        }
        Event::Refused { peer, why } => {
            let text = match why {
                HandshakeRefusal::Origin(origin) => format!(
                    "refused a web page ({origin}) from {peer}: only programs on this computer \
                     connect, never a browser page"
                ),
                HandshakeRefusal::Host(host) => format!(
                    "refused a connection from {peer} naming the host {host:?}: only 127.0.0.1, \
                     localhost or [::1] with the server's port"
                ),
                HandshakeRefusal::Version => {
                    format!("refused a connection from {peer}: not WebSocket version 13")
                }
                HandshakeRefusal::Malformed => {
                    format!("refused a connection from {peer}: not a WebSocket handshake")
                }
                HandshakeRefusal::Full => {
                    format!("refused a connection from {peer}: as many programs as Nexus takes are connected")
                }
                HandshakeRefusal::Busy => {
                    format!("refused a connection from {peer}: too many were opening at once")
                }
                HandshakeRefusal::Timeout => {
                    format!("closed a connection from {peer}: its handshake did not finish in time")
                }
                HandshakeRefusal::Incomplete => {
                    format!("a connection from {peer} ended before its handshake did")
                }
                HandshakeRefusal::NotLocal => {
                    format!("refused a connection from {peer}: only this computer may connect")
                }
            };
            ("error", text)
        }
        Event::Refusal { client, refusal } => {
            ("error", format!("refused {client}: {}", refusal.text()))
        }
        Event::CommandRefused { client, what } => (
            "info",
            format!(
                "{client} asked for {}, which Nexus does not offer through TCI",
                what.text()
            ),
        ),
    }
}

impl TciBackend for EngineTci {
    fn disconnected(&self, client: ClientId) {
        // ⛔ Until S3 a program keys nothing ([`transmit_verdict`]), so it has nothing of its own
        // on the air to end. S3 ends its over, microphone key, Tune and CW here, hard (design §3.4).
        lock(&self.refused).remove(&client);
    }

    fn event(&self, event: Event) {
        match &event {
            Event::Refusal { client, refusal } => {
                lock(&self.refused).insert(*client, refusal.text());
            }
            Event::CommandRefused { client, what } => {
                lock(&self.refused).insert(*client, what.text());
            }
            Event::Closed { client, .. } => {
                lock(&self.refused).remove(client);
            }
            Event::Connected { .. } | Event::Refused { .. } => {}
        }
        let (level, line) = describe(&event);
        self.log(level, line);
    }

    fn set_dial(&self, _client: ClientId, hz_asked: Hz) -> Result<Hz, Refusal> {
        let mut e = engine_lock(&self.engine);
        let now = Instant::now();
        let mut watch = lock(&self.dial);
        watch.look(dial_hz(&e), now);
        // Never under an over: a retune mid-over halts it (design Q9 A).
        if e.on_air() {
            return Err(Refusal::OnAir);
        }
        if watch.holds(now) {
            return Err(Refusal::OperatorChange);
        }
        let asked = u64::try_from(hz_asked.get()).map_err(|_| Refusal::Limits)?;
        if e.rig_covers_mhz(asked as f64 / 1_000_000.0) == Some(false) {
            return Err(Refusal::Limits);
        }
        crate::shared_retune(&mut e, asked);
        let dial = dial_hz(&e);
        watch.moved_by_program(dial);
        Ok(hz(dial))
    }

    fn set_mode(&self, _client: ClientId, mode: &Word) -> Result<Word, Refusal> {
        let mut e = engine_lock(&self.engine);
        if e.on_air() {
            return Err(Refusal::OnAir);
        }
        crate::shared_mode_change(&mut e, mode.as_str()).map_err(|_| Refusal::Family)?;
        Ok(tci_mode(&e))
    }

    fn set_drive(&self, _client: ClientId, level: u32) -> Result<u32, Refusal> {
        let mut e = engine_lock(&self.engine);
        // Down only (design Q10 A). A level Nexus does not know cannot be shown to be lower.
        let now = e.effective_rf_power().map(percent).ok_or(Refusal::Raise)?;
        if level > now {
            return Err(Refusal::Raise);
        }
        if level < now {
            // Through the one chokepoint, so the operator's ceiling still binds.
            e.set_rf_power(level as f32 / 100.0);
        }
        Ok(drive(&e))
    }

    fn set_tune_drive(&self, _client: ClientId, level: u32) -> Result<u32, Refusal> {
        let e = engine_lock(&self.engine);
        let now = tune_drive(&e);
        match level.cmp(&now) {
            std::cmp::Ordering::Equal => Ok(now),
            std::cmp::Ordering::Greater => Err(Refusal::Raise),
            // Lower would rewrite the operator's own Tune power setting. A program's Tune, and the
            // level it keys at, are S3's: not offered until then.
            std::cmp::Ordering::Less => Err(Refusal::Tier),
        }
    }

    fn key(&self, _client: ClientId, _kind: KeyKind, _audio_started: bool) -> Result<(), Refusal> {
        transmit_verdict(&engine_lock(&self.engine))
    }

    fn release(&self, client: ClientId) {
        // S3: from the program that owns the transmitter, a graceful end of its own over (§3.4).
        // From any other program, and until S3 that is every program: what is on the air stops,
        // as for WSJT-X's Halt Tx, and holds until the operator arms (design Q8 A).
        let mut e = engine_lock(&self.engine);
        if e.on_air() {
            e.halt_tx();
            drop(e);
            self.log(
                "info",
                format!("{client} sent trx:0,false: Nexus stopped transmitting, as for Halt Tx"),
            );
        }
    }

    fn tune_on(&self, _client: ClientId) -> Result<(), Refusal> {
        transmit_verdict(&engine_lock(&self.engine))
    }

    fn tune_off(&self, _client: ClientId) {
        // Whoever started it (design §3.5). Nothing when nothing tunes: a release with nothing to
        // release still invalidates the over Nexus has planned.
        let mut e = engine_lock(&self.engine);
        if e.tuning() || e.tx_owner() == Some(TxOwner::Tune) {
            e.set_tune(false);
        }
    }

    fn end_own(&self, _client: ClientId) {
        // `stop;`: the program's own key, Tune and CW, and nothing else. ⛔ Until S3 a program owns
        // none of them, so there is nothing to end.
    }

    fn tx_audio(&self, _client: ClientId, _format: AudioFormat, _frame: &[u8]) {
        // ⛔ Until S3 no program's over can be armed ([`transmit_verdict`] admits no key), so a frame
        // that got here is dropped, never kept for a later key (design §3.1 item 8). S3 checks the
        // owner again here (tempo-net's contract 6) and feeds its over through
        // `tci_audio::FromProgram`.
        self.tx_dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn audio_start(&self, _client: ClientId, _format: AudioFormat) -> Result<(), Refusal> {
        // The sound card's capture, through the server's own copy of it. A radio whose audio
        // Nexus takes another way (the Flex's DAX, the Icom network connection) has none for
        // programs yet: task S5.
        if self.feed.source().is_some() {
            Ok(())
        } else {
            Err(Refusal::NoSource)
        }
    }

    fn audio_stop(&self, _client: ClientId) {
        // The pump stops sending when the program's stream reads stopped.
    }

    fn cw(&self, _client: ClientId, _cw: Cw) -> Result<(), Refusal> {
        transmit_verdict(&engine_lock(&self.engine))
    }

    fn cw_stop(&self, _client: ClientId) {
        // From any origin (design §3.5), and CW only: `stop_cw` also cuts a slot over in flight.
        let mut e = engine_lock(&self.engine);
        if e.tx_owner() == Some(TxOwner::Cw) {
            e.stop_cw();
        }
    }
}

/// A program's stream as tempo-codec's frames read it.
fn stream_of(f: AudioFormat) -> Option<Stream> {
    Stream::new(
        f.rate.hz(),
        Format::from_code(f.sample_type.code())?,
        u32::try_from(f.channels.count()).ok()?,
        f.samples,
    )
}

/// Receive audio as one program's frames: `mono` at `source_rate` in, every whole frame it
/// completes out, encoded in the program's own rate, format and channels.
fn program_frames(
    to: &mut ToProgram,
    format: AudioFormat,
    source_rate: u32,
    mono: &[f32],
) -> Vec<Vec<u8>> {
    let (Some(stream), Some(trx)) = (stream_of(format), Trx::new(0)) else {
        return Vec::new();
    };
    let header = BlockHeader {
        trx,
        kind: BlockKind::RxAudio,
        sample_rate: stream.rate(),
        format: format.sample_type,
        channels: format.channels,
    };
    to.push(source_rate, stream, mono)
        .into_iter()
        .filter_map(|values| frame::encode_block(&header, LengthMeaning::Values, &values).ok())
        .collect()
}

/// TX_CHRONO for one armed over: at the arm, the over's lead asked for at once; then one request
/// per frame's worth of real time. A turn that comes late asks for the lead again rather than for
/// the time it missed, so a program is never asked for more than the lead ahead of real time. S3
/// may pace by the over's feed instead of the clock.
struct Pacer {
    client: ClientId,
    format: AudioFormat,
    chrono: Chrono,
    /// Frames each request asks for, and the stream's rate.
    frames: u64,
    rate: u64,
    since: Instant,
    /// Frames asked for since `since`.
    asked: u64,
}

impl Pacer {
    fn new(client: ClientId, format: AudioFormat, now: Instant) -> Option<Pacer> {
        let stream = stream_of(format)?;
        let chrono = Chrono {
            trx: Trx::new(0)?,
            sample_rate: stream.rate(),
            format: format.sample_type,
            channels: format.channels,
            length: u32::try_from(stream.values()).ok()?,
        };
        Some(Pacer {
            client,
            format,
            chrono,
            frames: u64::try_from(stream.frames()).ok()?.max(1),
            rate: u64::from(stream.rate()),
            since: now,
            asked: 0,
        })
    }

    /// How many requests are due at `now`.
    fn due(&mut self, now: Instant) -> u64 {
        let frames_in = |d: Duration| {
            u64::try_from(d.as_micros() * u128::from(self.rate) / 1_000_000).unwrap_or(u64::MAX)
        };
        let lead = frames_in(LEAD);
        let target = frames_in(now.saturating_duration_since(self.since)).saturating_add(lead);
        if target > self.asked.saturating_add(lead + self.frames) {
            self.asked = target - lead;
        }
        let n = target.saturating_sub(self.asked) / self.frames;
        self.asked += n * self.frames;
        n
    }
}

/// The pump's state between turns.
#[derive(Default)]
struct PumpState {
    reader: Option<ReceiveAudioReader>,
    streams: HashMap<ClientId, ToProgram>,
    pacer: Option<Pacer>,
}

impl PumpState {
    /// One turn: receive audio to each program that started it, then TX_CHRONO to the program
    /// whose over is `armed`, if any.
    fn turn(
        &mut self,
        handle: &Handle,
        feed: &Arc<ReceiveAudioFeed>,
        armed: Option<ClientId>,
        now: Instant,
    ) {
        let programs = handle.programs();
        self.receive(handle, feed, &programs, now);
        self.request(handle, &programs, armed, now);
    }

    /// Receive audio from the server's copy of the capture. Nothing is copied while no program
    /// listens: the reader is let go, and the feed idles.
    fn receive(
        &mut self,
        handle: &Handle,
        feed: &Arc<ReceiveAudioFeed>,
        programs: &[Program],
        now: Instant,
    ) {
        let listening: Vec<&Program> = programs.iter().filter(|p| p.audio).collect();
        self.streams
            .retain(|id, _| listening.iter().any(|p| p.id == *id));
        if listening.is_empty() {
            self.reader = None;
            return;
        }
        if self.reader.is_none() {
            self.reader = feed.source().and_then(|source| feed.subscribe(source).ok());
        }
        let Some(reader) = &self.reader else {
            return;
        };
        let mut ended = false;
        loop {
            match reader.read(now) {
                Ok(Some(block)) => {
                    for p in &listening {
                        let to = self.streams.entry(p.id).or_default();
                        for bytes in program_frames(to, p.format, block.source.rate, &block.samples)
                        {
                            handle.send_rx_audio(p.id, bytes);
                        }
                    }
                }
                Ok(None) => break,
                // The capture changed or closed: subscribe again on a later turn, to whatever
                // it is then.
                Err(_) => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.reader = None;
        }
    }

    /// TX_CHRONO to the program whose over is armed and to no one else: the engine says whose over
    /// is armed (`armed`), the server says whose `tci` key it admitted, and both must name it.
    fn request(
        &mut self,
        handle: &Handle,
        programs: &[Program],
        armed: Option<ClientId>,
        now: Instant,
    ) {
        let owner = armed.and_then(|client| {
            programs
                .iter()
                .find(|p| p.id == client && p.key == Some(KeyKind::Tci))
        });
        let Some(owner) = owner else {
            self.pacer = None;
            return;
        };
        if !self
            .pacer
            .as_ref()
            .is_some_and(|p| p.client == owner.id && p.format == owner.format)
        {
            self.pacer = Pacer::new(owner.id, owner.format, now);
        }
        if let Some(pacer) = self.pacer.as_mut() {
            for _ in 0..pacer.due(now) {
                handle.send_chrono(pacer.client, &pacer.chrono);
            }
        }
    }
}

/// The pump's thread, while the server runs.
struct Pump {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pump {
    fn start(
        handle: Handle,
        feed: Arc<ReceiveAudioFeed>,
        armed: Arc<Mutex<Option<ClientId>>>,
    ) -> Pump {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("tci-rx-audio".into())
            .spawn(move || {
                let mut pump = PumpState::default();
                while !flag.load(Ordering::Relaxed) {
                    let armed = *lock(&armed);
                    pump.turn(&handle, &feed, armed, Instant::now());
                    std::thread::sleep(PUMP_TICK);
                }
            })
            .ok();
        Pump { stop, thread }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The running server's handle, for the operator's Disconnect; `None` while it is off.
#[derive(Default)]
pub(crate) struct Serving(Mutex<Option<Handle>>);

impl Serving {
    fn disconnect(&self, id: u64) -> bool {
        // The handle is taken out and the lock let go before the server is called.
        let handle = lock(&self.0).clone();
        let Some(handle) = handle else {
            return false;
        };
        match handle.programs().into_iter().find(|p| p.id.get() == id) {
            Some(p) => {
                handle.disconnect(p.id);
                true
            }
            None => false,
        }
    }
}

/// The server the running program's manager serves, for [`disconnect`].
static SERVING: OnceLock<Arc<Serving>> = OnceLock::new();

/// The operator's Disconnect: program `id` (its number in the snapshot's status) has what it keyed
/// ended, then it is closed. Never under the engine lock (tempo-net's lock rule).
pub(crate) fn disconnect(id: u64) -> bool {
    SERVING.get().is_some_and(|serving| serving.disconnect(id))
}

/// A running server: the port the operator asked for (it may be serving on the next one), the
/// server, its pump.
struct Running {
    asked: u16,
    server: Server,
    pump: Pump,
}

/// A bind that failed, so it is neither retried nor reported every tick.
struct Failed {
    port: u16,
    at: Instant,
    why: String,
}

/// The sentence for a server that could not bind.
fn bind_message(port: u16, err: &std::io::Error) -> String {
    let next = port.saturating_add(1);
    if err.kind() == std::io::ErrorKind::AddrInUse {
        format!(
            "Programs cannot connect by TCI: ports {port} and {next} on this computer are taken, \
             usually by an SDR program's own TCI server or two other Nexus windows. Close the one \
             you do not need, or give Nexus's TCI server a port nothing else uses. Retrying quietly."
        )
    } else {
        format!(
            "Programs cannot connect by TCI: this computer refused port {port} ({err}). Give \
             Nexus's TCI server another port. Retrying quietly."
        )
    }
}

/// The manager: hot-applies the switch and the port, and publishes the station while it serves.
pub(crate) struct Manager {
    engine: SharedEngine,
    feed: Arc<ReceiveAudioFeed>,
    meters: MeterFeed,
    backend: Arc<EngineTci>,
    /// The program whose over is armed, as the engine said it at the last tick, for the pump.
    armed: Arc<Mutex<Option<ClientId>>>,
    serving: Arc<Serving>,
    running: Option<Running>,
    failed: Option<Failed>,
    retry: Duration,
}

impl Manager {
    fn new(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>, meters: MeterFeed) -> Manager {
        Manager {
            backend: Arc::new(EngineTci::new(engine.clone(), feed.clone())),
            engine,
            feed,
            meters,
            armed: Arc::new(Mutex::new(None)),
            serving: Arc::new(Serving::default()),
            running: None,
            failed: None,
            retry: BIND_RETRY,
        }
    }

    /// One look: the switch, then the station. Whether the server is running.
    fn tick(&mut self) -> bool {
        let want = wanted(&engine_lock(&self.engine));
        self.apply(want);
        self.publish();
        self.running.is_some()
    }

    /// Start, stop or move the server to what the operator asked for. Called with the engine lock
    /// released: stopping a server ends its programs' transmissions through the backend.
    fn apply(&mut self, want: Option<u16>) {
        if want == self.running.as_ref().map(|r| r.asked) {
            return;
        }
        if let Some(running) = self.running.take() {
            self.stop(running);
        }
        let Some(port) = want else {
            self.failed = None;
            return;
        };
        if self
            .failed
            .as_ref()
            .is_some_and(|f| f.port == port && f.at.elapsed() < self.retry)
        {
            return;
        }
        let first = station(&engine_lock(&self.engine), &[], self.meters.smeter_db());
        let config = Config {
            port,
            limits: Limits::default(),
        };
        let backend: Arc<dyn TciBackend> = self.backend.clone();
        match Server::start(config, backend, first) {
            Ok(server) => {
                let handle = server.handle();
                let pump = Pump::start(handle.clone(), self.feed.clone(), self.armed.clone());
                *lock(&self.serving.0) = Some(handle);
                conn_log(
                    LANE,
                    "info",
                    format!(
                        "programs on this computer can connect by TCI to 127.0.0.1:{}",
                        server.port()
                    ),
                );
                self.failed = None;
                self.running = Some(Running {
                    asked: port,
                    server,
                    pump,
                });
            }
            Err(err) => {
                let why = bind_message(port, &err);
                if !self.failed.as_ref().is_some_and(|f| f.port == port) {
                    tempo_core::applog::error("tci", &why);
                    conn_log(LANE, "error", why.clone());
                }
                self.failed = Some(Failed {
                    port,
                    at: Instant::now(),
                    why,
                });
            }
        }
    }

    /// Stop a server: the Disconnect can no longer reach it, the pump stops, then the server ends
    /// every program's transmissions, tells each `trx:0,false;` and closes (tempo-net's order).
    fn stop(&mut self, running: Running) {
        *lock(&self.serving.0) = None;
        running.pump.stop();
        running.server.stop();
        *lock(&self.armed) = None;
        conn_log(
            LANE,
            "info",
            "stopped: programs can no longer connect by TCI",
        );
    }

    /// The station to every program, and the server's line to the snapshot: built and published
    /// under the engine lock (tempo-net's contract 1). Neither call reaches the backend.
    fn publish(&mut self) {
        let mut e = engine_lock(&self.engine);
        let status = match &self.running {
            Some(running) => {
                let handle = running.server.handle();
                let programs = handle.programs();
                lock(&self.backend.dial).look(dial_hz(&e), Instant::now());
                handle.publish(station(&e, &programs, self.meters.smeter_db()));
                *lock(&self.armed) = armed_over(&e);
                let refused = lock(&self.backend.refused);
                Some(TciServerStatus {
                    port: Some(running.server.port()),
                    error: None,
                    programs: programs
                        .iter()
                        .map(|p| TciProgramStatus {
                            id: p.id.get(),
                            peer: p.peer.to_string(),
                            audio: p.audio,
                            keyed: p.key.is_some(),
                            refused: refused.get(&p.id).map(|r| (*r).to_string()),
                        })
                        .collect(),
                })
            }
            None => self.failed.as_ref().map(|f| TciServerStatus {
                port: None,
                error: Some(f.why.clone()),
                programs: Vec::new(),
            }),
        };
        e.set_tci_server_status(status);
    }
}

/// Start the manager. Nothing is bound until the operator turns the server on.
pub(crate) fn spawn(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>, meters: MeterFeed) {
    let mut manager = Manager::new(engine, feed, meters);
    let _ = SERVING.set(manager.serving.clone());
    let _ = std::thread::Builder::new()
        .name("tci-server".into())
        .spawn(move || loop {
            let on = manager.tick();
            std::thread::sleep(if on { TICK } else { IDLE_TICK });
        });
}

#[cfg(test)]
mod tests;
