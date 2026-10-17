//! Nexus's own link to YO3DMU's **PstRotatorAz**, over the UDP control the program documents,
//! served to the rest of Nexus as a rotctld on loopback.
//!
//! The source is PstRotatorAz's own User's Manual, Rev. 2.9, "UDP Interface" (pp. 11–12):
//!
//! - It listens on its **UDP Control Port**, 12000 by default ("Communication ▸ UDP Control
//!   Port…"), once **Setup ▸ UDP Control** is ticked. The sender may be on the same PC or another
//!   one on the local network.
//! - "The port used for antenna position report is UDP Port + 1": 12001 by default.
//! - Every message starts with `<PST>` and ends with `</PST>`. The commands used here, verbatim:
//!   `<PST><AZIMUTH>85</AZIMUTH></PST>` (set azimuth to 85°), `<PST><STOP>1</STOP></PST>` (stop
//!   the rotor, switch to manual mode), `<PST><PARK>1</PARK></PST>` (stop, manual mode, park the
//!   antenna) and `<PST>AZ?</PST>`, answered on port + 1 as `AZ:xxx<CR>`.
//!
//! **Why not Hamlib's PstRotator backend (model 3), which the picker used to run.** Its
//! `set_position` writes the bearing with `%f.2`, a typo for `%.2f`, so 90° goes out as
//! `90.000000.2`; it has no stop at all, so rotctld refuses `S`; its `get_position` sends `AZ?`
//! and returns a cached value at once, which is the TARGET after a move and 0 before any reply,
//! so a PstRotatorAz with UDP Control off showed a compass that looked alive; and it binds the
//! reply port to 12001 whatever control port is configured. Every one of those is a wire fact
//! from the bundled 4.7.1's `rot_pstrotator.c`, not from the manual, which is the authority here.
//!
//! **The shape.** The rest of Nexus — the Rotor pane, point-at-call, the satellite track, Nexus
//! Remote — already speaks rotctld to `127.0.0.1:<the profile's rotctld port>`. This link answers
//! there instead of a rotctld process, so none of them changes. It answers `p`, `P`, `S`, `K` and
//! `\dump_state`, the same short commands and `RPRT` codes a rotctld uses.
//!
//! **Nothing is reported as done unless PstRotatorAz answered.** UDP carries no acknowledgement,
//! and none of the commands above has a reply, so every command is followed by `AZ?`. `RPRT 0`
//! means PstRotatorAz answered it; silence is `RPRT -5` (Hamlib's timeout), and a position that
//! was not reported is never invented: the Rotor pane shows "—". NEEDS-BENCH: no PstRotatorAz has
//! run against this yet.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The rotator model the picker saves for PstRotatorAz: Hamlib's number for it, kept so a
/// station already set up as model 3 needs no change.
pub const MODEL: u32 = 3;

/// Where PstRotatorAz listens when it runs on this PC with its default UDP Control Port.
pub const DEFAULT_TARGET: &str = "127.0.0.1:12000";

/// How long a command waits for PstRotatorAz's `AZ:` reply. Shorter than the rotctld client's
/// 1.5 s poll deadline (`rotator::read_position`), so a silent PstRotatorAz is an `RPRT -5` the
/// client reads, not a client timeout.
const ANSWER_WAIT: Duration = Duration::from_millis(1_000);

/// The pause between the two STOP datagrams ([`STOP_DATAGRAM`]).
const STOP_GAP: Duration = Duration::from_millis(50);

/// How often the listener and each connection look at the halt flag.
const TICK: Duration = Duration::from_millis(50);

/// Stop the rotor and switch PstRotatorAz to manual mode (manual p. 12).
///
/// ⚠️ **It is sent twice.** The manual documents one; it is sent twice because UDP has no
/// acknowledgement, so one lost datagram is a STOP that never happened, and an operator report
/// says PstRotatorAz sometimes needs a second. Stopping a stopped rotor changes nothing, so the
/// repeat costs nothing.
pub const STOP_DATAGRAM: &str = "<PST><STOP>1</STOP></PST>";

/// Stop, switch to manual mode and park the antenna where PstRotatorAz's own park setting says
/// (manual p. 12). rotctld's `K`.
pub const PARK_DATAGRAM: &str = "<PST><PARK>1</PARK></PST>";

/// Ask for the antenna position, answered on the control port + 1 as `AZ:xxx<CR>` (manual p. 12).
pub const AZ_QUERY: &str = "<PST>AZ?</PST>";

/// What `\dump_state` answers, in rotctld's protocol-1 shape: PstRotatorAz turns azimuth only,
/// so the Rotor pane shows no elevation axis (Hamlib's backend declares 0–90° of one).
const DUMP_STATE: &str = "1\n3\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                          max_el=0.000000\nsouth_zero=0\nrot_type=Az\ndone\n";

/// The set-azimuth datagram for `az_deg`: a whole number of degrees in [0, 360), as in the
/// manual's own example (`<AZIMUTH>85</AZIMUTH>`). Rounded BEFORE it is wrapped, so 359.6° is
/// 0°, never 360.
pub fn azimuth_datagram(az_deg: f64) -> String {
    let whole = (az_deg.round() as i64).rem_euclid(360);
    format!("<PST><AZIMUTH>{whole}</AZIMUTH></PST>")
}

/// The azimuth in a position report, `AZ:xxx<CR>`. `None` for anything else, including the
/// `TGA:` target report, which is where the antenna is going, not where it is.
pub fn parse_azimuth(datagram: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(datagram).ok()?;
    text.split(['\r', '\n'])
        .find_map(|l| l.trim().strip_prefix("AZ:")?.trim().parse::<f64>().ok())
        .filter(|az| az.is_finite())
}

/// PstRotatorAz's address from the rotator port box. Empty means this PC, default port.
fn resolve(target: &str) -> std::io::Result<SocketAddr> {
    let t = match target.trim() {
        "" => DEFAULT_TARGET,
        t => t,
    };
    let bad = |why: String| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "the PstRotatorAz address {t:?} is not host:port ({why}): on the same PC it is \
                 {DEFAULT_TARGET}"
            ),
        )
    };
    t.to_socket_addrs()
        .map_err(|e| bad(e.to_string()))?
        .next()
        .ok_or_else(|| bad("it resolved to nothing".into()))
}

/// The UDP socket PstRotatorAz's replies arrive on, at its control port + 1. Commands are sent
/// from it as well, so a PstRotatorAz that answers the sender's port and one that answers its own
/// port + 1 land in the same place.
///
/// ⚠️ **Loopback when PstRotatorAz is on this PC** (the default, 127.0.0.1): nothing on the
/// network can reach the socket at all. Only a PstRotatorAz on another PC needs every interface,
/// and then [`Peer::azimuth`] reads nothing whose source is not that PC.
fn bind_replies(to: SocketAddr) -> std::io::Result<UdpSocket> {
    let port = to.port();
    let reply = port.checked_add(1).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "PstRotatorAz replies on its UDP Control Port + 1, and {port} has no port above it"
            ),
        )
    })?;
    let any: std::net::IpAddr = match to.ip() {
        ip if ip.is_loopback() => ip,
        ip if ip.is_ipv6() => Ipv6Addr::UNSPECIFIED.into(),
        _ => Ipv4Addr::UNSPECIFIED.into(),
    };
    UdpSocket::bind(SocketAddr::new(any, reply)).map_err(|e| {
        let other = port.saturating_add(2);
        std::io::Error::new(
            e.kind(),
            format!(
                "UDP port {reply} is already in use on this computer ({e}). PstRotatorAz sends \
                 its position replies there (its UDP Control Port + 1), so Nexus must listen on \
                 it. Close the program that holds {reply}, or change PstRotatorAz's \
                 Communication ▸ UDP Control Port… to another number, say {other}, and enter \
                 {host}:{other} as the rotator port here: its replies then come on {}.",
                other.saturating_add(1),
                host = to.ip(),
            ),
        )
    })
}

/// The PstRotatorAz end: where commands go and where its replies come back.
struct Peer {
    udp: UdpSocket,
    to: SocketAddr,
    /// One `AZ?` at a time, so a reply is read by the question that asked for it. Sends do not
    /// take it: a STOP never waits behind a poll.
    asking: Mutex<()>,
    /// Whether the last `AZ?` went unanswered, so a silent PstRotatorAz is logged once per
    /// episode rather than twice a second.
    silent: AtomicBool,
}

impl Peer {
    fn send(&self, datagram: &str) -> std::io::Result<()> {
        self.udp.send_to(datagram.as_bytes(), self.to).map(|_| ())
    }

    /// PstRotatorAz's antenna position, or `None` when it did not report one within
    /// [`ANSWER_WAIT`].
    fn azimuth(&self) -> Option<f64> {
        let _one = self.asking.lock().unwrap_or_else(|e| e.into_inner());
        let mut buf = [0u8; 512];
        // A late reply to an earlier question is not the answer to this one.
        if self.udp.set_nonblocking(true).is_ok() {
            while self.udp.recv_from(&mut buf).is_ok() {}
            let _ = self.udp.set_nonblocking(false);
        }
        let heard = self.send(AZ_QUERY).ok().and_then(|()| {
            let deadline = Instant::now() + ANSWER_WAIT;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() || self.udp.set_read_timeout(Some(left)).is_err() {
                    return None;
                }
                match self.udp.recv_from(&mut buf) {
                    // Only PstRotatorAz's own address may report a position: anyone else who
                    // can reach the port could otherwise set the bearing the compass shows. The
                    // manual names no source PORT for replies, so none is required.
                    Ok((n, from)) if from.ip() == self.to.ip() => {
                        if let Some(az) = parse_azimuth(&buf[..n]) {
                            return Some(az);
                        }
                    }
                    Ok(_) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        return None
                    }
                    // Windows reports an ICMP "port unreachable" from an earlier send — a PC
                    // where nothing listens on the control port — as an error on the NEXT
                    // receive. It is not the answer; keep waiting until the deadline.
                    Err(_) => std::thread::sleep(TICK),
                }
            }
        });
        let was_silent = self.silent.swap(heard.is_none(), Ordering::Relaxed);
        if heard.is_none() && !was_silent {
            tempo_core::applog::info(
                "rotator",
                &format!(
                    "PstRotatorAz at {} did not answer AZ? within {} ms on UDP {}: is Setup ▸ \
                     UDP Control ticked, and is its UDP Control Port {}?",
                    self.to,
                    ANSWER_WAIT.as_millis(),
                    self.to.port().saturating_add(1),
                    self.to.port()
                ),
            );
        } else if heard.is_some() && was_silent {
            tempo_core::applog::info("rotator", "PstRotatorAz is answering");
        }
        heard
    }

    /// `RPRT 0` when PstRotatorAz answers after `datagrams` went out, `RPRT -5` when it did not,
    /// `RPRT -6` when they could not be sent.
    fn command(&self, datagrams: &[&str]) -> &'static str {
        for (i, d) in datagrams.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(STOP_GAP);
            }
            if self.send(d).is_err() {
                return "RPRT -6\n";
            }
        }
        match self.azimuth() {
            Some(_) => "RPRT 0\n",
            None => "RPRT -5\n",
        }
    }
}

/// rotctld's answer to one command line. `None` asks to close the connection.
fn answer(peer: &Peer, line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let reply = match words.next()? {
        "p" | "\\get_pos" => match peer.azimuth() {
            Some(az) => format!("{az:.6}\n{:.6}\n", 0.0),
            None => "RPRT -5\n".into(),
        },
        "P" | "\\set_pos" => {
            let num = |w: Option<&str>| w.and_then(|w| w.parse::<f64>().ok());
            match (num(words.next()), num(words.next())) {
                // The ranges `\dump_state` declares, refused as Hamlib's frontend refuses them
                // (`RPRT -1`), so the satellite track's elevation probe learns "azimuth only".
                (Some(az), Some(el)) if (0.0..=360.0).contains(&az) && el == 0.0 => {
                    peer.command(&[&azimuth_datagram(az)]).into()
                }
                _ => "RPRT -1\n".into(),
            }
        }
        "S" | "\\stop" => peer.command(&[STOP_DATAGRAM, STOP_DATAGRAM]).into(),
        "K" | "\\park" => peer.command(&[PARK_DATAGRAM]).into(),
        "\\dump_state" => DUMP_STATE.into(),
        "q" | "Q" | "\\quit" => return None,
        _ => "RPRT -11\n".into(),
    };
    Some(reply)
}

/// One rotctld client, served until it hangs up or the link is dropped.
fn serve(mut conn: TcpStream, peer: Arc<Peer>, halt: Arc<AtomicBool>) {
    // An accepted socket inherits the listener's non-blocking mode on Windows.
    if conn.set_nonblocking(false).is_err() || conn.set_read_timeout(Some(TICK)).is_err() {
        return;
    }
    let mut pending = Vec::new();
    let mut buf = [0u8; 256];
    while !halt.load(Ordering::Relaxed) {
        match conn.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    let Some(reply) = answer(&peer, String::from_utf8_lossy(&line).trim()) else {
                        return;
                    };
                    if conn.write_all(reply.as_bytes()).is_err() {
                        return;
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

/// The running link: a rotctld-protocol listener on loopback, and the UDP socket to
/// PstRotatorAz. Dropping it stops both and frees both ports before it returns, so a link
/// restarted with new settings can bind them again at once.
pub struct PstRotatorLink {
    halt: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    tcp: SocketAddr,
}

impl PstRotatorLink {
    /// Start the link to the PstRotatorAz at `target` (`host:port`, its UDP Control Port; empty
    /// is [`DEFAULT_TARGET`]), answering rotctld commands on `127.0.0.1:tcp_port` (0: any free
    /// port, for a test).
    ///
    /// An `Err` names its cause in words an operator can act on: an address that is not
    /// `host:port`, the reply port held by another program, or the rotctld port taken.
    pub fn start(target: &str, tcp_port: u16) -> std::io::Result<Self> {
        let to = resolve(target)?;
        let udp = bind_replies(to)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, tcp_port)).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "could not listen on 127.0.0.1:{tcp_port} for Nexus's own rotator commands \
                     ({e}): another program holds that port"
                ),
            )
        })?;
        listener.set_nonblocking(true)?;
        let tcp = listener.local_addr()?;
        let peer = Arc::new(Peer {
            udp,
            to,
            asking: Mutex::new(()),
            silent: AtomicBool::new(false),
        });
        let halt = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&halt);
        let worker = std::thread::Builder::new()
            .name("pstrotator-link".into())
            .spawn(move || {
                let mut served: Vec<JoinHandle<()>> = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((conn, _)) => {
                            let (peer, stop) = (Arc::clone(&peer), Arc::clone(&stop));
                            if let Ok(h) = std::thread::Builder::new()
                                .name("pstrotator-client".into())
                                .spawn(move || serve(conn, peer, stop))
                            {
                                served.push(h);
                            }
                        }
                        // Nothing waiting, or an accept that failed on its own (a client that
                        // hung up first): look again shortly.
                        Err(_) => std::thread::sleep(TICK),
                    }
                    served.retain(|h| !h.is_finished());
                }
                for h in served {
                    let _ = h.join();
                }
            })?;
        tempo_core::applog::info(
            "rotator",
            &format!(
                "PstRotatorAz link: commands to UDP {to}, replies on UDP {}, rotctld on {tcp}",
                to.port() + 1
            ),
        );
        Ok(PstRotatorLink {
            halt,
            worker: Some(worker),
            tcp,
        })
    }

    /// Where the link answers rotctld commands.
    pub fn rotctld_addr(&self) -> SocketAddr {
        self.tcp
    }

    /// True while the link is serving.
    pub fn is_alive(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| !w.is_finished())
    }
}

impl Drop for PstRotatorLink {
    fn drop(&mut self) {
        self.halt.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Driven against a fake PstRotatorAz on real UDP sockets: it records every datagram it is
    //! sent and, when told to, answers `AZ?` on its port + 1 the way the manual says.
    use super::*;
    use crate::rotator;
    use std::sync::mpsc;

    /// A fake PstRotatorAz on 127.0.0.1. `answer`: the position it reports, or `None` for one
    /// with UDP Control switched off (it hears everything and says nothing).
    struct FakePst {
        port: u16,
        heard: mpsc::Receiver<String>,
    }

    fn fake(answer: Option<&'static str>) -> (FakePst, PstRotatorLink) {
        fake_late(answer, Duration::ZERO)
    }

    /// A fake that answers each `AZ?` `late` after it is asked.
    fn fake_late(answer: Option<&'static str>, late: Duration) -> (FakePst, PstRotatorLink) {
        // Its reply port is its own + 1, which another test may hold: try again on a fresh one.
        for _ in 0..50 {
            let sock = UdpSocket::bind("127.0.0.1:0").expect("a loopback UDP port");
            let port = sock.local_addr().expect("its port").port();
            let link = match PstRotatorLink::start(&format!("127.0.0.1:{port}"), 0) {
                Ok(link) => link,
                Err(_) => continue,
            };
            let (tx, heard) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = [0u8; 512];
                while let Ok((n, _)) = sock.recv_from(&mut buf) {
                    let d = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let (true, Some(a)) = (d == AZ_QUERY, answer) {
                        let sock = sock.try_clone().expect("the fake's socket");
                        std::thread::spawn(move || {
                            std::thread::sleep(late);
                            let _ = sock.send_to(a.as_bytes(), ("127.0.0.1", port + 1));
                        });
                    }
                    if tx.send(d).is_err() {
                        return;
                    }
                }
            });
            return (FakePst { port, heard }, link);
        }
        panic!("no loopback port pair free");
    }

    impl FakePst {
        /// Every datagram heard so far, other than `AZ?`.
        fn commands(&self) -> Vec<String> {
            self.heard.try_iter().filter(|d| d != AZ_QUERY).collect()
        }
    }

    fn addr(link: &PstRotatorLink) -> String {
        link.rotctld_addr().to_string()
    }

    #[test]
    fn the_datagrams_are_the_manuals_bytes() {
        assert_eq!(azimuth_datagram(90.0), "<PST><AZIMUTH>90</AZIMUTH></PST>");
        assert_eq!(azimuth_datagram(85.4), "<PST><AZIMUTH>85</AZIMUTH></PST>");
        assert_eq!(azimuth_datagram(123.5), "<PST><AZIMUTH>124</AZIMUTH></PST>");
        // Rounded first, then wrapped: never 360.
        assert_eq!(azimuth_datagram(359.6), "<PST><AZIMUTH>0</AZIMUTH></PST>");
        assert_eq!(azimuth_datagram(360.0), "<PST><AZIMUTH>0</AZIMUTH></PST>");
        assert_eq!(STOP_DATAGRAM, "<PST><STOP>1</STOP></PST>");
        assert_eq!(PARK_DATAGRAM, "<PST><PARK>1</PARK></PST>");
        assert_eq!(AZ_QUERY, "<PST>AZ?</PST>");
    }

    #[test]
    fn a_position_report_is_read_and_anything_else_is_not() {
        assert_eq!(parse_azimuth(b"AZ:123\r"), Some(123.0));
        assert_eq!(parse_azimuth(b"AZ:45.5\r"), Some(45.5));
        assert_eq!(parse_azimuth(b"AZ: 7"), Some(7.0));
        assert_eq!(
            parse_azimuth(b"TGA:90\r"),
            None,
            "the target is not the position"
        );
        assert_eq!(parse_azimuth(b"AZ:\r"), None);
        assert_eq!(parse_azimuth(b"AZ:nan\r"), None);
        assert_eq!(parse_azimuth(b"hello"), None);
    }

    #[test]
    fn pointing_at_90_sends_the_documented_bytes_and_is_confirmed() {
        let (pst, link) = fake(Some("AZ:12\r"));
        rotator::point(&addr(&link), 90.0).expect("PstRotatorAz answered, so RPRT 0");
        assert_eq!(pst.commands(), vec!["<PST><AZIMUTH>90</AZIMUTH></PST>"]);
    }

    #[test]
    fn the_compass_reads_what_pstrotatoraz_reports() {
        let (_pst, link) = fake(Some("AZ:237\r"));
        assert_eq!(rotator::read_azimuth(&addr(&link)), Some(237.0));
    }

    #[test]
    fn stop_goes_out_twice_and_park_once() {
        let (pst, link) = fake(Some("AZ:12\r"));
        rotator::stop(&addr(&link)).expect("a confirmed STOP");
        assert_eq!(pst.commands(), vec![STOP_DATAGRAM, STOP_DATAGRAM]);
        // Nothing in Nexus sends rotctld's `K` today; another program on the rotctld port may.
        let mut s = TcpStream::connect(link.rotctld_addr()).expect("the rotctld port");
        s.write_all(b"K\n").expect("sent");
        let mut got = [0u8; 32];
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let n = s.read(&mut got).expect("an answer");
        assert_eq!(&got[..n], b"RPRT 0\n");
        assert_eq!(pst.commands(), vec![PARK_DATAGRAM]);
    }

    #[test]
    fn a_silent_pstrotatoraz_shows_no_position_and_confirms_nothing() {
        let (pst, link) = fake(None);
        let a = addr(&link);
        let t = Instant::now();
        assert_eq!(
            rotator::read_azimuth(&a),
            None,
            "no position, never 0 or the target"
        );
        assert!(
            t.elapsed() < Duration::from_millis(1_400),
            "{:?}",
            t.elapsed()
        );
        let e = rotator::point(&a, 90.0).expect_err("nothing confirmed the move");
        assert!(e.to_string().contains("RPRT -5"), "{e}");
        // The command still went out: it is the confirmation that is missing.
        assert_eq!(pst.commands(), vec!["<PST><AZIMUTH>90</AZIMUTH></PST>"]);
        let e = rotator::stop(&a).expect_err("an unconfirmed STOP is not a STOP");
        assert!(e.to_string().contains("RPRT -5"), "{e}");
    }

    #[test]
    fn a_reply_from_another_address_is_ignored_and_pstrotatoraz_s_own_is_read() {
        // 127.0.0.2 is this machine too, but it is not the PstRotatorAz the port box names.
        let spoof = UdpSocket::bind("127.0.0.2:0").expect("a second loopback address");
        let (pst, link) = fake_late(Some("AZ:12\r"), Duration::from_millis(150));
        let reply = ("127.0.0.1", pst.port + 1);
        let a = addr(&link);
        let flood = std::thread::spawn(move || {
            for _ in 0..30 {
                let _ = spoof.send_to(b"AZ:999\r", reply);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert_eq!(
            rotator::read_azimuth(&a),
            Some(12.0),
            "the spoofed bearing is not read"
        );
        flood.join().unwrap();
    }

    #[test]
    fn a_loopback_pstrotatoraz_is_heard_on_loopback_only() {
        // This PC's address on its network, if it has one: `connect` on UDP only picks a route.
        let lan = UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
            .and_then(|s| s.local_addr())
            .map(|a| a.ip())
            .ok()
            .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
        let Some(lan) = lan else {
            eprintln!("!! NOT RUN, and NOT A PASS: no non-loopback address on this machine");
            return;
        };
        let (pst, _link) = fake(Some("AZ:12\r"));
        // The link holds only 127.0.0.1:<reply>, so the same port on the network address is free:
        // nothing on the LAN can reach the socket the link reads.
        UdpSocket::bind((lan, pst.port + 1)).expect("the reply port is not bound on the LAN");
    }

    #[test]
    fn a_late_reply_is_not_the_answer_to_the_next_question() {
        // Each answer arrives after the question has given up on it.
        let (_pst, link) = fake_late(Some("AZ:77\r"), ANSWER_WAIT + Duration::from_millis(150));
        let a = addr(&link);
        assert_eq!(rotator::read_azimuth(&a), None);
        std::thread::sleep(Duration::from_millis(300));
        // The first answer is waiting on the socket now; the second question must not take it.
        assert_eq!(rotator::read_azimuth(&a), None);
    }

    #[test]
    fn a_silent_poll_does_not_hold_up_a_stop() {
        let (pst, link) = fake(None);
        let a = addr(&link);
        let poll = {
            let a = a.clone();
            std::thread::spawn(move || rotator::read_azimuth(&a))
        };
        // The poll's AZ? is out, and it is waiting out its second of silence.
        assert_eq!(
            pst.heard.recv_timeout(Duration::from_secs(2)).as_deref(),
            Ok(AZ_QUERY)
        );
        let stop = {
            let a = a.clone();
            std::thread::spawn(move || rotator::stop(&a))
        };
        // STOP's two datagrams arrive while the poll still waits: they do not queue behind it.
        let mut stops = Vec::new();
        while stops.len() < 2 {
            let d = pst
                .heard
                .recv_timeout(Duration::from_millis(500))
                .expect("STOP went out while the poll was still waiting");
            if d != AZ_QUERY {
                stops.push(d);
            }
        }
        assert_eq!(stops, vec![STOP_DATAGRAM, STOP_DATAGRAM]);
        assert!(!poll.is_finished(), "the poll had not given up yet");
        assert_eq!(poll.join().unwrap(), None);
        assert!(stop.join().unwrap().is_err(), "nobody confirmed it");
    }

    #[test]
    fn azimuth_only_is_declared_and_elevation_refused() {
        let (pst, link) = fake(Some("AZ:12\r"));
        let a = addr(&link);
        let limits = rotator::read_limits(&a)
            .expect("answered")
            .expect("declared");
        assert_eq!(limits.model, MODEL);
        assert_eq!((limits.min_az, limits.max_az), (0.0, 360.0));
        assert_eq!(
            limits.elevation(),
            None,
            "no elevation axis in the Rotor pane"
        );
        let e = rotator::point_azel(&a, 90.0, 30.0).expect_err("an elevation is refused");
        assert!(e.to_string().contains("RPRT -1"), "{e}");
        assert!(pst.commands().is_empty(), "a refused command sends nothing");
    }

    #[test]
    fn a_taken_reply_port_is_named_with_the_way_out() {
        let held = UdpSocket::bind("0.0.0.0:0").expect("a UDP port");
        let reply = held.local_addr().unwrap().port();
        let control = reply - 1;
        let e = PstRotatorLink::start(&format!("127.0.0.1:{control}"), 0)
            .err()
            .expect("the reply port is taken");
        let words = e.to_string();
        assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse, "{words}");
        assert!(
            words.contains(&format!("UDP port {reply} is already in use")),
            "{words}"
        );
        assert!(
            words.contains(&format!("127.0.0.1:{}", control + 2)),
            "{words}"
        );
        drop(held);
    }

    #[test]
    fn an_address_that_is_not_host_port_says_so() {
        let e = PstRotatorLink::start("COM3", 0)
            .err()
            .expect("not an address");
        assert!(e.to_string().contains("not host:port"), "{e}");
    }

    #[test]
    fn dropping_the_link_frees_both_ports() {
        let (pst, link) = fake(Some("AZ:1\r"));
        let tcp = link.rotctld_addr();
        drop(link);
        UdpSocket::bind(("0.0.0.0", pst.port + 1)).expect("the reply port is free again");
        TcpListener::bind(tcp).expect("the rotctld port is free again");
    }
}
