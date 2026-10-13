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
    let mut e = Engine::new(call, "EN50", 0);
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
