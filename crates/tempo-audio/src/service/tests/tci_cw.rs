//! A TCI program's CW on the real radio loop (`RadioLoop::step`) and the CAT keyer's wire, a
//! rigctld that logs every line: its words reach the radio's keyer one at a time at the programs'
//! own speed, the operator's speed goes back on the radio's keyer after them, and every stop cuts
//! the word on the air mid-word (`\stop_morse`) and keys no word after it.

use super::cw_stop::{lines_since, logging_rigctld, step};
use super::*;
use tempo_net::tci::server::{Cw, CwSpeed};
use tempo_net::tci::wire::Text;

/// An engine in the CW section on 40 m with the CAT keyer on an IC-7300, TX armed by the operator
/// after the move from 20 m (a band change holds programs' keys until that arm), the operator's own
/// CW at 20 WPM, and "Programs on TCI may transmit" on.
fn station() -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 3073;
        s.tci_server_tx = true;
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer("cat", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_tx_enabled_by_operator(true);
        // The band change's halt left its one-shot cuts; taken, as the loop's next tick would, so
        // a cut on the wire is the test's own.
        let _ = (e.take_cw_abort(), e.take_slot_tx_abort());
        e.set_cw_wpm(20);
    }
    engine
}

fn mac(text: &str) -> Cw {
    Cw::Macro(Text::new(text).expect("printable text"))
}

/// The words the radio's keyer was handed (`b <word>`, Hamlib's send_morse), in order.
fn keyed(log: &Mutex<Vec<String>>) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|l| l.strip_prefix("b ").map(|w| w.trim().to_string()))
        .collect()
}

/// ⛔ EVERY STOP CUTS A PROGRAM'S CW MID-WORD ON THE RADIO'S KEYER: with program 1's macro's first
/// word handed to the radio (`b TEST`, 1.7 s of Morse at 20 WPM), `cw_macros_stop` from program 2,
/// Stop TX, program 1's lost connection, its `stop;` and "Programs on TCI may transmit" switched
/// off each put `\stop_morse` on the wire on the loop's next tick, 40 ms into the word, and no word
/// of the macro reaches the radio after it. CONTROL: with no stop, nothing is cut and the whole
/// macro reaches the radio.
#[test]
fn every_stop_cuts_a_programs_cw_mid_word_on_the_radios_keyer() {
    type Stop = fn(&mut Engine);
    let stops: [(&str, Option<Stop>); 6] = [
        ("control: no stop", None),
        ("cw_macros_stop", Some(|e| e.tci_cw_stop(2))),
        ("Stop TX", Some(|e| e.halt_tx())),
        ("its lost connection", Some(|e| e.tci_disconnected(1))),
        ("its stop;", Some(|e| e.tci_end_own(1))),
        (
            "the switch off",
            Some(|e| {
                let mut s = e.settings().clone();
                s.tci_server_tx = false;
                e.apply_settings(s);
            }),
        ),
    ];
    for (name, stop) in stops {
        let engine = station();
        let (addr, log) = logging_rigctld(|_| None);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut state = loop_state_for(&engine);
        assert_eq!(
            engine.lock().unwrap().tci_cw(1, &mac("TEST DE KD9TAW")),
            Ok(()),
            "{name}: premise: program 1's CW is admitted"
        );
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        assert_eq!(
            keyed(&log),
            ["TEST"],
            "{name}: premise: the first word keys"
        );
        let from = log.lock().unwrap().len();
        if let Some(stop) = stop {
            stop(&mut engine.lock().unwrap());
        }
        step(&mut state, &engine, &mut backend, &mut rig, 140.0);
        let cut = lines_since(&log, from).iter().any(|l| l == "\\stop_morse");
        for t in [2_000.0, 4_000.0, 6_000.0, 8_000.0, 10_000.0] {
            step(&mut state, &engine, &mut backend, &mut rig, t);
        }
        if stop.is_some() {
            assert!(cut, "{name}: the word on the air was not cut: {:?}", log);
            assert_eq!(keyed(&log), ["TEST"], "{name}: a word keyed after the stop");
        } else {
            assert!(!cut, "{name}: a cut with no stop");
            assert_eq!(keyed(&log), ["TEST", "DE", "KD9TAW"], "{name}");
        }
    }
}

/// ⛔ THE PROGRAMS' SPEED IS NOT THE OPERATOR'S, ON THE RADIO'S KEYER TOO: a program's words at its
/// 30 WPM reach the radio told 30 (`L KEYSPD 30` before them), and once they have gone the radio's
/// keyer is told the operator's 20 again, as after a CW speed match answer; the operator's own speed
/// never moves. CONTROL: an F-key after it keys at 20.
#[test]
fn a_programs_cw_keys_at_its_own_speed_and_the_operators_returns_after_it() {
    let engine = station();
    let (addr, log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    {
        let mut e = engine.lock().unwrap();
        assert_eq!(e.tci_cw_speed(CwSpeed::Set(30)), Ok(30));
        assert_eq!(e.tci_cw(1, &mac("TEST")), Ok(()));
    }
    for t in [100.0, 2_000.0, 4_000.0] {
        step(&mut state, &engine, &mut backend, &mut rig, t);
    }
    let lines = lines_since(&log, 0);
    let at = |l: &str| lines.iter().position(|x| x == l);
    let (to_30, word, to_20) = (at("L KEYSPD 30"), at("b TEST"), at("L KEYSPD 20"));
    assert!(
        matches!((to_30, word, to_20), (Some(a), Some(b), Some(c)) if a < b && b < c),
        "the radio's keyer: 30 for the program's word, then the operator's 20: {lines:?}"
    );
    assert_eq!(
        engine.lock().unwrap().cw_wpm(),
        20,
        "the operator's speed moved"
    );
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().send_cw("CQ");
    step(&mut state, &engine, &mut backend, &mut rig, 6_000.0);
    let after = lines_since(&log, from);
    assert!(
        after.iter().any(|l| l == "b CQ") && !after.iter().any(|l| l == "L KEYSPD 30"),
        "control: the F-key at the operator's speed: {after:?}"
    );
}
