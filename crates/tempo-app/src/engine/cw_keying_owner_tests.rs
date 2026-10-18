//! ⛔ CW OWNS THE TRANSMITTER UNTIL ITS WORD ENDS. The radio loop hands a keyer one word at a
//! time, and the CW queue empties as the last word goes: before this, the arbiter
//! ([`Engine::tx_owner`]) let go of the transmitter for the whole of the last word of every send
//! (and all of a one-word send), so a broker client's PTT, the radio's ATU tune-up, a voice
//! message or an RTTY over could key on top of it. The loop's word window
//! ([`Engine::observe_cw_keying`]) now owns it, every gate that asks the arbiter refuses with the
//! CW reason, and the window can never hold the transmitter past its own hard end.

use super::*;

const CW_BUSY: &str = "CW is sending — stop it first";

/// The CW screen with TX armed, a CAT broker client allowed to key, and a radio whose own tuner
/// can be started over CAT.
fn cw_station() -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    e.set_operating_mode("cw", false); // arms TX
    e.set_frequency(14.030, "20m", "CW");
    e.settings.cat_broker_ptt = true;
    e.observe_rig_tuner(Some(false), true);
    e
}

/// The last word of a send handed to the keyer: the queue is empty, the rig is sending it.
fn last_word_on_the_air(e: &mut Engine) {
    e.send_cw("TEST");
    assert_eq!(e.poll_cw_one().as_deref(), Some("TEST"), "premise: keyed");
    assert!(e.cw_queue.is_empty(), "premise: nothing is left queued");
    e.observe_cw_keying(Some(1_680.0)); // "TEST" at 20 WPM with its word space
}

/// What every gate that asks the arbiter answers, by value: broker PTT, the ATU tune-up, the
/// voice keyer on a Flex that has the mic, and the owner itself.
fn gates(
    e: &mut Engine,
) -> (
    Option<TxOwner>,
    bool,
    Result<(), String>,
    Result<(), String>,
) {
    let ptt = e.broker_ptt(true);
    if ptt {
        e.broker_ptt(false);
    }
    e.observe_flex_radio_has_mic(true);
    let voice = e.send_voice(vec![0.5; 100]);
    e.voice_tx = None;
    e.observe_flex_radio_has_mic(false);
    (e.tx_owner(), ptt, e.atu_tune_gate(), voice)
}

fn open() -> (
    Option<TxOwner>,
    bool,
    Result<(), String>,
    Result<(), String>,
) {
    (None, true, Ok(()), Ok(()))
}

fn refused() -> (
    Option<TxOwner>,
    bool,
    Result<(), String>,
    Result<(), String>,
) {
    (
        Some(TxOwner::Cw),
        false,
        Err(CW_BUSY.to_string()),
        Err(CW_BUSY.to_string()),
    )
}

#[test]
fn the_last_cw_word_owns_the_transmitter_for_every_gate_that_asks() {
    let mut e = cw_station();
    assert_eq!(gates(&mut e), open(), "control: idle, every gate is open");

    last_word_on_the_air(&mut e);
    assert_eq!(
        gates(&mut e),
        refused(),
        "the rig is sending the last word: nothing else may key on top of it"
    );
    assert_eq!(
        e.snapshot().radio.tx_busy_reason.as_deref(),
        Some(CW_BUSY),
        "and the strip says why"
    );
    // CW's own type-ahead is not a rival: the next message queues behind the word.
    assert_eq!(e.send_cw_armed("CQ"), Ok(()));
    assert_eq!(e.cw_queue.len(), 1, "queued behind the word on the air");

    // The same as while words are still queued, which the arbiter always owned.
    let mut queued = cw_station();
    queued.send_cw("CQ TEST");
    assert_eq!(queued.poll_cw_one().as_deref(), Some("CQ"));
    assert_eq!(
        gates(&mut queued),
        refused(),
        "control: queued words own it"
    );
}

#[test]
fn an_rtty_over_waits_for_the_cw_word_the_rig_is_still_sending() {
    let mut e = cw_station();
    last_word_on_the_air(&mut e);
    // Leaving CW drops what is queued, never the word already in the keyer.
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "USB");
    assert_eq!(e.rtty_send_text("CQ"), Err(CW_BUSY.to_string()));
    e.observe_cw_keying(None);
    assert_eq!(e.rtty_send_text("CQ"), Ok(()), "after the word, RTTY keys");
}

#[test]
fn after_the_word_every_gate_is_open_again() {
    let mut e = cw_station();
    last_word_on_the_air(&mut e);
    e.observe_cw_keying(None);
    assert_eq!(gates(&mut e), open());
    assert_eq!(e.snapshot().radio.tx_busy_reason, None);
}

#[test]
fn stop_mid_word_ends_the_window_at_once_and_the_next_tx_keys() {
    // Stop TX: the universal halt.
    let mut e = cw_station();
    last_word_on_the_air(&mut e);
    e.halt_tx();
    assert!(e.take_cw_abort(), "control: the word itself is cut");
    assert_eq!(
        e.tx_owner(),
        None,
        "Stop wins at once, before the loop's end"
    );
    e.set_tx_enabled_by_operator(true);
    assert_eq!(
        gates(&mut e),
        open(),
        "the operator's next transmission keys"
    );

    // The CW screen's own stop, and TX off.
    let stops: [(&str, fn(&mut Engine)); 2] = [
        ("the CW screen's Stop", Engine::stop_cw),
        ("TX off", |e| e.set_tx_enabled(false)),
    ];
    for (stop, press) in stops {
        let mut e = cw_station();
        last_word_on_the_air(&mut e);
        press(&mut e);
        assert!(e.take_cw_abort(), "{stop}: control: the word itself is cut");
        assert_eq!(e.tx_owner(), None, "{stop} ends it at once");
    }

    // A halt that does NOT stop CW (a slot over's failed unkey) leaves the word owning it.
    let mut e = cw_station();
    last_word_on_the_air(&mut e);
    e.halt_tx_for_failed_unkey("RPRT -9");
    assert!(!e.take_cw_abort(), "control: no CW stop");
    assert_eq!(e.tx_owner(), Some(TxOwner::Cw));
}

#[test]
fn a_lost_end_of_word_expires_on_time() {
    let mut e = cw_station();
    let before = Instant::now();
    last_word_on_the_air(&mut e);
    let after = Instant::now();
    let hard_end = e.cw_keying_until.expect("the window is held");
    let word = Duration::from_millis(1_680 + CW_KEYING_MARGIN_MS);
    assert!(
        hard_end >= before + word && hard_end <= after + word,
        "the hard end is the word's length plus the margin, nothing more"
    );
    assert_eq!(e.tx_owner(), Some(TxOwner::Cw), "control: owned before it");

    // No end ever arrives: the hard end passes.
    e.cw_keying_until = Some(Instant::now() - Duration::from_millis(1));
    assert_eq!(
        gates(&mut e),
        open(),
        "a lost end can never hold the transmitter"
    );

    // Nothing the loop could send holds it either.
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        e.observe_cw_keying(Some(bad));
        assert_eq!(e.tx_owner(), None, "{bad}");
    }
}
