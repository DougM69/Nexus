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

// ===== Typeahead (JS8Call's compose box while its message goes out) =====

/// The frame keyed at `slot` as the radio loop keys it, as a receiver decodes it: its text, first
/// and last flags. `None` when nothing keyed.
fn keyed_text(e: &mut Engine, slot: u64) -> Option<(String, bool, bool)> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Js8 { word, speed, .. } = &plan.waveform else {
        panic!("a JS8 plan carries the typed waveform");
    };
    let (frame, i3) = ::js8::proto::frame::decode_word(word, *speed).expect("decodes");
    let keyed = (frame.render(), i3.first, i3.last);
    let wave = plan.waveform.build();
    assert!(!e.commit_tx(&plan, wave, slot).is_empty(), "committed");
    Some(keyed)
}

/// ⭐ TYPEAHEAD ON THE ENGINE: after the first frame, what the box hands over is what the frames
/// still to go key, by value: the new text, as data, none flagged first and the last flagged last.
#[test]
fn typeahead_keys_the_typed_text_in_the_frames_still_to_go() {
    let mut e = js8_engine();
    queue_message(&mut e);
    let s = next_slot(&e);
    let first = keyed_text(&mut e, s).expect("the first frame");
    assert!(first.1 && !first.2, "premise: the message's first frame");
    let v = e.js8_typeahead_state().expect("the box holds the message");
    assert_eq!(v.sent, first.0, "what has gone");
    e.js8_typeahead(v.sent.chars().count(), "HELLO AGAIN".into())
        .expect("taken");
    let mut rest = Vec::new();
    while let Some(f) = keyed_text(&mut e, s + 1 + rest.len() as u64) {
        rest.push(f);
    }
    assert_eq!(
        rest.iter().map(|f| f.0.as_str()).collect::<String>(),
        "HELLO AGAIN"
    );
    assert!(
        rest.iter().all(|f| !f.1) && rest.last().is_some_and(|f| f.2),
        "data: none first, the last last: {rest:?}"
    );
}

/// A change in the box is an operator act, as a key press in JS8Call's box is: the idle count
/// restarts.
#[test]
fn a_typeahead_change_restarts_the_idle_count() {
    let mut e = js8_engine();
    queue_message(&mut e);
    let an_hour_ago = tempo_core::timing::now_unix_ms() as u64 - 60 * 60 * 1000;
    e.js8_station.mark_active(an_hour_ago);
    assert!(
        e.js8_state().idle_minutes >= 59,
        "premise: idle for an hour"
    );
    let v = e.js8_typeahead_state().expect("the box holds the message");
    e.js8_typeahead(0, v.unsent).expect("taken");
    assert_eq!(e.js8_state().idle_minutes, 0);
}

/// A MSG sent from the box is read-only while it goes, as in JS8Call, and the cockpit says why a
/// change was not taken.
#[test]
fn a_msg_from_the_box_takes_no_typeahead_and_says_why() {
    let mut e = js8_engine();
    e.js8_send_command(
        "W1AW".into(),
        ::js8::Command::Msg.id(),
        "HELLO THERE".into(),
    )
    .expect("queues");
    let v = e.js8_typeahead_state().expect("the box holds the message");
    assert!(v.locked);
    let refused = e
        .js8_typeahead(0, format!("{} AND MORE", v.unsent))
        .expect_err("refused");
    assert_eq!(
        refused,
        "A MSG or relay message cannot be changed while it goes out, as in JS8Call."
    );
    assert_eq!(e.js8_state().last_error, Some(refused));
}

/// Stop TX lets the box go: nothing of it is going out any more.
#[test]
fn stop_tx_lets_the_box_go() {
    let mut e = js8_engine();
    queue_message(&mut e);
    assert!(e.js8_typeahead_state().is_some());
    e.halt_tx();
    assert_eq!(e.js8_typeahead_state(), None);
}

/// Hold PTT between frames holds across text typed after the last frame, which goes straight
/// after it, as JS8Call holds once its box is framed again; with nothing typed it does not.
#[test]
fn hold_ptt_holds_across_text_typed_after_the_last_frame() {
    let mut e = js8_engine();
    e.settings.js8_hold_ptt = true;
    e.js8_send(None, "HI".into()).expect("queues");
    assert_eq!(e.js8_state().queue.len(), 1, "premise: a one-frame message");
    let s = next_slot(&e);
    let only = keyed_text(&mut e, s).expect("its frame");
    assert!(only.2, "premise: its last frame");
    assert!(!e.js8_holds_ptt_between_frames(), "nothing typed: no hold");
    e.js8_typeahead(only.0.chars().count(), "MORE".into())
        .expect("taken");
    assert!(e.js8_holds_ptt_between_frames(), "typed after it: held");
    assert_eq!(
        keyed_text(&mut e, s + 1),
        Some(("KD9TAW: MORE".to_string(), false, true)),
        "straight after, a message of its own, not flagged first"
    );
}

/// A move while the frame is still in its TX delay: the radio loop keyed it before its period and
/// handed the card the silence up to the period in front of the frame's wave (JS8Call's Modulator
/// sends that silence too, `JS8_Mode/Modulator.cpp:86-94`). What the card is handed back is as long
/// as what it still held: the silence still to play first, then the whole frame at the new offset,
/// its first tone where it was. (Built from the wave alone, it came back short by the silence, and
/// the card would have played the frame that much early.)
#[test]
fn a_move_in_the_tx_delay_keeps_the_silence_before_the_period_and_moves_the_whole_frame() {
    let mut e = js8_engine();
    e.settings.js8_tx_offset_while_sending = true;
    e.js8_send(None, "HELLO".into()).expect("queues");
    let s = next_slot(&e);
    let plan = e.plan_tx(s).expect("a frame planned");
    let TxWaveform::Js8 { word, speed, .. } = &plan.waveform else {
        panic!("a JS8 plan carries the typed waveform");
    };
    let (tones, speed) = (::js8::phy::encode_word(word, *speed), *speed);
    let wave = plan.waveform.build();
    assert!(!e.commit_tx(&plan, wave.clone(), s).is_empty(), "premise");
    e.set_tx_offset(1800.0);
    let mut rest = e.js8_retune_on_air(s).expect("the move is handed over");
    // 150 ms of the TX delay still to play, and the 63 samples before it a resampler asks for.
    let (silence, before) = (1_800, 63);
    let got = rest(wave.len() + silence, before);
    let moved = ::js8::phy::modulate(&tones, speed, 1800.0, tempo_core::tempo_fast::SAMPLE_RATE);
    assert_eq!(
        got.len(),
        before + silence + wave.len(),
        "as long as what the card held"
    );
    assert!(
        got[..before + silence].iter().all(|&x| x == 0.0),
        "the silence up to the period first"
    );
    assert!(
        got[before + silence..] == moved[..],
        "then the whole frame at 1800 Hz, sample for sample"
    );
}
