//! Stop's second road, at the shack: a UDP socket of its own beside the listener, and the one
//! thread that reads it and does nothing else. A computer driving the shack (`crate::client_mode`)
//! sends its Stop here as well as on the command link, so either road alone stops the station; its
//! heartbeat comes here too, and presence is renewed from nowhere else, so whatever can be on the
//! air, this road has just worked.
//!
//! ## The road
//!
//! - **The socket** is on the listener's own address, at [`PORT_AFTER`] past its TCP port (42077
//!   beside 42075), or at a port the system gives when that one is taken: the welcome names it, so
//!   the number is no contract. Only the listener's own subnet is heard ([`Hears`]).
//! - **One road per connection**, opened as the shack welcomes it ([`StopRoad::open`]): its own
//!   tag, named in the welcome, and its own keys, made from that connection's TLS session
//!   (`super::datagram`). They go when the connection does, but for [`AFTERLIFE`]: a Stop sent as
//!   the link drops, or after, is still taken.
//! - **The thread** holds the socket, the roads' keys, and [`Powers`]: the Stop path, presence's
//!   renewal, the held PTT, and who is paired. Nothing else: no Engine, no command link, no audio,
//!   no lock any of those hold. A test holds this file to that ([`Powers`]' own words). A Stop's
//!   own handling, from the datagram to its answer, takes no lock at all: whether its computer is
//!   still paired is kept on its road, asked when the road opens and again, before the next
//!   datagram is read, whenever who is paired changes.
//!
//! ## What it carries ([`Up`], [`Down`])
//!
//! From the computer: `stop` at the press and every 50 ms until it is acknowledged, numbered by how
//! many Stops its computer has pressed this run, itself included; `hold` and `release` for a held
//! PTT, each hold naming how many Stops had been pressed when its press began; `heartbeat` every
//! 500 ms, naming its lease and the last snapshot its screen fetched. From the shack: `stopped` for every Stop, each press's report, the microphone
//! over's state with its holes on the air so far (display only) when it changes and again after
//! each renewal, and presence's deadline at each renewal. Each message is one datagram of at most 128 bytes, and nothing is answered before this
//! computer has spoken: the shack answers the address of the last datagram that verified. The
//! messages' format has its own number, in the build's contract. Both ends read and write them with
//! the client core's (`tempo_client::stop_road::wire`), with the state echo's clock ([`StateClock`]).
//!
//! ## Stop is never refused here
//!
//! A Stop that verifies under a road's keys is that computer's, and it is admitted while the
//! computer is paired here, which is station control on this road: not its lease, the lease's age,
//! presence, a rate, or whether its connection is still open. It is noted first, by its number,
//! where the connection's command lane reads it ([`Ready`]); it then retires the current transmit
//! generation at once and stops every transmission at the station however it started
//! (`operations::RoadStop`); it is answered at once, never after the Engine. Its number refuses
//! nothing but a start or a press its computer made before it, whatever number it carries. Only two
//! things are refused: a datagram that does not verify, which is not this computer's Stop, and a computer
//! removed at the shack, which has no grant and changes nothing.
//!
//! ## Presence, and the press
//!
//! - **Presence** is renewed only by a heartbeat naming a snapshot this connection's command link
//!   took under two seconds before, by the shack's own clock ([`StateClock`]), and only under the
//!   computer's own lease (`operations::RoadPresence`). A number never sent, or older than that,
//!   renews nothing; the heartbeat that renews it is answered with the deadline. A renewal that
//!   finds the Engine busy is handed to it at the thread's next look, never waited for.
//! - **The press is a transmission start**, taken only while the held press is wired from afar
//!   (`command_dispatch::press_from_afar`), the connection is open, and this road's presence is
//!   live; a press new to the road also needs the state its computer's screen last fetched under
//!   two seconds old, and to have begun after every Stop this connection has had taken, on either
//!   road: a hold the network delivers after the Stop pressed after it never keys. Refused, it is said at each hold and never keys later; taken, it goes to the
//!   held PTT the engine decides (`PttHold`), and each of its reports comes back here for this
//!   computer alone. A hold for a press that will never key (refused there, or ended) is answered
//!   with why, one datagram for each hold, so the one report that said it can be lost. A release
//!   is always taken.
//! - **What a start on the command link needs** is read off [`Ready`]: the presence this road holds,
//!   the snapshot its newest heartbeat named, and the newest Stop of its computer's that either road
//!   has delivered. The road writes it without ever waiting; the connection's command lane reads it,
//!   and notes there the Stops the link delivers.
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_app::mic::{MicFeed, MicStatus};
use tempo_app::remote_control::ptt_hold::{HoldEnd, PttHold};
use tempo_app::remote_control::transmit::TransmitPermit;

use crate::command_dispatch::Refusal;

use super::super::operations::{RoadPresence, RoadStop};
use super::datagram::{self, End, Keys, Tag, EXPORTED};

// What the road carries, its keys' afterlife, the state echo's clock, how fresh a state must be
// and the format's text: the client core's, which both ends read.
#[cfg(test)]
pub(crate) use tempo_client::stop_road::wire::{contract_text, FRESH};
pub(crate) use tempo_client::stop_road::wire::{Down, StateClock, Up, AFTERLIFE};

/// The road's port, past the listener's TCP port: 42077 beside 42075, the audio road's 42076 between.
pub(crate) const PORT_AFTER: u16 = 2;
/// The longest the thread waits for a datagram before it looks at its roads again.
const TICK: Duration = Duration::from_millis(20);
/// Presses remembered as refused per road, so a refused press never keys later.
const REFUSED_KEPT: usize = 64;

// --- What a start on the command link needs ------------------------------------------------------

/// One connection's road as its command lane reads it before a transmission starts from that
/// computer: the presence this road holds now, and the snapshot its newest heartbeat named, judged
/// by the connection's own state clock (the stream's transmit rules: live presence, fresh state).
/// The road's thread writes it and never waits for the lock; the lane reads it, before a start and
/// again once the start has run.
pub(crate) struct Ready {
    presence: Mutex<Option<TransmitPermit>>,
    named: AtomicU64,
    clock: Arc<StateClock>,
    /// The newest Stop of this computer's that either road has delivered, by its number: how many
    /// Stops it had pressed then, that one included. 0 before the first.
    stopped: AtomicU64,
}

impl Ready {
    fn new(clock: Arc<StateClock>) -> Self {
        Self {
            presence: Mutex::new(None),
            named: AtomicU64::new(0),
            clock,
            stopped: AtomicU64::new(0),
        }
    }

    /// Whether this road holds presence that is live now.
    pub(crate) fn presence_live(&self, now: Instant) -> bool {
        self.presence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .is_some_and(|permit| permit.valid(now))
    }

    /// Whether a transmission may start from this computer now, as its road reads it: presence
    /// live, and the state its screen last fetched, as its newest heartbeat named it, taken no more
    /// than [`FRESH`] before now. The refusal of the first that does not hold.
    pub(crate) fn start(&self, now: Instant) -> Result<(), Refusal> {
        if !self.presence_live(now) {
            return Err(Refusal::NoPresence);
        }
        if !self
            .clock
            .fresh_waiting(self.named.load(Ordering::Acquire), now)
        {
            return Err(Refusal::StaleState);
        }
        Ok(())
    }

    /// The Stop numbered `stop` reached the shack from this computer, on either road. Noted before
    /// it retires the transmit generation, never waiting, and only ever raising the number: a late
    /// repeat of an older Stop leaves it as it is. It refuses nothing: a Stop is taken whatever
    /// number it carries.
    pub(crate) fn took_stop(&self, stop: u64) {
        self.stopped.fetch_max(stop, Ordering::SeqCst);
    }

    /// ⛔ STOP WINS, WHICHEVER ROAD IT CAME BY. Whether a start or a press, made when its computer
    /// had pressed `stops` Stops, came before a Stop that has since reached the shack: one the road
    /// delivered ahead of a start still on the command link, or one the link wrote ahead of a start
    /// that waited in its outbox. Then it never runs. Asked once the start's transmit generation is
    /// read, as each Stop is noted before it moves the generation: a Stop is either seen here, or it
    /// voids the start where the start waits.
    pub(crate) fn pressed_before_a_stop(&self, stops: u64) -> bool {
        self.stopped.load(Ordering::SeqCst) > stops
    }
}

// --- The road ------------------------------------------------------------------------------------

/// Everything the road's thread may do besides read and write its own socket: stop the station,
/// renew a computer's presence, hold or release its press, read the microphone over's state, and
/// ask whether a key is still paired. Each never waits for the Engine or for anything slow.
///
/// ⛔ THIS IS ALL IT HOLDS. The thread is built from this, its socket and its roads' keys, and can
/// name no Engine, command link, outbox, audio or runtime handle: that is the receive DSP thread's
/// isolation, applied again, so nothing that can stall them can stall a Stop. A test holds this
/// file's code to it (`road_names_nothing_of_the_station`).
pub(crate) struct Powers {
    pub(crate) stop: RoadStop,
    pub(crate) presence: RoadPresence,
    pub(crate) ptt: PttHold,
    pub(crate) mic: MicFeed,
    /// The device id of the paired computer holding the key with this pin, if one does: what TLS
    /// asks at each handshake. Asked when a road opens and whenever `pairings` moves.
    pub(crate) paired: super::tls::Paired,
    /// How many times who is paired has changed, read without a lock (`Book::revisions`).
    pub(crate) pairings: Arc<dyn Fn() -> u64 + Send + Sync>,
}

/// Who the road hears.
#[derive(Clone, Copy)]
pub(crate) enum Hears {
    /// The listener's own subnet, as its TCP gate does.
    Subnet(tempo_stream::lan::Network),
    /// This computer's own loopback, for a test: no listener ever binds there.
    #[cfg(test)]
    Loopback,
}

impl Hears {
    pub(crate) fn hears(self, ip: IpAddr) -> bool {
        match self {
            Hears::Subnet(network) => network.contains(ip),
            #[cfg(test)]
            Hears::Loopback => ip.is_loopback(),
        }
    }
}

/// One connection's road, as the thread keeps it.
struct Road {
    keys: Keys,
    device: String,
    pin: [u8; 32],
    session: String,
    clock: Arc<StateClock>,
    /// Where this computer last spoke from: where the shack answers.
    peer: Option<SocketAddr>,
    /// Whether its computer is still paired here, as last asked: what its Stop is admitted on.
    paired: bool,
    /// When its connection ended: from then only a Stop is taken, until [`AFTERLIFE`].
    closed: Option<Instant>,
    /// The presence this road last renewed.
    presence: Option<TransmitPermit>,
    /// What its command lane reads of it, and whether that is behind `presence`.
    ready: Arc<Ready>,
    unpublished: bool,
    /// The snapshot its newest heartbeat named.
    named: u64,
    /// The press this road last handed to the held PTT, until its release.
    pressing: Option<u64>,
    /// A renewal minted while the Engine was busy, with its deadline: handed over at the next look.
    pending: Option<(TransmitPermit, Instant)>,
    /// The microphone over as this computer was last told it.
    mic_told: Option<MicStatus>,
    /// Presses refused here: each never keys later.
    refused: VecDeque<u64>,
}

enum Change {
    Open(Tag, Box<Road>),
    Closed(Tag, Instant),
}

/// The road, as the listener and each connection hold it: where it is, and how a connection's road
/// is opened. Cheap to clone.
#[derive(Clone)]
pub(crate) struct StopRoad {
    port: u16,
    changes: mpsc::Sender<Change>,
    tags: Arc<AtomicU64>,
}

/// The road's socket and thread, as the listener holds them until it stops.
pub(crate) struct Running {
    road: StopRoad,
    stopping: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Running {
    pub(crate) fn road(&self) -> StopRoad {
        self.road.clone()
    }

    /// Stops the thread, and with it the socket and every road's keys: within one [`TICK`], so the
    /// caller waits for it off any thread that must not wait.
    pub(crate) fn stop(self) -> std::thread::JoinHandle<()> {
        self.stopping.store(true, Ordering::SeqCst);
        self.thread
    }
}

impl StopRoad {
    /// The road on `at`, or on `at`'s address at a port the system gives when `at`'s is taken, with
    /// its thread. `None` if no socket could be had or no thread started: the listener then runs
    /// without it, and Stop has the command link alone.
    pub(crate) fn start(at: SocketAddrV4, hears: Hears, powers: Powers) -> Option<Running> {
        Self::start_with(at, hears, powers, AFTERLIFE)
    }

    pub(crate) fn start_with(
        at: SocketAddrV4,
        hears: Hears,
        powers: Powers,
        afterlife: Duration,
    ) -> Option<Running> {
        let socket = UdpSocket::bind(at)
            .or_else(|_| UdpSocket::bind(SocketAddrV4::new(*at.ip(), 0)))
            .ok()?;
        socket.set_read_timeout(Some(TICK)).ok()?;
        let port = socket.local_addr().ok()?.port();
        let (changes, changed) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let pairings = (powers.pairings)();
        let thread = {
            let stopping = stopping.clone();
            std::thread::Builder::new()
                .name("nexus-remote-stop-road".into())
                .spawn(move || {
                    Thread {
                        socket,
                        hears,
                        powers,
                        changed,
                        stopping,
                        afterlife,
                        pairings,
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
            road: StopRoad {
                port,
                changes,
                tags: Arc::new(AtomicU64::new(start)),
            },
            stopping,
            thread,
        })
    }

    /// Opens a connection's road, as the shack welcomes it: its keys from `exported` (the
    /// connection's TLS exporter, under `datagram::STOP_LABEL` with `session` as the context), for
    /// `device` holding the key pinned as `pin`, its snapshots timed by `clock`. The road goes when
    /// the returned handle does, but for [`AFTERLIFE`]. `None` once the road has stopped.
    pub(crate) fn open(
        &self,
        exported: &[u8; EXPORTED],
        session: &str,
        device: &str,
        pin: [u8; 32],
        clock: Arc<StateClock>,
    ) -> Option<Opened> {
        let tag = self.tags.fetch_add(1, Ordering::SeqCst).to_be_bytes();
        let ready = Arc::new(Ready::new(clock.clone()));
        let road = Road {
            keys: Keys::new(exported, tag, End::Shack)?,
            device: device.to_string(),
            pin,
            session: session.to_string(),
            clock,
            peer: None,
            paired: false,
            closed: None,
            presence: None,
            ready: ready.clone(),
            unpublished: false,
            named: 0,
            pressing: None,
            pending: None,
            mic_told: None,
            refused: VecDeque::new(),
        };
        self.changes.send(Change::Open(tag, Box::new(road))).ok()?;
        Some(Opened {
            port: self.port,
            tag,
            ready,
            closing: self.changes.clone(),
        })
    }
}

/// A connection's road, open: what its welcome names. Closed when dropped.
pub(crate) struct Opened {
    pub(crate) port: u16,
    pub(crate) tag: Tag,
    ready: Arc<Ready>,
    closing: mpsc::Sender<Change>,
}

impl Opened {
    /// What this connection's command lane reads of its road before a start.
    pub(crate) fn ready(&self) -> Arc<Ready> {
        self.ready.clone()
    }

    /// The road as the welcome names it: `{"port", "tag"}`, the tag as lowercase hex.
    pub(crate) fn offer(&self) -> serde_json::Value {
        let tag: String = self.tag.iter().map(|b| format!("{b:02x}")).collect();
        serde_json::json!({ "port": self.port, "tag": tag })
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.closing.send(Change::Closed(self.tag, Instant::now()));
    }
}

/// A press as the held PTT knows it: this road's tag and the computer's number, so no two roads'
/// presses can be taken for one another. The audio road names a press's frames the same way.
pub(crate) fn press_id(tag: &Tag, press: u64) -> String {
    let tag: String = tag.iter().map(|b| format!("{b:02x}")).collect();
    format!("{PRESS_PREFIX}{tag}-{press:016x}")
}

/// What every press a road names begins with: a page's own presses are never named so.
const PRESS_PREFIX: &str = "road-";

/// Whether the held PTT's press `id` is one a road named ([`press_id`]): its reports go back by
/// that road, and the stream's page is never told of it.
pub(crate) fn is_road_press(id: &str) -> bool {
    id.starts_with(PRESS_PREFIX)
}

/// The road's tag and the computer's number in a press a road named.
fn road_press_of(id: &str) -> Option<(Tag, u64)> {
    let (tag, press) = id.strip_prefix(PRESS_PREFIX)?.split_once('-')?;
    let tag: Tag = tempo_stream::protocol::hex_bytes(tag)?.try_into().ok()?;
    let press = u64::from_str_radix(press, 16).ok()?;
    (press_id(&tag, press) == id).then_some((tag, press))
}

/// Tells `road`'s command lane the presence the road holds now, unless the lane is reading it this
/// very moment: then at the road's next look. The road's thread never waits for it.
fn publish(road: &mut Road) {
    if !road.unpublished {
        return;
    }
    let held = match road.ready.presence.try_lock() {
        Ok(held) => Some(held),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    };
    if let Some(mut held) = held {
        *held = road.presence.clone();
        road.unpublished = false;
    }
}

/// Hands `road`'s pending renewal to the engine, if the Engine is free: the road then holds it, and
/// its computer is told the deadline. A busy Engine leaves it pending for the next look, as a
/// stream's renewal waits for its next tick; one that lapsed meanwhile goes.
fn install(socket: &UdpSocket, presence: &RoadPresence, road: &mut Road, now: Instant) {
    let Some((permit, until)) = road.pending.take() else {
        return;
    };
    if !permit.valid(now) {
        return;
    }
    match presence.install(&permit, now) {
        None => road.pending = Some((permit, until)),
        Some(false) => {}
        Some(true) => {
            road.presence = Some(permit);
            road.unpublished = true;
            publish(road);
            let left = until.saturating_duration_since(now).as_millis();
            let left_ms = u32::try_from(left).unwrap_or(u32::MAX);
            answer(socket, road, &Down::Presence { left_ms });
            // And the microphone over's state again, at this look or the next: a computer that
            // lost the one datagram saying its over ended hears it within a renewal.
            road.mic_told = None;
        }
    }
}

/// `down`, sealed under `road`'s keys, to where its computer last spoke from.
fn answer(socket: &UdpSocket, road: &Road, down: &Down) {
    if let (Some(peer), Some(datagram)) =
        (road.peer, road.keys.send.seal(down.kind(), &down.body()))
    {
        let _ = socket.send_to(&datagram, peer);
    }
}

struct Thread {
    socket: UdpSocket,
    hears: Hears,
    powers: Powers,
    changed: mpsc::Receiver<Change>,
    stopping: Arc<AtomicBool>,
    afterlife: Duration,
    /// `Powers::pairings` when the roads' computers were last asked about.
    pairings: u64,
    roads: HashMap<Tag, Road>,
}

impl Thread {
    fn run(mut self) {
        let mut buffer = [0u8; 2048];
        let mut looked = Instant::now();
        while !self.stopping.load(Ordering::SeqCst) {
            let received = self.socket.recv_from(&mut buffer);
            // A road opened before its welcome went out is in the table before any datagram its
            // computer sent after reading that welcome is read.
            if !self.take_changes() {
                break;
            }
            self.ask_who_is_paired();
            match received {
                Ok((length, from)) => self.datagram(&buffer[..length], from, Instant::now()),
                Err(error) if tempo_stream::session::receive_ends_session(&error) => {
                    tempo_core::applog::warn(
                        "remote",
                        &format!("on this network: Stop's second road stopped ({error})"),
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
                    let mut road = *road;
                    road.paired =
                        (self.powers.paired)(&road.pin).as_deref() == Some(road.device.as_str());
                    self.roads.entry(tag).or_insert(road);
                }
                Ok(Change::Closed(tag, at)) => {
                    if let Some(road) = self.roads.get_mut(&tag) {
                        road.closed.get_or_insert(at);
                        road.presence = None;
                        road.pending = None;
                        road.unpublished = true;
                        publish(road);
                    }
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
        let Some(tag) = datagram::tag_of(bytes, datagram::MAX) else {
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
            Up::Stop { stop } => {
                // Its keys say whose it is; being paired here is station control on this road, read
                // off the road, so a Stop takes no lock. No other question: not the lease, its age,
                // presence, a rate, or the connection.
                if !road.paired {
                    return;
                }
                // Noted before the generation moves, where the command lane reads it: a start or a
                // press its computer made before this Stop is refused when it arrives.
                road.ready.took_stop(stop);
                self.powers.stop.stop();
                answer(&self.socket, road, &Down::Stopped { stop });
            }
            Up::Hold { press, stops } => {
                if road.closed.is_some() {
                    return;
                }
                // ⛔ A PRESS IS A TRANSMISSION START, under the stream's transmit rules: the held
                // press wired from afar, this computer's presence live on this road (which only its
                // own lease mints), and, for a press new to this road, the state its screen last
                // fetched under two seconds old and no Stop of its computer's taken since it began. Said at each hold, one datagram for each, and a
                // press refused never keys later. A press whose freshness cannot be read this moment
                // is neither: its next hold asks again.
                let new = road.pressing != Some(press);
                let fresh = if new {
                    match road.clock.freshness(road.named, now) {
                        Some(fresh) => fresh,
                        None => return,
                    }
                } else {
                    true
                };
                let refused = !crate::command_dispatch::press_from_afar()
                    || !road.presence.as_ref().is_some_and(|p| p.valid(now))
                    || road.refused.contains(&press)
                    || !fresh
                    || (new && road.ready.pressed_before_a_stop(stops));
                if refused {
                    if !road.refused.contains(&press) {
                        if road.refused.len() >= REFUSED_KEPT {
                            road.refused.pop_front();
                        }
                        road.refused.push_back(press);
                    }
                    let said = Down::Press {
                        press,
                        keyed: false,
                        end: Some(HoldEnd::Refused),
                    };
                    answer(&self.socket, road, &said);
                    return;
                }
                road.pressing = Some(press);
                let id = press_id(&tag, press);
                self.powers.ptt.hold(&id, now);
                // ⛔ A hold for a press that will never key is answered with why, one datagram for
                // each hold: the one report that said it may have been lost, and its computer would
                // go on holding an over that is not on the air, its microphone open and the
                // station's audio muted.
                if let Some(end) = self.powers.ptt.end_of(&id) {
                    let said = Down::Press {
                        press,
                        keyed: false,
                        end: Some(end),
                    };
                    answer(&self.socket, road, &said);
                }
            }
            // The off half: always taken.
            Up::Release { press } => {
                if road.pressing == Some(press) {
                    road.pressing = None;
                }
                self.powers.ptt.release(&press_id(&tag, press));
            }
            Up::Heartbeat { lease, state } => {
                if road.closed.is_some() {
                    return;
                }
                road.named = state;
                road.ready.named.store(state, Ordering::Release);
                if !road.clock.fresh(state, now) {
                    return;
                }
                let minted = self
                    .powers
                    .presence
                    .mint(&road.session, &road.device, &lease, now);
                if minted.is_some() {
                    road.pending = minted;
                }
                install(&self.socket, &self.powers.presence, road, now);
            }
        }
    }

    /// Who is paired, asked of the book again only when it changed (a count read without a lock),
    /// and before any datagram is read: a computer removed at the shack is refused from the next one
    /// on, while a Stop's own handling reads the answer off its road and takes no lock.
    fn ask_who_is_paired(&mut self) {
        let pairings = (self.powers.pairings)();
        if pairings != self.pairings {
            self.pairings = pairings;
            for road in self.roads.values_mut() {
                road.paired =
                    (self.powers.paired)(&road.pin).as_deref() == Some(road.device.as_str());
            }
        }
    }

    /// Every [`TICK`]: the roads past their afterlife go with their keys, a renewal the Engine was
    /// too busy for is handed over, each lane is told the presence its road holds, each press's
    /// report goes to the computer whose press it is, and each computer whose presence is live is
    /// told the microphone over's state when it changed, and again after each renewal.
    fn look(&mut self, now: Instant) {
        let afterlife = self.afterlife;
        self.roads.retain(|_, road| {
            road.closed
                .is_none_or(|at| now.saturating_duration_since(at) < afterlife)
        });
        if !self.roads.is_empty() {
            // Only the roads' own presses: the stream's page takes its own.
            for report in self.powers.ptt.reports_where(is_road_press) {
                let Some((tag, press)) = road_press_of(&report.hold_id) else {
                    continue;
                };
                if let Some(road) = self.roads.get(&tag) {
                    let said = Down::Press {
                        press,
                        keyed: report.keyed,
                        end: report.end,
                    };
                    answer(&self.socket, road, &said);
                }
            }
        }
        // Read only while someone may be told, and once a look.
        let mut status = None;
        for road in self.roads.values_mut() {
            install(&self.socket, &self.powers.presence, road, now);
            publish(road);
            let live = road.presence.as_ref().is_some_and(|p| p.valid(now));
            if !live || road.peer.is_none() {
                continue;
            }
            let status = *status.get_or_insert_with(|| self.powers.mic.status());
            if road.mic_told != Some(status) {
                road.mic_told = Some(status);
                answer(&self.socket, road, &Down::Mic(status));
            }
        }
    }
}
