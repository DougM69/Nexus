//! ⛔ NOTHING KEYS ON TOP OF A CW WORD. Three keying paths never ask the arbiter
//! ([`Engine::tx_owner`]): the tune carrier, manual PTT and the FT slot planner. Each refuses a
//! NEW key while CW is sending (words queued, or the word the rig is sending now), and each lets
//! go as before: stopping a tune and releasing PTT are never refused. A skipped FT slot leaves
//! the QSO exactly where it was, and the next free slot keys the same over.
//!
//! The voice keyer too: a message sent while CW is sending is refused with the CW reason, one
//! queued just before CW started is dropped instead of played, and stopping voice always works.

use super::*;

const CW_BUSY: &str = "CW is sending — stop it first";

/// The CW screen with TX armed.
fn cw_station() -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    e.set_operating_mode("cw", false); // arms TX
    e.set_frequency(14.030, "20m", "CW");
    e
}

/// How CW is sending: words still queued, or the last word handed to the rig's keyer.
#[derive(Clone, Copy, Debug)]
enum Cw {
    Queued,
    Keying,
}

fn start_cw(e: &mut Engine, how: Cw) {
    e.send_cw("TEST");
    if let Cw::Keying = how {
        assert_eq!(e.poll_cw_one().as_deref(), Some("TEST"), "premise: keyed");
        assert!(e.cw_queue.is_empty(), "premise: nothing is left queued");
        e.observe_cw_keying(Some(1_680.0)); // "TEST" at 20 WPM with its word space
    }
    assert_eq!(
        e.tx_owner(),
        Some(TxOwner::Cw),
        "premise: {how:?}: CW is sending"
    );
}

/// The CW word is over: nothing queued, and the loop said the word ended.
fn end_cw(e: &mut Engine) {
    e.cw_queue.clear();
    e.observe_cw_keying(None);
    assert_eq!(e.tx_owner(), None, "premise: CW has ended");
}

/// (tuning, manual PTT, the busy reason the operator sees)
fn keys(e: &Engine) -> (bool, bool, Option<String>) {
    (
        e.tuning(),
        e.manual_ptt(),
        e.snapshot().radio.tx_busy_reason,
    )
}

#[test]
fn tune_on_is_refused_while_cw_sends_and_keys_once_it_ends() {
    for how in [Cw::Queued, Cw::Keying] {
        let mut e = cw_station();
        start_cw(&mut e, how);
        e.set_tune(true);
        assert_eq!(
            keys(&e),
            (false, false, Some(CW_BUSY.to_string())),
            "{how:?}: Tune ON during CW is refused, with the CW reason"
        );
        end_cw(&mut e);
        e.set_tune(true);
        assert_eq!(
            keys(&e),
            (true, false, Some(TxOwner::Tune.busy_reason())),
            "{how:?}: Tune keys once CW has ended"
        );
    }
}

#[test]
fn tune_off_is_never_refused_while_cw_sends() {
    let mut e = cw_station();
    e.set_tune(true);
    assert!(e.tuning(), "premise: the carrier is up");
    // The loop reports a word on the air under it (the window, not the arbiter, is the input).
    e.observe_cw_keying(Some(1_680.0));
    assert!(e.cw_sending(), "premise: CW is sending");
    e.set_tune(false);
    assert!(!e.tuning(), "stopping a tune during CW is accepted");
}

#[test]
fn ptt_on_is_refused_while_cw_sends_and_keys_once_it_ends() {
    for how in [Cw::Queued, Cw::Keying] {
        let mut e = cw_station();
        start_cw(&mut e, how);
        e.set_ptt(true);
        assert_eq!(
            keys(&e),
            (false, false, Some(CW_BUSY.to_string())),
            "{how:?}: PTT ON during CW is refused, with the CW reason"
        );
        end_cw(&mut e);
        e.set_ptt(true);
        assert_eq!(
            keys(&e),
            (false, true, Some(TxOwner::ManualPtt.busy_reason())),
            "{how:?}: PTT keys once CW has ended"
        );
    }
}

#[test]
fn ptt_release_is_never_refused_while_cw_sends_and_stop_still_wins() {
    let mut e = cw_station();
    e.set_ptt(true);
    assert!(e.manual_ptt(), "premise: PTT is held");
    e.observe_cw_keying(Some(1_680.0));
    assert!(e.cw_sending(), "premise: CW is sending");
    e.set_ptt(false);
    assert!(!e.manual_ptt(), "releasing PTT during CW is accepted");

    let mut e = cw_station();
    e.set_ptt(true);
    e.observe_cw_keying(Some(1_680.0));
    e.halt_tx();
    assert_eq!(
        (e.manual_ptt(), e.cw_sending()),
        (false, false),
        "Stop TX drops the key and the CW at once"
    );
}

#[test]
fn with_no_cw_tune_and_ptt_key_as_before() {
    let mut e = cw_station();
    e.set_tune(true);
    assert!(e.tuning(), "Tune keys");
    e.set_tune(false);
    assert!(!e.tuning(), "and stops");
    e.set_ptt(true);
    assert!(e.manual_ptt(), "PTT keys");
    e.set_ptt(false);
    assert!(!e.manual_ptt(), "and releases");
}

/// An FT8 S&P QSO the partner has answered with a report at slot 1: our R-report is owed on
/// even slots.
fn ft_qso() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    e.set_frequency(14.074, "20m", "USB");
    e.call_station("W9XYZ");
    e.ingest_decodes_for_test(&[super::tests::dec_snr("K2DEF W9XYZ -10", -7)], 1);
    assert!(e.tx_enabled(), "premise: working a station arms TX");
    e
}

/// What the QSO is about to do: (state, the over owed, overs sent, stalled).
fn qso(e: &Engine) -> (String, Option<String>, u32, bool) {
    let q = e.snapshot().qso.expect("premise: a QSO is running");
    (q.state, q.tx_now, q.tx_count, q.stalled)
}

/// Plan, build and commit `slot` as the radio loop does: the text it keyed, if it keyed.
fn key_slot(e: &mut Engine, slot: u64) -> Option<String> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Native { text, .. } = &plan.waveform else {
        panic!("premise: an FT8 over is a native waveform");
    };
    let text = text.clone();
    let wave = plan.waveform.build();
    (!e.commit_tx(&plan, wave, slot).is_empty()).then_some(text)
}

#[test]
fn an_ft_slot_that_starts_during_cw_is_skipped_and_the_next_one_keys_the_same_over() {
    for how in [Cw::Queued, Cw::Keying] {
        let mut e = ft_qso();
        let before = qso(&e);
        assert_eq!(
            before.1.as_deref(),
            Some("W9XYZ K2DEF R-07"),
            "premise: the R-report is owed"
        );
        start_cw(&mut e, how);
        assert_eq!(
            key_slot(&mut e, 2),
            None,
            "{how:?}: the slot during CW does not key"
        );
        assert!(
            !e.app.radio.transmitting,
            "{how:?}: and nothing reads as on the air"
        );
        assert_eq!(qso(&e), before, "{how:?}: the QSO is untouched by the skip");
        end_cw(&mut e);
        assert_eq!(
            key_slot(&mut e, 4).as_deref(),
            Some("W9XYZ K2DEF R-07"),
            "{how:?}: the next slot after CW keys the same over"
        );
    }
}

#[test]
fn with_no_cw_the_ft_slot_keys_as_before() {
    let mut e = ft_qso();
    assert_eq!(
        key_slot(&mut e, 2).as_deref(),
        Some("W9XYZ K2DEF R-07"),
        "the first free slot keys"
    );
}

/// One second of a voice-keyer message.
fn message() -> Vec<f32> {
    vec![0.1; 12_000]
}

#[test]
fn voice_is_refused_while_cw_sends_and_plays_once_it_ends() {
    for flex in [false, true] {
        for how in [Cw::Queued, Cw::Keying] {
            let mut e = cw_station();
            // The Flex client with the radio holding the mic asks the arbiter beside voice.
            e.observe_flex_radio_has_mic(flex);
            start_cw(&mut e, how);
            assert_eq!(
                e.send_voice(message()),
                Err(CW_BUSY.to_string()),
                "{how:?}, Flex {flex}: voice during CW is refused, with the CW reason"
            );
            assert!(
                e.voice_tx.is_none(),
                "{how:?}, Flex {flex}: nothing was queued"
            );
            end_cw(&mut e);
            assert_eq!(
                e.send_voice(message()),
                Ok(()),
                "{how:?}, Flex {flex}: voice is taken once CW has ended"
            );
            if !flex {
                assert_eq!(e.poll_voice(), Some(message()), "{how:?}: and plays");
            }
        }
    }
}

#[test]
fn a_message_queued_before_cw_started_is_dropped_not_played() {
    for how in [Cw::Queued, Cw::Keying] {
        let mut e = cw_station();
        assert_eq!(e.send_voice(message()), Ok(()), "premise: queued");
        // An F-key fires before the radio loop has taken the message.
        e.send_cw("TEST");
        if let Cw::Keying = how {
            assert_eq!(e.poll_cw_one().as_deref(), Some("TEST"), "premise: keyed");
            e.observe_cw_keying(Some(1_680.0));
        }
        assert_eq!(
            e.poll_voice().map(|m| m.len()),
            None,
            "{how:?}: not played over the CW"
        );
        assert!(e.voice_tx.is_none(), "{how:?}: dropped, never held");
        end_cw(&mut e);
        assert_eq!(
            e.poll_voice().map(|m| m.len()),
            None,
            "{how:?}: not played once CW ends"
        );
    }
}

#[test]
fn stopping_voice_is_never_refused_while_cw_sends() {
    for how in [Cw::Queued, Cw::Keying] {
        let mut e = cw_station();
        assert_eq!(e.send_voice(message()), Ok(()), "premise: queued");
        e.send_cw("TEST");
        if let Cw::Keying = how {
            assert_eq!(e.poll_cw_one().as_deref(), Some("TEST"), "premise: keyed");
            e.observe_cw_keying(Some(1_680.0));
        }
        e.stop_voice();
        assert_eq!(
            (e.voice_tx.is_some(), e.take_voice_abort()),
            (false, true),
            "{how:?}: Stop drops the message and flushes the loop"
        );
    }
}

#[test]
fn with_no_cw_the_voice_keyer_plays_as_before() {
    let mut e = cw_station();
    assert_eq!(e.send_voice(message()), Ok(()), "taken");
    assert_eq!(e.poll_voice(), Some(message()), "and plays");
}
