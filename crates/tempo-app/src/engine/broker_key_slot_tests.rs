//! ⛔ AN ARMED FT RUN NEVER KEYS UNDER A CAT-BROKER CLIENT'S KEY. With "Other programs may key
//! transmit" on, a program on the CAT broker (WSJT-X, VarAC) that keys holds the transmitter
//! ([`Engine::broker_ptt`]). The slot planner ([`Engine::plan_tx`]) never asked, and planned its
//! over on top of the client's. It now plans none while that key is held, as for a program on the
//! TCI server, a tune or a CW word: the skipped slot leaves the run armed and its QSO exactly where
//! it was, and the next free slot after the release keys the same over. The client's own key, its
//! release and Stop TX are the broker's as before.

use super::*;

/// "Other programs may key transmit" on.
fn share_the_key(e: &mut Engine) {
    let mut settings = e.settings().clone();
    settings.cat_broker_ptt = true;
    e.apply_settings(settings);
}

/// An FT8 CQ run (Call CQ) on even slots.
fn cq_run() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    e.set_frequency(14.074, "20m", "USB");
    share_the_key(&mut e);
    e.start_cq(None).unwrap();
    assert!(e.tx_even(), "premise: the run calls on even slots");
    e
}

/// An FT8 S&P QSO the partner answered with a report at slot 1: our R-report is owed on even slots.
fn ft_qso() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    e.set_frequency(14.074, "20m", "USB");
    share_the_key(&mut e);
    e.call_station("W9XYZ");
    e.ingest_decodes_for_test(&[super::tests::dec_snr("K2DEF W9XYZ -10", -7)], 1);
    assert!(e.tx_enabled(), "premise: working a station arms TX");
    e
}

/// What the run is about to do: (state, the over owed, overs sent, stalled).
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
fn a_slot_under_a_broker_clients_key_is_skipped_and_the_next_free_one_keys_the_same_over() {
    type Scene = fn() -> Engine;
    let scenes: [(&str, Scene, &str); 2] = [
        ("a CQ run", cq_run, "CQ K2DEF FN31"),
        ("an S&P QSO", ft_qso, "W9XYZ K2DEF R-07"),
    ];
    for (name, scene, owed) in scenes {
        let mut e = scene();
        let before = qso(&e);
        assert_eq!(
            before.1.as_deref(),
            Some(owed),
            "premise: {name}: the over owed"
        );
        assert!(
            e.broker_ptt(true),
            "premise: {name}: the client keys between the overs"
        );
        assert_eq!(
            key_slot(&mut e, 2),
            None,
            "{name}: an over keyed under the client's key"
        );
        assert!(
            !e.app.radio.transmitting,
            "{name}: the skipped slot reads as on the air"
        );
        assert!(e.tx_enabled(), "{name}: the skipped slot disarmed the run");
        assert!(
            e.broker_ptt_active(),
            "{name}: the skipped slot dropped the client's key"
        );
        assert_eq!(qso(&e), before, "{name}: the run moved on a skipped slot");
        assert!(e.broker_ptt(false), "premise: {name}: the client lets go");
        assert_eq!(
            key_slot(&mut e, 4).as_deref(),
            Some(owed),
            "{name}: the next free slot did not key the same over"
        );
        assert_eq!(
            qso(&e).2,
            before.2 + 1,
            "{name}: the over was counted other than once"
        );
    }
}

/// Stop TX still wins: it drops the client's key, as the broker's kill switch always has, and no
/// later slot keys, the client's release included.
#[test]
fn stop_tx_under_a_broker_clients_key_drops_it_and_no_later_slot_keys() {
    let mut e = cq_run();
    assert!(e.broker_ptt(true), "premise: the client keys");
    e.halt_tx();
    assert!(!e.broker_ptt_active(), "Stop TX left the client's key up");
    assert!(!e.tx_enabled(), "Stop TX left the run armed");
    assert_eq!(key_slot(&mut e, 2), None, "a slot keyed after Stop TX");
    assert!(e.broker_ptt(false), "the client's release is always taken");
    assert_eq!(key_slot(&mut e, 4), None, "a slot keyed after the release");
    assert!(!e.broker_ptt(true), "the client keyed again after Stop TX");
}
