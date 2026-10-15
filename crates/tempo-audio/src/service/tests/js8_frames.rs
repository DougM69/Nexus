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
    /// with `text` queued and nothing sent yet, and Hold PTT between frames as it ships: off.
    fn new(speed: u8, text: &str) -> Js8Message {
        Js8Message::holding(speed, text, false)
    }

    /// …with Hold PTT between frames `hold`.
    fn holding(speed: u8, text: &str, hold: bool) -> Js8Message {
        let peer = retuning_peer(14_078_000, "PKTUSB", |line, state| {
            (line == "T 1").then(|| {
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let engine = Arc::new(Mutex::new(Engine::with_settings(
            tempo_app::settings::Settings {
                mycall: "KD9TAW".into(),
                mygrid: "EN52".into(),
                js8_hold_ptt: hold,
                ..Default::default()
            },
        )));
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
/// Fast 1.65 s (8.10 of 10), Turbo 1.70 s (4.05 of 6), JS8 60 1.12 s (2.63 of 4). JS8Call with Hold
/// PTT off unkeys between frames too: 200 ms after its `txDuration` (the same wave), and keys again
/// 200 ms (its TX delay) before the boundary.
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
            Js8Speed::Ultra => 1_122.0,
        };
        assert!(
            (gap - want_gap).abs() < 0.5,
            "{speed:?}: unkeyed {gap} ms between two frames"
        );
    }
}

/// A message long enough to take several frames at Normal.
const LONG: &str =
    "TEST MESSAGE WITH MULTIPLE FRAMES, LONG ENOUGH TO TAKE SEVERAL OVERS AT EVERY SPEED";

/// ⭐ HOLD PTT BETWEEN FRAMES ON: THE RADIO IS KEYED FROM THE FIRST FRAME TO THE END OF THE LAST. At
/// each speed, the first frame keys at its boundary and nothing unkeys until the last frame's own
/// end; each later frame's boundary asks for PTT again over the held key, as JS8Call asks again for
/// every frame (`prepareSending` → `emitPTT(true)`, mainwindow.cpp:2560-2572). Off, the scene above.
#[test]
fn with_hold_ptt_on_a_js8_message_stays_keyed_from_its_first_frame_to_the_end_of_its_last() {
    for speed in Js8Speed::ALL {
        let mut m = Js8Message::holding(speed.index(), "TEST MESSAGE WITH MULTIPLE FRAMES", true);
        assert!(
            m.frames >= 2,
            "{speed:?}: premise: a message of several frames"
        );
        let p = m.period_ms;
        let ptt = m.ptt_until(m.frames as f64 * p + p);
        let last = (m.frames - 1) as f64 * p;
        let mut want: Vec<(f64, String)> = (0..m.frames)
            .map(|k| (k as f64 * p, "T 1".to_string()))
            .collect();
        want.push((unkey_tick(speed, m.t0, last, p), "T 0".to_string()));
        assert_eq!(
            ptt, want,
            "{speed:?}: the PTT lines of a {}-frame message",
            m.frames
        );
    }
}

/// A held key ends at once when the operator stops between two frames: Stop TX (the hard stop's
/// abort) and TX Off (the TX-off cut: the hold is no over TX Off lets finish) both unkey on the next
/// tick, 14.02 s into the period, and nothing keys after. Held, the key would have run to the next
/// frame at 15 s.
#[test]
fn a_held_js8_key_drops_at_once_when_stop_tx_or_tx_off_comes_between_frames() {
    for stop in ["Stop TX", "TX Off"] {
        let mut m = Js8Message::holding(1, LONG, true);
        assert!(m.frames >= 3, "premise: a message of several frames");
        assert_eq!(
            m.ptt_until(14_000.0),
            [(0.0, "T 1".to_string())],
            "{stop}: premise: keyed, and still held past the frame's end at 13.40 s"
        );
        {
            let mut e = m.engine.lock().unwrap();
            match stop {
                "Stop TX" => e.halt_tx(),
                _ => e.set_tx_enabled(false),
            }
        }
        assert_eq!(
            m.ptt_until(3.0 * 15_000.0),
            [(14_020.0, "T 0".to_string())],
            "{stop}: unkeyed on the next tick, and nothing keyed after"
        );
    }
}

/// ⛔ A HOLD IS NEVER HELD AGAIN. When the boundary keys nothing while frames of the message are
/// still queued (here a clock repair holds transmit, which leaves the queue as it is), the held key
/// ends 250 ms after that boundary, on the tick at 15.26 s, and stays down: a hold that has run out
/// is not a slot over, so it is never held again whatever the engine says.
#[test]
fn a_held_js8_key_whose_next_frame_does_not_key_drops_just_after_the_boundary_and_stays_down() {
    let mut m = Js8Message::holding(1, LONG, true);
    assert!(m.frames >= 3, "premise: a message of several frames");
    assert_eq!(
        m.ptt_until(14_000.0),
        [(0.0, "T 1".to_string())],
        "premise: keyed, and still held past the frame's end at 13.40 s"
    );
    m.engine
        .lock()
        .unwrap()
        .hold_tx_for_clock_repair(std::time::Instant::now() + std::time::Duration::from_secs(600));
    assert_eq!(
        m.ptt_until(3.0 * 15_000.0),
        [(15_260.0, "T 0".to_string())],
        "unkeyed 250 ms after the boundary that keyed nothing, and nothing after"
    );
    assert_eq!(
        m.engine.lock().unwrap().js8_state().queue.len(),
        m.frames - 1,
        "premise: the rest of the message is still queued"
    );
}

/// The dial leaves the licence's privileges late in a frame, after the once-a-second JS8 tick that
/// would drop the rest of the message (`Engine::js8_tick`) and before the frame's end: the key is not
/// held past that end (13.40 s), since the next frame could not key there, and the next tick drops
/// the rest of the message. Earlier in the frame the tick drops it first, which ends a hold too.
#[test]
fn a_js8_key_is_not_held_once_the_dial_is_outside_the_privileges() {
    let mut m = Js8Message::holding(1, LONG, true);
    assert!(m.frames >= 3, "premise: a message of several frames");
    assert_eq!(m.ptt_until(13_100.0), [(0.0, "T 1".to_string())], "premise");
    {
        let mut e = m.engine.lock().unwrap();
        e.set_license_class("general");
        e.set_frequency(14.020, "20m", "USB");
        assert!(!e.tx_allowed(), "premise: 14.020 MHz is outside General");
        assert_eq!(
            e.js8_state().queue.len(),
            m.frames - 1,
            "premise: the rest of the message is still queued at the frame's end"
        );
    }
    assert_eq!(
        m.ptt_until(3.0 * 15_000.0),
        [(13_400.0, "T 0".to_string())],
        "unkeyed at the frame's own end, and nothing keyed after"
    );
}

/// Hold PTT holds within a message, never across two (JS8Call counts the frames of the one message
/// going out): two one-frame messages queued back to back (a CQ, then a query) key and unkey once
/// each.
#[test]
fn hold_ptt_never_holds_across_two_messages() {
    let mut m = Js8Message::holding(1, "CQ CQ CQ EN52", true);
    assert_eq!(m.frames, 1, "premise: a one-frame message");
    {
        let mut e = m.engine.lock().unwrap();
        e.js8_send(None, "W1AW SNR?".into()).unwrap();
        assert_eq!(
            e.js8_state().queue.len(),
            2,
            "premise: two one-frame messages"
        );
    }
    assert_eq!(
        m.ptt_until(2.0 * 15_000.0),
        [
            (0.0, "T 1".to_string()),
            (13_400.0, "T 0".to_string()),
            (15_000.0, "T 1".to_string()),
            (28_400.0, "T 0".to_string()),
        ]
    );
}
