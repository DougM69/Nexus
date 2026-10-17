//! ⭐ **While contest mode is on, no contact goes to the logging services live.** A contest's
//! contacts reach QRZ, ClubLog, eQSL, HRDLog, Cloudlog and WRL when the contest log is merged
//! into the Logbook at the end, under the session's own upload choice, and LoTW's automatic
//! upload waits for the contest to end. A contact that reaches the Logbook another way while the
//! switch is on (the everyday log form, the Logbook's own add, a log form whose screen lost the
//! contest) is held, and sent once, paced, when contest mode is turned off.
//!
//! A CW contest operator was answered 403 by QRZ on every contact while working a contest, and
//! never outside one: the contacts were going out live, one per QSO, while the contest ran.

use super::*;
use crate::test_util::StoredLog;

/// ARRL Field Day, 3A from Wisconsin, entered in S&P with nothing queued.
fn contest_on() -> Engine {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("Wisconsin is a current section");
    assert!(
        e.take_pending_uploads().is_empty(),
        "premise: nothing queued"
    );
    e
}

/// A CW contact as a log form sends it.
fn cw(call: &str) -> QsoRecord {
    let adif = format!(
        "<CALL:{}>{call}<BAND:3>20m<MODE:2>CW<FREQ:6>14.025<QSO_DATE:8>20261017\
         <TIME_ON:6>120000<RST_SENT:3>599<RST_RCVD:3>599<EOR>",
        call.len()
    );
    tempo_core::logbook::parse_adif(&adif)
        .pop()
        .expect("one record")
}

fn contest_off(e: &mut Engine) {
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
}

fn calls(pending: &[PendingUpload]) -> Vec<&str> {
    pending.iter().map(|p| p.rec.call.as_str()).collect()
}

/// The contest strip's CW contact goes to the contest journal and nowhere else; the everyday
/// log form's contact, logged while the switch is on, reaches the Logbook and is not sent.
#[test]
fn a_cw_contact_in_contest_mode_is_journalled_and_nothing_is_sent_live() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-contest-hold-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("fieldday.adi");
    let mut e = contest_on();
    e.set_fd_log_path(journal.clone());

    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    e.journal_mark().wait();
    let text = std::fs::read_to_string(&journal).expect("the journal was written");
    assert!(
        text.contains("<CALL:5>K1ABC"),
        "the contest contact is journalled"
    );
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());

    e.log_form_qso(cw("W1AW"), StateSource::Operator, Vec::new());
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "a contact logged with contest mode on is not sent live"
    );
    let logged: Vec<String> = e.stored_log().iter().map(|r| r.call.clone()).collect();
    assert_eq!(logged, ["W1AW"], "it is in the Logbook all the same");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The switch is what counts, not whether the screen still holds the contest: a station that
/// visited Chat with contest mode on has left the contest's session, and its CW log form logs
/// through the Logbook. That contact is not sent live either.
#[test]
fn with_the_switch_on_a_contact_logged_outside_the_contest_screen_is_not_sent_live() {
    let mut e = contest_on();
    e.set_mode("chat").unwrap();
    assert!(
        e.snapshot().field_day.is_none(),
        "premise: the contest screen is gone"
    );
    assert!(e.settings().fd_active, "premise: the switch is still on");

    e.log_qso(cw("K1ABC"));
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
}

/// Turning contest mode off sends what it held, once, as catch-up (paced, one slot apart); a
/// second save with it off sends nothing again, and a contact logged after goes out live.
#[test]
fn turning_contest_mode_off_sends_the_held_contacts_once_and_paced() {
    let mut e = contest_on();
    e.log_qso(cw("K1ABC"));
    e.log_qso(cw("W1AW"));
    assert!(e.take_pending_uploads().is_empty(), "premise: held");

    // A save with the switch still on sends nothing.
    let s = e.settings().clone();
    e.apply_settings(s);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());

    contest_off(&mut e);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC", "W1AW"]);
    for p in &sent {
        assert_eq!(
            p.origin,
            UploadOrigin::CatchUp,
            "{} is history, paced",
            p.rec.call
        );
        assert_eq!(
            p.legs,
            upload_legs::ALL,
            "{} owes every service",
            p.rec.call
        );
        assert_eq!(p.attempts, 0);
    }
    assert_eq!(
        sent[1].retry_after_unix - sent[0].retry_after_unix,
        CATCHUP_UPLOAD_SPACING_SECS,
        "one slot apart"
    );

    contest_off(&mut e);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "sent once"
    );
    // …and the next contest's end does not send them again.
    let mut s = e.settings().clone();
    s.fd_active = true;
    e.apply_settings(s);
    contest_off(&mut e);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "sent once, across contests"
    );

    e.log_qso(cw("N0CALL"));
    let live = e.take_pending_uploads();
    assert_eq!(
        calls(&live),
        ["N0CALL"],
        "with the switch off a contact goes out live"
    );
    assert_eq!(live[0].origin, UploadOrigin::Live);
}

/// LoTW's automatic upload waits while contest mode is on, and runs as before once it is off.
#[test]
fn the_lotw_automatic_upload_waits_while_contest_mode_is_on() {
    let mut s = Engine::new("W9XYZ", "EN61", 0).settings().clone();
    s.lotw_auto_upload = true;
    s.lotw_use_adif_location = false;
    s.lotw_station_location = "Home".into();
    s.lotw_last_auto_upload_unix = 0;
    s.fd_active = false;
    assert!(
        lotw_auto_upload_due(&s, false, 1_800_000_000),
        "premise: due"
    );
    s.fd_active = true;
    assert!(!lotw_auto_upload_due(&s, false, 1_800_000_000));
}
