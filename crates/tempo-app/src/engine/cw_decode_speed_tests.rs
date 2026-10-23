//! THE DECODE HEAD'S SPEED IS OFFERED ONLY WHILE IT IS FRESH. It is the classic decoder's estimate,
//! and a click on a call chip sets the keyer to it, so a reading from a station heard long ago would
//! send the next answer at the previous station's speed. It is shown, and so used, within the same
//! window CW speed match allows: a keyed element heard in the last 10 seconds.

use super::*;

/// The CW screen on 40 m.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_operating_mode("cw", false);
    e.set_frequency(7.03, "40m", "CW");
    e
}

/// The station on the operator's pitch: `text` keyed at `wpm`, then `silence` seconds.
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

#[test]
fn a_fresh_speed_is_still_offered() {
    let mut e = station();
    hear(&mut e, "TEST W1AW", 30, 1.0);
    let wpm = e.cw_decode().wpm;
    assert!(
        (28..=32).contains(&wpm),
        "a second after the station: its speed: {wpm}"
    );
    hear(&mut e, "", 30, 8.0);
    let wpm = e.cw_decode().wpm;
    assert!((28..=32).contains(&wpm), "nine seconds after: {wpm}");
}

#[test]
fn a_stale_speed_is_not_offered() {
    let mut e = station();
    hear(&mut e, "TEST W1AW", 30, 1.0);
    assert!(
        (28..=32).contains(&e.cw_decode().wpm),
        "control: the speed is offered while fresh"
    );
    hear(&mut e, "", 30, 10.0);
    assert_eq!(
        e.cw_decode().wpm,
        0,
        "eleven seconds after the last keyed element, the speed may be another station's"
    );
    hear(&mut e, "TEST K1ABC", 20, 1.0);
    let wpm = e.cw_decode().wpm;
    assert!(
        (16..=24).contains(&wpm),
        "the next station heard offers its own speed, not the last one's: {wpm}"
    );
}

/// A QSY lands on another signal, so the speed goes with the copy: a spot clicked, or the dial
/// moved, within ten seconds of the last station does not offer that station's speed.
#[test]
fn a_qsy_forgets_the_speed() {
    let mut e = station();
    hear(&mut e, "TEST W1AW", 30, 1.0);
    assert!(
        (28..=32).contains(&e.cw_decode().wpm),
        "control: the speed is offered while fresh"
    );
    e.work_spot("CW", 7.025, "40m");
    assert_eq!(
        e.cw_decode().wpm,
        0,
        "a spot clicked a second after the last station: that station's speed is not this one's"
    );
    hear(&mut e, "TEST K1ABC", 20, 1.0);
    let wpm = e.cw_decode().wpm;
    assert!(
        (16..=24).contains(&wpm),
        "control: the station on the spot offers its own speed: {wpm}"
    );
    e.set_frequency(7.028, "40m", "CW");
    assert_eq!(e.cw_decode().wpm, 0, "a dial change forgets it too");
}
