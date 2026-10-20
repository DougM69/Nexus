//! AN UNASSISTED CONTEST ENTRY DECODES NO CW. CQ WW's rules count a CW decoder as QSO-finding
//! assistance, so while the operator has declared an unassisted entry neither decoder gives
//! anything: no copy, no speed, no skim, and no speed for CW speed match. Copy heard before the
//! declaration does not come back when it ends, however it was declared. An assisted entry, and
//! operating outside a contest, decode exactly as before.

use super::*;
use tempo_core::cw_decode::CwDecode;

/// The CW screen on 40 m, with the AI decoder on as it ships.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_operating_mode("cw", false);
    e.set_frequency(7.03, "40m", "CW");
    e
}

/// `text` keyed at 25 WPM on the operator's pitch, then a second of silence.
fn hear(e: &mut Engine, text: &str) {
    let sr = tempo_fast::SAMPLE_RATE;
    let mut audio = vec![0.0f32; (sr * 0.2) as usize];
    audio.extend(tempo_core::cw::morse_samples(
        text,
        25,
        e.settings().cw_pitch_hz,
        sr as u32,
    ));
    audio.extend(vec![0.0f32; sr as usize]);
    for chunk in audio.chunks(1200) {
        e.feed_rx_audio(chunk);
    }
}

/// Declare or end the entry the way a settings save does (a backup restore, a save from another
/// computer), rather than by its own switch.
fn save_unassisted(e: &mut Engine, on: bool) {
    let mut s = e.settings().clone();
    s.unassisted_mode = on;
    e.apply_settings(s);
}

#[test]
fn an_assisted_station_still_copies_and_times_cw() {
    let mut e = station();
    e.set_ai_cw_enabled(false);
    hear(&mut e, "CQ TEST W1AW W1AW");
    let d = e.cw_decode();
    assert!(d.text.contains("W1AW"), "the classic copy: {:?}", d.text);
    assert!((22..=28).contains(&d.wpm), "its speed: {}", d.wpm);
    // With the AI decoder on, its copy is the copy.
    e.set_ai_cw_enabled(true);
    e.feed_rx_audio(&[0.0; 1200]);
    let (_, _, g) = e.ai_cw_window().expect("the AI ring fills");
    e.push_ai_cw_pass(g, "CQ TEST K1ABC ", "K1");
    assert_eq!(e.cw_decode().text, "CQ TEST K1ABC ");
    assert_eq!(e.cw_decode_pending(), "K1");
    assert_eq!(e.snapshot().ai_cw.text, "CQ TEST K1ABC ");
}

#[test]
fn an_unassisted_entry_shows_no_copy_and_no_speed() {
    let mut e = station();
    e.set_unassisted_mode(true);
    hear(&mut e, "CQ TEST W1AW W1AW");
    assert_eq!(
        e.cw_decode(),
        CwDecode::default(),
        "neither decoder's copy, nor the classic decoder's speed"
    );
}

#[test]
fn an_unassisted_entry_answers_at_my_own_speed() {
    let mut e = station();
    let mut s = e.settings().clone();
    s.cw_speed_match = true;
    s.cw_speed_match_min = 15;
    s.cw_speed_match_max = 35;
    e.apply_settings(s);
    e.select_peer("W1AW");
    hear(&mut e, "TEST W1AW");
    assert!(
        e.cw_reply_wpm().is_some(),
        "control: assisted, an answer goes at the speed measured for W1AW"
    );
    e.set_unassisted_mode(true);
    hear(&mut e, "TEST W1AW");
    assert_eq!(
        e.cw_reply_wpm(),
        None,
        "unassisted, no decoder measures the station: my own speed"
    );
}

#[test]
fn an_unassisted_entry_skims_nothing() {
    let mut e = station();
    hear(&mut e, "W1AW W1AW");
    assert!(
        !e.cw_skim().is_empty(),
        "control: assisted, the skim finds the station"
    );
    save_unassisted(&mut e, true);
    assert_eq!(
        e.cw_skim(),
        Vec::new(),
        "nothing from the moment it is declared"
    );
    hear(&mut e, "W1AW W1AW");
    assert_eq!(e.cw_skim(), Vec::new(), "nor from the audio after it");
}

#[test]
fn copy_heard_before_the_declaration_does_not_come_back_when_it_ends() {
    let mut e = station();
    e.set_ai_cw_enabled(false);
    hear(&mut e, "CQ TEST W1AW W1AW");
    assert!(
        e.cw_decode().text.contains("W1AW"),
        "control: copied before the declaration"
    );
    e.set_unassisted_mode(true);
    e.set_unassisted_mode(false);
    assert_eq!(
        e.cw_decode(),
        CwDecode::default(),
        "the entry over, neither the copy nor the speed from before it is shown again"
    );
}

#[test]
fn an_unassisted_entry_set_by_a_settings_save_decodes_nothing_either() {
    let mut e = station();
    e.set_ai_cw_enabled(false);
    hear(&mut e, "CQ TEST W1AW W1AW");
    save_unassisted(&mut e, true);
    assert_eq!(
        e.cw_decode(),
        CwDecode::default(),
        "nothing from the moment it is saved"
    );
    hear(&mut e, "CQ TEST K1ABC");
    assert_eq!(
        e.cw_decode(),
        CwDecode::default(),
        "nor from the audio after it"
    );
    save_unassisted(&mut e, false);
    let d = e.cw_decode();
    assert!(
        !d.text.contains("W1AW") && !d.text.contains("K1ABC"),
        "the entry over, nothing from before or during it: {:?}",
        d.text
    );
    hear(&mut e, "CQ TEST K2XYZ");
    assert!(
        e.cw_decode().text.contains("K2XYZ"),
        "control: decoding resumes"
    );
}

#[test]
fn an_unassisted_entry_set_by_a_settings_save_carries_no_ai_copy() {
    let mut e = station();
    e.feed_rx_audio(&[0.0; 1200]);
    let (_, _, g) = e.ai_cw_window().expect("the AI ring fills");
    e.push_ai_cw_pass(g, "CQ TEST W1AW ", "W1");
    assert_eq!(e.snapshot().ai_cw.text, "CQ TEST W1AW ", "control");
    save_unassisted(&mut e, true);
    assert_eq!(
        e.snapshot().ai_cw.text,
        "",
        "the snapshot carries no AI copy"
    );
    assert_eq!(e.cw_decode_pending(), "", "nor its unconfirmed tail");
    e.feed_rx_audio(&[0.0; 1200]);
    save_unassisted(&mut e, false);
    assert_eq!(
        e.cw_decode().text,
        "",
        "the entry over, the AI copy from before it is not shown again"
    );
}
