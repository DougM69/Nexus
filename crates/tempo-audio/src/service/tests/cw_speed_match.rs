//! CW SPEED MATCH on every keyer: an answer to a station keys at the speed the decoder measured
//! for it, and the keyer is told that speed BEFORE the answer's first word, never inside it; once
//! the answer has keyed, the keyer goes back to the operator's own speed (a CQ after it keys
//! there). With speed match off nothing changes. Each test runs the real `RadioLoop::step`.

use super::cw_stop::{keyer_speed_rigctld, lines_since, logging_rigctld, step, word_window_ms};
use super::*;

const OWN: u32 = 25;

/// An engine in the CW section on 40 m on `model` with `keyer`, at the operator's own 25 WPM,
/// speed match `on` (15..=35), working W1AW, which was just heard at 30 WPM. Returns the engine
/// and the speed the decoder measured.
fn station(
    model: u32,
    keyer: &str,
    on: bool,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> (Arc<Mutex<Engine>>, u32) {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    let measured = {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        s.cw_speed_match = on;
        s.cw_speed_match_min = 15;
        s.cw_speed_match_max = 35;
        set(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer(keyer, 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(OWN);
        e.select_peer("W1AW");
        hear(&mut e, "TEST W1AW", 30);
        e.cw_decode().wpm
    };
    assert!(
        (28..=32).contains(&measured) && measured != OWN,
        "premise: the station measured at about 30, not my own speed: {measured}"
    );
    (engine, measured)
}

/// The station on the operator's pitch: `text` at `wpm`, then a second of silence.
fn hear(e: &mut Engine, text: &str, wpm: u32) {
    let sr = tempo_fast::SAMPLE_RATE;
    let mut audio = vec![0.0f32; (sr * 0.2) as usize];
    audio.extend(tempo_core::cw::morse_samples(text, wpm, 600.0, sr as u32));
    audio.extend(vec![0.0f32; sr as usize]);
    for chunk in audio.chunks(1200) {
        e.feed_rx_audio(chunk);
    }
}

/// Step the loop every 5 ms over `[from, to)`; the speed sets (`L KEYSPD`) and the words
/// (`b <word>`) the radio was sent, each with the time it went.
#[allow(clippy::too_many_arguments)]
fn keyed(
    state: &mut RadioLoop,
    engine: &Arc<Mutex<Engine>>,
    backend: &mut MockBackend,
    rig: &mut Rig,
    log: &Mutex<Vec<String>>,
    from: f64,
    to: f64,
) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut seen = log.lock().unwrap().len();
    let mut t = from;
    while t < to {
        step(state, engine, backend, rig, t);
        let lines = lines_since(log, seen);
        seen += lines.len();
        out.extend(
            lines
                .into_iter()
                .filter(|l| l.starts_with("L KEYSPD") || l.starts_with("b "))
                .map(|l| (l, t)),
        );
        t += 5.0;
    }
    out
}

fn names(lines: &[(String, f64)]) -> Vec<&str> {
    lines.iter().map(|(l, _)| l.as_str()).collect()
}

fn at(lines: &[(String, f64)], line: &str) -> f64 {
    lines
        .iter()
        .find(|(l, _)| l == line)
        .unwrap_or_else(|| panic!("{line} never went: {lines:?}"))
        .1
}

/// ⭐ CAT: the radio is told the station's speed before the answer's first word, keeps it for the
/// whole answer, and is told the operator's own once the last word has keyed, before any CQ.
#[test]
fn a_cat_answer_sets_the_radios_speed_before_it_and_mine_after_it() {
    let (engine, m) = station(3073, "cat", true, |_| {});
    let (addr, log, knob) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw_as("{MYCALL} 5NN", true);
    let answer = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        5_000.0,
    );
    engine.lock().unwrap().send_cw("CQ");
    let cq = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        5_000.0,
        7_000.0,
    );
    let set_m = format!("L KEYSPD {m}");
    assert_eq!(
        (names(&answer), names(&cq)),
        (
            vec![set_m.as_str(), "b KD9TAW", "b 5NN", "L KEYSPD 25"],
            vec!["b CQ"]
        ),
        "the station's speed before the answer, mine after it, nothing inside it"
    );
    let back = at(&answer, "L KEYSPD 25") - at(&answer, "b 5NN");
    let window = word_window_ms("5NN", m);
    assert!(
        back >= window && back < window + 5.0,
        "my speed goes back as the answer's last word ends ({window} ms), not before: {back}"
    );
    assert_eq!(knob.load(std::sync::atomic::Ordering::Relaxed), OWN);
    assert_eq!(
        engine.lock().unwrap().cw_wpm(),
        OWN,
        "my own speed unchanged"
    );
}

/// Speed match off: the wire is exactly what it was, the operator's speed once, then the words.
#[test]
fn speed_match_off_leaves_the_cat_wire_as_it_was() {
    let (engine, _) = station(3073, "cat", false, |_| {});
    let (addr, log, _) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw_as("{MYCALL} 5NN", true);
    let answer = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        5_000.0,
    );
    engine.lock().unwrap().send_cw("CQ");
    let cq = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        5_000.0,
        7_000.0,
    );
    assert_eq!(
        (names(&answer), names(&cq)),
        (vec!["L KEYSPD 25", "b KD9TAW", "b 5NN"], vec!["b CQ"])
    );
}

/// ⭐ SMARTSDR CAT, WHERE NEXUS FOLLOWS THE RADIO'S SPEED: the speed Nexus set for the answer is
/// Nexus's own, so reading it back from the radio is not taken for the operator turning the knob.
/// The WPM control stays the operator's, the answer is paced at the station's speed, and the
/// operator's speed goes back afterwards.
#[test]
fn a_smartsdr_cat_answer_is_not_overwritten_by_following_the_radio() {
    let (engine, m) = station(2036, "cat", true, |_| {});
    let (addr, log, knob) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw_as("{MYCALL} 5NN", true);
    let answer = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        5_000.0,
    );
    assert!(
        log.lock().unwrap().iter().any(|l| l == "l KEYSPD"),
        "control: the radio's speed was read back (the follow ran)"
    );
    let set_m = format!("L KEYSPD {m}");
    assert_eq!(
        names(&answer),
        vec![set_m.as_str(), "b KD9TAW", "b 5NN", "L KEYSPD 25"]
    );
    let gap = at(&answer, "b 5NN") - at(&answer, "b KD9TAW");
    let window = word_window_ms("KD9TAW", m);
    assert!(
        gap >= window && gap < window + 5.0,
        "paced at the station's {m} WPM ({window} ms): {gap}"
    );
    assert_eq!(
        engine.lock().unwrap().cw_wpm(),
        OWN,
        "the control is not moved to the answer's speed"
    );
    assert_eq!(knob.load(std::sync::atomic::Ordering::Relaxed), OWN);
}

/// The operator turning the radio's knob during an answer is followed, and the rest of the
/// answer keys at the knob's speed: Nexus does not push the matched speed back over it.
#[test]
fn a_knob_turned_during_an_answer_wins_over_the_match() {
    let (engine, m) = station(2036, "cat", true, |_| {});
    let (addr, log, knob) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw_as("{MYCALL} 5NN TU", true);
    let first = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        110.0,
    );
    assert_eq!(
        names(&first),
        vec![format!("L KEYSPD {m}").as_str(), "b KD9TAW"]
    );
    knob.store(22, std::sync::atomic::Ordering::Relaxed);
    let rest = keyed(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        110.0,
        6_000.0,
    );
    assert_eq!(
        names(&rest),
        vec!["b 5NN", "b TU"],
        "no speed pushed back: {rest:?}"
    );
    assert_eq!(engine.lock().unwrap().cw_wpm(), 22, "the knob is followed");
}

/// ⭐ WINKEYER: its Set Speed carries the station's speed before the answer's first character,
/// none goes out inside the answer, and the operator's own goes out once its last word has keyed
/// (the paddle then keys at it too).
#[cfg(target_os = "linux")]
#[cfg(feature = "serial")]
#[test]
fn a_winkeyer_answer_sets_its_speed_before_it_and_mine_after_it() {
    use serialport::SerialPort;
    use std::io::{Read, Write};

    let (mut keyer, slave) = serialport::TTYPort::pair().expect("a pty pair");
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
    let drain = |k: &mut serialport::TTYPort| {
        k.set_timeout(Duration::from_millis(50)).unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        while let Ok(n) = k.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    };
    let (engine, m) = station(3073, "winkeyer", true, |s| s.winkeyer_port = port.clone());
    let mut rig = Rig::vox();
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    step(&mut state, &engine, &mut backend, &mut rig, 50.0); // the configured keyer opens
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error(),
        None,
        "control: the stand-in keyer must open cleanly"
    );
    let opening = drain(&mut keyer);
    engine.lock().unwrap().send_cw_as("{MYCALL} 5NN", true);
    let mut handed = Vec::new();
    let mut t = 100.0;
    while handed.len() < 2 {
        let busy = state.cw_busy_until;
        step(&mut state, &engine, &mut backend, &mut rig, t);
        if state.cw_busy_until != busy {
            handed.push(t);
        }
        t += 5.0;
        assert!(t < 5_000.0, "the answer never keyed");
    }
    // Up to just before the last word's window ends: no speed may change yet.
    let end = handed[1] + word_window_ms("5NN", m);
    while t < end - 5.0 {
        step(&mut state, &engine, &mut backend, &mut rig, t);
        t += 5.0;
    }
    let answer = drain(&mut keyer);
    while t < end + 10.0 {
        step(&mut state, &engine, &mut backend, &mut rig, t);
        t += 5.0;
    }
    let after = drain(&mut keyer);
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    assert!(
        find(&opening, &crate::winkeyer::wpm_cmd(OWN)).is_some(),
        "control: the keyer opened at my own speed: {opening:02x?}"
    );
    let set_m = find(&answer, &crate::winkeyer::wpm_cmd(m));
    let call = find(&answer, b"KD9TAW");
    let report = find(&answer, b"5NN");
    assert!(
        matches!((set_m, call, report), (Some(s), Some(c), Some(r)) if s < c && c < r),
        "the station's speed before the answer's first character: {answer:02x?}"
    );
    let speed_sets = answer.windows(2).filter(|w| w[0] == 0x02).count();
    assert_eq!(
        speed_sets, 1,
        "one Set Speed, before the answer: {answer:02x?}"
    );
    assert_eq!(
        after,
        crate::winkeyer::wpm_cmd(OWN).to_vec(),
        "my own speed once the answer has keyed"
    );
}

/// The soundcard keyer generates each word's tone at the speed it is given: the answer's words at
/// the station's speed, the CQ after it at mine.
#[test]
fn a_soundcard_answer_is_toned_at_the_station_speed_and_the_cq_at_mine() {
    let (engine, m) = station(3073, "soundcard", true, |_| {});
    let (addr, log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let tone = |word: &str, wpm: u32| {
        tempo_core::cw::morse_samples(word, wpm, 600.0, tempo_fast::SAMPLE_RATE as u32).len()
    };
    engine.lock().unwrap().send_cw_as("{MYCALL}", true);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    assert!(
        log.lock().unwrap().iter().any(|l| l == "T 1"),
        "control: the word keyed"
    );
    assert_eq!(
        backend.played.len(),
        tone("KD9TAW", m),
        "the answer at {m} WPM"
    );
    let mut t = 105.0;
    while state.cw_busy_until > 0.0 && t < state.cw_busy_until + 10.0 {
        step(&mut state, &engine, &mut backend, &mut rig, t);
        t += 5.0;
    }
    let before = backend.played.len();
    engine.lock().unwrap().send_cw("CQ");
    step(&mut state, &engine, &mut backend, &mut rig, t + 5.0);
    assert_eq!(
        backend.played.len() - before,
        tone("CQ", OWN),
        "the CQ at my own speed"
    );
}

/// Every keyer is paced by the speed its word is keyed at: the loop holds the next word for the
/// answer's word at the station's speed. The serial keyline is handed the same speed with the word
/// (`SerialKeyer::send(word, wpm)`); its port cannot be opened on a pty here, which the loop
/// reports, and the word's pacing still shows the speed it was given.
#[cfg(feature = "serial")]
#[test]
fn a_serial_keyline_answer_is_paced_at_the_station_speed() {
    let (engine, m) = station(3073, "serial", true, |s| {
        s.cw_key_port = "/dev/nonexistent-nexus-keyline".into();
    });
    let mut rig = Rig::vox();
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw_as("{MYCALL}", true);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    assert!(
        engine.lock().unwrap().cw_keyer_error().is_some(),
        "control: the serial branch owned the word"
    );
    assert_eq!(state.cw_word_matched, Some(m));
    assert!(
        (state.cw_busy_until - 100.0 - word_window_ms("KD9TAW", m)).abs() < 1e-6,
        "held for the word at {m} WPM: {}",
        state.cw_busy_until - 100.0
    );
}
