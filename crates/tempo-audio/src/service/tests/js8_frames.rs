//! A JS8 message of several frames, on the real loop: what PTT does between its frames.
//!
//! JS8 keys every period, one frame of the message each (`Engine::plan_js8_tx`), and each frame is
//! an over of its own: keyed at the boundary, held for its wave (the speed's start delay of silence
//! and 79 symbols) and `slot::TX_TAIL_MS`, then dropped (`slot::tx_deadline_ms`), so the radio
//! unkeys between two frames and keys again for the next. JS8Call 3.0.3 does the same unless "Hold
//! PTT between frames while there are more to be transmitted" is on (`HoldPTT`, off by default,
//! `JS8_UI/Configuration.cpp:2404`): its `stopTx2` then leaves PTT up while frames of the message
//! remain (`JS8_UI/mainwindow.cpp:2919-2932`).
//!
//! The loop is the real `RadioLoop::step`, a 20 ms tick at a time, on a rigctld that keys and a mock
//! sound card. Every scene runs at a synthetic time two minutes or more ahead of the real clock: the
//! key re-reads the clock and keeps the later of the two (`slot::slot_tx_phase`), so each key lands on
//! the tick the scene steps it at, as in `ft_window`.

use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};

/// A JS8 station with a message queued, and the loop that sends it.
struct Js8Message {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    /// The loop's clock, starting on the boundary the first frame keys at (the loop's first tick
    /// is a boundary of its own, so the scene starts there rather than before it).
    t: f64,
    /// That boundary.
    t0: f64,
    period_ms: f64,
    /// How many frames the message is.
    frames: usize,
}

impl Js8Message {
    /// A station on the 20 m JS8 watering hole at `speed` (an index into `Js8Speed::ALL`), TX on,
    /// with `text` queued and nothing sent yet.
    fn new(speed: u8, text: &str) -> Js8Message {
        let peer = retuning_peer(14_078_000, "PKTUSB", |line, state| {
            (line == "T 1").then(|| {
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        let (period_ms, frames) = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.js8_set_speed(speed).unwrap();
            e.js8_enter();
            e.set_frequency(14.078, "20m", "USB");
            e.set_tx_enabled(true);
            e.js8_send(None, text.into()).unwrap();
            (e.active_slot_secs() * 1000.0, e.js8_state().queue.len())
        };
        let t0 = ((now_unix_ms() + 120_000.0) / period_ms).ceil() * period_ms;
        Js8Message {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            t: t0,
            t0,
            period_ms,
            frames,
        }
    }

    /// Step the loop a tick at a time to `until` (ms from the first frame's boundary), and return
    /// each PTT line the steps put on the wire with the time of the tick that sent it.
    fn ptt_until(&mut self, until: f64) -> Vec<(f64, String)> {
        let mut seen = self.ptt_lines().len();
        let mut out = Vec::new();
        while self.t <= self.t0 + until {
            let at = self.t - self.t0;
            let tick = self.t;
            step_to(
                &self.engine,
                &mut self.state,
                &mut self.backend,
                &mut self.rig,
                &mut self.t,
                tick,
            );
            let lines = self.ptt_lines();
            out.extend(lines[seen..].iter().map(|l| (at, l.clone())));
            seen = lines.len();
        }
        out
    }

    fn ptt_lines(&self) -> Vec<String> {
        writes(&self.peer)
            .into_iter()
            .filter(|l| l.starts_with("T "))
            .collect()
    }
}

/// A JS8 frame's wave in ms: the speed's start delay of silence and 79 symbols
/// (`js8::phy::modulate`).
fn wave_ms(speed: Js8Speed) -> f64 {
    f64::from(speed.delay_ms()) + 79.0 * speed.nsps() as f64 / 12.0
}

/// The tick (ms from `t0`) the loop drops PTT on for a frame keyed `key` ms after `t0`: its first
/// 20 ms tick at or after the over's deadline, the key plus the wave (summed as the loop sums it,
/// in f32 seconds) and the 250 ms tail (`slot::play_keyed_over`, `slot::tx_deadline_ms`). Turbo's
/// wave sums to 4.05000019 s, so its 4.30 s deadline drops on the tick at 4.32 s.
fn unkey_tick(speed: Js8Speed, t0: f64, key: f64, period_ms: f64) -> f64 {
    let samples = speed.delay_ms() as usize * 12 + 79 * speed.nsps();
    let audio_ms = (samples as f32 / tempo_core::tempo_fast::SAMPLE_RATE) as f64 * 1000.0;
    let deadline = crate::slot::tx_deadline_ms(t0 + key, audio_ms, period_ms);
    let mut tick = t0 + key;
    while tick < deadline {
        tick += 20.0;
    }
    tick - t0
}

/// ⭐ TODAY, MEASURED: PTT DROPS AFTER EVERY FRAME AND KEYS AGAIN AT THE NEXT BOUNDARY. A message of
/// several frames at each speed, on the real loop: each frame keys at its boundary and unkeys when its
/// wave and the 250 ms tail have gone (`slot::tx_deadline_ms`), so the radio is unkeyed between two
/// frames for the rest of the period: Slow 3.97 s (wave 25.78 s of 30), Normal 1.61 s (13.14 of 15),
/// Fast 1.65 s (8.10 of 10), Turbo 1.70 s (4.05 of 6). JS8Call with Hold PTT off unkeys between frames
/// too: 200 ms after its `txDuration` (the same wave), and keys again 200 ms (its TX delay) before the
/// boundary.
#[test]
fn a_js8_message_unkeys_between_its_frames_and_keys_again_at_each_boundary() {
    for speed in Js8Speed::ALL {
        let mut m = Js8Message::new(speed.index(), "TEST MESSAGE WITH MULTIPLE FRAMES");
        assert!(
            m.frames >= 2,
            "{speed:?}: premise: a message of several frames"
        );
        let p = m.period_ms;
        assert_eq!(
            p,
            f64::from(speed.period_s()) * 1000.0,
            "{speed:?}: premise"
        );
        let ptt = m.ptt_until(m.frames as f64 * p + p);
        let want: Vec<(f64, String)> = (0..m.frames)
            .flat_map(|k| {
                let at = k as f64 * p;
                [
                    (at, "T 1".to_string()),
                    (unkey_tick(speed, m.t0, at, p), "T 0".to_string()),
                ]
            })
            .collect();
        assert_eq!(
            ptt, want,
            "{speed:?}: the PTT lines of a {}-frame message",
            m.frames
        );
        let gap = p - (wave_ms(speed) + crate::slot::TX_TAIL_MS);
        let want_gap = match speed {
            Js8Speed::Slow => 3_970.0,
            Js8Speed::Normal => 1_610.0,
            Js8Speed::Fast => 1_650.0,
            Js8Speed::Turbo => 1_700.0,
        };
        assert!(
            (gap - want_gap).abs() < 0.5,
            "{speed:?}: unkeyed {gap} ms between two frames"
        );
    }
}
