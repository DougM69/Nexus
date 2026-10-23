//! One province sent in two spellings, at a club — the whole stack over 127.0.0.1: real engines
//! running the Illinois QSO Party, the REAL `fdbridge` impls, `fdsync::serve_until` and the real
//! position pumps. Loopback only, ephemeral ports.
//!
//! The sponsor's provinces are its records table's: Newfoundland is `NF` there and Prince Edward
//! Island `PEI`, and many loggers send Canada Post's `NL` and `PE`. Two laptops that worked each
//! province under both spellings claim two provinces on every club line and in the club file, not
//! four. Read by value off the host's club line, the other laptop's, and the club Cabrillo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;

/// 64 hex digits for `tag`, from a per-run seed: never a real key.
fn throwaway_hex(tag: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
        .collect()
}

fn club_key(posid: &str) -> fdsync::PositionKey {
    fdsync::PositionKey::new(throwaway_hex(posid))
}

/// An Illinois QSO Party position in McLean County on the club's call, joining `join_addr`.
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
    e.set_mode("fieldday-sp").expect("the party runs");
    e.set_fd_position_key(club_key(posid));
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

/// Wait up to `secs` for `cond`; whether it came. The caller asserts by value after.
fn settle(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// QSY the way a band pick does, then log one CW contact from `qth`, as the strip does.
fn party(eng: &Shared, band: &str, mhz: f64, call: &str, qth: &str) {
    let mut e = engine_lock(eng);
    e.set_frequency(mhz, band, "CW");
    let fields = [
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), qth.to_string()),
    ];
    assert!(
        e.contest_log_manual(call, &fields, "CW", None)
            .expect("the party runs"),
        "{call} refused: fixture bug"
    );
}

fn club_rows(eng: &Shared) -> u64 {
    engine_lock(eng)
        .fd_club_log()
        .map(|c| c.qsos_raw())
        .unwrap_or(0)
}

/// `(score, qsos)` on this laptop's club line.
fn club_line(eng: &Shared) -> (u32, u64) {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| (c.score, c.qsos))
        .unwrap_or((0, 0))
}

/// ⭐ **PE, PEI, NL and NF from four stations are two provinces on every club line and in the
/// club file.** Measured before the fold (the readiness audit's probe): the host's club line read
/// 32 = 8 points × 4.
#[test]
fn the_club_counts_one_province_once_whichever_laptop_sent_which_spelling() {
    let dir = std::env::temp_dir().join(format!("nexus-club-prov-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = party_engine("aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_event.ilqp.jsonl"))
        .unwrap();
    let mut stops = vec![start_host(&host, listener), start_pump(&host, &addr)];
    let b = party_engine("bbbb0002", "CW tent", &addr);
    stops.push(start_pump(&b, &addr));

    party(&host, "20m", 14.030, "VY2AB", "PE");
    party(&b, "40m", 7.030, "VY2CD", "PEI");
    party(&host, "20m", 14.031, "VO1AB", "NL");
    party(&b, "40m", 7.031, "VO1CD", "NF");
    // POSITIVE CONTROL: all four reach the host, so the line below scores the club's four.
    assert!(
        settle(15, || club_rows(&host) == 4),
        "four rows at the host"
    );
    // 4 CW contacts × 2 points × 2 provinces (Prince Edward Island, Newfoundland).
    settle(5, || club_line(&host) == (16, 4));
    assert_eq!(club_line(&host), (16, 4), "the host's club line");
    settle(15, || club_line(&b) == (16, 4));
    assert_eq!(
        club_line(&b),
        (16, 4),
        "the other laptop's club line, as the host sends it"
    );
    let cab = engine_lock(&host)
        .fd_club_export(true)
        .expect("the club Cabrillo");
    let claimed: Vec<&str> = cab
        .lines()
        .filter(|l| l.starts_with("CLAIMED-SCORE:"))
        .collect();
    assert_eq!(claimed, ["CLAIMED-SCORE: 16"]);
    // Each contact is written as its station sent it.
    let mut sent: Vec<&str> = cab
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .filter_map(|l| l.split_whitespace().last())
        .collect();
    sent.sort();
    assert_eq!(sent, ["NF", "NL", "PE", "PEI"]);

    // CONTROL: another province is a multiplier of its own, at the club too.
    party(&b, "40m", 7.032, "VE3AB", "ON");
    settle(15, || club_line(&host) == (30, 5));
    assert_eq!(
        club_line(&host),
        (30, 5),
        "5 contacts × 2 points × 3 provinces"
    );

    for sd in &stops {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
