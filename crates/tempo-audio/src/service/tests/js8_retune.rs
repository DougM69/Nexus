//! "Allow TX offset changes while sending" on the real loop: a move of the TX offset while a JS8
//! frame is on the air moves the rest of that frame to the new offset, as JS8Call 3.0.3's Modulator
//! takes a new frequency mid-frame (`TxQSYAllowed`, Configuration.cpp:2305; `setFreq` →
//! `Modulator::setAudioFrequency`, mainwindow.cpp:4212-4217, UI_Constructor.cpp:238-239;
//! Modulator.cpp:217-229). Nothing else about the over changes: its length, its PTT lines and its
//! unkey are the unmoved frame's. With the option off the move is refused and the frame plays as it
//! was keyed.
//!
//! The loop is the real `RadioLoop::step`, a 20 ms tick at a time, on a rigctld that keys and the
//! mock sound card, which a scene tells how far its playback has got (`MockBackend::tx_unplayed`).
//! As in `js8_frames`, every scene runs two minutes or more ahead of the real clock, so each key
//! lands on the tick the scene steps it at.

use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};
use tempo_app::engine::js8::JS8_RETUNE_LEAD;

/// A short message: its first frame is the over each scene moves (the next keys a period later).
const TEXT: &str = "HELLO";

/// A JS8 station at Normal with TEXT queued at 1500 Hz, TX on, and the loop that sends it.
struct Frame {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    t: f64,
    t0: f64,
}

impl Frame {
    /// …with "Allow TX offset changes while sending" `allow`.
    fn new(allow: bool) -> Frame {
        Frame::with(allow, false, TEXT)
    }

    /// …with Hold PTT between frames `hold`, and `text` queued.
    fn with(allow: bool, hold: bool, text: &str) -> Frame {
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
                js8_tx_offset_while_sending: allow,
                js8_hold_ptt: hold,
                ..Default::default()
            },
        )));
        let period_ms = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.js8_set_speed(1).unwrap();
            e.js8_enter();
            e.set_frequency(14.078, "20m", "USB");
            e.set_tx_offset(1500.0);
            e.set_tx_enabled(true);
            e.js8_send(None, text.into()).unwrap();
            assert!(!e.js8_state().queue.is_empty(), "premise: a message queued");
            e.active_slot_secs() * 1000.0
        };
        let t0 = ((now_unix_ms() + 120_000.0) / period_ms).ceil() * period_ms;
        Frame {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            t: t0,
            t0,
        }
    }

    /// Step the loop to `until` ms from the frame's boundary.
    fn run_to(&mut self, until: u32) {
        let end = self.t0 + f64::from(until);
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            end,
        );
    }

    fn ptt_lines(&self) -> Vec<String> {
        writes(&self.peer)
            .into_iter()
            .filter(|l| l.starts_with("T "))
            .collect()
    }

    /// The operator moves the TX offset, as the waterfall's right-click does.
    fn move_offset(&self, hz: f32) {
        self.engine.lock().unwrap().set_tx_offset(hz);
    }
}

/// How often a stretch of audio crosses zero: twice a cycle of the tone it carries.
fn crossings(w: &[f32]) -> usize {
    w.windows(2)
        .filter(|p| (p[0] < 0.0) != (p[1] < 0.0))
        .count()
}

/// A Normal frame's symbol `k`: 500 ms of start delay, then 1920 samples a symbol at 12 kHz.
fn symbol(w: &[f32], k: usize) -> &[f32] {
    let start = 6_000 + k * 1_920;
    &w[start..start + 1_920]
}

/// ⭐ THE OPTION ON: THE FRAME ON THE AIR MOVES. Five seconds into a frame keyed at 1500 Hz the
/// operator moves the TX offset to 1800 Hz: on the next tick the sound card is handed the rest of
/// the frame, JS8_RETUNE_LEAD into it moved to 1800 Hz. What it had played stays as keyed, sample
/// for sample, and so does the lead; every whole symbol after the move carries 300 Hz more (48
/// cycles in 160 ms, 96 more zero crossings); no sample jumps at the move; the frame is as long as
/// it was; and PTT keys and unkeys exactly as for a frame nobody moved.
#[test]
fn with_offset_changes_on_a_move_mid_frame_moves_the_rest_of_the_frame() {
    let mut still = Frame::new(true);
    still.run_to(14_000);
    let mut f = Frame::new(true);
    f.run_to(5_000);
    let keyed = f.backend.played.clone();
    assert!(
        !keyed.is_empty(),
        "premise: the frame keyed at its boundary"
    );
    let played = 60_000;
    f.backend.tx_unplayed = keyed.len() - played;
    f.move_offset(1800.0);
    f.run_to(5_040);
    assert_eq!(f.backend.retunes, 1, "the rest handed to the card once");
    let now = &f.backend.played;
    assert_eq!(now.len(), keyed.len(), "as long as it was");
    let at = played + JS8_RETUNE_LEAD;
    assert_eq!(
        now[..at],
        keyed[..at],
        "what played, and the lead, as keyed"
    );
    for k in [30, 45, 70] {
        let (moved, was) = (crossings(symbol(now, k)), crossings(symbol(&keyed, k)));
        assert!(
            moved.abs_diff(was + 96) <= 2,
            "symbol {k}: {moved} crossings, keyed {was}: 300 Hz up"
        );
    }
    let step = |w: &[f32]| {
        w.windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(
        step(&now[at - 100..at + 100]) <= step(symbol(now, 40)) + 0.05,
        "no jump at the move"
    );
    f.run_to(14_000);
    assert_eq!(f.backend.retunes, 1, "one move, one hand-over");
    assert_eq!(
        f.ptt_lines(),
        still.ptt_lines(),
        "PTT as for a frame nobody moved"
    );
    assert_eq!(
        still.backend.played, keyed,
        "control: the unmoved frame as keyed"
    );
}

/// The option off (JS8Call's default): the move is refused while the frame is on the air, and the
/// frame plays as keyed, sample for sample.
#[test]
fn with_offset_changes_off_the_frame_on_the_air_plays_as_keyed() {
    let mut f = Frame::new(false);
    f.run_to(5_000);
    let keyed = f.backend.played.clone();
    f.backend.tx_unplayed = keyed.len() - 60_000;
    f.move_offset(1800.0);
    f.run_to(14_000);
    assert_eq!(f.backend.retunes, 0, "nothing handed to the card");
    assert_eq!(f.backend.played, keyed, "the frame as keyed");
    assert_eq!(
        f.engine.lock().unwrap().tx_offset_hz(),
        1500.0,
        "the move refused"
    );
}

/// No over on the air, nothing to move: a move after the frame's PTT has dropped hands the card
/// nothing (and the next frame plans at the new offset, as before).
#[test]
fn a_move_after_the_frame_has_gone_hands_the_card_nothing() {
    let mut f = Frame::new(true);
    f.run_to(14_000);
    assert_eq!(
        f.ptt_lines(),
        ["T 1", "T 0"],
        "premise: the frame keyed and unkeyed"
    );
    f.move_offset(1800.0);
    f.run_to(14_500);
    assert_eq!(f.backend.retunes, 0);
}

/// A move landing on the tick the frame's PTT drops (Normal: 13.14 s of wave and the 250 ms tail
/// from the key, so the 13.40 s tick) hands the card nothing: the over is over.
#[test]
fn a_move_on_the_tick_the_frame_ends_hands_the_card_nothing() {
    let mut f = Frame::new(true);
    f.run_to(13_380);
    assert_eq!(f.ptt_lines(), ["T 1"], "premise: still keyed at 13.38 s");
    f.move_offset(1800.0);
    f.run_to(13_400);
    assert_eq!(
        f.ptt_lines(),
        ["T 1", "T 0"],
        "premise: unkeyed on the 13.40 s tick"
    );
    assert_eq!(f.backend.retunes, 0);
}

/// With Hold PTT between frames on, the gap between two frames is a hold, not an over: nothing
/// plays, and a move in it hands the card nothing. The next frame is planned at the new offset.
#[test]
fn a_move_while_ptt_is_held_between_frames_hands_the_card_nothing() {
    let mut f = Frame::with(true, true, "HELLO FROM THE PARK TO YOU ALL");
    f.run_to(14_000);
    assert_eq!(f.ptt_lines(), ["T 1"], "premise: held between the frames");
    f.move_offset(1800.0);
    f.run_to(14_400);
    assert_eq!(f.backend.retunes, 0);
}
