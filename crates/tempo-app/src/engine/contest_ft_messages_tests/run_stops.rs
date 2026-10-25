//! The contest's Running stops after each contact, by value. Once a contact is logged and its
//! last over has gone out, TX goes off and the run's CQ waits: nothing keys until the operator
//! calls CQ again, with the strip's Call CQ, the Contest screen's Running or Enable TX. WSJT-X
//! 3.0.2 does the same with auto-sequencing on: logging the contact turns Enable Tx off
//! (`cease_auto_Tx_after_QSO`, `widgets/mainwindow.cpp:10518-10528`, from `on_logQSOButton_clicked`
//! :10530-10538), and so does the key of its 73 or RR73 (:8125-8139), with Tx6, the CQ, set up for
//! the next press. A caller who answers the run's CQ while it is being called is worked as before,
//! and outside a contest Call CQ still returns to its CQ.
//!
//! Driven against the scripted WSJT-X partner of the module above.

use super::*;

/// How the operator calls CQ again.
#[derive(Clone, Copy, Debug)]
enum Press {
    /// The Operate strip's Call CQ.
    CallCq,
    /// The Contest screen's Running.
    Running,
    /// Enable TX.
    EnableTx,
}

/// Every set of the contest's messages: the contest, the tier, the partner's WSJT-X activity, and
/// what the run sends for one contact, CQ first.
fn sets() -> Vec<(&'static str, Tier, Act, [&'static str; 3])> {
    let fd = ["CQ FD K1ABC FN42", "W9XYZ K1ABC R 2B EMA", "W9XYZ K1ABC 73"];
    let vhf = ["CQ TEST K1ABC FN42", "W9XYZ K1ABC R FN42", "W9XYZ K1ABC 73"];
    let std = ["CQ K1ABC FN42", "W9XYZ K1ABC -12", "W9XYZ K1ABC RR73"];
    let mut v = Vec::new();
    for tier in [Tier::Ft8, Tier::Ft4] {
        v.push(("arrlfd", tier, Act::FieldDay, fd));
        v.push(("arrlvhf_jun", tier, Act::NaVhf, vhf));
        v.push(("txqp", tier, Act::None, std));
    }
    // FT1 keeps Field Day's messages, whose runner closes on the caller's RR73 with nothing sent.
    v.push((
        "arrlfd",
        Tier::TempoFast,
        Act::FieldDay,
        ["CQ FD K1ABC FN42", "W9XYZ K1ABC R 2B EMA", ""],
    ));
    v
}

/// The contest's Running in `contest` on `tier`, one contact with a caller in `act`, played for
/// `SLOTS` slots with the radio loop's deferred TX off. Every over this station sent, by slot.
fn run_one_contact(contest: &str, tier: Tier, act: Act) -> (Engine, Vec<(u64, String)>) {
    let mut e = engine_for(contest, MY_GRID);
    e.set_tier(tier);
    e.set_mode("fieldday-run").expect("the run starts");
    let mut partner = Partner::new(act, false);
    let mut sent = Vec::new();
    for slot in 0..SLOTS {
        // The radio loop's deferred TX off, once the last over has played.
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                partner.hear(&text);
                sent.push((slot, text));
            }
        } else {
            match partner.transmit() {
                Some(text) => e.ingest_decodes_for_test(&[heard(&text)], slot + 1),
                None => e.ingest_decodes_for_test(&[], slot + 1),
            }
        }
    }
    (e, sent)
}

/// ⭐ **After a contact the run's CQ waits for the operator**, in every set of the contest's
/// messages: the contact goes to the log, TX goes off, and no CQ goes out in the slots after it.
/// Each of the operator's presses calls the CQ in the run's own period, and a station answering it
/// is worked as before.
#[test]
fn a_contest_run_stops_after_each_contact_until_the_operator_calls_cq() {
    for (contest, tier, act, overs) in sets() {
        for press in [Press::CallCq, Press::Running, Press::EnableTx] {
            // On FT1 the strip's Call CQ is a plain run outside the contest, as before.
            if tier == Tier::TempoFast && matches!(press, Press::CallCq) {
                continue;
            }
            let what = format!("{contest:?} {} {press:?}", tier_name(tier));
            let (mut e, sent) = run_one_contact(contest, tier, act);
            let want: Vec<&str> = overs.iter().copied().filter(|o| !o.is_empty()).collect();
            let texts: Vec<&str> = sent.iter().map(|(_, t)| t.as_str()).collect();
            assert_eq!(texts, want, "{what}: one contact, then no CQ");
            assert_eq!(
                rows_of(&e).len(),
                1,
                "{what}: the contact is logged, {:?}",
                rows_of(&e)
            );
            assert!(!e.tx_enabled(), "{what}: TX is off after the contact");
            assert_eq!(
                mode_line(&e),
                "FieldDay running=true state=CallingCq",
                "{what}: still the contest's Running, its CQ set up"
            );
            assert_eq!(
                keyed(&mut e, SLOTS),
                None,
                "{what}: nothing keys until a press"
            );

            // The press lands in the partner's period: the CQ goes out in the run's own.
            let at = SLOTS + 1;
            let r = match press {
                Press::CallCq => e.start_running(),
                Press::Running => e.set_mode("fieldday-run"),
                Press::EnableTx => {
                    e.set_tx_enabled(true);
                    Ok(())
                }
            };
            assert_eq!(r, Ok(()), "{what}");
            assert!(e.tx_enabled(), "{what}: the press turns TX on");
            assert_eq!(
                keyed(&mut e, at),
                None,
                "{what}: not in the partner's period"
            );
            assert_eq!(
                keyed(&mut e, at + 1).as_deref(),
                Some(overs[0]),
                "{what}: the CQ, in the run's period"
            );

            // A station answering that CQ is worked as before.
            let mut caller = Partner::new(act, false);
            caller.hear(overs[0]);
            let call = caller.transmit().expect("the caller answers the CQ");
            e.ingest_decodes_for_test(&[heard(&call)], at + 3);
            assert_eq!(
                keyed(&mut e, at + 3).as_deref(),
                Some(overs[1]),
                "{what}: the caller {call:?} is answered"
            );
        }
    }
}

/// ⛔ The CQ never keys while TX is about to go off. Where the radio loop folds the decode that
/// closes the contact and then makes its TX decision before it has turned TX off, the run's CQ is
/// not keyed: Field Day's runner on FT1 closes on the caller's RR73 with nothing to send.
#[test]
fn the_runs_cq_is_not_keyed_in_the_slot_its_contact_closes() {
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::TempoFast);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ FD K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("W9XYZ K1ABC R 2B EMA"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 4);
    assert_eq!(rows_of(&e).len(), 1, "logged");
    assert_eq!(keyed(&mut e, 4), None, "no CQ before TX goes off");
    assert!(e.take_pending_tx_disable(), "TX off is on its way");
    e.set_tx_enabled(false);
    assert_eq!(keyed(&mut e, 6), None, "nor after");
}

/// The control: outside a contest the strip's Call CQ returns to its CQ after a contact, TX on.
#[test]
fn outside_a_contest_call_cq_still_returns_to_its_cq() {
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.start_running().expect("Call CQ");
    let mut partner = Partner::new(Act::None, false);
    let mut sent = Vec::new();
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                partner.hear(&text);
                sent.push(text);
            }
        } else {
            match partner.transmit() {
                Some(text) => e.ingest_decodes_for_test(&[heard(&text)], slot + 1),
                None => e.ingest_decodes_for_test(&[], slot + 1),
            }
        }
    }
    assert_eq!(
        sent[..4],
        [
            "CQ K1ABC FN42",
            "W9XYZ K1ABC -12",
            "W9XYZ K1ABC RR73",
            "CQ K1ABC FN42"
        ],
        "{sent:?}"
    );
    assert!(e.tx_enabled(), "TX stays on");
}
