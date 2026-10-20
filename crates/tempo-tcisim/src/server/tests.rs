//! The simulator on the wire: real sockets, real WebSocket frames, the log it keeps. Each fault is
//! shown doing what it says, as a client sees it.

use super::*;
use crate::client::{Incoming, Peer};
use crate::event::{Keyer, Source};
use crate::frame::{self, fit, kind, Fit, SampleType};
use crate::synth::tone_level;

/// How long any one wait may take before a test fails. Generous: the suite runs in parallel.
const WAIT: Duration = Duration::from_secs(5);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn start(dialect: Dialect, faults: Vec<Fault>) -> Simulator {
    Simulator::start(Config::dialect(dialect).with(faults)).expect("the simulator starts")
}

/// A test's side of one connection: everything it read is kept in `seen`.
struct Client {
    peer: Peer,
    seen: Vec<Incoming>,
}

impl Client {
    fn connect(sim: &Simulator) -> Client {
        Client {
            peer: Peer::connect(sim.addr(), WAIT).expect("connects"),
            seen: Vec::new(),
        }
    }

    /// Connect and read through `ready;`.
    fn ready(sim: &Simulator) -> Client {
        let mut c = Client::connect(sim);
        c.text_until(|t| t.contains("ready;")).expect("ready");
        c
    }

    fn send(&mut self, text: &str) {
        self.peer.send_text(text).expect("sends");
    }

    /// Read until a message satisfies `pred` (within `within`); `None` if none did.
    fn until_within(
        &mut self,
        within: Duration,
        pred: impl Fn(&Incoming) -> bool,
    ) -> Option<Incoming> {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match self.peer.recv(left) {
                Ok(Some(m)) => {
                    self.seen.push(m.clone());
                    if pred(&m) {
                        return Some(m);
                    }
                    if m == Incoming::Closed {
                        return None;
                    }
                }
                Ok(None) | Err(_) => return None,
            }
        }
    }

    fn until(&mut self, pred: impl Fn(&Incoming) -> bool) -> Option<Incoming> {
        self.until_within(WAIT, pred)
    }

    fn text_until(&mut self, pred: impl Fn(&str) -> bool) -> Option<String> {
        match self.until(|m| matches!(m, Incoming::Text(t) if pred(t))) {
            Some(Incoming::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// The next frame of `kind`, with its header.
    fn frame(&mut self, kind: u32) -> Option<(Header, Vec<u8>)> {
        let found = self.until(|m| match m {
            Incoming::Binary(b) => Header::decode(b).is_some_and(|h| h.kind == kind),
            _ => false,
        });
        match found {
            Some(Incoming::Binary(b)) => Some((Header::decode(&b).unwrap(), b)),
            _ => None,
        }
    }

    /// Everything that arrives within `d`.
    fn drain(&mut self, d: Duration) -> Vec<Incoming> {
        let from = self.seen.len();
        self.until_within(d, |_| false);
        self.seen[from..].to_vec()
    }

    fn texts(&self) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|m| match m {
                Incoming::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }
}

fn texts(ms: &[Incoming]) -> Vec<String> {
    ms.iter()
        .filter_map(|m| match m {
            Incoming::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn headers(ms: &[Incoming], kind: u32) -> Vec<Header> {
    ms.iter()
        .filter_map(|m| match m {
            Incoming::Binary(b) => Header::decode(b).filter(|h| h.kind == kind),
            _ => None,
        })
        .collect()
}

/// When the simulator logged each frame of `kind` it sent to `conn`.
fn sent_frames(sim: &Simulator, conn: usize, kind: u32) -> Vec<Duration> {
    sim.log()
        .iter()
        .filter_map(|l| match &l.event {
            Event::FrameOut {
                conn: c,
                header: Some(h),
                ..
            } if *c == conn && h.kind == kind => Some(l.at),
            _ => None,
        })
        .collect()
}

/// When the simulator sent `conn` the first text containing `needle`.
fn sent_at(sim: &Simulator, conn: usize, needle: &str) -> Option<Duration> {
    sim.log().iter().find_map(|l| match &l.event {
        Event::TextOut { conn: c, text } if *c == conn && text.contains(needle) => Some(l.at),
        _ => None,
    })
}

/// A float32 stereo TX audio frame answering a chrono for `length` values: a `hz` tone.
fn answer(length: u32, hz: f64, phase: &mut u64) -> Vec<u8> {
    let frames = (length / 2) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        let v = (0.5 * (std::f64::consts::TAU * hz * *phase as f64 / 48_000.0).sin()) as f32;
        samples.extend([v, v]);
        *phase += 1;
    }
    Header {
        sample_rate: 48_000,
        format: SampleType::Float32.code(),
        length,
        kind: kind::TX_AUDIO,
        channels: 2,
        ..Header::default()
    }
    .frame(&frame::encode_samples(SampleType::Float32, &samples))
}

#[test]
fn a_client_gets_the_burst_then_its_answers_and_the_log_holds_both_in_order() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    let burst = c.texts();
    assert_eq!(
        burst.first().map(String::as_str),
        Some("vfo_limits:10000,30000000;")
    );
    assert_eq!(burst.last().map(String::as_str), Some("ready;"));
    assert!(burst.contains(&"protocol:ExpertSDR3,2.0;".to_string()));
    c.send("vfo:0,0,7074000;");
    assert_eq!(
        c.text_until(|t| t.starts_with("vfo:")).as_deref(),
        Some("vfo:0,0,7074000;")
    );
    assert!(sim.wait_for(WAIT, |log| log.iter().any(
        |l| matches!(&l.event, Event::TextOut { text, .. } if text == "vfo:0,0,7074000;")
    )));
    let log = sim.log();
    let place = |want: &dyn Fn(&Event) -> bool| log.iter().position(|l| want(&l.event)).unwrap();
    let connected = place(&|e| matches!(e, Event::Connected { conn: 0 }));
    let ready = place(&|e| matches!(e, Event::TextOut { text, .. } if text == "ready;"));
    let text_in = place(&|e| matches!(e, Event::TextIn { text, .. } if text == "vfo:0,0,7074000;"));
    let command = place(&|e| matches!(e, Event::Command { name, .. } if name == "vfo"));
    let echo = place(&|e| matches!(e, Event::TextOut { text, .. } if text == "vfo:0,0,7074000;"));
    assert!(connected < ready && ready < text_in && text_in < command && command < echo);
    assert_eq!(sim.commands(), ["vfo:0,0,7074000"]);
    assert_eq!(sim.commands_from(0), ["vfo:0,0,7074000"]);
}

#[test]
fn the_log_stays_in_time_order_and_every_answer_after_what_it_answers_under_streaming() {
    // Round trips, each command sent only once the last echo arrived, while receive audio streams
    // on the same connection, until at least ten frames went out among them: the log must show
    // every command after the echo that released it, and its times must never go backwards.
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    c.frame(kind::RX_AUDIO).expect("streaming");
    let frames = |c: &Client| {
        c.seen
            .iter()
            .filter(|m| matches!(m, Incoming::Binary(b) if Header::decode(b).is_some_and(|h| h.kind == kind::RX_AUDIO)))
            .count()
    };
    let before = frames(&c);
    let mut n = 0u64;
    let deadline = Instant::now() + WAIT;
    while n < 200 || (frames(&c) < before + 10 && Instant::now() < deadline) {
        let hz = 7_000_000 + n;
        c.send(&format!("vfo:0,0,{hz};"));
        let echo = format!("vfo:0,0,{hz};");
        c.text_until(|t| t == echo).expect("the echo");
        n += 1;
    }
    assert!(
        frames(&c) >= before + 10,
        "frames went out among the round trips"
    );
    let last = format!("vfo:0,0,{};", 7_000_000 + n - 1);
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(&l.event, Event::TextOut { text, .. } if *text == last))));
    let log = sim.log();
    for pair in log.windows(2) {
        assert!(
            pair[0].at <= pair[1].at,
            "out of order: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
    // Where each round trip's command and echo sit in the log, in one pass.
    let mut commands = std::collections::HashMap::new();
    let mut echoes = std::collections::HashMap::new();
    for (i, l) in log.iter().enumerate() {
        match &l.event {
            Event::Command { text, .. } => {
                if let Some(hz) = text.strip_prefix("vfo:0,0,") {
                    commands.insert(hz.to_string(), i);
                }
            }
            Event::TextOut { text, .. } => {
                if let Some(hz) = text
                    .strip_prefix("vfo:0,0,")
                    .and_then(|t| t.strip_suffix(';'))
                {
                    echoes.insert(hz.to_string(), i);
                }
            }
            _ => {}
        }
    }
    let mut last_echo = 0;
    for k in 0..n {
        let hz = (7_000_000 + k).to_string();
        let (cmd, out) = (commands[&hz], echoes[&hz]);
        assert!(
            last_echo < cmd && cmd < out,
            "round trip {k}: {last_echo} {cmd} {out}"
        );
        last_echo = out;
    }
    let first_cmd = commands["7000000"];
    let interleaved = log[first_cmd..last_echo]
        .iter()
        .filter(|l| matches!(&l.event, Event::FrameOut { header: Some(h), .. } if h.kind == kind::RX_AUDIO))
        .count();
    assert!(
        interleaved >= 10,
        "{interleaved} frames among the round trips"
    );
}

#[test]
fn a_change_reaches_every_client_and_a_read_only_the_asker() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut a = Client::ready(&sim);
    let mut b = Client::ready(&sim);
    a.send("modulation:0,digu;");
    assert_eq!(
        a.text_until(|t| t.starts_with("modulation")).as_deref(),
        Some("modulation:0,DIGU;")
    );
    assert_eq!(
        b.text_until(|t| t.starts_with("modulation")).as_deref(),
        Some("modulation:0,DIGU;")
    );
    b.send("drive:0;");
    assert_eq!(
        b.text_until(|t| t.starts_with("drive")).as_deref(),
        Some("drive:0,50;")
    );
    assert!(texts(&a.drain(ms(200)))
        .iter()
        .all(|t| !t.starts_with("drive")));
}

#[test]
fn a_tci_key_is_pulled_with_tx_chrono_and_the_answers_are_logged_and_kept() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    c.text_until(|t| t == "audio_start:0;")
        .expect("acknowledged");
    c.frame(kind::RX_AUDIO).expect("receive audio");
    c.send("trx:0,true,tci;");
    c.text_until(|t| t == "trx:0,true;").expect("echoed");
    let mut phase = 0;
    for _ in 0..10 {
        let (h, bytes) = c.frame(kind::TX_CHRONO).expect("a chrono");
        assert_eq!(bytes.len(), frame::HEADER_LEN, "a chrono is a header alone");
        assert_eq!((h.length, h.channels, h.sample_rate), (2048, 2, 48_000));
        c.peer
            .send_binary(answer(h.length, 1_000.0, &mut phase))
            .unwrap();
    }
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .filter(|l| matches!(l.event, Event::FrameIn { .. }))
        .count()
        >= 10));
    for e in sim.events() {
        if let Event::FrameIn {
            fit, asked, ours, ..
        } = e
        {
            assert_eq!((fit, asked, ours), (Fit::Values, Some(Fit::Values), true));
        }
    }
    let audio = sim.tx_audio(0);
    assert_eq!(audio.len(), 10 * 1024);
    assert!(tone_level(&audio, 48_000, 1_000.0) > 0.4);
    assert_eq!(
        sim.radio().trx[0].key.map(|k| (k.by, k.source)),
        Some((Keyer::Client(0), Source::Tci))
    );
}

#[test]
fn rx_audio_and_iq_carry_the_carrier_where_the_dial_and_the_panorama_put_it() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    c.send("audio_stream_channels:1;audio_start:0;iq_start:0;");
    let mut audio = Vec::new();
    let mut iq = Vec::new();
    while audio.len() < 9_600 || iq.len() < 9_600 {
        match c.until(|m| matches!(m, Incoming::Binary(_))) {
            Some(Incoming::Binary(b)) => {
                let h = Header::decode(&b).unwrap();
                let s =
                    frame::decode_samples(SampleType::Float32, &b[frame::HEADER_LEN..]).unwrap();
                match h.kind {
                    kind::RX_AUDIO => audio.extend(s),
                    kind::IQ => iq.extend(s),
                    _ => {}
                }
            }
            _ => panic!("the streams stopped"),
        }
    }
    // The carrier at 14.075 MHz, the dial at 14.074 MHz in USB: 1 kHz in the audio.
    assert!(tone_level(&audio, 48_000, 1_000.0) > 0.02);
    // The panorama centred at 14.080 MHz: −5 kHz in the IQ. Rotate it up by 5 kHz and it is DC.
    let n = iq.len() / 2;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for k in 0..n {
        let (i, q) = (f64::from(iq[2 * k]), f64::from(iq[2 * k + 1]));
        let w = std::f64::consts::TAU * 5_000.0 * k as f64 / 48_000.0;
        re += i * w.cos() - q * w.sin();
        im += i * w.sin() + q * w.cos();
    }
    let level = (re * re + im * im).sqrt() / n as f64;
    assert!((level - 10f64.powf(-30.0 / 20.0)).abs() < 0.003, "{level}");
}

#[test]
fn loopback_carries_the_transmitted_audio_back_into_the_receive_audio() {
    let config = Config {
        receive: Receive {
            carriers: Vec::new(),
            tones: Vec::new(),
            clip: None,
            loopback: true,
        },
        ..Config::default()
    };
    let sim = Simulator::start(config).unwrap();
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    let mut phase = 0;
    let mut heard = Vec::new();
    // Half a second of the loopback's audio is 24 answers from this thread: a loaded machine can
    // take many times the usual wait to send them.
    let deadline = Instant::now() + WAIT * 6;
    while heard.len() < 24_000 && Instant::now() < deadline {
        match c.until(|m| matches!(m, Incoming::Binary(_))) {
            Some(Incoming::Binary(b)) => {
                let h = Header::decode(&b).unwrap();
                if h.kind == kind::TX_CHRONO {
                    c.peer
                        .send_binary(answer(h.length, 700.0, &mut phase))
                        .unwrap();
                } else if h.kind == kind::RX_AUDIO && phase > 0 {
                    let s = frame::decode_samples(SampleType::Float32, &b[frame::HEADER_LEN..])
                        .unwrap();
                    // The loopback lays each answer where it arrives, or straight after the one
                    // before, so an answer later than any before it leaves a silence and moves
                    // the tone after it along, as on the air. How late each answer comes is this
                    // machine's scheduling (this client's, and the simulator's own writes of the
                    // chronos), not the loopback's, so the silences are left out: nothing else is
                    // on the band, and what remains must be the tone as it was sent.
                    heard.extend(s.iter().step_by(2).copied().filter(|v| *v != 0.0));
                }
            }
            _ => break,
        }
    }
    assert!(heard.len() >= 24_000, "{} samples came back", heard.len());
    let level = tone_level(&heard, 48_000, 700.0);
    assert!((level - 0.5).abs() < 0.01, "{level}");
}

#[test]
fn split_concat_sends_the_burst_as_one_message_cut_inside_the_named_command() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::SplitConcat {
            cut: Some(Cut {
                starting: "vfo:0,0,".into(),
                at: 9,
            }),
        }],
    );
    let mut c = Client::ready(&sim);
    let burst = c.texts();
    assert_eq!(burst.len(), 2, "{burst:?}");
    assert!(
        burst[0].ends_with(";vfo:0,0,1"),
        "{}",
        &burst[0][burst[0].len() - 40..]
    );
    assert!(burst[1].starts_with("4074000;vfo:0,1,14074000;"));
    assert!(burst.concat().contains(";vfo:0,0,14074000;"));
    // After the cut, what goes out together is still one message.
    c.send("vfo:0,0,7074000;");
    let echo = c.text_until(|t| t.contains("vfo:0,0,7074000;")).unwrap();
    assert_eq!(echo, "vfo:0,0,7074000;tx_frequency:7074000;");
}

#[test]
fn no_ready_ends_the_burst_without_ready() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::NoReady]);
    let mut c = Client::connect(&sim);
    c.text_until(|t| t == "start;")
        .expect("the burst's last line");
    assert!(texts(&c.drain(ms(300)))
        .iter()
        .all(|t| !t.contains("ready")));
    assert!(c.texts().iter().all(|t| !t.contains("ready")));
}

#[test]
fn no_echo_keys_and_unkeys_with_no_trx_line_and_still_pulls_audio() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::NoEcho]);
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    c.frame(kind::TX_CHRONO).expect("audio is pulled");
    assert!(sim.radio().trx[0].key.is_some());
    c.send("trx:0,true;trx:0;");
    let after = texts(&c.drain(ms(300)));
    c.send("trx:0,false;");
    let after_unkey = texts(&c.drain(ms(300)));
    assert!(sim.radio().trx[0].key.is_none());
    let trx_lines: Vec<&String> = after
        .iter()
        .chain(&after_unkey)
        .filter(|t| t.starts_with("trx"))
        .collect();
    assert!(trx_lines.is_empty(), "{trx_lines:?}");
}

#[test]
fn stuck_trx_answers_an_unkey_with_true_and_stays_keyed_for_the_next_connection() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::StuckTrx]);
    let mut c = Client::ready(&sim);
    c.send("trx:0,true;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    c.send("trx:0,false;");
    assert_eq!(
        c.text_until(|t| t.starts_with("trx")).as_deref(),
        Some("trx:0,true;")
    );
    c.peer.abort();
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(l.event, Event::Orphaned { trx: 0, conn: 0 }))));
    let next = Client::ready(&sim);
    assert!(next.texts().contains(&"trx:0,true;".to_string()));
    assert!(sim.radio().trx[0].key.is_some());
}

#[test]
fn no_audio_ack_answers_nothing_streams_nothing_and_a_tci_key_takes_the_microphone() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::NoAudioAck]);
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    let got = c.drain(ms(300));
    assert!(texts(&got).iter().all(|t| !t.starts_with("audio_start")));
    assert!(headers(&got, kind::RX_AUDIO).is_empty());
    c.send("trx:0,true,tci;");
    c.text_until(|t| t == "trx:0,true;")
        .expect("the radio keys");
    assert!(headers(&c.drain(ms(300)), kind::TX_CHRONO).is_empty());
    assert!(sim
        .events()
        .contains(&Event::MicFallback { conn: 0, trx: 0 }));
    assert_eq!(
        sim.radio().trx[0].key.map(|k| k.source),
        Some(Source::Mic(crate::event::Mic::Selected))
    );
}

#[test]
fn rx_audio_stall_stops_the_frames_while_the_stream_still_counts() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::RxAudioStall { after: ms(150) }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    c.text_until(|t| t == "audio_start:0;").unwrap();
    c.frame(kind::RX_AUDIO).expect("frames at first");
    c.drain(ms(400));
    assert!(
        headers(&c.drain(ms(300)), kind::RX_AUDIO).is_empty(),
        "frames after the stall"
    );
    assert!(sim.events().iter().any(|e| matches!(
        e,
        Event::Fault {
            name: "RxAudioStall",
            ..
        }
    )));
    assert_eq!(sim.radio().clients[&0].audio_running, [0]);
    // A tci key still takes TCI audio: the stream counts as running.
    c.send("trx:0,true,tci;");
    c.frame(kind::TX_CHRONO).expect("chrono");
}

#[test]
fn no_chrono_echoes_the_key_and_never_asks_for_audio() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::NoChrono]);
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    assert!(headers(&c.drain(ms(500)), kind::TX_CHRONO).is_empty());
    assert_eq!(sim.radio().trx[0].key.map(|k| k.source), Some(Source::Tci));
    assert!(sim.events().iter().any(|e| matches!(
        e,
        Event::Fault {
            name: "NoChrono",
            ..
        }
    )));
}

#[test]
fn chrono_late_holds_the_first_chrono_back_by_its_delay() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoLate { delay: ms(300) }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    c.frame(kind::TX_CHRONO).unwrap();
    // Measured from the radio's decision to key (the echo goes out with it, the dialect having no
    // echo delay); a write is stamped no earlier than it was due.
    let keyed = sim
        .log()
        .iter()
        .find(|l| matches!(l.event, Event::Keyed { .. }))
        .unwrap()
        .at;
    let first = sent_frames(&sim, 0, kind::TX_CHRONO)[0];
    assert!(
        first + ms(5) >= keyed + ms(300),
        "keyed {keyed:?}, first chrono {first:?}"
    );
    assert!(
        first < keyed + ms(300) + ms(250),
        "not far later either: {first:?}"
    );
}

#[test]
fn chrono_burst_sends_its_chronos_in_groups_at_the_same_average_rate() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoBurst { burst: 4 }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .filter(|l| matches!(&l.event, Event::FrameOut { header: Some(h), .. } if h.kind == kind::TX_CHRONO))
        .count()
        >= 12));
    // Exact times are the model's test; on the wire, under load, only the shape.
    let times = sent_frames(&sim, 0, kind::TX_CHRONO);
    for group in times.chunks(4).take(3) {
        assert!(
            group[3] - group[0] < ms(20),
            "a group goes out together: {group:?}"
        );
    }
    let gap = times[4] - times[0];
    assert!(
        gap > ms(40) && gap < ms(200),
        "groups four periods apart: {gap:?}"
    );
    c.drain(ms(10));
}

#[test]
fn chrono_stall_stops_asking_while_the_radio_stays_keyed() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::ChronoStall { after: ms(200) }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true,tci;");
    c.frame(kind::TX_CHRONO).unwrap();
    c.drain(ms(400));
    assert!(headers(&c.drain(ms(300)), kind::TX_CHRONO).is_empty());
    let n = sent_frames(&sim, 0, kind::TX_CHRONO).len();
    assert!(
        (1..=12).contains(&n),
        "at most about 200 ms of chronos: {n}"
    );
    assert!(sim.radio().trx[0].key.is_some());
    assert!(sim.events().iter().any(|e| matches!(
        e,
        Event::Fault {
            name: "ChronoStall",
            ..
        }
    )));
}

#[test]
fn length_as_frames_doubles_each_frames_payload_for_the_same_length() {
    for (faults, payload, asks) in [
        (Vec::new(), 2048 * 4, Fit::Values),
        (vec![Fault::LengthAsFrames], 2048 * 2 * 4, Fit::Frames),
    ] {
        let sim = start(Dialect::expert_sdr3(), faults);
        let mut c = Client::ready(&sim);
        c.send("audio_start:0;");
        let (h, bytes) = c.frame(kind::RX_AUDIO).unwrap();
        assert_eq!(
            (h.length, h.channels, bytes.len() - frame::HEADER_LEN),
            (2048, 2, payload)
        );
        c.send("trx:0,true,tci;");
        let (h, _) = c.frame(kind::TX_CHRONO).unwrap();
        assert_eq!(h.length, 2048);
        // The answer this reading asks for, 2048 values or 2048 frames of two channels, is the
        // one the simulator judges as fitting what it asked.
        let values = if asks == Fit::Values { 2048 } else { 4096 };
        assert_eq!(fit(h.length, h.channels, values), asks);
        let reply = Header {
            kind: kind::TX_AUDIO,
            ..h
        }
        .frame(&frame::encode_samples(
            SampleType::Float32,
            &vec![0.0; values],
        ));
        c.peer.send_binary(reply).unwrap();
        assert!(sim.wait_for(WAIT, |log| log.iter().any(
            |l| matches!(l.event, Event::FrameIn { asked: Some(a), .. } if a == asks)
        )));
    }
}

#[test]
fn format_code_4_marks_every_float32_frame_with_an_unknown_code() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::FormatCode4]);
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;iq_start:0;");
    let (audio, bytes) = c.frame(kind::RX_AUDIO).unwrap();
    assert_eq!(audio.format, 4);
    assert_eq!(
        bytes.len() - frame::HEADER_LEN,
        2048 * 4,
        "still float32 samples"
    );
    assert_eq!(c.frame(kind::IQ).unwrap().0.format, 4);
    assert_eq!(SampleType::from_code(4), None);
}

#[test]
fn a_foreign_key_is_seen_by_every_client_and_its_chronos_go_to_every_audio_stream() {
    let fault = Fault::ForeignKey {
        after: ms(300),
        hold: ms(300),
        chrono_to_all: true,
    };
    let sim = start(Dialect::expert_sdr3(), vec![fault]);
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    c.text_until(|t| t == "trx:0,true;")
        .expect("another program keys");
    assert_eq!(sim.radio().trx[0].key.map(|k| k.by), Some(Keyer::Operator));
    c.frame(kind::TX_CHRONO).expect("its chronos reach us");
    c.text_until(|t| t == "trx:0,false;")
        .expect("and it unkeys");
    // We answered none: the log shows no frame from us.
    assert!(!sim
        .events()
        .iter()
        .any(|e| matches!(e, Event::FrameIn { .. })));
    assert!(sim.commands_from(0).iter().all(|c| !c.starts_with("trx")));

    // The control: a client that answers them is logged doing so, and not as the key's owner.
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::ForeignKey {
            after: ms(300),
            hold: ms(300),
            chrono_to_all: true,
        }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    let (h, _) = c.frame(kind::TX_CHRONO).unwrap();
    let mut phase = 0;
    c.peer
        .send_binary(answer(h.length, 1_000.0, &mut phase))
        .unwrap();
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(l.event, Event::FrameIn { ours: false, .. }))));
}

#[test]
fn server_stop_stops_the_sdr_with_no_unkey_echo_and_refuses_keys_until_start() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::ServerStop { after: ms(100) }],
    );
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;trx:0,true;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    c.text_until(|t| t == "stop;").expect("the SDR stops");
    let after = c.drain(ms(300));
    assert!(
        texts(&after).iter().all(|t| !t.starts_with("trx")),
        "no unkey echo"
    );
    let snapshot = sim.radio();
    assert!(!snapshot.running && snapshot.trx[0].key.is_none());
    c.send("trx:0,true;");
    let got = c.drain(ms(300));
    assert!(texts(&got).is_empty() && headers(&got, kind::RX_AUDIO).is_empty());
    assert!(sim.events().iter().any(|e| matches!(
        e,
        Event::Refused {
            why: crate::event::Refusal::Stopped,
            ..
        }
    )));
    c.send("start;");
    c.text_until(|t| t == "start;").unwrap();
    c.frame(kind::RX_AUDIO).expect("frames again");
}

#[test]
fn transmit_not_allowed_and_receive_only_refuse_every_key_with_no_echo() {
    for (fault, line) in [
        (Fault::TxEnableFalse, "tx_enable:0,false;"),
        (Fault::ReceiveOnly, "receive_only:true;"),
    ] {
        let sim = start(Dialect::expert_sdr3(), vec![fault]);
        let mut c = Client::ready(&sim);
        assert!(c.texts().contains(&line.to_string()));
        c.send("trx:0,true;tune:0,true;cw_macros:0,E;");
        assert!(texts(&c.drain(ms(300)))
            .iter()
            .all(|t| !t.starts_with("trx") && !t.starts_with("tune")));
        assert!(sim.radio().trx[0].key.is_none());
    }
}

#[test]
fn band_restore_follows_a_band_change_with_the_bands_drive() {
    let fault = Fault::BandRestore {
        drive: 100,
        tune_drive: 100,
        modulation: None,
    };
    let sim = start(Dialect::expert_sdr3(), vec![fault]);
    let mut c = Client::ready(&sim);
    c.send("vfo:0,0,7074000;");
    c.text_until(|t| t == "vfo:0,0,7074000;").unwrap();
    assert_eq!(
        c.text_until(|t| t.starts_with("drive")).as_deref(),
        Some("drive:0,100;")
    );
    assert_eq!(
        c.text_until(|t| t.starts_with("tune_drive")).as_deref(),
        Some("tune_drive:0,100;")
    );
}

#[test]
fn the_hold_ignores_another_clients_write_inside_its_window() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::Hold200ms { window: ms(400) }],
    );
    let mut a = Client::ready(&sim);
    let mut b = Client::ready(&sim);
    b.send("vfo:0,0,7010000;");
    a.text_until(|t| t == "vfo:0,0,7010000;").unwrap();
    a.send("vfo:0,0,7020000;");
    assert!(texts(&a.drain(ms(150)))
        .iter()
        .all(|t| t != "vfo:0,0,7020000;"));
    assert!(sim
        .events()
        .iter()
        .any(|e| matches!(e, Event::Held { conn: 0, .. })));
    a.drain(ms(300));
    a.send("vfo:0,0,7020000;");
    a.text_until(|t| t == "vfo:0,0,7020000;")
        .expect("taken once the hold has passed");
}

#[test]
fn half_open_goes_silent_with_the_socket_open() {
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::HalfOpen { after: ms(500) }],
    );
    let mut c = Client::ready(&sim);
    c.peer.ping().unwrap();
    c.until(|m| *m == Incoming::Pong).expect("a pong before");
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
        l.event,
        Event::Fault {
            name: "HalfOpen",
            ..
        }
    ))));
    c.send("vfo:0,0,7074000;");
    c.peer.ping().unwrap();
    let got = c.drain(ms(500));
    assert!(got.is_empty(), "the server said {got:?}");
    assert!(
        sim.connections().contains(&0),
        "the connection is still open"
    );
}

#[test]
fn a_disconnect_mid_over_drops_the_client_and_the_dialect_or_the_fault_decides_the_key() {
    // ExpertSDR3 with the radio kept keyed: the next connection sees the transmitter still up.
    let sim = start(
        Dialect::expert_sdr3(),
        vec![Fault::DisconnectMidOver {
            after: ms(100),
            radio_stays_keyed: true,
        }],
    );
    let mut c = Client::ready(&sim);
    c.send("trx:0,true;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    assert_eq!(c.until(|m| *m == Incoming::Closed), Some(Incoming::Closed));
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| l.event
        == Event::Closed {
            conn: 0,
            by: Closer::Simulator
        })));
    assert!(Client::ready(&sim)
        .texts()
        .contains(&"trx:0,true;".to_string()));
    // AetherSDR unkeys a lost owner when the fault does not keep the key.
    let sim = start(
        Dialect::aether_sdr(),
        vec![Fault::DisconnectMidOver {
            after: ms(100),
            radio_stays_keyed: false,
        }],
    );
    let mut c = Client::ready(&sim);
    c.send("trx:0,true;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    c.until(|m| *m == Incoming::Closed);
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
        l.event,
        Event::Unkeyed {
            cause: crate::event::Cause::Left(0),
            ..
        }
    ))));
    let next = Client::ready(&sim);
    assert!(next.texts().concat().contains("trx:0,false;"));
}

#[test]
fn oversized_sends_one_huge_message_after_the_burst() {
    for binary in [false, true] {
        let sim = start(
            Dialect::expert_sdr3(),
            vec![Fault::Oversized {
                bytes: 200_000,
                binary,
            }],
        );
        let mut c = Client::ready(&sim);
        let got = c.until(|m| match m {
            Incoming::Text(t) => t.len() >= 200_000,
            Incoming::Binary(b) => b.len() >= 200_000,
            _ => false,
        });
        match got {
            Some(Incoming::Text(t)) if !binary => {
                assert_eq!(t.len(), 200_000);
                assert!(t.starts_with("x_oversized:") && t.ends_with(';'));
            }
            Some(Incoming::Binary(b)) if binary => assert_eq!(b.len(), frame::HEADER_LEN + 200_000),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn garbage_arrives_in_order_and_the_session_goes_on() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::Garbage]);
    let mut c = Client::ready(&sim);
    let want_text = fault::garbage_text();
    let want_frames = fault::garbage_frames();
    let mut text = Vec::new();
    let mut frames = Vec::new();
    while text.len() < want_text.len() || frames.len() < want_frames.len() {
        match c.until(|_| true) {
            Some(Incoming::Text(t)) => text.push(t),
            Some(Incoming::Binary(b)) => frames.push(b),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(text, want_text);
    assert_eq!(frames, want_frames);
    c.send("vfo:0,0,7074000;");
    c.text_until(|t| t == "vfo:0,0,7074000;")
        .expect("the session goes on");
}

#[test]
fn holds_cw_keeps_the_radio_keyed_after_the_text_and_through_the_stop() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::HoldsCw]);
    let mut c = Client::ready(&sim);
    c.send("cw_macros_delay:0;cw_macros_speed:40;cw_macros:0,EE;");
    c.text_until(|t| t == "trx:0,true;").unwrap();
    assert!(texts(&c.drain(ms(400))).iter().all(|t| t != "trx:0,false;"));
    c.send("cw_macros_stop;");
    assert!(texts(&c.drain(ms(200))).iter().all(|t| t != "trx:0,false;"));
    assert_eq!(sim.radio().trx[0].key.map(|k| k.source), Some(Source::Cw));
    c.send("trx:0,false;");
    c.text_until(|t| t == "trx:0,false;")
        .expect("an unkey releases it");
}

#[test]
fn an_unknown_version_replaces_the_protocol_line() {
    let fault = Fault::UnknownVersion {
        program: "FooSDR".into(),
        version: "9.9".into(),
    };
    let sim = start(Dialect::expert_sdr3(), vec![fault]);
    let c = Client::ready(&sim);
    assert!(c.texts().contains(&"protocol:FooSDR,9.9;".to_string()));
    assert!(c
        .texts()
        .iter()
        .all(|t| !t.starts_with("protocol:ExpertSDR3")));
}

#[test]
fn slow_holds_every_text_back_by_its_delay_and_not_the_frames() {
    let sim = start(Dialect::expert_sdr3(), vec![Fault::Slow { delay: ms(250) }]);
    let mut c = Client::ready(&sim);
    let log = sim.log();
    let connected = log
        .iter()
        .find(|l| l.event == Event::Connected { conn: 0 })
        .unwrap()
        .at;
    let first = log
        .iter()
        .find(|l| matches!(l.event, Event::TextOut { .. }))
        .unwrap()
        .at;
    assert!(first >= connected + ms(250), "{connected:?} then {first:?}");
    c.send("audio_start:0;vfo:0,0,7074000;");
    c.text_until(|t| t == "vfo:0,0,7074000;").unwrap();
    let log = sim.log();
    let command = log
        .iter()
        .find(|l| matches!(&l.event, Event::Command { name, .. } if name == "vfo"))
        .unwrap()
        .at;
    let echo = sent_at(&sim, 0, "vfo:0,0,7074000;").unwrap();
    assert!(echo >= command + ms(250));
    // A frame is not held back: the first receive audio frame beat the acknowledgement.
    let frame = sent_frames(&sim, 0, kind::RX_AUDIO)[0];
    let ack = sent_at(&sim, 0, "audio_start:0;").unwrap();
    assert!(frame < ack, "frame {frame:?}, ack {ack:?}");
}

#[test]
fn a_clean_close_and_a_lost_connection_are_told_apart() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    Client::ready(&sim).peer.close(WAIT);
    Client::ready(&sim).peer.abort();
    assert!(sim.wait_for(WAIT, |log| {
        let closed: Vec<Event> = log
            .iter()
            .filter(|l| matches!(l.event, Event::Closed { .. }))
            .map(|l| l.event.clone())
            .collect();
        closed
            == [
                Event::Closed {
                    conn: 0,
                    by: Closer::Client,
                },
                Event::Closed {
                    conn: 1,
                    by: Closer::Dropped,
                },
            ]
    }));
}

#[test]
fn a_dropped_connection_on_demand_is_closed_by_the_simulator() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    sim.drop_connection(0);
    assert_eq!(c.until(|m| *m == Incoming::Closed), Some(Incoming::Closed));
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| l.event
        == Event::Closed {
            conn: 0,
            by: Closer::Simulator
        })));
}

#[test]
fn dropping_the_simulator_closes_its_clients() {
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut c = Client::ready(&sim);
    c.send("audio_start:0;");
    let started = Instant::now();
    drop(sim);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the drop joined promptly"
    );
    assert_eq!(c.until(|m| *m == Incoming::Closed), Some(Incoming::Closed));
}

#[test]
fn the_operator_and_a_spot_click_reach_every_client() {
    let sim = start(Dialect::aether_sdr(), Vec::new());
    let mut c = Client::ready(&sim);
    sim.operator("vfo:0,0,21074000;");
    assert_eq!(
        c.text_until(|t| t.starts_with("vfo:")).as_deref(),
        Some("vfo:0,0,21074000;")
    );
    c.send("spot:K1ABC,cw,14025000,0,;");
    sim.click_spot("K1ABC", 14_025_000);
    assert_eq!(
        c.text_until(|t| t.contains("clicked_on_spot")).as_deref(),
        Some("clicked_on_spot:K1ABC,14025000;")
    );
    assert_eq!(
        c.text_until(|t| t.contains("clicked_on_spot")).as_deref(),
        Some("rx_clicked_on_spot:0,0,K1ABC,14025000;")
    );
    assert!(sim.radio().trx[0].key.is_none(), "a click keys nothing");
}

#[test]
fn many_simulators_side_by_side_never_cross() {
    // Sixteen simulators at once, each its own device name: every client reaches its own
    // simulator, and no two share a port.
    let handles: Vec<_> = (0..16)
        .map(|i| {
            thread::spawn(move || {
                let dialect = Dialect {
                    device: format!("SIM-{i}"),
                    ..Dialect::expert_sdr3()
                };
                let sim = Simulator::start(Config::dialect(dialect)).unwrap();
                let c = Client::ready(&sim);
                assert!(
                    c.texts().contains(&format!("device:SIM-{i};")),
                    "simulator {i}"
                );
                sim.addr().port()
            })
        })
        .collect();
    let mut ports: Vec<u16> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    ports.sort_unstable();
    ports.dedup();
    assert_eq!(ports.len(), 16);
}

#[test]
fn a_rejected_handshake_is_logged_and_the_simulator_carries_on() {
    use std::io::Write;
    let sim = start(Dialect::expert_sdr3(), Vec::new());
    let mut raw = TcpStream::connect(sim.addr()).unwrap();
    raw.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(l.event, Event::Rejected { .. }))));
    let c = Client::ready(&sim);
    assert!(c.texts().contains(&"ready;".to_string()));
}
