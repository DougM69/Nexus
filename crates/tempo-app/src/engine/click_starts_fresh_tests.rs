//! Clicking a station always starts the contact fresh, as WSJT-X's double-click does, and the QSO
//! start time (TIME_ON) is set and reset where WSJT-X sets and resets `m_dateTimeQSOOn`.
//!
//! Field report (KR4FQG, 2026-10-26): a partial S&P QSO with 7P8 stalled; hours later a click on
//! 7P8 sent the hours-old roger with the hours-old report, 7P8 answered RR73, and the contact
//! logged under the start time of the first attempt. The cause was the re-click rule in
//! `call_station_ctx`: a click naming no message on the station still held by the stalled QSO
//! kept that QSO, its queued roger and its start time.
//!
//! WSJT-X (master `0f7d5928`): `handleDecodeSelection` resets the start time
//! (`set_dateTimeQSO(-1)`, mainwindow.cpp:9271) and `processMessage` picks the Tx message from the
//! clicked message alone (DecodedMessageReaction.cpp `planProcessMessage`): a CQ or a message to
//! someone else selects Tx1 (:696), a message to me maps by its type, and the report is re-read
//! from that decode (`SetReport`, :400). The start time is reset when Tx1 or a CQ is selected
//! (:9129, :9213) and after logging (:10683), and set, only if not already set, when Tx2 or Tx3 is
//! selected (:9168, :9178) to `now - (n-2)*T/R - (seconds mod T/R)` (`set_dateTimeQSO`,
//! :9087-9107). Pressing Enable Tx, or a Tx2/Tx3 button on a QSO whose start time is already set,
//! changes neither the message nor the start time.

use super::tests::dec_snr;
use super::*;
use crate::test_util::StoredLog;

const ME: &str = "KR4FQG";
const DX: &str = "7P8RU";
const HOURS: u64 = 3 * 3600;
const OLD_ROGER: &str = "7P8RU KR4FQG R-12";
const TX1: &str = "7P8RU KR4FQG EM85";

fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// Transmit once on whichever of the two slots is ours; the text that went out.
fn key_once(e: &mut Engine, s: u64) -> String {
    let text = tx_now(e);
    for slot in [s, s + 1] {
        if !e.poll_tx(slot).is_empty() {
            return text;
        }
    }
    panic!("nothing keyed at slot {s} or {} (queued: {text:?})", s + 1)
}

/// The first, unfinished attempt: double-click 7P8RU's CQ, they answer once with a report, we
/// roger it (R-12) three times, they are never heard again, the operator turns TX off. TIME_ON is
/// then back-dated by three hours. With `roll`, hours of other traffic roll the 240-row decode
/// history so nothing of 7P8RU is left in it; without, their old report to us is still there.
/// Either way 7P8RU then calls CQ again on this band, heard at -08.
fn stale_attempt(roll: bool) -> (Engine, u64) {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;

    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 11);
    e.work_station(DX, None, Some("CQ 7P8RU KG30"), Some(-12), Some(1500.0))
        .expect("first attempt starts");
    assert_eq!(tx_now(&e), TX1, "premise: a fresh call opens with Tx1");
    key_once(&mut e, 12);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -12)], 14);
    assert_eq!(tx_now(&e), OLD_ROGER, "premise: their report, our roger");
    let mut s = 15;
    for _ in 0..3 {
        key_once(&mut e, s);
        e.ingest_decodes_for_test(&[dec_snr("CQ N0ABC EM48", -3)], s + 2);
        s += 4;
    }
    e.set_tx_enabled(false); // the operator gives up
    let started = e
        .qso_start_unix
        .expect("premise: the first attempt has a start time");
    let backdated = started - HOURS;
    e.qso_start_unix = Some(backdated);

    if roll {
        for k in 0..150u64 {
            e.ingest_decodes_for_test(
                &[dec_snr("CQ N0ABC EM48", -3), dec_snr("CQ W1AW FN31", -9)],
                100 + 2 * k,
            );
        }
        assert!(
            e.latest_decode_slot_from(DX).is_none(),
            "premise: ring rolled"
        );
    } else {
        assert!(
            e.latest_reply_from(DX, ME).is_some(),
            "premise: their old report to us is still in the history"
        );
    }
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -8)], 1001);
    (e, backdated)
}

/// Run the contact the click started to its log entry: our Tx1, their report (`their`) heard at
/// `heard`, our roger, their RR73, our 73. Returns every over that went out.
fn complete(e: &mut Engine, from: u64, their: i32, heard: i32) -> Vec<String> {
    let mut sent = vec![key_once(e, from)];
    e.ingest_decodes_for_test(
        &[dec_snr(&format!("KR4FQG 7P8RU {their:+03}"), heard)],
        from + 3,
    );
    sent.push(key_once(e, from + 4));
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", heard)], from + 7);
    sent.push(key_once(e, from + 8));
    e.ingest_decodes_for_test(&[], from + 11);
    sent
}

/// What a click started must be a fresh contact: Tx1 now, no start time until our roger is
/// selected, then a start time of NOW, and a log entry with the reports of THIS exchange.
fn assert_fresh_contact(mut e: Engine, backdated: u64, band: &str, route: &str) {
    assert_eq!(
        tx_now(&e),
        TX1,
        "{route}: the click must open with Tx1, never the old roger"
    );
    assert_eq!(
        e.qso_start_unix, None,
        "{route}: the click resets the start time and Tx1 keeps it reset (mainwindow.cpp:9271, :9129)"
    );
    let sent = complete(&mut e, 1200, -10, -6);
    assert!(
        !sent.iter().any(|t| t == OLD_ROGER),
        "{route}: the hours-old roger went out: {sent:?}"
    );
    assert_eq!(
        sent[1], "7P8RU KR4FQG R-06",
        "{route}: the report heard NOW"
    );
    let log = e.stored_log();
    assert_eq!(log.len(), 1, "{route}: the contact logged");
    let rec = &log[0];
    assert!(
        rec.when_unix > backdated + HOURS - 120,
        "{route}: TIME_ON is this exchange's, not the stalled attempt's: {} vs {backdated}",
        rec.when_unix
    );
    assert_eq!(rec.rst_sent.as_deref(), Some("-06"), "{route}: RST_SENT");
    assert_eq!(rec.rst_rcvd.as_deref(), Some("-10"), "{route}: RST_RCVD");
    assert_eq!(rec.band, band, "{route}: band");
}

/// A — the reported path: a click naming no message (Call Roster double-click, station card Work)
/// hours later.
#[test]
fn a_roster_click_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "roster click");
}

/// A, the Needed board and alerts: the same click carrying the station's grid.
#[test]
fn a_needed_board_click_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.call_station_with_grid(DX, Some("KG30"));
    assert_fresh_contact(e, backdated, "20m", "Needed click");
}

/// A, the Remote page and the cockpit's Work with an offset: `work_station` with no message.
#[test]
fn a_work_click_without_a_message_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.work_station(DX, Some("KG30"), None, None, Some(1500.0))
        .expect("the click is taken");
    assert_fresh_contact(e, backdated, "20m", "work click");
}

/// A, with the old report to us STILL in the decode history: the station's latest transmission is
/// its new CQ, and a WSJT-X double-click on that CQ selects Tx1. Resuming from the older report
/// would send a roger for an exchange the station has since walked away from.
#[test]
fn a_click_answers_the_stations_latest_transmission_not_an_older_reply() {
    let (mut e, backdated) = stale_attempt(false);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "roster click, history not rolled");
}

/// The positive control for the rule above: when the station's latest transmission IS its report
/// to us, a click resumes from it as a double-click on it would (Tx3), with the report re-read
/// from that decode, and the start time restarts with the selection of Tx3.
#[test]
fn a_click_mid_qso_resumes_from_the_stations_latest_message_to_us() {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 11);
    e.call_station(DX);
    assert_eq!(tx_now(&e), TX1);
    key_once(&mut e, 12);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 14);
    assert_eq!(tx_now(&e), "7P8RU KR4FQG R-09");
    e.qso_start_unix = Some(1); // mark the start time so a reset and a new set are both visible
    let before = now_unix_secs();
    e.call_station(DX);
    let after = now_unix_secs();
    assert_eq!(tx_now(&e), "7P8RU KR4FQG R-09", "Tx3, from their report");
    let on = e.qso_start_unix.expect("Tx3 selected sets the start time");
    assert!(
        (wsjtx_qso_on(before, 3, 15.0)..=wsjtx_qso_on(after, 3, 15.0)).contains(&on),
        "reset by the click, set by Tx3's selection: {on}"
    );
}

/// A logger's UDP Reply always names the message (WSJT-X `replyToCQ` is `processMessage`).
#[test]
fn a_udp_reply_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.set_tx_enabled(true); // a UDP Reply is refused behind the operator's TX Off
    e.answer_udp_reply(DX, "CQ 7P8RU KG30", -8, Some(1500.0))
        .expect("the reply is taken");
    assert_fresh_contact(e, backdated, "20m", "UDP Reply");
}

/// B — control: a double-click on the CQ line.
#[test]
fn b_a_double_click_on_the_cq_line_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.work_station(DX, None, Some("CQ 7P8RU KG30"), Some(-8), Some(1500.0))
        .expect("the double-click is taken");
    assert_fresh_contact(e, backdated, "20m", "double-click");
}

/// C — a Tx2 button click: WSJT-X selects Tx2 and `set_dateTimeQSO(2)` returns at once because
/// the start time is already set (:9099). The message changes as clicked; the start time stays.
#[test]
fn c_a_tx2_button_keeps_the_start_time_wsjtx_already_has() {
    let (mut e, backdated) = stale_attempt(true);
    e.override_next_tx(DX, None, "7P8RU KR4FQG -08");
    assert_eq!(tx_now(&e), "7P8RU KR4FQG -08");
    assert_eq!(e.qso_start_unix, Some(backdated));
}

/// C — a Tx1 button click resets the start time (`on_txrb1_toggled`, :9129).
#[test]
fn c_a_tx1_button_resets_the_start_time() {
    let (mut e, _) = stale_attempt(true);
    e.override_next_tx(DX, None, TX1);
    assert_eq!(tx_now(&e), TX1);
    assert_eq!(e.qso_start_unix, None);
}

/// D — Enable Tx alone resumes what is queued, as WSJT-X's Enable Tx does: nothing changes.
#[test]
fn d_enable_tx_alone_resumes_unchanged() {
    let (mut e, backdated) = stale_attempt(true);
    e.set_tx_enabled(true);
    assert_eq!(key_once(&mut e, 1002), OLD_ROGER);
    assert_eq!(e.qso_start_unix, Some(backdated));
}

/// E — after a band change, once the station is heard on the new band: a roster click opens a new
/// contact there with Tx1 and a new start time, and the old band's roger never goes out.
#[test]
fn e_a_roster_click_on_the_new_band_starts_fresh_there() {
    let (mut e, backdated) = stale_attempt(false);
    e.set_frequency(21.074, "15m", "USB");
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -6)], 1101);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "15m", "roster click on 15m");
}

/// F — control: a QSO already finished.
#[test]
fn f_a_finished_qso_reclicked_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    if let Mode::Qso { station, .. } = &mut e.mode {
        station.state = tempo_core::qso::State::Done;
        station.pending = None;
    }
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "re-click after Done");
}

/// Answering a CQ-run caller's grid selects Tx2: the start time is the current period's start.
#[test]
fn a_cq_runs_first_answer_sets_the_start_time_at_tx2() {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-run").expect("CQ");
    assert_eq!(e.qso_start_unix, None, "a CQ keeps it reset");
    let before = now_unix_secs();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    let after = now_unix_secs();
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "premise: Tx2");
    let on = e.qso_start_unix.expect("Tx2 selected sets it");
    assert!((wsjtx_qso_on(before, 2, 15.0)..=wsjtx_qso_on(after, 2, 15.0)).contains(&on));
}

/// `set_dateTimeQSO(n)`, mainwindow.cpp:9087-9107, by value.
#[test]
fn the_start_time_is_wsjtx_set_datetime_qso() {
    let minute = 28_000_000 * 60;
    // FT8, 37 s into the minute: the period began 7 s ago.
    assert_eq!(wsjtx_qso_on(minute + 37, 2, 15.0), minute + 30);
    assert_eq!(wsjtx_qso_on(minute + 37, 3, 15.0), minute + 15);
    // FT4: int(7.5) = 7 per period back, fmod(37, 7.5) = 7.
    assert_eq!(wsjtx_qso_on(minute + 37, 2, 7.5), minute + 30);
    assert_eq!(wsjtx_qso_on(minute + 37, 3, 7.5), minute + 23);
}
