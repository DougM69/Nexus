//! A contest picked while Contest mode is on takes effect on Save: the save ends the running
//! session and starts the new one exactly as turning the master off and on does. Each test
//! builds two stations alike and holds the one switched by the save to the one switched by
//! off-and-on, BY VALUE — the session and its id, the CQ run and the TX latch, the session's log
//! and what it kept out, and what club sync's supervisor is told to run — and each station's
//! journal on disk to its own bytes before the switch (the two stations log their Field Day
//! contact a moment apart, so their journals' timestamps may differ by a second). And to the
//! passive-first rule: the new contest is entered in S&P, never calling CQ.

use super::*;
use crate::logstore::tests::{engine_on_store, Dir};
use std::sync::Mutex;

/// The shell's settings save, as `save_and_publish` makes it: the session's rows read first when
/// the save opens a session, then the save, then the session's sweep built from them. Whether the
/// sweep was installed — or, when the rows could not be read, the refusal.
fn save(m: &Mutex<Engine>, next: Settings) -> Result<bool, String> {
    let asked = next.clone();
    with_session_rows(
        m,
        |e| e.save_opens_session(&asked),
        |e, rows| {
            e.apply_settings(next);
            rows.is_some_and(|r| e.open_session_from(r))
        },
    )
}

/// `edit` made to the station's saved settings, then saved.
fn save_with(m: &Mutex<Engine>, edit: impl FnOnce(&mut Settings)) -> Result<bool, String> {
    let mut next = engine_lock(m).settings().clone();
    edit(&mut next);
    save(m, next)
}

/// A station running ARRL Field Day under `pick` and calling CQ in it, hosting its club, with
/// the Illinois QSO Party's station data already set (so a pick of the party can start), and
/// one Field Day contact in its journal, on disk.
fn running_field_day(d: &Dir, pick: &str) -> Mutex<Engine> {
    let mut e = engine_on_store(d);
    e.set_fd_log_path(d.0.join("fd.adi"));
    let m = Mutex::new(e);
    let opened = save_with(&m, |s| {
        s.fd_event = pick.into();
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        s.fd_position_id = "a1b2c3d4".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
        s.fd_host_enable = true;
        s.fd_host_port = 7373;
        s.fd_active = true;
    });
    assert_eq!(opened, Ok(true), "premise: Field Day opened with its sweep");
    let mut e = engine_lock(&m);
    e.set_mode("fieldday-run")
        .expect("premise: calling CQ in Field Day");
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    let logged = e.contest_log_manual(
        "K1ABC",
        &[pair("CLASS", "2A"), pair("SECTION", "EMA")],
        "CW",
        None,
    );
    assert_eq!(logged, Ok(true), "premise: a Field Day contact");
    let mark = e.journal_mark();
    drop(e);
    mark.wait();
    m
}

/// One station's contest state, by value, for two stations to be compared field by field.
#[derive(Debug, PartialEq)]
struct Contest {
    /// `Some(running)` in a contest session, `None` outside one.
    running: Option<bool>,
    /// The session's contest (rules-file id), its id and its sponsor's contest id.
    session: Option<(String, String, String)>,
    cq_running: bool,
    tx_enabled: bool,
    immediate_tx: bool,
    queued: (usize, usize),
    master: bool,
    pick: String,
    /// The calls in the session's log.
    log: Vec<String>,
    kept_out: Option<crate::dto::FdKeptOutDto>,
    /// What club sync's supervisor reads each second: the targets, the host's contest and the
    /// position's. A change in either contest tears that side down and starts it again.
    club: ((Option<u16>, Option<String>), String, String),
}

/// The station's journal on disk, once every write it has handed over has landed.
fn journal(m: &Mutex<Engine>, d: &Dir) -> String {
    let mark = engine_lock(m).journal_mark();
    mark.wait();
    std::fs::read_to_string(d.0.join("fd.adi")).unwrap_or_default()
}

fn state(m: &Mutex<Engine>) -> Contest {
    let e = engine_lock(m);
    let (running, session) = match &e.mode {
        Mode::FieldDay { station, running } => {
            let s = &station.log.session;
            (
                Some(*running),
                Some((
                    s.event_id.clone(),
                    format!("{:?}", s.my_location),
                    s.contest_id.clone(),
                )),
            )
        }
        _ => (None, None),
    };
    let fd = e.snapshot().field_day;
    Contest {
        running,
        session,
        cq_running: e.cq_running,
        tx_enabled: e.tx_enabled(),
        immediate_tx: e.immediate_tx,
        queued: (e.tx_queue.len(), e.broadcast_queue.len()),
        master: e.settings().fd_active,
        pick: e.settings().fd_event.clone(),
        log: fd
            .as_ref()
            .map(|f| f.log.iter().map(|q| q.call.clone()).collect())
            .unwrap_or_default(),
        kept_out: fd.and_then(|f| f.kept_out),
        club: (
            e.fd_sync_targets(),
            e.fd_club_contest(),
            e.fd_position_contest(),
        ),
    }
}

/// The session's start, for the window the switch was made in.
fn session_start(m: &Mutex<Engine>) -> Option<u64> {
    match &engine_lock(m).mode {
        Mode::FieldDay { station, .. } => Some(station.log.session.start_unix),
        _ => None,
    }
}

/// ★ A NEW PICK SAVED WHILE CONTEST MODE IS ON SWITCHES THE RUNNING CONTEST, EXACTLY AS OFF AND ON
/// DOES. Two stations alike, both running ARRL Field Day and calling CQ, hosting their club, one
/// Field Day contact each. One saves the Illinois QSO Party with the master left on; the other
/// turns the master off (the pick saved with it) and on again — the only way to switch before.
/// Every value read afterwards is the same: the party's session and its id, entered in S&P with
/// the CQ run over and nothing queued; the Field Day contact out of the party's log and counted as
/// kept out; club sync's host and position both moved to the party; and each opened the session
/// with its sweep, read before the switch. Each journal is its own bytes from before the switch.
#[test]
fn a_new_pick_saved_while_the_mode_is_on_switches_exactly_as_off_and_on() {
    let (d1, d2) = (Dir::new("pick-saved"), Dir::new("pick-cycled"));
    let (saved, cycled) = (
        running_field_day(&d1, "arrlfd"),
        running_field_day(&d2, "arrlfd"),
    );
    let before = state(&saved);
    let journals = (journal(&saved, &d1), journal(&cycled, &d2));
    assert_eq!(before, state(&cycled), "premise: two stations alike");
    assert_eq!(
        (before.running, before.cq_running, before.tx_enabled),
        (Some(true), true, true),
        "premise: calling CQ in Field Day"
    );
    assert_eq!(before.log, vec!["K1ABC"], "premise: one Field Day contact");
    assert_eq!(
        before.club,
        (
            (Some(7373), Some("127.0.0.1:7373".to_string())),
            "arrlfd".to_string(),
            "arrlfd".to_string()
        ),
        "premise: the club hosted and joined for Field Day"
    );

    let t0 = now_unix_secs();
    let by_save = save_with(&saved, |s| s.fd_event = "ilqp".into());
    let by_off = save_with(&cycled, |s| {
        s.fd_active = false;
        s.fd_event = "ilqp".into();
    });
    let by_on = save_with(&cycled, |s| s.fd_active = true);
    let t1 = now_unix_secs();

    let (after, reference) = (state(&saved), state(&cycled));
    assert_eq!(
        after, reference,
        "the save does exactly what off-and-on does"
    );
    assert_eq!(
        (journal(&saved, &d1), journal(&cycled, &d2)),
        journals.clone(),
        "each journal as it was, by either way: nothing rewritten, nothing lost"
    );
    assert_eq!(
        after.session,
        Some((
            "ilqp".to_string(),
            "MyLocation { county: Some(\"COOK\"), state: \"IL\", dxcc: false }".to_string(),
            "IL QSO Party".to_string()
        )),
        "the party's session, from the place off-and-on gives it"
    );
    assert_eq!(
        after.running,
        Some(false),
        "entered in S&P: never calling CQ"
    );
    assert!(!after.cq_running, "the Field Day CQ run is over");
    assert_eq!(after.queued, (0, 0), "nothing queued to send");
    assert!(after.master, "the master still on");
    assert!(
        after.log.is_empty(),
        "the Field Day contact is not in the party's log"
    );
    assert_eq!(
        after.kept_out,
        Some(crate::dto::FdKeptOutDto {
            other_contest: 1,
            other_running: 0
        }),
        "it is kept apart, and the screen is told"
    );
    assert_eq!(
        after.club,
        (
            (Some(7373), Some("127.0.0.1:7373".to_string())),
            "ilqp".to_string(),
            "ilqp".to_string()
        ),
        "club sync's supervisor is told the party for host and position alike, so it tears the \
         Field Day club down and starts the party's"
    );
    for (m, how) in [(&saved, "the save"), (&cycled, "off-and-on")] {
        let start = session_start(m).expect("a session");
        assert!(
            (t0..=t1).contains(&start),
            "{how}: the session starts at the switch ({start} in {t0}..={t1})"
        );
    }
    assert_eq!(
        (by_save, by_off, by_on),
        (Ok(true), Ok(false), Ok(true)),
        "the save opened the party's session with its sweep, read first, as the turn-on does"
    );

    // The contacts that follow stay apart in both: the party's contact beside Field Day's,
    // which is kept on disk exactly as Field Day wrote it.
    let fd_row = |journal: &str| {
        let at = journal.find("<CALL:5>K1ABC").expect("Field Day's row");
        let end = at + journal[at..].find("<EOR>").expect("its end") + "<EOR>".len();
        journal[at..end].to_string()
    };
    for (m, d, was) in [(&saved, &d1, &journals.0), (&cycled, &d2, &journals.1)] {
        let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
        let logged = engine_lock(m).contest_log_manual(
            "N9AAA",
            &[pair("RST", "599"), pair("QTH", "LAKE")],
            "CW",
            None,
        );
        assert_eq!(logged, Ok(true), "a party contact");
        assert_eq!(
            state(m).log,
            vec!["N9AAA"],
            "the party's log holds the party's"
        );
        let now = journal(m, d);
        assert!(
            now.contains(&fd_row(was)) && now.contains("<CALL:5>N9AAA"),
            "Field Day's contact kept as written, beside the party's:\n{now}"
        );
    }
}

/// A pick the station cannot start yet — the party's county left blank — ends the running
/// contest exactly as off-and-on does and starts nothing: out of the contest, the CQ run over,
/// the master still on, the journal untouched. The Contest screen's start blocker is what then
/// says what is missing; the next save that fills it enters the party.
#[test]
fn a_pick_that_cannot_start_yet_ends_the_old_contest_as_off_and_on_does() {
    let (d1, d2) = (Dir::new("blocked-saved"), Dir::new("blocked-cycled"));
    let (saved, cycled) = (
        running_field_day(&d1, "arrlfd"),
        running_field_day(&d2, "arrlfd"),
    );
    let journals = (journal(&saved, &d1), journal(&cycled, &d2));
    let by_save = save_with(&saved, |s| {
        s.fd_event = "ilqp".into();
        s.contest_qth_county.clear();
    });
    let by_off = save_with(&cycled, |s| {
        s.fd_active = false;
        s.fd_event = "ilqp".into();
        s.contest_qth_county.clear();
    });
    let by_on = save_with(&cycled, |s| s.fd_active = true);
    let (after, reference) = (state(&saved), state(&cycled));
    assert_eq!(
        after, reference,
        "the save does exactly what off-and-on does"
    );
    assert_eq!(
        (after.running, after.cq_running, after.master),
        (None, false, true),
        "out of the contest, the CQ run over, the master still on"
    );
    assert_eq!(
        (journal(&saved, &d1), journal(&cycled, &d2)),
        journals,
        "each journal untouched"
    );
    assert_eq!(
        (by_save, by_off, by_on),
        (Ok(false), Ok(false), Ok(false)),
        "no session opened"
    );
    // CONTROL: the county filled, the next save enters the party.
    assert_eq!(
        save_with(&saved, |s| s.contest_qth_county = "COOK".into()),
        Ok(true)
    );
    assert_eq!(state(&saved).session.map(|s| s.0), Some("ilqp".to_string()));
}

/// ⭐ ONLY A SAVE THAT MOVES THE PICK SWITCHES. A save that leaves it where it was — the scoring
/// panel's bonus tick, any other field — keeps the running contest, its CQ run and its session,
/// whatever the session runs: a pick the rules data cannot resolve runs Field Day's session under
/// another id, and every later save would otherwise restart it.
#[test]
fn a_save_that_leaves_the_pick_keeps_the_running_contest() {
    for pick in ["", "arrlfd", "nosuchparty"] {
        let d = Dir::new("pick-kept");
        let m = running_field_day(&d, pick);
        let (before, start) = (state(&m), session_start(&m));
        assert_eq!(
            before.session.as_ref().map(|s| s.0.as_str()),
            Some("arrlfd"),
            "premise ({pick:?}): Field Day runs"
        );
        assert_eq!(
            save_with(&m, |s| s.fd_bonuses.push("media".into())),
            Ok(false),
            "{pick:?}: an unrelated save opens no session"
        );
        let after = state(&m);
        assert_eq!(
            (
                after.running,
                after.cq_running,
                &after.session,
                session_start(&m)
            ),
            (Some(true), true, &before.session, start),
            "{pick:?}: the same session, still calling CQ"
        );
    }
}

/// A save that moves the pick to the contest ALREADY running keeps it: a pick the rules data
/// could not resolve ran Field Day, and picking ARRL Field Day names what runs. Nothing ends.
#[test]
fn a_pick_of_the_contest_already_running_keeps_it() {
    let d = Dir::new("pick-same");
    let m = running_field_day(&d, "nosuchparty");
    let (before, start) = (state(&m), session_start(&m));
    assert_eq!(
        save_with(&m, |s| s.fd_event = "arrlfd".into()),
        Ok(false),
        "no session opened"
    );
    let after = state(&m);
    assert_eq!(
        (
            after.running,
            after.cq_running,
            &after.session,
            session_start(&m)
        ),
        (Some(true), true, &before.session, start),
        "the same session, still calling CQ"
    );
    assert_eq!(after.pick, "arrlfd", "and the pick saved");
}
