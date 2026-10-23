//! The contacts a laptop still owes its club once the mode has left the contest
//! ([`Engine::fd_sync_log`]), through the engine with no sockets: the link and the host's ack set
//! on the mirror as the pump sets them. Counted and sent while club sync runs, and only to the
//! club they were owed to, for the contest it runs, in the running they were logged in.
//! `tests/club_queue_mode_off.rs` runs the same over 127.0.0.1 with a host.

use super::*;
use crate::fdevent::SyncState;

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn save(e: &mut Engine, edit: impl FnOnce(&mut Settings)) {
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

fn party(e: &mut Engine, call: &str, county: &str) {
    assert!(e
        .contest_log_manual(
            call,
            &fields(&[("RST", "599"), ("QTH", county)]),
            "CW",
            None
        )
        .unwrap());
}

/// An Illinois QSO Party position in Kane County, joined to the club at 192.168.1.20: one
/// contact the host acked, then the link down and two it never had, then Contest mode off.
fn two_owed_mode_off() -> Engine {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    save(&mut e, |s| {
        s.mycall = "W9XYZ".into();
        s.fd_active = true;
        s.fd_event = "ilqp".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "KANE".into();
        s.fd_position_id = "aaaa0001".into();
        s.fd_join_addr = "192.168.1.20:42073".into();
    });
    e.set_mode("fieldday-sp").expect("the party runs");
    party(&mut e, "K9AAA", "COOK");
    e.fd_mirror_mut().on_link(true, 1);
    e.fd_mirror_mut().acked = 1;
    e.fd_mirror_mut().on_link(false, 2);
    party(&mut e, "K9BBB", "LAKE");
    party(&mut e, "K9CCC", "WILL");
    save(&mut e, |s| s.fd_active = false);
    assert!(e.snapshot().field_day.is_none(), "premise: the mode is off");
    assert!(e.fd_sync_enabled(), "premise: club sync runs");
    e
}

/// The calls the pump would send now, past the host's ack.
fn owed(e: &Engine) -> Vec<String> {
    e.fd_sync_outbox(e.fd_mirror.acked)
        .into_iter()
        .map(|r| r.call)
        .collect()
}

/// ⭐ **With the mode off, the contacts the host never had are counted, sent, and sent as what
/// they are**: the chip's two, the outbox's two, and the JOIN of the contest and seq they were
/// logged under.
#[test]
fn with_the_mode_off_the_contacts_still_owed_are_counted_and_sent() {
    let e = two_owed_mode_off();
    assert_eq!(
        e.fd_sync_state(),
        SyncState::Offline {
            queued: 2,
            since: 2
        }
    );
    assert_eq!(owed(&e), ["K9BBB", "K9CCC"]);
    let (pos, _, _, max_seq) = e.fd_sync_identity();
    assert_eq!((pos.as_str(), max_seq), ("aaaa0001", 3));
    assert_eq!(e.fd_position_contest(), "ilqp");
}

/// ⭐ **They were that club's: pointed at another club, they are no longer sent or counted.** A
/// save that leaves the club link where it was keeps them (the CONTROL, first).
#[test]
fn the_contacts_the_mode_left_go_to_no_other_club() {
    let mut e = two_owed_mode_off();
    save(&mut e, |s| s.fd_operator = "K9OPB".into());
    assert_eq!(owed(&e), ["K9BBB", "K9CCC"], "control: the same club");
    save(&mut e, |s| s.fd_join_addr = "192.168.1.30:42073".into());
    assert_eq!(owed(&e), Vec::<String>::new(), "another club");
    assert_eq!(
        e.fd_sync_state(),
        SyncState::Offline {
            queued: 0,
            since: 2
        }
    );
    // And going back to the first address does not bring them back: the journal holds them,
    // and the contest's next session sends them.
    save(&mut e, |s| s.fd_join_addr = "192.168.1.20:42073".into());
    assert_eq!(owed(&e), Vec::<String>::new());
}

/// ⭐ **A log the mode left in an earlier running is not sent**: a laptop asleep with its mode off
/// from a club's rehearsal to the day must not send the rehearsal's contacts to the party's host,
/// which the journal restore keeps out of the party's log. Left a year and a month earlier here,
/// which no running of an annual contest spans.
#[test]
fn the_contacts_the_mode_left_wait_for_no_other_running() {
    let mut e = two_owed_mode_off();
    assert_eq!(owed(&e), ["K9BBB", "K9CCC"], "control: this running");
    e.fd_left_log.as_mut().expect("the mode left a log").1 -= 400 * 86_400;
    assert_eq!(owed(&e), Vec::<String>::new());
    assert_eq!(
        e.fd_sync_state(),
        SyncState::Offline {
            queued: 0,
            since: 2
        }
    );
}

/// ⭐ **Only the contest the club runs**: with another contest picked, the party's contacts are
/// not that club's; picked again, they are owed again.
#[test]
fn the_contacts_the_mode_left_go_only_to_their_own_contest() {
    let mut e = two_owed_mode_off();
    save(&mut e, |s| s.fd_event = "nyqp".into());
    assert!(
        e.fd_sync_enabled(),
        "premise: club sync runs the New York QSO Party"
    );
    assert_eq!(owed(&e), Vec::<String>::new());
    assert_eq!(
        e.fd_sync_state(),
        SyncState::Offline {
            queued: 0,
            since: 2
        }
    );
    assert_eq!(e.fd_position_contest(), "nyqp");
    save(&mut e, |s| s.fd_event = "ilqp".into());
    assert_eq!(owed(&e), ["K9BBB", "K9CCC"]);
}
