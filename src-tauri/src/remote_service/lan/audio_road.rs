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
//! - **An internet road** ([`AudioRoad::open_internet`]), one connection's in place of the LAN's:
//!   a socket of its own, ICE to find the pair its computer reaches it on (`tempo_client::ice`), and
//!   a thread of its own built from the same [`Powers`], heard only at the pair ICE selected.
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
use super::datagram::{self, End, Keys, Tag, AUDIO_MAX, EXPORTED};
use super::stop_road::{press_id, Carrier, Hears};
/// What the road carries (the client core's, which both ends read).
pub(crate) use tempo_client::audio_road::wire::{Down, Up};
use tempo_client::ice::IceRoad;

/// The road's port, past the listener's TCP port: 42076 beside 42075.
pub(crate) const PORT_AFTER: u16 = 1;
/// How long after its last `listen` a computer is still sent the station's audio.
pub(crate) const LISTEN_LAPSE: Duration = Duration::from_secs(2);
/// How often a listening computer's admission is asked again: the audio lane's own second.
const RECHECK: Duration = Duration::from_secs(1);
/// How long a frame may wait for the socket before it is dropped.
pub(crate) const SEND_WAIT: Duration = Duration::from_millis(1);
/// The longest the thread waits for a datagram before it looks at its roads again: half a frame.
pub(crate) const TICK: Duration = Duration::from_millis(10);
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
    /// What an internet road's own thread is built from: the same powers and counts, and the
    /// listener's stop.
    powers: Arc<Powers>,
    counts: Arc<Counts>,
    stopping: Arc<AtomicBool>,
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
        let powers = Arc::new(powers);
        let thread = {
            let (stopping, counts, powers) = (stopping.clone(), counts.clone(), powers.clone());
            std::thread::Builder::new()
                .name("nexus-remote-audio-road".into())
                .spawn(move || {
                    Thread {
                        carrier: Carrier::Lan(socket, hears),
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
        Some(Running {
            road: AudioRoad {
                port,
                changes,
                tags: Arc::new(AtomicU64::new(super::stop_road::first_tag())),
                powers,
                counts: counts.clone(),
                stopping: stopping.clone(),
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
    /// The road of a listener with no socket of its own on the LAN, the room's tunnel's: only
    /// internet roads open on it ([`AudioRoad::open_internet`]), each on its own socket and thread
    /// from `powers`, ending with its connection. It has no thread, so nothing of its own to stop.
    pub(crate) fn internet_only(powers: Powers) -> Self {
        Self {
            port: 0,
            // Nothing reads it: a road of the LAN's never opens here.
            changes: mpsc::channel().0,
            tags: Arc::new(AtomicU64::new(super::stop_road::first_tag())),
            powers: Arc::new(powers),
            counts: Arc::new(Counts::default()),
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn open(
        &self,
        exported: &[u8; EXPORTED],
        session: &str,
        device: &str,
        stop: Option<Tag>,
    ) -> Option<Opened> {
        let (tag, road) = self.road(exported, session, device, stop)?;
        self.changes.send(Change::Open(tag, Box::new(road))).ok()?;
        Some(Opened {
            port: self.port,
            tag,
            closing: self.changes.clone(),
        })
    }

    /// Opens a connection's internet road: on `ice` (its own socket, bound with the road's tick
    /// and written with [`SEND_WAIT`], heard only at the pair ICE selects), with a thread of its own
    /// built from the same powers, its keys from `exported` (the connection's TLS exporter under
    /// `datagram::INTERNET_AUDIO_LABEL`, with `session` as the context). It goes, its thread with
    /// it, when the returned handle does. `None` once the listener has stopped, or if no thread
    /// started.
    pub(crate) fn open_internet(
        &self,
        ice: Arc<IceRoad>,
        exported: &[u8; EXPORTED],
        session: &str,
        device: &str,
        stop: Option<Tag>,
    ) -> Option<Opened> {
        if self.stopping.load(Ordering::SeqCst) {
            return None;
        }
        let (tag, road) = self.road(exported, session, device, stop)?;
        let (closing, changed) = mpsc::channel();
        let thread = Thread {
            carrier: Carrier::Internet(ice),
            powers: self.powers.clone(),
            changed,
            stopping: self.stopping.clone(),
            counts: self.counts.clone(),
            roads: HashMap::from([(tag, road)]),
        };
        std::thread::Builder::new()
            .name("nexus-remote-internet-audio-road".into())
            .spawn(move || thread.run())
            .ok()?;
        Some(Opened {
            port: 0,
            tag,
            closing,
        })
    }

    /// A new road's tag and its keys at the shack's end. Tags rise from a random start, and none
    /// reads as a STUN message on an internet road's socket.
    fn road(
        &self,
        exported: &[u8; EXPORTED],
        session: &str,
        device: &str,
        stop: Option<Tag>,
    ) -> Option<(Tag, Road)> {
        let tag = loop {
            let tag = self.tags.fetch_add(1, Ordering::SeqCst).to_be_bytes();
            if !tempo_client::ice::reads_as_stun(&tag) {
                break tag;
            }
        };
        let road = Road {
            keys: Keys::sized(exported, tag, End::Shack, AUDIO_MAX)?,
            device: device.to_string(),
            session: session.to_string(),
            stop,
            peer: None,
            listening: None,
            decoding: None,
        };
        Some((tag, road))
    }
}

/// A connection's audio road, open: what its welcome names. Closed when dropped.
pub(crate) struct Opened {
    port: u16,
    pub(crate) tag: Tag,
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

/// `down`, sealed under `road`'s keys, to where its computer last spoke from (on the internet, by
/// the pair ICE selected): whether it went.
fn send(carrier: &Carrier, road: &Road, down: &Down) -> bool {
    match (road.peer, road.keys.send.seal(down.kind(), &down.body())) {
        (Some(peer), Some(datagram)) => carrier.send_to(&datagram, peer),
        _ => false,
    }
}

/// One thread of the road: the listener's, for every connection's LAN road on its one socket, or
/// one connection's internet road, alone on its own.
struct Thread {
    carrier: Carrier,
    powers: Arc<Powers>,
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
            let received = self.carrier.recv_from(&mut buffer);
            // A road opened before its welcome went out is in the table before anything its
            // computer sent after reading that welcome is read. An internet road's thread opens no
            // other, and goes with its one road.
            let internet = matches!(self.carrier, Carrier::Internet(_));
            if (!self.take_changes() && !internet) || (internet && self.roads.is_empty()) {
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
        if !self.carrier.hears(from) {
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
                let counted = if send(&self.carrier, road, &down) {
                    &self.counts.rx_sent
                } else {
                    &self.counts.rx_dropped
                };
                counted.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
