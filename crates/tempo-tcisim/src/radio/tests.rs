//! The model on its own: commands in, effects out, its timers run in order on a test clock. No
//! sockets and no threads, so every timing here is exact.

use super::*;
use crate::dialect::Dialect;
use crate::frame::{kind, Header};
use std::collections::BTreeMap;

/// The radio with its own timers, run on a test clock.
struct Bench {
    r: Radio,
    t0: Instant,
    now: Instant,
    timers: BTreeMap<(Instant, u64), Action>,
    seq: u64,
    /// Every effect so far, with the clock reading it came at.
    fx: Vec<(Duration, Effect)>,
}

impl Bench {
    fn new(dialect: Dialect, faults: Vec<Fault>) -> Bench {
        let t0 = Instant::now();
        Bench {
            r: Radio::new(
                dialect,
                faults,
                Receive::default(),
                Meters::default(),
                48_000,
                t0,
            ),
            t0,
            now: t0,
            timers: BTreeMap::new(),
            seq: 0,
            fx: Vec::new(),
        }
    }

    fn absorb(&mut self) {
        let at = self.now - self.t0;
        for e in self.r.take() {
            if let Effect::Timer { at, action } = &e {
                self.seq += 1;
                self.timers.insert((*at, self.seq), action.clone());
            }
            self.fx.push((at, e));
        }
    }

    fn connect(&mut self, conn: usize) {
        self.r.connect(conn, self.now);
        self.absorb();
    }

    fn send(&mut self, conn: usize, text: &str) {
        self.r.on_text(conn, text, self.now);
        self.absorb();
    }

    fn operator(&mut self, text: &str) {
        self.r.operator(text, self.now);
        self.absorb();
    }

    fn binary(&mut self, conn: usize, bytes: &[u8]) {
        self.r.on_binary(conn, bytes, self.now);
        self.absorb();
    }

    fn leave(&mut self, conn: usize, by: Closer) {
        self.r.disconnect(conn, by, self.now);
        self.absorb();
    }

    /// Run every timer due within `d`, in order, and move the clock on by `d`.
    fn advance(&mut self, d: Duration) {
        let end = self.now + d;
        while let Some((&(at, seq), _)) = self.timers.first_key_value() {
            if at > end {
                break;
            }
            let action = self.timers.remove(&(at, seq)).unwrap();
            self.now = at.max(self.now);
            self.r.on_action(action, self.now);
            self.absorb();
        }
        self.now = end;
    }

    fn mark(&self) -> usize {
        self.fx.len()
    }

    /// Text effects that reach `conn` from `since`.
    fn texts(&self, conn: usize, since: usize) -> Vec<String> {
        self.fx[since..]
            .iter()
            .filter_map(|(_, e)| match e {
                Effect::Text { to: To::All, text } => Some(text.clone()),
                Effect::Text {
                    to: To::One(c),
                    text,
                } if *c == conn => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Frames to `conn` from `since`, with the clock reading each came at.
    fn frames(&self, conn: usize, since: usize) -> Vec<(Duration, Header, usize)> {
        self.fx[since..]
            .iter()
            .filter_map(|(at, e)| match e {
                Effect::Frame { to, bytes } if *to == conn => {
                    Header::decode(bytes).map(|h| (*at, h, bytes.len() - frame::HEADER_LEN))
                }
                _ => None,
            })
            .collect()
    }

    fn events(&self, since: usize) -> Vec<Event> {
        self.fx[since..]
            .iter()
            .filter_map(|(_, e)| match e {
                Effect::Log(ev) => Some(ev.clone()),
                _ => None,
            })
            .collect()
    }

    fn key(&self, t: usize) -> Option<Key> {
        self.r.trx[t].key
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn chronos(frames: &[(Duration, Header, usize)]) -> Vec<Duration> {
    frames
        .iter()
        .filter(|(_, h, _)| h.kind == kind::TX_CHRONO)
        .map(|(at, _, _)| *at)
        .collect()
}

/// A float32 stereo TX audio frame of `frames` frames, every sample `v`.
fn tx_frame(frames: usize, v: f32, length: u32) -> Vec<u8> {
    let h = Header {
        sample_rate: 48_000,
        format: SampleType::Float32.code(),
        length,
        kind: kind::TX_AUDIO,
        channels: 2,
        ..Header::default()
    };
    h.frame(&frame::encode_samples(
        SampleType::Float32,
        &vec![v; frames * 2],
    ))
}

#[test]
fn every_dialects_burst_carries_its_identity_in_its_order_and_ends_as_it_does() {
    for d in Dialect::all() {
        let mut b = Bench::new(d.clone(), Vec::new());
        b.connect(0);
        let texts = b.texts(0, 0);
        let joined = texts.concat();
        if d.burst_in_one_message {
            assert_eq!(texts.len(), 1, "{}", d.name);
        } else {
            assert!(texts.len() > 20, "{}: one message per command", d.name);
        }
        // The identity lines, in the profile's order, before anything else. Read message by
        // message: joined, piHPSDR's unterminated lines would swallow the `ready;` after them.
        let commands: Vec<wire::Command> =
            texts.iter().flat_map(|t| wire::parse(t).commands).collect();
        let head: Vec<&str> = commands.iter().take(8).map(|c| c.name.as_str()).collect();
        let want: Vec<&str> = d
            .identity_order
            .iter()
            .map(|i| match i {
                Ident::VfoLimits => "vfo_limits",
                Ident::IfLimits => "if_limits",
                Ident::TrxCount => "trx_count",
                Ident::ChannelCount => "channels_count",
                Ident::Device => "device",
                Ident::ReceiveOnly => "receive_only",
                Ident::Modulations => "modulations_list",
                Ident::Protocol => "protocol",
            })
            .collect();
        assert_eq!(head, want, "{}", d.name);
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        let ready = names.iter().position(|n| *n == "ready").expect("ready");
        if d.run_state_after_ready {
            assert_eq!(names[ready + 1..], ["start"], "{}", d.name);
        } else {
            assert_eq!(ready, names.len() - 1, "{}: ready is last", d.name);
        }
        assert!(joined.contains(&format!("protocol:{},{};", d.program, d.version)));
        assert!(joined.contains(&format!("device:{};", d.device)));
        // A state line for every transceiver.
        for t in 0..d.trx_count {
            assert!(joined.contains(&format!("vfo:{t},0,")), "{}", d.name);
            assert!(joined.contains(&format!("trx:{t},false;")), "{}", d.name);
        }
        match d.drive {
            DriveForm::PerTrx => assert!(joined.contains("drive:0,50;"), "{}", d.name),
            DriveForm::Bare => assert!(joined.contains(";drive:50;"), "{}", d.name),
        }
    }
    // piHPSDR's two unterminated lines arrive as their own messages, before ready.
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    let texts = b.texts(0, 0);
    let quirk = texts
        .iter()
        .position(|t| t == "audio_stream_channels: 2")
        .unwrap();
    assert_eq!(texts[quirk + 1], "audio_stream_samples: 1024");
    assert_eq!(texts[quirk + 2], "ready;");
}

#[test]
fn a_set_is_broadcast_a_read_answered_to_the_asker_and_a_bad_value_ignored() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.connect(1);
    let m = b.mark();
    b.send(0, "vfo:0,0,7074000;");
    let both = ["vfo:0,0,7074000;", "tx_frequency:7074000;"];
    assert_eq!(b.texts(0, m), both);
    assert_eq!(b.texts(1, m), both);
    let m = b.mark();
    b.send(1, "vfo:0,0;modulation:0;drive:0;");
    assert_eq!(
        b.texts(1, m),
        ["vfo:0,0,7074000;", "modulation:0,USB;", "drive:0,50;"]
    );
    assert!(b.texts(0, m).is_empty(), "a read goes to the asker only");
    // Outside vfo_limits, a mode not listed, a drive over 100, a transceiver that does not exist:
    // ignored, not echoed.
    let m = b.mark();
    b.send(
        0,
        "vfo:0,0,31000000;modulation:0,PKTUSB;drive:0,101;vfo:2,0,7074000;",
    );
    assert!(b.texts(0, m).is_empty());
    let ignored = b
        .events(m)
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Ignored {
                    why: Why::Value,
                    ..
                }
            )
        })
        .count();
    assert_eq!(ignored, 4);
    assert_eq!(b.r.trx[0].vfo[0], 7_074_000);
    // A mode is answered in the server's spelling; a sideband change mirrors the filter.
    let m = b.mark();
    b.send(0, "modulation:0,lsb;");
    assert_eq!(
        b.texts(0, m),
        ["modulation:0,LSB;", "rx_filter_band:0,-2900,-200;"]
    );
}

#[test]
fn a_tci_key_with_its_audio_running_is_pulled_with_tx_chrono_at_the_rate_it_asks_for() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    assert_eq!(
        b.key(0).map(|k| (k.by, k.source)),
        Some((Keyer::Client(0), Source::Tci))
    );
    assert_eq!(b.texts(0, m), ["trx:0,true;"]);
    b.advance(ms(1000));
    let times = chronos(&b.frames(0, m));
    // 2048 values a chrono, two channels: 1024 frames, 21.333 ms at 48 kHz, from the echo.
    assert_eq!(times.len(), 47, "{times:?}");
    for (n, at) in times.iter().enumerate() {
        let want = Duration::from_secs_f64(n as f64 * 1024.0 / 48_000.0);
        assert!(
            at.abs_diff(want) < ms(1),
            "chrono {n} at {at:?}, want {want:?}"
        );
    }
    let h = b
        .frames(0, m)
        .into_iter()
        .find(|(_, h, _)| h.kind == kind::TX_CHRONO)
        .unwrap();
    assert_eq!(
        (h.1.length, h.1.channels, h.1.sample_rate, h.2),
        (2048, 2, 48_000, 0)
    );
    // A frame answering it fits as values; a frame of frames does not fit what was asked.
    let m = b.mark();
    b.binary(0, &tx_frame(1024, 0.25, 2048));
    b.binary(0, &tx_frame(2048, 0.25, 2048));
    let asked: Vec<Option<Fit>> = b
        .events(m)
        .iter()
        .filter_map(|e| match e {
            Event::FrameIn { asked, ours, .. } => {
                assert!(ours);
                Some(*asked)
            }
            _ => None,
        })
        .collect();
    assert_eq!(asked, [Some(Fit::Values), Some(Fit::Frames)]);
    assert_eq!(b.r.tx_audio(0).len(), 1024 + 2048);
    // The unkey stops the chronos and echoes.
    let m = b.mark();
    b.send(0, "trx:0,false;");
    b.advance(ms(200));
    assert_eq!(b.texts(0, m), ["trx:0,false;"]);
    assert!(chronos(&b.frames(0, m)).is_empty());
}

#[test]
fn a_tci_key_without_audio_takes_the_microphone_where_the_spec_says_and_tci_audio_elsewhere() {
    // ExpertSDR3 (spec §4.2): the microphone, and no TX_CHRONO.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    b.advance(ms(200));
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Mic(Mic::Selected)));
    assert!(b
        .events(m)
        .contains(&Event::MicFallback { conn: 0, trx: 0 }));
    assert!(chronos(&b.frames(0, m)).is_empty());
    // A key naming no source takes the selected microphone, and is no fallback.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Mic(Mic::Selected)));
    assert!(!b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::MicFallback { .. })));
    b.send(0, "trx:0,true,mic2;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Mic(Mic::Mic2)));
    // piHPSDR starts the client's transmit audio itself and says so.
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Tci));
    let texts = b.texts(0, m);
    assert_eq!(texts.last().map(String::as_str), Some("trx:0,true;"));
    assert!(texts.contains(&"audio_start:0;".to_string()), "{texts:?}");
    b.advance(ms(100));
    assert!(!chronos(&b.frames(0, m)).is_empty());
    // A 1.x server has no source argument: TCI audio only when the stream runs.
    let mut b = Bench::new(Dialect::expert_sdr2(), Vec::new());
    b.connect(0);
    b.send(0, "trx:0,true,tci;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Mic(Mic::Selected)));
    b.send(0, "trx:0,false;audio_start:0;trx:0,true;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Tci));
}

#[test]
fn ownership_follows_each_dialect() {
    // None (the spec): another client's unkey ends my key.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.connect(1);
    b.send(0, "trx:0,true;");
    b.send(1, "trx:0,false;");
    assert_eq!(b.key(0), None);

    // The first keyer owns it (piHPSDR): another's key and unkey are answered with the state.
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    b.connect(1);
    b.send(0, "trx:0,true;");
    let m = b.mark();
    b.send(1, "trx:0,false;");
    assert!(b.key(0).is_some());
    assert_eq!(b.texts(1, m), ["trx:0,true;"]);
    b.send(1, "trx:0,true;");
    assert_eq!(b.key(0).map(|k| k.by), Some(Keyer::Client(0)));
    // The owner keeps it across overs.
    b.send(0, "trx:0,false;");
    b.advance(ms(100));
    b.send(1, "trx:0,true;");
    assert_eq!(b.key(0), None, "client 1 is not the owner");
    // An owner that closes cleanly while keyed leaves the key up with no owner (no unkey on a
    // clean close); a bare unkey from anyone else then changes nothing, and a key-then-unkey does.
    b.send(0, "trx:0,true;");
    b.leave(0, Closer::Client);
    assert!(b.key(0).is_some_and(|k| k.orphaned));
    assert_eq!(b.r.owner, None);
    let m = b.mark();
    b.send(1, "trx:0,false;");
    assert!(
        b.key(0).is_some(),
        "piHPSDR ignores an unkey from a client that does not own it"
    );
    assert_eq!(b.texts(1, m), ["trx:0,true;"]);
    b.send(1, "trx:0,true;");
    b.send(1, "trx:0,false;");
    b.advance(ms(100));
    assert_eq!(b.key(0), None);

    // The initiator alone may release (AetherSDR).
    let mut b = Bench::new(Dialect::aether_sdr(), Vec::new());
    b.connect(0);
    b.connect(1);
    b.send(0, "trx:0,true;");
    b.advance(ms(100));
    let m = b.mark();
    b.send(1, "trx:0,false;");
    assert!(b.key(0).is_some());
    assert_eq!(b.texts(1, m), ["trx:0,true;"]);
    assert!(b.events(m).iter().any(|e| matches!(
        e,
        Event::Refused {
            why: Refusal::NotOwner,
            ..
        }
    )));
    // A lost owner is unkeyed.
    let m = b.mark();
    b.leave(0, Closer::Dropped);
    assert_eq!(b.key(0), None);
    assert_eq!(b.texts(1, m), ["trx:0,false;"]);
}

#[test]
fn aethersdr_echoes_once_the_radio_transmits_and_pulls_audio_from_then() {
    let mut b = Bench::new(Dialect::aether_sdr(), Vec::new());
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    assert!(
        b.texts(0, m).is_empty(),
        "no echo before the radio transmits"
    );
    b.advance(ms(59));
    assert!(b.texts(0, m).is_empty());
    b.advance(ms(2));
    assert_eq!(b.texts(0, m), ["trx:0,true;"]);
    let times = chronos(&b.frames(0, m));
    assert_eq!(times.first(), Some(&ms(60)));
    let first = b
        .frames(0, m)
        .into_iter()
        .find(|(_, h, _)| h.kind == kind::TX_CHRONO)
        .unwrap();
    assert_eq!(first.1.length, 2048);
}

#[test]
fn pihpsdr_holds_the_key_fifty_ms_after_an_unkey_while_tci_audio_ran() {
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    b.send(0, "trx:0,true,tci;");
    let m = b.mark();
    b.send(0, "trx:0,false;");
    assert!(b.key(0).is_some());
    b.advance(ms(49));
    assert!(b.key(0).is_some());
    b.advance(ms(2));
    assert_eq!(b.key(0), None);
    assert_eq!(b.texts(0, m), ["trx:0,false;"]);
    // From the microphone there is no tail.
    b.send(0, "trx:0,true;");
    b.send(0, "trx:0,false;");
    assert_eq!(b.key(0), None);
}

#[test]
fn every_refusal_of_a_key_is_logged_and_keys_nothing() {
    for (faults, why) in [
        (vec![Fault::TxEnableFalse], Refusal::TxDisabled),
        (vec![Fault::ReceiveOnly], Refusal::ReceiveOnly),
    ] {
        let mut b = Bench::new(Dialect::expert_sdr3(), faults);
        b.connect(0);
        let burst = b.texts(0, 0).concat();
        assert!(burst.contains("tx_enable:0,false;") || burst.contains("receive_only:true;"));
        for key in [
            "trx:0,true;",
            "trx:0,true,tci;",
            "tune:0,true;",
            "keyer:0,true,0;",
            "cw_macros:0,CQ;",
        ] {
            let m = b.mark();
            b.send(0, key);
            assert_eq!(b.key(0), None, "{key}");
            assert!(b.texts(0, m).is_empty(), "{key}: no echo");
            assert!(
                b.events(m)
                    .iter()
                    .any(|e| matches!(e, Event::Refused { why: w, .. } if *w == why)),
                "{key}"
            );
        }
        // A stop is still taken.
        let m = b.mark();
        b.send(0, "trx:0,false;");
        assert_eq!(b.texts(0, m), ["trx:0,false;"]);
    }
    // A stopped SDR refuses a key too.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "stop;trx:0,true;");
    assert_eq!(b.key(0), None);
}

#[test]
fn a_cw_macro_keys_for_its_timed_length_queues_behind_and_stops_on_cw_macros_stop() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "cw_macros_speed:20;cw_macros_delay:50;");
    let m = b.mark();
    b.send(0, "cw_macros:0,PARIS;");
    assert_eq!(b.key(0).map(|k| k.source), Some(Source::Cw));
    assert_eq!(b.texts(0, m), ["trx:0,true;"]);
    // PARIS at 20 wpm keys for 43 dits of 60 ms, after the 50 ms delay.
    assert!(b.events(m).contains(&Event::CwStarted {
        trx: 0,
        text: "PARIS".into(),
        ms: 2580
    }));
    b.send(0, "cw_macros:0,E;");
    b.advance(ms(50 + 2580 - 1));
    assert!(b.key(0).is_some());
    b.advance(ms(2));
    // The queued E follows at once, still keyed.
    assert!(b.key(0).is_some());
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::CwStarted { text, .. } if text == "E")));
    b.advance(ms(61));
    assert_eq!(b.key(0), None);
    assert_eq!(
        b.texts(0, m).last().map(String::as_str),
        Some("trx:0,false;")
    );
    // cw_macros_stop ends it mid-word and unkeys.
    b.send(0, "cw_macros:0,TEST TEST;");
    b.advance(ms(300));
    let m = b.mark();
    b.send(0, "cw_macros_stop;");
    assert_eq!(b.key(0), None);
    assert!(b.events(m).contains(&Event::CwEnded {
        trx: 0,
        stopped: true
    }));
    // Escaped reserved characters are turned back.
    let m = b.mark();
    b.send(0, "cw_macros:0,5NN~ TU;");
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::CwStarted { text, .. } if text == "5NN, TU")));
}

#[test]
fn a_held_cw_key_survives_cw_macros_stop_until_an_unkey() {
    let mut b = Bench::new(Dialect::expert_sdr3(), vec![Fault::HoldsCw]);
    b.connect(0);
    b.send(0, "cw_macros:0,E;");
    b.advance(ms(1000));
    assert_eq!(
        b.key(0).map(|k| k.source),
        Some(Source::Cw),
        "held after the text"
    );
    b.send(0, "cw_macros:0,TEST;cw_macros_stop;");
    assert!(b.key(0).is_some(), "held through the stop");
    let m = b.mark();
    b.send(0, "trx:0,false;");
    assert_eq!(b.key(0), None);
    assert_eq!(b.texts(0, m), ["trx:0,false;"]);
}

#[test]
fn a_cw_message_sends_its_callsign_and_takes_a_correction_before_the_callsign_starts() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "cw_macros_speed:20;cw_macros_delay:0;");
    let m = b.mark();
    b.send(0, "cw_msg:0,TU,RA6$2,599;");
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::CwStarted { text, .. } if text == "TU RA6 RA6 599")));
    b.send(0, "cw_msg:RA6LH$2;");
    b.advance(ms(20_000));
    let texts = b.texts(0, m);
    // The callsign as finally sent, once, however many times it was repeated (spec §3.2.2).
    assert!(
        texts.contains(&"callsign_send:RA6LH;".to_string()),
        "{texts:?}"
    );
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::CwStarted { text, .. } if text == "TU RA6LH RA6LH 599")));
    // Too late once the callsign has started.
    b.send(0, "cw_msg:0,TU,K1ABC,599;");
    b.advance(ms(1_500));
    let m = b.mark();
    b.send(0, "cw_msg:K1XYZ;");
    assert!(b.events(m).iter().any(|e| matches!(
        e,
        Event::Refused {
            why: Refusal::TooLate,
            ..
        }
    )));
}

#[test]
fn the_hold_refuses_another_clients_write_for_its_window_and_not_the_holders() {
    let mut b = Bench::new(
        Dialect::expert_sdr3(),
        vec![Fault::Hold200ms { window: ms(200) }],
    );
    b.connect(0);
    b.connect(1);
    b.operator("vfo:0,0,7010000;");
    let m = b.mark();
    b.send(0, "vfo:0,0,7020000;");
    assert!(b.texts(0, m).is_empty());
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::Held { conn: 0, .. })));
    b.advance(ms(201));
    b.send(0, "vfo:0,0,7020000;");
    assert_eq!(b.r.trx[0].vfo[0], 7_020_000);
    // Client 0 now holds it: client 1 is held back, client 0 is not.
    let m = b.mark();
    b.send(1, "vfo:0,0,7030000;");
    b.send(0, "vfo:0,0,7040000;");
    assert_eq!(b.r.trx[0].vfo[0], 7_040_000);
    assert!(b
        .events(m)
        .iter()
        .any(|e| matches!(e, Event::Held { conn: 1, .. })));
}

#[test]
fn a_band_change_restores_the_bands_drive_under_band_restore() {
    let mut b = Bench::new(
        Dialect::expert_sdr3(),
        vec![Fault::BandRestore {
            drive: 100,
            tune_drive: 80,
            modulation: Some("CW".into()),
        }],
    );
    b.connect(0);
    let m = b.mark();
    b.send(0, "vfo:0,0,14080000;"); // same band: nothing restored
    assert_eq!(
        b.texts(0, m),
        ["vfo:0,0,14080000;", "tx_frequency:14080000;"]
    );
    let m = b.mark();
    b.send(0, "vfo:0,0,7030000;");
    assert_eq!(
        b.texts(0, m),
        [
            "vfo:0,0,7030000;",
            "modulation:0,CW;",
            "drive:0,100;",
            "tune_drive:0,80;",
            "tx_frequency:7030000;"
        ]
    );
    assert!(b.events(m).contains(&Event::BandChanged {
        trx: 0,
        band: "40m",
        restored: true
    }));
    assert_eq!((b.r.trx[0].drive, b.r.trx[0].tune_drive), (100, 80));
}

#[test]
fn sensor_pushes_follow_the_dialect_and_tx_power_follows_the_drive() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(
        0,
        "rx_sensors_enable:true,100;tx_sensors_enable:true,100;drive:0,40;",
    );
    let m = b.mark();
    b.advance(ms(100));
    let texts = b.texts(0, m);
    assert!(
        texts.contains(&"rx_channel_sensors:0,0,-73.0;".to_string()),
        "{texts:?}"
    );
    assert!(texts.contains(&"rx_channel_sensors:1,1,-73.0;".to_string()));
    assert!(texts.contains(&"tx_sensors:0,-30.0,0.0,0.0,1.0;".to_string()));
    b.send(0, "trx:0,true;");
    let m = b.mark();
    b.advance(ms(100));
    assert!(b
        .texts(0, m)
        .contains(&"tx_sensors:0,-30.0,40.0,40.0,1.2;".to_string()));
    // Disabled: nothing more.
    b.send(0, "rx_sensors_enable:false;tx_sensors_enable:false;");
    let m = b.mark();
    b.advance(ms(500));
    assert!(b.texts(0, m).is_empty());
    // piHPSDR pushes both spellings once a second, whatever interval is asked for.
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    b.send(0, "rx_sensors_enable:true,50;");
    let m = b.mark();
    b.advance(ms(999));
    assert!(b.texts(0, m).is_empty());
    b.advance(ms(1));
    assert_eq!(
        b.texts(0, m),
        [
            "rx_sensors:0,-73.0;",
            "rx_channel_sensors:0,0,-73.0;",
            "rx_channel_sensors:0,1,-73.0;"
        ]
    );
    // The older poll: answered where the dialect does, ignored where it does not.
    let m = b.mark();
    b.send(0, "rx_smeter:0,0;");
    assert_eq!(b.texts(0, m), ["rx_smeter:0,0,-73.0;"]);
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "rx_smeter:0,0;");
    assert!(b.texts(0, m).is_empty());
}

#[test]
fn spots_are_kept_replaced_deleted_and_cleared_and_a_click_is_sent_in_the_dialects_spellings() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "spot:K1ABC,CW,14025000,4294901760,needed;spot:k1abc,CW,14026000,0,;spot:W2X,FT8,14074500,0;");
    let calls: Vec<(String, u64)> = b.r.spots.iter().map(|s| (s.call.clone(), s.hz)).collect();
    assert_eq!(
        calls,
        [
            ("k1abc".to_string(), 14_026_000),
            ("W2X".to_string(), 14_074_500)
        ]
    );
    b.send(0, "spot_delete:K1ABC;");
    assert_eq!(b.r.spots.len(), 1);
    b.send(0, "spot_clear;");
    assert!(b.r.spots.is_empty());
    let m = b.mark();
    b.r.click("W2X", 14_074_500, b.now);
    b.absorb();
    assert_eq!(
        b.texts(0, m),
        [
            "clicked_on_spot:W2X,14074500;",
            "rx_clicked_on_spot:0,0,W2X,14074500;"
        ]
    );
    // A click never keys anything.
    assert_eq!(b.key(0), None);
}

#[test]
fn a_click_names_its_transceiver_and_channel_only_in_the_receiver_spelling() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.r.click_on(1, 1, "W2X", 7_030_000, b.now);
    b.absorb();
    assert_eq!(
        b.texts(0, m),
        [
            "clicked_on_spot:W2X,7030000;",
            "rx_clicked_on_spot:1,1,W2X,7030000;"
        ]
    );
    // A server that speaks only the legacy spelling names neither.
    let mut b = Bench::new(Dialect::expert_sdr2(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.r.click_on(1, 1, "W2X", 7_030_000, b.now);
    b.absorb();
    assert_eq!(b.texts(0, m), ["clicked_on_spot:W2X,7030000;"]);
}

#[test]
fn aethersdr_keeps_every_spot_deletes_by_call_and_frequency_and_draws_only_a_hash_colour() {
    let mut b = Bench::new(Dialect::aether_sdr(), Vec::new());
    b.connect(0);
    b.send(
        0,
        "spot:K1ABC,CW,14025000,#FFF23EC0,needed;spot:K1ABC,CW,14026000,4294065856,;\
         spot:W2X,FT8,14074500,#00FF00,;",
    );
    let spots = |b: &Bench| -> Vec<(String, u64, Option<u32>)> {
        b.r.spots
            .iter()
            .map(|s| (s.call.clone(), s.hz, s.drawn))
            .collect()
    };
    assert_eq!(
        spots(&b),
        [
            ("K1ABC".to_string(), 14_025_000, Some(0xFFF2_3EC0)),
            ("K1ABC".to_string(), 14_026_000, None),
            ("W2X".to_string(), 14_074_500, Some(0xFF00_FF00)),
        ]
    );
    b.send(0, "spot_delete:K1ABC;");
    assert_eq!(spots(&b).len(), 3, "a delete with no frequency is ignored");
    b.send(0, "spot_delete:K1ABC,14026050;");
    assert_eq!(
        spots(&b),
        [
            ("K1ABC".to_string(), 14_025_000, Some(0xFFF2_3EC0)),
            ("W2X".to_string(), 14_074_500, Some(0xFF00_FF00)),
        ],
        "the K1ABC within 100 Hz"
    );
    b.send(0, "spot_clear;");
    assert!(spots(&b).is_empty());
    // CONTROL: the spec's store keeps one spot per call, draws the decimal colour, refuses `#`.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(
        0,
        "spot:K1ABC,CW,14025000,4294065856,;spot:K1ABC,CW,14026000,4294065856,;\
         spot:W2X,FT8,14074500,#FF00FF00,;spot_delete:W2X,14074500;",
    );
    assert_eq!(
        spots(&b),
        [("K1ABC".to_string(), 14_026_000, Some(0xFFF2_3EC0))]
    );
}

#[test]
fn stream_settings_are_answered_shape_the_frames_and_pihpsdr_refuses_its_unusual_ones() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "audio_samplerate:12000;audio_stream_channels:1;audio_stream_sample_type:int16;audio_start:0;");
    assert_eq!(
        b.texts(0, m),
        [
            "audio_samplerate:12000;",
            "audio_stream_channels:1;",
            "audio_stream_sample_type:int16;",
            "audio_start:0;"
        ]
    );
    b.advance(ms(1000));
    let frames = b.frames(0, m);
    let rx: Vec<_> = frames
        .iter()
        .filter(|(_, h, _)| h.kind == kind::RX_AUDIO)
        .collect();
    // 512 samples a frame at 12 kHz, mono, int16: 42.7 ms a frame.
    assert_eq!(rx.len(), 23);
    for (k, (at, _, _)) in rx.iter().enumerate() {
        let want = Duration::from_secs_f64((k + 1) as f64 * 512.0 / 12_000.0);
        assert!(
            at.abs_diff(want) < Duration::from_micros(10),
            "frame {k} at {at:?}"
        );
    }
    let (_, h, payload) = rx[0];
    assert_eq!(
        (h.sample_rate, h.channels, h.length, h.format, *payload),
        (12_000, 1, 512, 0, 1024)
    );
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "audio_samplerate:12000;audio_start:0;");
    assert_eq!(
        b.texts(0, m),
        ["audio_samplerate:12000;"],
        "no acknowledgement"
    );
    assert!(b.events(m).iter().any(|e| matches!(
        e,
        Event::Refused {
            why: Refusal::Format,
            ..
        }
    )));
}

#[test]
fn an_unmodelled_parameter_is_kept_broadcast_and_read_back() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.connect(1);
    let m = b.mark();
    b.send(0, "agc_mode:0,fast;");
    assert_eq!(b.texts(1, m), ["agc_mode:0,fast;"]);
    let m = b.mark();
    b.send(1, "agc_mode:0;volume;");
    assert_eq!(b.texts(1, m), ["agc_mode:0,fast;", "volume:-12;"]);
    // A notice only a server sends, and a name nobody knows, are ignored.
    let m = b.mark();
    b.send(1, "tx_enable:0,true;not_a_command:1;");
    let whys: Vec<Why> = b
        .events(m)
        .into_iter()
        .filter_map(|e| match e {
            Event::Ignored { why, .. } => Some(why),
            _ => None,
        })
        .collect();
    assert_eq!(whys, [Why::ServerOnly, Why::Unknown]);
}

#[test]
fn the_legacy_drive_has_no_transceiver() {
    let mut b = Bench::new(Dialect::expert_sdr2(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(0, "drive:30;drive;drive:0,40;");
    assert_eq!(b.texts(0, m), ["drive:30;", "drive:30;"]);
    assert_eq!(b.r.trx[0].drive, 30);
}

#[test]
fn a_carrier_moves_in_the_receive_audio_with_the_dial() {
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "audio_stream_channels:1;audio_start:0;");
    let m = b.mark();
    b.advance(ms(500));
    let audio = |b: &Bench, m: usize| -> Vec<f32> {
        b.fx[m..]
            .iter()
            .filter_map(|(_, e)| match e {
                Effect::Frame { to: 0, bytes } => {
                    let h = Header::decode(bytes)?;
                    (h.kind == kind::RX_AUDIO).then(|| {
                        frame::decode_samples(SampleType::Float32, &bytes[frame::HEADER_LEN..])
                            .unwrap()
                    })
                }
                _ => None,
            })
            .flatten()
            .collect()
    };
    let at_1k = audio(&b, m);
    assert!(synth::tone_level(&at_1k, 48_000, 1_000.0) > 0.02);
    b.send(0, "vfo:0,0,14073500;"); // the 14.075 MHz carrier is now 1.5 kHz up
    let m = b.mark();
    b.advance(ms(500));
    let at_1500 = audio(&b, m);
    assert!(synth::tone_level(&at_1500, 48_000, 1_500.0) > 0.02);
    assert!(synth::tone_level(&at_1500, 48_000, 1_000.0) < 0.005);
}

#[test]
fn pihpsdr_acts_only_on_the_commands_in_its_table() {
    let mut b = Bench::new(Dialect::pihpsdr(), Vec::new());
    b.connect(0);
    let m = b.mark();
    b.send(
        0,
        "iq_start:0;spot:K1ABC,CW,14025000,0,;keyer:0,true,0;start;",
    );
    b.advance(ms(200));
    assert!(b.texts(0, m).is_empty());
    assert!(b.frames(0, m).is_empty(), "no IQ");
    assert!(b.r.spots.is_empty() && b.key(0).is_none());
    let unsupported = b
        .events(m)
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Ignored {
                    why: Why::Unsupported,
                    ..
                }
            )
        })
        .count();
    assert_eq!(unsupported, 4);
    // What is in its table is answered as before, and its own operator is not limited by it.
    b.send(0, "vfo:0,0,7074000;");
    assert_eq!(b.r.trx[0].vfo[0], 7_074_000);
    b.operator("start;");
    assert!(b.r.running);
}

#[test]
fn chrono_burst_and_chrono_stall_shape_the_pull_exactly() {
    // Bursts of four, every four periods: 0, 85.3, 170.7 ms, four at each.
    let mut b = Bench::new(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoBurst { burst: 4 }],
    );
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    b.advance(ms(200));
    let times = chronos(&b.frames(0, m));
    assert_eq!(times.len(), 12, "{times:?}");
    for (i, at) in times.iter().enumerate() {
        let group = (i / 4) as f64;
        let want = Duration::from_secs_f64(group * 4.0 * 1024.0 / 48_000.0);
        assert!(
            at.abs_diff(want) < Duration::from_micros(10),
            "chrono {i} at {at:?}"
        );
    }
    // A stall after 100 ms: chronos at 0, 21.3 … 85.3 ms, then none; the key stays.
    let mut b = Bench::new(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoStall { after: ms(100) }],
    );
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    b.advance(ms(500));
    assert_eq!(chronos(&b.frames(0, m)).len(), 5);
    assert!(b.key(0).is_some());
    // A late first chrono: 300 ms after the echo, then the cadence.
    let mut b = Bench::new(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoLate { delay: ms(300) }],
    );
    b.connect(0);
    b.send(0, "audio_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    b.advance(ms(400));
    let times = chronos(&b.frames(0, m));
    assert_eq!(times.first(), Some(&ms(300)));
    assert_eq!(times.len(), 5, "300, 321, 343, 364, 385 ms");
}

#[test]
fn a_radio_that_runs_late_gives_up_what_it_cannot_send_in_time_and_keeps_to_its_clock() {
    // The radio's turn comes a second late, as in a process starved of CPU: the receive audio, IQ
    // and TX_CHRONOs due meanwhile are not all sent at once. Each stream gives up what is too
    // late and goes on from its next frame on its own grid, as an SDR program whose buffer
    // overflowed would: its clock did not wait for it.
    let mut b = Bench::new(Dialect::expert_sdr3(), Vec::new());
    b.connect(0);
    b.send(0, "audio_start:0;iq_start:0;");
    let m = b.mark();
    b.send(0, "trx:0,true,tci;");
    let t0 = b.now - b.t0;
    b.advance(ms(100));
    // 2048 values a frame, two channels: 1024 frames, 21.333 ms, for each of the three. Receive
    // audio and IQ at 21, 43, 64, 85 ms; chronos from the key at 0 ms.
    let period = 1024.0 / 48_000.0;
    assert_eq!(b.frames(0, m).len(), 4 + 4 + 5);
    let m = b.mark();
    b.now += ms(1000);
    b.advance(Duration::ZERO);
    let at_once = b.frames(0, m);
    assert!(
        at_once.is_empty(),
        "{} frames went out at once, up to a second late",
        at_once.len()
    );
    // Each stream says what it gave up: the 47 frames due from 106.7 ms to 1088 ms.
    let mut lost: Vec<(u32, u64)> = b
        .events(m)
        .iter()
        .filter_map(|e| match e {
            Event::Lost { kind, frames, .. } => Some((*kind, *frames)),
            _ => None,
        })
        .collect();
    lost.sort_unstable();
    assert_eq!(
        lost,
        [(kind::IQ, 47), (kind::RX_AUDIO, 47), (kind::TX_CHRONO, 47)]
    );
    // From the next frame due: 1109.3, 1130.7, 1152, 1173.3 and 1194.7 ms, for each of the three.
    b.advance(ms(100));
    let resumed = b.frames(0, m);
    assert_eq!(resumed.len(), 3 * 5, "{resumed:?}");
    for (at, h, _) in &resumed {
        let n = (*at - t0).as_secs_f64() / period;
        assert!(
            (n - n.round()).abs() < 1e-3 && n.round() >= 52.0,
            "kind {} at {at:?} is off its grid",
            h.kind
        );
    }
}
