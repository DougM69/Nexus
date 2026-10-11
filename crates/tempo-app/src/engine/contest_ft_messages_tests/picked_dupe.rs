//! A station this running's contest log already holds, picked off the air: a double-click on its
//! CQ, or a logger's Reply to it. The contact is made, and the other station logs it, so it is
//! logged here too: marked a dupe, worth nothing, and everywhere a contact goes — the contest's
//! journal and files, the Logbook at the Merge, the club. A typed dupe in the same contest is
//! still refused, and S&P's own pounce still passes the station by.
//!
//! Driven through the verbs the gestures call (`work_station`, with the arguments the cockpit's
//! double-click and the radio loop's Reply pass), against the scripted WSJT-X partner of the
//! module above, on a station built the way the app launches one.

use super::*;
use crate::logstore::tests::{open_fast, Dir};
use std::sync::Mutex;

/// How the operator picks the CQ.
#[derive(Clone, Copy, Debug)]
enum Pick {
    /// The cockpit's double-click (the `call_station` command), which passes the grid.
    DoubleClick,
    /// A logger's WSJT-X Reply, as the radio loop hands it over: no grid, the CQ's frequency.
    Reply,
}

/// A launch in `contest` on FT8, as the app makes one: the Logbook store and the contest's
/// journal in `d`, a club position, and the contest the switch left on entered with the
/// session's rows read first. Launched again on the same `d`, it is a restart.
fn launch(contest: &str, d: &Dir) -> Engine {
    let mut s = engine_for(contest, MY_GRID).settings().clone();
    s.fd_position_id = "a1b2c3d4".into();
    let mut e = Engine::with_settings(s);
    e.attach_log_store(open_fast(d));
    e.set_fd_log_path(d.0.join("fieldday.adi"));
    e.set_tier(Tier::Ft8);
    let shared = Mutex::new(e);
    with_session_rows_at_launch(
        &shared,
        |e| e.restore_opens_session(),
        |e, rows| {
            e.restore_field_day_if_enabled();
            if let Some(rows) = rows {
                e.open_session_from(rows);
            }
        },
    )
    .expect("the session's rows are read");
    let e = shared.into_inner().expect("the engine");
    assert!(
        matches!(e.mode, Mode::FieldDay { running: false, .. }),
        "{contest}: the contest's S&P"
    );
    e
}

/// W9XYZ calls CQ from slot `from` (even) on, the operator picks it, and W9XYZ finishes the
/// contact as WSJT-X does. Every over this station sent.
fn worked(e: &mut Engine, act: Act, pick: Pick, from: u64) -> Vec<String> {
    let mut partner = Partner::new(act, true);
    let mut sent = Vec::new();
    let mut picked = false;
    for slot in from..from + SLOTS {
        // The radio loop's deferred "Disable Tx after sending 73", once the last over has played.
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(e, slot) {
                partner.hear(&text);
                sent.push(text);
            }
            continue;
        }
        let Some(text) = partner.transmit() else {
            e.ingest_decodes_for_test(&[], slot + 1);
            continue;
        };
        e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
        if !picked && text.starts_with("CQ ") {
            picked = true;
            let r = match pick {
                Pick::DoubleClick => {
                    e.work_station(DX, Some(DX_GRID), Some(&text), Some(DX_SNR), None)
                }
                Pick::Reply => e.work_station(DX, None, Some(&text), Some(DX_SNR), Some(1500.0)),
            };
            assert_eq!(r, Ok(()), "{pick:?} on {text:?}");
        }
    }
    sent
}

/// The contest log's rows, as `(call, dupe mark)`.
fn rows(e: &Engine) -> Vec<(String, bool)> {
    match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| (q.call.clone(), q.dupe))
            .collect(),
        _ => Vec::new(),
    }
}

/// What each row records of its contact: both exchanges, the mode and submode, and the band.
fn contents(e: &Engine) -> Vec<String> {
    match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| {
                let (rx, tx) = (fields(&q.rx), fields(&q.tx));
                format!("{rx} {tx} {} {} {}", q.mode, q.submode, q.band)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The contest's numbers on screen: the count, the sections, the points, the score chip and
/// the multipliers.
fn numbers(e: &Engine) -> (usize, usize, u32, u32, u32, Option<u32>, Vec<String>) {
    let fd = e.snapshot().field_day.expect("the contest runs");
    (
        fd.qso_count,
        fd.sections,
        fd.points,
        fd.powered_points,
        fd.total_score,
        fd.mult_count,
        fd.worked_sections,
    )
}

/// `(call, dupe mark)` per row as `n` W9XYZ rows, the first not a dupe.
fn w9xyz(n: usize) -> Vec<(String, bool)> {
    (0..n).map(|i| (DX.to_string(), i > 0)).collect()
}

/// The records of an ADIF text.
fn records(adif: &str) -> usize {
    adif.to_ascii_uppercase().matches("<EOR>").count()
}

/// ⭐ **A dupe picked off the air is logged, marked and worth nothing** — by the cockpit's
/// double-click and by a logger's Reply alike, in Field Day and in a party that sends the
/// standard messages: the same overs on the air as the first contact, a row marked a dupe, the
/// contest's numbers on screen as they were, and the Contest screen's log showing it marked.
/// Picked a third time, the dupe logged before is still there: every pick rebuilds the
/// contest's station from its journal.
#[test]
fn a_double_clicked_or_replied_dupe_is_logged_marked_and_worth_nothing() {
    for (contest, act) in [("arrlfd", Act::FieldDay), ("txqp", Act::None)] {
        for pick in [Pick::DoubleClick, Pick::Reply] {
            let d = Dir::new("picked-dupe");
            let mut e = launch(contest, &d);
            let first = worked(&mut e, act, Pick::DoubleClick, 0);
            assert_eq!(
                rows(&e),
                w9xyz(1),
                "{contest} {pick:?}: PREMISE, the contact"
            );
            let before = numbers(&e);

            let again = worked(&mut e, act, pick, SLOTS);
            assert_eq!(again, first, "{contest} {pick:?}: the same overs");
            assert_eq!(rows(&e), w9xyz(2), "{contest} {pick:?}: logged, marked");
            let contents = contents(&e);
            assert_eq!(
                contents[1], contents[0],
                "{contest} {pick:?}: written as the contact it repeats"
            );
            assert_eq!(numbers(&e), before, "{contest} {pick:?}: worth nothing");
            let shown: Vec<bool> = e
                .snapshot()
                .field_day
                .map(|fd| fd.log.iter().map(|q| q.dupe).collect())
                .unwrap_or_default();
            assert_eq!(shown, [false, true], "{contest} {pick:?}: shown marked");

            worked(&mut e, act, pick, 2 * SLOTS);
            assert_eq!(rows(&e), w9xyz(3), "{contest} {pick:?}: and again");
            assert_eq!(numbers(&e), before, "{contest} {pick:?}: still nothing");
        }
    }
}

/// ⭐ **…and it goes everywhere a contact goes**: into the journal on disk, and back out of it on
/// a restart still marked; into the contest's Cabrillo file as a QSO line and into its ADIF; into
/// the Logbook at the Merge, as the real QSO it is; and to the club, like any contact.
#[test]
fn a_picked_dupe_is_journaled_exported_merged_and_sent_to_the_club() {
    for (contest, act) in [("arrlfd", Act::FieldDay), ("txqp", Act::None)] {
        let d = Dir::new("picked-dupe-out");
        let mut e = launch(contest, &d);
        worked(&mut e, act, Pick::DoubleClick, 0);
        worked(&mut e, act, Pick::Reply, SLOTS);
        assert_eq!(rows(&e), w9xyz(2), "{contest}: PREMISE, the dupe logged");
        e.journal_mark().wait();
        let journal = std::fs::read_to_string(d.0.join("fieldday.adi")).expect("the journal");
        assert_eq!(records(&journal), 2, "{contest}: journaled");

        let cabrillo = e.export_log("cabrillo").expect("the Cabrillo file");
        let qso_lines = cabrillo
            .lines()
            .filter(|l| l.starts_with("QSO:") && l.contains(DX))
            .count();
        assert_eq!(qso_lines, 2, "{contest}: a QSO line in the Cabrillo file");
        let adif = e.export_log("adif").expect("the contest ADIF");
        assert_eq!(records(&adif), 2, "{contest}: in the contest ADIF");
        let club: Vec<String> = e.fd_sync_outbox(0).into_iter().map(|w| w.call).collect();
        assert_eq!(club, [DX, DX], "{contest}: sent to the club");
        let merged = e.fd_merge_to_general().expect("the contest runs");
        assert_eq!(merged.added(), 2, "{contest}: merged into the Logbook");
        let logbook: Vec<String> = e.stored_log().iter().map(|r| r.call.clone()).collect();
        assert_eq!(logbook, [DX, DX], "{contest}: the Logbook holds it");

        // A restart: the journal brings it back, still marked, worth what it was worth.
        let at_quit = numbers(&e);
        e.persist_fd_log();
        e.journal_mark().wait();
        drop(e);
        let back = launch(contest, &d);
        assert_eq!(rows(&back), w9xyz(2), "{contest}: back after a restart");
        assert_eq!(numbers(&back), at_quit, "{contest}: scored the same");
    }
}
