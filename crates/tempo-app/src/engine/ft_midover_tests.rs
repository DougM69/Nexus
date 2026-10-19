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
use crate::test_util::StoredLog;

const TX2: &str = "W1AW KD9TAW -10";
const TX3: &str = "W1AW KD9TAW R-12";
const RR73: &str = "W1AW KD9TAW RR73";
const BYE73: &str = "W1AW KD9TAW 73";
/// W1AW's RR73 to us.
const RR73_TO_US: &str = "KD9TAW W1AW RR73";

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
    /// A Tx-slot click asks, not a decode.
    clicked: bool,
    handed: Option<Vec<f32>>,
}

impl Route {
    fn new(left: usize) -> Route {
        Route {
            left,
            takes: true,
            clicked: false,
            handed: None,
        }
    }

    fn switch(&mut self, e: &mut Engine, slot: u64, now_ms: f64) -> Option<FtSwitch> {
        let (left, takes) = (self.left, self.takes);
        let mut handed = None;
        let got = e.ft_midover_switch(slot, now_ms, self.clicked, |tail| {
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

/// A route that cannot replace what it holds (Flex DAX): the over goes out as first chosen, and
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
    route.clicked = true;
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

/// ⭐ THE USER'S CASE, WITH WSJT-X's RR73 RULE. We answered W1AW's grid with the report (Tx2)
/// and keyed it again; W1AW had our report, rogered it with RR73 straight away, and only the full
/// pass found that RR73, 3 s into our second Tx2. WSJT-X answers an RR73 received while it sends
/// the report with 73 (`m_ntx=5`, `widgets/mainwindow.cpp:9171-9214`) and regenerates the over
/// (:9446): the rest of this over is the 73, and Tx2 never goes out again.
#[test]
fn rr73_folded_while_the_report_is_on_the_air_switches_the_rest_to_73() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    assert_eq!(
        key(&mut e, 100).as_deref(),
        Some(TX2),
        "premise: the report"
    );
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2), "premise: and again");
    e.take_all_txt_pending();

    hear(&mut e, RR73_TO_US, -21, 102);
    let mut route = Route::new(100_000);
    assert_eq!(
        route.switch(&mut e, 102, at(102, 3_000.0)),
        switched(TX2, BYE73)
    );
    let bye = wave_of(BYE73);
    assert_eq!(
        route.handed.as_deref(),
        Some(&bye[bye.len() - 100_000..]),
        "the rest of the over is the 73's, in place"
    );
    let rows = e.take_all_txt_pending();
    let tx = tx_rows(&rows);
    assert_eq!(tx.len(), 1, "a second Tx row: {rows:?}");
    assert!(tx[0].ends_with(BYE73), "{}", tx[0]);
    assert_eq!(tx_now(&e), None, "the 73 goes once");
    assert!(e.take_pending_tx_disable(), "Disable Tx after 73: armed");
    assert_eq!(key(&mut e, 104), None, "Tx2 never goes out again");
    assert_eq!(e.stored_log().len(), 1, "the contact is logged");
}

/// The same in a CQ run: the caller answered our CQ with its grid, rogered our report with RR73,
/// and the RR73 folds during the report's second over. The rest is the 73; TX stays on (a run
/// returns to CQ) and the next over is the CQ, not the report.
#[test]
fn in_a_cq_run_rr73_folded_during_the_report_closes_with_73_and_the_run_calls_cq() {
    let mut e = station();
    e.start_cq(None).expect("premise: Call CQ");
    let _ = e.take_immediate_tx();
    let cq = key(&mut e, 100).expect("premise: the CQ");
    hear(&mut e, "KD9TAW W1AW FN31", -10, 102);
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2));
    assert_eq!(key(&mut e, 104).as_deref(), Some(TX2), "premise: again");
    hear(&mut e, RR73_TO_US, -21, 104);
    let mut route = Route::new(100_000);
    assert_eq!(
        route.switch(&mut e, 104, at(104, 2_500.0)),
        switched(TX2, BYE73)
    );
    assert!(!e.take_pending_tx_disable(), "a run keeps TX on");
    hear(&mut e, "CQ K1ABC FN42", -5, 106);
    assert_eq!(
        key(&mut e, 106).as_deref(),
        Some(cq.as_str()),
        "the run calls CQ"
    );
}

/// ⛔ #236 (`reference-ft8-wrong-73-fabricated-sender`) still holds now that a QSO awaiting the
/// roger answers RR73 with 73: a bystander Fox's multiplex whose sender-less first half is
/// addressed to us never closes the contact or keys a 73, mid-over or at the next key.
#[test]
fn a_bystander_fox_rr73_never_closes_a_qso_awaiting_the_roger() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW FN31", -10, 100);
    assert_eq!(key(&mut e, 100).as_deref(), Some(TX2));
    hear(&mut e, "KD9TAW RR73; NEXTHOUND <PJ4DX> -08", -10, 100);
    let mut route = Route::new(50_000);
    assert_eq!(route.switch(&mut e, 100, at(100, 2_000.0)), None);
    assert!(e.stored_log().is_empty(), "nothing logged");
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2), "the report again");
    // Control: the partner's own RR73 does close it.
    hear(&mut e, RR73_TO_US, -10, 102);
    assert_eq!(
        route.switch(&mut e, 102, at(102, 2_000.0)),
        switched(TX2, BYE73)
    );
}

const RRR: &str = "W1AW KD9TAW RRR";
const CQ: &str = "CQ KD9TAW EN52";
const BYE73_TO_US: &str = "KD9TAW W1AW 73";

/// A station that rogers with RRR (Settings: RRR instead of RR73), working W1AW from its R-report:
/// our RRR is keyed in slot 100.
fn rogering_with_rrr() -> Engine {
    let mut e = station();
    e.settings.prefer_rrr = true;
    work(&mut e, "KD9TAW W1AW R-12", -10, 100);
    assert_eq!(key(&mut e, 100).as_deref(), Some(RRR), "premise: our RRR");
    e.take_all_txt_pending();
    e
}

/// ⭐ WSJT-X's Tx6 AFTER THE PARTNER's 73. We sent RRR (WSJT-X's ROGERS; RRR is not a 73, so
/// sequencing goes on); W1AW's 73 folds during the over. WSJT-X sets `m_ntx=6`
/// (`widgets/mainwindow.cpp:9216-9231`), regenerates the over (:9446) and turns Enable Tx off
/// (`cease_auto_Tx_after_QSO`, :10518): the rest of this over is the CQ, and nothing goes after it.
#[test]
fn the_partners_73_during_our_rrr_switches_the_rest_to_the_cq_and_tx_goes_off() {
    let mut e = rogering_with_rrr();
    hear(&mut e, BYE73_TO_US, -10, 100);
    let mut route = Route::new(80_000);
    assert_eq!(
        route.switch(&mut e, 100, at(100, 2_000.0)),
        switched(RRR, CQ)
    );
    let cq = wave_of(CQ);
    assert_eq!(route.handed.as_deref(), Some(&cq[cq.len() - 80_000..]));
    let rows = e.take_all_txt_pending();
    let tx = tx_rows(&rows);
    assert_eq!(tx.len(), 1, "{rows:?}");
    assert!(tx[0].ends_with(CQ), "{}", tx[0]);
    assert!(e.take_pending_tx_disable(), "TX off after this over");
    assert!(!e.take_pending_tx_disable(), "…once");
    assert_eq!(key(&mut e, 102), None, "and no CQ after it");
    assert_eq!(e.stored_log().len(), 1, "the contact is logged once");
}

/// With "Disable Tx after sending 73" off, the rest of the over is still the CQ, as WSJT-X's
/// regenerated Tx6 is; the CQ is never called again on its own: the QSO is over and nothing is
/// queued, so no over follows.
#[test]
fn the_cq_after_the_partners_73_is_never_called_again_on_its_own() {
    let mut e = rogering_with_rrr();
    e.settings.disable_tx_after_73 = false;
    hear(&mut e, BYE73_TO_US, -10, 100);
    let mut route = Route::new(80_000);
    assert_eq!(
        route.switch(&mut e, 100, at(100, 2_000.0)),
        switched(RRR, CQ)
    );
    assert!(!e.take_pending_tx_disable(), "the setting is off");
    for slot in [102, 104, 106] {
        hear(&mut e, "CQ K1ABC FN42", -5, slot);
        assert_eq!(key(&mut e, slot), None, "slot {slot}");
    }
}

/// ⛔ NOT AFTER A 73 WENT OUT. Our RR73 is a 73 (`message_is_73`, :315-321): at its key WSJT-X's
/// auto-sequencing turns Enable Tx off (:8105-8139) and no decode reaches `processMessage`
/// (:7542-7560). The partner's 73 (or anything else) leaves the RR73 on the air.
#[test]
fn the_partners_73_during_our_rr73_leaves_the_rr73_on_the_air() {
    let mut e = station();
    work(&mut e, "KD9TAW W1AW R-12", -10, 100);
    assert_eq!(key(&mut e, 100).as_deref(), Some(RR73), "premise: our RR73");
    hear(&mut e, BYE73_TO_US, -10, 100);
    let mut route = Route::new(80_000);
    assert_eq!(route.switch(&mut e, 100, at(100, 2_000.0)), None);
    assert!(route.handed.is_none());
    assert_eq!(key(&mut e, 102), None, "nothing after it");
}

/// ⛔ A CQ RUN's OWN BOOKKEEPING NEVER CUTS THE CLOSING OVER. The run logs the contact and returns
/// to its CQ on the first fold after our RR73 (or RRR) is keyed, whatever that fold holds. That is
/// not WSJT-X's `processMessage` reacting to a message: the over on the air goes out to its end,
/// and the CQ follows at the next over. With the partner's 73 the RR73 also stays (a 73 went out);
/// the RRR becomes the CQ, as WSJT-X's Tx6.
#[test]
fn a_cq_run_returning_to_cq_leaves_our_closing_over_on_the_air() {
    for (rrr, heard, switched_to) in [
        (false, "CQ K1ABC FN42", None),
        (false, BYE73_TO_US, None),
        (true, "CQ K1ABC FN42", None),
        (true, BYE73_TO_US, Some(CQ)),
    ] {
        let what = format!("RRR {rrr}, heard {heard:?}");
        let mut e = station();
        e.settings.prefer_rrr = rrr;
        e.start_cq(None).expect("premise: Call CQ");
        let _ = e.take_immediate_tx();
        assert_eq!(key(&mut e, 100).as_deref(), Some(CQ), "{what}");
        hear(&mut e, "KD9TAW W1AW FN31", -10, 102);
        assert_eq!(key(&mut e, 102).as_deref(), Some(TX2), "{what}");
        hear(&mut e, "KD9TAW W1AW R-12", -12, 104);
        let closing = if rrr { RRR } else { RR73 };
        assert_eq!(key(&mut e, 104).as_deref(), Some(closing), "{what}");
        hear(&mut e, heard, -10, 104);
        assert_eq!(e.ft_next_over(104).as_deref(), Some(CQ), "premise: {what}");
        let mut route = Route::new(80_000);
        assert_eq!(
            route.switch(&mut e, 104, at(104, 1_000.0)),
            switched_to.and_then(|to| switched(closing, to)),
            "{what}"
        );
        assert_eq!(e.stored_log().len(), 1, "{what}: logged once");
        assert_eq!(key(&mut e, 106).as_deref(), Some(CQ), "{what}: then the CQ");
    }
}

/// A run that gives up on a caller who went silent (`cq_stall_overs`) returns to CQ at a fold,
/// which is again the run's bookkeeping: the report on the air goes out to its end.
#[test]
fn giving_up_on_a_silent_caller_leaves_the_over_on_the_air() {
    let mut e = station();
    e.start_cq(None).expect("premise: Call CQ");
    let _ = e.take_immediate_tx();
    key(&mut e, 100).expect("premise: the CQ");
    hear(&mut e, "KD9TAW W1AW FN31", -10, 102);
    let mut slot = 102;
    while e.ft_next_over(slot).as_deref() == Some(TX2) {
        assert_eq!(key(&mut e, slot).as_deref(), Some(TX2), "premise");
        hear(&mut e, "CQ K1ABC FN42", -5, slot);
        if e.ft_next_over(slot).as_deref() != Some(TX2) {
            break;
        }
        slot += 2;
        assert!(slot < 130, "premise: the run gives up");
    }
    assert_eq!(
        e.ft_next_over(slot).as_deref(),
        Some(CQ),
        "premise: given up"
    );
    let mut route = Route::new(80_000);
    assert_eq!(route.switch(&mut e, slot, at(slot, 1_000.0)), None);
}

/// K1ABC (2B EMA) in `event`, on 20 m FT8 at 1500 Hz, TX on the even slots: the contest screen's
/// settings, as the contest message tests fill them.
fn contest(event: &str) -> Engine {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    let mut s = e.settings().clone();
    s.fd_event = event.into();
    s.fd_class = if event == "wfd" { "1H" } else { "2B" }.into();
    s.fd_section = "EMA".into();
    s.contest_qth_state = "MA".into();
    s.tx_even = true;
    s.fd_active = true;
    s.write_all_txt = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e
}

/// The contest sequencer switches the same way inside a contact (WSJT-X's `processMessage` runs
/// in its Field Day activity too): running ARRL Field Day, our rogered exchange is on the air when
/// the caller's RR73 folds, and the rest is the 73 WSJT-X's runner answers it with (:9210-9214).
#[test]
fn a_field_day_run_switches_its_exchange_to_73_when_the_callers_rr73_folds() {
    let mut e = contest("arrlfd");
    e.set_mode("fieldday-run").expect("the run starts");
    let cq = key(&mut e, 0).expect("premise: the CQ");
    hear(&mut e, "K1ABC W9XYZ 1D WI", -12, 2);
    let exch = key(&mut e, 2).expect("premise: the rogered exchange");
    assert_eq!(exch, "W9XYZ K1ABC R 2B EMA");
    assert_eq!(key(&mut e, 4).as_deref(), Some(exch.as_str()), "again");
    e.take_all_txt_pending();
    hear(&mut e, "K1ABC W9XYZ RR73", -12, 4);
    let mut route = Route::new(80_000);
    assert_eq!(
        route.switch(&mut e, 4, at(4, 2_000.0)),
        switched(&exch, "W9XYZ K1ABC 73")
    );
    let bye = wave_of("W9XYZ K1ABC 73");
    assert_eq!(route.handed.as_deref(), Some(&bye[bye.len() - 80_000..]));
    assert_eq!(
        tx_rows(&e.take_all_txt_pending()).len(),
        1,
        "a second Tx row"
    );
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("premise: the contest");
    };
    assert_eq!(station.log.qsos().len(), 1, "logged");
    assert!(!e.take_pending_tx_disable(), "a run keeps TX on");
    hear(&mut e, "CQ K2DEF FN31", -5, 6);
    assert_eq!(
        key(&mut e, 6).as_deref(),
        Some(cq.as_str()),
        "the run calls CQ"
    );
}

/// S&P in the contest: our exchange is on the air when the runner's rogered exchange folds; the
/// rest is our RR73 (WSJT-X's Tx4). Then S&P is listening again and picks the next CQ by itself,
/// but that is a new contact: it never replaces the RR73 on the air.
#[test]
fn contest_sp_switches_to_rr73_and_never_starts_the_next_contact_mid_over() {
    let mut e = contest("arrlfd");
    e.set_mode("fieldday-sp").expect("S&P starts");
    e.set_tx_enabled(true);
    hear(&mut e, "CQ FD W9XYZ EN37", -12, 2);
    let exch = key(&mut e, 2).expect("premise: our exchange");
    assert_eq!(exch, "W9XYZ K1ABC 2B EMA");
    assert_eq!(key(&mut e, 4).as_deref(), Some(exch.as_str()), "again");
    hear(&mut e, "K1ABC W9XYZ R 1D WI", -12, 4);
    let mut route = Route::new(80_000);
    assert_eq!(
        route.switch(&mut e, 4, at(4, 1_500.0)),
        switched(&exch, "W9XYZ K1ABC RR73")
    );
    // The next CQ heard in the same over: S&P returns to listening and answers it.
    hear(&mut e, "CQ FD K2DEF FN31", -9, 4);
    hear(&mut e, "CQ FD K2DEF FN31", -9, 4);
    assert_eq!(
        e.ft_next_over(4).as_deref(),
        Some("K2DEF K1ABC 2B EMA"),
        "premise: S&P picked the next CQ"
    );
    assert_eq!(route.switch(&mut e, 4, at(4, 3_000.0)), None);
}

/// ⛔ THE OPERATOR STARTS EVERY CONTEST QSO: a CQ on the air is never switched to an answer, and
/// a contact the run closes without a 73 (Winter Field Day) never becomes a CQ mid-over.
#[test]
fn a_contest_never_switches_a_cq_or_switches_to_one() {
    let mut e = contest("arrlfd");
    e.set_mode("fieldday-run").expect("the run starts");
    key(&mut e, 0).expect("premise: the CQ");
    hear(&mut e, "K1ABC W9XYZ 1D WI", -12, 0);
    assert_eq!(
        e.ft_next_over(0).as_deref(),
        Some("W9XYZ K1ABC R 2B EMA"),
        "premise: the caller is answered next"
    );
    let mut route = Route::new(80_000);
    assert_eq!(
        route.switch(&mut e, 0, at(0, 1_000.0)),
        None,
        "the CQ stays"
    );

    let mut e = contest("wfd");
    e.set_mode("fieldday-run").expect("the run starts");
    let cq_wfd = key(&mut e, 0).expect("premise: the CQ");
    hear(&mut e, "K1ABC W9XYZ 1H WI", -12, 2);
    let exch = key(&mut e, 2).expect("premise: the rogered exchange");
    hear(&mut e, "K1ABC W9XYZ RR73", -12, 2);
    assert_eq!(
        e.ft_next_over(2),
        Some(cq_wfd.clone()),
        "premise: back to CQ"
    );
    assert_eq!(
        route.switch(&mut e, 2, at(2, 1_000.0)),
        None,
        "{exch} stays"
    );
    assert_eq!(key(&mut e, 4), Some(cq_wfd), "the CQ at the next over");
}

/// A contest contact the operator picked (a double-click on the CQ) and closed by the switch: the
/// RR73 that leaves mid-over arms "Disable Tx after sending 73" as at a key; S&P's own pounce
/// does not.
#[test]
fn a_picked_contest_contact_closed_mid_over_turns_tx_off_once() {
    for picked in [true, false] {
        let mut e = contest("arrlfd");
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        hear(&mut e, "CQ FD W9XYZ EN37", -12, 2);
        if picked {
            e.work_station(
                "W9XYZ",
                Some("EN37"),
                Some("CQ FD W9XYZ EN37"),
                Some(-12),
                None,
            )
            .expect("premise: the double-click");
        }
        let exch = key(&mut e, 2).expect("premise: our exchange");
        hear(&mut e, "K1ABC W9XYZ R 1D WI", -12, 2);
        let mut route = Route::new(80_000);
        assert_eq!(
            route.switch(&mut e, 2, at(2, 1_500.0)),
            switched(&exch, "W9XYZ K1ABC RR73"),
            "picked {picked}"
        );
        assert_eq!(e.take_pending_tx_disable(), picked, "picked {picked}");
        assert!(!e.take_pending_tx_disable(), "picked {picked}: once");
    }
}
