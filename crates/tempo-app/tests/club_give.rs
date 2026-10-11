//! A club position given to another laptop, through the whole stack over 127.0.0.1: real
//! engines, the REAL `fdbridge` impls, `fdsync::serve_until` and the real position pumps, on the
//! Illinois QSO Party (the club's next event). Loopback only, ephemeral ports.
//!
//! The case is a laptop swapped mid-event: the host gives a position to the laptop that comes to
//! take it over. Every laptop's club dupe warning is read by value off the snapshot its screen
//! reads, as the strip reads it (`contestDupe`): its own log first, then the club's keys.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;
type Keys = BTreeSet<Vec<String>>;

/// 64 hex digits for `tag`, from a per-run seed: never a real key.
fn throwaway_hex(tag: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
        .collect()
}

fn club_key(tag: &str) -> fdsync::PositionKey {
    fdsync::PositionKey::new(throwaway_hex(tag))
}

/// An Illinois QSO Party station in Kane County on the club's call, joining `join_addr`, with
/// the club key `key` names.
fn ilqp_engine(posid: &str, name: &str, join_addr: &str, key: &str) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    let mut s = e.settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = posid.into();
    s.fd_position_name = name.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the party runs");
    e.set_fd_position_key(club_key(key));
    Arc::new(Mutex::new(e))
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

/// Wait up to `secs` for `cond`; past it, panic naming what `show` reads.
fn wait_showing(what: &str, secs: u64, mut cond: impl FnMut() -> bool, show: impl Fn() -> String) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}\nsaw: {}", show());
}

/// Log `call` from `county`.
fn party(eng: &Shared, call: &str, county: &str) {
    let rows = vec![vec![
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), county.to_string()),
    ]];
    let logged = engine_lock(eng)
        .contest_log_manual_rows(call, &rows, "CW", None)
        .expect("the party runs");
    assert!(logged.iter().all(|&ok| ok), "{call} refused: fixture bug");
}

/// The dupe keys of `eng`'s own contest log, as its snapshot carries them.
fn own_keys(eng: &Shared) -> Keys {
    engine_lock(eng)
        .snapshot()
        .field_day
        .expect("the party runs")
        .log
        .iter()
        .map(|q| q.dkey.clone())
        .collect()
}

/// The club's keys `eng`'s strip warns on: its snapshot's club block.
fn club_keys(eng: &Shared) -> Keys {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.dkeys.into_iter().collect())
        .unwrap_or_default()
}

/// The host's own lists, the ones every connection is sent.
fn host_keys(host: &Shared) -> Keys {
    engine_lock(host)
        .fd_club_log()
        .map(|c| c.dkeys().iter().cloned().collect())
        .unwrap_or_default()
}

/// The one key of `eng`'s own log whose call is `call`.
fn key_of(eng: &Shared, call: &str) -> Vec<String> {
    let keys: Vec<_> = own_keys(eng)
        .into_iter()
        .filter(|k| k.first().is_some_and(|c| c == call))
        .collect();
    assert_eq!(keys.len(), 1, "one {call} in that log: fixture bug");
    keys[0].clone()
}

fn union(sets: &[&Keys]) -> Keys {
    sets.iter().flat_map(|s| s.iter().cloned()).collect()
}

/// The host's club Cabrillo, as it would be sent in.
fn club_cabrillo(host: &Shared) -> String {
    engine_lock(host)
        .fd_club_export(true)
        .expect("the club file")
}

/// Every call this test works.
const WORKED: [&str; 5] = ["K9NEW", "K9OLD", "K9SHR", "W9CCC", "W9HHH"];

/// The worked calls on the club Cabrillo's QSO lines, one per line, sorted.
fn cabrillo_calls(host: &Shared) -> Vec<String> {
    let mut calls: Vec<String> = club_cabrillo(host)
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .flat_map(|l| l.split_whitespace().filter(|w| WORKED.contains(w)))
        .map(str::to_string)
        .collect();
    calls.sort();
    calls
}

/// Each position's QSO count on the host's board, by position id.
fn board_qsos(host: &Shared) -> BTreeSet<(String, u64)> {
    engine_lock(host)
        .fd_club_log()
        .map(|c| {
            c.positions()
                .iter()
                .map(|(id, p)| (id.clone(), p.qsos_raw))
                .collect()
        })
        .unwrap_or_default()
}

fn last_error(eng: &Shared) -> Option<String> {
    engine_lock(eng).fd_mirror_mut().last_error.clone()
}

/// The handles the host's own screen shows a Give button for.
fn give_handles(eng: &Shared) -> Vec<u64> {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.refused.iter().filter_map(|r| r.handle).collect())
        .unwrap_or_default()
}

/// Ctrl+D twice at `eng`: the removal's id, to restore it by.
fn take_back_newest(eng: &Shared) -> u64 {
    let fd = engine_lock(eng)
        .snapshot()
        .field_day
        .expect("the party runs");
    let q = fd.log.last().expect("a contact");
    let (call, when) = (q.call.clone(), q.when_unix);
    engine_lock(eng)
        .contest_remove_last(&call, when)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("{call} refused: {r:?}"))
        .entry
        .id()
}

fn restore(eng: &Shared, id: u64) {
    engine_lock(eng)
        .contest_restore(id)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("restore refused: {r:?}"));
}

/// ⭐ **A give takes the old laptop's contacts out of every laptop's club dupe warning at once,
/// and the new laptop's come in.** The CW tent's laptop works K9OLD (no other position has) and
/// K9SHR (the Phone tent has too); a second laptop comes to take the CW tent over, with K9NEW
/// logged, is turned away by name, and the host presses Give. Within a tick of the press the host's
/// own laptop and the Phone tent stop warning on K9OLD, and once the new laptop is in, every
/// laptop's club keys are the host's, the positions' own logs (the host's, the Phone tent's, the
/// new laptop's) less its own. CONTROLS: K9SHR stays the Phone tent's on every laptop; every
/// laptop's own log and own dupe keys are what they were, the old laptop's too; the club Cabrillo
/// and the board count the host's, the Phone tent's and the new laptop's contacts; the new laptop's
/// Ctrl+D still takes K9NEW out of every laptop's warning and the club file, and Restore brings it
/// back with the file the bytes it was.
#[test]
fn a_give_takes_the_old_laptops_contacts_out_of_every_laptops_club_dupe_warning() {
    let dir = std::env::temp_dir().join(format!("club-give-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = ilqp_engine("aaaa0001", "HQ", &addr, "aaaa0001");
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_give.jsonl"))
        .expect("the club starts");
    let mut sds = vec![start_host(&host, listener), start_pump(&host, &addr)];
    let phone = ilqp_engine("cccc0003", "Phone tent", &addr, "cccc0003");
    sds.push(start_pump(&phone, &addr));
    let old = ilqp_engine(
        "bbbb0002",
        "CW tent",
        &addr,
        "the laptop that held the CW tent",
    );
    sds.push(start_pump(&old, &addr));

    party(&host, "W9HHH", "LAKE");
    party(&phone, "K9SHR", "COOK");
    party(&phone, "W9CCC", "WILL");
    // Only once the Phone tent's K9SHR is in, so the club's key is the Phone tent's first.
    wait_showing(
        "three rows at the host",
        10,
        || engine_lock(&host).fd_club_log().unwrap().rows().len() == 3,
        || format!("{:?}", cabrillo_calls(&host)),
    );
    party(&old, "K9OLD", "JODA");
    party(&old, "K9SHR", "COOK");
    let (k9old, k9shr) = (key_of(&old, "K9OLD"), key_of(&phone, "K9SHR"));
    assert_eq!(k9shr, key_of(&old, "K9SHR"), "premise: one key, two tents");
    let (own_host, own_phone, own_old) = (own_keys(&host), own_keys(&phone), own_keys(&old));
    let before = union(&[&own_host, &own_phone, &own_old]);
    let show = || {
        format!(
            "host lists {:?}\nhost warns {:?}\nphone warns {:?}",
            host_keys(&host),
            club_keys(&host),
            club_keys(&phone)
        )
    };
    wait_showing(
        "every laptop warns on the club's keys",
        10,
        || {
            host_keys(&host) == before
                && [(&host, &own_host), (&phone, &own_phone), (&old, &own_old)]
                    .iter()
                    .all(|(eng, own)| club_keys(eng) == &before - *own)
        },
        show,
    );
    assert!(
        club_keys(&host).contains(&k9old) && club_keys(&phone).contains(&k9old),
        "premise: K9OLD is a club dupe on the host and the Phone tent"
    );

    let new = ilqp_engine("bbbb0002", "CW tent", &addr, "bbbb0002");
    party(&new, "K9NEW", "DUPG");
    let own_new = own_keys(&new);
    sds.push(start_pump(&new, &addr));
    let held = Some(tempo_app::fdevent::POSITION_HELD.to_string());
    wait_showing(
        "the new laptop turned away by name, with a Give button on the host",
        10,
        || last_error(&new) == held && give_handles(&host).len() == 1,
        || {
            format!(
                "told {:?}, buttons {:?}",
                last_error(&new),
                give_handles(&host)
            )
        },
    );

    let handle = give_handles(&host)[0];
    let press = Instant::now();
    engine_lock(&host)
        .fd_club_give_position(handle)
        .expect("the host gives the position");
    wait_showing(
        "the host and the Phone tent stop warning on K9OLD",
        10,
        || !club_keys(&host).contains(&k9old) && !club_keys(&phone).contains(&k9old),
        show,
    );
    let dropped_ms = press.elapsed().as_millis();
    let after = union(&[&own_host, &own_phone, &own_new]);
    wait_showing(
        "every laptop warns on the new holder's club",
        30,
        || {
            host_keys(&host) == after
                && [(&host, &own_host), (&phone, &own_phone), (&new, &own_new)]
                    .iter()
                    .all(|(eng, own)| club_keys(eng) == &after - *own)
        },
        || format!("{}\nnew warns {:?}", show(), club_keys(&new)),
    );
    println!("K9OLD left both laptops' warnings {dropped_ms} ms after the press");
    assert_eq!(last_error(&old), held, "the old laptop is turned away");
    for eng in [&host, &new] {
        assert!(
            club_keys(eng).contains(&k9shr),
            "K9SHR stays the Phone tent's"
        );
    }
    assert_eq!(own_keys(&host), own_host, "CONTROL: the host's own keys");
    assert_eq!(
        own_keys(&phone),
        own_phone,
        "CONTROL: the Phone tent's own keys"
    );
    assert_eq!(
        own_keys(&new),
        own_new,
        "CONTROL: the new laptop's own keys"
    );
    assert_eq!(
        own_keys(&old),
        own_old,
        "CONTROL: the old laptop keeps its log"
    );
    assert_eq!(
        cabrillo_calls(&host),
        ["K9NEW", "K9SHR", "W9CCC", "W9HHH"],
        "CONTROL: the club file"
    );
    let counts: BTreeSet<(String, u64)> = [("aaaa0001", 1), ("bbbb0002", 1), ("cccc0003", 2)]
        .iter()
        .map(|(id, n)| (id.to_string(), *n))
        .collect();
    assert_eq!(board_qsos(&host), counts, "CONTROL: the board's counts");

    let file = club_cabrillo(&host);
    let k9new = key_of(&new, "K9NEW");
    let id = take_back_newest(&new);
    wait_showing(
        "K9NEW leaves the club and every other laptop's warning",
        10,
        || {
            !host_keys(&host).contains(&k9new)
                && !club_keys(&host).contains(&k9new)
                && !club_keys(&phone).contains(&k9new)
        },
        show,
    );
    assert_eq!(cabrillo_calls(&host), ["K9SHR", "W9CCC", "W9HHH"]);
    restore(&new, id);
    wait_showing(
        "K9NEW back in the club and every other laptop's warning",
        10,
        || {
            host_keys(&host).contains(&k9new)
                && club_keys(&host).contains(&k9new)
                && club_keys(&phone).contains(&k9new)
        },
        show,
    );
    assert_eq!(
        club_cabrillo(&host),
        file,
        "the club file is the bytes it was"
    );

    for sd in &sds {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
