//! The voice keyer in Phone at the shack through Nexus's own Flex client with native DAX audio on
//! (operator ruling, 2026-10-12, "Change it now", and the transmit text the operator signed that
//! day): between messages the radio takes its transmit audio from its own mic, and for each message
//! Nexus asks it for DAX, keys only once the radio reports it, plays the message over DAX, unkeys at
//! its end and gives the mic back only after the radio has proved the unkey. A message that cannot
//! go out is dropped and says why, never played later; Stop TX ends one at once; a session lost
//! mid-message alarms, and the next session unkeys before the mic comes back; SSTV and APRS stay
//! refused; the mic's own PTT during a message keeps today's alarm until a bench shows what the
//! radio reports for it.
//!
//! The radio is the SmartSDR simulator and the loop the real `RadioLoop::step`, as in the parent
//! module. What the radio saw is read by value, in its order ([`voice_trace`]).

use super::*;

/// Phone at the shack on `session`, settled: the transmit slice in USB with native audio on, so the
/// radio takes its transmit audio from its own mic, and nothing of ours keyed.
fn shack(session: SimSession) -> FlexScene {
    let mut s = FlexScene::with_session(true, session);
    shack_on(&mut s);
    s
}

/// Put `s` in Phone at the shack and wait until it has settled there.
fn shack_on(s: &mut FlexScene) {
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until(s, "the radio never had the mic", has_mic);
    run_until(s, "the client never settled", client_ready);
}

/// Phone at the shack on the bundled radio, which reports the slice's mode back.
fn phone() -> FlexScene {
    shack(reports(&["USB"]))
}

/// `session` with `command` answered with success and then `items`, in place of its own rule.
fn with_rule(mut session: SimSession, command: &str, items: Vec<Item>) -> SimSession {
    let rule = (
        Pattern::Exact(command.to_string()),
        vec![Rule {
            code: "0".to_string(),
            message: String::new(),
            items,
        }],
    );
    session.rules.retain(|(p, _)| *p != rule.0);
    session.rules.push(rule);
    session
}

fn send(line: &str) -> Item {
    Item::Send(line.to_string())
}

/// The interlock line for `state`, keyed by `source` under `handle` (`{h}`: our own).
fn interlock(handle: &str, state: &str, source: &str) -> String {
    format!(
        "S0|interlock tx_client_handle=0x{handle} state={state} reason= source={source} \
         tx_allowed=1 amplifier="
    )
}

/// The radio letting go of the transmitter: READY naming no client.
const IDLE: &str =
    "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";

/// The bundled radio reporting its DAX source `ms` after Nexus writes it, instead of at once.
fn slow_dax(ms: u64) -> SimSession {
    with_rule(
        reports(&["USB"]),
        "transmit set dax=1",
        vec![Item::Wait(ms), send("S0|transmit dax=1")],
    )
}

/// The radio's own word for its DAX source in a transmit status line: `dax=1` or `dax=0`.
fn dax_word(line: &str) -> Option<&str> {
    line.split_whitespace().find(|t| t.starts_with("dax="))
}

/// What the radio saw from `since` on, over TCP and so in one order: Nexus's DAX transmit stream,
/// the DAX source written and the radio's report of it, the keying commands, and the interlock's
/// `idle` (READY naming no client).
fn voice_trace(s: &FlexScene, since: Instant) -> Vec<String> {
    s.log()
        .into_iter()
        .filter(|(at, _)| *at >= since)
        .filter_map(|(_, e)| match e {
            SimEvent::Command { text, .. }
                if text.starts_with("transmit set dax")
                    || text.starts_with("xmit ")
                    || text == "stream create type=dax_tx" =>
            {
                Some(text)
            }
            SimEvent::Sent { line, .. } if line.contains("|transmit ") => {
                dax_word(&line).map(|w| format!("radio {w}"))
            }
            SimEvent::Sent { line, .. } if line.contains(IDLE) => Some("idle".to_string()),
            _ => None,
        })
        .collect()
}

/// When the radio first saw `want` (a command, or `radio dax=…` / `idle` as [`voice_trace`] names
/// them) at or after `since`.
fn seen_at(s: &FlexScene, since: Instant, want: &str) -> Option<Instant> {
    s.log()
        .into_iter()
        .filter(|(at, _)| *at >= since)
        .find(|(_, e)| match e {
            SimEvent::Command { text, .. } => text == want,
            SimEvent::Sent { line, .. } if want == "idle" => line.contains(IDLE),
            SimEvent::Sent { line, .. } if line.contains("|transmit ") => {
                dax_word(line).is_some_and(|w| want == format!("radio {w}"))
            }
            _ => false,
        })
        .map(|(at, _)| at)
}

/// The DAX transmit datagrams the radio received at or after `since`: when, and how many 24 kHz
/// frames each carried.
fn dax_in(s: &FlexScene, since: Instant) -> Vec<(Instant, usize)> {
    s.log()
        .into_iter()
        .filter(|(at, _)| *at >= since)
        .filter_map(|(at, e)| match e {
            SimEvent::UdpIn { bytes, .. } if bytes.len() > 1 => {
                let v = tempo_net::flexvita::parse_vita(&bytes).expect("a VITA-49 packet");
                let stereo = tempo_net::flex::streams::decode_dax_audio(
                    v.packet_class.expect("a class"),
                    v.payload,
                )
                .expect("whole float32 stereo frames");
                Some((at, stereo.len() / 2))
            }
            _ => None,
        })
        .collect()
}

/// The voice keyer's last dropped message, as the UI receives it: its cause, or null.
fn dropped(s: &FlexScene) -> serde_json::Value {
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    snap["radio"]["voiceDropped"]["cause"].clone()
}

/// The operator's PTT press the client kept off the air for the radio's DAX, as the UI receives it:
/// its mode, or null.
fn ptt_refused(s: &FlexScene) -> serde_json::Value {
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    snap["radio"]["pttRefused"]["mode"].clone()
}

/// Run the loop until the radio has seen `want` after `since`, watching what the engine is told
/// each tick: whether the radio has the mic, and whether the Phone screen shows the mic off.
fn run_until_seen(
    s: &mut FlexScene,
    since: Instant,
    want: &str,
) -> std::collections::BTreeSet<(bool, bool)> {
    let mut told = std::collections::BTreeSet::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    while seen_at(s, since, want).is_none() {
        assert!(
            Instant::now() < deadline,
            "never saw {want}: {:?}",
            voice_trace(s, since)
        );
        s.step(now_unix_ms());
        let radio = s.engine.lock().unwrap().snapshot().radio;
        told.insert((radio.flex_radio_has_mic, radio.flex_dax_tx));
        std::thread::sleep(Duration::from_millis(20));
    }
    told
}

/// A message of `secs` at the keyer's 12 kHz.
fn message_of(secs: f32) -> Vec<f32> {
    vec![0.05; (secs * 12_000.0) as usize]
}

/// The whole message, as DAX frames at 24 kHz.
const ONE_SECOND_OF_DAX: usize = 24_000;

/// ⭐ DAX ONLY FOR THE MESSAGE. The radio has the mic; a message asks it for DAX (Nexus's transmit
/// stream first, on the first message after a connect), keys only once the radio reports DAX, plays
/// the whole message over it, unkeys at its end plus the tail, and puts the mic back only after the
/// radio has let go of the transmitter. The engine is never told the radio lost the mic, the
/// Phone screen never shows the mic off, and the status lane says nothing was dropped. A second
/// message needs only the write.
#[test]
fn a_voice_message_switches_to_dax_plays_and_gives_the_mic_back() {
    let mut s = phone();
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message());
    assert_eq!(sent, Ok(()), "the voice keyer plays in Phone at the shack");
    let told = run_until_seen(&mut s, since, "radio dax=0");
    s.run(200);
    assert_eq!(
        dropped(&s),
        serde_json::Value::Null,
        "a message that played was not dropped"
    );
    assert_eq!(
        voice_trace(&s, since),
        [
            "stream create type=dax_tx",
            "transmit set dax=1",
            "radio dax=1",
            "xmit 1",
            "xmit 0",
            "idle",
            "transmit set dax=0",
            "radio dax=0"
        ]
    );
    assert_eq!(
        told.into_iter().collect::<Vec<_>>(),
        [(true, false)],
        "(has the mic, mic off) as the engine was told, every tick"
    );
    let at = |want: &str| seen_at(&s, since, want).expect(want);
    let audio = dax_in(&s, since);
    assert!(
        audio
            .iter()
            .all(|(t, _)| *t > at("radio dax=1") && *t < at("xmit 0")),
        "every DAX datagram after the radio took DAX and before the unkey"
    );
    let frames: usize = audio.iter().map(|(_, n)| n).sum();
    assert!(
        (ONE_SECOND_OF_DAX - 2 * 128..=ONE_SECOND_OF_DAX).contains(&frames),
        "the whole message went out: {frames} frames"
    );
    let over = at("xmit 0").duration_since(at("xmit 1"));
    assert!(
        over >= Duration::from_millis(1_250) && over < Duration::from_millis(3_000),
        "the over is the message and its tail: {over:?}"
    );

    // A second message: the stream is there, so only the write.
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(200);
    assert_eq!(dropped(&s), serde_json::Value::Null);
    assert_eq!(
        voice_trace(&s, since),
        [
            "transmit set dax=1",
            "radio dax=1",
            "xmit 1",
            "xmit 0",
            "idle",
            "transmit set dax=0",
            "radio dax=0"
        ]
    );
}

/// The bundled radio reporting its VOX on (`vox_enable=1`), as SmartSDR's own switch leaves it.
fn with_vox_on() -> SimSession {
    let mut s = reports(&["USB"]);
    for (pattern, rules) in &mut s.rules {
        if *pattern == Pattern::Exact("sub tx all".to_string()) {
            for item in rules.iter_mut().flat_map(|r| r.items.iter_mut()) {
                if let Item::Send(line) = item {
                    *line = line.replace(" vox_enable=0 ", " vox_enable=1 ");
                }
            }
        }
    }
    s
}

/// VOX ON IS ALLOWED (operator ruling, 2026-10-12, "Allow it"): a message plays with the radio's
/// VOX on, through the same switch, and its unkey is checked as every over's is.
#[test]
fn the_voice_keyer_plays_with_the_radios_vox_on() {
    let mut s = shack(with_vox_on());
    assert_eq!(
        s.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::flex)
            .and_then(|d| d.session().snapshot().model.transmit.vox_enable),
        Some(true),
        "premise: the radio reports its VOX on"
    );
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message());
    assert_eq!(sent, Ok(()));
    run_until_seen(&mut s, since, "radio dax=0");
    assert_eq!(
        voice_trace(&s, since),
        [
            "stream create type=dax_tx",
            "transmit set dax=1",
            "radio dax=1",
            "xmit 1",
            "xmit 0",
            "idle",
            "transmit set dax=0",
            "radio dax=0"
        ]
    );
}

/// ⭐ NEVER KEYED BEFORE THE RADIO REPORTS DAX, AND NEVER LATER THAN 2 SECONDS. The radio never
/// answers the DAX source write, so it goes on taking its mic: the message waits, keys nothing, and
/// is dropped 2 seconds after the press with its reason, never played after that.
#[test]
fn a_voice_message_never_keys_while_the_radio_still_takes_the_mic() {
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::Unanswered {
            starting: "transmit set dax".into(),
            then_silent: false,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    shack_on(&mut s);
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message());
    assert_eq!(sent, Ok(()));
    s.run(1_500);
    let waiting = s.engine.lock().unwrap().tx_owner();
    s.run(1_000);
    let after = s.engine.lock().unwrap().tx_owner();
    s.run(1_000);
    assert_eq!(
        format!(
            "at 1.5 s {waiting:?}, at 2.5 s {after:?} dropped={} wire={:?}",
            dropped(&s),
            voice_trace(&s, since)
        ),
        "at 1.5 s Some(Voice), at 2.5 s None dropped=\"noDax\" \
         wire=[\"stream create type=dax_tx\", \"transmit set dax=1\"]"
    );
    assert_eq!(keys(&s), 0, "nothing keyed");
}

/// ⭐ ANYTHING ELSE KEYING FIRST DROPS THE MESSAGE, here Nexus's own PTT while the radio is still
/// switching (it reports DAX 800 ms after the write): the message is dropped with its reason, and
/// it never plays, also once the radio reports DAX and the PTT is let go. The press itself is kept
/// off the air: the radio is switching to DAX, so the operator's voice would not go out.
#[test]
fn a_message_waiting_for_dax_is_dropped_when_nexus_keys_first() {
    let mut s = shack(slow_dax(800));
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    s.run(300);
    assert!(
        voice_trace(&s, since).contains(&"transmit set dax=1".to_string()),
        "premise: the switch is under way: {:?}",
        voice_trace(&s, since)
    );
    s.engine.lock().unwrap().set_ptt(true);
    s.run(300);
    s.engine.lock().unwrap().set_ptt(false);
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(500);
    assert_eq!(
        format!(
            "dropped={} keys={} wire={:?}",
            dropped(&s),
            keys(&s),
            voice_trace(&s, since)
        ),
        "dropped=\"keyed\" keys=0 wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \
         \"radio dax=1\", \"transmit set dax=0\", \"radio dax=0\"]"
    );
}

/// …and the radio keyed by something else while the message waits: here the operator's own mic
/// PTT on the radio, reported by the interlock before the radio reports DAX. The message is
/// dropped with its reason and never plays, though the radio then reports DAX with its transmitter
/// free.
#[test]
fn a_message_waiting_for_dax_is_dropped_when_the_radio_is_keyed_first() {
    let mic = with_rule(
        reports(&["USB"]),
        "transmit set dax=1",
        vec![
            send(&interlock("00000000", "PTT_REQUESTED", "MIC")),
            Item::Wait(30),
            send(&interlock("00000000", "TRANSMITTING", "MIC")),
            Item::Wait(400),
            send(IDLE),
            Item::Wait(200),
            send("S0|transmit dax=1"),
        ],
    );
    let mut s = shack(mic);
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(500);
    assert_eq!(
        format!(
            "dropped={} keys={} wire={:?}",
            dropped(&s),
            keys(&s),
            voice_trace(&s, since)
        ),
        "dropped=\"keyed\" keys=0 wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \
         \"idle\", \"radio dax=1\", \"transmit set dax=0\", \"radio dax=0\"]"
    );
}

/// ⭐ THE LATCH, AFTER THE WAIT. TX turned off while a message waits for the radio's DAX: the
/// message is dropped when the radio reports DAX, never keyed, and the mic comes back.
#[test]
fn tx_off_while_a_message_waits_keys_nothing() {
    let mut s = shack(slow_dax(800));
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    s.run(300);
    s.engine.lock().unwrap().set_tx_enabled(false);
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(300);
    assert_eq!(
        format!(
            "owner={:?} keys={} wire={:?}",
            s.engine.lock().unwrap().tx_owner(),
            keys(&s),
            voice_trace(&s, since)
        ),
        "owner=None keys=0 wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \
         \"radio dax=1\", \"transmit set dax=0\", \"radio dax=0\"]"
    );
}

/// ⭐ STOP TX ENDS A MESSAGE AT ONCE: the unkey goes out within a tick of the press, never waiting
/// for the routing; the rest of the message never reaches the radio; and the mic comes back only
/// after the radio has let go of the transmitter. Both stops: the screen's Stop TX (`halt_tx`) and
/// the keyer's own ■ Stop.
#[test]
fn stop_tx_mid_message_unkeys_and_drops_the_rest_before_the_mic_comes_back() {
    for keyers_own in [false, true] {
        let mut s = phone();
        let since = Instant::now();
        s.engine
            .lock()
            .unwrap()
            .send_voice(message_of(3.0))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while dax_in(&s, since).len() < 20 {
            assert!(
                Instant::now() < deadline,
                "the message never went out as DAX"
            );
            s.step(now_unix_ms());
            std::thread::sleep(Duration::from_millis(20));
        }
        if keyers_own {
            s.engine.lock().unwrap().stop_voice();
        } else {
            s.engine.lock().unwrap().halt_tx();
        }
        let pressed = Instant::now();
        s.step(now_unix_ms());
        assert!(
            s.sim.wait_for(Duration::from_secs(2), |log| log.iter().any(
                |l| matches!(&l.event, SimEvent::Command { text, .. } if text == "xmit 0")
            )),
            "no unkey after one tick"
        );
        let unkey = seen_at(&s, since, "xmit 0").unwrap();
        assert!(
            unkey.duration_since(pressed) < Duration::from_millis(500),
            "the unkey waited"
        );
        run_until_seen(&mut s, since, "radio dax=0");
        assert_eq!(
            voice_trace(&s, since),
            [
                "stream create type=dax_tx",
                "transmit set dax=1",
                "radio dax=1",
                "xmit 1",
                "xmit 0",
                "idle",
                "transmit set dax=0",
                "radio dax=0"
            ],
            "keyer's own stop: {keyers_own}"
        );
        let audio = dax_in(&s, since);
        let late: Vec<Duration> = audio
            .iter()
            .filter(|(t, _)| *t > unkey)
            .map(|(t, _)| t.duration_since(unkey))
            .collect();
        assert!(
            late.iter().all(|d| *d < Duration::from_millis(30)),
            "DAX went on after the unkey: {late:?}"
        );
        let frames: usize = audio.iter().map(|(_, n)| n).sum();
        assert!(
            frames < ONE_SECOND_OF_DAX,
            "the rest of a 3 s message was dropped: {frames} frames went out"
        );
    }
}

/// ⭐ NATIVE AUDIO OFF MID-MESSAGE ENDS IT AT ONCE (operator ruling, 2026-10-12, "End the message
/// and unkey", and the line the signed text gained for it). The message's audio goes with the DAX
/// route, so the over ends as the keyer's own Stop ends it: the unkey within a few ticks of the
/// switch, never at the message's own end; nothing of the rest reaches the radio; and the mic comes
/// back after the radio has let go. TX On stays as it was: the latch on, no lane, no alarm.
#[test]
fn native_audio_off_mid_message_ends_it_and_unkeys_with_tx_on_as_it_was() {
    let mut s = phone();
    let since = Instant::now();
    s.engine
        .lock()
        .unwrap()
        .send_voice(message_of(3.0))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while dax_in(&s, since).len() < 20 {
        assert!(
            Instant::now() < deadline,
            "the message never went out as DAX"
        );
        s.step(now_unix_ms());
        std::thread::sleep(Duration::from_millis(20));
    }
    s.set_native_audio(false);
    let off = Instant::now();
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(300);
    let unkey = seen_at(&s, since, "xmit 0").expect("the unkey");
    let late: Vec<Duration> = dax_in(&s, since)
        .iter()
        .filter(|(t, _)| *t > unkey)
        .map(|(t, _)| t.duration_since(unkey))
        .collect();
    let frames: usize = dax_in(&s, since).iter().map(|(_, n)| n).sum();
    // One engine lock at a time: a guard taken inside the format below would live to its end.
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    let tx_enabled = s.engine.lock().unwrap().tx_enabled();
    let alarm = names_an_alarm(&cat_status(&s).1);
    assert_eq!(
        format!(
            "unkey_within_500ms={} late_dax={} frames_under_a_second={} tx_enabled={tx_enabled} \
             dropped={} slot_lane={} alarm={alarm} wire={:?}",
            unkey.duration_since(off) < Duration::from_millis(500),
            late.iter().any(|d| *d >= Duration::from_millis(30)),
            frames < ONE_SECOND_OF_DAX,
            snap["radio"]["voiceDropped"],
            snap["radio"]["slotAudioLost"],
            voice_trace(&s, since)
        ),
        "unkey_within_500ms=true late_dax=false frames_under_a_second=true tx_enabled=true \
         dropped=null slot_lane=null alarm=false wire=[\"stream create type=dax_tx\", \
         \"transmit set dax=1\", \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"idle\", \"transmit \
         set dax=0\", \"radio dax=0\"]",
        "the unkey {:?} after native audio went off",
        unkey.duration_since(off)
    );
}

/// …and Stop during the wait: the message never plays, the radio is never keyed, and the mic comes
/// back once the radio has reported the DAX it was switching to.
#[test]
fn stop_tx_while_a_message_waits_keys_nothing_and_gives_the_mic_back() {
    let mut s = shack(slow_dax(800));
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    s.run(300);
    s.engine.lock().unwrap().halt_tx();
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(500);
    assert_eq!(
        format!("keys={} wire={:?}", keys(&s), voice_trace(&s, since)),
        "keys=0 wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \"radio dax=1\", \
         \"transmit set dax=0\", \"radio dax=0\"]"
    );
}

/// ⭐ A MESSAGE LONGER THAN THE CLIENT'S DAX CAN CARRY IS REFUSED, NOT CUT. The DAX transmit queue
/// keeps the newest 20 seconds, so a 25-second message would lose its start: it is refused with the
/// reason, and nothing reaches the radio.
#[test]
fn a_message_longer_than_the_dax_queue_is_refused_not_cut() {
    let mut s = phone();
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message_of(25.0));
    s.run(1_000);
    assert_eq!(
        format!("{sent:?} wire={:?}", voice_trace(&s, since)),
        "Err(\"The voice keyer can't play this message through the Flex native client: it is \
         longer than 20 seconds, the most the client's DAX carries, so its start would be lost. \
         Nothing was keyed.\") wire=[]"
    );
}

/// ⭐ SOMETHING ELSE ALREADY TRANSMITTING REFUSES THE MESSAGE, with that transmission's own words:
/// here the operator's PTT held. Nothing reaches the radio for the message.
#[test]
fn a_message_is_refused_while_something_else_is_transmitting() {
    let mut s = phone();
    s.engine.lock().unwrap().set_ptt(true);
    run_until(&mut s, "the PTT never keyed", |s| keys(s) == 1);
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message());
    s.run(500);
    s.engine.lock().unwrap().set_ptt(false);
    s.run(500);
    assert_eq!(
        format!("{sent:?} wire={:?}", voice_trace(&s, since)),
        "Err(\"Mic PTT is held — release it first\") wire=[\"xmit 0\", \"idle\"]"
    );
}

/// …and a second message while one plays: the first is something else transmitting for it. The
/// engine does not see an over the loop is playing, so the send is taken; the loop drops it at once,
/// with its reason on the status lane, and the first message plays to its end, alone in the DAX
/// queue, with the mic back after it.
#[test]
fn a_second_message_while_one_plays_is_dropped_and_says_why() {
    let mut s = phone();
    let since = Instant::now();
    s.engine
        .lock()
        .unwrap()
        .send_voice(message_of(2.0))
        .unwrap();
    run_until_seen(&mut s, since, "xmit 1");
    s.run(300);
    let second = s.engine.lock().unwrap().send_voice(message());
    s.run(200);
    let dropped_while_playing = dropped(&s);
    run_until_seen(&mut s, since, "radio dax=0");
    let frames: usize = dax_in(&s, since).iter().map(|(_, n)| n).sum();
    assert_eq!(
        format!(
            "{second:?} dropped={dropped_while_playing} keys={} wire={:?}",
            keys(&s),
            voice_trace(&s, since)
        ),
        "Ok(()) dropped=\"keyed\" keys=1 wire=[\"stream create type=dax_tx\", \"transmit set \
         dax=1\", \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"idle\", \"transmit set dax=0\", \
         \"radio dax=0\"]"
    );
    assert!(
        (2 * ONE_SECOND_OF_DAX - 2 * 128..=2 * ONE_SECOND_OF_DAX).contains(&frames),
        "only the first message went out, whole: {frames} frames"
    );
}

/// ⭐ THE LIVE MIC IS UNTOUCHED WITHOUT A MESSAGE. Nexus's own PTT in Phone at the shack keys the
/// radio on its mic: no DAX source written, no DAX audio. Right after a message, until the mic is
/// back, the same PTT is kept off the air with the reason on the status lane; once the mic is back
/// it keys on the mic again, still with no DAX.
#[test]
fn the_live_mic_is_untouched_without_a_message() {
    let mut s = phone();
    let since = Instant::now();
    s.engine.lock().unwrap().set_ptt(true);
    run_until(&mut s, "the PTT never keyed", |s| keys(s) == 1);
    s.engine.lock().unwrap().set_ptt(false);
    run_until_seen(&mut s, since, "idle");
    assert_eq!(
        format!(
            "wire={:?} dax={}",
            voice_trace(&s, since),
            dax_in(&s, since).len()
        ),
        "wire=[\"xmit 1\", \"xmit 0\", \"idle\"] dax=0"
    );

    // A message, and the PTT pressed as soon as its over ends, before the mic is back.
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until_seen(&mut s, since, "xmit 0");
    s.engine.lock().unwrap().set_ptt(true);
    run_until_seen(&mut s, since, "idle");
    s.run(300);
    let refused = ptt_refused(&s);
    s.engine.lock().unwrap().set_ptt(false);
    run_until_seen(&mut s, since, "radio dax=0");
    assert_eq!(
        format!("refused={refused} wire={:?}", voice_trace(&s, since)),
        "refused=\"USB\" wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \"radio \
         dax=1\", \"xmit 1\", \"xmit 0\", \"idle\", \"transmit set dax=0\", \"radio dax=0\"]",
        "the PTT kept off the air while the radio was still on the keyer's DAX"
    );

    // The mic back: the PTT keys on it, with no DAX written and none sent.
    let since = Instant::now();
    s.engine.lock().unwrap().set_ptt(true);
    run_until_seen(&mut s, since, "xmit 1");
    s.run(300);
    s.engine.lock().unwrap().set_ptt(false);
    run_until_seen(&mut s, since, "idle");
    assert_eq!(
        format!(
            "wire={:?} dax={}",
            voice_trace(&s, since),
            dax_in(&s, since).len()
        ),
        "wire=[\"xmit 1\", \"xmit 0\", \"idle\"] dax=0"
    );
}

/// ⭐ NEXUS'S PTT HELD THROUGH A MESSAGE NEVER HOLDS THE KEYER'S DAX ON THE AIR. Pressed while the
/// message plays, it is kept off the air (the radio takes DAX, not the mic); at the message's end
/// the over is unkeyed all the same, where on the mic the held PTT would carry the operator's voice
/// on; and while the PTT stays held the radio is not keyed on that DAX again. Let go, the mic comes
/// back.
#[test]
fn a_ptt_held_through_a_message_never_holds_the_keyers_dax() {
    let mut s = phone();
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    run_until_seen(&mut s, since, "xmit 1");
    s.run(200);
    s.engine.lock().unwrap().set_ptt(true);
    run_until_seen(&mut s, since, "idle");
    s.run(500);
    let held = format!(
        "keys={} refused={} wire={:?}",
        keys(&s),
        ptt_refused(&s),
        voice_trace(&s, since)
    );
    s.engine.lock().unwrap().set_ptt(false);
    run_until_seen(&mut s, since, "radio dax=0");
    assert_eq!(
        held,
        "keys=1 refused=\"USB\" wire=[\"stream create type=dax_tx\", \"transmit set dax=1\", \
         \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"idle\"]"
    );
}

/// ⭐ SSTV AND APRS STAY REFUSED WHILE A MESSAGE PLAYS (operator ruling, 2026-10-12, "Stay
/// refused"): the radio takes DAX only for the message, so the Phone screen is still told the radio
/// has the mic, and a picture or a beacon is refused with the reason, nothing queued.
#[test]
fn sstv_and_aprs_still_refuse_while_a_message_plays() {
    let mut s = phone();
    let since = Instant::now();
    s.engine
        .lock()
        .unwrap()
        .send_voice(message_of(3.0))
        .unwrap();
    run_until_seen(&mut s, since, "xmit 1");
    s.run(300);
    let (sstv, aprs) = {
        let mut e = s.engine.lock().unwrap();
        (
            e.sstv_send(picture(), "Scottie 1".to_string()),
            e.aprs_beacon(41.88, -87.63, '/', '>', "", &[]),
        )
    };
    assert_eq!(
        format!("{sstv:?} {aprs:?}"),
        "Err(\"SSTV can't send while the radio has the mic: with Flex native DAX audio on, Phone \
         at the shack uses the radio's own mic, so the picture would not go out. Nothing was \
         keyed.\") Err(\"APRS can't send while the radio has the mic: with Flex native DAX audio \
         on, a voice mode such as FM uses the radio's own mic, so the packet would not go out. \
         Nothing was keyed.\")"
    );
}

/// Beside SmartSDR's DAX the radio's DAX source is not Nexus's to write (operator ruling,
/// 2026-10-03), and the radio is not on its mic for Nexus: a message keys and plays as it does
/// today, and no DAX source is written. A control.
#[test]
fn beside_smartsdrs_dax_a_message_writes_no_dax_source() {
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::ForeignDaxTx(
            tempo_flexsim::ForeignDax::default(),
        )],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until(&mut s, "the client never saw the other program", |s| {
        s.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::flex)
            .is_some_and(FlexDaemon::other_dax_feeder)
    });
    s.run(500);
    let since = Instant::now();
    let sent = s.engine.lock().unwrap().send_voice(message());
    run_until_seen(&mut s, since, "idle");
    assert_eq!(
        format!(
            "{sent:?} has_mic={} wire={:?}",
            has_mic(&s),
            voice_trace(&s, since)
        ),
        "Ok(()) has_mic=false wire=[\"xmit 1\", \"xmit 0\", \"idle\"]"
    );
}

/// The bundled radio as [`holder_kept`] has it, reporting the slice's USB back.
fn holder_kept_in_usb() -> SimSession {
    with_rule(
        holder_kept(),
        "slice set 0 mode=USB",
        vec![send("S{h}|slice 0 mode=USB")],
    )
}

/// ⭐ A SESSION LOST MID-MESSAGE. The radio drops the connection while the message plays and stays
/// keyed. No DAX leaves after the loss; the operator is told the radio may still be transmitting;
/// the loop starts a fresh session, whose first stop unkeys the lost session's transmitter, and the
/// radio's DAX source goes back to the mic only after that.
#[test]
fn a_session_lost_mid_message_stops_dax_and_unkeys_before_the_mic_comes_back() {
    let mut s = FlexScene::with_faults(
        true,
        holder_kept_in_usb(),
        vec![tempo_flexsim::Fault::DisconnectMidOver {
            after: Duration::from_millis(300),
            radio_stays_keyed: true,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    s.replace_on_rebuild = true;
    shack_on(&mut s);
    let since = Instant::now();
    s.engine
        .lock()
        .unwrap()
        .send_voice(message_of(3.0))
        .unwrap();
    let detail = status_after_the_rebuild(&mut s);
    assert!(
        detail.starts_with(
            "the connection to the radio was lost during a transmission — it may still be \
             transmitting. Check the radio now."
        ),
        "{detail}"
    );
    let log = s.log();
    let lost = log
        .iter()
        .find(|(at, e)| *at >= since && matches!(e, SimEvent::Closed { conn: 0, .. }))
        .map(|(at, _)| *at)
        .expect("the first session closed");
    assert!(
        dax_in(&s, since).iter().all(|(t, _)| *t < lost),
        "DAX after the session was lost"
    );
    // The new session writes the mic once the loop has put its fresh slice back in USB, which
    // took 7.4 s after the rebuild when measured alone: until then the slice reads as digital.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !sent(&s.sim.log(), 1)
        .iter()
        .any(|c| c == "transmit set dax=0")
    {
        assert!(
            Instant::now() < deadline,
            "the new session never put the mic back: {:?}",
            sent(&s.sim.log(), 1)
        );
        s.run(100);
    }
    let on_new = sent(&s.sim.log(), 1);
    let unkey = on_new.iter().position(|c| c == "xmit 0");
    let mic = on_new.iter().position(|c| c == "transmit set dax=0");
    assert!(
        unkey.is_some() && unkey < mic,
        "the new session's unkey goes first: {on_new:?}"
    );
}

/// The bundled radio reporting the operator's own mic PTT on the radio 300 ms into an over of ours,
/// under our handle, as the radio might (what it reports is not known until a bench shows it).
fn mic_ptt_mid_over() -> SimSession {
    with_rule(
        reports(&["USB"]),
        "xmit 1",
        vec![
            send(&interlock("{h}", "PTT_REQUESTED", "SW")),
            Item::Wait(30),
            send(&interlock("{h}", "TRANSMITTING", "SW")),
            Item::Wait(300),
            send(&interlock("{h}", "TRANSMITTING", "MIC")),
        ],
    )
}

/// ⭐ THE MIC'S PTT DURING A MESSAGE KEEPS TODAY'S RULE until a bench shows what the radio reports
/// for it (operator ruling, 2026-10-12: "Your mic wins, like the paddle", after the bench). The
/// message is not cut short: its over runs to its end; then the radio cannot prove the unkey, so
/// Nexus sends it again, tells the operator in amber, and the loop starts a fresh session.
#[test]
fn the_mics_ptt_during_a_message_still_alarms_and_reconnects() {
    let mut s = FlexScene::with_faults(true, mic_ptt_mid_over(), Vec::new(), short_deadline());
    s.replace_on_rebuild = true;
    shack_on(&mut s);
    let since = Instant::now();
    s.engine.lock().unwrap().send_voice(message()).unwrap();
    let detail = status_after_the_rebuild(&mut s);
    let over = seen_at(&s, since, "xmit 0")
        .expect("the unkey")
        .duration_since(seen_at(&s, since, "xmit 1").expect("the key"));
    assert!(
        detail.starts_with(
            "the radio did not confirm the unkey — it may still be transmitting. Check the radio \
             now."
        ),
        "{detail}"
    );
    assert!(
        over >= Duration::from_millis(1_250),
        "the message was cut short: {over:?}"
    );
    assert_eq!(
        sent(&s.sim.log(), 0)
            .iter()
            .filter(|c| c.starts_with("xmit "))
            .collect::<Vec<_>>(),
        ["xmit 1", "xmit 0", "xmit 0"],
        "the unkey sent again, unproven"
    );
}

/// The client's session taking `MIC` under our handle as the operator's own mic, as a bench might
/// name it (no bench has shown what the radio reports for the mic's PTT during an over).
fn mic_named() -> tempo_net::flex::session::Config {
    let mut config = tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap());
    config.operator_mic = Some(tempo_net::flex::ptt_evidence::OperatorKey {
        source: "MIC",
        handle: tempo_net::flex::ptt_evidence::KeyHandle::Ours,
    });
    config
}

/// When the radio first sent a keying line under `source` at or after `since`.
fn keyed_under(s: &FlexScene, since: Instant, source: &str) -> Option<Instant> {
    s.log()
        .into_iter()
        .filter(|(at, _)| *at >= since)
        .find(|(_, e)| {
            matches!(e, SimEvent::Sent { line, .. }
                if line.contains("|interlock ") && line.contains(" state=TRANSMITTING ")
                    && line.contains(&format!(" source={source} ")))
        })
        .map(|(at, _)| at)
}

/// ⭐ YOUR MIC WINS (operator rulings, 2026-10-12, "Your mic wins, like the paddle", built switched
/// off): with the mic named as a bench would name it, the operator's mic PTT 300 ms into a message
/// ends the message as the keyer's own Stop does. Nexus's unkey goes out on the radio's first line
/// for the mic, the message's DAX stops with it, the rest is dropped, and nothing alarms or
/// reconnects: no amber, no lane, TX On as it was. Once the mic lets go and the radio is idle, the
/// mic is put back as after any message.
#[test]
fn with_a_mic_named_the_mics_ptt_during_a_message_ends_it_with_no_alarm() {
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::OperatorMic {
            after: Duration::from_millis(300),
            source: "MIC".into(),
            handle: None,
            hold: Duration::from_millis(2_000),
        }],
        mic_named(),
    );
    shack_on(&mut s);
    let since = Instant::now();
    s.engine
        .lock()
        .unwrap()
        .send_voice(message_of(3.0))
        .unwrap();
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(300);
    let mic = keyed_under(&s, since, "MIC").expect("the mic's PTT");
    let unkey = seen_at(&s, since, "xmit 0").expect("the unkey");
    let late: Vec<Duration> = dax_in(&s, since)
        .iter()
        .filter(|(t, _)| *t > mic)
        .map(|(t, _)| t.duration_since(mic))
        .collect();
    let frames: usize = dax_in(&s, since).iter().map(|(_, n)| n).sum();
    // One engine lock at a time: a guard taken inside the format below would live to its end.
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    let tx_enabled = s.engine.lock().unwrap().tx_enabled();
    let alarm = names_an_alarm(&cat_status(&s).1);
    assert_eq!(
        format!(
            "unkey_after_mic={} late_dax={} frames_under_a_second={} tx_enabled={tx_enabled} \
             dropped={} alarm={alarm} rebuilt={} wire={:?}",
            unkey >= mic && unkey.duration_since(mic) < Duration::from_millis(100),
            late.iter().any(|d| *d >= Duration::from_millis(30)),
            frames < ONE_SECOND_OF_DAX,
            snap["radio"]["voiceDropped"],
            s.rebuilt.load(std::sync::atomic::Ordering::Relaxed),
            voice_trace(&s, since)
        ),
        "unkey_after_mic=true late_dax=false frames_under_a_second=true tx_enabled=true \
         dropped=null alarm=false rebuilt=false wire=[\"stream create type=dax_tx\", \
         \"transmit set dax=1\", \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"xmit 0\", \"idle\", \
         \"transmit set dax=0\", \"radio dax=0\"]",
        "the unkey {:?} after the mic's line",
        unkey.saturating_duration_since(mic)
    );
}
