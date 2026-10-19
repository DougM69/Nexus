//! Stop TX (Esc) in the middle of a CW message ends the keying on every keyer, and the stops that
//! need no answer from the radio go out first.
//!
//! The field report (IL QSO Party, 1.20.0-test4): "I saw the TX stopped with Esc, but a CW macro
//! already started did not immediately stop." The halt reaches the engine and the TX indicator
//! drops at once; what keys the rest of a word is whatever the keyer still holds. Each test runs
//! the real `RadioLoop::step` against a rigctld that logs every line it is sent.

use super::*;
use std::io::{BufRead, BufReader, Write};

/// A rigctld that logs each line it is sent and answers it, `hook` first: `Some(reply)` answers
/// with that (an empty reply writes nothing, as Hamlib does for a raw `w` set), `None` falls
/// through to `RPRT 0`.
fn logging_rigctld(
    hook: impl Fn(&str) -> Option<String> + Send + 'static,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let reply = hook(&l).unwrap_or_else(|| match l.as_str() {
                    "f" => "7030000\n".to_string(),
                    "m" => "CW\n500\n".to_string(),
                    "t" => "0\n".to_string(),
                    _ => "RPRT 0\n".to_string(),
                });
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// An engine in the CW section on 40 m with `keyer`, on `model`, its macro word queued.
fn cw_engine(
    model: u32,
    keyer: &str,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        set(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer(keyer, 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
        e.send_cw("TEST DE KD9TAW");
    }
    engine
}

/// Step the loop once at `t` ms.
fn step(
    state: &mut RadioLoop,
    engine: &Arc<Mutex<Engine>>,
    backend: &mut MockBackend,
    rig: &mut Rig,
    t: f64,
) {
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    state
        .step(
            engine,
            backend,
            rig,
            &sinks,
            t,
            &mut ra,
            &mut rr,
            &mut station,
        )
        .unwrap();
}

/// The lines the rigctld was sent from `from` on.
fn lines_since(log: &Mutex<Vec<String>>, from: usize) -> Vec<String> {
    log.lock().unwrap()[from..].to_vec()
}

/// Key the first word, then press Stop TX (Esc) and run the tick that acts on it. Returns the
/// rigctld's lines from the stop on.
fn key_then_stop(
    engine: &Arc<Mutex<Engine>>,
    backend: &mut MockBackend,
    rig: &mut Rig,
    log: &Mutex<Vec<String>>,
) -> Vec<String> {
    let mut state = loop_state_for(engine);
    step(&mut state, engine, backend, rig, 100.0);
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().halt_tx();
    step(&mut state, engine, backend, rig, 140.0);
    lines_since(log, from)
}

/// ⭐ THE SOUNDCARD KEYER UNKEYS BEFORE THE RADIO IS ASKED TO STOP ITS OWN CW. The soundcard
/// keyer's tone is Nexus's audio: the flush and the unkey end it. The CAT CW stop (`\stop_morse`)
/// is a question to the radio that a slow or silent radio holds for a whole CAT deadline, and it
/// used to go first, so the carrier stayed up behind it.
#[test]
fn a_soundcard_stop_unkeys_before_the_radio_is_asked_to_stop_cw() {
    let engine = cw_engine(3073, "soundcard", |_| {});
    let (addr, log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    // POSITIVE CONTROL: the word was keyed and played, or the stop has nothing to stop.
    assert!(
        log.lock().unwrap().iter().any(|l| l == "T 1"),
        "control: the soundcard word must key the radio: {:?}",
        log.lock().unwrap()
    );
    assert!(
        !backend.played.is_empty(),
        "control: the word's tone must be playing"
    );
    let flushes = backend.flush_calls;
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().halt_tx();
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    let after = lines_since(&log, from);
    let unkey = after.iter().position(|l| l == "T 0");
    let cw_stop = after.iter().position(|l| l == "\\stop_morse");
    assert!(backend.flush_calls > flushes, "the tone is flushed");
    assert!(
        matches!((unkey, cw_stop), (Some(u), Some(c)) if u < c),
        "the unkey (T 0) must go out before the CAT CW stop: {after:?}"
    );
}

/// ⭐ A YAESU WHOSE CAT REFERENCE HAS A PLAYBACK STOP GETS IT. The bundled Hamlib (4.7.1) sends
/// a Yaesu newcat radio's CAT CW as `KM1<text>;` `KY6;` and has no `stop_morse` for it at all
/// (`\stop_morse` answers `RPRT -11` and nothing reaches the radio), so the word in the radio
/// played to its end. Yaesu's own references define `KY P1 P2;` with P2 `0` as the stop of a CW
/// TEXT (P1 `0`) or MESSAGE (P1 `1`) memory's playback for the FT-710 and the FTX-1, and Nexus
/// now sends both after `\stop_morse`. The FT-991A and the FTDX10 references have no stop form
/// of `KY`, so nothing raw is sent to them, nor to any other make.
#[test]
fn an_ft710_and_an_ftx1_stop_gets_yaesus_own_playback_stop_and_no_other_radio_does() {
    for (model, name, stops) in [
        (1049u32, "FT-710", vec!["w KY00;", "w KY10;"]),
        (1051, "FTX-1", vec!["w KY00;", "w KY10;"]),
        (1035, "FT-991A", vec![]),
        (1042, "FTDX10", vec![]),
        (3073, "IC-7300", vec![]),
        (2037, "TS-590SG", vec![]),
    ] {
        let engine = cw_engine(model, "cat", |_| {});
        // A raw `w` set draws no answer from Hamlib (measured on an FT-710, `Rig::send_raw_set`).
        let (addr, log) = logging_rigctld(|l| l.starts_with("w KY").then(String::new));
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let after = key_then_stop(&engine, &mut backend, &mut rig, &log);
        // POSITIVE CONTROL: the CAT keyer keyed the word, and the stop asked the radio.
        assert!(
            log.lock().unwrap().iter().any(|l| l.starts_with("b TEST")),
            "{name}: control: the word must go to the radio's keyer: {:?}",
            log.lock().unwrap()
        );
        let cw_stop = after
            .iter()
            .position(|l| l == "\\stop_morse")
            .unwrap_or_else(|| panic!("{name}: the CAT CW stop must go out: {after:?}"));
        // The raw stops are written without waiting for an answer; give the rigctld time to
        // read them (and, where none is expected, time to prove none came).
        std::thread::sleep(Duration::from_millis(200));
        let raw: Vec<String> = lines_since(&log, 0)
            .into_iter()
            .filter(|l| l.starts_with("w KY"))
            .collect();
        assert_eq!(raw, stops, "{name}: the raw playback stops sent");
        if let Some(first) = after.iter().position(|l| l.starts_with("w KY")) {
            assert!(cw_stop < first, "{name}: after the CAT CW stop: {after:?}");
        }
    }
}

/// ⭐ THE WINKEYER IS CLEARED BEFORE THE RADIO IS ASKED TO STOP ITS CW. K1EL's Clear Buffer
/// (`0A`) "will cancel any Morse character in progress immediately ending it in midstream" (WK3
/// datasheet, p. 10). It is one byte on a serial port Nexus owns. It used to wait behind the CAT
/// CW stop, which a slow or silent radio holds for a whole CAT deadline, while the WinKeyer kept
/// keying. The rigctld here looks at the keyer's wire at the moment `\stop_morse` reaches it.
#[cfg(target_os = "linux")]
#[cfg(feature = "serial")]
#[test]
fn a_winkeyer_is_cleared_before_the_radio_is_asked_to_stop_cw() {
    use serialport::SerialPort;
    use std::io::Read;

    let (keyer, slave) = serialport::TTYPort::pair().expect("a pty pair");
    let port = slave.name().expect("the pts side has a path");
    let mut responder = keyer.try_clone().expect("clone the pty master");
    responder
        .set_timeout(Duration::from_millis(3000))
        .expect("bounded read");
    std::thread::spawn(move || {
        let mut host_open = [0u8; 2];
        if responder.read_exact(&mut host_open).is_ok() {
            let _ = responder.write_all(&[0x17]); // WK2, revision 23
            let _ = responder.flush();
        }
    });
    let wire = Arc::new(Mutex::new((keyer, Vec::<u8>::new())));
    let seen = Arc::clone(&wire);
    // What the keyer's wire held each time `\stop_morse` arrived: had it been cleared yet?
    let cleared_at_cw_stop = Arc::new(Mutex::new(Vec::<bool>::new()));
    let record = Arc::clone(&cleared_at_cw_stop);
    let (addr, log) = logging_rigctld(move |l| {
        if l == "\\stop_morse" {
            let mut w = seen.lock().unwrap();
            let (k, bytes) = &mut *w;
            k.set_timeout(Duration::from_millis(50)).unwrap();
            let mut buf = [0u8; 256];
            while let Ok(n) = k.read(&mut buf) {
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            record
                .lock()
                .unwrap()
                .push(bytes.contains(&crate::winkeyer::CLEAR_BUFFER[0]));
        }
        None
    });
    let engine = cw_engine(3073, "winkeyer", |s| s.winkeyer_port = port.clone());
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let after = key_then_stop(&engine, &mut backend, &mut rig, &log);
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error(),
        None,
        "control: the stand-in keyer must open cleanly"
    );
    assert!(
        after.iter().any(|l| l == "\\stop_morse"),
        "control: the CAT CW stop still goes out: {after:?}"
    );
    let bytes = wire.lock().unwrap().1.clone();
    assert!(
        bytes.windows(4).any(|w| w == b"TEST"),
        "control: the word reached the keyer's wire: {bytes:02x?}"
    );
    assert_eq!(
        cleared_at_cw_stop.lock().unwrap().as_slice(),
        &[true],
        "the WinKeyer's Clear Buffer must already be on its wire when the radio is asked: {bytes:02x?}"
    );
}
