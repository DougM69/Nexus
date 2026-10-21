//! The driver over sockets no simulated program makes, each scripted here on loopback for one
//! connection: a server that trickles its bytes, one that never answers the close or holds the
//! connection open after its own, and a close's end of the TCP connection as the server reads it.
//! Every wait is bounded, and a driver that does not keep to its bound fails its test rather than
//! hanging the suite.

use std::net::TcpListener;
use std::thread;

use super::*;
use crate::tci::frame::{encode_block, BlockHeader, Channels, LengthMeaning, SampleFormat};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(5);

/// The gap between the bytes a trickling server sends once its WebSocket is open: inside one of
/// the driver's reads ([`READ_POLL`]), so a timeout on each read alone never ends that read.
const TRICKLE_GAP: Duration = Duration::from_millis(5);

/// The longest a trickling server keeps it up: well past every wait here.
const TRICKLE_FOR: Duration = Duration::from_secs(10);

/// How late a stop or a window may come here: one of the driver's reads ([`READ_POLL`]) and a
/// margin for a loaded box. A read the trickle held would last [`TRICKLE_FOR`].
const PAST_DUE: Duration = Duration::from_millis(250);

/// How far past the handshake's deadline the connect, or past the drain the close, may return: a
/// margin for a loaded box.
const PAST_THE_DEADLINE: Duration = Duration::from_millis(500);

/// How many closes the reset test makes: what it guards against is a race.
const CLOSES: usize = 4;

/// An ExpertSDR3 server's burst, ready to transmit on receiver 0.
const READY: &str = "protocol:ExpertSDR3,2.0;device:SunSDR2DX;receive_only:false;trx_count:1;\
                     tx_enable:0,true;trx:0,false;tune:0,false;start;ready;";

fn trx0() -> Trx {
    Trx::new(0).expect("trx 0")
}

fn connect(addr: SocketAddr, config: Config) -> Connection {
    Connection::connect(addr, config, Box::new(NoStreams)).expect("connected")
}

/// `f` on a thread of its own, awaited for at most `limit`: what it returned, and how long it took.
fn within<T: Send + 'static>(
    what: &str,
    limit: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> (T, Duration) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let started = Instant::now();
        let value = f();
        let _ = tx.send((value, started.elapsed()));
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("{what} was still waiting after {limit:?}"))
}

/// Wait for an event `pick` accepts; every event seen up to it, that one last.
fn expect(conn: &Connection, what: &str, pick: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no {what} within {WAIT:?}; saw {seen:?}");
        if let Some(event) = conn.next_event(left) {
            let hit = pick(&event);
            seen.push(event);
            if hit {
                return seen;
            }
        }
    }
}

/// The client's text messages, as a scripted server read them.
type Heard = Arc<Mutex<Vec<String>>>;

/// Wait until the server has read `text` from the client `times` times.
fn reached(heard: &Heard, text: &str, times: usize) -> bool {
    let until = Instant::now() + WAIT;
    loop {
        if lock(heard).iter().filter(|t| *t == text).count() >= times {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// The server's side of a scripted connection, its WebSocket open.
struct Peer {
    ws: WebSocket<TcpStream>,
    heard: Heard,
}

impl Peer {
    fn send(&mut self, text: &str) -> bool {
        self.ws.send(Message::text(text)).is_ok()
    }

    /// Read what the client sends, for up to `wait`. `false` once the connection is over: the
    /// client's close frame (answered), or an error.
    fn listen(&mut self, wait: Duration) -> bool {
        let _ = self.ws.get_ref().set_read_timeout(Some(wait));
        match self.ws.read() {
            Ok(Message::Text(text)) => {
                lock(&self.heard).push(text.as_str().to_string());
                true
            }
            Ok(Message::Close(_)) => {
                let _ = self.ws.flush();
                false
            }
            Ok(_) => true,
            Err(tungstenite::Error::Io(e)) => retryable(&e),
            Err(_) => false,
        }
    }

    /// Listen until the client has sent `text`; whether it did within [`WAIT`].
    fn wait_for(&mut self, text: &str) -> bool {
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            if lock(&self.heard).iter().any(|t| t == text) {
                return true;
            }
            if !self.listen(Duration::from_millis(10)) {
                return false;
            }
        }
        false
    }

    /// The rest of the connection: one binary frame, its header at once and its payload a byte
    /// every [`TRICKLE_GAP`], never whole, the client listened to between the bytes, until it
    /// goes or [`TRICKLE_FOR`] passes.
    fn trickle(mut self) {
        // FIN + binary, unmasked as a server's frames are, a 16-bit length of 65000.
        let head = [0x82, 126, 0xFD, 0xE8];
        if self.ws.flush().is_err() || self.ws.get_mut().write_all(&head).is_err() {
            return;
        }
        let until = Instant::now() + TRICKLE_FOR;
        while Instant::now() < until && self.listen(TRICKLE_GAP) {
            if self.ws.get_mut().write_all(b"a").is_err() {
                return;
            }
        }
    }
}

/// A server on loopback for one connection: the WebSocket handshake, then `script` on a thread of
/// its own. Its address, and the client's texts as the script reads them.
fn scripted(script: impl FnOnce(Peer) + Send + 'static) -> (SocketAddr, Heard) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let addr = listener.local_addr().expect("its address");
    let heard = Heard::default();
    let peer_heard = Arc::clone(&heard);
    thread::spawn(move || {
        let Ok((tcp, _)) = listener.accept() else {
            return;
        };
        if let Ok(ws) = tungstenite::accept(tcp) {
            script(Peer {
                ws,
                heard: peer_heard,
            });
        }
    });
    (addr, heard)
}

/// A server that answers the WebSocket handshake with its status line, then a header a byte every
/// `gap`, never ending it.
fn trickles_its_handshake(gap: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let addr = listener.local_addr().expect("its address");
    thread::spawn(move || {
        let Ok((mut tcp, _)) = listener.accept() else {
            return;
        };
        let mut request = [0u8; 1024];
        let _ = tcp.read(&mut request);
        if tcp
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nX-Trickle: ")
            .is_err()
        {
            return;
        }
        let until = Instant::now() + TRICKLE_FOR;
        while Instant::now() < until {
            thread::sleep(gap);
            if tcp.write_all(b"a").is_err() {
                return;
            }
        }
    });
    addr
}

/// One receive audio block for receiver 0.
fn rx_block() -> Vec<u8> {
    let header = BlockHeader {
        trx: trx0(),
        kind: BlockKind::RxAudio,
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
    };
    encode_block(&header, LengthMeaning::Values, &[0.0; 512]).expect("a block")
}

/// ⭐ A SERVER THAT TRICKLES ITS HANDSHAKE CANNOT HOLD THE CONNECT PAST ITS DEADLINE: its answer a
/// byte every 150 ms, each far inside a read's timeout, while the WebSocket layer reads on until
/// the answer is whole. Only a deadline over the whole handshake ends it in
/// [`HANDSHAKE_TIMEOUT`]; the daemon that starts a session waits in this connect.
#[test]
fn a_server_that_trickles_its_handshake_cannot_hold_the_connect_past_its_deadline() {
    let addr = trickles_its_handshake(Duration::from_millis(150));
    let (connected, took) = within("the connect", HANDSHAKE_TIMEOUT + WAIT, move || {
        Connection::connect(addr, Config::new(trx0()), Box::new(NoStreams)).map(drop)
    });
    assert!(
        matches!(connected, Err(ConnectError::Handshake(_))),
        "{connected:?}"
    );
    assert!(took >= HANDSHAKE_TIMEOUT, "it gave up early: {took:?}");
    assert!(
        took < HANDSHAKE_TIMEOUT + PAST_THE_DEADLINE,
        "it overran its deadline: {took:?}"
    );
}

/// ⭐ A SERVER THAT TRICKLES A FRAME CANNOT HOLD A STOP: once ready, the server sends one frame a
/// byte every [`TRICKLE_GAP`], never whole. The WebSocket layer reads on until a frame is whole,
/// so with a timeout on each read alone the driver stays in that one read, and a caller's stop
/// waits in the queue behind it. Each read ends at the driver's next look at requests and time,
/// and the frame is kept for the next.
#[test]
fn a_server_that_trickles_a_frame_cannot_hold_a_stop() {
    let (addr, heard) = scripted(|mut peer| {
        if peer.send(READY) {
            peer.trickle();
        }
    });
    let conn = Arc::new(connect(addr, Config::new(trx0())));
    assert!(
        conn.wait_until(WAIT, |s| s.phase == Phase::Ready),
        "never ready"
    );
    // Time for the driver to be inside the trickled frame: a stop asked now comes behind its read.
    thread::sleep(Duration::from_millis(100));
    let caller = Arc::clone(&conn);
    let (stopped, took) = within("the stop", WAIT, move || caller.end_ours());
    assert_eq!(stopped, StopOutcome::Sent(vec![Stop::Trx(trx0())]));
    assert!(took < PAST_DUE, "the stop waited {took:?} behind a read");
    assert!(
        reached(&heard, "trx:0,false;", 1),
        "the stop never reached the server"
    );
}

/// ⭐ A SERVER THAT TRICKLES CANNOT HOLD THE CHRONO WINDOW: a key that takes Nexus's audio, and the
/// server, rather than pull that audio, trickles a frame. The window ends the over at its time,
/// since the SDR may be transmitting its microphone, however the bytes come.
#[test]
fn a_server_that_trickles_cannot_hold_the_chrono_window() {
    let (addr, heard) = scripted(|mut peer| {
        if !peer.send(READY) || !peer.wait_for("audio_start:0;") || !peer.send("audio_start:0;") {
            return;
        }
        // Receive audio until the key; then neither an echo nor a TX_CHRONO, a trickle instead.
        while !lock(&peer.heard).iter().any(|t| t == "trx:0,true,tci;") {
            let block = peer.ws.send(Message::binary(rx_block()));
            if block.is_err() || !peer.listen(Duration::from_millis(20)) {
                return;
            }
        }
        peer.trickle();
    });
    let conn = connect(addr, Config::new(trx0()));
    assert!(
        conn.wait_until(WAIT, |s| s.phase == Phase::Ready
            && s.rx_audio_fresh
            && s.model.trx(trx0()).audio.is_true()),
        "never ready for a key that takes Nexus's audio"
    );
    let keyed = Instant::now();
    conn.key(TxStart::Audio).expect("admitted");
    expect(&conn, "halt by the chrono window", |e| {
        *e == Event::Halted(Halt::NoChrono)
    });
    let took = keyed.elapsed();
    assert!(
        took < Duration::from_millis(CHRONO_WINDOW_MS) + PAST_DUE,
        "the over was ended {took:?} after its key"
    );
    assert!(
        reached(&heard, "trx:0,false;", 1),
        "the stop never reached the server"
    );
}

/// ⭐ A SERVER THAT TRICKLES CANNOT HOLD THE UNKEY DEADLINE: a stuck transmitter that is heard from
/// after our stop, saying it still transmits, and then trickles a frame. The escalation (the stops
/// again, the alarm, the close) comes at its deadline, however the bytes come.
#[test]
fn a_server_that_trickles_cannot_hold_the_unkey_deadline() {
    let config = Config {
        unkey_deadline_ms: 600,
        ..Config::new(trx0())
    };
    let (addr, heard) = scripted(|mut peer| {
        if !peer.send(READY)
            || !peer.wait_for("trx:0,true;")
            || !peer.send("trx:0,true;")
            || !peer.wait_for("trx:0,false;")
        {
            return;
        }
        // Still on the air after our stop, and saying so, for a while; then a trickled frame.
        for _ in 0..5 {
            if !peer.send("trx:0,true;") || !peer.listen(Duration::from_millis(50)) {
                return;
            }
        }
        peer.trickle();
    });
    let conn = connect(addr, config);
    assert!(
        conn.wait_until(WAIT, |s| s.phase == Phase::Ready),
        "never ready"
    );
    conn.key(TxStart::Microphone).expect("admitted");
    expect(&conn, "the echo", |e| {
        *e == Event::KeyConfirmed(TxStart::Microphone)
    });
    let stopped = Instant::now();
    assert!(matches!(conn.end_ours(), StopOutcome::Sent(_)));
    expect(&conn, "escalation", |e| *e == Event::UnkeyUnconfirmed);
    let took = stopped.elapsed();
    assert!(
        took < Duration::from_millis(config.unkey_deadline_ms) + PAST_DUE,
        "the escalation came {took:?} after our stop"
    );
    assert!(
        reached(&heard, "trx:0,false;", 2),
        "the stop sent again never reached the server"
    );
}

/// ⭐ A SERVER THAT NEVER ANSWERS THE CLOSE COSTS THE DRAIN AND NO MORE: it streams and never
/// reads, so our close frame is never answered. The close reads what still comes for
/// [`CLOSE_DRAIN`], waiting for the answer, then lets the socket go.
#[test]
fn a_server_that_never_answers_the_close_costs_the_drain_and_no_more() {
    let (addr, _) = scripted(|mut peer| {
        let until = Instant::now() + TRICKLE_FOR;
        while Instant::now() < until && peer.send("rx_smeter:0,0,-100;") {
            thread::sleep(Duration::from_millis(1));
        }
    });
    let conn = connect(addr, Config::new(trx0()));
    // The stream under way.
    thread::sleep(Duration::from_millis(50));
    let (_, took) = within("the close", WAIT, move || conn.close());
    assert!(
        took >= CLOSE_DRAIN,
        "it let the socket go without waiting for the server's answer: {took:?}"
    );
    assert!(
        took < CLOSE_DRAIN + PAST_THE_DEADLINE,
        "the drain overran: {took:?}"
    );
}

/// A stop asked of a session that has closed is answered at once, not after the close's drain: the
/// server ends the session with its close frame, then holds the connection open and says nothing,
/// so the drain waits out its time.
#[test]
fn a_stop_after_the_session_has_closed_is_answered_at_once() {
    let (addr, _) = scripted(|mut peer| {
        let _ = peer.ws.close(None);
        let _ = peer.ws.flush();
        thread::sleep(TRICKLE_FOR);
    });
    let conn = connect(addr, Config::new(trx0()));
    expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    let asked = Instant::now();
    assert_eq!(conn.end_ours(), StopOutcome::NotConnected);
    let took = asked.elapsed();
    assert!(
        took < CLOSE_DRAIN / 4,
        "a stop asked of a closed session waited {took:?}"
    );
}

/// Stream to the client until its close frame, echoing its stop as the SDR programs do (after a
/// moment's work); answer the close; then read the socket to its end. `Ok` for a FIN; for a reset,
/// or anything else, what happened.
fn stream_then_read_the_end(peer: &mut Peer) -> Result<(), String> {
    let until = Instant::now() + WAIT;
    loop {
        if Instant::now() >= until {
            return Err("no close frame came".into());
        }
        let _ = peer.send("rx_smeter:0,0,-100;");
        let _ = peer
            .ws
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(1)));
        match peer.ws.read() {
            Ok(Message::Close(_)) => break,
            Ok(Message::Text(text)) if text.as_str() == "trx:0,false;" => {
                thread::sleep(Duration::from_millis(20));
                let _ = peer.send("trx:0,false;");
                thread::sleep(Duration::from_millis(10));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if retryable(&e) => {}
            Err(e) => return Err(format!("before the close frame: {e}")),
        }
    }
    // A server's answer is written, and then the WebSocket layer says the connection is done.
    match peer.ws.flush() {
        Ok(()) | Err(tungstenite::Error::ConnectionClosed) => {}
        Err(e) => return Err(format!("the answer to the close: {e}")),
    }
    let tcp = peer.ws.get_mut();
    tcp.set_read_timeout(Some(WAIT))
        .map_err(|e| e.to_string())?;
    let mut buf = [0u8; 1024];
    loop {
        match tcp.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(format!("the end: {e}")),
        }
    }
}

/// ⭐ A CLOSE WHILE THE SERVER STREAMS ENDS WITH A FIN, NOT A RESET: the server streams, echoes our
/// stop, answers our close frame, and then reads its socket to the end. A socket let go with the
/// server's last messages unread ends in a reset instead, and a server that writes before it
/// reads, or whose system drops what it has not read when a reset comes, then never reads the
/// lines sent just before the close.
#[test]
fn a_close_while_the_server_streams_ends_with_a_fin_not_a_reset() {
    for close in 0..CLOSES {
        let (end_tx, end) = mpsc::channel();
        let (addr, _) = scripted(move |mut peer| {
            let _ = end_tx.send(stream_then_read_the_end(&mut peer));
        });
        let conn = connect(addr, Config::new(trx0()));
        // The stream under way.
        thread::sleep(Duration::from_millis(50));
        conn.close();
        let end = end.recv_timeout(WAIT).expect("the server saw the end");
        assert_eq!(
            end,
            Ok(()),
            "close {close}: the connection did not end with a FIN"
        );
    }
}
