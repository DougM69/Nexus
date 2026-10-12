//! ⭐ **How fast the club sync shows each thing, measured** — the whole stack over 127.0.0.1, as
//! `club_sync_party_loopback.rs` runs it: real engines running the Illinois QSO Party, the REAL
//! `fdbridge` impls, `fdsync::serve_until` and the real position pumps. Loopback only, ephemeral
//! ports, no real network.
//!
//! A host (itself a position, over its own loopback listener) and two laptops, A and B. Each
//! round: A logs a contact, moves band and takes the contact back with Remove last; the host logs
//! one of its own; B logs a contact the host already worked (a club dupe). At the end the host
//! removes A. For each, the time from the act to the moment the HOST's snapshot and ANOTHER
//! laptop's snapshot show it — read from `Engine::snapshot()`, what every screen polls — printed
//! as one table.
//!
//! A measurement, not a gate: `#[ignore]`d, because a time taken on a loaded test box is not a
//! verdict on the code. Run it with
//! `cargo test -p tempo-app --test club_sync_latency -- --ignored --nocapture`.
//! The heartbeat (`fdsync::PING_SECS`, 5 s) is the rhythm: what rides it alone can take up to 5 s.

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

/// The Contest screen's Remove last, on this laptop's newest contact.
fn remove_last(eng: &Shared) {
    let fd = fd_json(eng);
    let newest = fd["log"]
        .as_array()
        .and_then(|l| l.last())
        .expect("a contact to remove")
        .clone();
    let call = newest["call"].as_str().unwrap().to_string();
    let when = newest["whenUnix"].as_u64().unwrap();
    engine_lock(eng)
        .contest_remove_last(&call, when)
        .expect("the party is running")
        .expect("the newest contact is the one named");
}

/// What this laptop's screens are sent about the contest: its snapshot's Field Day block.
fn fd_json(eng: &Shared) -> Value {
    serde_json::to_value(engine_lock(eng).snapshot().field_day).unwrap()
}

/// One reading of every screen at once.
struct View {
    h: Value,
    a: Value,
    b: Value,
    /// The host's club log, by `(position, seq)`, its taken-back rows left out.
    host_rows: Vec<(String, u64)>,
}

fn view(h: &Shared, a: &Shared, b: &Shared) -> View {
    let host_rows = engine_lock(h)
        .fd_club_log()
        .map(|c| c.rows().iter().map(|r| (r.posid.clone(), r.seq)).collect())
        .unwrap_or_default();
    View {
        h: fd_json(h),
        a: fd_json(a),
        b: fd_json(b),
        host_rows,
    }
}

fn synced(fd: &Value) -> bool {
    fd["club"]["syncState"] == "synced" && fd["club"]["queued"] == 0
}

fn club_qsos(fd: &Value) -> u64 {
    fd["club"]["qsos"].as_u64().unwrap_or(0)
}

fn board_row<'a>(fd: &'a Value, pos: &str) -> Option<&'a Value> {
    fd["club"]["board"]
        .as_array()?
        .iter()
        .find(|r| r["posid"] == pos)
}

fn row_qsos(fd: &Value, pos: &str) -> u64 {
    board_row(fd, pos)
        .and_then(|r| r["qsos"].as_u64())
        .unwrap_or(0)
}

fn row_band(fd: &Value, pos: &str) -> String {
    board_row(fd, pos)
        .and_then(|r| r["band"].as_str())
        .unwrap_or("")
        .to_string()
}

/// The while-typing verdict's club keys (`club.dkeys`) hold one for `call`.
fn club_key_for(fd: &Value, call: &str) -> bool {
    fd["club"]["dkeys"]
        .as_array()
        .is_some_and(|ks| ks.iter().any(|k| k[0] == call))
}

/// The county board lights `county`, whoever worked it: this laptop's own contacts
/// (`worked`) or, where the build sends one, the club's (`club`).
fn county_lit(fd: &Value, county: &str) -> bool {
    fd["boards"].as_array().is_some_and(|bs| {
        bs.iter().filter(|b| b["id"] == "county").any(|b| {
            ["worked", "club"].iter().any(|list| {
                b[list]
                    .as_array()
                    .is_some_and(|vs| vs.iter().any(|v| v == county))
            })
        })
    })
}

type Check = Box<dyn Fn(&View) -> bool>;

/// One thing to time: what it is, the screen it shows on, and when it does.
struct Probe {
    what: String,
    on: &'static str,
    check: Check,
}

fn probe(what: &str, on: &'static str, check: impl Fn(&View) -> bool + 'static) -> Probe {
    Probe {
        what: what.to_string(),
        on,
        check: Box::new(check),
    }
}

/// Every probe's time from `t0` to the first reading that shows it, `None` past `limit`.
fn measure(
    (h, a, b): (&Shared, &Shared, &Shared),
    t0: Instant,
    limit: Duration,
    probes: Vec<Probe>,
) -> Vec<(String, &'static str, Option<u128>)> {
    let mut seen: Vec<Option<u128>> = vec![None; probes.len()];
    loop {
        let v = view(h, a, b);
        let at = t0.elapsed();
        for (i, p) in probes.iter().enumerate() {
            if seen[i].is_none() && (p.check)(&v) {
                seen[i] = Some(at.as_millis());
            }
        }
        if seen.iter().all(Option::is_some) || at >= limit {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    probes
        .into_iter()
        .zip(seen)
        .map(|(p, s)| (p.what, p.on, s))
        .collect()
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

/// The table: every probe's readings over the rounds, in the order first measured.
#[derive(Default)]
struct Table {
    rows: Vec<(String, &'static str, Vec<Option<u128>>)>,
}

impl Table {
    fn add(&mut self, results: Vec<(String, &'static str, Option<u128>)>) {
        for (what, on, ms) in results {
            match self.rows.iter_mut().find(|r| r.0 == what && r.1 == on) {
                Some(r) => r.2.push(ms),
                None => self.rows.push((what, on, vec![ms])),
            }
        }
    }

    fn print(&self, limit: Duration) {
        eprintln!("\n| What | Shows on | Fastest | Median | Slowest | Rounds |");
        eprintln!("|---|---|---|---|---|---|");
        for (what, on, ms) in &self.rows {
            let mut got: Vec<u128> = ms.iter().flatten().copied().collect();
            got.sort_unstable();
            let missed = ms.len() - got.len();
            let cell = |v: Option<&u128>| match v {
                Some(ms) => format!("{:.2} s", *ms as f64 / 1000.0),
                None => format!("never (> {} s)", limit.as_secs()),
            };
            let (fast, med, slow) = if got.is_empty() {
                (cell(None), cell(None), cell(None))
            } else {
                (
                    cell(got.first()),
                    cell(got.get(got.len() / 2)),
                    if missed > 0 {
                        cell(None)
                    } else {
                        cell(got.last())
                    },
                )
            };
            eprintln!(
                "| {what} | {on} | {fast} | {med} | {slow} | {}{} |",
                ms.len(),
                if missed > 0 {
                    format!(", {missed} never")
                } else {
                    String::new()
                }
            );
        }
    }
}

const POS_A: &str = "bbbb0002";
const POS_B: &str = "cccc0003";
const POS_H: &str = "aaaa0001";

#[test]
#[ignore = "a measurement, printed for the record: run it with --ignored --nocapture"]
fn club_sync_latency_table() {
    const ROUNDS: usize = 3;
    let limit = Duration::from_secs(12);
    let dir = std::env::temp_dir().join(format!("club-latency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let h = party_engine(POS_H, "HQ", &addr);
    engine_lock(&h)
        .fd_host_start(dir.join("club.jsonl"))
        .expect("host the party");
    let host_sd = start_host(&h, listener);
    let a = party_engine(POS_A, "Laptop A", &addr);
    let b = party_engine(POS_B, "Laptop B", &addr);
    let pumps = [
        start_pump(&h, &addr),
        start_pump(&a, &addr),
        start_pump(&b, &addr),
    ];
    wait_until("three positions in, each one synced", 20, || {
        let v = view(&h, &a, &b);
        [&v.h, &v.a, &v.b].iter().all(|fd| synced(fd))
            && [POS_H, POS_A, POS_B]
                .iter()
                .all(|p| board_row(&v.b, p).is_some() && board_row(&v.h, p).is_some())
    });

    let engines = (&h, &a, &b);
    let mut table = Table::default();
    let counties_a = ["ADAM", "ALEX", "BOND", "BOON", "BROW"];
    let counties_h = ["BURO", "CALH", "CARR", "CASS", "CHAM"];
    for r in 0..ROUNDS {
        // ---- A logs a contact nobody has worked, from a county nobody has worked.
        let (call, county) = (format!("K9A{r}A"), counties_a[r]);
        let before = view(&h, &a, &b);
        let (qsos_h, qsos_b) = (club_qsos(&before.h), club_qsos(&before.b));
        let (row_h, row_b) = (row_qsos(&before.h, POS_A), row_qsos(&before.b, POS_A));
        let rows_before = before.host_rows.len();
        let t0 = Instant::now();
        log_party(&a, &call, county);
        let (c1, c2) = (call.clone(), call.clone());
        table.add(measure(
            engines,
            t0,
            limit,
            vec![
                probe("A logs a contact: Synced on A", "laptop A", |v| {
                    synced(&v.a)
                }),
                probe("A logs a contact: in the club log", "host", move |v| {
                    v.host_rows.len() > rows_before
                }),
                probe("A logs a contact: board QSOS", "host", move |v| {
                    row_qsos(&v.h, POS_A) > row_h
                }),
                probe("A logs a contact: board QSOS", "laptop B", move |v| {
                    row_qsos(&v.b, POS_A) > row_b
                }),
                probe("A logs a contact: club counters", "host", move |v| {
                    club_qsos(&v.h) > qsos_h
                }),
                probe("A logs a contact: club counters", "laptop B", move |v| {
                    club_qsos(&v.b) > qsos_b
                }),
                probe("A logs a contact: dupe warning", "host", move |v| {
                    club_key_for(&v.h, &c1)
                }),
                probe("A logs a contact: dupe warning", "laptop B", move |v| {
                    club_key_for(&v.b, &c2)
                }),
                probe("A logs a contact: county board", "host", move |v| {
                    county_lit(&v.h, county)
                }),
                probe("A logs a contact: county board", "laptop B", move |v| {
                    county_lit(&v.b, county)
                }),
            ],
        ));
        let after = view(&h, &a, &b);
        let (lit_h, lit_b) = (county_lit(&after.h, county), county_lit(&after.b, county));

        // ---- A moves band.
        let band = if r % 2 == 0 { "40m" } else { "20m" };
        let dial = if band == "40m" { 7.030 } else { 14.030 };
        let t0 = Instant::now();
        engine_lock(&a).set_frequency(dial, band, "CW");
        table.add(measure(
            engines,
            t0,
            limit,
            vec![
                probe("A moves band: board band", "host", move |v| {
                    row_band(&v.h, POS_A) == band
                }),
                probe("A moves band: board band", "laptop B", move |v| {
                    row_band(&v.b, POS_A) == band
                }),
            ],
        ));

        // ---- A takes its contact back: Remove last.
        let before = view(&h, &a, &b);
        let (qsos_h, qsos_b) = (club_qsos(&before.h), club_qsos(&before.b));
        let (row_h, row_b) = (row_qsos(&before.h, POS_A), row_qsos(&before.b, POS_A));
        let t0 = Instant::now();
        remove_last(&a);
        let (c1, c2) = (call.clone(), call.clone());
        let mut probes = vec![
            probe("A's Remove last: Synced on A", "laptop A", |v| synced(&v.a)),
            probe("A's Remove last: board QSOS", "host", move |v| {
                row_qsos(&v.h, POS_A) < row_h
            }),
            probe("A's Remove last: board QSOS", "laptop B", move |v| {
                row_qsos(&v.b, POS_A) < row_b
            }),
            probe("A's Remove last: club counters", "host", move |v| {
                club_qsos(&v.h) < qsos_h
            }),
            probe("A's Remove last: club counters", "laptop B", move |v| {
                club_qsos(&v.b) < qsos_b
            }),
            probe("A's Remove last: dupe warning gone", "host", move |v| {
                !club_key_for(&v.h, &c1)
            }),
            probe("A's Remove last: dupe warning gone", "laptop B", move |v| {
                !club_key_for(&v.b, &c2)
            }),
        ];
        // A county that never lit has nothing to go dark, so it is timed only where it lit.
        if lit_h {
            probes.push(probe("A's Remove last: county board", "host", move |v| {
                !county_lit(&v.h, county)
            }));
        }
        if lit_b {
            probes.push(probe(
                "A's Remove last: county board",
                "laptop B",
                move |v| !county_lit(&v.b, county),
            ));
        }
        let mut got = measure(engines, t0, limit, probes);
        if !lit_h {
            got.push(("A's Remove last: county board".into(), "host", None));
        }
        if !lit_b {
            got.push(("A's Remove last: county board".into(), "laptop B", None));
        }
        table.add(got);

        // ---- The host logs one of its own.
        let (call, county) = (format!("K9H{r}H"), counties_h[r]);
        let before = view(&h, &a, &b);
        let qsos_b = club_qsos(&before.b);
        let row_b = row_qsos(&before.b, POS_H);
        let t0 = Instant::now();
        log_party(&h, &call, county);
        let c1 = call.clone();
        table.add(measure(
            engines,
            t0,
            limit,
            vec![
                probe("The host logs a contact: Synced on the host", "host", |v| {
                    synced(&v.h)
                }),
                probe(
                    "The host logs a contact: board QSOS",
                    "laptop B",
                    move |v| row_qsos(&v.b, POS_H) > row_b,
                ),
                probe(
                    "The host logs a contact: club counters",
                    "laptop B",
                    move |v| club_qsos(&v.b) > qsos_b,
                ),
                probe(
                    "The host logs a contact: dupe warning",
                    "laptop B",
                    move |v| club_key_for(&v.b, &c1),
                ),
                probe(
                    "The host logs a contact: county board",
                    "laptop B",
                    move |v| county_lit(&v.b, county),
                ),
            ],
        ));

        // ---- B works the host's station again on the same band and mode: a club dupe, which
        // the club keeps and counts in its raw total, and which adds no key.
        wait_until("B has the host's key", 10, || {
            club_key_for(&fd_json(&b), &call)
        });
        let before = view(&h, &a, &b);
        let (qsos_h, qsos_a) = (club_qsos(&before.h), club_qsos(&before.a));
        let (row_h, row_a) = (row_qsos(&before.h, POS_B), row_qsos(&before.a, POS_B));
        let t0 = Instant::now();
        log_party(&b, &call, county);
        table.add(measure(
            engines,
            t0,
            limit,
            vec![
                probe("B logs a club dupe: Synced on B", "laptop B", |v| {
                    synced(&v.b)
                }),
                probe("B logs a club dupe: board QSOS", "host", move |v| {
                    row_qsos(&v.h, POS_B) > row_h
                }),
                probe("B logs a club dupe: board QSOS", "laptop A", move |v| {
                    row_qsos(&v.a, POS_B) > row_a
                }),
                probe("B logs a club dupe: club counters", "host", move |v| {
                    club_qsos(&v.h) > qsos_h
                }),
                probe("B logs a club dupe: club counters", "laptop A", move |v| {
                    club_qsos(&v.a) > qsos_a
                }),
            ],
        ));
    }

    // ---- The host's operator removes laptop A from the event.
    let limit_removed = Duration::from_secs(30);
    let t0 = Instant::now();
    engine_lock(&h)
        .fd_club_remove_position(POS_A)
        .expect("the host removes A");
    table.add(measure(
        engines,
        t0,
        limit_removed,
        vec![
            probe("The host removes A: its row says Removed", "host", |v| {
                board_row(&v.h, POS_A).is_some_and(|r| r["link"] == "removed")
            }),
            probe("The host removes A: A is told", "laptop A", |v| {
                v.a["club"]["lastError"].is_string()
            }),
            // A position's board says Removed where its host's line flags the position, and
            // otherwise marks the line ⚠ once the host has not heard the position for longer
            // than the link's dead timeout (the UI's `notHeard`), or it leaves the board.
            probe(
                "The host removes A: its row marked or gone",
                "laptop B",
                |v| {
                    board_row(&v.b, POS_A).is_none_or(|r| {
                        r["link"] == "removed"
                            || (r["link"].is_null()
                                && r["lastSeenSecs"].as_u64().unwrap_or(0) > fdsync::DEAD_SECS)
                    })
                },
            ),
        ],
    ));

    table.print(limit);
    eprintln!(
        "(removal step timed out at {} s; heartbeat {} s)",
        limit_removed.as_secs(),
        fdsync::PING_SECS
    );
    for sd in pumps.iter().chain([&host_sd]) {
        sd.store(true, Ordering::Relaxed);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
