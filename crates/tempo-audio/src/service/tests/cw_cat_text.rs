//! ⛔ THE RADIO'S OWN CW KEYER IS NEVER HANDED A `;`. Hamlib sends the CAT keyer's text inside a
//! radio command (`send_morse`: `KY <text>;` to a Kenwood or an Elecraft, `KM1<text>;` to a newer
//! Yaesu), and those radios end a command at the first `;`: one in the text ended the keyer's
//! command early, and the radio read the rest as a command of its own. Hamlib's TS-2000 backend
//! wrote `KY TEST;` and then `FA00007000000      ;` for `b TEST;FA00007000000` (rigctl 4.5.5, a
//! fake radio on a pty): a retune to 7.000 MHz while the radio keys CW. The text reaches the
//! keyer from the operator's typing and macros, from a callbook (`{HISNAME}`, `{HISSTATE}`) and
//! from a computer driving the station. Each test runs the real `RadioLoop::step` against a
//! rigctld that records every line exactly as it was sent.

use super::cw_stop::{lines_since, step};
use super::*;
use std::io::{BufRead, BufReader, Write};

/// The second command the earlier measurement saw the radio read after the CW text's `;`: VFO A
/// to 7.000 MHz.
const RETUNE: &str = "FA00007000000";

/// A rigctld that records each line it is sent exactly as sent (only the `\n` that ends it taken
/// off), and answers `f` 7.030 MHz, `m` CW, `t` not keyed and anything else `RPRT 0`.
fn recording_rigctld() -> (String, Arc<Mutex<Vec<String>>>) {
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
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match reader.read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&bytes);
                let line = line.strip_suffix('\n').unwrap_or(&line).to_string();
                let reply = match line.trim() {
                    "f" => "7030000\n",
                    "m" => "CW\n500\n",
                    "t" => "0\n",
                    _ => "RPRT 0\n",
                };
                log2.lock().unwrap().push(line);
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// A TS-2000 on the CAT keyer in the CW section on 40 m, at 20 WPM.
fn station() -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 2014;
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer("cat", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
    }
    engine
}

/// The radio loop, its rig on the recording rigctld, and the time of its next tick (ms).
struct Scene {
    engine: Arc<Mutex<Engine>>,
    log: Arc<Mutex<Vec<String>>>,
    rig: Rig,
    backend: MockBackend,
    state: RadioLoop,
    t: f64,
}

impl Scene {
    fn new(engine: Arc<Mutex<Engine>>) -> Scene {
        let (addr, log) = recording_rigctld();
        let state = loop_state_for(&engine);
        Scene {
            engine,
            log,
            rig: Rig::rigctld(&addr),
            backend: MockBackend::new(),
            state,
            t: 100.0,
        }
    }

    /// Run `ticks` ticks of the loop, 5 s apart: a tick takes the next word once the one before has
    /// keyed and its word space passed (under 5 s at 20 WPM for every word here but the long name,
    /// about 18 s).
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            step(
                &mut self.state,
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                self.t,
            );
            self.t += 5_000.0;
        }
    }

    /// Every line the rigctld was sent from `from` on.
    fn lines(&self, from: usize) -> Vec<String> {
        lines_since(&self.log, from)
    }

    /// How many lines the rigctld has been sent.
    fn sent(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    fn keyer_error(&self) -> Option<String> {
        self.engine.lock().unwrap().cw_keyer_error()
    }
}

/// The lines that handed the radio's keyer a word (`b <word>`, Hamlib's send_morse), as sent.
fn keyed(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.starts_with("b "))
        .cloned()
        .collect()
}

/// After a refused word: the controls, each in the scene the refusal left. A new message keys from
/// the next tick: nothing waits on a word that was never sent, and none of the refused message is
/// left to key first. Ordinary text keys exactly as before, its one-character word padded to two
/// (`b K `, issue #86), and the CW screen's warning goes once a word keys; then the punctuation and
/// prosigns Nexus's table has (`?`, `=`, `+`, `/`, `(`, `)`) key too.
fn controls_key_as_before(s: &mut Scene, send: impl Fn(&mut Engine, &str)) {
    let from = s.sent();
    send(&mut s.engine.lock().unwrap(), "CQ TEST DE KD9TAW K");
    s.run(1);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b CQ"],
        "control: a new message keys at once after a refused word"
    );
    s.run(5);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b CQ", "b TEST", "b DE", "b KD9TAW", "b K "],
        "control: ordinary text keys exactly as before"
    );
    assert_eq!(s.keyer_error(), None, "control: a word that keys clears it");
    let from = s.sent();
    send(&mut s.engine.lock().unwrap(), "TU 5NN? = + R/ (KN)");
    s.run(7);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b TU", "b 5NN?", "b = ", "b + ", "b R/", "b (KN)"],
        "control: the punctuation and prosigns Nexus's table has still key"
    );
    assert_eq!(s.keyer_error(), None);
}

/// ⛔ A `;` TYPED IN THE CW SCREEN (or in an F-key macro) NEVER REACHES THE RADIO'S KEYER. The word
/// that holds it is not sent, the rest of the message is dropped with it, as for a refused key, and
/// the CW screen says why, once. The word before it keys as it always did. CONTROLS: the same
/// scene then keys ordinary text and the punctuation Nexus's table has exactly as before.
#[test]
fn a_semicolon_typed_in_cw_never_reaches_the_radios_keyer_and_the_cw_screen_says_why() {
    let mut s = Scene::new(station());
    s.engine
        .lock()
        .unwrap()
        .send_cw(&format!("CQ TEST;{RETUNE} DE KD9TAW"));
    s.run(6);
    let lines = s.lines(0);
    assert!(
        !lines.iter().any(|l| l.contains(RETUNE)),
        "the word with the `;` reached the rigctld: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        ["b CQ"],
        "the word before it keys, and nothing after it: the rest of the message is dropped"
    );
    let why = s.keyer_error().expect("the CW screen says why");
    assert!(
        why.starts_with("CW not sent: \";\""),
        "the CW screen says the `;` was not sent: {why}"
    );
    controls_key_as_before(&mut s, |e, text| e.send_cw(text));
}

/// ⛔ A CALLBOOK NAME OR STATE WITH A `;` KEYS THE REST, NEVER THE `;`. `{HISNAME}` and `{HISSTATE}`
/// are the worked station's own callbook entry, put into Nexus's CW by an F-key macro. Each is kept
/// to what the keyers send where it enters, so a `;` is taken out, and the name and the state still
/// key around it: no word carries a `;`, so each is one command to the radio. CONTROL: a plain
/// name keys as it is.
#[test]
fn a_callbook_name_with_a_semicolon_keys_the_rest_of_the_name_and_never_the_semicolon() {
    let mut s = Scene::new(station());
    {
        let mut e = s.engine.lock().unwrap();
        e.select_peer("K2DEF");
        e.set_cw_peer_info("K2DEF".into(), format!("BOB;{RETUNE}"), "O;H".into());
        e.send_cw("TU {HISNAME} {HISSTATE} 73");
    }
    s.run(10);
    let lines = s.lines(0);
    assert!(
        !keyed(&lines).iter().any(|l| l.contains(';')),
        "a `;` reached the radio's keyer: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        ["b TU", &format!("b BOB{RETUNE}"), "b OH", "b 73"],
        "the rest of the name, and the state, key"
    );
    assert_eq!(s.keyer_error(), None);
    // CONTROL: a plain name keys as it is.
    let from = s.sent();
    {
        let mut e = s.engine.lock().unwrap();
        e.set_cw_peer_info("K2DEF".into(), "Bob".into(), "OH".into());
        e.send_cw("TU {HISNAME} {HISSTATE}");
    }
    s.run(4);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b TU", "b Bob", "b OH"],
        "control: a plain name keys as it is"
    );
}

/// ⛔ CW TYPED ON A COMPUTER DRIVING THE STATION TAKES THE SAME ROAD, AND THE SAME RULE. Remote's CW
/// reaches the station's own `send_cw` handler (src-tauri), which runs exactly this for a call from
/// afar: the text as that computer sent it into the engine's one CW queue, then the note that a
/// start from afar ran. The radio loop keys it as it keys the station's own, so the word with the
/// `;` never reaches the radio's keyer, the rest is dropped and the CW screen says why. CONTROLS:
/// from afar, ordinary text and the table's punctuation key exactly as before.
#[test]
fn cw_typed_from_afar_never_hands_the_radios_keyer_a_semicolon() {
    let from_afar = |e: &mut Engine, text: &str| {
        let under = e.remote_transmit_stand_down_generation();
        e.send_cw_as(text, false);
        e.note_started_from_afar(under);
    };
    let mut s = Scene::new(station());
    from_afar(
        &mut s.engine.lock().unwrap(),
        &format!("CQ TEST;{RETUNE} DE KD9TAW"),
    );
    // Two ticks: `CQ`, then the refused word. The controls' message follows on the very next tick,
    // well inside the refused word's own length (18 s of Morse), so nothing may wait on it, and its
    // first word is the one keyed only if the rest of this message was dropped.
    s.run(2);
    let lines = s.lines(0);
    assert!(
        !lines.iter().any(|l| l.contains(RETUNE)),
        "the word with the `;` from afar reached the rigctld: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        ["b CQ"],
        "the word before it keys, and nothing after it: the rest of the message is dropped"
    );
    let why = s.keyer_error().expect("the CW screen says why");
    assert!(
        why.starts_with("CW not sent: \";\""),
        "the CW screen says the `;` was not sent: {why}"
    );
    controls_key_as_before(&mut s, from_afar);
}
