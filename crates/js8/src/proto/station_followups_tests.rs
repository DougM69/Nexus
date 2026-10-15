//! Three follow-ups to JS8Call 3.0.3's automatic replies and held messages (processCommandActivity.cpp
//! read as the spec): a MSG that reads "MSG n" is a MSG, never a QUERY MSG n; a held message whose
//! hand-over would not fit one transmission at the current speed is not offered, and the Inbox is
//! told why; and a heartbeat's acknowledgement goes through JS8Call's @ALLCALL cache, checked and
//! recorded as JS8Call checks and records it. Every reply is checked BY VALUE of what keys: the
//! words of its frames, its origin, the offset it keys at, its speed and the period it goes in.
use super::*;
use crate::proto::compose::frames_with_grid;
use crate::proto::frame::{decode_word, encode_frame};
use crate::proto::reassembly::Checksum;

const T0: u64 = 10_000_000;
const MIN: u64 = 60_000;

/// My station: KD9TAW in EN52 at `speed`, in @FUN, replies going by themselves (the confirmation
/// off) so each one keys; JS8Call's other defaults (AUTO and relay on).
fn cfg(speed: Speed) -> StationConfig {
    StationConfig {
        mycall: "KD9TAW".into(),
        grid: "EN52".into(),
        speed,
        autoreply_confirmation: false,
        groups: vec!["@FUN".into()],
        ..Default::default()
    }
}

/// The same, acknowledging heartbeats: heartbeat networking, HB ACK and AUTO on.
fn hb_cfg() -> StationConfig {
    StationConfig {
        hb_networking: true,
        hb_ack: true,
        ..cfg(Speed::Normal)
    }
}

/// A directed message heard at `at_ms`.
fn ev(from: &str, to: &str, cmd: Command, text: &str, snr: i32, at_ms: u64) -> MessageEvent {
    MessageEvent::Message(Message {
        from: from.into(),
        to: CallRef::parse(to),
        to_text: to.into(),
        cmd: Some(cmd),
        num: None,
        text: text.into(),
        checksum: Checksum::NotRequired,
        path: vec![],
        freq_hz: 1500.0,
        snr_db: snr,
        speed: Speed::Normal,
        first_ms: at_ms,
        last_ms: at_ms,
        frames: 1,
        complete: true,
        compound_from: None,
        grid: None,
        cq: None,
    })
}

/// A heartbeat heard at `at_ms`.
fn hb(from: &str, snr: i32, at_ms: u64) -> MessageEvent {
    MessageEvent::Message(Message {
        from: from.into(),
        to: CallRef::parse("@HB"),
        to_text: "@HB".into(),
        cmd: None,
        num: None,
        text: String::new(),
        checksum: Checksum::NotRequired,
        path: vec![],
        freq_hz: 900.0,
        snr_db: snr,
        speed: Speed::Normal,
        first_ms: at_ms,
        last_ms: at_ms,
        frames: 1,
        complete: true,
        compound_from: None,
        grid: Some("FN30".into()),
        cq: None,
    })
}

/// `from` leaves `body` (`<target> <text>`) here with `MSG TO:`; its ACK is taken off the queue.
fn store_from(s: &mut Station, from: &str, body: &str, at_ms: u64) {
    s.on_event(&ev(from, "KD9TAW", Command::MsgTo, body, -5, at_ms), at_ms);
    s.drop_queue();
}

/// The replies made and not yet keyed, by text: asking, queued, or in the outbox.
fn replies(s: &Station) -> Vec<String> {
    let mine = format!("{}: ", s.base());
    s.pending
        .iter()
        .map(|p| p.text.clone())
        .chain(s.queued.iter().map(|q| q.text.clone()))
        .chain(s.outbox.iter().map(|o| {
            o.display
                .strip_prefix(&mine)
                .unwrap_or(&o.display)
                .to_string()
        }))
        .collect()
}

/// Every frame the station keys, period after period from `from_ms`, each booked as the engine
/// books it (`note_tx_done`).
fn key_all(s: &mut Station, from_ms: u64) -> Vec<(u64, TxFrame)> {
    let period = u64::from(s.config().speed.period_s()) * 1000;
    let mut out = Vec::new();
    for k in 0..60 {
        let t = from_ms + k * period;
        s.process_tx_queue();
        let mut rng = || 0u32;
        let Some(f) = s.next_frame(t, &|_| false, &mut rng) else {
            break;
        };
        s.note_tx_done(&f, t);
        out.push((t, f));
    }
    out
}

/// The words KD9TAW keys for `text` at `speed`, as compose frames it.
fn words(text: &str, speed: Speed) -> Vec<Word87> {
    frames_with_grid("KD9TAW", "EN52", None, text, speed)
        .expect("composes")
        .iter()
        .map(|(f, i3)| encode_frame(f, *i3, speed).expect("packs"))
        .collect()
}

/// What a receiver prints for the frames keyed, run together.
fn decoded(frames: &[(u64, TxFrame)]) -> String {
    frames
        .iter()
        .map(|(_, f)| decode_word(&f.word, f.speed).expect("decodes").0.render())
        .collect()
}

/// `text` is what keys, by value: those words, with `origin`, at my speed, in the period after
/// `asked_ms`; an automatic reply on my own offset, an acknowledgement on a heartbeat spot.
fn assert_keys(s: &mut Station, asked_ms: u64, text: &str, origin: Origin) {
    let speed = s.config().speed;
    let period = u64::from(speed.period_s()) * 1000;
    let first = asked_ms / period * period + period;
    let keyed = key_all(s, first);
    let got: Vec<Word87> = keyed.iter().map(|(_, f)| f.word).collect();
    assert_eq!(
        got,
        words(text, speed),
        "keys {text:?}, decoded {:?}",
        decoded(&keyed)
    );
    assert_eq!(keyed[0].0, first, "in the period after the question");
    for (_, f) in &keyed {
        assert_eq!((f.origin, f.speed), (origin, speed), "{text:?}");
        match origin {
            Origin::HbAck => assert!(
                matches!(f.freq_hint, FreqHint::HbSubband(_)),
                "an acknowledgement keys on a heartbeat spot"
            ),
            _ => assert_eq!(f.freq_hint, FreqHint::Dial, "a reply keys on my own offset"),
        }
    }
}

fn state_of(s: &Station, id: u32) -> Option<InboxState> {
    s.inbox().iter().find(|e| e.id == id).map(|e| e.state)
}

// ---- a MSG that reads "MSG n" ------------------------------------------------------------------

/// `KD9TAW MSG MSG 1` is a MSG whose text is "MSG 1": filed for the operator and acknowledged, and
/// nothing is handed over. JS8Call dispatches on the command: ` MSG` files it (:756), ` QUERY`
/// with "MSG n" asks for message n (:864-981). Before, any command whose text read "MSG n" was
/// taken for QUERY MSG n, so this MSG was lost and K1ABC was handed message 1, or the group's
/// message 2, which any station may ask for. QUERY MSG 1 itself still hands message 1 over.
#[test]
fn a_msg_that_reads_msg_n_is_filed_and_acknowledged_not_taken_for_query_msg_n() {
    let mut s = Station::new(cfg(Speed::Normal));
    store_from(&mut s, "W1AW", "K1ABC T1", T0);
    store_from(&mut s, "W1AW", "@FUN NET AT EIGHT", T0 + 1);
    assert_eq!(
        (state_of(&s, 1), state_of(&s, 2)),
        (Some(InboxState::Store), Some(InboxState::Store))
    );
    for (n, at) in [(1, T0 + MIN), (2, T0 + 2 * MIN)] {
        s.on_event(
            &ev("K1ABC", "KD9TAW", Command::Msg, &format!("MSG {n}"), -5, at),
            at,
        );
        assert_eq!(replies(&s), ["K1ABC ACK"], "MSG MSG {n} is acknowledged");
        assert_keys(&mut s, at, "K1ABC ACK", Origin::AutoReply);
    }
    let filed: Vec<(&str, &str, &str, InboxState)> = s
        .inbox()
        .iter()
        .filter(|e| e.from == "K1ABC")
        .map(|e| (e.from.as_str(), e.to.as_str(), e.text.as_str(), e.state))
        .collect();
    assert_eq!(
        filed,
        [
            ("K1ABC", "KD9TAW", "MSG 1", InboxState::Unread),
            ("K1ABC", "KD9TAW", "MSG 2", InboxState::Unread),
        ],
        "each MSG is filed for the operator"
    );
    assert_eq!(
        (state_of(&s, 1), state_of(&s, 2)),
        (Some(InboxState::Store), Some(InboxState::Store)),
        "nothing was handed over"
    );
    let at = T0 + 3 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, at), at);
    assert_eq!(replies(&s), ["K1ABC MSG T1 FROM W1AW"], "QUERY MSG 1");
    assert_keys(&mut s, at, "K1ABC MSG T1 FROM W1AW", Origin::AutoReply);
    assert_eq!(state_of(&s, 1), Some(InboxState::Delivered));
}

// ---- a held message too long to hand over at this speed ---------------------------------------

/// The hand-over of `text` to K1ABC from W1AW, at `speed`.
fn hand_over(text: &str, suffix: &str, speed: Speed) -> Result<usize, ComposeError> {
    frames_with_grid(
        "KD9TAW",
        "EN52",
        None,
        &format!("K1ABC MSG {text} FROM W1AW{suffix}"),
        speed,
    )
    .map(|f| f.len())
}

/// The shortest run of "QRV " whose hand-over to K1ABC does not fit one transmission at Slow, and
/// does at Normal.
fn too_long_at_slow() -> String {
    let text = (1..200)
        .map(|n| "QRV ".repeat(n).trim_end().to_string())
        .find(|t| {
            matches!(
                hand_over(t, "", Speed::Slow),
                Err(ComposeError::TooLong { .. })
            )
        })
        .expect("some run is too long at Slow");
    assert!(
        hand_over(&text, "", Speed::Normal).is_ok(),
        "and fits at Normal"
    );
    text
}

/// A held message whose hand-over would not fit one transmission at the current speed (Store…
/// refuses such a message up front; one that came over the air can be one) is held but not
/// offered: QUERY MSGS offers the next one that fits, and says NO when none does; a heartbeat's
/// acknowledgement names none; QUERY MSG n hands nothing over; the delivery of another names it
/// not as NEXT MSG ID; and the `+k` counts it not. The Inbox is told which, how many frames, the
/// cap, and the speed it would go at. At that speed it is offered and handed over as before.
#[test]
fn a_held_message_too_long_to_hand_over_at_this_speed_is_not_offered() {
    let long = too_long_at_slow();
    let frames = hand_over(&long, "", Speed::Normal).expect("fits at Normal");
    let mut c = cfg(Speed::Slow);
    c.hb_networking = true;
    c.hb_ack = true;
    let mut s = Station::new(c.clone());
    store_from(&mut s, "W1AW", &format!("K1ABC {long}"), T0);
    store_from(&mut s, "W1AW", "K1ABC T2", T0 + 1);
    let Err(ComposeError::TooLong {
        frames: at_slow, ..
    }) = hand_over(&long, "", Speed::Slow)
    else {
        unreachable!("too long at Slow")
    };
    assert_eq!(
        s.held_too_long(),
        [HeldTooLong {
            id: 1,
            frames: at_slow,
            max: 19,
            fits_at: Some(Speed::Normal),
        }],
        "the Inbox is told which, why, and the speed it goes at"
    );

    let ask = |s: &mut Station, cmd: Command, text: &str, at: u64| -> Vec<String> {
        s.on_event(&ev("K1ABC", "KD9TAW", cmd, text, -5, at), at);
        replies(s)
    };
    let t = T0 + MIN;
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t),
        ["K1ABC YES MSG ID 2"]
    );
    assert_keys(&mut s, t, "K1ABC YES MSG ID 2", Origin::AutoReply);
    let t = T0 + 3 * MIN;
    assert_eq!(
        ask(&mut s, Command::Query, "MSG 1", t),
        Vec::<String>::new(),
        "QUERY MSG 1 hands nothing over"
    );
    assert_eq!(state_of(&s, 1), Some(InboxState::Store), "still held");
    s.on_event(&hb("K1ABC", -5, t + 1), t + 1);
    assert_eq!(
        replies(&s),
        ["K1ABC HEARTBEAT SNR -05 MSG ID 2"],
        "the acknowledgement offers the one that fits"
    );
    assert_keys(
        &mut s,
        t + 1,
        "K1ABC HEARTBEAT SNR -05 MSG ID 2",
        Origin::HbAck,
    );
    let t = T0 + 30 * MIN;
    assert_eq!(
        ask(&mut s, Command::Query, "MSG 2", t),
        ["K1ABC MSG T2 FROM W1AW"],
        "no NEXT MSG ID for the one that does not fit"
    );
    assert_keys(&mut s, t, "K1ABC MSG T2 FROM W1AW", Origin::AutoReply);
    let t = T0 + 33 * MIN;
    assert_eq!(ask(&mut s, Command::QueryMsgs, "", t), ["K1ABC NO"]);
    assert_keys(&mut s, t, "K1ABC NO", Origin::AutoReply);

    // At Normal it fits: offered, and handed over by value.
    c.speed = Speed::Normal;
    s.set_config(c);
    assert_eq!(
        s.held_too_long(),
        Vec::<HeldTooLong>::new(),
        "nothing to say at Normal"
    );
    let t = T0 + 40 * MIN;
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t),
        ["K1ABC YES MSG ID 1"]
    );
    assert_keys(&mut s, t, "K1ABC YES MSG ID 1", Origin::AutoReply);
    let t = T0 + 42 * MIN;
    let delivery = format!("K1ABC MSG {long} FROM W1AW");
    assert_eq!(ask(&mut s, Command::Query, "MSG 1", t), [delivery.clone()]);
    let keyed = key_all(&mut s, t / 15_000 * 15_000 + 15_000);
    assert_eq!(
        keyed.len(),
        frames,
        "the whole hand-over, one frame a period"
    );
    assert_eq!(
        keyed.iter().map(|(_, f)| f.word).collect::<Vec<_>>(),
        words(&delivery, Speed::Normal)
    );
    assert_eq!(state_of(&s, 1), Some(InboxState::Delivered));
}

/// The count after an offer (`+k`) counts only what could be handed over at this speed, and a
/// group's message that would not fit is not offered to the group either.
#[test]
fn a_message_too_long_at_this_speed_is_neither_counted_nor_offered_to_a_group() {
    let long = too_long_at_slow();
    let mut s = Station::new(cfg(Speed::Slow));
    store_from(&mut s, "W1AW", "K1ABC T1", T0);
    store_from(&mut s, "W1AW", &format!("K1ABC {long}"), T0 + 1);
    store_from(&mut s, "W1AW", "K1ABC T3", T0 + 2);
    let t = T0 + MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, t), t);
    assert_eq!(
        replies(&s),
        ["K1ABC YES MSG ID 1 +1"],
        "T3 counted, the long one not"
    );
    s.drop_queue();
    let t = T0 + 2 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t), t);
    assert_eq!(
        replies(&s),
        ["K1ABC MSG T1 FROM W1AW NEXT MSG ID 3"],
        "the next that fits"
    );
    s.drop_queue();

    let mut s = Station::new(cfg(Speed::Slow));
    store_from(&mut s, "W1AW", &format!("@FUN {long}"), T0);
    let t = T0 + MIN;
    s.on_event(&ev("K1ABC", "@FUN", Command::QueryMsgs, "", -5, t), t);
    assert_eq!(replies(&s), ["K1ABC NO"]);
    s.drop_queue();
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t + 1),
        t + 1,
    );
    assert_eq!(replies(&s), Vec::<String>::new(), "nor handed over");
    assert_eq!(s.held_too_long().len(), 1, "and the Inbox is told");
}

/// A message whose hand-over fits, but not with `NEXT MSG ID m` after it, is handed over without
/// it: the next one is offered on the next QUERY MSGS, rather than this one never going.
#[test]
fn a_hand_over_that_fits_only_without_next_msg_id_goes_without_it() {
    let text = (1..200)
        .map(|n| "QRV ".repeat(n).trim_end().to_string())
        .find(|t| {
            hand_over(t, "", Speed::Slow).is_ok()
                && hand_over(t, " NEXT MSG ID 2", Speed::Slow).is_err()
        })
        .expect("a run that fits only without the suffix");
    let mut s = Station::new(cfg(Speed::Slow));
    store_from(&mut s, "W1AW", &format!("K1ABC {text}"), T0);
    store_from(&mut s, "W1AW", "K1ABC T2", T0 + 1);
    let t = T0 + MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, t), t);
    assert_eq!(replies(&s), ["K1ABC YES MSG ID 1 +1"]);
    s.drop_queue();
    let t = T0 + 2 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t), t);
    let delivery = format!("K1ABC MSG {text} FROM W1AW");
    assert_eq!(replies(&s), [delivery.clone()]);
    assert_keys(&mut s, t, &delivery, Origin::AutoReply);
    assert_eq!(state_of(&s, 1), Some(InboxState::Delivered));
    let t = T0 + 30 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, t), t);
    assert_eq!(replies(&s), ["K1ABC YES MSG ID 2"]);
}

/// A reply that no longer composes when it is taken off the queue (the speed went down while it
/// asked) is not sent, and the log says so; a delivery it carried stays held.
#[test]
fn a_reply_that_no_longer_fits_when_it_goes_is_said_and_its_message_stays_held() {
    let long = too_long_at_slow();
    let mut c = cfg(Speed::Normal);
    c.autoreply_confirmation = true;
    let mut s = Station::new(c.clone());
    store_from(&mut s, "W1AW", &format!("K1ABC {long}"), T0);
    let t = T0 + MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t), t);
    let p = s.pending_reply().expect("asks first");
    c.speed = Speed::Slow;
    s.set_config(c);
    let acts = s
        .answer_reply(&p.display, p.fires_at_ms, true, t + 1_000)
        .expect("still asking");
    let Err(ComposeError::TooLong { frames, max }) = hand_over(&long, "", Speed::Slow) else {
        unreachable!("too long at Slow")
    };
    let said = format!(
        "Auto-reply not sent, it needs {frames} frames and one transmission at this speed holds \
         {max}: K1ABC MSG {long} FROM W1AW"
    );
    assert!(
        acts.iter()
            .any(|a| matches!(a, StationAction::Toast { text, .. } if *text == said)),
        "said: {acts:?}"
    );
    assert!(key_all(&mut s, t + 15_000).is_empty(), "nothing keys");
    assert_eq!(state_of(&s, 1), Some(InboxState::Store), "still held");
}

// ---- the @ALLCALL cache and the heartbeat's acknowledgement -----------------------------------

fn held_off_hb(acts: &[StationAction], who: &str) -> bool {
    acts.iter()
        .any(|a| matches!(a, StationAction::HbAckHeldOff { from } if from == who))
}

fn held_off_allcall(acts: &[StationAction], who: &str) -> bool {
    acts.iter()
        .any(|a| matches!(a, StationAction::RateLimited { from } if from == who))
}

/// JS8Call reads a heartbeat (to @HB) as @ALLCALL traffic (`isAllCallIncluded`, mainwindow.cpp:
/// 6088-6090), so its @ALLCALL cache applies: a station whose heartbeat it acknowledged is not
/// acknowledged again for 15 minutes (the check, processCommandActivity.cpp:303-311; the record,
/// :719-723). By value: the acknowledgement at T0, nothing at T0 + 10 min, and the next at
/// T0 + 15 min.
#[test]
fn a_heartbeat_is_acknowledged_at_most_once_in_fifteen_minutes() {
    let mut s = Station::new(hb_cfg());
    let acts = s.on_event(&hb("W1AW", -5, T0), T0);
    assert!(!held_off_hb(&acts, "W1AW"));
    assert_keys(&mut s, T0, "W1AW HEARTBEAT SNR -05", Origin::HbAck);
    let t = T0 + 10 * MIN;
    let acts = s.on_event(&hb("W1AW", -5, t), t);
    assert_eq!(replies(&s), Vec::<String>::new(), "10 minutes on: held off");
    assert!(held_off_hb(&acts, "W1AW"), "and said: {acts:?}");
    assert!(key_all(&mut s, t + 15_000).is_empty());
    let t = T0 + 15 * MIN;
    s.on_event(&hb("W1AW", -7, t), t);
    assert_keys(&mut s, t, "W1AW HEARTBEAT SNR -07", Origin::HbAck);
}

/// Both ways through the one cache: an @ALLCALL QUERY CALL from a station whose heartbeat was just
/// acknowledged is held off, and so is the acknowledgement of a heartbeat from a station just
/// answered on @ALLCALL. 15 minutes on, each is answered.
#[test]
fn the_acknowledgement_and_the_allcall_replies_share_the_allcall_cache() {
    let mut s = Station::new(hb_cfg());
    // K1ABC is heard, so QUERY CALL K1ABC? has an answer.
    s.on_event(&ev("K1ABC", "N0XYZ", Command::SnrQuery, "", -12, T0), T0);
    s.on_event(&hb("W1AW", -5, T0), T0);
    assert_keys(&mut s, T0, "W1AW HEARTBEAT SNR -05", Origin::HbAck);
    let t = T0 + MIN;
    let acts = s.on_event(
        &ev("W1AW", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t),
        t,
    );
    assert_eq!(replies(&s), Vec::<String>::new(), "held off");
    assert!(held_off_allcall(&acts, "W1AW"), "said: {acts:?}");
    let t = T0 + 15 * MIN;
    s.on_event(
        &ev("W1AW", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t),
        t,
    );
    assert_keys(&mut s, t, "W1AW YES -12 (15M)", Origin::AutoReply);

    // The other way: answered on @ALLCALL, then its heartbeat.
    let mut s = Station::new(hb_cfg());
    store_from(&mut s, "N0XYZ", "W1AW HELLO", T0);
    let t = T0 + MIN;
    s.on_event(&ev("W1AW", "@ALLCALL", Command::QueryMsgs, "", -5, t), t);
    assert_keys(&mut s, t, "W1AW YES MSG ID 1", Origin::AutoReply);
    let t = T0 + 2 * MIN;
    let acts = s.on_event(&hb("W1AW", -5, t), t);
    assert_eq!(replies(&s), Vec::<String>::new(), "no acknowledgement");
    assert!(held_off_hb(&acts, "W1AW"));
    let t = T0 + 16 * MIN;
    s.on_event(&hb("W1AW", -5, t), t);
    assert_keys(&mut s, t, "W1AW HEARTBEAT SNR -05 MSG ID 1", Origin::HbAck);
}

/// JS8Call keys its cache on the call as heard (`d.from`): W1AW/P is another entry than W1AW, for
/// the acknowledgement and for every @ALLCALL reply alike.
#[test]
fn the_allcall_cache_keys_on_the_call_as_heard() {
    let mut s = Station::new(hb_cfg());
    s.on_event(&ev("K1ABC", "N0XYZ", Command::SnrQuery, "", -12, T0), T0);
    s.on_event(&hb("W1AW", -5, T0), T0);
    s.drop_queue();
    let t = T0 + MIN;
    s.on_event(&hb("W1AW/P", -5, t), t);
    assert_keys(&mut s, t, "W1AW/P HEARTBEAT SNR -05", Origin::HbAck);
    let t = T0 + 2 * MIN;
    s.on_event(
        &ev("W1AW/P", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t),
        t,
    );
    assert_eq!(
        replies(&s),
        Vec::<String>::new(),
        "W1AW/P was just acknowledged"
    );
    s.on_event(
        &ev("N2DEF/P", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t),
        t,
    );
    assert_eq!(replies(&s), ["N2DEF/P YES -12 (2M)"]);
    s.drop_queue();
    s.on_event(
        &ev("N2DEF", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t + 1),
        t + 1,
    );
    assert_eq!(
        replies(&s),
        ["N2DEF YES -12 (2M)"],
        "N2DEF is not N2DEF/P, just answered"
    );
}

/// The cache is checked before the 55-minute rule, as JS8Call checks it before it runs that rule
/// (processCommandActivity.cpp:303-311 before :708-717): a heartbeat the cache holds off is not
/// counted by the rule. A station that heartbeats every 10 minutes is acknowledged at 0, held off
/// at 10, and stopped by the rule at 20 (not at 10); that heartbeat still enters the cache, so its
/// @ALLCALL QUERY CALL at 25 is held off. A station already stopped, or on the deny list, is
/// turned away first and enters nothing: its @ALLCALL QUERY CALL is answered.
#[test]
fn the_allcall_cache_comes_before_the_55_minute_rule_and_a_stopped_station_enters_nothing() {
    let mut c = hb_cfg();
    c.hb_rate_limit = true;
    c.hb_ack_deny = vec!["N0DEN".into()];
    let mut s = Station::new(c);
    s.on_event(&ev("K1ABC", "N0XYZ", Command::SnrQuery, "", -12, T0), T0);
    s.on_event(&hb("W1AW", -5, T0), T0);
    assert_keys(&mut s, T0, "W1AW HEARTBEAT SNR -05", Origin::HbAck);
    let t = T0 + 10 * MIN;
    let acts = s.on_event(&hb("W1AW", -5, t), t);
    assert!(held_off_hb(&acts, "W1AW"), "held off by the cache");
    assert_eq!(s.hb_rate_blocked(), Vec::<String>::new(), "not by the rule");
    let t = T0 + 20 * MIN;
    let acts = s.on_event(&hb("W1AW", -5, t), t);
    assert!(
        acts.iter()
            .any(|a| matches!(a, StationAction::HbAckRateLimited { from } if from == "W1AW")),
        "the rule stops it now: {acts:?}"
    );
    assert_eq!(replies(&s), Vec::<String>::new());
    let t = T0 + 25 * MIN;
    s.on_event(
        &ev("W1AW", "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t),
        t,
    );
    assert_eq!(
        replies(&s),
        Vec::<String>::new(),
        "its heartbeat at 20 entered the cache"
    );

    // Turned away first, nothing entered: answered on @ALLCALL a minute after its heartbeat.
    for who in ["W1AW", "N0DEN"] {
        let t = T0 + 60 * MIN;
        let acts = s.on_event(&hb(who, -5, t), t);
        assert!(!held_off_hb(&acts, who), "{who}");
        assert_eq!(
            replies(&s),
            Vec::<String>::new(),
            "{who}: no acknowledgement"
        );
        let t = T0 + 61 * MIN;
        s.on_event(&ev(who, "@ALLCALL", Command::QueryCall, "K1ABC?", -5, t), t);
        assert_eq!(
            replies(&s),
            [format!("{who} YES -12 (1H)")],
            "{who}: answered on @ALLCALL"
        );
        s.drop_queue();
    }
}
