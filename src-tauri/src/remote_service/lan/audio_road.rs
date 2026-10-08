//! The audio road, at the shack: a UDP socket of its own beside the listener, and the one thread
//! that reads it. A computer driving the shack (`crate::client_mode`) hears the station's receive
//! audio on it and, once a press from afar is allowed, speaks into the station's microphone over
//! on it. Nothing on it can stop anything, or keep anything from stopping: Stop has its own road,
//! socket and thread (`super::stop_road`), and this one holds none of their handles.
//!
//! ## The road
//!
//! - **The socket** is on the listener's own address, at [`PORT_AFTER`] past its TCP port (42076,
//!   between the command link's 42075 and Stop's road's 42077), or at a port the system gives when
//!   that one is taken: the welcome names it (`audioRoad`). Only the listener's own subnet is heard.
//! - **One road per connection**, opened as the shack welcomes it ([`AudioRoad::open`]): its own
//!   tag and its own keys, from that connection's TLS session under the audio road's own label
//!   (`super::datagram::AUDIO_LABEL`), so they are never Stop's. They go the moment the connection
//!   does: late audio is worth nothing.
//! - **One thread** for every connection's road (as Stop's road keeps one), holding the socket,
//!   the roads' keys and [`Powers`]: the station's one receive encoder, the audio lane's admission
//!   (which never waits), the held PTT, read only, and the microphone over's feed. No Engine, no
//!   command link, no Stop road.
//!
//! ## What it carries ([`Up`], [`Down`]), one 20 ms frame a datagram, at most `AUDIO_MAX` bytes
//!
//! From the computer: `listen`, every half second while it wants the station's audio, naming its
//! lease; `mic`, one frame of its microphone (Opus, voice, 32 kbit/s constant), only while its
//! press is live, with the press's number and the frame's. From the shack: `rx`, one frame of
//! receive audio (the stream's own encoder: CELT, 24 kbit/s), with the capture's epoch and the
//! frame's number. Nothing is answered before the computer has spoken, and only to the address of
//! the last datagram that verified.
//!
//! ## Receive audio: the audio lane's rule, and its "drop, never queue"
//!
//! Sent while the computer listens and controls the station, exactly as the relay's Listen lane
//! admits a browser (`Authority::audio_admitted`: the operator's control grant and this computer's
//! own live lease), asked when it starts and every second; contention is not a refusal. A computer
//! that stops saying `listen` for [`LISTEN_LAPSE`] is sent nothing more. The station encodes each
//! frame once for every listener ([`ReceiveFanout`]); a frame the socket will not take within
//! [`SEND_WAIT`] is dropped, never queued, so audio never piles up behind a stalled network.
//!
//! ## The microphone: a transmission, taken only for this computer's keyed press
//!
//! A `mic` frame belongs to a press, and a press is a transmission start: refused, before anything
//! decodes it, unless the held press is wired from afar (`command_dispatch::press_from_afar`). Past
//! that switch a frame is taken only while its press is the one the held PTT has keyed, named as
//! Stop's road names it on the same connection (`PttHold::keyed_by`), so a frame can never reach
//! an over another computer's or a page's press armed. It is then decoded here (`MicDecoder`, a
//! fresh one for each press) to the transmit route's 12 kHz, its place on the over's clock taken
//! from its own number, and handed to the microphone over (`MicFeed`), which takes audio only while
//! an over is armed: M1–M12 hold as they are, from the lead-in to the 200 ms gap.
use std::collections::HashMap;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tempo_app::mic::{MicFeed, MicFrame};
use tempo_app::remote_control::ptt_hold::PttHold;
use tempo_audio::mic_decode::{MicDecoder, FRAME_SAMPLES};

use super::super::audio::{ReceiveFanout, Subscription};
use super::datagram::{self, End, Keys, Tag, AUDIO_MAX, EXPORTED, HEADER};
use super::stop_road::{press_id, Hears};

/// The road's port, past the listener's TCP port: 42076 beside 42075.
pub(crate) const PORT_AFTER: u16 = 1;
/// How long after its last `listen` a computer is still sent the station's audio.
pub(crate) const LISTEN_LAPSE: Duration = Duration::from_secs(2);
/// How often a listening computer's admission is asked again: the audio lane's own second.
const RECHECK: Duration = Duration::from_secs(1);
/// How long a frame may wait for the socket before it is dropped.
const SEND_WAIT: Duration = Duration::from_millis(1);
/// The longest the thread waits for a datagram before it looks at its roads again: half a frame.
const TICK: Duration = Duration::from_millis(10);
/// The road's message format, in the build's contract.
const FORMAT: u8 = 1;
/// The AEAD's tag after the body.
const SEAL: usize = 16;
/// The longest body a datagram on this road carries.
const BODY: usize = AUDIO_MAX - HEADER - SEAL;

const LISTEN: u8 = 1;
const MIC: u8 = 2;
const RX: u8 = 16;

/// What a computer sends on its audio road.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Up {
    /// Send the station's audio here: the computer's lease.
    Listen { lease: String },
    /// One frame of the computer's microphone: its press, its number in that press, and the packet.
    Mic {
        press: u64,
        frame: u32,
        packet: Vec<u8>,
    },
}

/// What the shack sends back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Down {
    /// One frame of receive audio: the capture's epoch, its number in that epoch, and the packet.
    Rx {
        epoch: u64,
        seq: u32,
        packet: Vec<u8>,
    },
}

impl Up {
    pub(crate) fn kind(&self) -> u8 {
        match self {
            Up::Listen { .. } => LISTEN,
            Up::Mic { .. } => MIC,
        }
    }

    pub(crate) fn body(&self) -> Vec<u8> {
        match self {
            Up::Listen { lease } => lease.as_bytes().to_vec(),
            Up::Mic {
                press,
                frame,
                packet,
            } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&frame.to_be_bytes());
                body.extend_from_slice(packet);
                body
            }
        }
    }

    /// A body that verified, read exactly: a lease the station could have issued, or a frame with a
    /// packet in it.
    pub(crate) fn read(kind: u8, body: &[u8]) -> Option<Self> {
        Some(match kind {
            LISTEN if body.len() == 36 => {
                let lease = std::str::from_utf8(body).ok()?;
                if !super::super::transport::identifier(lease) {
                    return None;
                }
                Up::Listen {
                    lease: lease.to_string(),
                }
            }
            MIC if (13..=BODY).contains(&body.len()) => Up::Mic {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                frame: u32::from_be_bytes(body[8..12].try_into().ok()?),
                packet: body[12..].to_vec(),
            },
            _ => return None,
        })
    }
}

impl Down {
    pub(crate) fn kind(&self) -> u8 {
        match self {
            Down::Rx { .. } => RX,
        }
    }

    pub(crate) fn body(&self) -> Vec<u8> {
        match self {
            Down::Rx { epoch, seq, packet } => {
                let mut body = epoch.to_be_bytes().to_vec();
                body.extend_from_slice(&seq.to_be_bytes());
                body.extend_from_slice(packet);
                body
            }
        }
    }

    /// A body that verified, read exactly.
    pub(crate) fn read(kind: u8, body: &[u8]) -> Option<Self> {
        match kind {
            RX if (13..=BODY).contains(&body.len()) => Some(Down::Rx {
                epoch: u64::from_be_bytes(body[..8].try_into().ok()?),
                seq: u32::from_be_bytes(body[8..12].try_into().ok()?),
                packet: body[12..].to_vec(),
            }),
            _ => None,
        }
    }
}

/// The road's format, for the build's contract (`super::wire::contract`).
pub(crate) fn contract_text() -> String {
    format!(
        "audio road {} {} format {FORMAT} max {AUDIO_MAX} up listen={LISTEN}(lease) \
         mic={MIC}(press,frame,opus) down rx={RX}(epoch,seq,opus)\n",
        datagram::VERSION,
        String::from_utf8_lossy(datagram::AUDIO_LABEL),
    )
}

/// The audio lane's admission, as the road asks it: a session, a device and a lease, now. Never
/// waits: `remoteBusy` when the authority is busy, which is not a refusal.
pub(crate) type Admitted =
    Arc<dyn Fn(&str, &str, &str, Instant) -> Result<(), &'static str> + Send + Sync>;

/// Everything the road's thread may do besides read and write its own socket.
pub(crate) struct Powers {
    /// The station's one receive encoder, shared with the relay's Listen lane and every stream.
    /// `None` on a station with no capture path: the road then sends nothing.
    pub(crate) receive: Option<Arc<ReceiveFanout>>,
    /// May this computer hear the station (`Authority::audio_admitted`)?
    pub(crate) admitted: Admitted,
    /// The held PTT, read: which press is keyed.
    pub(crate) ptt: PttHold,
    /// The microphone over's feed.
    pub(crate) mic: MicFeed,
}

/// What the road has done, counted: for the tests, and for nothing that decides.
#[derive(Default)]
pub(crate) struct Counts {
    /// Microphone frames that verified and were read.
    pub(crate) mic_read: AtomicU64,
    /// …of which refused: every start from afar refused, or not the keyed press's.
    pub(crate) mic_refused: AtomicU64,
    /// …of which the microphone over took.
    pub(crate) mic_taken: AtomicU64,
    /// Receive frames sent.
    pub(crate) rx_sent: AtomicU64,
    /// Receive frames dropped: the socket would not take them in time, or a listener's own bound.
    pub(crate) rx_dropped: AtomicU64,
}

/// A listening computer.
struct Listening {
    lease: String,
    /// When it last said `listen`.
    heard: Instant,
    /// When its admission was last asked, and the answer.
    checked: Option<Instant>,
    admitted: bool,
    /// Its share of the station's one encoder, while it is sent audio.
    audio: Option<Subscription>,
}

/// One connection's road, as the thread keeps it.
struct Road {
    keys: Keys,
    device: String,
    session: String,
    /// Stop's road on the same connection: its tag names this computer's presses at the held PTT.
    stop: Option<Tag>,
    /// Where this computer last spoke from: where the shack sends.
    peer: Option<SocketAddr>,
    listening: Option<Listening>,
    /// The press whose frames are being decoded, and its decoder.
    decoding: Option<(u64, MicDecoder)>,
}

enum Change {
    Open(Tag, Box<Road>),
    Closed(Tag),
}

/// The road, as the listener and each connection hold it. Cheap to clone.
#[derive(Clone)]
pub(crate) struct AudioRoad {
    port: u16,
    changes: mpsc::Sender<Change>,
    tags: Arc<AtomicU64>,
}

/// The road's socket and thread, as the listener holds them until it stops.
pub(crate) struct Running {
    road: AudioRoad,
    stopping: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
    #[cfg_attr(not(test), allow(dead_code))]
    counts: Arc<Counts>,
}

impl Running {
    pub(crate) fn road(&self) -> AudioRoad {
        self.road.clone()
    }

    /// What the road has done so far.
    #[cfg(test)]
    pub(crate) fn counts(&self) -> &Counts {
        &self.counts
    }

    /// Stops the thread, and with it the socket and every road's keys, within one [`TICK`].
    pub(crate) fn stop(self) -> std::thread::JoinHandle<()> {
        self.stopping.store(true, Ordering::SeqCst);
        self.thread
    }
}

impl AudioRoad {
    /// The road on `at`, or on `at`'s address at a port the system gives when `at`'s is taken, with
    /// its thread. `None` if no socket could be had or no thread started: the listener then runs
    /// without it, and the computer has no audio.
    pub(crate) fn start(at: SocketAddrV4, hears: Hears, powers: Powers) -> Option<Running> {
        let socket = UdpSocket::bind(at)
            .or_else(|_| UdpSocket::bind(SocketAddrV4::new(*at.ip(), 0)))
            .ok()?;
        socket.set_read_timeout(Some(TICK)).ok()?;
        socket.set_write_timeout(Some(SEND_WAIT)).ok()?;
        let port = socket.local_addr().ok()?.port();
        let (changes, changed) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Counts::default());
        let thread = {
            let (stopping, counts) = (stopping.clone(), counts.clone());
            std::thread::Builder::new()
                .name("nexus-remote-audio-road".into())
                .spawn(move || {
                    Thread {
                        socket,
                        hears,
                        powers,
                        changed,
                        stopping,
                        counts,
                        roads: HashMap::new(),
                    }
                    .run()
                })
                .ok()?
        };
        let start = {
            use ring::rand::SecureRandom;
            let mut bytes = [0; 8];
            let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
            u64::from_be_bytes(bytes)
        };
        Some(Running {
            road: AudioRoad {
                port,
                changes,
                tags: Arc::new(AtomicU64::new(start)),
            },
            stopping,
            thread,
            counts,
        })
    }

    /// Opens a connection's road, as the shack welcomes it: its keys from `exported` (the
    /// connection's TLS exporter under `datagram::AUDIO_LABEL`, with `session` as the context), for
    /// `device`, whose presses Stop's road on the same connection names by `stop`. The road goes
    /// when the returned handle does. `None` once the road has stopped.
    pub(crate) fn open(
        &self,
        exported: &[u8; EXPORTED],
        session: &str,
        device: &str,
        stop: Option<Tag>,
    ) -> Option<Opened> {
        let tag = self.tags.fetch_add(1, Ordering::SeqCst).to_be_bytes();
        let road = Road {
            keys: Keys::sized(exported, tag, End::Shack, AUDIO_MAX)?,
            device: device.to_string(),
            session: session.to_string(),
            stop,
            peer: None,
            listening: None,
            decoding: None,
        };
        self.changes.send(Change::Open(tag, Box::new(road))).ok()?;
        Some(Opened {
            port: self.port,
            tag,
            closing: self.changes.clone(),
        })
    }
}

/// A connection's audio road, open: what its welcome names. Closed when dropped.
pub(crate) struct Opened {
    port: u16,
    tag: Tag,
    closing: mpsc::Sender<Change>,
}

impl Opened {
    /// The road as the welcome names it: `{"port", "tag"}`, the tag as lowercase hex.
    pub(crate) fn offer(&self) -> serde_json::Value {
        let tag: String = self.tag.iter().map(|b| format!("{b:02x}")).collect();
        serde_json::json!({ "port": self.port, "tag": tag })
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.closing.send(Change::Closed(self.tag));
    }
}

/// `down`, sealed under `road`'s keys, to where its computer last spoke from: whether it went.
fn send(socket: &UdpSocket, road: &Road, down: &Down) -> bool {
    match (road.peer, road.keys.send.seal(down.kind(), &down.body())) {
        (Some(peer), Some(datagram)) => socket.send_to(&datagram, peer).is_ok(),
        _ => false,
    }
}

struct Thread {
    socket: UdpSocket,
    hears: Hears,
    powers: Powers,
    changed: mpsc::Receiver<Change>,
    stopping: Arc<AtomicBool>,
    counts: Arc<Counts>,
    roads: HashMap<Tag, Road>,
}

impl Thread {
    fn run(mut self) {
        let mut buffer = [0u8; 2048];
        let mut looked = Instant::now();
        while !self.stopping.load(Ordering::SeqCst) {
            let received = self.socket.recv_from(&mut buffer);
            // A road opened before its welcome went out is in the table before anything its
            // computer sent after reading that welcome is read.
            if !self.take_changes() {
                break;
            }
            match received {
                Ok((length, from)) => self.datagram(&buffer[..length], from, Instant::now()),
                Err(error) if tempo_stream::session::receive_ends_session(&error) => {
                    tempo_core::applog::warn(
                        "remote",
                        &format!("on this network: the audio road stopped ({error})"),
                    );
                    break;
                }
                Err(_) => {}
            }
            let now = Instant::now();
            if now.saturating_duration_since(looked) >= TICK {
                looked = now;
                self.look(now);
            }
        }
    }

    /// Roads opened and closed since the last look. `false` once nothing can open one any more.
    fn take_changes(&mut self) -> bool {
        loop {
            match self.changed.try_recv() {
                Ok(Change::Open(tag, road)) => {
                    self.roads.entry(tag).or_insert(*road);
                }
                // Its keys and its share of the encoder go at once.
                Ok(Change::Closed(tag)) => {
                    self.roads.remove(&tag);
                }
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// One datagram: dropped unread unless it comes from the subnet, names a road here, and verifies
    /// under that road's keys with a sequence it has not seen; then read, and acted on.
    fn datagram(&mut self, bytes: &[u8], from: SocketAddr, now: Instant) {
        if !self.hears.hears(from.ip()) {
            return;
        }
        let Some(tag) = datagram::tag_of(bytes, AUDIO_MAX) else {
            return;
        };
        let Some(road) = self.roads.get_mut(&tag) else {
            return;
        };
        let Some((kind, body)) = road.keys.receive.open(bytes) else {
            return;
        };
        let Some(up) = Up::read(kind, &body) else {
            return;
        };
        road.peer = Some(from);
        match up {
            Up::Listen { lease } => match road.listening.as_mut() {
                Some(listening) if listening.lease == lease => listening.heard = now,
                // A new lease is asked about afresh.
                _ => {
                    road.listening = Some(Listening {
                        lease,
                        heard: now,
                        checked: None,
                        admitted: false,
                        audio: None,
                    })
                }
            },
            Up::Mic {
                press,
                frame,
                packet,
            } => {
                self.counts.mic_read.fetch_add(1, Ordering::Relaxed);
                // ⛔ A PRESS'S AUDIO IS A TRANSMISSION: refused before anything decodes it unless
                // the held press is wired from afar; past that switch, only the press the held PTT
                // keyed for this computer, named as Stop's road names it.
                let keyed = crate::command_dispatch::press_from_afar()
                    && road
                        .stop
                        .is_some_and(|stop| self.powers.ptt.keyed_by(&press_id(&stop, press)));
                if !keyed {
                    self.counts.mic_refused.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                if road.decoding.as_ref().map(|(on, _)| *on) != Some(press) {
                    road.decoding = MicDecoder::new().ok().map(|decoder| (press, decoder));
                }
                // A packet libopus refuses is that moment's silence, as a lost one is.
                let Some(samples) = road
                    .decoding
                    .as_mut()
                    .and_then(|(_, decoder)| decoder.decode(&packet))
                else {
                    return;
                };
                let taken = self.powers.mic.push(MicFrame {
                    seq: u64::from(frame),
                    media: u64::from(frame) * FRAME_SAMPLES as u64,
                    arrived: now,
                    samples,
                });
                if taken.is_ok() {
                    self.counts.mic_taken.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Every [`TICK`]: each listening computer is asked about again on the lane's second, given
    /// its share of the encoder once admitted, and sent every frame encoded since the last look.
    fn look(&mut self, now: Instant) {
        for road in self.roads.values_mut() {
            let Some(listening) = road.listening.as_mut() else {
                continue;
            };
            if now.saturating_duration_since(listening.heard) > LISTEN_LAPSE {
                road.listening = None;
                continue;
            }
            if listening
                .checked
                .is_none_or(|at| now.saturating_duration_since(at) >= RECHECK)
            {
                listening.checked = Some(now);
                match (self.powers.admitted)(&road.session, &road.device, &listening.lease, now) {
                    Ok(()) => listening.admitted = true,
                    // Contention, not a refusal: as it was.
                    Err("remoteBusy") => {}
                    Err(_) => {
                        listening.admitted = false;
                        listening.audio = None;
                    }
                }
                if listening.admitted && listening.audio.is_none() {
                    listening.audio = self
                        .powers
                        .receive
                        .as_ref()
                        .and_then(|receive| receive.subscribe().ok());
                }
            }
            let taken = match listening.audio.as_ref().map(|audio| audio.take(now)) {
                Some(Ok(taken)) => taken,
                // The capture source changed, or the encoder ended: a fresh share at the next
                // second's look, on the new source, under a new epoch.
                Some(Err(_)) => {
                    listening.audio = None;
                    continue;
                }
                None => continue,
            };
            self.counts
                .rx_dropped
                .fetch_add(taken.dropped, Ordering::Relaxed);
            for frame in taken.frames {
                let down = Down::Rx {
                    epoch: frame.epoch,
                    seq: frame.seq,
                    packet: frame.packet,
                };
                let counted = if send(&self.socket, road, &down) {
                    &self.counts.rx_sent
                } else {
                    &self.counts.rx_dropped
                };
                counted.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
