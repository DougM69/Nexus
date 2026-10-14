//! Club sync for a contest that is not a Field Day — the whole stack over 127.0.0.1:
//! real engines running the Illinois QSO Party, the REAL `fdbridge` impls,
//! `fdsync::serve_until` and the real position pumps. Loopback only, ephemeral ports,
//! no real network.
//!
//! What the club needs on the day: a host (itself position #1, over its own loopback
//! listener) and a second position merge into one club log that keeps every county,
//! counts CW and digital as one mode, and writes the party's own Cabrillo; the second
//! position to work a station has the club's key for it before it logs; a position that
//! joins LATE gets the whole club state; one that drops off logs offline and catches up
//! when it comes back; a restarted host rebuilds the party's club from its journal; and
//! a position logging another contest is refused, by name, without one row reaching the
//! club.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;

/// 64 hex digits for `tag`, generated (a per-run seed through the operating system's hasher):
/// the same all through one run, and never a real key or hash.
fn throwaway_hex(tag: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
        .collect()
}

/// The club key a position `posid` sends this run: the same through its restarts, as a
/// laptop's own key is (the shell keeps it beside settings.json).
fn club_key(posid: &str) -> fdsync::PositionKey {
    fdsync::PositionKey::new(throwaway_hex(posid))
}

/// An Illinois QSO Party position at an Illinois county: master on, the party picked,
/// S&P, a position id and a name, pointed at the club.
fn party_engine(posid: &str, name: &str, join_addr: &str) -> Shared {
    party_engine_on("W9XYZ", posid, name, join_addr)
}

/// [`party_engine`]'s position, on the station call `call`.
fn party_engine_on(call: &str, posid: &str, name: &str, join_addr: &str) -> Shared {
    party_engine_kept(call, posid, name, join_addr, None)
}

/// [`party_engine_on`]'s position, keeping its own contest journal at `journal` as every laptop
/// does: Contest mode turned off and on again, or Nexus reopened, brings its contacts back.
fn party_engine_kept(
    call: &str,
    posid: &str,
    name: &str,
    join_addr: &str,
    journal: Option<&std::path::Path>,
) -> Shared {
    let mut e = Engine::new(call, "EN50", 0);
    if let Some(path) = journal {
        e.set_fd_log_path(path.to_path_buf());
    }
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "MCLN".into();
    s.contest_entry_class = "UNLIMITED".into();
    s.fd_position_id = posid.into();
    s.fd_position_name = name.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("enter the party");
    e.set_fd_position_key(club_key(posid));
    Arc::new(Mutex::new(e))
}

/// A Field Day position — the wrong contest for this club.
fn field_day_engine(posid: &str, join_addr: &str) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "IL".into();
    s.fd_position_id = posid.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("enter Field Day");
    e.set_fd_position_key(club_key(posid));
    Arc::new(Mutex::new(e))
}

/// Bind a loopback listener with SO_REUSEADDR (the host "restart" rebinds the same port
/// while accepted sockets may sit in TIME_WAIT).
fn reusable_listener(port: u16) -> std::net::TcpListener {
    let raw = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap();
    raw.set_reuse_address(true).unwrap();
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    raw.bind(&addr.into()).unwrap();
    raw.listen(16).unwrap();
    raw.into()
}

fn start_host(eng: &Shared, listener: std::net::TcpListener) -> Arc<AtomicBool> {
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn ClubBackend> = Arc::new(EngineClubBackend(eng.clone()));
    let sd2 = sd.clone();
    std::thread::spawn(move || fdsync::serve_until(listener, backend, sd2));
    sd
}

fn start_pump(eng: &Shared, addr: &str) -> Arc<AtomicBool> {
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn PositionSync> = Arc::new(EnginePositionSync::new(eng.clone()));
    let (a, sd2) = (addr.to_string(), sd.clone());
    std::thread::spawn(move || fdsync::run_position_until(&a, backend, sd2));
    sd
}

/// Log one party contact the way the entry strip does: the RST at the digits the mode
/// uses, and the county they sent.
fn log_party(eng: &Shared, call: &str, county: &str, mode: &str) {
    let (rst, sub) = match mode {
        "PH" => ("59", None),
        "DIG" => ("599", Some("RTTY")),
        _ => ("599", None),
    };
    let fields = [
        ("RST".to_string(), rst.to_string()),
        ("QTH".to_string(), county.to_string()),
    ];
    assert!(
        engine_lock(eng)
            .contest_log_manual(call, &fields, mode, sub)
            .expect("the party is running"),
        "{call} {mode} from {county} refused by the position's own log — fixture bug"
    );
}

fn wait_until(what: &str, secs: u64, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}");
}

/// Rows the host's club log holds, raw (dupes included).
fn club_rows(eng: &Shared) -> u64 {
    engine_lock(eng)
        .fd_club_log()
        .map(|c| c.qsos_raw())
        .unwrap_or(0)
}

/// The club keys a position's mirror holds.
fn mirror_keys(eng: &Shared) -> HashSet<Vec<String>> {
    engine_lock(eng).fd_mirror_mut().dkeys.clone()
}

/// The club-only keys a position's screen is handed for its while-typing check.
fn screen_club_keys(eng: &Shared) -> Vec<Vec<String>> {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.dkeys)
        .unwrap_or_default()
}

/// The newest row's key under the party's own dupe rule, from the position's own log.
fn newest_own_key(eng: &Shared) -> Vec<String> {
    let e = engine_lock(eng);
    let f = e.snapshot().field_day.expect("the party is running");
    f.log.last().expect("a logged row").dkey.clone()
}

#[test]
fn an_illinois_qso_party_club_merges_dupes_late_joins_drops_and_a_host_restart() {
    let dir = std::env::temp_dir().join(format!("ilqp-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_event.ilqp.jsonl");

    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");

    // The host — also position #1, joined over its own loopback listener.
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let p2 = party_engine("bbbb0002", "CW tent", &addr);
    let p2_sd = start_pump(&p2, &addr);

    // The host works K9AAA on CW from Cook.
    log_party(&host, "K9AAA", "COOK", "CW");
    wait_until("the host's contact reached the club", 10, || {
        club_rows(&host) == 1
    });
    wait_until("the club key reached the CW tent", 10, || {
        !screen_club_keys(&p2).is_empty()
    });
    let warned = screen_club_keys(&p2);
    // The CW tent works the same station on RTTY — the same CW/digital mode at this
    // party. Its own log takes it (it has never worked K9AAA); the CLUB is what knew.
    log_party(&p2, "K9AAA", "COOK", "DIG");
    let key = newest_own_key(&p2);
    assert!(
        warned.contains(&key),
        "before it logged, the second tent held the club's key for this contact \
         {key:?} — what its while-typing warning reads: {warned:?}"
    );
    // A new county is a new station (the mobile rule), and a phone contact elsewhere.
    log_party(&p2, "K9AAA", "WILL", "CW");
    log_party(&p2, "K9BBB", "KANE", "PH");
    wait_until("four rows merged", 10, || club_rows(&host) == 4);
    {
        let eng = engine_lock(&host);
        let club = eng.fd_club_log().unwrap();
        assert_eq!(club.event_id, "ilqp", "the club runs the party");
        assert_eq!(club.qsos_unique(), 3, "the RTTY repeat is the club dupe");
    }

    // --- a late joiner gets the whole club state ---------------------------------
    let p3 = party_engine("cccc0003", "SSB tent", &addr);
    let p3_sd = start_pump(&p3, &addr);
    wait_until("the late joiner's mirror holds every club key", 10, || {
        mirror_keys(&p3).len() == 3
    });
    assert_eq!(
        mirror_keys(&p3),
        mirror_keys(&p2),
        "late or early, one club"
    );

    // --- a position logging another contest is refused, by name ------------------
    let fd = field_day_engine("dddd0004", &addr);
    log_party_fd(&fd);
    let fd_sd = start_pump(&fd, &addr);
    wait_until("the Field Day position was told why", 10, || {
        engine_lock(&fd).fd_mirror_mut().last_error.is_some()
    });
    let why = engine_lock(&fd).fd_mirror_mut().last_error.clone().unwrap();
    assert!(
        why.starts_with(
            "this club is running Illinois QSO Party, and this Nexus is logging ARRL Field Day."
        ),
        "the refusal names both contests as the picker does: {why}"
    );
    fd_sd.store(true, Ordering::Relaxed);
    assert_eq!(
        club_rows(&host),
        4,
        "not one Field Day row reached the party's club"
    );
    // The HOST refused the join itself. The position checks the welcome too and would
    // refuse with the same words, so the sentence alone cannot say which end did it —
    // a host that let the join through would hold the position on its board.
    assert!(
        !engine_lock(&host)
            .fd_club_log()
            .unwrap()
            .positions()
            .contains_key("dddd0004"),
        "the host refused the Field Day position at JOIN"
    );

    // --- the CW tent drops off, logs offline, and catches up ---------------------
    p2_sd.store(true, Ordering::Relaxed);
    wait_until("the CW tent's link is down", 10, || {
        !engine_lock(&p2).fd_mirror_mut().connected
    });
    log_party(&p2, "K9CCC", "DUPG", "CW");
    let state = engine_lock(&p2).fd_sync_state();
    match state {
        tempo_app::fdevent::SyncState::Offline { queued, .. } => {
            assert_eq!(queued, 1, "the offline contact queues")
        }
        other => panic!("expected Offline, got {other:?}"),
    }
    let p2_sd = start_pump(&p2, &addr);
    wait_until("the offline contact reached the club", 10, || {
        club_rows(&host) == 5
    });

    // --- the club file is the party's -------------------------------------------
    let cab = engine_lock(&host)
        .fd_club_export(true)
        .expect("the club file");
    assert!(cab.contains("CONTEST: ILLINOIS QSO PARTY\n"), "{cab}");
    assert!(cab.contains("ENTRY-CLASS: UNLIMITED\n"), "{cab}");
    // Each line carries both sides' county — ours (McLean) and theirs — with the RST at
    // the digits its mode uses. The times are the positions' own clocks, so only the
    // exchange columns are pinned.
    let lines: Vec<&str> = cab.lines().filter(|l| l.starts_with("QSO:")).collect();
    for tail in [
        "W9XYZ 599 MCLN K9AAA 599 COOK",
        "W9XYZ 599 MCLN K9AAA 599 WILL",
        "W9XYZ 59 MCLN K9BBB 59 KANE",
        "W9XYZ 599 MCLN K9CCC 599 DUPG",
    ] {
        assert!(
            lines.iter().any(|l| l.ends_with(tail)),
            "no QSO line ends {tail:?}: {cab}"
        );
    }
    // Five rows, one of them the club dupe (the RTTY repeat): in the file as a zero-point line,
    // as every contest logs its dupes since the operator's ruling of 2026-10-11.
    assert_eq!(lines.len(), 5, "{cab}");

    // --- the host restarts: its journal rebuilds the party's club ----------------
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    let host2 = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(club_rows(&host2), 5, "the journal alone rebuilds the club");
    assert_eq!(
        engine_lock(&host2).fd_club_export(true).unwrap(),
        cab,
        "…and the same club file"
    );
    let host2_sd = start_host(&host2, reusable_listener(port));
    let host2_pump_sd = start_pump(&host2, &addr);
    log_party(&p3, "K9EEE", "LAKE", "PH");
    wait_until("the reborn host takes new contacts", 20, || {
        club_rows(&host2) == 6
    });
    wait_until("every position converges on the reborn host", 20, || {
        let n = engine_lock(&host2).fd_club_counts().0;
        mirror_keys(&p2).len() == n && mirror_keys(&p3).len() == n
    });

    for sd in [host2_sd, host2_pump_sd, p2_sd, p3_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The lines of a Cabrillo file that start `{tag}:`.
fn header<'a>(cab: &'a str, tag: &str) -> Vec<&'a str> {
    cab.lines()
        .filter(|l| l.starts_with(&format!("{tag}:")))
        .collect()
}

/// ⭐ **The club's files keep the call and the entry class the club was hosted under, whatever
/// the host laptop is set to afterwards.** "Contest over?" puts back the station's own call and
/// class in the very save that turns Contest mode off, and the Club Cabrillo button is on the
/// Contest screen, which shows only with the mode on. The files used to read both at export, so
/// a host that ended the party before exporting submitted the club's contacts under its
/// operator's own call, with no class.
///
/// CONTROL first: before the put-back the file is the club's, and the put-back really lands in
/// Settings. Then the file after it, after Contest mode goes back on under the operator's own
/// call to reach the button, and after the host restarts from its journal so set: each the very
/// bytes of the file before it, the Club ADIF too.
#[test]
fn the_club_files_keep_the_call_and_class_the_club_was_hosted_under() {
    let dir = std::env::temp_dir().join(format!("ilqp-hosted-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_event.ilqp.jsonl");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let p2 = party_engine("bbbb0002", "CW tent", &addr);
    let p2_sd = start_pump(&p2, &addr);
    log_party(&host, "K9AAA", "COOK", "PH");
    log_party(&p2, "K9BBB", "KANE", "CW");
    wait_until("both contacts merged", 10, || club_rows(&host) == 2);

    let cabrillo = |eng: &Shared| {
        engine_lock(eng)
            .fd_club_export(true)
            .expect("club cabrillo")
    };
    let adif = |eng: &Shared| engine_lock(eng).fd_club_export(false).expect("club adif");
    let before = cabrillo(&host);
    let before_adif = adif(&host);
    assert_eq!(
        (header(&before, "CALLSIGN"), header(&before, "ENTRY-CLASS")),
        (vec!["CALLSIGN: W9XYZ"], vec!["ENTRY-CLASS: UNLIMITED"]),
        "CONTROL: the club's file before the put-back: {before}"
    );
    let lines: Vec<&str> = before.lines().filter(|l| l.starts_with("QSO:")).collect();
    assert_eq!(lines.len(), 2, "{before}");
    assert!(
        lines.iter().all(|l| l.contains(" W9XYZ ")),
        "CONTROL: every contact sent as the club: {before}"
    );

    // "Contest over?" → Turn off with the call and the class ticked: ONE save, the mode off.
    let put_back = |eng: &Shared, active: bool| {
        let mut e = engine_lock(eng);
        let mut s = e.settings().clone();
        s.fd_active = active;
        s.mycall = "K9OWN".into();
        s.contest_entry_class = String::new();
        e.apply_settings(s);
    };
    put_back(&host, false);
    {
        let e = engine_lock(&host);
        assert_eq!(
            (
                e.settings().mycall.as_str(),
                e.settings().contest_entry_class.as_str(),
                e.settings().fd_active,
                e.fd_hosting(),
            ),
            ("K9OWN", "", false, true),
            "CONTROL: the put-back landed, the mode is off, and the laptop still hosts"
        );
    }
    let after = cabrillo(&host);
    assert_eq!(
        (header(&after, "CALLSIGN"), header(&after, "ENTRY-CLASS")),
        (vec!["CALLSIGN: W9XYZ"], vec!["ENTRY-CLASS: UNLIMITED"]),
        "the club's call and class after the put-back: {after}"
    );
    assert_eq!(after, before, "the club file after the put-back");
    assert_eq!(adif(&host), before_adif, "the Club ADIF after the put-back");

    // Contest mode back on, to reach the Club Cabrillo button, under the operator's own call.
    {
        let mut e = engine_lock(&host);
        let mut s = e.settings().clone();
        s.fd_active = true;
        e.apply_settings(s);
        let _ = e.set_mode("fieldday-sp");
    }
    {
        let e = engine_lock(&host);
        assert_eq!(
            (
                e.settings().mycall.as_str(),
                e.snapshot().field_day.and_then(|f| f.club).is_some()
            ),
            ("K9OWN", true),
            "CONTROL: the mode is back on under the operator's own call, the club block with it"
        );
    }
    assert_eq!(
        cabrillo(&host),
        before,
        "the club file with the mode back on"
    );

    // The host restarts with the put-back saved, and rebuilds the club from its journal.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    p2_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    let host2 = party_engine("aaaa0001", "HQ", &addr);
    put_back(&host2, false);
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        club_rows(&host2),
        2,
        "CONTROL: the journal rebuilds the club"
    );
    assert_eq!(
        engine_lock(&host2).settings().mycall,
        "K9OWN",
        "CONTROL: the restarted host is on the operator's own call"
    );
    assert_eq!(
        cabrillo(&host2),
        before,
        "the club file after the host restarts"
    );
    assert_eq!(
        adif(&host2),
        before_adif,
        "the Club ADIF after the host restarts"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The host's warnings: each position sending another county than the club, as
/// `(name, sent, club's)`.
fn other_county(eng: &Shared) -> Vec<(String, String, String)> {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.other_location)
        .unwrap_or_default()
        .into_iter()
        .map(|o| (o.pos_name, o.sent, o.club))
        .collect()
}

/// ⭐ **The host names a position sending another county than the club's.** A laptop set up on
/// another Illinois county joins (only the side of the state line is checked at JOIN), and every
/// contact it logs sends that county under the club's call: a busted exchange for each station
/// it works. Its contacts are still taken, as sent; the host's screen now says which position,
/// what it sends and what the club sends, from its newest contact.
///
/// CONTROL: a position on the club's county, and the host itself, are not named; the joined
/// laptops' own screens carry no such list; and the position named is not once its newest
/// contact sends the club's county again.
#[test]
fn the_host_names_a_position_sending_another_county() {
    let dir = std::env::temp_dir().join(format!("ilqp-county-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_event.ilqp.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let kane = party_engine("bbbb0002", "CW tent", &addr);
    engine_lock(&kane)
        .contest_i_moved(vec![("QTH".into(), "KANE".into())])
        .expect("the CW tent is set up on Kane");
    let kane_sd = start_pump(&kane, &addr);
    let ssb = party_engine("cccc0003", "SSB tent", &addr);
    let ssb_sd = start_pump(&ssb, &addr);
    log_party(&host, "K9AAA", "COOK", "PH");
    log_party(&kane, "K9BBB", "COOK", "CW");
    log_party(&ssb, "K9CCC", "LAKE", "PH");
    wait_until("three contacts merged", 10, || club_rows(&host) == 3);
    let cab = engine_lock(&host)
        .fd_club_export(true)
        .expect("club cabrillo");
    assert!(
        cab.lines()
            .any(|l| l.starts_with("QSO:") && l.ends_with("W9XYZ 599 KANE K9BBB 599 COOK")),
        "CONTROL: the contact is taken, as the CW tent sent it: {cab}"
    );
    let warning = |name: &str, sent: &str| vec![(name.to_string(), sent.into(), "MCLN".into())];
    assert_eq!(
        other_county(&host),
        warning("CW tent", "KANE"),
        "the host names the CW tent, Kane against the club's McLean, and no one else"
    );
    assert!(
        other_county(&kane).is_empty() && other_county(&ssb).is_empty(),
        "CONTROL: the host's alone"
    );

    // The CW tent put right: its next contact sends the club's county.
    engine_lock(&kane)
        .contest_i_moved(vec![("QTH".into(), "MCLN".into())])
        .expect("the CW tent is set to McLean");
    log_party(&kane, "K9DDD", "WILL", "CW");
    wait_until("the fourth contact merged", 10, || club_rows(&host) == 4);
    assert_eq!(
        other_county(&host),
        Vec::<(String, String, String)>::new(),
        "named no more once its newest contact sends the club's county"
    );
    for sd in [host_sd, host_pump_sd, kane_sd, ssb_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// One Field Day contact, so the refused position has a row it would have sent.
fn log_party_fd(eng: &Shared) {
    assert!(engine_lock(eng)
        .fd_log_manual("K1ABC", "2A", "EMA", "CW")
        .expect("Field Day mode"));
}

/// A party position set up out of state: Indiana, so it sends its state instead of a county.
fn indiana_party_engine(posid: &str, name: &str, join_addr: &str) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IN".into();
    s.fd_position_id = posid.into();
    s.fd_position_name = name.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("enter the party");
    e.set_fd_position_key(club_key(posid));
    Arc::new(Mutex::new(e))
}

/// ⭐ **A position set up in another exchange role than the club's is refused, by name, on
/// both screens** — over the real bridge and sockets. In the Illinois QSO Party a station in
/// Illinois sends its county and one outside it sends its state, and a club entry is one
/// station in one place: an Indiana-configured laptop at the club's site would send every
/// contact with the wrong exchange into the club's in-state log. CONTROL: the same position,
/// set up in Illinois, joins and its contact merges — and the host's note for it goes.
#[test]
fn a_position_set_up_out_of_state_is_refused_by_name_on_both_screens() {
    let dir = std::env::temp_dir().join(format!("ilqp-role-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_event.ilqp.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let out = indiana_party_engine("bbbb0002", "SSB tent", &addr);
    log_party(&out, "K9AAA", "COOK", "PH");
    let out_sd = start_pump(&out, &addr);

    let position_error = |eng: &Shared| {
        engine_lock(eng)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .and_then(|c| c.last_error)
            .unwrap_or_default()
    };
    wait_until("the position's screen says why", 10, || {
        position_error(&out).contains("out-of-state")
    });
    let said = position_error(&out);
    assert!(
        said.contains("in-state") && said.contains("Your station data"),
        "{said}"
    );
    let refused = engine_lock(&host)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.refused)
        .unwrap_or_default();
    assert_eq!(refused.len(), 1, "the host's screen names it: {refused:?}");
    assert_eq!(
        (
            refused[0].pos_name.as_str(),
            refused[0].call.as_str(),
            refused[0].reason.as_str()
        ),
        ("SSB tent", "W9XYZ", said.as_str()),
        "who, and the very sentence it was sent"
    );
    assert_eq!(
        club_rows(&host),
        0,
        "not one of its contacts reached the club"
    );

    // CONTROL: set up in Illinois, the same position joins and its contact merges.
    out_sd.store(true, Ordering::Relaxed);
    let fixed = party_engine("bbbb0002", "SSB tent", &addr);
    log_party(&fixed, "K9BBB", "COOK", "PH");
    let fixed_sd = start_pump(&fixed, &addr);
    wait_until("the position set up in Illinois merges", 20, || {
        club_rows(&host) == 1
    });
    wait_until("the host's note for it goes", 10, || {
        engine_lock(&host)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .is_some_and(|c| c.refused.is_empty())
    });
    for sd in [host_sd, host_pump_sd, fixed_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A position on another station call than the host's is refused, by name, on both
/// screens** — over the real bridge and sockets. At the Illinois QSO Party every laptop of a
/// club entry sends the club's call: one still set to its owner's call would send that call
/// on the air while the club's file claims its contacts under the host's. CONTROL: the same
/// position on the club's call joins and its contact merges — and the host's note for it goes.
#[test]
fn a_position_on_another_call_is_refused_by_name_on_both_screens() {
    let dir = std::env::temp_dir().join(format!("ilqp-call-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_event.ilqp.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let own = party_engine_on("K9ABC", "bbbb0002", "SSB tent", &addr);
    log_party(&own, "K9AAA", "COOK", "PH");
    let own_sd = start_pump(&own, &addr);

    let position_error = |eng: &Shared| {
        engine_lock(eng)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .and_then(|c| c.last_error)
            .unwrap_or_default()
    };
    wait_until("the position's screen says why", 10, || {
        position_error(&own).contains("this Nexus as K9ABC,")
    });
    let said = position_error(&own);
    assert!(
        said.contains("on the air as W9XYZ") && said.contains("Callsign on the air"),
        "{said}"
    );
    let refused = engine_lock(&host)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.refused)
        .unwrap_or_default();
    assert_eq!(refused.len(), 1, "the host's screen names it: {refused:?}");
    assert_eq!(
        (
            refused[0].pos_name.as_str(),
            refused[0].call.as_str(),
            refused[0].reason.as_str()
        ),
        ("SSB tent", "K9ABC", said.as_str()),
        "who, on which call, and the very sentence it was sent"
    );
    assert_eq!(
        club_rows(&host),
        0,
        "not one of its contacts reached the club"
    );

    // CONTROL: on the club's call, the same position joins and its contact merges.
    own_sd.store(true, Ordering::Relaxed);
    let fixed = party_engine("bbbb0002", "SSB tent", &addr);
    log_party(&fixed, "K9BBB", "COOK", "PH");
    let fixed_sd = start_pump(&fixed, &addr);
    wait_until("the position on the club's call merges", 20, || {
        club_rows(&host) == 1
    });
    wait_until("the host's note for it goes", 10, || {
        engine_lock(&host)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .is_some_and(|c| c.refused.is_empty())
    });
    for sd in [host_sd, host_pump_sd, fixed_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each QSO line of a club's Cabrillo file as `(call sent, call worked)`.
fn worked(cab: &str) -> Vec<(String, String)> {
    cab.lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f[5].to_string(), f[f.len() - 3].to_string())
        })
        .collect()
}

/// ⭐ **After "Contest over?" on the host, a laptop of the club still on the club's call is
/// taken back in, and what it still owes reaches the club's log and files.** The put-back gives
/// the host laptop its operator's own call, and the JOIN's call gate read the host's Settings, so
/// a laptop rejoining on the club's call was turned away ("this club is on the air as K9OWN and
/// this Nexus as W9XYZ"), and the contacts it logged while its link was down never reached the
/// club. While the host holds the club's log, the gate, the welcome and the beacon read the call
/// the club's files are written under.
///
/// CONTROL first: the CW tent's first contact merges, and the put-back lands with the laptop
/// still hosting. Then the CW tent, back on the club's call, joins, and its waiting contact is in
/// the club's log, once, and in the Club Cabrillo and Club ADIF under the club's call; its
/// welcome and the host's beacon name the club's call. A laptop on another call is still turned
/// away, by the same sentence naming the club's call, and none of its contacts reaches the club.
/// The host's own laptop, its link made again on the operator's own call, is in as it always was.
/// And when the host closes Nexus the CW tent's next contact waits, its link lost; the host
/// reopened from its journal still holds the club's call, and the contact arrives.
#[test]
fn a_laptop_on_the_club_call_rejoins_after_the_hosts_put_back() {
    let dir = std::env::temp_dir().join(format!("ilqp-rejoin-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_event.ilqp.jsonl");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let cw = party_engine("bbbb0002", "CW tent", &addr);
    let cw_sd = start_pump(&cw, &addr);
    log_party(&cw, "K9AAA", "COOK", "CW");
    wait_until("the CW tent's first contact merged", 10, || {
        club_rows(&host) == 1
    });

    // Whether the host holds a live link for `pos`.
    let connected = |eng: &Shared, pos: &str| {
        engine_lock(eng)
            .fd_club_log()
            .is_some_and(|c| c.connected(pos))
    };
    // What a laptop's screen says of its link: where it stands, and the host's words when it
    // was turned away.
    let link = |eng: &Shared| {
        engine_lock(eng)
            .fd_club_link()
            .map(|l| (l.state, l.told.unwrap_or_default()))
            .unwrap_or_default()
    };
    let joined = || ("joined".to_string(), String::new());
    // A laptop's club chip: its state and the contacts the club's log does not have yet.
    let chip = |eng: &Shared| {
        engine_lock(eng)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .map(|c| (c.sync_state, c.queued))
            .unwrap_or_default()
    };
    let club_file = |eng: &Shared| {
        engine_lock(eng)
            .fd_club_export(true)
            .expect("club cabrillo")
    };
    let sent_as_the_club = |calls: &[&str]| {
        calls
            .iter()
            .map(|c| ("W9XYZ".to_string(), c.to_string()))
            .collect::<Vec<_>>()
    };

    // The CW tent's link goes (it is carried out of the Wi-Fi's reach) and it logs on: that
    // contact waits in its own log.
    cw_sd.store(true, Ordering::Relaxed);
    wait_until("the host hears the CW tent's link go", 10, || {
        !connected(&host, "bbbb0002")
    });
    log_party(&cw, "K9BBB", "KANE", "CW");

    // "Contest over?" on the host: Turn off with the call and the class ticked, ONE save.
    let put_back = |eng: &Shared| {
        let mut e = engine_lock(eng);
        let mut s = e.settings().clone();
        s.fd_active = false;
        s.mycall = "K9OWN".into();
        s.contest_entry_class = String::new();
        e.apply_settings(s);
    };
    put_back(&host);
    {
        let e = engine_lock(&host);
        assert_eq!(
            (
                e.settings().mycall.as_str(),
                e.settings().fd_active,
                e.fd_hosting()
            ),
            ("K9OWN", false, true),
            "CONTROL: the put-back landed, the mode is off, and the laptop still hosts"
        );
    }

    // The CW tent is back, still on the club's call.
    let cw_sd = start_pump(&cw, &addr);
    wait_until("the CW tent is answered", 20, || {
        club_rows(&host) == 2 || link(&cw).0 == "turnedAway"
    });
    assert_eq!(
        (club_rows(&host), link(&cw)),
        (2, joined()),
        "the CW tent, on the club's call, is taken back in and its waiting contact merges"
    );
    assert_eq!(
        (
            engine_lock(&cw)
                .fd_club_link()
                .map(|l| l.host_call)
                .unwrap_or_default(),
            engine_lock(&host).fd_club_call()
        ),
        ("W9XYZ".to_string(), "W9XYZ".to_string()),
        "its welcome names the club's call, and so does the host's beacon"
    );
    let cab = club_file(&host);
    assert_eq!(
        worked(&cab),
        sent_as_the_club(&["K9AAA", "K9BBB"]),
        "the contact that waited is in the Club Cabrillo once, sent as the club: {cab}"
    );
    let adif = engine_lock(&host).fd_club_export(false).expect("club adif");
    assert!(
        adif.contains("<CALL:5>K9BBB"),
        "…and in the Club ADIF: {adif}"
    );

    // CONTROL: a laptop on another call is still turned away, by the sentence it always met, now
    // naming the club's call, and not one of its contacts reaches the club.
    let other = party_engine_on("K9ABC", "cccc0003", "SSB tent", &addr);
    log_party(&other, "K9CCC", "COOK", "PH");
    let other_sd = start_pump(&other, &addr);
    wait_until("the laptop on another call is answered", 10, || {
        link(&other).0 == "turnedAway" || club_rows(&host) == 3
    });
    let contest = engine_lock(&host).fd_club_log().unwrap().contest_id.clone();
    assert_eq!(
        (link(&other), club_rows(&host)),
        (
            (
                "turnedAway".to_string(),
                tempo_app::fdevent::call_mismatch(&contest, "W9XYZ", "K9ABC")
            ),
            2
        ),
        "the laptop on another call is turned away and none of its contacts merges"
    );
    other_sd.store(true, Ordering::Relaxed);

    // The host's own laptop, its link made again on the operator's own call (as reopening Nexus
    // makes it): in, as it always was.
    host_pump_sd.store(true, Ordering::Relaxed);
    wait_until("the host hears its own link go", 10, || {
        !connected(&host, "aaaa0001") && !engine_lock(&host).fd_mirror_mut().connected
    });
    let host_pump_sd = start_pump(&host, &addr);
    wait_until("the host's own laptop is answered", 20, || {
        matches!(link(&host).0.as_str(), "joined" | "turnedAway")
    });
    assert_eq!(
        link(&host),
        joined(),
        "the host's own laptop is in on the operator's own call"
    );

    // The host closes Nexus: no club log is held. The CW tent's link is lost, and its next
    // contact waits in its own log, counted on its chip.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    wait_until("the CW tent's link is lost", 10, || link(&cw).0 == "lost");
    log_party(&cw, "K9DDD", "WILL", "CW");
    assert_eq!(
        chip(&cw),
        ("offline".to_string(), 1),
        "the CW tent's contact waits"
    );

    // Reopened with the put-back saved, the host rebuilds the club from its journal, the call it
    // was hosted under with it, and the CW tent rejoins by itself and sends what waited.
    std::thread::sleep(Duration::from_millis(600));
    let host2 = party_engine("aaaa0001", "HQ", &addr);
    put_back(&host2);
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        (club_rows(&host2), engine_lock(&host2).fd_club_call()),
        (2, "W9XYZ".to_string()),
        "the reopened host holds the club's contacts and the call they were sent under"
    );
    let host2_sd = start_host(&host2, reusable_listener(port));
    let host2_pump_sd = start_pump(&host2, &addr);
    wait_until("the CW tent is answered by the reopened host", 30, || {
        club_rows(&host2) == 3 || link(&cw).0 == "turnedAway"
    });
    assert_eq!(
        (club_rows(&host2), link(&cw)),
        (3, joined()),
        "the CW tent rejoins the reopened host and its waiting contact merges"
    );
    let cab = club_file(&host2);
    assert_eq!(
        worked(&cab),
        sent_as_the_club(&["K9AAA", "K9BBB", "K9DDD"]),
        "{cab}"
    );

    for sd in [cw_sd, host2_sd, host2_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A laptop's own contest log, by call.
fn own_calls(eng: &Shared) -> Vec<String> {
    engine_lock(eng)
        .snapshot()
        .field_day
        .map(|f| f.log.iter().map(|r| r.call.clone()).collect())
        .unwrap_or_default()
}

/// A journal as a Nexus older than the call on each contact wrote it: every `APP_NEXUS_ONAIR`
/// field, and the space after it, taken out.
fn as_an_older_nexus_wrote_it(adif: &str) -> String {
    const TAG: &str = "<APP_NEXUS_ONAIR:";
    let mut out = String::new();
    let mut rest = adif;
    while let Some(at) = rest.find(TAG) {
        out.push_str(&rest[..at]);
        let after = &rest[at + TAG.len()..];
        let close = after.find('>').expect("a closed field");
        let len: usize = after[..close].parse().expect("a field length");
        rest = &after[close + 1 + len + 1..];
    }
    out.push_str(rest);
    out
}

/// ⭐ **The club's log takes a contact only on the call it holds.** After "Contest over?" on the
/// host, which puts the operator's own call back, the host turns Contest mode on again under that
/// call to reach the Club Cabrillo button, still hosting. A contact it then logged went up over its
/// own link into the club's log and its files, under the club's call. A contact logged on another
/// call than the club's now stays in that laptop's own log: it is never sent, the club chip does
/// not count it as owed, and the laptop's own screen says so.
///
/// CONTROL first: the party merges, the put-back lands and the mode is back on under the
/// operator's own call while the host holds the club's. Then the host's contact on its own call
/// is in its own log and nowhere in the club's, while a contact on the club's call after it is
/// taken. A contact that carries no call (a journal an older Nexus wrote) follows the call its
/// link joined under, the club's, and is taken. A joined laptop that changes its call keeps its
/// link, and its next contact stays out. And the host reopened with the club's call typed back
/// still keeps out the contact it logged on its own call: each contact's call is in its journal.
#[test]
fn a_contact_logged_on_another_call_stays_out_of_the_club_log() {
    let dir = std::env::temp_dir().join(format!("ilqp-other-call-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("fd_event_event.ilqp.jsonl");
    let hq_log = dir.join("hq.adi");
    let ssb_log = dir.join("ssb.adi");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = party_engine_kept("W9XYZ", "aaaa0001", "HQ", &addr, Some(&hq_log));
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let cw = party_engine("bbbb0002", "CW tent", &addr);
    let cw_sd = start_pump(&cw, &addr);
    log_party(&host, "K9AAA", "COOK", "PH");
    log_party(&cw, "K9BBB", "KANE", "CW");
    wait_until("the party's two contacts merged", 10, || {
        club_rows(&host) == 2
    });

    // Where a laptop's link stands.
    let link = |eng: &Shared| {
        engine_lock(eng)
            .fd_club_link()
            .map(|l| l.state)
            .unwrap_or_default()
    };
    // A laptop's club chip: its state and the contacts the club's log does not have yet.
    let chip = |eng: &Shared| {
        engine_lock(eng)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .map(|c| (c.sync_state, c.queued))
            .unwrap_or_default()
    };
    let synced = || ("synced".to_string(), 0);
    // What a laptop's own screen says it logged on another call than the club's.
    let other_call = |eng: &Shared| {
        engine_lock(eng)
            .fd_club_link()
            .and_then(|l| l.other_call)
            .unwrap_or(0)
    };
    // The Club Cabrillo's contacts, (call sent, call worked), in call order.
    let club_file = |eng: &Shared| {
        let cab = engine_lock(eng)
            .fd_club_export(true)
            .expect("club cabrillo");
        let mut worked = worked(&cab);
        worked.sort();
        worked
    };
    let sent_as_the_club = |calls: &[&str]| {
        calls
            .iter()
            .map(|c| ("W9XYZ".to_string(), c.to_string()))
            .collect::<Vec<_>>()
    };
    let club_adif = |eng: &Shared| engine_lock(eng).fd_club_export(false).expect("club adif");

    // "Contest over?" on the host, ONE save, then Contest mode back on under the operator's own
    // call to reach the Club Cabrillo button.
    {
        let mut e = engine_lock(&host);
        let mut s = e.settings().clone();
        s.fd_active = false;
        s.mycall = "K9OWN".into();
        s.contest_entry_class = String::new();
        e.apply_settings(s);
        let mut s = e.settings().clone();
        s.fd_active = true;
        e.apply_settings(s);
    }
    {
        let e = engine_lock(&host);
        assert_eq!(
            (
                e.settings().mycall.as_str(),
                e.fd_hosting(),
                e.fd_club_call(),
                e.snapshot().field_day.and_then(|f| f.club).is_some()
            ),
            ("K9OWN", true, "W9XYZ".to_string(), true),
            "CONTROL: the put-back landed, the host holds the club's call, and the mode is back on"
        );
    }
    log_party(&host, "K9EEE", "COOK", "PH");
    // A contact on the club's call after it, from the CW tent.
    log_party(&cw, "K9FFF", "WILL", "CW");
    wait_until("the CW tent's contact merged", 10, || club_rows(&host) >= 3);
    // Long enough for the host's own link to send anything it was going to.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        club_file(&host),
        sent_as_the_club(&["K9AAA", "K9BBB", "K9FFF"]),
        "the contact the host logged on its own call is not in the Club Cabrillo"
    );
    assert_eq!(club_rows(&host), 3, "…nor in the club's log");
    assert!(!club_adif(&host).contains("K9EEE"), "…nor in the Club ADIF");
    assert_eq!(
        own_calls(&host),
        ["K9AAA", "K9EEE"],
        "it is in the host's own log"
    );
    wait_until("the host's chip settles", 10, || chip(&host) == synced());
    assert_eq!(
        other_call(&host),
        1,
        "the host's own screen counts the contact logged on another call"
    );
    assert_eq!(
        (chip(&cw), other_call(&cw)),
        (synced(), 0),
        "CONTROL: the CW tent, on the club's call"
    );

    // A laptop whose journal an older Nexus wrote: the contact it logged before its link came up
    // carries no call.
    {
        let first = party_engine_kept("W9XYZ", "cccc0003", "SSB tent", &addr, Some(&ssb_log));
        log_party(&first, "K9GGG", "KANE", "PH");
        let mark = engine_lock(&first).journal_mark();
        mark.wait();
    }
    let written = std::fs::read_to_string(&ssb_log).unwrap();
    let older = as_an_older_nexus_wrote_it(&written);
    assert!(
        written.contains("<APP_NEXUS_ONAIR:5>W9XYZ")
            && !older.contains("APP_NEXUS_ONAIR")
            && older.contains("<CALL:5>K9GGG"),
        "CONTROL: this build writes the call, and the older journal carries none: {older}"
    );
    std::fs::write(&ssb_log, older).unwrap();
    let ssb = party_engine_kept("W9XYZ", "cccc0003", "SSB tent", &addr, Some(&ssb_log));
    assert_eq!(
        own_calls(&ssb),
        ["K9GGG"],
        "CONTROL: its contact is back from the journal"
    );
    let ssb_sd = start_pump(&ssb, &addr);
    wait_until("the SSB tent's contact merged", 10, || {
        club_rows(&host) == 4
    });
    assert_eq!(
        club_file(&host),
        sent_as_the_club(&["K9AAA", "K9BBB", "K9FFF", "K9GGG"]),
        "a contact with no call, on a link joined on the club's call, is taken"
    );
    wait_until("the SSB tent's chip settles", 10, || chip(&ssb) == synced());
    assert_eq!(other_call(&ssb), 0, "nothing of the SSB tent's is kept out");

    // The CW tent, joined, changes its call; its link stays up.
    {
        let mut e = engine_lock(&cw);
        let mut s = e.settings().clone();
        s.mycall = "K9ABC".into();
        e.apply_settings(s);
    }
    log_party(&cw, "K9HHH", "WILL", "CW");
    log_party(&ssb, "K9JJJ", "WILL", "CW");
    wait_until("the SSB tent's second contact merged", 10, || {
        club_rows(&host) >= 5
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(link(&cw), "joined", "CONTROL: the CW tent's link stays up");
    assert_eq!(
        club_file(&host),
        sent_as_the_club(&["K9AAA", "K9BBB", "K9FFF", "K9GGG", "K9JJJ"]),
        "the CW tent's contact on its new call is not in the Club Cabrillo"
    );
    wait_until("the CW tent's chip settles", 10, || chip(&cw) == synced());
    assert_eq!(
        other_call(&cw),
        1,
        "the CW tent's own screen counts its contact on the new call"
    );

    // The host closes Nexus and is reopened with the club's call typed back in Settings.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    let host2 = party_engine_kept("W9XYZ", "aaaa0001", "HQ", &addr, Some(&hq_log));
    assert_eq!(
        own_calls(&host2),
        ["K9AAA", "K9EEE"],
        "CONTROL: the host's journal brings both its contacts back"
    );
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        (club_rows(&host2), engine_lock(&host2).fd_club_call()),
        (5, "W9XYZ".to_string()),
        "CONTROL: the reopened host rebuilds the club's log under the club's call"
    );
    let host2_sd = start_host(&host2, reusable_listener(port));
    let host2_pump_sd = start_pump(&host2, &addr);
    wait_until("the host's own laptop joins again", 20, || {
        link(&host2) == "joined"
    });
    wait_until("the reopened host's chip settles", 10, || {
        chip(&host2) == synced()
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        club_file(&host2),
        sent_as_the_club(&["K9AAA", "K9BBB", "K9FFF", "K9GGG", "K9JJJ"]),
        "the contact the host logged on its own call stays out after it reopens on the club's call"
    );
    assert_eq!(other_call(&host2), 1, "and its screen still says so");

    for sd in [cw_sd, ssb_sd, host2_sd, host2_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
