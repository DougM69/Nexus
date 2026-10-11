//! ⭐ **A contest log merged into the Logbook twice writes each contact once** — across I moved,
//! a County edited in Settings, a mode change and a restart, and for contacts merged before
//! this build.
//!
//! The merge names each Logbook copy of a contest contact by its `APP_NEXUS_QID` and skips a
//! contact whose copy is already there. That name was built from where the operator was, so
//! after a move and a restart (or a County edited in Settings and the contest entered again) the
//! same contact had a new name, and a second merge wrote it again; a Logbook edit or delete of
//! an earlier copy no longer reached its contest row either. Every test here goes through the
//! engine, the real logbook store and the real contest journal, and counts the Logbook's rows.

use super::*;
use crate::logstore::tests::{flush, open_fast, Dir};
use crate::test_util::StoredLog;
use std::sync::Mutex;
use tempo_core::logbook::{LogOp, UploadOutcome};

/// This laptop's position id: the second part of every merge identity it mints.
const POS: &str = "a1b2c3d4";

/// The Illinois QSO Party, worked by an Illinois mobile starting in Kane County.
fn ilqp_settings() -> Settings {
    let mut s = Engine::new("W9XYZ", "EN51", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = POS.into();
    s
}

/// ARRL Field Day, 3A from Wisconsin.
fn field_day_settings() -> Settings {
    let mut s = Engine::new("W9XYZ", "EN51", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = POS.into();
    s
}

/// A launch on the data folder `d`, as the app makes one: the settings loaded, the logbook store
/// opened, the contest journal's path set, and the contest the master left on entered again with
/// the session's rows read first — a new session, built from those settings, with the journal's
/// rows read back into it.
fn launch(d: &Dir, s: &Settings) -> Engine {
    let mut e = Engine::with_settings(s.clone());
    e.attach_log_store(open_fast(d));
    e.set_fd_log_path(d.0.join("fieldday.adi"));
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
    let e = shared.into_inner().unwrap();
    assert!(
        e.in_field_day(),
        "the contest the master left on runs again"
    );
    e
}

/// A switch to `spec`, as the mode command makes it: the session's rows read first.
fn switch(e: Engine, spec: &str) -> Engine {
    let shared = Mutex::new(e);
    with_session_rows(
        &shared,
        |e| e.mode_opens_session(spec),
        |e, rows| {
            e.set_mode(spec).expect("the switch");
            if let Some(rows) = rows {
                e.open_session_from(rows);
            }
        },
    )
    .expect("the session's rows are read");
    shared.into_inner().unwrap()
}

/// A quit, as the app makes one: the contest journal written and on disk, and the logbook's
/// writes on disk. The settings the next launch starts from.
fn quit(e: Engine) -> Settings {
    e.persist_fd_log();
    e.journal_mark().wait();
    flush(&e);
    e.settings().clone()
}

fn party(e: &mut Engine, call: &str, county: &str) {
    let rx = [("RST", "599"), ("QTH", county)].map(|(k, v)| (k.to_string(), v.to_string()));
    assert!(
        e.contest_log_manual(call, &rx, "CW", None).unwrap(),
        "{call} logs"
    );
}

/// The Logbook's calls, in log order.
fn calls(e: &Engine) -> Vec<String> {
    e.stored_log().iter().map(|r| r.call.clone()).collect()
}

/// The Logbook's merge identities, in log order.
fn qids(e: &Engine) -> Vec<String> {
    e.stored_log()
        .iter()
        .map(|r| {
            r.contest
                .as_deref()
                .map_or(String::new(), |c| c.qid.clone())
        })
        .collect()
}

/// The merge, as `(written, already there)`.
fn merge(e: &mut Engine) -> (usize, usize) {
    let r = e.fd_merge_to_general().expect("the contest runs");
    (r.added(), r.already)
}

/// The contest log's calls, in log order.
fn contest_calls(e: &Engine) -> Vec<String> {
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    station.log.qsos().iter().map(|q| q.call.clone()).collect()
}

/// ⭐ **Merge, I moved, restart, merge: each contact is in the Logbook once.** The mobile merged
/// from Kane, drove into Will, pressed I moved, and the next launch's session was built in Will.
/// The second merge wrote both contacts again.
#[test]
fn a_merge_after_i_moved_and_a_restart_writes_nothing_already_merged() {
    let d = Dir::new("contest-merge-moved");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0), "the first merge writes both");
    // CONTROL: the same merge again, before anything moves, writes nothing.
    assert_eq!(merge(&mut e), (0, 2));
    let merged = qids(&e);
    let moved = e
        .contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    assert_eq!(
        moved.contest_qth_county, "WILL",
        "the next launch starts in Will"
    );
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "PREMISE: the journal brought both contacts back"
    );
    assert_eq!(
        merge(&mut e),
        (0, 2),
        "nothing merged before is written again"
    );
    assert_eq!(
        calls(&e),
        ["K9AAA", "W9BBB"],
        "each contact once in the Logbook"
    );
    assert_eq!(
        qids(&e),
        merged,
        "the copies already there keep their names"
    );

    // CONTROL: a contact logged after the restart, from Will, is merged — once.
    party(&mut e, "K9CCC", "DUPG");
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(merge(&mut e), (0, 3));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "K9CCC"]);
}

/// ⭐ **A County edited in Settings mid-party, and the contest entered again** — no I moved and
/// no restart: the session is built from Settings each time the contest is entered from another
/// mode, so the edit renamed it the same way.
#[test]
fn a_county_edited_in_settings_mid_party_renames_nothing_already_merged() {
    let d = Dir::new("contest-merge-county-edit");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    let mut s = e.settings().clone();
    s.contest_qth_county = "WILL".into();
    e.apply_settings(s);
    // Out of the contest and back in, as a switch to another mode and back makes it.
    let e = switch(e, "chat");
    let mut e = switch(e, "fieldday-sp");
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "PREMISE: the rows came back"
    );
    assert_eq!(merge(&mut e), (0, 2), "the mode change renamed nothing");
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"]);

    // …and a restart on the edited County.
    let s = quit(e);
    let mut e = launch(&d, &s);
    assert_eq!(merge(&mut e), (0, 2), "nor did the restart");
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"]);
}

/// ⭐ **Field Day's I moved** (a section, the same way): merge from Wisconsin, move to Illinois,
/// restart, merge.
#[test]
fn a_field_day_merge_after_i_moved_and_a_restart_writes_nothing_already_merged() {
    let d = Dir::new("contest-merge-fd-moved");
    let mut e = launch(&d, &field_day_settings());
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
    assert_eq!(merge(&mut e), (2, 0));
    let moved = e
        .contest_i_moved(vec![("SECTION".into(), "IL".into())])
        .expect("IL is a section");
    assert_eq!(moved.fd_section, "IL");
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K1ABC", "W1AW"],
        "PREMISE: both came back"
    );
    assert_eq!(
        merge(&mut e),
        (0, 2),
        "nothing merged from Wisconsin is written again"
    );
    assert_eq!(calls(&e), ["K1ABC", "W1AW"]);
    // CONTROL: a contact from Illinois is merged, once.
    assert!(e.fd_log_manual("K2DEF", "1D", "MN", "CW").unwrap());
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(calls(&e), ["K1ABC", "W1AW", "K2DEF"]);
}

/// Two contacts merged from Kane, I moved to Will, and a restart: the state an edit or a delete
/// of an earlier copy meets.
fn merged_then_moved_and_restarted(d: &Dir) -> Engine {
    let mut e = launch(d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    launch(d, &s)
}

/// The Logbook copy of `call`.
fn copy_of(e: &Engine, call: &str) -> QsoRecord {
    QsoRecord::clone(
        e.stored_log()
            .iter()
            .find(|r| r.call == call)
            .expect("merged"),
    )
}

/// ⭐ **A Logbook edit to a contact merged before the move still reaches its contest row**, and
/// so the Cabrillo.
#[test]
fn a_logbook_edit_after_i_moved_and_a_restart_reaches_the_contest_row() {
    let d = Dir::new("contest-merge-edit");
    let mut e = merged_then_moved_and_restarted(&d);
    // The operator fixes a busted call in the Logbook.
    let mut fixed = copy_of(&e, "K9AAA");
    fixed.call = "K9AAB".into();
    assert!(e.update_qso(fixed.id.unwrap(), fixed), "the edit applies");
    assert_eq!(
        contest_calls(&e),
        ["K9AAB", "W9BBB"],
        "the contest row follows"
    );
    let cbr = e.export_log("cabrillo").expect("one entry");
    assert!(
        cbr.contains("K9AAB") && !cbr.contains("K9AAA"),
        "the Cabrillo kept the busted call:\n{cbr}"
    );
}

/// ⭐ **…and a Logbook delete of one still takes it out of the contest log**, kept as removed.
#[test]
fn a_logbook_delete_after_i_moved_and_a_restart_reaches_the_contest_row() {
    let d = Dir::new("contest-merge-delete");
    let mut e = merged_then_moved_and_restarted(&d);
    assert!(e.delete_qso(copy_of(&e, "W9BBB").id.unwrap()));
    assert_eq!(
        contest_calls(&e),
        ["K9AAA"],
        "the deleted contact left the contest log"
    );
    assert_eq!(
        e.contest_removed()[0].rows[0].call,
        "W9BBB",
        "…kept, restorable"
    );
}

/// The club's rows are named by position and number alone, and a move and a restart change
/// neither: the outbox after the restart is the outbox before it.
#[test]
fn the_club_outbox_is_the_same_after_i_moved_and_a_restart() {
    let d = Dir::new("contest-merge-club");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    let wire = |e: &Engine| -> Vec<(String, u64, String)> {
        e.fd_sync_outbox(0)
            .into_iter()
            .map(|w| (w.pos, w.seq, w.call))
            .collect()
    };
    let before = wire(&e);
    assert_eq!(
        before,
        [
            (POS.to_string(), 1, "K9AAA".to_string()),
            (POS.to_string(), 2, "W9BBB".to_string())
        ]
    );
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(wire(&e), before);
}

/// The Logbook as an earlier build's merge left it, through the real store: K9AAA and W9BBB
/// merged from Kane under the name that build built from where the operator was —
/// `<contest>:KANE:<position>:<number>`, the shape of the copies origin/main wrote in tempo-core's
/// `contest-merge` fixtures — and W9BBB's copy uploaded to QRZ. Then I moved to Will and a
/// restart, on this build.
fn merged_by_an_earlier_build_then_moved(d: &Dir) -> Engine {
    earlier_build_merged_as(d, &["IL QSO Party:KANE"])
}

/// [`merged_by_an_earlier_build_then_moved`], with the contacts merged once under each session
/// name in `sessions`: two names is the logbook that build doubled (merged from Kane, then again
/// under Will after the restart).
fn earlier_build_merged_as(d: &Dir, sessions: &[&str]) -> Engine {
    let mut e = launch(d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    for session in sessions {
        let copies: Vec<QsoRecord> = {
            let Mode::FieldDay { station, .. } = &e.mode else {
                panic!("the contest runs")
            };
            let mut lb = tempo_core::logbook::Logbook::new();
            tempo_core::contest::merge_into_general(&station.log, POS, &mut lb);
            lb.records()
                .iter()
                .zip(station.log.qsos())
                .map(|(r, q)| {
                    let mut r = QsoRecord::clone(r);
                    r.id = None;
                    let c = r.contest.as_deref_mut().expect("merged");
                    c.session = session.to_string();
                    c.qid = format!("{session}:{POS}:{}", q.seq);
                    r
                })
                .collect()
        };
        e.station.append(copies, false);
    }
    let w9bbb = copy_of(&e, "W9BBB");
    assert!(e.stamp_qrz_upload(&w9bbb, UploadOutcome::Accepted, 1_792_343_000, None));
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    launch(d, &s)
}

/// ⭐ **Copies an earlier build merged are recognised through the store**: the merge reads each
/// copy's name AND when its contact was made off the store, and writes neither contact again —
/// only the one that build never merged.
#[test]
fn copies_an_earlier_build_merged_are_not_written_again() {
    let d = Dir::new("contest-merge-earlier-build");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    assert_eq!(merge(&mut e), (0, 2));
    party(&mut e, "N9CCC", "DUPG");
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "N9CCC"]);
    assert_eq!(
        qids(&e),
        [
            format!("IL QSO Party:KANE:{POS}:1"),
            format!("IL QSO Party:KANE:{POS}:2"),
            qid_now(&e, 3),
        ],
        "the earlier copies keep their names; the new one is named by the contest's running"
    );
}

/// The name this build gives row `seq` of the running contest.
fn qid_now(e: &Engine, seq: u64) -> String {
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    let q = station
        .log
        .qsos()
        .iter()
        .find(|q| q.seq == seq)
        .expect("the row");
    tempo_core::contest::MergeIdentity::of(&station.log, POS, q)
        .qid()
        .to_string()
}

/// …a Logbook edit of one reaches its contest row, and a Logbook delete of the other takes it
/// out of the contest log.
#[test]
fn an_edit_or_delete_of_an_earlier_builds_copy_reaches_its_contest_row() {
    let d = Dir::new("contest-merge-earlier-edit");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    let mut fixed = copy_of(&e, "K9AAA");
    fixed.call = "K9AAB".into();
    assert!(e.update_qso(fixed.id.unwrap(), fixed));
    assert_eq!(contest_calls(&e), ["K9AAB", "W9BBB"]);
    assert!(e.delete_qso(copy_of(&e, "W9BBB").id.unwrap()));
    assert_eq!(contest_calls(&e), ["K9AAB"]);
}

/// …and a removal's answer names the services holding an earlier build's copy, and claims no
/// copy for a contact that was never merged — the copies of the rows before it are not its.
#[test]
fn a_removal_names_the_services_holding_an_earlier_builds_copy() {
    let d = Dir::new("contest-merge-earlier-removal");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    party(&mut e, "N9CCC", "DUPG");
    let newest = |e: &Engine| {
        let fd = e.snapshot().field_day.expect("the contest runs");
        let q = fd.log.last().expect("a contact");
        (q.call.clone(), q.when_unix)
    };
    let shared = Mutex::new(e);
    let (call, when) = newest(&engine_lock(&shared));
    let gone = super::contest_removal::remove_last(&shared, &call, when)
        .unwrap()
        .unwrap();
    assert_eq!(gone.removal.entry.rows[0].call, "N9CCC");
    assert_eq!(gone.logbook, None, "N9CCC was never merged");
    let (call, when) = newest(&engine_lock(&shared));
    let gone = super::contest_removal::remove_last(&shared, &call, when)
        .unwrap()
        .unwrap();
    assert_eq!(gone.removal.entry.rows[0].call, "W9BBB");
    assert_eq!(
        gone.logbook,
        Some(vec!["qrz"]),
        "the earlier build's copy, uploaded to QRZ"
    );
}

/// A merge after the restart that writes nothing queues nothing, with the session's uploads on:
/// a copy already in the Logbook is never sent again.
#[test]
fn a_merge_after_a_restart_queues_nothing_already_merged() {
    let d = Dir::new("contest-merge-uploads");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    e.fd_set_upload(true, vec!["qrz".into()]).unwrap();
    assert_eq!(merge(&mut e), (1, 0));
    assert_eq!(
        e.take_pending_uploads().len(),
        1,
        "CONTROL: the first merge queues it"
    );
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    let mut e = launch(&d, &s);
    e.take_pending_uploads();
    e.fd_set_upload(true, vec!["qrz".into()]).unwrap();
    assert_eq!(merge(&mut e), (0, 1));
    assert!(e.take_pending_uploads().is_empty());
}

/// ⭐ **Deleting one of two copies an earlier build wrote tidies the logbook and keeps the contest
/// row**; deleting the last copy takes the contact out of the contest log, as any delete does —
/// through the command every logbook delete goes through.
#[test]
fn deleting_a_duplicate_an_earlier_build_wrote_keeps_the_contest_row() {
    let d = Dir::new("contest-merge-earlier-doubled");
    let e = earlier_build_merged_as(&d, &["IL QSO Party:KANE", "IL QSO Party:WILL"]);
    assert_eq!(
        calls(&e),
        ["K9AAA", "W9BBB", "K9AAA", "W9BBB"],
        "PREMISE: the logbook that build doubled"
    );
    let copy = |e: &Engine, qid: &str| {
        e.stored_log()
            .iter()
            .find(|r| r.contest.as_deref().is_some_and(|c| c.qid == qid))
            .and_then(|r| r.id)
            .expect("the copy")
    };
    let delete = |e: Engine, id| {
        let shared = Mutex::new(e);
        let (made, _) =
            crate::logwrite::change_ops(&shared, id, None, &[LogOp::Delete(id)], "delete");
        assert!(made.is_ok(), "the logbook delete is made: {made:?}");
        shared.into_inner().unwrap()
    };
    let id = copy(&e, &format!("IL QSO Party:WILL:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "K9AAA"]);
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "another copy of W9BBB is in the logbook"
    );
    assert!(e.contest_removed().is_empty());
    // The last copy: the contact leaves the contest log, kept as removed.
    let id = copy(&e, &format!("IL QSO Party:KANE:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(calls(&e), ["K9AAA", "K9AAA"]);
    assert_eq!(contest_calls(&e), ["K9AAA"]);
    assert_eq!(e.contest_removed()[0].rows[0].call, "W9BBB");
}
