//! Find a TCI server: one short connection that reads what an SDR program's TCI server says about
//! itself, up to `ready;`, and closes. Settings ▸ Radio ▸ Rig & CAT's "Find TCI server" asks it, and
//! so does the setup wizard when it looks for an SDR program on this computer.
//!
//! **It writes nothing.** No TCI command and no frame: the only bytes it sends are the WebSocket
//! handshake that opens the connection, the close frame that ends it, and the pong the WebSocket
//! layer owes a ping. So it never keys, never changes the radio and never touches another
//! program's transmission. That is why it reads the server with the session's own layers (the
//! codec, the model and the dialect pin, as [`super::session::Session::on_text`] reads a burst)
//! and does not run a [`super::session::Session`]: a session writes once `ready;` comes (Nexus's
//! audio stream, the meter pushes), and every close of one sends `trx:<trx>,false;`, which on a
//! server with no notion of an owner (ExpertSDR3, by the spec) ends another program's over.
//!
//! **Bounded.** The TCP connect waits at most until the probe's deadline, and so does every read
//! and write after it ([`Deadlined`]): each is given only the time left, and none starts once it
//! has run out. A timeout on each read alone would bound one read, not the probe: the WebSocket
//! layer reads on until a handshake answer or a message is whole, so a server sending a byte just
//! inside each read's timeout could hold the probe for as long as it liked. So a server that says
//! nothing, or trickles, costs the budget and no more; the close frame then gets [`CLOSE_WRITE`].
//! What the server sends is read under the session's bounds ([`MAX_WS_MESSAGE`], then the codec's).

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::dialect::{Evidence, Preference};
use super::model::Model;
use super::session::{CONNECT_TIMEOUT, MAX_WS_MESSAGE};
use super::{server, wire, Dialect};

/// The longest the close frame may take once the server has been read.
pub const CLOSE_WRITE: Duration = Duration::from_millis(250);

/// What a TCI server said about itself before `ready;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// The protocol line's program and version, as sent (`protocol:ExpertSDR3,2.0;`). Not an
    /// identity: piHPSDR, AetherSDR and Thetis's emulation announce ExpertSDR3 too.
    pub program: Option<String>,
    pub version: Option<String>,
    /// The radio the program says it drives (`device:SunSDR2DX;`).
    pub device: Option<String>,
    /// How many transceivers it serves (`trx_count:2;`).
    pub trx_count: Option<u32>,
    /// The dialect a session would pin from this burst with the dialect left to Nexus.
    pub dialect: Dialect,
}

impl Hello {
    /// Nexus's own TCI server (`device:Nexus;`, [`server::DEVICE`]): this Nexus or another one,
    /// never an SDR program.
    pub fn is_nexus(&self) -> bool {
        self.device.as_deref() == Some(server::DEVICE)
    }
}

/// Why no TCI server was found at an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// Nothing took the connection before the deadline: refused, unreachable or silent.
    NoAnswer(String),
    /// Something took it but did not answer as a WebSocket server: not a TCI server.
    NotTci(String),
    /// A WebSocket server, but no `ready;` before the deadline or before it closed.
    NoReady,
}

/// Connect to `addr`, read the server's handshake to `ready;` and close, within `budget` (and
/// [`CLOSE_WRITE`] for the close frame).
pub fn probe(addr: SocketAddr, budget: Duration) -> Result<Hello, Miss> {
    let deadline = Instant::now() + budget;
    let wait = left(deadline)
        .map(|left| left.min(CONNECT_TIMEOUT))
        .ok_or_else(|| Miss::NoAnswer("no time to connect".into()))?;
    let tcp = TcpStream::connect_timeout(&addr, wait).map_err(|e| Miss::NoAnswer(e.to_string()))?;
    let limits = WebSocketConfig::default()
        .max_message_size(Some(MAX_WS_MESSAGE))
        .max_frame_size(Some(MAX_WS_MESSAGE));
    let url = format!("ws://{addr}/");
    // The handshake's request out and the server's answer in, and every read after them, before
    // the deadline.
    let stream = Deadlined { tcp, deadline };
    let (mut ws, _) = tungstenite::client::client_with_config(url.as_str(), stream, Some(limits))
        .map_err(|e| Miss::NotTci(e.to_string()))?;
    let hello = read_to_ready(&mut ws);
    // The close frame, and nothing before it, in an allowance of its own.
    ws.get_mut().deadline = Instant::now() + CLOSE_WRITE;
    let _ = ws.close(None);
    let _ = ws.flush();
    hello
}

/// The probe's socket as the WebSocket layer sees it: every read and write is given only the time
/// left before `deadline`, and none starts once it has run out. A socket timeout bounds one read,
/// and tungstenite reads on until a handshake answer or a message is whole (it gives up on a
/// handshake only after 512 reads or 64 KiB, sooner for tiny reads, and on a message at the
/// 1 MiB cap), so a timeout set once would let a server that trickles bytes, each inside it, hold
/// the probe far past its deadline. The TCI server reads its handshake against its deadline the
/// same way (`server::conn::Wire`).
struct Deadlined {
    tcp: TcpStream,
    deadline: Instant,
}

impl Deadlined {
    /// The time left before the deadline, or `TimedOut` once none is.
    fn time_left(&self) -> io::Result<Duration> {
        left(self.deadline)
            .ok_or_else(|| io::Error::new(ErrorKind::TimedOut, "the probe's time is up"))
    }
}

impl Read for Deadlined {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tcp.set_read_timeout(Some(self.time_left()?))?;
        self.tcp.read(buf)
    }
}

impl Write for Deadlined {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tcp.set_write_timeout(Some(self.time_left()?))?;
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// The time before `deadline`, if any is left: a socket timeout of zero is refused.
fn left(deadline: Instant) -> Option<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    (!left.is_zero()).then_some(left)
}

/// Read the server's messages until `ready;`, each on its own as the session reads them (a command
/// cut at the end of one message is refused, never joined to the next: piHPSDR's unterminated burst
/// lines), into the model and the dialect evidence, until the deadline.
fn read_to_ready(ws: &mut WebSocket<Deadlined>) -> Result<Hello, Miss> {
    let mut evidence = Evidence::default();
    let mut model = Model::new();
    loop {
        if left(ws.get_ref().deadline).is_none() {
            return Err(Miss::NoReady);
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                for notice in wire::notices(text.as_str()).flatten() {
                    evidence.notice(&notice);
                    model.apply(&notice);
                }
                if model.ready {
                    return Ok(Hello {
                        program: evidence.program.clone(),
                        version: evidence.version.clone(),
                        device: model.device.known().map(|d| d.as_str().to_string()),
                        trx_count: model.trx_count.known().copied(),
                        dialect: evidence.pin(Preference::Auto),
                    });
                }
            }
            Ok(Message::Close(_)) => return Err(Miss::NoReady),
            // A frame, a ping (the WebSocket layer answers it), a pong.
            Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            // A read timed out, or the time ran out: the loop's deadline decides.
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err(Miss::NoReady),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    use tempo_tcisim::{
        Closer, Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Fault, Simulator,
    };

    use super::*;
    use crate::tci::dialect::UNKNOWN;
    use crate::tci::session::{Config, Connection, NoStreams};
    use crate::tci::{DriveForm, Trx, EXPERT_SDR2, EXPERT_SDR3};

    /// Long enough for any expected answer on a loaded box.
    const WAIT: Duration = Duration::from_secs(5);

    fn sim(dialect: SimDialect, faults: impl IntoIterator<Item = Fault>) -> Simulator {
        Simulator::start(SimConfig::dialect(dialect).with(faults)).expect("the simulator starts")
    }

    /// The probe on its own thread, its answer awaited for at most `limit`: a probe that does not
    /// keep to its budget fails the test instead of hanging the suite.
    fn probe_within(
        addr: SocketAddr,
        budget: Duration,
        limit: Duration,
    ) -> (Result<Hello, Miss>, Duration) {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let started = Instant::now();
            let found = probe(addr, budget);
            let _ = tx.send((found, started.elapsed()));
        });
        rx.recv_timeout(limit).unwrap_or_else(|_| {
            panic!("the probe of {addr} was still running after {limit:?} (budget {budget:?})")
        })
    }

    /// Every simulated server, as it announces itself.
    fn servers() -> Vec<SimDialect> {
        vec![
            SimDialect::expert_sdr3(),
            SimDialect::thetis(),
            SimDialect::pihpsdr(),
            SimDialect::expert_sdr2(),
            SimDialect::aether_sdr(),
        ]
    }

    /// Wait until the simulator has seen connection `conn` end, and say how.
    fn closed(sim: &Simulator, conn: usize) -> Option<Closer> {
        let ended = |log: &[tempo_tcisim::Logged]| {
            log.iter()
                .any(|l| matches!(&l.event, SimEvent::Closed { conn: c, .. } if *c == conn))
        };
        sim.wait_for(WAIT, ended);
        sim.events().into_iter().find_map(|e| match e {
            SimEvent::Closed { conn: c, by } if c == conn => Some(by),
            _ => None,
        })
    }

    /// What client `conn` wrote, as the simulator logged it: text messages and frames.
    fn written_by(sim: &Simulator, conn: usize) -> Vec<String> {
        sim.events()
            .into_iter()
            .filter_map(|e| match e {
                SimEvent::TextIn { conn: c, text } if c == conn => Some(text),
                SimEvent::FrameIn { conn: c, bytes, .. } if c == conn => {
                    Some(format!("a frame of {bytes} bytes"))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn finds_each_simulated_server_and_says_what_it_announced() {
        for server in servers() {
            let sim = sim(server.clone(), []);
            let (found, _) = probe_within(sim.addr(), WAIT, WAIT * 2);
            let hello = found.unwrap_or_else(|e| panic!("{}: {e:?}", server.name));
            assert_eq!(
                hello.program.as_deref(),
                Some(server.program.as_str()),
                "{}",
                server.name
            );
            assert_eq!(
                hello.version.as_deref(),
                Some(server.version.as_str()),
                "{}",
                server.name
            );
            assert_eq!(
                hello.device.as_deref(),
                Some(server.device.as_str()),
                "{}",
                server.name
            );
            assert_eq!(hello.trx_count, Some(server.trx_count), "{}", server.name);
            assert!(!hello.is_nexus(), "{}", server.name);
        }
    }

    #[test]
    fn the_dialect_is_the_one_a_session_would_pin_from_the_same_burst() {
        // Everything that announces ExpertSDR3 is read as the spec's server, the TCI 1.x line as
        // ExpertSDR2 with its global drive, and a program this build does not know as unknown.
        for (server, want) in [
            (SimDialect::expert_sdr3(), EXPERT_SDR3),
            (SimDialect::thetis(), EXPERT_SDR3),
            (SimDialect::pihpsdr(), EXPERT_SDR3),
            (SimDialect::aether_sdr(), EXPERT_SDR3),
            (SimDialect::expert_sdr2(), EXPERT_SDR2),
        ] {
            let sim = sim(server.clone(), []);
            let hello = probe_within(sim.addr(), WAIT, WAIT * 2).0.expect("found");
            assert_eq!(hello.dialect.name, want.name, "{}", server.name);
            assert_eq!(hello.dialect.trx_source, want.trx_source, "{}", server.name);
        }
        assert_eq!(EXPERT_SDR2.drive, DriveForm::Global);
        let unknown = sim(
            SimDialect::expert_sdr3(),
            [Fault::UnknownVersion {
                program: "MysterySDR".into(),
                version: "9.9".into(),
            }],
        );
        let hello = probe_within(unknown.addr(), WAIT, WAIT * 2)
            .0
            .expect("found");
        assert_eq!(
            (hello.program.as_deref(), hello.version.as_deref()),
            (Some("MysterySDR"), Some("9.9"))
        );
        assert_eq!(hello.dialect.name, UNKNOWN.name);
    }

    #[test]
    fn the_probe_writes_nothing_and_closes_cleanly() {
        // The positive control first: a session on the same kind of server writes as soon as the
        // server says `ready;` (its audio stream and meter pushes), and the simulator's log shows
        // it, up to the session's close. That close comes with the server's IQ arriving too, and
        // the server still reads its last lines and its close frame: the close reads on to the
        // server's answer, so the socket does not go with the server's frames unread.
        let control = sim(SimDialect::expert_sdr3(), []);
        let trx = Trx::new(0).expect("trx 0");
        let session = Connection::connect(control.addr(), Config::new(trx), Box::new(NoStreams))
            .expect("the session connects");
        let wrote = control.wait_for(WAIT, |log| {
            log.iter()
                .any(|l| matches!(&l.event, SimEvent::TextIn { conn: 0, .. }))
        });
        session
            .send(wire::Command::Stream(wire::StreamSetting::IqStart(trx)))
            .expect("iq_start sent");
        let streaming = control.wait_for(WAIT, |log| {
            log.iter()
                .filter(|l| {
                    matches!(&l.event, SimEvent::FrameOut { conn: 0, header: Some(h), .. }
                        if h.kind == tempo_tcisim::frame::kind::IQ)
                })
                .count()
                >= 3
        });
        session.close();
        let control_closed = closed(&control, 0);
        let control_wrote = written_by(&control, 0);
        assert!(streaming, "control: the server's IQ never came");
        assert!(
            wrote
                && ["trx:0,false;", "iq_stop:0;"]
                    .iter()
                    .all(|t| control_wrote.iter().any(|w| w == t)),
            "control: the log shows what a client writes, its close's lines too: {control_wrote:?}"
        );
        assert_eq!(
            control_closed,
            Some(Closer::Client),
            "control: the session's close frame was never read"
        );

        for server in servers() {
            let sim = sim(server.clone(), []);
            probe_within(sim.addr(), WAIT, WAIT * 2).0.expect("found");
            assert_eq!(
                closed(&sim, 0),
                Some(Closer::Client),
                "{}: the probe closed with a close frame",
                server.name
            );
            assert!(
                !sim.events()
                    .iter()
                    .any(|e| matches!(e, SimEvent::Keyed { .. } | SimEvent::MicFallback { .. })),
                "{}: the radio keyed",
                server.name
            );
            assert_eq!(
                written_by(&sim, 0),
                Vec::<String>::new(),
                "{}: the probe wrote to the server",
                server.name
            );
        }
    }

    #[test]
    fn a_burst_cut_across_messages_still_reaches_ready() {
        for faults in [
            vec![Fault::SplitConcat { cut: None }],
            vec![Fault::Slow {
                delay: Duration::from_millis(150),
            }],
        ] {
            let sim = sim(SimDialect::expert_sdr3(), faults.clone());
            let hello = probe_within(sim.addr(), WAIT, WAIT * 2)
                .0
                .unwrap_or_else(|e| panic!("{faults:?}: {e:?}"));
            assert_eq!(hello.device.as_deref(), Some("SunSDR2DX"), "{faults:?}");
        }
    }

    #[test]
    fn nexus_own_server_is_told_apart_from_an_sdr_program() {
        let nexus = sim(
            SimDialect {
                device: server::DEVICE.into(),
                ..SimDialect::expert_sdr3()
            },
            [],
        );
        let hello = probe_within(nexus.addr(), WAIT, WAIT * 2).0.expect("found");
        assert!(hello.is_nexus(), "{hello:?}");
        assert_eq!(hello.device.as_deref(), Some("Nexus"));
    }

    #[test]
    fn nothing_listening_is_no_answer_at_once() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
            listener.local_addr().expect("its address").port()
        };
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let (found, took) = probe_within(addr, WAIT, WAIT * 2);
        assert!(matches!(found, Err(Miss::NoAnswer(_))), "{found:?}");
        assert!(
            took < Duration::from_secs(2),
            "a refused connection took {took:?}"
        );
    }

    /// A listener on loopback that answers each connection with `reply` (or holds it silent), for
    /// as long as the test runs.
    fn listener(reply: Option<&'static [u8]>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let addr = listener.local_addr().expect("its address");
        thread::spawn(move || {
            let mut held = Vec::new();
            for mut stream in listener.incoming().flatten() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                if let Some(reply) = reply {
                    let _ = stream.write_all(reply);
                }
                held.push(stream);
            }
        });
        addr
    }

    #[test]
    fn something_that_is_not_a_websocket_server_is_not_tci() {
        for reply in [
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
            &b"RPRT -1\n"[..],
        ] {
            let (found, _) = probe_within(listener(Some(reply)), WAIT, WAIT * 2);
            assert!(
                matches!(found, Err(Miss::NotTci(_))),
                "{:?}: {found:?}",
                String::from_utf8_lossy(reply)
            );
        }
    }

    #[test]
    fn a_listener_that_never_answers_costs_the_budget_and_no_more() {
        let budget = Duration::from_millis(400);
        let (found, took) = probe_within(listener(None), budget, WAIT);
        assert!(matches!(found, Err(Miss::NotTci(_))), "{found:?}");
        assert!(took >= budget, "it gave up early: {took:?}");
        assert!(
            took < budget + Duration::from_millis(1_500),
            "it overran its budget: {took:?}"
        );
    }

    #[test]
    fn a_websocket_server_that_never_says_ready_costs_the_budget_and_no_more() {
        let budget = Duration::from_millis(600);
        let sim = sim(SimDialect::expert_sdr3(), [Fault::NoReady]);
        let (found, took) = probe_within(sim.addr(), budget, WAIT);
        assert_eq!(found, Err(Miss::NoReady));
        assert!(took >= budget, "it gave up early: {took:?}");
        assert!(
            took < budget + Duration::from_millis(1_500),
            "it overran its budget: {took:?}"
        );
        // It still closed, and wrote nothing while it waited.
        assert_eq!(closed(&sim, 0), Some(Closer::Client));
        assert_eq!(written_by(&sim, 0), Vec::<String>::new());
    }

    #[test]
    fn a_slow_server_past_the_budget_is_no_ready_within_the_budget() {
        let budget = Duration::from_millis(500);
        let sim = sim(
            SimDialect::expert_sdr3(),
            [Fault::Slow {
                delay: Duration::from_secs(3),
            }],
        );
        let (found, took) = probe_within(sim.addr(), budget, WAIT);
        assert_eq!(found, Err(Miss::NoReady));
        assert!(
            took < budget + Duration::from_millis(1_500),
            "it overran its budget: {took:?}"
        );
    }

    /// The budget the trickle tests give the probe, the gap between the bytes their servers
    /// trickle (well inside any one read's timeout), and how far past its deadline the probe may
    /// return: the close frame's own allowance and a margin for a loaded box.
    const TRICKLE_BUDGET: Duration = Duration::from_millis(600);
    const TRICKLE_GAP: Duration = Duration::from_millis(150);
    const PAST_THE_DEADLINE: Duration = Duration::from_millis(500);

    /// Send `head` at once, then one byte every [`TRICKLE_GAP`] for as long as the client stays.
    fn trickle(out: &mut impl Write, head: &[u8]) {
        if out.write_all(head).is_err() {
            return;
        }
        loop {
            thread::sleep(TRICKLE_GAP);
            if out.write_all(b"a").and_then(|()| out.flush()).is_err() {
                return;
            }
        }
    }

    /// A server on loopback that trickles each client: its HTTP answer (`websocket` false: a
    /// status line, then a header that never ends), or, after a real WebSocket handshake, one text
    /// message whose 200 bytes come one at a time.
    fn trickler(websocket: bool) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let addr = listener.local_addr().expect("its address");
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                thread::spawn(move || {
                    if websocket {
                        let Ok(mut ws) = tungstenite::accept(stream) else {
                            return;
                        };
                        // FIN + text, unmasked (a server's), a 16-bit length of 200.
                        trickle(ws.get_mut(), &[0x81, 126, 0x00, 200]);
                    } else {
                        let mut request = [0u8; 1024];
                        let _ = stream.read(&mut request);
                        trickle(
                            &mut stream,
                            b"HTTP/1.1 101 Switching Protocols\r\nX-Trickle: ",
                        );
                    }
                });
            }
        });
        addr
    }

    #[test]
    fn a_server_that_trickles_its_handshake_cannot_hold_the_probe_past_its_deadline() {
        // Each byte comes well inside one read's timeout, so only a deadline over the whole
        // handshake, not a timeout on each read, ends it in time.
        let (found, took) = probe_within(trickler(false), TRICKLE_BUDGET, WAIT);
        assert!(matches!(found, Err(Miss::NotTci(_))), "{found:?}");
        assert!(took >= TRICKLE_BUDGET, "it gave up early: {took:?}");
        assert!(
            took < TRICKLE_BUDGET + PAST_THE_DEADLINE,
            "it overran its deadline: {took:?}"
        );
    }

    #[test]
    fn a_server_that_trickles_a_message_cannot_hold_the_probe_past_its_deadline() {
        // The handshake completes; then one message arrives a byte at a time, each well inside
        // one read's timeout, and the WebSocket layer reads on until the message is whole.
        let (found, took) = probe_within(trickler(true), TRICKLE_BUDGET, WAIT);
        assert_eq!(found, Err(Miss::NoReady));
        assert!(took >= TRICKLE_BUDGET, "it gave up early: {took:?}");
        assert!(
            took < TRICKLE_BUDGET + PAST_THE_DEADLINE,
            "it overran its deadline: {took:?}"
        );
    }
}
