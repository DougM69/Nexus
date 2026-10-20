//! The TCI server's app side against a real engine and the real server on loopback: what a program
//! is refused, what its stops do, the dial, mode and power rules, the receive audio it is sent,
//! TX_CHRONO, the manager's switch and port, and the server's line in the snapshot.

use std::net::{Ipv4Addr, TcpListener, TcpStream};

use tempo_app::settings::{OperatingMode, Settings};
use tempo_audio::receive_audio::ReceiveSource;
use tempo_net::tci::frame::{RawHeader, SampleFormat, HEADER_LEN};
use tokio_tungstenite::tungstenite::{self, Message, WebSocket};

use super::*;

const WAIT: Duration = Duration::from_secs(5);

/// A station on 20 m FT8 with transmit armed and allowed: every gate a program could pass is open,
/// so whatever refuses it is this file's.
fn armed() -> SharedEngine {
    let mut e = Engine::with_settings(Settings {
        mycall: "KD9TAW".into(),
        mygrid: "EN52".into(),
        operating_mode: OperatingMode::Digital,
        band: "20m".into(),
        dial_mhz: 14.074,
        sideband: "USB".into(),
        ..Settings::default()
    });
    e.set_tx_enabled(true);
    assert!(
        e.tx_enabled() && e.tx_allowed() && !e.on_air(),
        "premise: transmit armed and allowed on 14.074, nothing on the air"
    );
    Arc::new(Mutex::new(e))
}

/// The manager serving on a port the system picks, with the receive feed it was given.
fn serving(engine: &SharedEngine) -> (Manager, Arc<ReceiveAudioFeed>) {
    let feed = Arc::new(ReceiveAudioFeed::default());
    let mut m = Manager::new(engine.clone(), feed.clone(), MeterFeed::default());
    m.apply(Some(0));
    assert!(m.running.is_some(), "the server starts on a free port");
    (m, feed)
}

fn port(m: &Manager) -> u16 {
    m.running.as_ref().expect("serving").server.port()
}

fn handle(m: &Manager) -> Handle {
    m.running.as_ref().expect("serving").server.handle()
}

/// A message from the server.
#[derive(Debug)]
enum Got {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>),
}

/// A program on this computer: a plain WebSocket client, as WSJT-X's is (no `Origin`).
struct Client {
    ws: WebSocket<TcpStream>,
}

impl Client {
    fn connect(port: u16) -> Client {
        let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("the server accepts");
        let (ws, _) =
            tungstenite::client(format!("ws://127.0.0.1:{port}/"), tcp).expect("the handshake");
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("a read timeout");
        Client { ws }
    }

    fn send(&mut self, line: &str) {
        self.ws.send(Message::text(line)).expect("sent");
    }

    /// The next message, if one comes within `within`.
    fn next(&mut self, within: Duration) -> Option<Got> {
        let until = Instant::now() + within;
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Some(Got::Text(t.as_str().to_string())),
                Ok(Message::Binary(b)) => return Some(Got::Binary(b.to_vec())),
                Ok(Message::Close(frame)) => {
                    return Some(Got::Close(frame.map(|f| u16::from(f.code))))
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return None,
            }
            if Instant::now() >= until {
                return None;
            }
        }
    }

    /// Every message up to and including the text line `last`.
    fn until(&mut self, last: &str) -> Vec<Got> {
        let until = Instant::now() + WAIT;
        let mut got = Vec::new();
        while let Some(m) = self.next(until.saturating_duration_since(Instant::now())) {
            let done = matches!(&m, Got::Text(t) if t == last);
            got.push(m);
            if done {
                return got;
            }
        }
        panic!("no {last:?} within {WAIT:?}; got {got:?}");
    }

    /// The text lines up to and including `last`.
    fn lines_until(&mut self, last: &str) -> Vec<String> {
        self.until(last)
            .into_iter()
            .filter_map(|m| match m {
                Got::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    /// Whatever comes within `within`.
    fn drain(&mut self, within: Duration) -> Vec<Got> {
        let until = Instant::now() + within;
        let mut got = Vec::new();
        while let Some(m) = self.next(until.saturating_duration_since(Instant::now())) {
            got.push(m);
        }
        got
    }
}

/// A program connected and greeted (the init burst ends with `start;`), and its number.
fn program(handle: &Handle, port: u16) -> (Client, ClientId, Vec<String>) {
    let before: Vec<ClientId> = handle.programs().iter().map(|p| p.id).collect();
    let mut client = Client::connect(port);
    let burst = client.lines_until("start;");
    let id = handle
        .programs()
        .iter()
        .map(|p| p.id)
        .find(|id| !before.contains(id))
        .expect("the server lists the program");
    (client, id, burst)
}

/// Whether this lane of the connection log has a line holding `needle`.
fn logged(needle: &str) -> bool {
    crate::CONN_LOG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|ev| ev.connector == LANE && ev.message.reveal_on_screen().contains(needle))
}

/// Wait for `f` on the engine, or `WAIT`.
fn eventually(engine: &SharedEngine, f: impl Fn(&Engine) -> bool) -> bool {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if f(&engine_lock(engine)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// ⛔ THE CLOSED DEFAULT. With every Nexus gate open (armed, allowed, nothing on the air), every way
/// a program can ask to transmit is refused, on the wire as TCI refuses a key and at the backend as
/// the station's switch; `tx_enable` says false from the same verdict; and nothing in the engine
/// moves. CONTROL: the transmit latch is still up afterwards, so nothing here disarmed the station
/// to get its refusals.
#[test]
fn every_transmit_a_program_asks_for_is_refused_until_the_engine_has_tci_rules() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, burst) = program(&h, port(&m));
    assert!(
        burst.contains(&"tx_enable:0,false;".to_string()),
        "the burst tells the program it may not transmit: {burst:?}"
    );
    for asked in [
        // No receive source on this station: refused, and a key that needs one is refused anyway.
        "audio_start:0;",
        "trx:0,true,tci;",
        "trx:0,true;",
        "trx:0,true,mic1;",
        "tune:0,true;",
        "cw_macros:0,CQ TEST;",
    ] {
        client.send(asked);
    }
    // A read the server answers in order, after everything above.
    client.send("modulation:0;");
    let lines = client.lines_until("modulation:0,digu;");
    m.publish();
    client.send("modulation:0;");
    let mut lines_after = client.lines_until("modulation:0,digu;");
    let mut all = lines.clone();
    all.append(&mut lines_after);
    for keyed in [
        "trx:0,true;",
        "tune:0,true;",
        "tx_enable:0,true;",
        "audio_start:0;",
    ] {
        assert!(
            !all.iter().any(|l| l == keyed),
            "a program was told {keyed} though the engine has no TCI rules: {all:?}"
        );
    }
    assert_eq!(
        lines.iter().filter(|l| *l == "trx:0,false;").count(),
        3,
        "each of the three keys is answered as a key that did not take: {lines:?}"
    );
    assert!(lines.contains(&"tune:0,false;".to_string()), "{lines:?}");

    let tci = m.backend.clone();
    for kind in [
        KeyKind::Tci,
        KeyKind::Mic(tempo_net::tci::server::Mic::Selected),
    ] {
        assert_eq!(tci.key(id, kind, true), Err(Refusal::Switch), "{kind:?}");
    }
    assert_eq!(tci.tune_on(id), Err(Refusal::Switch));
    let text = tempo_net::tci::wire::Text::new("CQ").expect("a CW text");
    assert_eq!(tci.cw(id, Cw::Macro(text)), Err(Refusal::Switch));

    {
        let e = engine_lock(&engine);
        assert!(
            e.tx_enabled(),
            "CONTROL: the station stays armed; the refusals were the TCI side's"
        );
        assert!(!e.on_air() && !e.tuning() && e.tx_owner().is_none());
    }
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let refused = status
        .and_then(|s| s.programs.into_iter().find(|p| p.id == id.get()))
        .and_then(|p| p.refused);
    assert_eq!(
        refused.as_deref(),
        Some(Refusal::Switch.text()),
        "the status names the last refusal"
    );
    assert!(logged(&format!("refused {id}: {}", Refusal::Switch.text())));
    assert!(logged(&format!(
        "refused {id}: {}",
        Refusal::NoSource.text()
    )));
}

/// A program's `trx:0,false` stops what is on the air, as WSJT-X's Halt Tx does (design Q8 A): the
/// over is cut at the loop's next tick (the abort armed, the slot flag down), transmit disarms, and
/// the stop HOLDS, so entering a section does not re-arm. CONTROL: with nothing on the air it
/// changes nothing at all.
#[test]
fn a_program_s_trx_false_stops_what_is_on_the_air_and_the_stop_holds() {
    for on_air in [true, false] {
        let engine = armed();
        if on_air {
            engine_lock(&engine).app.set_transmitting(true);
        }
        let (m, _feed) = serving(&engine);
        let h = handle(&m);
        let (mut client, _, _) = program(&h, port(&m));
        client.send("trx:0,false;");
        client.send("modulation:0;");
        client.lines_until("modulation:0,digu;");
        let mut e = engine_lock(&engine);
        if on_air {
            assert!(
                !e.tx_enabled() && !e.on_air(),
                "a program's trx:0,false stops the over and disarms"
            );
            assert!(
                e.take_slot_tx_abort(),
                "the loop cuts the over at its next tick"
            );
            e.set_operating_mode("phone", false);
            assert!(!e.tx_enabled(), "the stop holds until the operator arms");
        } else {
            assert!(
                e.tx_enabled(),
                "CONTROL: nothing on the air, nothing stopped"
            );
            assert!(!e.take_slot_tx_abort(), "CONTROL: no abort armed");
        }
    }
}

/// The dial is the operator's while anything is on the air (design Q9 A): a program's write is
/// refused and the program told the dial in force. Off the air it retunes through the CAT broker's
/// own rule, band and all. CONTROL: the same write lands once the over ends.
#[test]
fn a_program_s_dial_is_refused_while_anything_is_on_the_air() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    engine_lock(&engine).app.set_transmitting(true);
    assert_eq!(
        tci.set_dial(id, hz(7_074_000)),
        Err(Refusal::OnAir),
        "a retune under an over"
    );
    client.send("vfo:0,0,7074000;");
    let lines = client.lines_until("tx_frequency:14074000;");
    assert!(
        lines.contains(&"vfo:0,0,14074000;".to_string()),
        "{lines:?}"
    );
    assert_eq!(dial_hz(&engine_lock(&engine)), 14_074_000);

    engine_lock(&engine).app.set_transmitting(false);
    assert_eq!(tci.set_dial(id, hz(7_074_000)), Ok(hz(7_074_000)));
    let e = engine_lock(&engine);
    assert_eq!(dial_hz(&e), 7_074_000);
    assert_eq!(
        e.settings().band,
        "40m",
        "the band follows, as for the CAT broker"
    );
}

/// The spec's §3.5: a change made in the radio program holds the parameter for 200 ms. A dial the
/// operator just moved refuses a program's write, whether the manager or the write saw the move
/// first; 200 ms on, the same write lands. The hold runs from the move as the manager saw it, not
/// from a program's first write after it: a write long after the move lands at once. CONTROL: a
/// program's own retune holds nothing.
#[test]
fn a_program_s_dial_waits_out_200_ms_after_the_operator_moves_it() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    assert_eq!(tci.set_dial(id, hz(14_075_000)), Ok(hz(14_075_000)));
    assert_eq!(
        tci.set_dial(id, hz(14_076_000)),
        Ok(hz(14_076_000)),
        "CONTROL: a program's own retune holds nothing"
    );
    let wait_out = || std::thread::sleep(OPERATOR_HOLD + Duration::from_millis(50));
    for manager_sees_it_first in [false, true] {
        engine_lock(&engine).set_frequency(14.080, "20m", "USB");
        if manager_sees_it_first {
            m.publish();
        }
        assert_eq!(
            tci.set_dial(id, hz(14_077_000)),
            Err(Refusal::OperatorChange),
            "manager first: {manager_sees_it_first}"
        );
        assert_eq!(dial_hz(&engine_lock(&engine)), 14_080_000);
        wait_out();
        assert_eq!(tci.set_dial(id, hz(14_077_000)), Ok(hz(14_077_000)));
    }
    engine_lock(&engine).set_frequency(14.081, "20m", "USB");
    m.publish();
    wait_out();
    assert_eq!(
        tci.set_dial(id, hz(14_078_000)),
        Ok(hz(14_078_000)),
        "a write well after the move the manager saw is not held"
    );
}

/// Power only down (design Q10 A), through the one chokepoint, so the operator's ceiling holds. A
/// level Nexus does not know cannot be shown lower and is refused. Tune power: never up; down is
/// not offered until S3.
#[test]
fn a_program_may_lower_power_and_never_raise_it() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    assert_eq!(
        tci.set_drive(id, 10),
        Err(Refusal::Raise),
        "no level known: refused"
    );
    engine_lock(&engine).set_rf_power(0.5);
    assert_eq!(
        tci.set_drive(id, 60),
        Err(Refusal::Raise),
        "a raise is refused"
    );
    assert_eq!(engine_lock(&engine).effective_rf_power(), Some(0.5));
    assert_eq!(tci.set_drive(id, 50), Ok(50), "the level in force");
    assert_eq!(tci.set_drive(id, 40), Ok(40), "a program may lower it");
    assert_eq!(engine_lock(&engine).effective_rf_power(), Some(0.4));

    // The ceiling holds: the radio read back above the digital cap, a program lowers to a level
    // still above it, and the cap is what is commanded.
    let capped = armed();
    {
        let mut e = engine_lock(&capped);
        let mut s = e.settings().clone();
        s.max_power_digital = Some(0.3);
        e.apply_settings(s);
        e.observe_rig_power(0.5);
    }
    let (m2, _feed2) = serving(&capped);
    let h2 = handle(&m2);
    let (_c2, id2, _) = program(&h2, port(&m2));
    assert_eq!(m2.backend.set_drive(id2, 40), Ok(30));

    {
        let mut e = engine_lock(&engine);
        let mut s = e.settings().clone();
        s.tune_power_pct = Some(20);
        e.apply_settings(s);
    }
    assert_eq!(tci.set_tune_drive(id, 20), Ok(20));
    assert_eq!(tci.set_tune_drive(id, 30), Err(Refusal::Raise));
    assert_eq!(tci.set_tune_drive(id, 10), Err(Refusal::Tier));
}

/// The mode is the section's: the CAT broker's rule moves only the side inside its family, and
/// nothing moves while anything is on the air.
#[test]
fn a_program_s_mode_follows_the_cat_broker_s_rule() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    let word = |w: &str| Word::new(w).expect("a mode word");
    assert_eq!(tci.set_mode(id, &word("digl")), Ok(word("digl")));
    assert_eq!(engine_lock(&engine).rig_mode_effective(), "PKTLSB");
    assert_eq!(
        tci.set_mode(id, &word("usb")),
        Err(Refusal::Family),
        "SSB is not the Digital section's"
    );
    engine_lock(&engine).app.set_transmitting(true);
    assert_eq!(tci.set_mode(id, &word("digu")), Err(Refusal::OnAir));
    assert_eq!(engine_lock(&engine).rig_mode_effective(), "PKTLSB");
}

/// `tune:0,false` ends Tune whoever started it, and `cw_macros_stop` ends CW from any origin
/// (design §3.5). CONTROL: `cw_macros_stop` with an FT over on the air and no CW leaves the over
/// alone (`stop_cw` would also cut it).
#[test]
fn a_program_s_tune_and_cw_stops_end_only_what_they_name() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();

    engine_lock(&engine).set_tune(true);
    assert!(engine_lock(&engine).tuning(), "premise: the operator tunes");
    tci.tune_off(id);
    assert!(
        !engine_lock(&engine).tuning(),
        "a program's tune:0,false ends it"
    );

    {
        let mut e = engine_lock(&engine);
        e.set_operating_mode("cw", false);
        e.send_cw("CQ TEST");
        assert_eq!(e.tx_owner(), Some(TxOwner::Cw), "premise: CW is sending");
    }
    tci.cw_stop(id);
    assert_eq!(
        engine_lock(&engine).tx_owner(),
        None,
        "cw_macros_stop ends it"
    );

    let ft = armed();
    let (m2, _feed2) = serving(&ft);
    let h2 = handle(&m2);
    let (_c2, id2, _) = program(&h2, port(&m2));
    {
        let mut e = engine_lock(&ft);
        e.app.set_transmitting(true);
        let _ = e.take_slot_tx_abort();
    }
    m2.backend.cw_stop(id2);
    let mut e = engine_lock(&ft);
    assert!(
        !e.take_slot_tx_abort() && e.on_air(),
        "CONTROL: an FT over is not CW, and goes on"
    );
}

/// What programs are told is the engine's: the dial, the mode as a TCI word, power and Tune power,
/// `trx` for anything on the air (Tune included), the S-meter in dBm on each side of 30 MHz.
#[test]
fn the_station_programs_are_told_is_the_engine_s() {
    let engine = armed();
    let mut e = engine_lock(&engine);
    e.set_rf_power(0.4);
    let s = station(&e, &[], Some(6));
    assert_eq!(s.dial, hz(14_074_000));
    assert_eq!(s.mode.as_str(), "digu");
    assert_eq!((s.drive, s.tune_drive), (40, 40));
    assert!(!s.trx && !s.tune && !s.receive_only);
    assert_eq!(s.rx_dbm, Some(-67.0), "S9+6 dB on 20 m");
    assert_eq!(s.tx_buffering_ms, 100);
    e.set_tune(true);
    let tuned = station(&e, &[], None);
    assert!(tuned.trx && tuned.tune, "a tune carrier is on the air too");
    e.set_tune(false);
    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(station(&e, &[], Some(0)).rx_dbm, Some(-93.0), "S9 on 2 m");
}

/// Receive audio goes to each program that started it, in that program's own stream (rate,
/// format, channels, frame length), and to no other; once none listens, the pump lets its reader go,
/// so the feed copies nothing.
#[test]
fn receive_audio_goes_to_each_program_that_started_it_in_its_own_stream() {
    let engine = armed();
    let feed = Arc::new(ReceiveAudioFeed::default());
    let source = ReceiveSource {
        epoch: 1,
        rate: 48_000,
    };
    feed.replace_source(Some(source));
    let backend = Arc::new(EngineTci::new(engine.clone(), feed.clone()));
    let first = station(&engine_lock(&engine), &[], None);
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        backend,
        first,
    )
    .expect("a loopback port");
    let h = server.handle();
    let (mut wide, wide_id, _) = program(&h, server.port());
    let (mut narrow, narrow_id, _) = program(&h, server.port());
    let (mut deaf, _, _) = program(&h, server.port());
    wide.send("audio_start:0;");
    wide.lines_until("audio_start:0;");
    for line in [
        "audio_samplerate:12000;",
        "audio_stream_channels:1;",
        "audio_stream_sample_type:int16;",
        "audio_stream_samples:256;",
        "audio_start:0;",
    ] {
        narrow.send(line);
    }
    narrow.lines_until("audio_start:0;");

    let mut pump = PumpState::default();
    pump.turn(&h, &feed, None, Instant::now());
    // A subscription drops the first batch it sees; then 120 ms of a steady level.
    for _ in 0..7 {
        feed.publish(source, Instant::now(), &[0.25; 960]);
        pump.turn(&h, &feed, None, Instant::now());
    }

    let frames = |client: &mut Client| -> Vec<Vec<u8>> {
        client
            .drain(Duration::from_millis(300))
            .into_iter()
            .filter_map(|m| match m {
                Got::Binary(b) => Some(b),
                _ => None,
            })
            .collect()
    };
    let wide_frames = frames(&mut wide);
    assert!(wide_frames.len() >= 4, "{} frames", wide_frames.len());
    for f in &wide_frames {
        let head = RawHeader::read(f).expect("a header");
        assert_eq!(
            (
                head.kind,
                head.receiver,
                head.sample_rate,
                head.format,
                head.channels,
                head.length
            ),
            (
                BlockKind::RxAudio.code(),
                0,
                48_000,
                SampleFormat::Float32.code(),
                2,
                2048
            ),
            "the default stream: 48 kHz float32 stereo, 2048 values"
        );
        assert_eq!(f.len(), HEADER_LEN + 2048 * 4);
        assert!(
            f[HEADER_LEN..]
                .chunks_exact(4)
                .all(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) == 0.25),
            "48 kHz to 48 kHz is the audio itself"
        );
    }
    let narrow_frames = frames(&mut narrow);
    assert!(
        !narrow_frames.is_empty(),
        "the 12 kHz program is sent audio"
    );
    for f in &narrow_frames {
        let head = RawHeader::read(f).expect("a header");
        assert_eq!(
            (
                head.kind,
                head.sample_rate,
                head.format,
                head.channels,
                head.length
            ),
            (
                BlockKind::RxAudio.code(),
                12_000,
                SampleFormat::Int16.code(),
                1,
                256
            ),
            "its own stream: 12 kHz int16 mono, 256 values"
        );
        assert_eq!(f.len(), HEADER_LEN + 256 * 2);
    }
    assert!(
        frames(&mut deaf).is_empty(),
        "a program that did not start audio is sent none"
    );

    wide.send("audio_stop:0;");
    wide.lines_until("audio_stop:0;");
    narrow.send("audio_stop:0;");
    narrow.lines_until("audio_stop:0;");
    pump.turn(&h, &feed, None, Instant::now());
    assert!(
        feed.subscribe(source).is_ok(),
        "with no program listening the pump holds no reader"
    );
    let _ = (wide_id, narrow_id);
}

/// A backend that admits every key, so the server holds a `tci` key and would pass a TX_CHRONO
/// (tempo-net's own guard): the pump's own rule is all that is left to test.
struct Admits;

impl TciBackend for Admits {
    fn disconnected(&self, _: ClientId) {}
    fn set_dial(&self, _: ClientId, hz: Hz) -> Result<Hz, Refusal> {
        Ok(hz)
    }
    fn set_mode(&self, _: ClientId, mode: &Word) -> Result<Word, Refusal> {
        Ok(mode.clone())
    }
    fn set_drive(&self, _: ClientId, level: u32) -> Result<u32, Refusal> {
        Ok(level)
    }
    fn set_tune_drive(&self, _: ClientId, level: u32) -> Result<u32, Refusal> {
        Ok(level)
    }
    fn key(&self, _: ClientId, _: KeyKind, _: bool) -> Result<(), Refusal> {
        Ok(())
    }
    fn release(&self, _: ClientId) {}
    fn tune_on(&self, _: ClientId) -> Result<(), Refusal> {
        Ok(())
    }
    fn tune_off(&self, _: ClientId) {}
    fn end_own(&self, _: ClientId) {}
    fn tx_audio(&self, _: ClientId, _: AudioFormat, _: &[u8]) {}
    fn audio_start(&self, _: ClientId, _: AudioFormat) -> Result<(), Refusal> {
        Ok(())
    }
    fn audio_stop(&self, _: ClientId) {}
    fn cw_stop(&self, _: ClientId) {}
}

fn chronos(client: &mut Client, within: Duration) -> Vec<RawHeader> {
    client
        .drain(within)
        .into_iter()
        .filter_map(|m| match m {
            Got::Binary(b) => RawHeader::read(&b).filter(|h| h.kind == 3),
            _ => None,
        })
        .collect()
}

/// S-M34: TX_CHRONO goes only to the program whose over the engine says is armed, and only while it
/// holds a `tci` key: none with no armed over, none for an over armed for someone who holds no key.
/// CONTROL: armed for the key holder, the requests come, in its stream.
#[test]
fn tx_chrono_goes_only_to_the_program_whose_over_is_armed() {
    let feed = Arc::new(ReceiveAudioFeed::default());
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        Arc::new(Admits),
        station(&engine_lock(&armed()), &[], None),
    )
    .expect("a loopback port");
    let h = server.handle();
    let (mut keyed, keyed_id, _) = program(&h, server.port());
    let (mut other, other_id, _) = program(&h, server.port());
    keyed.send("trx:0,true,tci;");
    keyed.lines_until("trx:0,true;");
    assert_eq!(
        h.programs()
            .iter()
            .find(|p| p.id == keyed_id)
            .and_then(|p| p.key),
        Some(KeyKind::Tci),
        "premise: the server holds the program's tci key"
    );

    let mut pump = PumpState::default();
    for (armed, case) in [
        (None, "no over armed"),
        (Some(other_id), "an over armed for a program with no key"),
    ] {
        for _ in 0..10 {
            pump.turn(&h, &feed, armed, Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            chronos(&mut keyed, Duration::from_millis(100)).is_empty(),
            "{case}: no TX_CHRONO to the key holder"
        );
        assert!(
            chronos(&mut other, Duration::from_millis(50)).is_empty(),
            "{case}"
        );
    }

    for _ in 0..10 {
        pump.turn(&h, &feed, Some(keyed_id), Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    let asked = chronos(&mut keyed, Duration::from_millis(200));
    assert!(
        asked.len() >= 4,
        "CONTROL: the armed over's program is asked for audio: {}",
        asked.len()
    );
    assert!(asked
        .iter()
        .all(|c| (c.receiver, c.sample_rate, c.length) == (0, 48_000, 2048)));
    assert!(chronos(&mut other, Duration::from_millis(50)).is_empty());
}

/// The pacer asks for the over's lead at the arm, then one request per frame of real time, and a
/// late turn never makes a burst: after a stall it asks for the lead again, not for the time it lost.
#[test]
fn the_pacer_asks_for_the_lead_then_real_time_and_never_a_burst() {
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        Arc::new(Admits),
        station(&engine_lock(&armed()), &[], None),
    )
    .expect("a loopback port");
    let (_client, id, _) = program(&server.handle(), server.port());
    let t0 = Instant::now();
    let mut pacer = Pacer::new(id, AudioFormat::default(), t0).expect("the default stream");
    // 48 kHz, 1024 frames a request (2048 values stereo); the lead is 100 ms, 4800 frames.
    assert_eq!(pacer.due(t0), 4, "the lead at the arm");
    let mut asked = 4;
    for ms in (10..=1000).step_by(10) {
        asked += pacer.due(t0 + Duration::from_millis(ms));
    }
    let frames = asked * 1024;
    assert!(
        (48_000 + 4_800 - 1_024..=48_000 + 4_800).contains(&frames),
        "a second of real time and the lead: {frames} frames"
    );
    let late = pacer.due(t0 + Duration::from_millis(3000));
    assert!(
        late <= 5,
        "after a 2 s stall, the lead again, not a burst: {late}"
    );
}

/// The manager follows the switch and the port, as the CAT broker's does: on, a program connects
/// and the snapshot names the port and the program; a new port moves the server (the program on the
/// old one is told `trx:0,false;` and closed, and the old port is free); off, nothing listens and
/// the snapshot's line is gone.
#[test]
fn the_server_follows_its_switch_and_its_port() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let first = port(&m);
    let addrs = m
        .running
        .as_ref()
        .expect("serving")
        .server
        .local_addrs()
        .to_vec();
    assert!(
        !addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback()),
        "this computer only: {addrs:?}"
    );
    let h = handle(&m);
    let (mut client, id, burst) = program(&h, first);
    assert!(burst.contains(&"device:Nexus;".to_string()));
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let status = status.expect("the snapshot carries the server");
    assert_eq!(status.port, Some(first));
    assert_eq!(status.programs.len(), 1);
    assert_eq!(status.programs[0].id, id.get());
    assert!(status.programs[0].peer.starts_with("127.0.0.1:"));

    let quiet = crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
    let moved_to = quiet.local_addr().expect("a port").port();
    drop(quiet);
    m.apply(Some(moved_to));
    let second = port(&m);
    assert!(
        second == moved_to || second == moved_to + 1,
        "the new port, or the next: {second}"
    );
    let told = client.drain(Duration::from_millis(500));
    assert!(
        told.iter()
            .any(|g| matches!(g, Got::Text(t) if t == "trx:0,false;")),
        "the program on the old port is told nothing is on the air: {told:?}"
    );
    assert!(
        told.iter().any(|g| matches!(g, Got::Close(Some(1001)))),
        "…and closed as going away: {told:?}"
    );
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, first)).is_err(),
        "the old port is free"
    );
    let (_c2, _, _) = program(&handle(&m), second);
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    assert_eq!(status.and_then(|s| s.port), Some(second));

    m.apply(None);
    assert!(m.running.is_none());
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, second)).is_err(),
        "switched off, nothing listens"
    );
    m.publish();
    assert_eq!(
        engine_lock(&engine).snapshot().radio.tci_server,
        None,
        "switched off, the snapshot has no line for it"
    );
}

/// A port that will not bind is said once, in the snapshot and the log, and retried quietly; the
/// server starts once the port is free and the retry is due.
#[test]
fn a_port_that_will_not_bind_is_reported_once_and_retried() {
    let engine = armed();
    let feed = Arc::new(ReceiveAudioFeed::default());
    let mut m = Manager::new(engine.clone(), feed, MeterFeed::default());
    let held = crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
    let taken = held.local_addr().expect("a port").port();
    let next = TcpListener::bind((Ipv4Addr::LOCALHOST, taken + 1));
    m.apply(Some(taken));
    assert!(m.running.is_none(), "both ports are taken");
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let status = status.expect("switched on, the snapshot says why it is not serving");
    assert_eq!(status.port, None);
    let why = status.error.expect("a reason");
    assert!(
        why.contains(&format!("ports {taken} and {}", taken + 1)),
        "{why}"
    );
    let said = |why: &str| {
        crate::CONN_LOG
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|ev| ev.connector == LANE && ev.message.reveal_on_screen() == why)
            .count()
    };
    assert_eq!(said(&why), 1, "said in the connection log");
    m.apply(Some(taken));
    assert!(m.running.is_none(), "not retried before the retry is due");
    m.retry = Duration::ZERO;
    m.apply(Some(taken));
    assert!(m.running.is_none(), "still taken at the retry");
    assert_eq!(said(&why), 1, "said once, however often it is retried");

    drop(held);
    drop(next);
    m.apply(Some(taken));
    assert!(m.running.is_some(), "the free port is taken at the retry");
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    assert_eq!(status.and_then(|s| s.error), None);
}

/// The operator's Disconnect closes the program (1000) and it leaves the list; a number no program
/// has does nothing.
#[test]
fn the_operator_s_disconnect_closes_the_program() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, _) = program(&h, port(&m));
    assert!(!m.serving.disconnect(id.get() + 100), "no such program");
    assert!(m.serving.disconnect(id.get()));
    let told = client.drain(Duration::from_millis(500));
    assert!(
        told.iter().any(|g| matches!(g, Got::Close(Some(1000)))),
        "{told:?}"
    );
    let until = Instant::now() + WAIT;
    while !h.programs().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.programs().is_empty(), "the program left the list");
}

/// A web page is refused at the handshake (tempo-net's gate), and the operator reads which page in
/// the connection log.
#[test]
fn a_web_page_s_connection_is_refused_and_named_in_the_log() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let p = port(&m);
    let origin = "https://tci-page.example";
    let request = tungstenite::handshake::client::Request::builder()
        .uri(format!("ws://127.0.0.1:{p}/"))
        .header("Host", format!("127.0.0.1:{p}"))
        .header("Origin", origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .expect("a request");
    let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, p)).expect("the server accepts");
    assert!(
        tungstenite::client(request, tcp).is_err(),
        "a handshake carrying Origin is refused"
    );
    assert!(eventually(&engine, |_| logged(&format!(
        "refused a web page ({origin})"
    ))));
}
