//! A dupe typed into the contest strip: Enter on a station already in this running's log, in the
//! CW, Phone or RTTY cockpit. In the contests that refused one until the operator's ruling of
//! 2026-10-11 (both Field Days and the five state parties), it is logged now, marked a dupe and
//! worth nothing, as it already was in the contests that report their dupes.
//!
//! Driven through the verbs the strip's commands call: `fd_log_manual` (Field Day's two
//! positional slots, as the CW/Phone cockpits send them), `fd_log_manual_submode` (the RTTY
//! cockpit's) and `contest_log_manual` (a party's field vector).

use super::*;
use crate::logstore::tests::Dir;

/// A station in `contest`, Contest mode on, its journal in `d`: K1ABC in Massachusetts, 2B EMA at
/// Field Day, out of state at a party.
fn station(contest: &str, d: &Dir) -> Engine {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    let mut s = e.settings().clone();
    s.fd_event = contest.into();
    s.fd_class = "2B".into();
    s.fd_section = "EMA".into();
    s.contest_qth_state = "MA".into();
    s.fd_active = true;
    e.apply_settings(s);
    e.set_fd_log_path(d.0.join("fieldday.adi"));
    e.set_mode("fieldday-sp").expect("the contest starts");
    e
}

/// W9ABC typed in and logged in `mode`: "CW", "PH", or "DIG" with RTTY on the air. At Field Day
/// its class and section, at the Illinois party its report and county. The engine's answer: did
/// the contact enter the log.
fn typed(e: &mut Engine, contest: &str, mode: &str) -> Result<bool, String> {
    let rtty = (mode == "DIG").then_some("RTTY");
    if contest == "arrlfd" {
        return match rtty {
            Some(sub) => e.fd_log_manual_submode("W9ABC", "2A", "WI", mode, sub),
            None => e.fd_log_manual("W9ABC", "2A", "WI", mode),
        };
    }
    let rst = if mode == "PH" { "59" } else { "599" };
    let fields = [("RST", rst), ("QTH", "ADAM")].map(|(k, v)| (k.to_string(), v.to_string()));
    e.contest_log_manual("W9ABC", &fields, mode, rtty)
}

/// The contest log's dupe marks, in log order.
fn marks(e: &Engine) -> Vec<bool> {
    match &e.mode {
        Mode::FieldDay { station, .. } => station.log.qsos().iter().map(|q| q.dupe).collect(),
        _ => Vec::new(),
    }
}

/// The contest's numbers on screen: the count, the sections, the points, the score chip and the
/// multipliers.
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

/// ⭐ **Enter on a dupe logs it, marked, for 0 points** (the operator's ruling, 2026-10-11: "Enter
/// on a dupe logs it marked as a dupe for 0 points"), in Field Day and the Illinois
/// party, from the CW, Phone and RTTY strips: the engine says it entered the log, the Contest
/// screen shows it marked, the numbers on screen are as they were, and the journal on disk holds
/// it.
#[test]
fn a_typed_dupe_is_logged_marked_and_worth_nothing() {
    for contest in ["arrlfd", "ilqp"] {
        for mode in ["CW", "PH", "DIG"] {
            let d = Dir::new("typed-dupe");
            let mut e = station(contest, &d);
            assert_eq!(typed(&mut e, contest, mode), Ok(true), "{contest} {mode}");
            let before = numbers(&e);

            assert_eq!(
                typed(&mut e, contest, mode),
                Ok(true),
                "{contest} {mode}: the dupe enters the log"
            );
            assert_eq!(marks(&e), [false, true], "{contest} {mode}: marked");
            assert_eq!(numbers(&e), before, "{contest} {mode}: worth nothing");
            let shown: Vec<bool> = e
                .snapshot()
                .field_day
                .map(|fd| fd.log.iter().map(|q| q.dupe).collect())
                .unwrap_or_default();
            assert_eq!(shown, [false, true], "{contest} {mode}: shown marked");
            e.journal_mark().wait();
            let journal = std::fs::read_to_string(d.0.join("fieldday.adi")).expect("the journal");
            let records = journal.to_ascii_uppercase().matches("<EOR>").count();
            assert_eq!(records, 2, "{contest} {mode}: journaled");
        }
    }
}
