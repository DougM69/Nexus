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

/// What the cockpit says when the move is refused.
const HELD: &str = "TX offset not moved: a JS8 message is waiting to go or going out, and the \
     offset stays put until it has gone, as in JS8Call. To move it while sending, turn on \
     Settings ▸ Digital ▸ JS8 ▸ Allow TX offset changes while sending.";

/// ⭐ OFF, AS JS8CALL SHIPS IT: THE TX OFFSET STAYS PUT FROM SEND UNTIL THE MESSAGE HAS GONE. A move
/// is refused while the message waits for its first frame, between two frames, and while its last
/// frame is on the air, and the cockpit says why; every frame keys at 1500 Hz. Once the boundary
/// after the last frame has planned nothing, the move is taken. Putting the offset where it already
/// is is never refused. (Before this option the move was taken mid-message and the frames after it
/// keyed at 1800 Hz: one message on two offsets.)
#[test]
fn by_default_the_js8_tx_offset_stays_put_until_the_message_has_gone() {
    let mut e = js8_engine();
    let frames = queue_message(&mut e);
    let s = next_slot(&e);
    let mut keyed = Vec::new();
    e.set_tx_offset(1500.0);
    assert_eq!(
        e.js8_state().last_error,
        None,
        "the offset it already has: no refusal"
    );
    e.set_tx_offset(1800.0);
    assert_eq!(
        e.tx_offset_hz(),
        1500.0,
        "refused while the message waits to start"
    );
    assert_eq!(
        e.js8_state().last_error.as_deref(),
        Some(HELD),
        "…and the cockpit says why"
    );
    for k in 0..frames as u64 {
        keyed.push(key_frame(&mut e, s + k));
        e.set_tx_offset(1800.0);
        assert_eq!(e.tx_offset_hz(), 1500.0, "refused after frame {}", k + 1);
    }
    assert_eq!(keyed, vec![Some(1500.0); frames], "every frame at 1500 Hz");
    assert_eq!(
        key_frame(&mut e, s + frames as u64),
        None,
        "the message has gone"
    );
    e.set_tx_offset(1800.0);
    assert_eq!(e.tx_offset_hz(), 1800.0, "taken once nothing is going out");
}

/// ⭐ ON: THE MOVE IS TAKEN, AND THE FRAMES STILL TO GO KEY AT THE NEW OFFSET. The first frame keys
/// at 1500 Hz; the offset is moved to 1800 Hz between two frames, and every frame after it keys at
/// 1800 Hz. The frame on the air keeps its own (its wave was built at its key). This is what Nexus
/// did before the option, whatever the setting.
#[test]
fn with_the_option_on_a_move_mid_message_keys_the_frames_still_to_go_at_the_new_offset() {
    let mut e = js8_engine();
    e.settings.js8_tx_offset_while_sending = true;
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
    assert_eq!(e.js8_state().last_error, None, "nothing refused");
}

/// FT modes are untouched: an FT8 over on the air does not hold the TX offset, whatever the JS8
/// option says.
#[test]
fn an_ft8_over_on_the_air_does_not_hold_the_tx_offset() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.start_cq(None).expect("CQ");
    let slot = u64::from(!e.tx_even());
    assert!(e.plan_tx(slot).is_some(), "premise: an FT8 over planned");
    assert!(e.snapshot().radio.transmitting, "premise: on the air");
    e.set_tx_offset(1800.0);
    assert_eq!(e.tx_offset_hz(), 1800.0);
}
