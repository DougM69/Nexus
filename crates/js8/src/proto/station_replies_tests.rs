//! The automatic replies and held messages of JS8Call 3.0.3 (processCommandActivity.cpp, Inbox.cpp,
//! UI_Constructor.cpp read as the spec): QUERY CALL, AGN?, 3.0's held-message texts, group
//! store-and-forward, how long a held message stays and what the Inbox is told when a cap drops
//! one, and "Store message…". Every reply is checked BY VALUE of what keys: the words of its
//! frames, its origin, the offset it keys at, its speed and the period it goes in. Then each new
//! reply against each reply gate: ask first, the allow/deny lists, the compose box and a message
//! to me still arriving, AUTO off, and the idle watchdog.
use super::*;
use crate::proto::compose::frames_with_grid;
use crate::proto::crc16::checksum3;
use crate::proto::frame::{decode_word, encode_frame};
use crate::proto::reassembly::Checksum;

const T0: u64 = 10_000_000;
const PERIOD: u64 = 15_000;

/// My station: KD9TAW in EN52 at Normal, in @FUN, replies going by themselves (the confirmation
/// off) so each one keys; JS8Call's other defaults (AUTO and relay on, idle watchdog 60 min).
fn cfg() -> StationConfig {
    StationConfig {
        mycall: "KD9TAW".into(),
        grid: "EN52".into(),
        speed: Speed::Normal,
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

/// W1AW leaves `body` (`<target> <text>`) here with `MSG TO:`; its ACK is taken off the queue.
fn store(s: &mut Station, body: &str, at_ms: u64) {
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::MsgTo, body, -5, at_ms),
        at_ms,
    );
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
    let mut out = Vec::new();
    for k in 0..60 {
        let t = from_ms + k * PERIOD;
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

/// The words KD9TAW keys for `text`, as compose frames it.
fn words(text: &str) -> Vec<Word87> {
    frames_with_grid("KD9TAW", "EN52", None, text, Speed::Normal)
        .expect("composes")
        .iter()
        .map(|(f, i3)| encode_frame(f, *i3, Speed::Normal).expect("packs"))
        .collect()
}

/// What a receiver prints for the frames keyed, run together.
fn decoded(frames: &[(u64, TxFrame)]) -> String {
    frames
        .iter()
        .map(|(_, f)| decode_word(&f.word, f.speed).expect("decodes").0.render())
        .collect()
}

/// `text` is what keys, by value: those words, an automatic reply on my own offset at Normal, in
/// the period after `asked_ms`.
fn assert_keys(s: &mut Station, asked_ms: u64, text: &str) {
    let first = asked_ms / PERIOD * PERIOD + PERIOD;
    let keyed = key_all(s, first);
    let got: Vec<Word87> = keyed.iter().map(|(_, f)| f.word).collect();
    assert_eq!(
        got,
        words(text),
        "keys {text:?}, decoded {:?}",
        decoded(&keyed)
    );
    assert_eq!(keyed[0].0, first, "in the period after the question");
    for (_, f) in &keyed {
        assert_eq!(
            (f.origin, f.freq_hint, f.speed),
            (Origin::AutoReply, FreqHint::Dial, Speed::Normal),
            "an automatic reply, on my offset, at my speed"
        );
    }
}

// ---- QUERY CALL ------------------------------------------------------------------------------

/// JS8Call's `since()`, upper-cased as its compose box upper-cases every reply.
#[test]
fn since_reads_as_js8calls_age_upper_cased() {
    for (secs, age) in [
        (0, "NOW"),
        (14, "NOW"),
        (15, "15S"),
        (59, "45S"),
        (60, "1M"),
        (725, "12M"),
        (3_599, "59M"),
        (3_600, "1H"),
        (86_399, "23H"),
        (86_400, "1D"),
        (200_000, "2D"),
    ] {
        assert_eq!(since(secs), age, "{secs} s");
    }
}

/// The call asked about is the first callsign in the text, as `parseCallsigns` finds it; a
/// locator is not one.
#[test]
fn the_call_asked_about_is_the_first_callsign_in_the_text() {
    for (text, call) in [
        ("W1AW?", Some("W1AW")),
        ("w1aw?", Some("W1AW")),
        ("VE3/W1AW?", Some("VE3/W1AW")),
        ("W1AW/P?", Some("W1AW/P")),
        ("EN52 W1AW?", Some("W1AW")),
        ("?", None),
        ("HELLO?", None),
    ] {
        assert_eq!(first_callsign(text).as_deref(), call, "{text:?}");
    }
}

/// `<from> YES <snr> (<age>)` when the station asked about is heard: the SNR and age it was last
/// heard at, keyed in the next period on my own offset.
#[test]
fn query_call_is_answered_yes_with_the_snr_and_age_of_the_heard_station() {
    let mut s = Station::new(cfg());
    s.on_event(&hb("W1AW", -12, T0), T0);
    let asked = T0 + 12 * 60_000 + 5_000;
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW?", -7, asked),
        asked,
    );
    assert_eq!(replies(&s), ["K1ABC YES -12 (12M)"]);
    assert_keys(&mut s, asked, "K1ABC YES -12 (12M)");
}

/// Nothing is said about a station not heard, or one its callsign aging has passed.
#[test]
fn query_call_about_a_station_not_heard_or_aged_draws_nothing() {
    let mut s = Station::new(cfg());
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW?", -7, T0),
        T0,
    );
    assert!(replies(&s).is_empty(), "W1AW not heard");

    let mut c = cfg();
    c.callsign_aging_min = 10;
    let mut s = Station::new(c);
    s.on_event(&hb("W1AW", -12, T0), T0);
    let late = T0 + 10 * 60_000;
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW?", -7, late),
        late,
    );
    assert!(replies(&s).is_empty(), "aged 10 minutes, aging 10");
    let in_time = T0 + 10 * 60_000 - 1_000;
    let mut s = Station::new(StationConfig {
        callsign_aging_min: 10,
        ..cfg()
    });
    s.on_event(&hb("W1AW", -12, T0), T0);
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW?", -7, in_time),
        in_time,
    );
    assert_eq!(
        replies(&s),
        ["K1ABC YES -12 (9M)"],
        "control: inside the aging"
    );
}

/// JS8Call matches the call asked about against a heard call and its base call, in the heard
/// list's key order: VE3/W1AW comes before W1AW and answers for W1AW.
#[test]
fn query_call_matches_a_heard_call_or_its_base_call_in_key_order() {
    let mut s = Station::new(cfg());
    s.on_event(&hb("W1AW", -20, T0), T0);
    s.on_event(&hb("VE3/W1AW", -3, T0), T0);
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryCall, "W1AW?", -7, T0),
        T0,
    );
    assert_eq!(replies(&s), ["K1ABC YES -03 (NOW)"]);
}

/// QUERY CALL is answered on @ALLCALL and on a group I joined, and nowhere else; on @ALLCALL once
/// per asking station per 15 minutes, as JS8Call's @ALLCALL cache holds it off.
#[test]
fn query_call_on_allcall_is_answered_once_per_station_per_interval() {
    for (to, answered) in [("@FUN", true), ("@OTHER", false), ("N0XYZ", false)] {
        let mut s = Station::new(cfg());
        s.on_event(&hb("W1AW", -12, T0), T0);
        s.on_event(&ev("K1ABC", to, Command::QueryCall, "W1AW?", -7, T0), T0);
        assert_eq!(!replies(&s).is_empty(), answered, "QUERY CALL to {to}");
    }
    let mut s = Station::new(cfg());
    s.on_event(&hb("W1AW", -12, T0), T0);
    let asks = |s: &mut Station, at: u64| {
        let acts = s.on_event(
            &ev("K1ABC", "@ALLCALL", Command::QueryCall, "W1AW?", -7, at),
            at,
        );
        let limited = acts
            .iter()
            .any(|a| matches!(a, StationAction::RateLimited { from } if from == "K1ABC"));
        let made = replies(s);
        s.drop_queue();
        (made, limited)
    };
    assert_eq!(
        asks(&mut s, T0),
        (vec!["K1ABC YES -12 (NOW)".to_string()], false)
    );
    assert_eq!(asks(&mut s, T0 + 60_000), (vec![], true), "held off");
    assert_eq!(
        asks(&mut s, T0 + 15 * 60_000),
        (vec!["K1ABC YES -12 (15M)".to_string()], false),
        "15 minutes on"
    );
}

// ---- AGN? ------------------------------------------------------------------------------------

/// AGN? sends my last message again: the same words, on my offset, in the next period.
#[test]
fn agn_sends_my_last_message_again() {
    let mut s = Station::new(cfg());
    s.send_command(&CallRef::Base("W1AW".into()), Command::Snr, "-05", T0)
        .expect("composes");
    let first = key_all(&mut s, T0 + PERIOD);
    assert_eq!(decoded(&first), "KD9TAW: W1AW SNR -05 ");
    let asked = T0 + 5 * PERIOD;
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::AgnQuery, "", -9, asked),
        asked,
    );
    assert_eq!(replies(&s), ["W1AW SNR -05"]);
    assert_keys(&mut s, asked, "W1AW SNR -05");
    let first_words: Vec<Word87> = first.iter().map(|(_, f)| f.word).collect();
    assert_eq!(
        first_words,
        words("W1AW SNR -05"),
        "the same words as the first time"
    );
    // Only the LAST message: each message's first frame starts it afresh.
    s.send_command(
        &CallRef::Base("W1AW".into()),
        Command::HwCpyQuery,
        "",
        T0 + 20 * PERIOD,
    )
    .expect("composes");
    key_all(&mut s, T0 + 21 * PERIOD);
    let again = T0 + 25 * PERIOD;
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::AgnQuery, "", -9, again),
        again,
    );
    assert_eq!(
        replies(&s),
        ["W1AW HW CPY?"],
        "the last message, not the two"
    );
}

/// …the whole message as a receiver decoded it, the checksum it carried included, as JS8Call
/// sends `m_lastTxMessage` again; and the leading `KD9TAW:` its compose box would hold drops out
/// as compose drops it, so the frames are the ones that text would key.
#[test]
fn agn_sends_a_multi_frame_message_again_as_it_was_decoded() {
    let mut s = Station::new(cfg());
    s.send_command(
        &CallRef::Base("W1AW".into()),
        Command::Msg,
        "HELLO THERE",
        T0,
    )
    .expect("composes");
    let first = key_all(&mut s, T0 + PERIOD);
    assert!(first.len() > 1, "control: more than one frame");
    let heard = decoded(&first);
    let again = format!("W1AW MSG HELLO THERE {}", checksum3("HELLO THERE"));
    assert_eq!(heard.trim_end(), format!("KD9TAW: {again}"));
    let asked = T0 + 10 * PERIOD;
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::AgnQuery, "", -9, asked),
        asked,
    );
    assert_eq!(replies(&s), std::slice::from_ref(&again));
    assert_eq!(
        words(&format!("KD9TAW: {again}")),
        words(&again),
        "with the prefix or without it, compose keys the same words"
    );
    assert_keys(&mut s, asked, &again);
}

/// A heartbeat sent last goes again as one, on my offset: an automatic reply, not a beacon.
#[test]
fn agn_after_a_heartbeat_sends_the_heartbeat_frame_on_my_offset() {
    // A heartbeat goes out only with heartbeat networking on, as in JS8Call.
    let mut c = cfg();
    c.hb_networking = true;
    let mut s = Station::new(c);
    s.heartbeat_now(T0).expect("composes");
    let first = key_all(&mut s, T0 + PERIOD);
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0].1.freq_hint, FreqHint::HbSubband(_)));
    let asked = T0 + 4 * PERIOD;
    s.on_event(
        &ev("W1AW", "KD9TAW", Command::AgnQuery, "", -9, asked),
        asked,
    );
    assert_eq!(replies(&s), ["@HB HEARTBEAT EN52"]);
    let again = key_all(&mut s, asked / PERIOD * PERIOD + PERIOD);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].1.word, first[0].1.word, "the heartbeat's own word");
    assert_eq!(
        (again[0].1.origin, again[0].1.freq_hint),
        (Origin::AutoReply, FreqHint::Dial)
    );
}

/// No AGN? reply with nothing sent, nor to @ALLCALL, a group or another station.
#[test]
fn agn_is_answered_only_to_my_call_and_only_once_i_have_sent_something() {
    let mut s = Station::new(cfg());
    s.on_event(&ev("W1AW", "KD9TAW", Command::AgnQuery, "", -9, T0), T0);
    assert!(replies(&s).is_empty(), "nothing sent yet");
    for to in ["@ALLCALL", "@FUN", "N0XYZ"] {
        let mut s = Station::new(cfg());
        s.send_command(&CallRef::Base("W1AW".into()), Command::Snr, "-05", T0)
            .expect("composes");
        key_all(&mut s, T0 + PERIOD);
        s.on_event(
            &ev("W1AW", to, Command::AgnQuery, "", -9, T0 + 3 * PERIOD),
            T0 + 3 * PERIOD,
        );
        assert!(replies(&s).is_empty(), "AGN? to {to}");
    }
}

// ---- 3.0's held-message texts ----------------------------------------------------------------

/// QUERY MSGS is answered `YES MSG ID n +k`, k the count past n; QUERY MSG n is answered with the
/// message, `NEXT MSG ID m` and `+k`, counted before it is marked; each delivery taken in turn.
#[test]
fn held_message_replies_read_as_js8call_3() {
    let mut s = Station::new(cfg());
    for (i, body) in ["K1ABC T1", "K1ABC T2", "K1ABC T3"].iter().enumerate() {
        store(&mut s, body, T0 + i as u64);
    }
    let ask = |s: &mut Station, cmd: Command, text: &str, at: u64| -> Vec<String> {
        s.on_event(&ev("K1ABC", "KD9TAW", cmd, text, -5, at), at);
        let made = replies(s);
        key_all(s, at / PERIOD * PERIOD + PERIOD);
        made
    };
    let t = T0 + 60_000;
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t),
        ["K1ABC YES MSG ID 1 +2"]
    );
    assert_eq!(
        ask(&mut s, Command::Query, "MSG 1", t + 60_000),
        ["K1ABC MSG T1 FROM W1AW NEXT MSG ID 2 +1"]
    );
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t + 120_000),
        ["K1ABC YES MSG ID 2 +1"]
    );
    assert_eq!(
        ask(&mut s, Command::Query, "MSG 2", t + 180_000),
        ["K1ABC MSG T2 FROM W1AW NEXT MSG ID 3"]
    );
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t + 240_000),
        ["K1ABC YES MSG ID 3"]
    );
    assert_eq!(
        ask(&mut s, Command::Query, "MSG 3", t + 300_000),
        ["K1ABC MSG T3 FROM W1AW"]
    );
    assert_eq!(
        ask(&mut s, Command::QueryMsgs, "", t + 360_000),
        ["K1ABC NO"]
    );
}

/// The heartbeat's acknowledgement, 3.0's form: `MSG ID n +k)`, the parenthesis unopened as
/// JS8Call 3.0.3 keys it; and the count is of the call AS HEARD, so K1ABC/P is told of the
/// message held for K1ABC and of no more.
#[test]
fn the_heartbeat_ack_says_how_many_more_wait_as_js8call_3_does() {
    let mut c = cfg();
    c.hb_ack = true;
    // An acknowledgement goes out only with heartbeat networking on, as in JS8Call.
    c.hb_networking = true;
    // A periodic heartbeat an hour off, so only the acknowledgement keys meanwhile.
    c.hb_interval_min = 60;
    let mut s = Station::new(c.clone());
    s.set_hb(true, T0);
    for (i, body) in ["K1ABC T1", "K1ABC T2", "K1ABC T3"].iter().enumerate() {
        store(&mut s, body, T0 + i as u64);
    }
    s.on_event(&hb("K1ABC", -5, T0 + 1_000), T0 + 1_000);
    assert_eq!(replies(&s), ["K1ABC HEARTBEAT SNR -05 MSG ID 1 +2)"]);
    let keyed = key_all(&mut s, T0 + PERIOD);
    assert_eq!(
        keyed.iter().map(|(_, f)| f.word).collect::<Vec<_>>(),
        words("K1ABC HEARTBEAT SNR -05 MSG ID 1 +2)"),
        "decoded {:?}",
        decoded(&keyed)
    );
    assert!(matches!(keyed[0].1.freq_hint, FreqHint::HbSubband(_)));
    s.on_event(&hb("K1ABC/P", -5, T0 + 600_000), T0 + 600_000);
    assert_eq!(replies(&s), ["K1ABC/P HEARTBEAT SNR -05 MSG ID 1"]);

    let mut s = Station::new(c);
    s.set_hb(true, T0);
    store(&mut s, "K1ABC T1", T0);
    s.on_event(&hb("K1ABC", -5, T0 + 1_000), T0 + 1_000);
    assert_eq!(
        replies(&s),
        ["K1ABC HEARTBEAT SNR -05 MSG ID 1"],
        "one: no count"
    );
}

// ---- group store-and-forward ------------------------------------------------------------------

/// A `MSG TO:@FUN` is offered to a station that asks the group, handed over when it asks for it,
/// and then offered to it no more, while another station still gets it: offered once each. A
/// station that asks for it by number again is handed it again, as JS8Call 3.0.3 hands it
/// (processCommandActivity.cpp:905-935: only the `@` is checked), and the record of who had it
/// keeps one row for it.
#[test]
fn a_group_message_is_offered_once_to_each_station_and_handed_over_whenever_asked() {
    let mut s = Station::new(cfg());
    store(&mut s, "@FUN NET AT EIGHT", T0);
    assert_eq!(s.inbox()[0].to, "@FUN");
    let ask = |s: &mut Station, from: &str, to: &str, cmd: Command, text: &str, at: u64| {
        s.on_event(&ev(from, to, cmd, text, -5, at), at);
        let made = replies(s);
        key_all(s, at / PERIOD * PERIOD + PERIOD);
        made
    };
    let t = T0 + 60_000;
    assert_eq!(
        ask(&mut s, "K1ABC", "@FUN", Command::QueryMsgs, "", t),
        ["K1ABC YES MSG ID 1"]
    );
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t + 60_000),
        t + 60_000,
    );
    assert_eq!(replies(&s), ["K1ABC MSG NET AT EIGHT FROM W1AW"]);
    assert_keys(&mut s, t + 60_000, "K1ABC MSG NET AT EIGHT FROM W1AW");
    assert_eq!(s.group_recipients, [(1, "K1ABC".to_string())]);
    assert_eq!(
        s.inbox()[0].state,
        InboxState::Store,
        "still held for the group"
    );
    assert_eq!(
        ask(&mut s, "K1ABC", "@FUN", Command::QueryMsgs, "", t + 300_000),
        ["K1ABC NO"]
    );
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, t + 360_000),
        t + 360_000,
    );
    assert_eq!(
        replies(&s),
        ["K1ABC MSG NET AT EIGHT FROM W1AW"],
        "asked for again, handed to K1ABC again"
    );
    assert_keys(&mut s, t + 360_000, "K1ABC MSG NET AT EIGHT FROM W1AW");
    assert_eq!(
        s.group_recipients,
        [(1, "K1ABC".to_string())],
        "one row for K1ABC, however often it is handed over"
    );
    assert_eq!(
        ask(&mut s, "K1ABC", "@FUN", Command::QueryMsgs, "", t + 400_000),
        ["K1ABC NO"],
        "and still offered to K1ABC no more"
    );
    assert_eq!(
        ask(&mut s, "N0XYZ", "@FUN", Command::QueryMsgs, "", t + 420_000),
        ["N0XYZ YES MSG ID 1"]
    );
}

/// A group's message is offered only to a query addressed to that group, and only when I joined
/// it: not to a query to my call, nor on @ALLCALL; and a message held for the asker itself comes
/// first, with the group's counted in the `+k` of a query to the group.
#[test]
fn a_group_message_is_offered_on_the_groups_own_query() {
    let mut s = Station::new(cfg());
    store(&mut s, "@FUN NET AT EIGHT", T0);
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, T0 + 1),
        T0 + 1,
    );
    assert_eq!(replies(&s), ["K1ABC NO"], "to my call");
    s.drop_queue();
    s.on_event(
        &ev("K1ABC", "@ALLCALL", Command::QueryMsgs, "", -5, T0 + 2),
        T0 + 2,
    );
    assert!(replies(&s).is_empty(), "on @ALLCALL");

    let mut s = Station::new(StationConfig {
        groups: Vec::new(),
        ..cfg()
    });
    store(&mut s, "@FUN NET AT EIGHT", T0);
    s.on_event(
        &ev("K1ABC", "@FUN", Command::QueryMsgs, "", -5, T0 + 1),
        T0 + 1,
    );
    assert!(replies(&s).is_empty(), "a group I did not join");

    let mut s = Station::new(cfg());
    store(&mut s, "K1ABC MINE", T0);
    store(&mut s, "@FUN NET AT EIGHT", T0 + 1);
    s.on_event(
        &ev("K1ABC", "@FUN", Command::QueryMsgs, "", -5, T0 + 2),
        T0 + 2,
    );
    assert_eq!(
        replies(&s),
        ["K1ABC YES MSG ID 1 +1"],
        "its own first, the group's counted"
    );
    s.drop_queue();
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, T0 + 3),
        T0 + 3,
    );
    assert_eq!(
        replies(&s),
        ["K1ABC YES MSG ID 1"],
        "to my call the group's is not counted"
    );
    s.drop_queue();
    s.on_event(
        &ev("K1ABC", "@FUN", Command::Query, "MSG 2", -5, T0 + 4),
        T0 + 4,
    );
    assert_eq!(
        replies(&s),
        ["K1ABC MSG NET AT EIGHT FROM W1AW NEXT MSG ID 1"],
        "asked of the group: the next one named"
    );
}

/// Two days: a group's message is offered until 48 h after it was stored, then dropped.
#[test]
fn a_group_message_is_offered_for_48_hours() {
    let mut s = Station::new(cfg());
    store(&mut s, "@FUN NET AT EIGHT", T0);
    let edge = T0 + held::GROUP_HOLD_MS;
    s.on_event(
        &ev("K1ABC", "@FUN", Command::QueryMsgs, "", -5, edge - 1),
        edge - 1,
    );
    assert_eq!(replies(&s), ["K1ABC YES MSG ID 1"]);
    s.drop_queue();
    s.on_event(&ev("N0XYZ", "@FUN", Command::QueryMsgs, "", -5, edge), edge);
    assert_eq!(replies(&s), ["N0XYZ NO"]);
    s.tick(edge);
    assert!(s.inbox().is_empty(), "dropped at 48 h");
}

/// A heartbeat is offered a group's message when its destination is a group I joined: a heartbeat
/// is addressed to @HB, so a station that joined @HB offers the messages left for @HB.
#[test]
fn a_heartbeat_is_offered_the_message_of_a_group_it_is_addressed_to() {
    for (groups, said) in [
        (vec!["@HB".to_string()], "K1ABC HEARTBEAT SNR -05 MSG ID 1"),
        (Vec::new(), "K1ABC HEARTBEAT SNR -05"),
    ] {
        let mut c = cfg();
        c.hb_ack = true;
        c.hb_networking = true;
        c.groups = groups;
        let mut s = Station::new(c);
        s.set_hb(true, 0);
        store(&mut s, "@HB NET AT EIGHT", T0);
        s.on_event(&hb("K1ABC", -5, T0 + 1), T0 + 1);
        assert_eq!(replies(&s), [said]);
    }
}

/// A No leaves the group's message unhanded; the record of who had it survives a restart.
#[test]
fn a_group_handover_waits_for_the_yes_and_is_journaled() {
    let mut s = Station::new(StationConfig {
        autoreply_confirmation: true,
        ..cfg()
    });
    store(&mut s, "@FUN NET AT EIGHT", T0);
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, T0 + 1),
        T0 + 1,
    );
    let p = s.pending_reply().expect("asks");
    s.answer_reply(&p.display, p.fires_at_ms, false, T0 + 2);
    assert!(s.group_recipients.is_empty(), "a No hands nothing over");
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, T0 + 3),
        T0 + 3,
    );
    let p = s.pending_reply().expect("asks again");
    s.answer_reply(&p.display, p.fires_at_ms, true, T0 + 4);
    assert_eq!(s.group_recipients, [(1, "K1ABC".to_string())]);
    let json = serde_json::to_string(&s.snapshot(T0 + 5)).unwrap();
    let mut back = Station::new(cfg());
    back.restore(serde_json::from_str(&json).unwrap(), T0 + 6);
    assert_eq!(back.group_recipients, [(1, "K1ABC".to_string())]);
}

// ---- how long a held message stays, and what a cap says ---------------------------------------

/// Over the count cap, mail held for others drops one already handed over before the oldest
/// still waiting, and the Inbox is told which and why.
#[test]
fn a_cap_drops_a_delivered_message_first_and_the_inbox_says_which_and_why() {
    let mut s = Station::new(cfg());
    for i in 0..100u64 {
        store(&mut s, &format!("T{i:04}X HELLO {i}"), T0 + i);
    }
    let id = s
        .inbox()
        .iter()
        .find(|e| e.to == "T0049X")
        .map(|e| e.id)
        .unwrap();
    s.on_event(
        &ev(
            "T0049X",
            "KD9TAW",
            Command::Query,
            &format!("MSG {id}"),
            -5,
            T0 + 200,
        ),
        T0 + 200,
    );
    key_all(&mut s, T0 + PERIOD);
    assert!(s.inbox_dropped().is_empty());
    store(&mut s, "T0100X HELLO 100", T0 + 1_000_000);
    let notes: Vec<(String, InboxState, DropReason, u64)> = s
        .inbox_dropped()
        .iter()
        .map(|d| (d.entry.to.clone(), d.entry.state, d.why, d.at_ms))
        .collect();
    assert_eq!(
        notes,
        [(
            "T0049X".to_string(),
            InboxState::Delivered,
            DropReason::Count,
            T0 + 1_000_000
        )],
        "the delivered one went, not the oldest"
    );
    store(&mut s, "T0101X HELLO 101", T0 + 1_000_001);
    assert_eq!(
        s.inbox_dropped()[1].entry.to,
        "T0000X",
        "then the oldest still waiting"
    );
    assert!(s
        .inbox()
        .iter()
        .all(|e| e.to != "T0000X" && e.to != "T0049X"));
    assert!(s.clear_inbox_dropped());
    assert!(s.inbox_dropped().is_empty());
    assert!(!s.clear_inbox_dropped());
}

/// The byte cap and my own mail's cap are said too; the Inbox keeps the last ten notes, and a
/// restart keeps them. (A `MSG TO:` text is clamped to 512 bytes, so a hundred held rows stay
/// under 64 KB: the byte cap is reached here with rows a journal from elsewhere could hold.)
#[test]
fn the_inbox_notes_name_the_cap_and_keep_the_last_ten() {
    let mut s = Station::new(cfg());
    for i in 0..80u32 {
        s.inbox.push(InboxEntry {
            id: i + 1,
            from: "W1AW".into(),
            to: format!("T{i:04}X"),
            text: "Y".repeat(1_000),
            path: Vec::new(),
            state: InboxState::Store,
            at_ms: T0 + u64::from(i),
            freq_hz: 1500.0,
            snr_db: -5,
        });
    }
    let mut acts = Vec::new();
    s.prune_inbox(T0 + 100, &mut acts);
    let bytes: usize = s
        .inbox()
        .iter()
        .map(|e| e.from.len() + e.to.len() + e.text.len())
        .sum();
    assert!(bytes <= MAX_INBOX_BYTES, "back under the cap: {bytes}");
    assert!(
        s.inbox().len() < 70,
        "control: the byte cap dropped rows, not the count cap"
    );
    assert!(s.inbox_dropped().iter().all(|d| d.why == DropReason::Bytes));
    assert_eq!(s.inbox_dropped().len(), MAX_DROP_NOTES, "the last ten");
    assert_eq!(
        s.inbox_dropped().last().map(|d| d.entry.to.as_str()),
        Some(format!("T{:04}X", 80 - s.inbox().len() - 1).as_str()),
        "oldest first: the last note is the newest of those dropped"
    );

    let mut s = Station::new(cfg());
    for i in 0..101u64 {
        s.on_event(
            &ev(
                "N0XYZ",
                "KD9TAW",
                Command::Msg,
                &format!("HI {i}"),
                -5,
                T0 + i,
            ),
            T0 + i,
        );
    }
    s.drop_queue();
    let d = &s.inbox_dropped()[0];
    assert_eq!(
        (d.entry.text.as_str(), d.entry.state, d.why),
        ("HI 0", InboxState::Unread, DropReason::Count)
    );
    let json = serde_json::to_string(&s.snapshot(T0)).unwrap();
    let mut back = Station::new(cfg());
    back.restore(serde_json::from_str(&json).unwrap(), T0);
    assert_eq!(back.inbox_dropped(), s.inbox_dropped());
}

// ---- Store message… ---------------------------------------------------------------------------

/// The operator leaves a message here for K1ABC: held as from my call, offered when K1ABC asks,
/// and handed over as JS8Call hands it, `<to> MSG <text> FROM KD9TAW`.
#[test]
fn a_stored_message_is_held_from_me_and_handed_over_when_asked() {
    let mut s = Station::new(cfg());
    let acts = s
        .store_message("k1abc", "  see you\tfriday\n", T0)
        .expect("stored");
    assert!(acts.contains(&StationAction::InboxChanged));
    let e = &s.inbox()[0];
    assert_eq!(
        (e.from.as_str(), e.to.as_str(), e.text.as_str(), e.state),
        ("KD9TAW", "K1ABC", "SEE YOU FRIDAY", InboxState::Store)
    );
    assert_eq!(s.outbox_len() + s.pending_len(), 0, "keys nothing");
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, T0 + 1),
        T0 + 1,
    );
    assert_eq!(replies(&s), ["K1ABC YES MSG ID 1"]);
    key_all(&mut s, T0 + PERIOD);
    let asked = T0 + 100_000;
    s.on_event(
        &ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -5, asked),
        asked,
    );
    assert_eq!(replies(&s), ["K1ABC MSG SEE YOU FRIDAY FROM KD9TAW"]);
    assert_keys(&mut s, asked, "K1ABC MSG SEE YOU FRIDAY FROM KD9TAW");
    assert_eq!(s.inbox()[0].state, InboxState::Delivered);
}

/// Held for the call as given, as JS8Call holds it for the call selected: VE3/K1ABC's is found
/// for VE3/K1ABC.
#[test]
fn a_stored_message_for_a_compound_call_is_held_for_that_call() {
    let mut s = Station::new(cfg());
    s.store_message("VE3/K1ABC", "HI", T0).expect("stored");
    s.on_event(
        &ev("VE3/K1ABC", "KD9TAW", Command::QueryMsgs, "", -5, T0 + 1),
        T0 + 1,
    );
    assert_eq!(replies(&s), ["VE3/K1ABC YES MSG ID 1"]);
}

/// Refused, with nothing stored: no call of mine, no station (none, @ALLCALL, @HB), no text, and
/// a delivery too long to go in one transmission.
#[test]
fn store_message_refuses_what_could_never_be_handed_over() {
    let mut s = Station::new(cfg());
    for (to, text, why) in [
        ("", "HI", StoreRefusal::NotAStation),
        ("@ALLCALL", "HI", StoreRefusal::NotAStation),
        ("@HB", "HI", StoreRefusal::NotAStation),
        ("HELLO", "HI", StoreRefusal::NotAStation),
        ("K1ABC", " \t ", StoreRefusal::Empty),
        ("K1ABC", "ÉÉ", StoreRefusal::Empty),
    ] {
        assert_eq!(s.store_message(to, text, T0), Err(why), "{to:?} {text:?}");
    }
    let mut slow = Station::new(StationConfig {
        speed: Speed::Slow,
        ..cfg()
    });
    let long = "QXZJ7 ".repeat(80);
    assert!(matches!(
        slow.store_message("K1ABC", &long, T0),
        Err(StoreRefusal::TooLong { .. })
    ));
    let mut nobody = Station::new(StationConfig {
        mycall: String::new(),
        ..cfg()
    });
    assert_eq!(
        nobody.store_message("K1ABC", "HI", T0),
        Err(StoreRefusal::NoCallsign)
    );
    assert!(s.inbox().is_empty() && slow.inbox().is_empty() && nobody.inbox().is_empty());
    assert!(
        s.store_message("@FUN", "HI ALL", T0).is_ok(),
        "control: a group's"
    );
}

// ---- each new reply against each reply gate ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Reply {
    QueryCall,
    QueryCallAllcall,
    Agn,
    GroupOffer,
    GroupDelivery,
    StoredDelivery,
}

const REPLIES: [Reply; 6] = [
    Reply::QueryCall,
    Reply::QueryCallAllcall,
    Reply::Agn,
    Reply::GroupOffer,
    Reply::GroupDelivery,
    Reply::StoredDelivery,
];

/// A station ready to make `r` once its question arrives, the question, and the reply.
fn ready(r: Reply, c: StationConfig) -> (Station, MessageEvent, &'static str) {
    let mut s = Station::new(c);
    s.mark_active(T0);
    let at = T0 + 60_000;
    match r {
        Reply::QueryCall | Reply::QueryCallAllcall => {
            s.on_event(&hb("W1AW", -12, T0), T0);
            let to = if r == Reply::QueryCall {
                "KD9TAW"
            } else {
                "@ALLCALL"
            };
            (
                s,
                ev("K1ABC", to, Command::QueryCall, "W1AW?", -7, at),
                "K1ABC YES -12 (1M)",
            )
        }
        Reply::Agn => {
            s.send_command(&CallRef::Base("K1ABC".into()), Command::Snr, "-05", T0)
                .expect("composes");
            key_all(&mut s, T0);
            (
                s,
                ev("K1ABC", "KD9TAW", Command::AgnQuery, "", -7, at),
                "K1ABC SNR -05",
            )
        }
        Reply::GroupOffer => {
            store(&mut s, "@FUN NET AT EIGHT", T0);
            (
                s,
                ev("K1ABC", "@FUN", Command::QueryMsgs, "", -7, at),
                "K1ABC YES MSG ID 1",
            )
        }
        Reply::GroupDelivery => {
            store(&mut s, "@FUN NET AT EIGHT", T0);
            (
                s,
                ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at),
                "K1ABC MSG NET AT EIGHT FROM W1AW",
            )
        }
        Reply::StoredDelivery => {
            s.store_message("K1ABC", "SEE YOU FRIDAY", T0)
                .expect("stored");
            (
                s,
                ev("K1ABC", "KD9TAW", Command::Query, "MSG 1", -7, at),
                "K1ABC MSG SEE YOU FRIDAY FROM KD9TAW",
            )
        }
    }
}

fn at(e: &MessageEvent) -> u64 {
    match e {
        MessageEvent::Message(m) => m.last_ms,
        MessageEvent::Frame(_) => 0,
    }
}

/// Control for every case below: with no gate in the way, each reply keys, by value.
#[test]
fn each_new_reply_keys_with_no_gate_in_the_way() {
    for r in REPLIES {
        let (mut s, q, said) = ready(r, cfg());
        s.on_event(&q, at(&q));
        assert_eq!(replies(&s), [said], "{r:?}");
        assert_keys(&mut s, at(&q), said);
    }
}

/// C8: with the confirmation on (JS8Call's default) each asks first; nothing keys before the
/// Yes, and a No keys nothing.
#[test]
fn each_new_reply_asks_first_and_a_no_keys_nothing() {
    for r in REPLIES {
        let (mut s, q, said) = ready(
            r,
            StationConfig {
                autoreply_confirmation: true,
                ..cfg()
            },
        );
        s.on_event(&q, at(&q));
        let p = s.pending_reply().unwrap_or_else(|| panic!("{r:?} asks"));
        assert_eq!(p.display, format!("KD9TAW: {said}"), "{r:?}");
        assert!(
            key_all(&mut s, at(&q)).is_empty(),
            "{r:?}: nothing before the Yes"
        );
        s.answer_reply(&p.display, p.fires_at_ms, false, at(&q) + 1);
        assert!(
            key_all(&mut s, at(&q)).is_empty(),
            "{r:?}: a No keys nothing"
        );
    }
}

/// C9: a station the lists keep out is answered in no way.
#[test]
fn each_new_reply_obeys_the_allow_and_deny_lists() {
    for r in REPLIES {
        for c in [
            StationConfig {
                autoreply_deny: vec!["K1ABC".into()],
                ..cfg()
            },
            StationConfig {
                autoreply_allow: vec!["N0XYZ".into()],
                ..cfg()
            },
        ] {
            let (mut s, q, _) = ready(r, c);
            s.on_event(&q, at(&q));
            assert!(replies(&s).is_empty(), "{r:?}");
            assert!(key_all(&mut s, at(&q)).is_empty(), "{r:?}");
        }
    }
}

/// C10: no reply while the compose box holds text, or while a message to me is still arriving.
#[test]
fn each_new_reply_waits_for_the_compose_box_and_the_receive_buffers() {
    for r in REPLIES {
        for busy in ["box", "arriving"] {
            let (mut s, q, _) = ready(r, cfg());
            if busy == "box" {
                s.sync_composer(true, None);
            } else {
                s.note_rx_buffers(true, ["KD9TAW"]);
            }
            let acts = s.on_event(&q, at(&q));
            assert!(replies(&s).is_empty(), "{r:?} while {busy}");
            assert!(
                acts.iter().any(|a| matches!(a, StationAction::Toast { text, .. } if text.starts_with("Auto-reply not made"))),
                "{r:?} while {busy}: said"
            );
            s.sync_composer(false, None);
            s.note_rx_buffers(false, []);
            assert!(key_all(&mut s, at(&q)).is_empty(), "{r:?} while {busy}");
        }
    }
}

/// C11: with AUTO off nothing keys by itself. JS8Call makes no QUERY CALL or QUERY MSGS reply at
/// all then (both ask AUTO, processCommandActivity.cpp:983, :1053); AGN? and a delivery go to the
/// compose box for the operator to send.
#[test]
fn each_new_reply_with_auto_off_keys_nothing_and_goes_to_the_compose_box_as_js8call() {
    for r in REPLIES {
        let (mut s, q, said) = ready(
            r,
            StationConfig {
                autoreply: false,
                ..cfg()
            },
        );
        s.on_event(&q, at(&q));
        assert!(
            key_all(&mut s, at(&q)).is_empty(),
            "{r:?} keyed with AUTO off"
        );
        let (_, offered) = s.sync_composer(false, None);
        let expect = match r {
            Reply::QueryCall | Reply::QueryCallAllcall | Reply::GroupOffer => None,
            Reply::Agn | Reply::GroupDelivery | Reply::StoredDelivery => Some(said.to_string()),
        };
        assert_eq!(offered.map(|(_, t)| t), expect, "{r:?}");
    }
}

/// C12: while the idle watchdog stands nothing is answered.
#[test]
fn each_new_reply_is_held_by_the_idle_watchdog() {
    for r in REPLIES {
        let (mut s, q, _) = ready(r, cfg());
        let trip = T0 + 61 * 60_000;
        s.tick(trip);
        assert!(s.idle_tripped(), "{r:?}: tripped");
        let late = MessageEvent::Message(match &q {
            MessageEvent::Message(m) => Message {
                last_ms: trip + 1_000,
                ..m.clone()
            },
            MessageEvent::Frame(_) => unreachable!(),
        });
        s.on_event(&late, trip + 1_000);
        assert!(replies(&s).is_empty(), "{r:?}");
        assert!(key_all(&mut s, trip + 1_000).is_empty(), "{r:?}");
    }
}
