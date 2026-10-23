//! ⭐ **Every contest log numbers its contacts 1, 2, 3…** — the contests that send a serial and
//! the ones that do not.
//!
//! The number is the one N1MM shows: in a serial contest it IS the serial the contact was sent,
//! never a second count beside it; in every other contest it is the contact's place in the run,
//! in the order the contacts were logged. Either way it is a number already given: a removed
//! contact keeps its number out of use, an edit keeps it, a restart gives every row back the
//! number it had and the next contact the next one.
//!
//! ⚠️ Nothing here is written down. The number is read off the rows, live and removed, every
//! time it is asked for, so a journal written before numbers existed loads exactly as it did,
//! and the journal a numbered log writes carries no number at all.
use tempo_core::contest::{merge_into_general, ContestSession, FieldValue, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{ContactEdit, FdEvent, FieldDayLog};
use tempo_core::logbook::Logbook;

/// Inside the Illinois QSO Party, the instant `tests/contest_remove_last.rs` works it at.
const T0: u64 = 1_792_342_800;
/// Inside the Sweepstakes CW weekend.
const SS: u64 = 1_794_085_440;

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ilqp_session() -> ContestSession {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("ilqp is a shipped ruleset");
    let station = StationData {
        contest_qth_state: "IL".into(),
        contest_qth_county: "KANE".into(),
        ..Default::default()
    };
    ContestSession::for_ruleset(rs, &station).unwrap_or_else(|e| panic!("ilqp refused: {e}"))
}

fn ilqp_log() -> FieldDayLog {
    FieldDayLog::new("W9XYZ", ilqp_session(), "20m")
}

fn party(log: &mut FieldDayLog, call: &str, county: &str, at: u64) {
    assert!(
        log.log_fields_at(
            call,
            &fields(&[("RST", "599"), ("QTH", county)]),
            "CW",
            "",
            0,
            at,
        ),
        "{call} was logged"
    );
}

fn ss_session() -> ContestSession {
    let rs = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("arrlss_cw is shipped");
    let station = StationData {
        mycall: "W9XYZ".into(),
        fd_section: "WI".into(),
        contest_check: "74".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        ..Default::default()
    };
    ContestSession::for_ruleset(rs, &station).expect("the entrant is fully declared")
}

fn serial_of(tx: &[FieldValue]) -> &str {
    tx.iter()
        .find(|v| v.key == "NR")
        .map(|v| v.raw.as_str())
        .expect("Sweepstakes sends a serial")
}

/// Work `call` in Sweepstakes as the operator does: compose (which issues the serial), then log.
fn ss_work(log: &mut FieldDayLog, call: &str, at: u64) -> u32 {
    let issued = serial_of(&log.session.compose_for(call, at).tx)
        .parse()
        .expect("a number");
    assert!(log.log_fields_at(
        call,
        &fields(&[
            ("NR", "12"),
            ("PREC", "A"),
            ("CALL", call),
            ("CK", "71"),
            ("SEC", "CT"),
        ]),
        "CW",
        "",
        0,
        at,
    ));
    issued
}

/// The number of every live row, in the log's order.
fn numbers(log: &FieldDayLog) -> Vec<Option<u32>> {
    let by_seq = log.qso_numbers();
    log.qsos()
        .iter()
        .map(|q| by_seq.get(&q.seq).copied())
        .collect()
}

fn restart(journal: &str, session: ContestSession) -> FieldDayLog {
    let mut log = FieldDayLog::new("W9XYZ", session, "20m");
    log.merge_adif(journal, 0);
    log
}

#[test]
fn a_contest_that_sends_no_serial_numbers_its_contacts_in_logged_order() {
    let mut log = ilqp_log();
    assert_eq!(
        log.session.serial_now(),
        None,
        "PREMISE: ILQP sends a county, not a serial"
    );
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    assert_eq!(numbers(&log), [Some(1), Some(2), Some(3)]);
}

/// A contact typed in late, with an earlier time, takes the next number — the order the contacts
/// were LOGGED in, as N1MM numbers them, not the order of their times.
#[test]
fn a_back_dated_contact_takes_the_next_number_and_moves_nobody() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0 + 600);
    party(&mut log, "W9BBB", "LAKE", T0 + 660);
    party(&mut log, "K9CCC", "DUPG", T0);
    let by_call: Vec<(String, Option<u32>)> = log
        .qsos()
        .iter()
        .map(|q| (q.call.clone(), log.qso_numbers().get(&q.seq).copied()))
        .collect();
    assert!(
        by_call.contains(&("K9AAA".into(), Some(1)))
            && by_call.contains(&("W9BBB".into(), Some(2)))
            && by_call.contains(&("K9CCC".into(), Some(3))),
        "{by_call:?}"
    );
}

/// ⭐ **A removed contact's number stays spent** — N1MM's rule: the number may already have
/// been read out or copied, so the next contact never takes it back.
#[test]
fn a_removed_contact_keeps_its_number_out_of_use() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    // The middle one, as a Logbook delete of its merged copy takes it out.
    let middle = log.qsos()[1].seq;
    assert!(log.remove_row(middle, T0 + 130).is_some());
    assert_eq!(numbers(&log), [Some(1), Some(3)], "no renumbering");
    // …and the newest, as Remove takes it out.
    log.remove_last("K9CCC", T0 + 120, T0 + 140)
        .expect("the newest");
    party(&mut log, "N9DDD", "WILL", T0 + 180);
    assert_eq!(
        numbers(&log),
        [Some(1), Some(4)],
        "the next contact takes the next number"
    );
    // A removed contact put back comes back with its own number.
    let id = log.removed()[0].id();
    log.restore(id).expect("restored");
    assert_eq!(numbers(&log), [Some(1), Some(2), Some(4)]);
}

#[test]
fn a_dupe_is_a_contact_and_takes_a_number() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "K9AAA", "COOK", T0 + 60);
    assert!(log.qsos()[1].dupe, "PREMISE: ILQP logs its dupes");
    assert_eq!(numbers(&log), [Some(1), Some(2)]);
}

#[test]
fn an_edited_contact_keeps_its_number() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    let edit = ContactEdit {
        call: "W9BBX".into(),
        rows: vec![fields(&[("RST", "599"), ("QTH", "WILL")])],
        band: "40m".into(),
        mode: "CW".into(),
    };
    log.edit_contact("W9BBB", T0 + 60, &edit).expect("edited");
    let by_seq = log.qso_numbers();
    let edited = log
        .qsos()
        .iter()
        .find(|q| q.call == "W9BBX")
        .expect("there");
    assert_eq!(by_seq.get(&edited.seq), Some(&2));
    assert_eq!(numbers(&log).len(), 3);
}

/// ⭐ **A restart gives every row the number it had, the next contact the next one**, a removed
/// contact's number included.
#[test]
fn a_restart_resumes_the_numbers_and_never_repeats_one() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    log.remove_last("K9CCC", T0 + 120, T0 + 130)
        .expect("the newest");
    let journal = log.journal_adif();
    let mut back = restart(&journal, ilqp_session());
    assert_eq!(back.qso_count(), 2, "POSITIVE CONTROL: the rows came back");
    assert_eq!(numbers(&back), [Some(1), Some(2)]);
    party(&mut back, "N9DDD", "WILL", T0 + 180);
    assert_eq!(
        numbers(&back),
        [Some(1), Some(2), Some(4)],
        "3 was removed, never reused"
    );
}

/// The number is read off the rows and never written beside them: the journal a numbered log
/// writes is the journal it always wrote.
#[test]
fn the_journal_carries_no_number() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    let journal = log.journal_adif();
    assert!(!journal.contains("<STX:"), "{journal}");
}

/// A journal from before row sequence numbers (no `APP_NEXUS_QSEQ`) is numbered by the order its
/// rows were written in, which is the order they were logged.
#[test]
fn a_journal_without_sequence_numbers_is_numbered_in_file_order() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    let journal = log.journal_adif();
    let stripped: String = journal
        .split("<APP_NEXUS_QSEQ:")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else {
                // Drop "n>value" up to the next tag.
                part[part.find('<').expect("another tag follows")..].to_string()
            }
        })
        .collect();
    assert!(
        !stripped.contains("APP_NEXUS_QSEQ"),
        "PREMISE: no sequence numbers left"
    );
    let back = restart(&stripped, ilqp_session());
    assert_eq!(back.qso_count(), 3, "POSITIVE CONTROL");
    assert_eq!(numbers(&back), [Some(1), Some(2), Some(3)]);
}

/// Clear test contacts starts the numbers again at 1, as it starts the serial run again.
#[test]
fn clear_test_contacts_starts_the_numbers_again() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    log.remove_all(2, "W9BBB", T0 + 60, T0 + 70)
        .expect("cleared");
    party(&mut log, "K9CCC", "DUPG", T0 + 120);
    assert_eq!(numbers(&log), [Some(1)]);
    let back = restart(&log.journal_adif(), ilqp_session());
    assert_eq!(numbers(&back), [Some(1)], "…after a restart too");
    // A test contact put back after the clear keeps the number it had in the run it was logged
    // in, as it keeps the serial it was sent; it does not take a number in the new run.
    let first = log.removed()[0].id();
    log.restore(first).expect("restored");
    assert_eq!(
        log.qsos()[0].call,
        "K9AAA",
        "PREMISE: it goes back in its seq place"
    );
    assert_eq!(numbers(&log), [Some(1), Some(1)]);
}

/// ⭐ **In a serial contest the number IS the serial the contact was sent** — one count, not two.
/// A station given a number and never logged leaves that number unused, and the log shows the
/// numbers that went on the air.
#[test]
fn a_serial_contests_number_is_the_serial_it_sent() {
    let mut log = FieldDayLog::new("W9XYZ", ss_session(), "20m");
    let a = ss_work(&mut log, "K2DEF", SS);
    // A station called and given 2, who never came back.
    let _ = log.session.compose_for("W1AW", SS + 30);
    log.session.clear_in_flight();
    let c = ss_work(&mut log, "N5XYZ", SS + 60);
    assert_eq!((a, c), (1, 3), "PREMISE: 2 went out and was never logged");
    assert_eq!(numbers(&log), [Some(1), Some(3)]);
}

#[test]
fn a_field_day_log_is_numbered_too() {
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
        "20m",
    );
    assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
    assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
    assert_eq!(numbers(&log), [Some(1), Some(2)]);
}

/// ⭐ **The file the operator exports carries the number as ADIF `STX`** in a contest that sends
/// no serial; in a serial contest the serial already is `STX`, once.
#[test]
fn the_exported_adif_carries_the_number_as_stx() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    let adi = log.submission_adif();
    let records: Vec<&str> = adi
        .split("<EOR>")
        .filter(|r| r.contains("<CALL:"))
        .collect();
    assert_eq!(records.len(), 2);
    assert!(records[0].contains("<STX:1>1"), "{}", records[0]);
    assert!(records[1].contains("<STX:1>2"), "{}", records[1]);

    let mut ss = FieldDayLog::new("W9XYZ", ss_session(), "20m");
    ss_work(&mut ss, "K2DEF", SS);
    let _ = ss.session.compose_for("W1AW", SS + 30);
    ss.session.clear_in_flight();
    ss_work(&mut ss, "N5XYZ", SS + 60);
    let adi = ss.submission_adif();
    let records: Vec<&str> = adi
        .split("<EOR>")
        .filter(|r| r.contains("<CALL:"))
        .collect();
    assert_eq!(records[1].matches("<STX:").count(), 1, "{}", records[1]);
    assert!(records[1].contains("<STX:1>3"), "{}", records[1]);
}

/// …and so does its copy in the lifetime log.
#[test]
fn a_merged_copy_carries_the_number_as_stx() {
    let mut log = ilqp_log();
    party(&mut log, "K9AAA", "COOK", T0);
    party(&mut log, "W9BBB", "LAKE", T0 + 60);
    let mut lb = Logbook::new();
    let r = merge_into_general(&log, "a1b2c3d4", &mut lb);
    assert_eq!(r.added(), 2);
    let stx: Vec<Option<u32>> = lb
        .records()
        .iter()
        .map(|rec| rec.contest.as_deref().and_then(|c| c.stx))
        .collect();
    assert_eq!(stx, [Some(1), Some(2)]);
}
