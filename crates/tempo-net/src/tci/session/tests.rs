//! The session's rules on a manual clock: every window, deadline and readback decided by the
//! times given, with the server's messages typed in and everything the session writes recorded.
//! The simulator suites (`sim_tests`) run the same rules over a real WebSocket.

use super::*;
use crate::tci::frame::{decode, encode_block, BlockHeader, Channels, LengthMeaning, SampleFormat};
use crate::tci::{EXPERT_SDR2, THETIS_NATIVE};

fn trx(n: u32) -> Trx {
    Trx::new(n).expect("a receiver number")
}

/// What the session wrote.
#[derive(Debug, Clone, PartialEq)]
enum Out {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Close,
}

/// A recording transport; `broken` fails every write.
#[derive(Default)]
struct Wire {
    out: Vec<Out>,
    broken: bool,
}

impl Outbox for Wire {
    fn text(&mut self, text: &ClientText) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Text(text.as_str().to_string()));
        true
    }

    fn binary(&mut self, frame: Vec<u8>) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Binary(frame));
        true
    }

    fn ping(&mut self) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Ping);
        true
    }

    fn close(&mut self) {
        self.out.push(Out::Close);
    }
}

impl Wire {
    /// Everything written since the last look.
    fn take(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The text messages written since the last look (the rest is forgotten too).
    fn texts(&mut self) -> Vec<String> {
        self.take()
            .into_iter()
            .filter_map(|o| match o {
                Out::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }
}

/// Transmit audio at a constant level; counts what it is given and asked.
#[derive(Default)]
struct Level {
    transmits: usize,
    received: usize,
    iq: usize,
}

const LEVEL: f32 = 0.25;

impl Streams for Level {
    fn receive(&mut self, _block: &Block<'_>) {
        self.received += 1;
    }

    fn iq(&mut self, _block: &Block<'_>) {
        self.iq += 1;
    }

    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        assert_eq!(rate, 48_000);
        self.transmits += 1;
        out.fill(LEVEL);
        out.len()
    }
}

/// An ExpertSDR3 burst for a radio ready to transmit on receiver 0, up to `ready;`.
const BURST: &str = "protocol:ExpertSDR3,2.0;device:SunSDR2DX;receive_only:false;trx_count:2;\
                     tx_enable:0,true;trx:0,false;tune:0,false;drive:0,50;tune_drive:0,10;start;";

fn block(trx: u32, kind: BlockKind) -> Vec<u8> {
    let header = BlockHeader {
        trx: Trx::new(trx).expect("trx"),
        kind,
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
    };
    encode_block(&header, LengthMeaning::Values, &[0.0; 512]).expect("a block")
}

fn chrono(trx: u32) -> Vec<u8> {
    Chrono {
        trx: Trx::new(trx).expect("trx"),
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
        length: 2048,
    }
    .encode()
    .expect("a chrono")
}

struct Bench {
    s: Session,
    wire: Wire,
    streams: Level,
    now: u64,
}

impl Bench {
    fn new(config: Config) -> Bench {
        Bench {
            s: Session::new(config, 0),
            wire: Wire::default(),
            streams: Level::default(),
            now: 0,
        }
    }

    /// Ready on receiver 0, `audio_start` acknowledged and one receive block in, at 3 ms; the
    /// setup it wrote and its events forgotten.
    fn ready(config: Config) -> Bench {
        let mut b = Bench::new(config);
        b.at(1).text(BURST);
        b.text("ready;");
        b.at(2).text("audio_start:0;");
        b.at(3).rx();
        b.wire.take();
        b.events();
        b
    }

    fn at(&mut self, ms: u64) -> &mut Bench {
        self.now = ms;
        self
    }

    fn text(&mut self, text: &str) {
        self.s.on_text(&mut self.wire, text, self.now);
    }

    fn binary(&mut self, bytes: &[u8]) {
        self.s
            .on_binary(&mut self.wire, &mut self.streams, bytes, self.now);
    }

    fn rx(&mut self) {
        self.binary(&block(0, BlockKind::RxAudio));
    }

    fn poll(&mut self) {
        self.s.poll(&mut self.wire, self.now);
    }

    fn key(&mut self, start: TxStart) -> Result<(), Refusal> {
        self.s.key(&mut self.wire, start, self.now)
    }

    fn end_ours(&mut self) -> StopOutcome {
        self.s.end_ours(&mut self.wire, self.now)
    }

    fn close(&mut self) {
        self.s.close(&mut self.wire, self.now);
    }

    fn events(&mut self) -> Vec<Event> {
        self.s.take_events()
    }

    /// A TX_CHRONO for `trx`, and the answer's samples (channels interleaved) if one went out.
    fn chrono(&mut self, trx: u32) -> Option<Vec<f32>> {
        self.binary(&chrono(trx));
        let answers: Vec<Vec<u8>> = self
            .wire
            .take()
            .into_iter()
            .filter_map(|o| match o {
                Out::Binary(b) => Some(b),
                _ => None,
            })
            .collect();
        match answers.as_slice() {
            [] => None,
            [bytes] => match decode(bytes).expect("our answer decodes") {
                Frame::Block(b) => {
                    assert_eq!(b.header.kind, BlockKind::TxAudio);
                    assert_eq!(b.header.trx, Trx::new(trx).expect("trx"));
                    assert_eq!(b.header.channels, Channels::Two);
                    assert_eq!(b.reading, Some(LengthMeaning::Values));
                    Some(b.samples().collect())
                }
                Frame::Chrono(_) => panic!("a chrono answered with a chrono"),
            },
            more => panic!("{} answers to one chrono", more.len()),
        }
    }
}

fn stop_trx0() -> StopOutcome {
    StopOutcome::Sent(vec![Stop::Trx(trx(0))])
}

fn closed(end: End, carried: Option<TxStart>) -> Event {
    Event::Closed {
        end,
        carried: carried.map(|kind| OurKey { trx: trx(0), kind }),
    }
}

#[test]
fn the_burst_reaches_ready_and_the_setup_goes_out_in_order() {
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    assert!(b.wire.texts().is_empty(), "nothing is written before ready");
    assert_eq!(b.s.phase(), Phase::AwaitReady);
    b.text("ready;");
    assert_eq!(b.s.phase(), Phase::Ready);
    assert_eq!(
        b.wire.texts(),
        [
            "audio_start:0;",
            "rx_sensors_enable:true,200;",
            "tx_sensors_enable:true,200;"
        ]
    );
    assert_eq!(
        b.events(),
        [Event::Ready {
            dialect: EXPERT_SDR3,
            program: Some("ExpertSDR3".into()),
            version: Some("2.0".into()),
            device: Some("SunSDR2DX".into()),
        }]
    );
    // A second ready changes nothing.
    b.text("ready;");
    assert!(b.wire.texts().is_empty());
    assert!(b.events().is_empty());

    // Thetis announcing itself, Nexus's audio off, receiver 1: no stream, the S-meter polled.
    let mut b = Bench::new(Config {
        audio: false,
        ..Config::new(trx(1))
    });
    b.at(1).text("protocol:Thetis,2.10.3.14;ready;");
    assert!(b.wire.texts().is_empty());
    assert!(matches!(
        b.events().as_slice(),
        [Event::Ready { dialect, .. }] if *dialect == THETIS_NATIVE
    ));
    b.at(2).poll();
    assert_eq!(b.wire.texts(), ["rx_smeter:1,0;"]);
    b.at(501).poll();
    assert!(b.wire.texts().is_empty(), "not before METER_POLL_MS");
    b.at(502).poll();
    assert_eq!(b.wire.texts(), ["rx_smeter:1,0;"]);
}

#[test]
fn no_ready_in_time_ends_the_session_with_the_stops_first() {
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    b.at(4_999).poll();
    assert_eq!(b.s.phase(), Phase::AwaitReady);
    b.wire.take();
    b.at(5_000).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(b.events(), [closed(End::NoReady, None)]);
    // Closed: nothing more is written, stops included.
    assert_eq!(b.end_ours(), StopOutcome::NotConnected);
    b.at(9_000).poll();
    assert!(b.wire.take().is_empty());
}

#[test]
fn a_digital_key_is_answered_chrono_by_chrono_and_only_while_ours() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // No key of ours: a chrono gets nothing.
    assert_eq!(b.at(5).chrono(0), None);
    b.at(10).key(TxStart::Audio).expect("admitted");
    assert_eq!(b.wire.texts(), ["trx:0,true,tci;"]);
    assert!(b.s.ours_on_air() && b.s.audio_over_open());
    // Exactly the frames asked for (2048 values: 1024 stereo frames), Nexus's audio on both.
    let answer = b.at(12).chrono(0).expect("answered");
    assert_eq!(answer.len(), 2048);
    assert!(answer.iter().all(|s| (*s - LEVEL).abs() < 1e-6));
    // Another transceiver's chrono is not ours.
    assert_eq!(b.at(13).chrono(1), None);
    b.at(14).text("trx:0,true;");
    assert_eq!(b.events(), [Event::KeyConfirmed(TxStart::Audio)]);
    let snap = b.s.snapshot();
    assert!(snap.key_confirmed && snap.over_open);
    assert_eq!(
        snap.timing,
        Timing {
            echo_ms: Some(4),
            first_chrono_ms: Some(2)
        }
    );
    // Our stop: silence from now on, until the server reports the unkey.
    assert_eq!(b.at(50).end_ours(), stop_trx0());
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    assert!(!b.s.audio_over_open());
    let after_stop = b.at(52).chrono(0).expect("answered while keyed");
    assert!(
        after_stop.iter().all(|s| *s == 0.0),
        "silence after our stop"
    );
    b.at(60).text("trx:0,false;");
    assert_eq!(b.events(), [Event::Unkeyed]);
    assert!(!b.s.ours_on_air());
    // Unkeyed: nothing, not even silence.
    assert_eq!(b.at(62).chrono(0), None);
    assert_eq!(
        b.streams.transmits, 1,
        "Nexus's audio only while the over was open"
    );
    assert_eq!(b.s.snapshot().chronos_not_ours, 3);
}

#[test]
fn a_report_that_can_predate_the_key_is_not_proof_of_its_end() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Microphone).expect("admitted");
    // Crossed in flight: the server's `false` from before it read our key.
    b.at(11).text("trx:0,false;");
    assert!(b.s.ours_on_air(), "a report before the echo proves nothing");
    assert!(b.events().is_empty());
    b.at(12).text("trx:0,true;");
    assert_eq!(b.events(), [Event::KeyConfirmed(TxStart::Microphone)]);
    // After the echo, before our stop: the server ended the over itself.
    b.at(20).text("trx:0,false;");
    assert_eq!(
        b.events(),
        [Event::Halted(Halt::ServerEnded), Event::Unkeyed]
    );
    assert!(!b.s.ours_on_air());
    assert!(
        b.wire.texts().iter().all(|t| t != "trx:0,false;"),
        "no stop needed"
    );
}

#[test]
fn the_chrono_window_ends_a_digital_key_the_server_does_not_pull() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Audio).expect("admitted");
    b.at(101).text("trx:0,true;");
    b.wire.take();
    b.events();
    b.at(399).poll();
    assert!(b.wire.texts().is_empty(), "inside the window");
    b.at(400).poll();
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    assert_eq!(b.events(), [Event::Halted(Halt::NoChrono)]);
    assert!(b.s.ours_on_air(), "kept until the server reports the unkey");

    // A chrono inside the window keeps the over, for as long as it is pulled.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Audio).expect("admitted");
    b.at(101).text("trx:0,true;");
    assert!(b.at(399).chrono(0).is_some());
    b.events();
    for t in (400..1_400).step_by(100) {
        b.at(t).chrono(0);
        b.poll();
    }
    assert!(b.events().is_empty());
    assert!(b.s.audio_over_open());
    // A microphone key is not pulled: no chrono window.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Microphone).expect("admitted");
    b.at(101).text("trx:0,true;");
    b.events();
    b.at(1_000).poll();
    assert!(b.events().is_empty());
}

#[test]
fn the_echo_window_ends_a_key_the_server_does_not_echo() {
    for start in [TxStart::Microphone, TxStart::Tune, TxStart::Audio] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(100).key(start).expect("admitted");
        b.wire.take();
        for t in (200..1_600).step_by(100) {
            if start == TxStart::Audio {
                b.at(t).chrono(0);
            }
            b.at(t).rx();
            b.poll();
        }
        b.at(1_599).poll();
        assert!(b.events().is_empty(), "{start:?} inside the window");
        b.wire.take();
        b.at(1_600).poll();
        let stops = if start == TxStart::Tune {
            vec!["tune:0,false;", "trx:0,false;"]
        } else {
            vec!["trx:0,false;"]
        };
        assert_eq!(b.wire.texts(), stops, "{start:?}");
        assert_eq!(b.events(), [Event::Halted(Halt::NoEcho)], "{start:?}");
    }
}

#[test]
fn an_unkey_not_reported_in_time_is_sent_again_and_the_session_closes() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.at(100).end_ours();
    b.wire.take();
    b.events();
    for t in (200..2_100).step_by(100) {
        b.at(t).rx();
        b.poll();
    }
    assert_eq!(
        b.s.phase(),
        Phase::Ready,
        "the deadline is 2 s after the first stop"
    );
    b.wire.take();
    b.at(2_100).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close],
        "the stops again, then the close"
    );
    assert_eq!(
        b.events(),
        [
            Event::UnkeyUnconfirmed,
            closed(End::UnkeyUnconfirmed, Some(TxStart::Audio))
        ]
    );
    assert!(b.s.ours_on_air(), "the belief goes to the next session");
}

#[test]
fn stops_go_out_in_every_phase_but_closed() {
    // M13. Before ready, nothing of ours keyed: still sent.
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    assert_eq!(b.end_ours(), stop_trx0());
    assert_eq!(
        b.s.stop(&mut b.wire, Stop::Tune(trx(0)), 2),
        StopOutcome::Sent(vec![Stop::Tune(trx(0))])
    );
    assert_eq!(
        b.s.stop(&mut b.wire, Stop::CwMacros, 3),
        StopOutcome::Sent(vec![Stop::CwMacros])
    );
    assert_eq!(
        b.wire.texts(),
        ["trx:0,false;", "tune:0,false;", "cw_macros_stop;"]
    );
    // Ready and idle, and while another program transmits.
    b.at(4).text("ready;");
    b.text("trx:0,true;");
    b.wire.take();
    assert_eq!(b.end_ours(), stop_trx0());
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    // While closing.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Tune).expect("admitted");
    b.at(11).text("tune:0,true;");
    b.at(12).close();
    assert_eq!(b.s.phase(), Phase::Closing);
    b.wire.take();
    assert_eq!(
        b.end_ours(),
        StopOutcome::Sent(vec![Stop::Tune(trx(0)), Stop::Trx(trx(0))])
    );
    assert_eq!(b.wire.texts(), ["tune:0,false;", "trx:0,false;"]);
    // Closed: there is nothing to write to.
    b.s.transport_closed(20);
    assert_eq!(b.end_ours(), StopOutcome::NotConnected);
}

#[test]
fn a_lost_link_keeps_the_keyed_belief() {
    // M4, on the manual clock.
    for start in [TxStart::Audio, TxStart::Microphone, TxStart::Tune] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(10).key(start).expect("admitted");
        b.at(11).text(if start == TxStart::Tune {
            "tune:0,true;"
        } else {
            "trx:0,true;"
        });
        b.events();
        b.s.transport_closed(12);
        assert_eq!(b.events(), [closed(End::Lost, Some(start))], "{start:?}");
        assert!(b.s.ours_on_air(), "{start:?}");
        assert_eq!(
            b.s.snapshot().keyed,
            Some(OurKey {
                trx: trx(0),
                kind: start
            })
        );
    }
}

#[test]
fn a_carried_key_is_stopped_first_and_ends_on_the_servers_word() {
    let carried = OurKey {
        trx: trx(0),
        kind: TxStart::Audio,
    };
    // The burst says still keyed: stops first after ready, the belief held until the report.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    assert!(b.s.ours_on_air());
    b.at(1).text(&BURST.replace("trx:0,false;", "trx:0,true;"));
    assert!(b.wire.texts().is_empty());
    b.text("ready;");
    assert_eq!(
        b.wire.texts()[0],
        "trx:0,false;",
        "the first command after ready"
    );
    assert!(b.s.ours_on_air());
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::AlreadyKeyed),
        "nothing new while it may be up"
    );
    b.at(5).text("trx:0,false;");
    assert!(b.events().contains(&Event::Unkeyed));
    assert!(!b.s.ours_on_air());

    // The burst says unkeyed: the server's word on a new link ends the belief, and the stops
    // still go first.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    b.at(1).text(BURST);
    assert!(!b.s.ours_on_air());
    b.text("ready;");
    assert_eq!(b.wire.texts()[0], "trx:0,false;");

    // A carried tune on another receiver is stopped where it is.
    let mut b = Bench::new(Config {
        carried: Some(OurKey {
            trx: trx(1),
            kind: TxStart::Tune,
        }),
        ..Config::new(trx(0))
    });
    b.at(1).text(&format!("{BURST}tune:1,true;ready;"));
    assert_eq!(b.wire.texts()[..2], ["tune:1,false;", "trx:1,false;"]);
    assert_eq!(
        b.end_ours(),
        StopOutcome::Sent(vec![
            Stop::Tune(trx(1)),
            Stop::Trx(trx(1)),
            Stop::Trx(trx(0)),
        ])
    );
    b.at(3).text("tune:1,false;");
    assert!(!b.s.ours_on_air());
    // Unconfirmed, it escalates like any stop.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    b.at(1).text(&BURST.replace("trx:0,false;", "trx:0,true;"));
    b.text("ready;");
    b.events();
    for t in (100..2_000).step_by(100) {
        b.at(t).rx();
        b.text("trx:0,true;");
        b.poll();
    }
    b.at(2_001).poll();
    assert_eq!(
        b.events(),
        [
            Event::UnkeyUnconfirmed,
            closed(End::UnkeyUnconfirmed, Some(TxStart::Audio))
        ]
    );
}

#[test]
fn the_sdr_stopping_mid_over_halts_it_and_refuses_keys() {
    // M12, on the manual clock.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.events();
    b.at(20).text("stop;");
    assert_eq!(
        b.events(),
        [Event::Running(false), Event::Halted(Halt::SdrStopped)]
    );
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    b.at(21).text("trx:0,false;");
    assert_eq!(b.events(), [Event::Unkeyed]);
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::SdrStopped { unreadable: false })
    );
    b.at(30).text("start;");
    assert_eq!(b.events(), [Event::Running(true)]);
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}

#[test]
fn silence_while_keyed_stops_the_over_and_a_stale_link_closes() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.events();
    b.wire.take();
    b.at(1_510).poll();
    assert!(b.events().is_empty(), "1499 ms of silence");
    b.at(1_511).poll();
    assert_eq!(b.events(), [Event::Halted(Halt::Silent)]);
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    b.at(5_010).poll();
    assert_eq!(b.s.phase(), Phase::Ready);
    b.wire.take();
    b.at(5_011).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(
        b.events(),
        [closed(End::Stale, Some(TxStart::Audio))],
        "a half-open link never clears the belief"
    );
}

#[test]
fn pings_go_out_and_anything_heard_keeps_the_link() {
    let mut b = Bench::ready(Config::new(trx(0)));
    let pings = |b: &mut Bench| b.wire.take().iter().filter(|o| **o == Out::Ping).count();
    b.at(999).poll();
    assert_eq!(pings(&mut b), 0);
    b.at(1_000).poll();
    assert_eq!(pings(&mut b), 1);
    // A pong (or anything) every second keeps it for good.
    for t in (1_500..20_000).step_by(1_000) {
        b.s.heard(t);
        b.at(t).poll();
    }
    assert_eq!(b.s.phase(), Phase::Ready);
    // A ping that cannot be written ends the session.
    b.wire.broken = true;
    b.at(21_000).poll();
    assert!(b.s.is_closed());
    assert_eq!(b.events(), [closed(End::Lost, None)]);
}

#[test]
fn an_echo_naming_another_source_ends_a_digital_over() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.wire.take();
    b.at(11).text("trx:0,true,mic1;");
    assert_eq!(
        b.events(),
        [
            Event::KeyConfirmed(TxStart::Audio),
            Event::Halted(Halt::WrongSource(Source::Mic1))
        ]
    );
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    // Naming Nexus's audio, or nothing, is fine; a microphone key may name a microphone.
    for (start, echo) in [
        (TxStart::Audio, "trx:0,true,tci;"),
        (TxStart::Audio, "trx:0,true;"),
        (TxStart::Microphone, "trx:0,true,mic2;"),
    ] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(10).key(start).expect("admitted");
        b.at(11).text(echo);
        assert_eq!(b.events(), [Event::KeyConfirmed(start)], "{echo}");
    }
}

#[test]
fn a_refused_readback_is_asked_again_at_most_once_a_second() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("trx:0,maybe;drive:0,-5;");
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::TransmitStateUnknown)
    );
    b.at(11).poll();
    assert_eq!(b.wire.texts(), ["trx:0;", "drive:0;"]);
    b.at(500).poll();
    assert!(b.wire.texts().is_empty());
    b.at(1_011).poll();
    assert_eq!(b.wire.texts(), ["trx:0;", "drive:0;"]);
    b.at(1_100).text("trx:0,false;drive:0,50;");
    b.at(2_200).poll();
    assert!(b.wire.texts().is_empty());
    b.rx();
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}

#[test]
fn a_command_cut_off_at_a_message_end_is_unknown() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // `trx:0,false;` cut across two messages: neither half is read as anything.
    b.at(10).text("vfo:0,0,14074000;trx:0,");
    b.text("false;");
    assert!(b.s.model().trx(trx(0)).transmitting.is_unreadable());
    assert_eq!(
        b.s.model().trx(trx(0)).vfo[0].known().map(|h| h.get()),
        Some(14_074_000),
        "the command before the cut is read"
    );
    b.at(11).poll();
    assert_eq!(b.wire.texts(), ["trx:0;"]);
    // piHPSDR's unterminated burst lines name nothing the model holds.
    let mut m = Bench::new(Config::new(trx(0)));
    m.at(1).text(BURST);
    m.text("audio_stream_channels: 2");
    m.text("ready;");
    assert_eq!(m.s.phase(), Phase::Ready);
    assert_eq!(cut_name("a;b:1,2"), Some("b"));
    assert_eq!(cut_name(" trx :0,"), Some("trx"));
    assert_eq!(cut_name("ready;"), None);
    assert_eq!(cut_name("x;!!:1"), None);
}

#[test]
fn another_programs_key_is_reported_and_gets_no_audio() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("trx:0,true;");
    assert_eq!(b.events(), [Event::ForeignKey { on: true }]);
    assert_eq!(
        b.key(TxStart::Audio),
        Err(Refusal::Transmitting { tune: false })
    );
    assert_eq!(b.at(11).chrono(0), None);
    // Another receiver's key is not ours to report.
    b.at(12).text("trx:1,true;");
    assert!(b.events().is_empty());
    b.at(13).text("trx:0,false;");
    assert_eq!(b.events(), [Event::ForeignKey { on: false }]);
    b.rx();
    assert_eq!(b.key(TxStart::Audio), Ok(()));
    assert_eq!(b.streams.transmits, 0);
}

#[test]
fn a_close_on_purpose_sends_the_stops_and_waits_for_the_unkey() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).text("trx:0,true;");
    b.wire.take();
    b.events();
    b.at(20).close();
    assert_eq!(b.wire.take(), [Out::Text("trx:0,false;".into())]);
    assert_eq!(b.s.phase(), Phase::Closing);
    assert_eq!(
        b.s.send(&mut b.wire, &Command::SpotClear, 21),
        Err(SendError::NotReady)
    );
    b.at(30).text("trx:0,false;");
    assert_eq!(b.wire.take(), [Out::Close]);
    assert_eq!(b.events(), [Event::Unkeyed, closed(End::Closed, None)]);

    // No report within the wait: closed anyway, the belief handed on.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).text("trx:0,true;");
    b.events();
    b.at(20).close();
    b.at(1_019).poll();
    assert_eq!(b.s.phase(), Phase::Closing);
    b.at(1_020).poll();
    assert_eq!(b.events(), [closed(End::Closed, Some(TxStart::Audio))]);

    // Idle: the stop, then the close at once.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).close();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(b.events(), [closed(End::Closed, None)]);
}

#[test]
fn a_failed_write_ends_the_session_and_keeps_the_belief() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.wire.broken = true;
    // The key's write fails: the belief was set before it.
    assert_eq!(b.at(10).key(TxStart::Audio), Ok(()));
    assert!(b.s.is_closed());
    assert_eq!(b.events(), [closed(End::Lost, Some(TxStart::Audio))]);
}

#[test]
fn the_drive_nexus_writes_is_the_ceiling_its_readback_is_held_to() {
    // M7, on the manual clock.
    let mut b = Bench::ready(Config::new(trx(0)));
    let drive = |b: &mut Bench, level: u32, t: u64| {
        let write = Command::Set(Setting::drive(trx(0), level, &EXPERT_SDR3));
        b.s.send(&mut b.wire, &write, t).expect("sent");
    };
    drive(&mut b, 30, 10);
    assert_eq!(b.wire.texts(), ["drive:0,30;"]);
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::DriveAboveCeiling {
            drive: 50,
            ceiling: 30,
            tune: false
        }),
        "until the server echoes it"
    );
    b.at(11).text("drive:0,30;");
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
    b.at(12).text("trx:0,true;");
    b.end_ours();
    b.at(13).text("trx:0,false;");
    // A band change restores the band's drive.
    b.at(20)
        .text("vfo:0,0,7074000;drive:0,100;tune_drive:0,100;");
    b.rx();
    assert_eq!(
        b.key(TxStart::Audio),
        Err(Refusal::DriveAboveCeiling {
            drive: 100,
            ceiling: 30,
            tune: false
        })
    );
    drive(&mut b, 30, 21);
    b.at(22).text("drive:0,30;");
    assert_eq!(b.key(TxStart::Audio), Ok(()));
    // A 1.x drive write is the ceiling too.
    let mut b = Bench::ready(Config::new(trx(0)));
    let write = Command::Set(Setting::drive(trx(0), 20, &EXPERT_SDR2));
    b.s.send(&mut b.wire, &write, 10).expect("sent");
    assert_eq!(b.s.snapshot().drive_ceiling, Some(20));
    // Another receiver's drive is not ours.
    let other = Command::Set(Setting::Drive {
        trx: Some(trx(1)),
        level: 5,
    });
    b.s.send(&mut b.wire, &other, 11).expect("sent");
    assert_eq!(b.s.snapshot().drive_ceiling, Some(20));
}

#[test]
fn cat_waits_for_ready_and_nothing_is_written_after_the_close() {
    let mut b = Bench::new(Config::new(trx(0)));
    let read = Command::Read(Query::Trx(trx(0)));
    assert_eq!(b.s.send(&mut b.wire, &read, 1), Err(SendError::NotReady));
    assert_eq!(b.key(TxStart::Microphone), Err(Refusal::NotReady));
    b.at(1).text(&format!("{BURST}ready;"));
    b.wire.take();
    assert_eq!(b.s.send(&mut b.wire, &read, 2), Ok(()));
    assert_eq!(b.wire.texts(), ["trx:0;"]);
    b.s.transport_closed(3);
    assert_eq!(b.s.send(&mut b.wire, &read, 4), Err(SendError::Closed));
    b.at(5).text("trx:0,true;");
    assert!(b.wire.take().is_empty());
}

#[test]
fn spot_clicks_are_reported_and_receive_blocks_reach_the_streams() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text(
        "clicked_on_spot:K1ABC,14025000;rx_clicked_on_spot:0,1,W1AW,7030000;\
         rx_clicked_on_spot:1,0,N0CALL,3530000;",
    );
    let calls: Vec<String> = b
        .events()
        .into_iter()
        .map(|e| match e {
            Event::SpotClicked { call, hz } => format!("{} {}", call.as_str(), hz.get()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        calls,
        ["K1ABC 14025000", "W1AW 7030000"],
        "receiver 1's is not ours"
    );
    assert!(b.wire.take().is_empty(), "a click writes nothing");
    let before = b.streams.received;
    b.at(11).binary(&block(0, BlockKind::RxAudio));
    b.binary(&block(1, BlockKind::RxAudio));
    b.binary(&block(0, BlockKind::Iq));
    assert_eq!(b.streams.received, before + 1);
    assert_eq!(b.streams.iq, 1);
    // A frame that does not decode is counted and dropped.
    b.binary(&[0u8; 10]);
    assert_eq!(b.s.snapshot().frames.refused, 1);
}

#[test]
fn receive_audio_must_be_fresh_for_a_digital_key() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // The last block came at 3 ms.
    b.at(1_003).text("trx:0,false;");
    assert!(b.s.admits(TxStart::Audio, 1_003).is_ok());
    assert_eq!(
        b.s.admits(TxStart::Audio, 1_004),
        Err(Refusal::NoReceiveAudio)
    );
    assert_eq!(
        b.at(1_004).key(TxStart::Audio),
        Err(Refusal::NoReceiveAudio)
    );
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}
