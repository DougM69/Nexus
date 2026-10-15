//! JS8Call 3.0.3's own design where Nexus had departed from it (processCommandActivity.cpp,
//! mainwindow.cpp and Configuration.cpp read as the spec): a group's message asked for again is
//! handed over again; the heard list keeps a call as heard, `/P` included, so a QUERY CALL about a
//! portable call is answered; the 55-minute rule puts the stations it stops on the "never
//! acknowledge" list; a QUERY reading "MSGS n" asks for nothing; and a hand-over keeps its
//! `FROM <call>` however long the held text. Every reply is checked BY VALUE of what keys: the
//! words of its frames, its origin, its offset, its speed and the period it goes in.
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
    for k in 0..200 {
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

/// `text` is what keys, by value: those words, an automatic reply on my own offset at my speed,
/// in the period after `asked_ms`, one frame a period.
fn assert_keys(s: &mut Station, asked_ms: u64, text: &str) {
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
    for (i, (at, f)) in keyed.iter().enumerate() {
        assert_eq!(*at, first + i as u64 * period, "one frame a period");
        assert_eq!(
            (f.origin, f.freq_hint, f.speed),
            (Origin::AutoReply, FreqHint::Dial, speed),
            "an automatic reply, on my offset, at my speed: {text:?}"
        );
    }
}

/// Nothing keys from `from_ms` on.
fn assert_keys_nothing(s: &mut Station, from_ms: u64) {
    let keyed = key_all(s, from_ms);
    assert!(keyed.is_empty(), "keyed {:?}", decoded(&keyed));
}

fn heard_calls(s: &Station) -> Vec<String> {
    s.heard().iter().map(|h| h.call.clone()).collect()
}

// ---- the heard list keeps a call as heard -----------------------------------------------------

/// QUERY CALL about W1AW/P, heard as W1AW/P, is answered with W1AW/P's report, as JS8Call answers
/// it: its call activity keeps the call as heard (`m_callActivity[d.call]`, mainwindow.cpp:
/// 2297-2311) and QUERY CALL matches the call asked about against each key or its base call
/// (processCommandActivity.cpp:1074-1095). Nexus kept W1AW/P as W1AW, and the question went
/// unanswered.
#[test]
fn query_call_about_a_portable_call_is_answered_when_that_call_was_heard() {
    let mut s = Station::new(cfg(Speed::Normal));
    s.on_event(&hb("W1AW/P", -12, T0), T0);
    assert_eq!(heard_calls(&s), ["W1AW/P"], "kept as heard");
    let at = T0 + 5_000;
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW/P?", -7, at),
        at,
    );
    assert_eq!(replies(&s), ["K1ABC YES -12 (NOW)"]);
    assert_keys(&mut s, at, "K1ABC YES -12 (NOW)");
}

/// W1AW and W1AW/P are two stations to JS8Call, each with its own report: a question about W1AW
/// draws W1AW's (first in key order, :1074), one about W1AW/P draws W1AW/P's, and with only W1AW
/// heard a question about W1AW/P draws nothing (W1AW/P is neither W1AW nor its base call), as in
/// JS8Call.
#[test]
fn a_call_and_its_portable_form_are_two_stations_each_with_its_own_report() {
    let mut s = Station::new(cfg(Speed::Normal));
    s.on_event(&hb("W1AW", -3, T0), T0);
    s.on_event(&hb("W1AW/P", -15, T0 + 1_000), T0 + 1_000);
    assert_eq!(heard_calls(&s), ["W1AW", "W1AW/P"]);
    let snr: Vec<i32> = s.heard().iter().map(|h| h.snr_db).collect();
    assert_eq!(snr, [-3, -15], "each its own report");
    // W1AW/P was heard 299 s before the second question: 4M, as JS8Call's since() says it.
    for (asked, reply, at) in [
        ("W1AW?", "K1ABC YES -03 (NOW)", T0 + 5_000),
        ("W1AW/P?", "K1ABC YES -15 (4M)", T0 + 5 * MIN),
    ] {
        s.on_event(
            &ev("K1ABC", "KD9TAW", Command::QueryCall, asked, -7, at),
            at,
        );
        assert_eq!(replies(&s), [reply], "QUERY CALL {asked}");
        assert_keys(&mut s, at, reply);
    }

    let mut s = Station::new(cfg(Speed::Normal));
    s.on_event(&hb("W1AW", -3, T0), T0);
    s.on_event(
        &ev(
            "K1ABC",
            "KD9TAW",
            Command::QueryCall,
            "W1AW/P?",
            -7,
            T0 + 5_000,
        ),
        T0 + 5_000,
    );
    assert!(replies(&s).is_empty(), "W1AW/P not heard: nothing");
    assert_keys_nothing(&mut s, T0 + 15_000);
}

/// A MSG to me from W1AW/P is filed under W1AW/P, as JS8Call stores its sender (`{"FROM",
/// d.from}`, mainwindow.cpp:6356): the call the heard list keeps it under, so the station's row is
/// the one its unread message flags. HEARING? from W1AW/P leaves out W1AW/P alone, JS8Call's
/// `call == d.from` (processCommandActivity.cpp:391), and names W1AW, heard as well.
#[test]
fn a_portable_station_s_message_and_hearing_reply_use_the_call_as_heard() {
    let mut s = Station::new(cfg(Speed::Normal));
    s.on_event(&hb("W1AW", -3, T0), T0);
    s.on_event(
        &ev("W1AW/P", "KD9TAW", Command::Msg, "HELLO", -9, T0 + 1_000),
        T0 + 1_000,
    );
    let mail: Vec<(&str, &str, InboxState)> = s
        .inbox()
        .iter()
        .map(|e| (e.from.as_str(), e.text.as_str(), e.state))
        .collect();
    assert_eq!(mail, [("W1AW/P", "HELLO", InboxState::Unread)]);
    assert_eq!(heard_calls(&s), ["W1AW", "W1AW/P"]);
    assert_keys(&mut s, T0 + 1_000, "W1AW/P ACK");
    let at = T0 + 2 * MIN;
    s.on_event(
        &ev("W1AW/P", "KD9TAW", Command::HearingQuery, "", -9, at),
        at,
    );
    assert_eq!(replies(&s), ["W1AW/P HEARING W1AW"]);
    assert_keys(&mut s, at, "W1AW/P HEARING W1AW");
}

// ---- QUERY "MSGS n" ---------------------------------------------------------------------------

/// A QUERY reading "MSGS 1" asks for nothing: JS8Call takes the text's first word and answers only
/// when it is `MSG` (processCommandActivity.cpp:875-891). "MSG 1" still hands message 1 over.
#[test]
fn a_query_reading_msgs_n_hands_nothing_over() {
    let mut s = Station::new(cfg(Speed::Normal));
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::MsgTo, "K1ABC FRIDAY", -5, T0),
        T0,
    );
    s.drop_queue();
    let at = T0 + MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSGS 1", -7, at), at);
    assert!(replies(&s).is_empty(), "MSGS 1: no reply");
    assert_keys_nothing(&mut s, at);
    assert_eq!(s.inbox()[0].state, InboxState::Store, "still held");
    let at = T0 + 2 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at), at);
    assert_keys(&mut s, at, "K1ABC MSG FRIDAY FROM W1AW");
}

// ---- a hand-over keeps its FROM ---------------------------------------------------------------

/// A held text that fills the reply's cap is cut, not the reply: the hand-over always ends
/// `FROM <call>`, at the speeds where it fits one transmission (Fast, JS8 40, JS8 60). Before, the
/// reply was cut at the cap, and the `FROM KD9TAW` with it.
#[test]
fn a_long_held_message_s_hand_over_keeps_its_from_at_the_fast_speeds() {
    for speed in [Speed::Fast, Speed::Turbo, Speed::Ultra] {
        let mut s = Station::new(cfg(speed));
        let text = "HELLO FROM THE PARK ".repeat(25);
        let text = text.trim();
        assert_eq!(text.len(), 499, "premise: a held text near the cap");
        s.store_message("K1ABC", text, T0).expect("held");
        let at = T0 + MIN;
        s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at), at);
        let head = "K1ABC MSG ";
        let tail = " FROM KD9TAW";
        let want = format!(
            "{head}{}{tail}",
            &text[..MAX_TEXT_LEN - head.len() - tail.len()]
        );
        assert_eq!(want.len(), MAX_TEXT_LEN, "premise: the reply fills the cap");
        assert_eq!(replies(&s), [want.clone()], "{speed:?}");
        assert_keys(&mut s, at, &want);
        assert_eq!(s.inbox()[0].state, InboxState::Delivered, "{speed:?}");
    }
}

/// A hand-over the cap would cut into to name the next message goes without its NEXT MSG ID, as one
/// that fits only without it does (the next one is offered on the next QUERY MSGS), never with the
/// cap taking the end of it.
#[test]
fn a_hand_over_the_cap_would_cut_goes_without_its_next_msg_id() {
    let mut s = Station::new(cfg(Speed::Turbo));
    let text = "HELLO FROM THE PARK ".repeat(24);
    let text = text.trim();
    s.store_message("K1ABC", text, T0).expect("held");
    s.store_message("K1ABC", "SECOND", T0 + 1).expect("held");
    let bare = format!("K1ABC MSG {text} FROM KD9TAW");
    assert!(
        bare.len() < MAX_TEXT_LEN && bare.len() + " NEXT MSG ID 2".len() > MAX_TEXT_LEN,
        "premise: only the NEXT MSG ID runs over the cap: {}",
        bare.len()
    );
    let at = T0 + MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at), at);
    assert_eq!(replies(&s), [bare.clone()]);
    assert_keys(&mut s, at, &bare);
    // Control: a short one still names the next.
    let at = T0 + 3 * MIN;
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 2", -7, at), at);
    assert_eq!(replies(&s), ["K1ABC MSG SECOND FROM KD9TAW"]);
    let mut s = Station::new(cfg(Speed::Turbo));
    s.store_message("K1ABC", "FIRST", T0).expect("held");
    s.store_message("K1ABC", "SECOND", T0 + 1).expect("held");
    s.on_event(&ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at), at);
    assert_eq!(
        replies(&s),
        ["K1ABC MSG FIRST FROM KD9TAW NEXT MSG ID 2"],
        "control: room for it, the next one is named"
    );
}

// ---- the 55-minute rule writes the "never acknowledge" list -----------------------------------

/// The rule puts W1AW on the "never acknowledge" list itself, after the operator's own calls and
/// once, as JS8Call's `addToHbBlacklist` appends it (Configuration.cpp:1226-1231), and the list,
/// not a list of the rule's own, is what turns W1AW away: taken off it, W1AW is acknowledged again.
/// A full list takes no more, and the rule says so in its action as ever.
#[test]
fn the_55_minute_rule_puts_its_stations_on_the_never_acknowledge_list() {
    let on = |deny: Vec<String>| StationConfig {
        hb_networking: true,
        hb_ack: true,
        hb_rate_limit: true,
        hb_ack_deny: deny,
        ..cfg(Speed::Normal)
    };
    let acks = |s: &mut Station, call: &str, at: u64| -> Vec<String> {
        s.on_event(&hb(call, -5, at), at);
        let made = replies(s);
        s.drop_queue();
        made
    };
    let mut s = Station::new(on(vec!["K9ZZZ".into()]));
    assert_eq!(acks(&mut s, "W1AW", T0), ["W1AW HEARTBEAT SNR -05"]);
    let at = T0 + 30 * MIN;
    s.on_event(&hb("W1AW", -5, at), at);
    assert!(replies(&s).is_empty(), "stopped");
    assert_eq!(
        s.config().hb_ack_deny,
        ["K9ZZZ", "W1AW"],
        "on the list, after the operator's own"
    );
    assert_eq!(
        s.hb_rate_blocked(),
        ["W1AW"],
        "the rule's, for the HB ACK chip"
    );
    // The list the engine hands back, with W1AW in it, changes nothing; the operator taking W1AW
    // off it does.
    s.set_config(on(vec!["K9ZZZ".into(), "W1AW".into()]));
    assert!(
        acks(&mut s, "W1AW", T0 + 50 * MIN).is_empty(),
        "still on it"
    );
    s.set_config(on(vec!["K9ZZZ".into()]));
    assert!(s.hb_rate_blocked().is_empty(), "the rule's no more");
    assert_eq!(
        acks(&mut s, "W1AW", T0 + 70 * MIN),
        ["W1AW HEARTBEAT SNR -05"],
        "off the list: acknowledged again"
    );

    // A list already full takes no more; the rule still reports the stop.
    let full: Vec<String> = (0..MAX_HB_RATE).map(|i| format!("K{i}AA")).collect();
    let mut s = Station::new(on(full.clone()));
    acks(&mut s, "W1AW", T0);
    let at = T0 + 30 * MIN;
    let actions = s.on_event(&hb("W1AW", -5, at), at);
    assert_eq!(s.config().hb_ack_deny, full, "full: W1AW not added");
    assert!(
        actions.contains(&StationAction::HbAckRateLimited {
            from: "W1AW".into()
        }),
        "{actions:?}"
    );
}
