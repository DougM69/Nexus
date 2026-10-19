//! CW SPEED MATCH: a message that answers a station keys at the speed the decoder measured for
//! that station, held to the operator's min/max; a CQ, and everything after the answer, keys at
//! the operator's own speed. Off by default. A stale or missing estimate, or a station the copy
//! does not name, keys at the operator's own speed. The speed is fixed per message, before its
//! first word, and never changes inside it.

use super::*;

const OWN: u32 = 25;

/// The CW screen with TX armed, own speed 25, speed match `on` (15..=35), working W1AW.
fn station(on: bool) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let mut s = e.settings().clone();
    s.cw_speed_match = on;
    s.cw_speed_match_min = 15;
    s.cw_speed_match_max = 35;
    e.apply_settings(s);
    e.set_license_class("extra");
    e.set_operating_mode("cw", false);
    e.set_frequency(7.03, "40m", "CW");
    e.set_cw_wpm(OWN);
    e.select_peer("W1AW");
    e
}

/// The station heard on the operator's pitch: `text` keyed at `wpm`, then `silence` seconds.
fn hear(e: &mut Engine, text: &str, wpm: u32, silence: f32) {
    let sr = tempo_fast::SAMPLE_RATE;
    let mut audio = vec![0.0f32; (sr * 0.2) as usize];
    audio.extend(tempo_core::cw::morse_samples(
        text,
        wpm,
        e.settings().cw_pitch_hz,
        sr as u32,
    ));
    audio.extend(vec![0.0f32; (sr * silence) as usize]);
    for chunk in audio.chunks(1200) {
        e.feed_rx_audio(chunk);
    }
}

/// Every queued word with the speed the loop keys it at, in order.
fn keyed(e: &mut Engine) -> Vec<(String, u32)> {
    std::iter::from_fn(|| e.poll_cw_word().map(|(w, wpm, _)| (w, wpm))).collect()
}

fn at(words: &[&str], wpm: u32) -> Vec<(String, u32)> {
    words.iter().map(|w| (w.to_string(), wpm)).collect()
}

#[test]
fn an_answer_keys_at_the_measured_speed_and_the_cq_after_it_at_my_own() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    let measured = e.cw_decode().wpm;
    assert!(
        (28..=32).contains(&measured),
        "premise: the decoder measured about 30: {measured}"
    );
    e.send_cw_as("{MYCALL} 5NN", true);
    assert_eq!(e.cw_next_wpm(), measured, "set before the first word keys");
    assert_eq!(keyed(&mut e), at(&["KD9TAW", "5NN"], measured));
    assert_eq!(e.cw_next_wpm(), OWN, "after the answer: my own speed");
    e.send_cw("CQ TEST");
    assert_eq!(
        keyed(&mut e),
        at(&["CQ", "TEST"], OWN),
        "a CQ keys at my own"
    );
    assert_eq!(e.cw_wpm(), OWN, "my own speed is never changed");
}

#[test]
fn speed_match_off_keys_an_answer_at_my_own_speed() {
    assert!(!Settings::default().cw_speed_match, "speed match ships OFF");
    let mut e = station(false);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN));
    assert_eq!(e.cw_reply_wpm(), None);
}

#[test]
fn a_message_not_marked_as_an_answer_keys_at_my_own_speed() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    e.send_cw("{MYCALL}");
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN));
}

#[test]
fn a_stale_estimate_keys_at_my_own_speed() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 9.0);
    assert!(
        e.cw_reply_wpm().is_some(),
        "control: 9 s after the station's last element the speed is still used"
    );
    hear(&mut e, "", 30, 2.0);
    assert_eq!(e.cw_reply_wpm(), None, "11 s on, the speed is stale");
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN));
}

#[test]
fn no_estimate_keys_at_my_own_speed() {
    let mut e = station(true);
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN));
}

#[test]
fn a_station_the_copy_does_not_name_keys_at_my_own_speed() {
    let mut e = station(true);
    hear(&mut e, "TEST K1ABC", 30, 1.0);
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN), "W1AW was never copied");
    e.select_peer("K1ABC");
    assert!(
        e.cw_reply_wpm().is_some(),
        "control: the station copied is matched"
    );
    e.clear_peer();
    assert_eq!(e.cw_reply_wpm(), None, "no station worked, no match");
}

#[test]
fn a_portable_call_in_the_copy_names_the_station() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW/P", 30, 1.0);
    assert!(e.cw_reply_wpm().is_some());
}

#[test]
fn the_speed_is_held_to_my_min_and_max() {
    let mut e = station(true);
    // Warmed on a 28 WPM station first: from a cold start the classic decoder cannot time a fast
    // station at all (see the next test).
    hear(&mut e, "CQ TEST K1ABC", 28, 1.0);
    hear(&mut e, "TEST W1AW", 40, 1.0);
    let fast = e.cw_decode().wpm;
    assert!(fast > 36, "premise: a fast station: {fast}");
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], 35), "held to the max");

    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 10, 1.0);
    let slow = e.cw_decode().wpm;
    assert!(slow < 13, "premise: a slow station: {slow}");
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], 15), "held to the min");
}

#[test]
fn an_answer_queued_behind_a_cq_changes_speed_only_between_the_messages() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    let m = e.cw_decode().wpm;
    e.send_cw("CQ TEST");
    e.send_cw_as("{MYCALL} 5NN", true);
    e.send_cw("CQ");
    let mut words = at(&["CQ", "TEST"], OWN);
    words.extend(at(&["KD9TAW", "5NN"], m));
    words.extend(at(&["CQ"], OWN));
    assert_eq!(keyed(&mut e), words);
}

#[test]
fn enter_sends_message_answers_match_too() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    let m = e.cw_decode().wpm;
    e.send_cw_armed_as("{MYCALL}", true).unwrap();
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], m));
    e.send_cw_armed("CQ TEST").unwrap();
    assert_eq!(keyed(&mut e), at(&["CQ", "TEST"], OWN));
}

/// The operator moving the speed (the control, or the radio's knob that the loop follows) wins:
/// what is still queued keys at the new speed.
#[test]
fn a_speed_the_operator_chooses_drops_the_match_for_what_is_queued() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    let m = e.cw_decode().wpm;
    e.send_cw_as("{MYCALL} 5NN IL", true);
    assert_eq!(e.poll_cw_word(), Some(("KD9TAW".into(), m, true)));
    e.set_cw_wpm(OWN);
    assert_eq!(
        e.poll_cw_word(),
        Some(("5NN".into(), m, false)),
        "the same speed again is no choice (the control's settle-save)"
    );
    e.set_cw_wpm(20);
    assert_eq!(keyed(&mut e), at(&["IL"], 20));
}

/// A stop drops the answer with the rest of the queue; the next message is told its own speed.
#[test]
fn a_stopped_answer_leaves_nothing_behind_for_the_next_message() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 30, 1.0);
    e.send_cw_as("{MYCALL} 5NN IL", true);
    assert!(e.poll_cw_word().is_some());
    e.stop_cw();
    assert_eq!(e.cw_next_wpm(), OWN);
    e.send_cw("CQ TEST");
    assert_eq!(keyed(&mut e), at(&["CQ", "TEST"], OWN));
}

/// Only the classic decoder's own copy can name the station for its speed: the AI decoder measures
/// no speed, so a call only the AI copied never lends the classic estimate to the answer. (This
/// once rode on the classic decoder mistiming a fast station from a cold start; it no longer does,
/// see the next test, so the premise is now a different station in the classic copy.)
#[test]
fn a_call_only_the_ai_copied_does_not_choose_the_speed() {
    let mut e = station(true);
    hear(&mut e, "TEST K1ABC", 30, 1.0);
    assert!(
        !e.cw_decode().text.contains("W1AW"),
        "premise: the classic copy does not name W1AW"
    );
    let epoch = e.ai_cw_epoch;
    e.push_ai_cw_pass(epoch, "TEST W1AW", "");
    assert!(
        e.cw_decode().text.contains("W1AW"),
        "premise: the AI copy names the station"
    );
    assert_eq!(e.cw_reply_wpm(), None);
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], OWN));
}

/// A fast station heard from a cold start is timed right (the classic decoder locks onto 15-50
/// WPM within a few characters), so its answer is matched, held to the operator's max.
#[test]
fn a_fast_station_from_a_cold_start_is_matched_and_held_to_my_max() {
    let mut e = station(true);
    hear(&mut e, "TEST W1AW", 40, 1.0);
    let measured = e.cw_decode().wpm;
    assert!(
        (36..=44).contains(&measured),
        "premise: the cold decoder times 40 WPM: {measured}"
    );
    assert_eq!(e.cw_reply_wpm(), Some(35));
    e.send_cw_as("{MYCALL}", true);
    assert_eq!(keyed(&mut e), at(&["KD9TAW"], 35));
}

/// Each word says whether it starts its message, where a keyer told its speed once per message
/// is told it; a word queued outside any message counts as a start.
#[test]
fn each_word_says_whether_it_starts_its_message() {
    let mut e = station(false);
    e.send_cw("CQ TEST");
    e.send_cw("K");
    let starts: Vec<bool> = std::iter::from_fn(|| e.poll_cw_word().map(|(_, _, s)| s)).collect();
    assert_eq!(starts, [true, false, true]);
    e.cw_queue.push_back("E".into());
    assert_eq!(e.poll_cw_word().map(|(_, _, s)| s), Some(true));
}
