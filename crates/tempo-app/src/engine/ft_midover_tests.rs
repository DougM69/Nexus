//! WSJT-X's mid-over message switch on the engine (`Engine::ft_midover_switch`).
//!
//! WSJT-X 3.0.2 (`widgets/mainwindow.cpp`): a decode that moves the QSO while an over is on the
//! air sets `m_restart` (:9446); the next GUI tick regenerates the wave for the new message
//! (:7880) and writes a second `Tx` row to ALL.TXT (:8087-8091); the Modulator reads the new wave
//! on from the sample it had reached (`Modulator/Modulator.cpp:329`). A Tx-slot click does the same
//! (:8751-8816). Which message follows which is `processMessage`'s: an R+report while we send the
//! report goes to RR73 (`word_3.startsWith ('R')` → `m_ntx=4`, :9250-9256), RR73 while we send the
//! R+report goes to 73 (`m_ntx=5`, :9172-9175, :9210-9214).
//!
//! Each scene keys its overs as the radio loop does (plan, build, commit) and hands the switch a
//! stand-in for the audio route that records what it was given.

use super::ft_midover::FtSwitch;
use super::*;

const TX2: &str = "W1AW KD9TAW -10";
const TX3: &str = "W1AW KD9TAW R-12";
const RR73: &str = "W1AW KD9TAW RR73";
const BYE73: &str = "W1AW KD9TAW 73";

/// A station on 20 m FT8 at 1500 Hz with ALL.TXT on, TX on the even slots.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.settings.write_all_txt = true;
    e
}

/// Hear `message` (W1AW's over) folded at the boundary of `slot`.
fn hear(e: &mut Engine, message: &str, snr: i32, slot: u64) {
    e.hear_for_test(message, snr, slot);
}

/// Work W1AW from the line it sent us, heard at `slot`, as a double-click does.
fn work(e: &mut Engine, line: &str, snr: i32, slot: u64) {
    hear(e, line, snr, slot);
    e.call_station_ctx("W1AW", Some("FN31"), Some(line), Some(snr), None)
        .expect("premise: W1AW worked");
    let _ = e.take_immediate_tx();
}

/// Key `slot`'s over as the radio loop does at a boundary, and say what went out.
fn key(e: &mut Engine, slot: u64) -> Option<String> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Native { text, .. } = &plan.waveform else {
        panic!("an FT8 plan is a native waveform");
    };
    let text = text.clone();
    let wave = plan.waveform.build();
    (!e.commit_tx(&plan, wave, slot).is_empty()).then_some(text)
}

/// `ms` into `slot`, on the FT8 period.
fn at(slot: u64, ms: f64) -> f64 {
    slot as f64 * 15_000.0 + ms
}

/// The over `text` would key as, at 1500 Hz.
fn wave_of(text: &str) -> Vec<f32> {
    TxWaveform::Native {
        kind: modes::ModeKind::Ft8,
        text: text.into(),
        f0: 1500.0,
    }
    .build()
}

/// A stand-in for the audio route: `left` samples not played yet; it takes the switch (`takes`)
/// and keeps what it was handed.
struct Route {
    left: usize,
    takes: bool,
    handed: Option<Vec<f32>>,
}

impl Route {
    fn new(left: usize) -> Route {
        Route {
            left,
            takes: true,
            handed: None,
        }
    }

    fn switch(&mut self, e: &mut Engine, slot: u64, now_ms: f64) -> Option<FtSwitch> {
        let (left, takes) = (self.left, self.takes);
        let mut handed = None;
        let got = e.ft_midover_switch(slot, now_ms, |tail| {
            handed = Some(tail(left, 0));
            takes
        });
        self.handed = handed;
        got
    }
}

fn tx_rows(rows: &[String]) -> Vec<&String> {
    rows.iter().filter(|r| r.contains(" Tx ")).collect()
}

fn tx_now(e: &Engine) -> Option<String> {
    e.snapshot().qso.and_then(|q| q.tx_now)
}

fn switched(from: &str, to: &str) -> Option<FtSwitch> {
    Some(FtSwitch::Switched {
        from: from.into(),
        to: to.into(),
    })
}

/// ⭐ THE REPORTED CASE, AS WSJT-X SEQUENCES IT. We answered W1AW's grid with the report (Tx2) and
/// keyed it again because W1AW's R-12 was not decoded before the boundary; it folds 3 s into the
/// over. WSJT-X goes to RR73 (`m_ntx=4`) and regenerates the over: the rest of this over is RR73,
/// tone for tone where an RR73 keyed at the boundary would have them, and a second Tx row says so.
#[test]
fn an_r_report_folded_while_the_report_is_on_the_air_switches_the_over_to_rr73() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    assert_eq!(
        key(&mut e, 100).as_deref(),
        Some(TX2),
        "premise: the report"
    );
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2), "premise: and again");
    let keyed_rows = tx_rows(&e.take_all_txt_pending()).len();
    assert_eq!(keyed_rows, 2, "premise: one Tx row per over keyed");

    hear(&mut e, "KD9TAW W1AW R-12", -12, 102);
    assert_eq!(
        tx_now(&e).as_deref(),
        Some(RR73),
        "premise: the sequencer moved on"
    );
    let mut route = Route::new(100_000);
    assert_eq!(
        route.switch(&mut e, 102, at(102, 3_000.0)),
        switched(TX2, RR73)
    );
    let rr73 = wave_of(RR73);
    assert_eq!(
        route.handed.as_deref(),
        Some(&rr73[rr73.len() - 100_000..]),
        "the rest of the over is RR73's, in place"
    );
    let rows = e.take_all_txt_pending();
    let tx = tx_rows(&rows);
    assert_eq!(tx.len(), 1, "a second Tx row: {rows:?}");
    assert_eq!(
        *tx[0],
        crate::alltxt::all_txt_line(
            1_533,
            e.settings.dial_mhz,
            true,
            "FT8",
            0,
            0.0,
            1500.0,
            RR73
        ),
        "stamped when the switch was made, 3 s into slot 102"
    );
    let Mode::Qso { station, .. } = &e.mode else {
        panic!("premise: a QSO");
    };
    assert_eq!(station.tx_count, 1, "the RR73 counted as sent once");
    assert_eq!(
        tx_now(&e).as_deref(),
        Some(RR73),
        "the next over repeats it"
    );
    assert_eq!(
        key(&mut e, 104).as_deref(),
        Some(RR73),
        "as WSJT-X repeats Tx4"
    );
}

/// RR73 folded while our R+report is on the air: WSJT-X goes to 73 (`m_ntx=5`). After the 73,
/// "Disable Tx after sending 73" turns TX off once the over has played: armed once, by this switch,
/// and not again by the next boundary, which has nothing left to send.
#[test]
fn rr73_folded_while_the_roger_is_on_the_air_switches_to_73_and_disables_tx_once() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW -10", -12, 100);
    assert_eq!(key(&mut e, 100).as_deref(), Some(TX3), "premise: the roger");
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX3), "premise: and again");
    assert!(!e.take_pending_tx_disable(), "premise: nothing armed yet");

    hear(&mut e, "KD9TAW W1AW RR73", -12, 102);
    assert!(
        !e.pending_tx_disable,
        "premise: the fold alone arms nothing"
    );
    let mut route = Route::new(60_000);
    assert_eq!(
        route.switch(&mut e, 102, at(102, 6_000.0)),
        switched(TX3, BYE73)
    );
    let bye = wave_of(BYE73);
    assert_eq!(route.handed.as_deref(), Some(&bye[bye.len() - 60_000..]));
    assert_eq!(tx_now(&e), None, "the 73 goes once");
    assert!(e.take_pending_tx_disable(), "Disable Tx after 73: armed");
    assert!(!e.take_pending_tx_disable(), "…once");
    assert_eq!(key(&mut e, 104), None, "nothing left to send");
    assert!(
        !e.take_pending_tx_disable(),
        "and the next boundary arms nothing"
    );
}

/// The partner repeats its grid while we wait for the roger: WSJT-X 3.0 re-reads the report from
/// the newer decode (`:9426`) and the over on the air is regenerated with it. The over was counted
/// once, at its key; the switch does not count it again.
#[test]
fn a_report_re_read_from_a_repeat_switches_the_over_and_counts_it_once() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    key(&mut e, 100);
    key(&mut e, 102);
    hear(&mut e, "KD9TAW W1AW FN31", -15, 102);
    let mut route = Route::new(50_000);
    assert_eq!(
        route.switch(&mut e, 102, at(102, 2_000.0)),
        switched(TX2, "W1AW KD9TAW -15")
    );
    let Mode::Qso { station, .. } = &e.mode else {
        panic!("premise: a QSO");
    };
    assert_eq!(station.tx_count, 2, "two overs keyed, two counted");
}

/// Nothing to change, nothing switched: a decode that leaves the next message as it is hands the
/// route nothing and writes no row.
#[test]
fn a_fold_that_changes_nothing_switches_nothing() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    key(&mut e, 100);
    key(&mut e, 102);
    e.take_all_txt_pending();
    hear(&mut e, "CQ K1ABC FN42", -5, 102);
    let mut route = Route::new(50_000);
    assert_eq!(route.switch(&mut e, 102, at(102, 2_000.0)), None);
    assert!(route.handed.is_none(), "nothing handed");
    assert!(tx_rows(&e.take_all_txt_pending()).is_empty(), "no row");
}

/// ⛔ SLOT LIVENESS: only the over keyed in the live slot. The radio loop names the live slot; an
/// over the engine handed out for another slot is never rebuilt (the stale-boundary incident).
#[test]
fn only_the_over_keyed_in_the_live_slot_is_switched() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    key(&mut e, 100);
    key(&mut e, 102);
    hear(&mut e, "KD9TAW W1AW R-12", -12, 102);
    let mut route = Route::new(50_000);
    for slot in [100, 103, 104] {
        assert_eq!(
            route.switch(&mut e, slot, at(slot, 2_000.0)),
            None,
            "slot {slot}"
        );
    }
    assert!(route.handed.is_none());
    assert_eq!(
        route.switch(&mut e, 102, at(102, 2_000.0)),
        switched(TX2, RR73),
        "control: the live slot's"
    );
}

/// ⛔ EVERY REASON NOT TO TRANSMIT STILL HOLDS: with TX off (or Stop TX), the slot no longer this
/// station's, the TX offset or the dial moved, or a tune up, the over on the air is never rebuilt.
#[test]
fn an_over_that_would_not_be_planned_now_is_never_rebuilt() {
    type Change = fn(&mut Engine);
    let cases: [(&str, Change); 6] = [
        ("TX off", |e| e.set_tx_enabled(false)),
        ("Stop TX", |e| e.halt_tx()),
        ("the other slot", |e| e.set_tx_even(false)),
        ("the TX offset moved", |e| e.set_tx_offset(1700.0)),
        ("the dial moved", |e| e.settings.dial_mhz = 14.0745),
        ("a licence that refuses it", |e| {
            e.set_license_class("technician")
        }),
    ];
    for (what, change) in cases {
        let mut e = station();
        work(&mut e, "KD9TAW W1AW FN31", -10, 100);
        key(&mut e, 100);
        key(&mut e, 102);
        hear(&mut e, "KD9TAW W1AW R-12", -12, 102);
        e.take_all_txt_pending();
        change(&mut e);
        let mut route = Route::new(50_000);
        assert_eq!(route.switch(&mut e, 102, at(102, 2_000.0)), None, "{what}");
        assert!(route.handed.is_none(), "{what}: nothing handed");
        assert!(
            tx_rows(&e.take_all_txt_pending()).is_empty(),
            "{what}: no row"
        );
    }
}

/// A route that cannot replace what it holds (nothing left to play): the over goes out as first chosen, and
/// nothing about the QSO moves: no row, nothing counted; the next over sends the new message, as
/// it always has.
#[test]
fn a_route_that_cannot_replace_its_audio_leaves_the_over_and_the_qso_as_they_were() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    key(&mut e, 100);
    key(&mut e, 102);
    hear(&mut e, "KD9TAW W1AW R-12", -12, 102);
    e.take_all_txt_pending();
    let mut route = Route::new(50_000);
    route.takes = false;
    assert_eq!(
        route.switch(&mut e, 102, at(102, 2_000.0)),
        Some(FtSwitch::NotReplaced {
            from: TX2.into(),
            to: RR73.into()
        })
    );
    assert!(tx_rows(&e.take_all_txt_pending()).is_empty(), "no row");
    let Mode::Qso { station, .. } = &e.mode else {
        panic!("premise: a QSO");
    };
    assert_eq!(station.tx_count, 0, "the RR73 not counted as sent");
    assert_eq!(
        key(&mut e, 104).as_deref(),
        Some(RR73),
        "the next over sends it"
    );
}

/// The Tx-slot click (WSJT-X's `on_txb4_clicked`, :8784): during the over the click asks for the
/// switch, and the over goes out as the clicked message from here on. A click with no over on the
/// air is answered by the next key and asks nothing of it.
#[test]
fn a_tx_slot_click_during_an_over_switches_it_to_the_clicked_message() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    e.override_next_tx("W1AW", Some("FN31"), TX3);
    assert!(e.take_ft_restart(), "the click asks for the switch");
    e.override_next_tx("W1AW", Some("FN31"), TX2);
    assert_eq!(key(&mut e, 100).as_deref(), Some(TX2));
    assert!(!e.take_ft_restart(), "the key answered the click before it");

    e.override_next_tx("W1AW", Some("FN31"), RR73);
    assert!(
        e.take_ft_restart(),
        "a click during the over asks for the switch"
    );
    let mut route = Route::new(40_000);
    assert_eq!(
        route.switch(&mut e, 100, at(100, 4_000.0)),
        switched(TX2, RR73)
    );
    let Mode::Qso { station, .. } = &e.mode else {
        panic!("premise: a QSO");
    };
    assert_eq!(station.tx_count, 1, "the clicked message counted once");
}

/// The one over owed to a partner a CQ run has left (they repeated their R-report: they did not
/// copy our RR73). WSJT-X answers it the moment it is decoded (its partner and `m_ntx` survive the
/// contact); here, during the CQ over that resumed the run, the CQ switches to the RR73, which is
/// spent, and the next over is the CQ again.
#[test]
fn a_late_repeat_from_the_partner_the_run_left_switches_the_cq_to_the_owed_roger() {
    let mut e = station();
    e.start_cq(None).expect("premise: Call CQ");
    let _ = e.take_immediate_tx();
    let cq = key(&mut e, 100).expect("premise: the CQ");
    hear(&mut e, "KD9TAW W1AW FN31", -10, 102);
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2));
    hear(&mut e, "KD9TAW W1AW R-12", -12, 104);
    assert_eq!(key(&mut e, 104).as_deref(), Some(RR73));
    hear(&mut e, "CQ K1ABC FN42", -5, 106);
    assert_eq!(
        key(&mut e, 106).as_deref(),
        Some(cq.as_str()),
        "premise: logged, the run resumed"
    );
    hear(&mut e, "KD9TAW W1AW R-12", -12, 106);
    let mut route = Route::new(40_000);
    assert_eq!(
        route.switch(&mut e, 106, at(106, 1_000.0)),
        switched(&cq, RR73)
    );
    assert!(e.recent_partner.is_none(), "the owed over is spent");
    assert_eq!(key(&mut e, 108).as_deref(), Some(cq.as_str()), "back to CQ");
}
