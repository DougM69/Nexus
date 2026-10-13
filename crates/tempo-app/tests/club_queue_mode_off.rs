//! A laptop's contacts still on their way to the club when it leaves the contest — the whole
//! stack over 127.0.0.1: real engines running the Illinois QSO Party with their contest
//! journals attached as the shell attaches them, the REAL `fdbridge` impls,
//! `fdsync::serve_until` and the real position pumps. Loopback only, ephemeral ports.
//!
//! The end of a party: a laptop's link drops with contacts the host does not have yet, and the
//! laptop leaves the contest before the link comes back — its operator turns Contest mode off,
//! or opens a screen that leaves the contest's mode. Club sync still runs (the join address is
//! set), so those contacts must still go up when the link is back, and must still be counted
//! while it is not. Each case is read by value off the laptop's chip and the host's club log.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_app::fdevent::SyncState;
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

/// An Illinois QSO Party position in McLean County on the club's call, joining `join_addr`,
/// with its contest journal in `dir` (`set_fd_log_path`, as the shell sets it at startup).
fn party_engine(posid: &str, name: &str, join_addr: &str, dir: &std::path::Path) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    e.set_fd_log_path(dir.join(format!("fieldday_backup_{posid}.adi")));
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

fn listener() -> std::net::TcpListener {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap()
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

fn wait_until(what: &str, secs: u64, cond: impl FnMut() -> bool) {
    assert!(settle(secs, cond), "timed out waiting for: {what}");
}

/// Log one CW contact from `county` on 40 m, as the strip does.
fn party(eng: &Shared, mhz: f64, call: &str, county: &str) {
    let mut e = engine_lock(eng);
    e.set_frequency(mhz, "40m", "CW");
    let fields = [
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), county.to_string()),
    ];
    assert!(
        e.contest_log_manual(call, &fields, "CW", None)
            .expect("the party runs"),
        "{call} refused: fixture bug"
    );
}

/// The calls the host's club log holds from position `pos`, in the order they arrived.
fn host_calls_from(host: &Shared, pos: &str) -> Vec<String> {
    engine_lock(host)
        .fd_club_log()
        .map(|club| {
            club.rows()
                .iter()
                .filter(|r| r.posid == pos)
                .map(|r| r.call.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn state(eng: &Shared) -> SyncState {
    engine_lock(eng).fd_sync_state()
}

/// A host, and position B on its own laptop, both with their journals in `dir`.
struct Club {
    host: Shared,
    b: Shared,
    addr: String,
    stops: Vec<Arc<AtomicBool>>,
    dir: std::path::PathBuf,
}

impl Club {
    /// B logs one contact that reaches the host; then B's link goes down and B logs two more.
    fn with_two_queued_on_b(tag: &str) -> Club {
        let dir =
            std::env::temp_dir().join(format!("nexus-club-queue-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let l = listener();
        let addr = format!("127.0.0.1:{}", l.local_addr().unwrap().port());
        let host = party_engine("aaaa0001", "HQ", &addr, &dir);
        engine_lock(&host)
            .fd_host_start(dir.join("fd_event_event.ilqp.jsonl"))
            .unwrap();
        let stops = vec![start_host(&host, l), start_pump(&host, &addr)];
        let b = party_engine("bbbb0002", "CW tent", &addr, &dir);
        let b_link = start_pump(&b, &addr);
        party(&b, 7.030, "K9AAA", "COOK");
        // POSITIVE CONTROL: with Contest mode on, B's contact reaches the host — the harness
        // sees a row arrive, so the misses below are the engine's.
        wait_until("B's first contact at the host", 15, || {
            host_calls_from(&host, "bbbb0002") == ["K9AAA"]
        });
        wait_until("B synced", 15, || state(&b) == SyncState::Synced);
        b_link.store(true, Ordering::Relaxed);
        wait_until("B's link is down", 15, || {
            !engine_lock(&b).fd_mirror_mut().connected
        });
        party(&b, 7.031, "K9BBB", "KANE");
        party(&b, 7.032, "K9CCC", "LAKE");
        assert!(
            matches!(state(&b), SyncState::Offline { queued: 2, .. }),
            "premise: two contacts queued on B, got {:?}",
            state(&b)
        );
        Club {
            host,
            b,
            addr,
            stops,
            dir,
        }
    }

    /// B's link back: a new pump, as the shell's supervisor starts one.
    fn b_link_back(&mut self) {
        self.stops.push(start_pump(&self.b, &self.addr));
    }

    fn stop(self) {
        for sd in &self.stops {
            sd.store(true, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// ⭐ **Contest mode off on a laptop with contacts still queued: they are still counted, and
/// they go up when the link is back, with the mode still off.**
///
/// Measured before the fix (the readiness audit's probe): `Offline { queued: 2 }` became
/// `Offline { queued: 0 }` the moment the mode went off, and with the link back for four
/// seconds the host still held B's first contact only — the other two reached it only once the
/// mode was on again. A laptop packed up at 0100Z with its mode off never sent them.
#[test]
fn contest_mode_off_keeps_the_queued_contacts_going_to_the_club() {
    let mut club = Club::with_two_queued_on_b("off");
    let (host, b) = (club.host.clone(), club.b.clone());
    {
        let mut e = engine_lock(&b);
        let mut s = e.settings().clone();
        s.fd_active = false;
        e.apply_settings(s);
        // Premises: the mode is off, its screens are gone, and club sync still runs.
        assert!(!e.settings().fd_active);
        assert!(
            e.snapshot().field_day.is_none(),
            "the Contest block goes with the mode"
        );
        assert!(e.fd_sync_enabled(), "the join address is still set");
    }
    assert!(
        matches!(state(&b), SyncState::Offline { queued: 2, .. }),
        "the two contacts are still queued here with the mode off, got {:?}",
        state(&b)
    );
    // The link comes back, with B's mode still off.
    club.b_link_back();
    settle(15, || host_calls_from(&host, "bbbb0002").len() >= 3);
    assert_eq!(
        host_calls_from(&host, "bbbb0002"),
        ["K9AAA", "K9BBB", "K9CCC"],
        "every contact B logged, once each"
    );
    wait_until("B synced, mode off", 15, || state(&b) == SyncState::Synced);
    // And the mode back on (a restart or a Settings save does this): the session restored from
    // the journal is the one the host already holds, each contact once.
    {
        let mut e = engine_lock(&b);
        let mut s = e.settings().clone();
        s.fd_active = true;
        e.apply_settings(s);
        assert!(
            e.snapshot().field_day.is_some(),
            "premise: the party is back on B"
        );
    }
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        host_calls_from(&host, "bbbb0002"),
        ["K9AAA", "K9BBB", "K9CCC"],
        "each contact once, the mode back on"
    );
    assert_eq!(state(&b), SyncState::Synced);
    club.stop();
}

/// ⭐ **Leaving the contest's mode with Contest mode still on keeps them going too.** Opening
/// the Chat screen (`set_mode("chat")`, as the screen does) leaves the contest's mode while the
/// switch stays on; the contacts queued then go up when the link is back, and with the contest
/// entered again the host holds each once.
#[test]
fn leaving_the_contest_mode_for_chat_keeps_the_queued_contacts_going() {
    let mut club = Club::with_two_queued_on_b("chat");
    let (host, b) = (club.host.clone(), club.b.clone());
    {
        let mut e = engine_lock(&b);
        e.set_mode("chat").expect("Chat opens");
        assert!(e.settings().fd_active, "premise: Contest mode is still on");
        assert!(
            e.snapshot().field_day.is_none(),
            "premise: B is out of the contest's mode"
        );
    }
    assert!(
        matches!(state(&b), SyncState::Offline { queued: 2, .. }),
        "the two contacts are still queued here in Chat, got {:?}",
        state(&b)
    );
    club.b_link_back();
    settle(15, || host_calls_from(&host, "bbbb0002").len() >= 3);
    assert_eq!(
        host_calls_from(&host, "bbbb0002"),
        ["K9AAA", "K9BBB", "K9CCC"],
        "every contact B logged, once each"
    );
    wait_until("B synced, in Chat", 15, || state(&b) == SyncState::Synced);
    engine_lock(&b)
        .set_mode("fieldday-sp")
        .expect("back into the party");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        host_calls_from(&host, "bbbb0002"),
        ["K9AAA", "K9BBB", "K9CCC"],
        "each contact once, back in the party"
    );
    assert_eq!(state(&b), SyncState::Synced);
    club.stop();
}
