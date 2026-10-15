//! JS8's send mechanics on the engine: the TX offset moved while a message goes out.
//!
//! JS8Call 3.0.3 refuses an offset change while a message is queued for transmit or going out
//! (`changeFreq`, `JS8_UI/mainwindow.cpp:5381-5386`: `isMessageQueuedForTransmit() &&
//! !tx_qsy_allowed()`, where queued is `m_transmitting || m_txFrameCount > 0`, :3264-3266), unless
//! "Allow Tx frequency changes while transmitting" is on (`TxQSYAllowed`, off by default,
//! `JS8_UI/Configuration.cpp:2305`).

use super::*;

/// A JS8 station on the 20 m watering hole, TX on, its TX offset at 1500 Hz.
fn js8_engine() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.set_tx_enabled(true);
    e
}

/// The first slot after now, on the JS8 period.
fn next_slot(e: &Engine) -> u64 {
    let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
    tempo_core::timing::now_unix_ms() as u64 / period_ms + 1
}

/// Key one frame at `slot` as the radio loop does at a boundary (plan, build, commit), and say
/// the offset it keyed at, or `None` when nothing keyed.
fn key_frame(e: &mut Engine, slot: u64) -> Option<f32> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Js8 { f0, .. } = &plan.waveform else {
        panic!("a JS8 plan carries the typed waveform");
    };
    let f0 = *f0;
    let wave = plan.waveform.build();
    (!e.commit_tx(&plan, wave, slot).is_empty()).then_some(f0)
}

/// A message long enough to take several frames at every speed.
const LONG: &str =
    "TEST MESSAGE WITH MULTIPLE FRAMES, LONG ENOUGH TO TAKE SEVERAL OVERS AT EVERY SPEED";

/// A message of several frames queued, and how many frames it is.
fn queue_message(e: &mut Engine) -> usize {
    e.js8_send(None, LONG.into()).expect("queues");
    let frames = e.js8_state().queue.len();
    assert!(
        frames >= 3,
        "premise: a message of several frames: {frames}"
    );
    frames
}

/// ⭐ TODAY, MEASURED: THE OFFSET MOVES, AND THE FRAMES STILL TO GO KEY AT THE NEW ONE. The first
/// frame keys at 1500 Hz; the offset is moved to 1800 Hz between two frames, and every frame after
/// it keys at 1800 Hz: one message, split across two offsets. The frame on the air when the offset
/// moves keeps its own (its wave was built at its key).
#[test]
fn today_a_tx_offset_moved_mid_message_moves_the_frames_still_to_go() {
    let mut e = js8_engine();
    let frames = queue_message(&mut e);
    let s = next_slot(&e);
    assert_eq!(key_frame(&mut e, s), Some(1500.0), "the first frame");
    e.set_tx_offset(1800.0);
    assert_eq!(e.tx_offset_hz(), 1800.0, "the move is taken");
    let rest: Vec<Option<f32>> = (1..frames as u64)
        .map(|k| key_frame(&mut e, s + k))
        .collect();
    assert_eq!(
        rest,
        vec![Some(1800.0); frames - 1],
        "the frames after the move"
    );
    assert_eq!(
        key_frame(&mut e, s + frames as u64),
        None,
        "the message has gone"
    );
}
