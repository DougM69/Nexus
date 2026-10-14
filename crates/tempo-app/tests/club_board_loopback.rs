//! ⭐ **Every laptop's multiplier board lights what ANY position of the club worked** — the
//! whole stack over 127.0.0.1, as `club_sync_party_loopback.rs` runs it: real engines running the
//! Illinois QSO Party, the REAL `fdbridge` impls, `fdsync::serve_until` and the real position
//! pumps. Loopback only, ephemeral ports, no real network.
//!
//! A county one laptop works lights on the host's county board and on every other laptop's, and
//! each board keeps this laptop's own contacts apart: `worked` is this laptop's log, `club` the
//! club's (the host's club log on the host, what the host sent on a position). Read from
//! `Engine::snapshot()`, the JSON every screen is sent, so nothing here leans on a type the
//! screens never see.

use serde_json::Value;
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

/// An Illinois QSO Party position at McLean county: master on, the party picked, S&P, a
/// position id and a name, pointed at the club.
fn party_engine(posid: &str, name: &str, join_addr: &str) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
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
    e.set_fd_position_key(fdsync::PositionKey::new(throwaway_hex(posid)));
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

/// Log one party contact the way the entry strip does.
fn log_party(eng: &Shared, call: &str, county: &str) {
    let fields = [
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), county.to_string()),
    ];
    assert!(
        engine_lock(eng)
            .contest_log_manual(call, &fields, "CW", None)
            .expect("the party is running"),
        "{call} from {county} refused by the position's own log — fixture bug"
    );
}

/// What this laptop's screens are sent about the contest: its snapshot's Field Day block.
fn fd_json(eng: &Shared) -> Value {
    serde_json::to_value(engine_lock(eng).snapshot().field_day).unwrap()
}

/// One list of the county board: `worked` (this laptop's log) or `club` (the club's), sorted.
fn county(eng: &Shared, list: &str) -> Vec<String> {
    let fd = fd_json(eng);
    let mut v: Vec<String> = fd["boards"]
        .as_array()
        .and_then(|bs| bs.iter().find(|b| b["id"] == "county"))
        .and_then(|b| b[list].as_array())
        .map(|vs| {
            vs.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn synced(eng: &Shared) -> bool {
    let fd = fd_json(eng);
    fd["club"]["syncState"] == "synced" && fd["club"]["queued"] == 0
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

/// ⭐ **The host's board lights a county a position worked; a position's board lights one the
/// host or another position worked; each marks its own apart** — and a county a position takes
/// back with Remove last goes dark everywhere it lit.
///
/// The screens poll the snapshot; the club's list reaches a position with the club line its host
/// sends the moment a contact brings a new key, well inside one heartbeat, so 8 s is a bound with
/// room on a loaded box, not the rhythm.
#[test]
fn every_laptops_county_board_lights_what_any_position_worked() {
    let dir = std::env::temp_dir().join(format!("club-board-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("club.jsonl"))
        .expect("host the party");
    let host_sd = start_host(&host, listener);
    let a = party_engine("bbbb0002", "Laptop A", &addr);
    let b = party_engine("cccc0003", "Laptop B", &addr);
    let pumps = [
        start_pump(&host, &addr),
        start_pump(&a, &addr),
        start_pump(&b, &addr),
    ];
    wait_until("three positions in, each synced", 20, || {
        synced(&host) && synced(&a) && synced(&b)
    });

    // Laptop A works Cook; the host works Will.
    log_party(&a, "K9AAA", "COOK");
    log_party(&host, "K9HHH", "WILL");
    wait_until("every board lights Cook and Will", 8, || {
        [&host, &a, &b]
            .iter()
            .all(|e| county(e, "club") == ["COOK", "WILL"])
    });
    // Each keeps its own apart: `worked` is this laptop's log and nothing else.
    assert_eq!(county(&host, "worked"), ["WILL"], "the host's own");
    assert_eq!(county(&a, "worked"), ["COOK"], "laptop A's own");
    assert!(
        county(&b, "worked").is_empty(),
        "laptop B worked nothing itself"
    );

    // A takes Cook back: it goes dark on every board, and Will stays.
    let fd = fd_json(&a);
    let newest = fd["log"].as_array().and_then(|l| l.last()).unwrap().clone();
    engine_lock(&a)
        .contest_remove_last(
            newest["call"].as_str().unwrap(),
            newest["whenUnix"].as_u64().unwrap(),
        )
        .expect("the party is running")
        .expect("the newest contact is the one named");
    wait_until("Cook goes dark on every board", 8, || {
        [&host, &a, &b]
            .iter()
            .all(|e| county(e, "club") == ["WILL"])
    });
    assert!(county(&a, "worked").is_empty(), "A's own log let Cook go");

    for sd in pumps.iter().chain([&host_sd]) {
        sd.store(true, Ordering::Relaxed);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A laptop joined to a host OLDER than the club's list shows its own board, as it always
/// did.** An older host's club line carries no multiplier values: the line below is the shape a
/// host built before them writes, byte for byte (`fdsync`'s `old_lines`), so this laptop's board
/// lights only what this laptop worked. CONTROL: the same line with the list on it, as a host that
/// sends one writes it, lights the club's.
#[test]
fn a_laptop_joined_to_an_older_host_shows_its_own_board() {
    let a = party_engine("bbbb0002", "Laptop A", "127.0.0.1:9");
    log_party(&a, "K9AAA", "COOK");
    let older = r#"{"t":"snap","reset":true,"dupes":[],"dkeys":[["K9AAA","20M","CW","COOK","MCLN"],["K9HHH","20M","CW","WILL","MCLN"]],"sections":[],"score":8,"qsos":2,"board":[]}"#;
    let Some(fdsync::Msg::Snap(st)) = fdsync::decode_line(older) else {
        panic!("an older host's snap decodes");
    };
    engine_lock(&a).fd_mirror_mut().apply(&st);
    assert_eq!(county(&a, "worked"), ["COOK"]);
    assert!(
        county(&a, "club").is_empty(),
        "an older host sent no club list, so the board is this laptop's own"
    );
    // CONTROL: the list on the same line lights the club's.
    let newer = older.replace(
        r#""sections":[],"#,
        r#""sections":[],"mults":[["QTH","COOK"],["QTH","WILL"]],"#,
    );
    let Some(fdsync::Msg::Snap(st)) = fdsync::decode_line(&newer) else {
        panic!("a newer host's snap decodes");
    };
    engine_lock(&a).fd_mirror_mut().apply(&st);
    assert_eq!(county(&a, "club"), ["COOK", "WILL"]);
    assert_eq!(county(&a, "worked"), ["COOK"], "its own stays its own");
}
