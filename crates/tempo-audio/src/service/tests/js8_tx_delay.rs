//! JS8's TX delay on the real loop: a frame keys JS8Call's `TxDelay` before its period, its first
//! tone at the period's start delay; the RX period before it ends at the key; Stop TX and TX Off end
//! a frame still in its lead at once; Hold PTT between frames keys the next frame on its boundary
//! with no lead; FT8 keys where it always did.
//!
//! JS8Call 3.0.3 asks for PTT on the GUI tick that finds the period `TxDelay` (0.2 s by default)
//! from its end (`prepareSending`, `JS8_UI/mainwindow.cpp:2507-2572`), and its Modulator sends
//! silence up to the period and the speed's start delay (`JS8_Mode/Modulator.cpp:86-94`).
//!
//! The loop is the real `RadioLoop::step`, a 20 ms tick at a time, on a rigctld that keys and the
//! mock sound card. Every scene runs two minutes or more ahead of the real clock, so each key lands
//! on the tick the scene steps it at (the key re-reads the clock and keeps the later of the two,
//! `slot::slot_tx_phase`), and starts a second before the first frame's TX delay: the loop's first
//! tick takes the boundary of the period before, far past its late start, so nothing keys there.

use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};

/// A JS8 station with a message queued, and the loop that sends it.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    t: f64,
    /// The boundary of the first frame's period.
    t0: f64,
    period_ms: f64,
}

/// A message long enough to take several frames at every speed.
const LONG: &str =
    "TEST MESSAGE WITH MULTIPLE FRAMES, LONG ENOUGH TO TAKE SEVERAL OVERS AT EVERY SPEED";

impl Scene {
    /// A station on the 20 m JS8 watering hole at `speed` (an index into `Js8Speed::ALL`), TX on,
    /// its TX delay `delay_ms`, Hold PTT between frames `hold`, and `text` queued unless empty.
    fn new(speed: u8, delay_ms: u32, hold: bool, text: &str) -> Scene {
        Scene::with(
            speed,
            tempo_app::settings::Settings {
                js8_tx_delay_ms: delay_ms,
                js8_hold_ptt: hold,
                ..Default::default()
            },
            text,
        )
    }

    /// …with `settings` (the callsign, locator and heartbeat networking set here).
    fn with(speed: u8, settings: tempo_app::settings::Settings, text: &str) -> Scene {
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
                js8_hb_networking: true,
                ..settings
            },
        )));
        let period_ms = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.js8_set_speed(speed).unwrap();
            e.js8_enter();
            e.set_frequency(14.078, "20m", "USB");
            e.set_tx_enabled(true);
            if !text.is_empty() {
                e.js8_send(None, text.into()).unwrap();
            }
            e.active_slot_secs() * 1000.0
        };
        let t0 = ((now_unix_ms() + 120_000.0) / period_ms).ceil() * period_ms;
        Scene {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            t: t0 - 1_000.0,
            t0,
            period_ms,
        }
    }

    /// Step the loop a tick at a time to `until` (ms from `t0`), and return each PTT line the steps
    /// put on the wire with the time of the tick that sent it (ms from `t0`).
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

    /// How many frames are still queued.
    fn queued(&self) -> usize {
        self.engine.lock().unwrap().js8_state().queue.len()
    }
}

/// A frame's wave at `speed`, in ms: the start delay's silence and 79 symbols.
fn wave_ms(speed: Js8Speed) -> f64 {
    f64::from(speed.delay_ms()) + 79.0 * speed.nsps() as f64 / 12.0
}

/// The first 20 ms tick of a scene (its ticks are `t0 - 1000 + 20k`) at or after `ms` from `t0`.
fn tick_at_or_after(ms: f64) -> f64 {
    (ms / 20.0).ceil() * 20.0
}

/// ⭐ PTT KEYS THE TX DELAY BEFORE EACH FRAME'S PERIOD, AT EVERY SPEED. A message of several frames:
/// each frame keys 200 ms (JS8Call's default TX delay) before its period's boundary, and unkeys at
/// its own end on the slot clock, the 250 ms tail after its wave; at 0.5 s of TX delay, 500 ms
/// before. Nexus keyed on the boundary's tick, and every receiver read the key's latency as DT.
#[test]
fn ptt_keys_the_tx_delay_before_each_frames_period_at_every_speed() {
    for (delay, lead) in [(200, 200.0), (500, 500.0)] {
        for speed in Js8Speed::ALL {
            let mut s = Scene::new(speed.index(), delay, false, LONG);
            let frames = s.queued();
            assert!(
                frames >= 2,
                "{speed:?}: premise: a message of several frames"
            );
            let p = s.period_ms;
            let ptt = s.ptt_until(frames as f64 * p + p);
            let want: Vec<(f64, String)> = (0..frames)
                .flat_map(|k| {
                    let start = k as f64 * p;
                    [
                        (start - lead, "T 1".to_string()),
                        (
                            tick_at_or_after(start + wave_ms(speed) + crate::slot::TX_TAIL_MS),
                            "T 0".to_string(),
                        ),
                    ]
                })
                .collect();
            assert_eq!(ptt, want, "{speed:?}, TX delay {delay} ms: the PTT lines");
        }
    }
}

/// ⭐ THE FIRST TONE AT THE PERIOD'S START AND THE START DELAY. The frame keyed 200 ms before its
/// period is handed to the card with 200 ms of silence in front of its wave, so its first tone
/// plays 700 ms after the key: at the period's start and 500 ms, as on JS8Call's wall clock. What
/// follows is the whole frame, sample for sample.
#[test]
fn a_frame_keyed_in_its_tx_delay_plays_its_first_tone_at_the_start_delay() {
    let mut s = Scene::new(Js8Speed::Normal.index(), 200, false, "HELLO");
    let ptt = s.ptt_until(14_000.0);
    assert_eq!(
        ptt,
        [(-200.0, "T 1".to_string()), (13_400.0, "T 0".to_string())],
        "premise: keyed in its TX delay"
    );
    let played = &s.backend.played;
    assert_eq!(
        played.len(),
        2_400 + 6_000 + 79 * 1_920,
        "silence, then the frame"
    );
    let first = played.iter().position(|&x| x != 0.0).expect("a tone");
    assert_eq!(
        -200.0 + first as f64 / 12.0,
        500.0,
        "the first tone is not at the period's start and 500 ms"
    );
}

/// FT8 IS UNTOUCHED: an FT8 CQ queued a second before its slot keys on the boundary's tick, not
/// 200 ms before it, and plays from there as it always did.
#[test]
fn ft8_keys_on_its_boundary_as_before() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |line, state| {
        (line == "T 1").then(|| {
            state.keyed = true;
            "RPRT 0\n".to_string()
        })
    });
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.broadcast("CQ KD9TAW EN52");
        let _ = e.take_immediate_tx();
    }
    let pair = 30_000.0;
    let t0 = ((now_unix_ms() + 120_000.0) / pair).ceil() * pair;
    let (mut state, mut backend, mut rig) = (
        loop_state(),
        MockBackend::new(),
        Rig::rigctld(&peer.address),
    );
    let mut t = t0 - 1_000.0;
    let mut keyed_at = None;
    while t <= t0 + 1_000.0 && keyed_at.is_none() {
        let tick = t;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, tick);
        if writes(&peer).iter().any(|l| l == "T 1") {
            keyed_at = Some(tick - t0);
        }
    }
    assert_eq!(keyed_at, Some(0.0), "FT8's key moved");
}

/// ⭐ HOLD PTT BETWEEN FRAMES: NO LEAD BETWEEN FRAMES. The first frame keys in its TX delay; the
/// transmitter is then held, and each later frame of the message keys on its boundary's tick over
/// the held key, as JS8Call's PTT is up already then, and its wave is on the slot clock all the
/// same. PTT drops once, after the last frame.
#[test]
fn with_hold_ptt_the_frames_after_the_first_key_on_their_boundary() {
    let mut s = Scene::new(Js8Speed::Normal.index(), 200, true, LONG);
    let frames = s.queued();
    assert!(frames >= 3, "premise: a message of several frames");
    let p = s.period_ms;
    let ptt = s.ptt_until(frames as f64 * p + p);
    let mut want = vec![(-200.0, "T 1".to_string())];
    want.extend((1..frames).map(|k| (k as f64 * p, "T 1".to_string())));
    let last = (frames - 1) as f64 * p;
    want.push((
        tick_at_or_after(last + wave_ms(Js8Speed::Normal) + crate::slot::TX_TAIL_MS),
        "T 0".to_string(),
    ));
    assert_eq!(ptt, want);
}

/// ⭐ STOP TX OR TX OFF INSIDE THE LEAD SENDS NOTHING. Keyed 200 ms before its period, the frame
/// is stopped 100 ms before it: the next tick unkeys and drops what the card holds, long before its
/// first tone's time (500 ms into the period), and nothing keys after. TX Off is no exception here:
/// the frame has sent nothing, and its period is the next cycle TX Off turns down.
#[test]
fn stop_tx_or_tx_off_inside_the_lead_unkeys_at_once_and_sends_nothing() {
    for stop in ["Stop TX", "TX Off"] {
        let mut s = Scene::new(Js8Speed::Normal.index(), 200, false, LONG);
        assert_eq!(
            s.ptt_until(-100.0),
            [(-200.0, "T 1".to_string())],
            "{stop}: premise: keyed in the TX delay"
        );
        let flushed = s.backend.flush_calls;
        {
            let mut e = s.engine.lock().unwrap();
            match stop {
                "Stop TX" => e.halt_tx(),
                _ => e.set_tx_enabled(false),
            }
        }
        assert_eq!(
            s.ptt_until(3.0 * 15_000.0),
            [(-80.0, "T 0".to_string())],
            "{stop}: unkeyed on the next tick, and nothing keyed after"
        );
        assert!(
            s.backend.flush_calls > flushed,
            "{stop}: what the card held was not dropped"
        );
    }
}

/// Control: TX Off once the frame's period has begun lets the frame finish, as TX Off lets any
/// over in flight finish; only the lead is cut.
#[test]
fn tx_off_after_the_period_has_begun_lets_the_frame_finish() {
    let mut s = Scene::new(Js8Speed::Normal.index(), 200, false, LONG);
    assert_eq!(
        s.ptt_until(1_000.0),
        [(-200.0, "T 1".to_string())],
        "premise"
    );
    s.engine.lock().unwrap().set_tx_enabled(false);
    assert_eq!(
        s.ptt_until(3.0 * 15_000.0),
        [(13_400.0, "T 0".to_string())],
        "the frame plays to its end, and nothing keys after"
    );
}

/// ⭐ THE RX PERIOD BEFORE A FRAME ENDS AT ITS KEY. With a frame to send, the boundary is taken in
/// the TX delay: the period just ending goes to the decoder then, with what has been heard of it,
/// and the key follows. With nothing to send, nothing happens in the TX delay, and the period is
/// decoded on its boundary's tick, whole, as before.
#[test]
fn the_period_before_a_frame_is_decoded_from_its_key_and_otherwise_at_its_boundary() {
    for (text, decoded_at) in [("HELLO", -200.0), ("", 0.0)] {
        let mut s = Scene::new(Js8Speed::Normal.index(), 200, false, text);
        // The loop's first tick takes the boundary of the period before, with nothing heard yet;
        // then something is heard in that period, so its own boundary has it to decode.
        assert!(s.ptt_until(-1_000.0).is_empty(), "premise: nothing keyed");
        assert!(!s.state.decode_in_flight, "premise: nothing to decode yet");
        s.backend.queue_capture(vec![0.01; 12_000]);
        let mut dispatched = None;
        while s.t <= s.t0 + 400.0 && dispatched.is_none() {
            let at = s.t - s.t0;
            let tick = s.t;
            let before = s.state.decode_in_flight;
            step_to(
                &s.engine,
                &mut s.state,
                &mut s.backend,
                &mut s.rig,
                &mut s.t,
                tick,
            );
            if !before && s.state.decode_in_flight {
                dispatched = Some(at);
            }
        }
        assert_eq!(
            dispatched,
            Some(decoded_at),
            "{text:?}: the period before was not decoded when expected"
        );
    }
}

/// ⭐ A HEARTBEAT GETS THE SAME LEAD. HB NET on, a heartbeat sent now keys 200 ms before the next
/// period, as every frame does: the loop's TX delay asks the same planner whatever the frame is.
#[test]
fn a_heartbeat_keys_in_its_tx_delay_too() {
    let mut s = Scene::new(Js8Speed::Normal.index(), 200, false, "");
    s.engine
        .lock()
        .unwrap()
        .js8_heartbeat_now()
        .expect("a heartbeat queued");
    assert_eq!(
        s.queued(),
        1,
        "premise: the heartbeat waits for the next period"
    );
    assert_eq!(
        s.ptt_until(14_000.0),
        [(-200.0, "T 1".to_string()), (13_400.0, "T 0".to_string())]
    );
}

/// How often a stretch of audio crosses zero: twice a cycle of the tone it carries.
fn crossings(w: &[f32]) -> usize {
    w.windows(2)
        .filter(|p| (p[0] < 0.0) != (p[1] < 0.0))
        .count()
}

/// ⭐ A MOVE IN THE TX DELAY KEEPS THE FIRST TONE ON TIME. "Allow TX offset changes while sending"
/// on, the TX offset moved 300 Hz up while the frame is still in its TX delay (20 ms of its 200 ms of
/// silence played): the card is handed back the rest of the silence and the whole frame at the new
/// offset, as long as what it held, so the first tone is still 500 ms into the period and every
/// symbol carries 300 Hz more (96 more zero crossings in 160 ms). Built from the frame alone, the
/// rest came back short by the silence, and the frame would have played that much early.
#[test]
fn a_move_in_the_tx_delay_moves_the_whole_frame_and_keeps_its_first_tone_on_time() {
    let mut s = Scene::with(
        Js8Speed::Normal.index(),
        tempo_app::settings::Settings {
            js8_tx_offset_while_sending: true,
            ..Default::default()
        },
        "HELLO",
    );
    assert_eq!(
        s.ptt_until(-180.0),
        [(-200.0, "T 1".to_string())],
        "premise"
    );
    let keyed = s.backend.played.clone();
    s.backend.tx_unplayed = keyed.len() - 240;
    {
        let mut e = s.engine.lock().unwrap();
        let hz = e.tx_offset_hz();
        e.set_tx_offset(hz + 300.0);
    }
    let _ = s.ptt_until(-160.0);
    assert_eq!(s.backend.retunes, 1, "the rest handed to the card once");
    let now = &s.backend.played;
    assert_eq!(now.len(), keyed.len(), "as long as it was");
    let first = |w: &[f32]| w.iter().position(|&x| x != 0.0);
    assert_eq!(
        first(now),
        Some(8_400),
        "the first tone is not 700 ms after the key"
    );
    assert_eq!(first(&keyed), Some(8_400), "control: as keyed");
    let symbol = |w: &[f32], k: usize| -> Vec<f32> {
        let start = 2_400 + 6_000 + k * 1_920;
        w[start..start + 1_920].to_vec()
    };
    for k in [0, 40, 78] {
        let (moved, was) = (crossings(&symbol(now, k)), crossings(&symbol(&keyed, k)));
        assert!(
            moved.abs_diff(was + 96) <= 2,
            "symbol {k}: {moved} crossings, keyed {was}: 300 Hz up"
        );
    }
}
