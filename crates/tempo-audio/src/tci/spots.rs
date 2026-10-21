//! Nexus's spots on the SDR program's own panadapter, and the SDR operator's clicks on them: the
//! radio's "Spots on the SDR panadapter" (`RadioProfile::tci_spots`, off as it comes).
//!
//! ## What the operator sees
//! With the switch on, the stations the Needed board lists with a frequency (cluster, skimmer and
//! the radio's own decodes) appear on the SDR program's panadapter, each in the colour of its need
//! (the board's own), the best first and at most [`TCI_SPOTS_MAX`] at once. A spot goes once its
//! station has not been heard for [`MAX_AGE_SECS`], and every spot goes when the switch goes off,
//! when the session closes, and when the radio stops being the one in use. A click on one of them
//! in the SDR program tunes Nexus to it and does nothing else: `Engine::tci_spot_clicked` moves
//! the dial, and refuses while anything is on the air. A click on anything else (another
//! program's spot, a spot on the B receiver, one on another transceiver's panorama) is logged and
//! ignored.
//!
//! ## The pieces, and the thread each runs on
//! - **The radio loop** calls [`tick`] every tick with the active radio's TCI client, if it has
//!   one: it offers the engine's spots ([`TciDaemon::offer_spots`], a store under a lock) and hands
//!   each click kept for it to `Engine::tci_spot_clicked`, under the engine lock. It never waits
//!   on the session.
//! - **The spots' own thread** ("tci-spots", started the first time the switch is on) puts the
//!   offer on the panadapter through the session, one command at a time ([`plan`]), and takes it
//!   off again. A command waits on the session's thread for one read at most.
//! - **The client's watcher** hands each click the session reports ([`Shared::clicked`]); a click
//!   is kept for the loop only when it is on one of ours.
//!
//! ## The lease
//! The radio loop renews its offer every tick. An offer it has not renewed for [`LEASE`] lapses
//! and the spots come off: a client a radio switch left to the monitor, whose session lives on,
//! shows nothing it no longer answers for, and its clicks are not taken.
//!
//! ## On the wire (TCI 2.0, §4.3)
//! `spot:<call>,<mode>,<hz>,<argb>,<text>;` puts one up, `spot_delete:<call>;` takes it down, and
//! `spot_clear;` takes down every spot: at connect (once the switch is on: what an earlier session
//! left is not known) and at close. A spot that changes is put up again, which replaces the one up
//! for the same call. Everything goes out through the codec's types, so nothing from a spot
//! can become a command of its own: a call that is not a callsign is not sent at all, the mode keeps
//! letters and digits, and the text keeps printable characters, the codec escaping its `:`, `,`
//! and `;` (as `^`, `~` and `*`, which are dropped from the text itself).
//!
//! ## Per server
//! The colour is ARGB, opaque, as the specification writes it (a decimal number). ExpertSDR3,
//! ExpertSDR2 and Thetis take spots and send clicks; piHPSDR's TCI has no spot commands, so its
//! panadapter shows none and sends no clicks. Nexus's own design, not a port.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempo_app::engine::tci_spots::{PanadapterSpot, PanadapterSpots};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::settings::TCI_SPOTS_MAX;
use tempo_net::tci::session::Connection;
use tempo_net::tci::wire::{Call, Command, Hz, Text, Token};
use tempo_net::tci::Channel;

use super::TciDaemon;

/// A spot comes off the panadapter once its station has not been heard for this long: the window
/// the Needed board reads a cluster spot in.
pub const MAX_AGE_SECS: i64 = 900;

/// How long the radio loop's offer lasts. It renews it every tick (20 ms); a CAT call that waits
/// on a slow radio holds the loop well under this.
pub const LEASE: Duration = Duration::from_secs(5);

/// How often the spots' thread looks again with nothing to wake it: for the ages and the lease.
const LOOK: Duration = Duration::from_secs(1);

/// How far a click may land from one of ours and still be on it, in Hz.
const CLICK_SLACK_HZ: i64 = 1_000;

/// The most clicks kept for the radio loop: it takes them every tick.
const CLICKS_KEPT: usize = 4;

/// A click the radio loop has not taken this long after it came is dropped: the loop was not on
/// this radio (a switch it had not handed over), and a dial that moves seconds after the click
/// would surprise the operator.
pub const CLICK_KEPT_FOR: Duration = Duration::from_secs(2);

/// The longest call put on the panadapter: the longest a click may name
/// (`Engine::tci_spot_clicked`), so every spot up is one a click can tune to.
const CALL_MAX: usize = 20;

/// The longest mode label and text sent with a spot.
const MODE_MAX: usize = 8;
const TEXT_MAX: usize = 64;

/// The words for a click that is not taken, for the connection log.
const CLICK_ON_B: &str = "it is on the B receiver; Nexus tunes only its main dial";
const CLICK_SPOTS_OFF: &str = "Nexus's spots are not on this radio's panadapter";
const CLICK_NOT_OURS: &str = "it is not one of Nexus's spots";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Seconds since the Unix epoch, as a spot's `heard_unix` counts them.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// One spot as it goes on the wire: each part a value only the codec renders.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Up {
    call: Call,
    mode: Token,
    hz: Hz,
    argb: u32,
    text: Text,
}

impl Up {
    /// The spot the station chose, as it may go out; `None` for one that cannot be a spot here (a
    /// call that is not a callsign, a frequency no radio has).
    fn of(spot: &PanadapterSpot) -> Option<Up> {
        let call = spot.call.trim().to_ascii_uppercase();
        if call.len() > CALL_MAX || !tempo_core::message::looks_like_call(&call) {
            return None;
        }
        Some(Up {
            call: Call::new(&call)?,
            mode: mode_label(&spot.mode),
            hz: Hz::new(spot.hz).filter(|hz| hz.get() > 0)?,
            argb: spot.argb,
            text: plain_text(&spot.text),
        })
    }

    fn command(&self) -> Command {
        Command::Spot {
            call: self.call.clone(),
            mode: self.mode.clone(),
            hz: self.hz,
            argb: self.argb,
            text: self.text.clone(),
        }
    }
}

/// The mode as the panadapter labels it: its letters and digits, upper case; "DATA" for one that
/// has none.
fn mode_label(mode: &str) -> Token {
    let word: String = mode
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(MODE_MAX)
        .collect::<String>()
        .to_ascii_uppercase();
    Token::new(&word)
        .or_else(|| Token::new("DATA"))
        .expect("letters and digits are a token")
}

/// The text as it may go out: printable characters only, without the three the codec uses to
/// escape (`^`, `~`, `*`), at most [`TEXT_MAX`]. The codec escapes its `:`, `,` and `;`.
fn plain_text(text: &str) -> Text {
    let kept: String = text
        .chars()
        .filter(|c| matches!(c, ' '..='~') && !matches!(c, '^' | '~' | '*'))
        .take(TEXT_MAX)
        .collect();
    Text::new(kept.trim()).unwrap_or_else(|| Text::new("").expect("no text is text"))
}

/// The spots that should be up now: those that can be sent and were heard within [`MAX_AGE_SECS`],
/// one per call (the first, the best), at most [`TCI_SPOTS_MAX`].
fn wanted(spots: &[PanadapterSpot], now_unix: i64) -> Vec<Up> {
    let mut up: Vec<Up> = Vec::new();
    for spot in spots {
        if up.len() >= TCI_SPOTS_MAX {
            break;
        }
        if now_unix.saturating_sub(spot.heard_unix) > MAX_AGE_SECS {
            continue;
        }
        let Some(u) = Up::of(spot) else {
            continue;
        };
        if !up.iter().any(|o| o.call == u.call) {
            up.push(u);
        }
    }
    up
}

/// One of ours on the panadapter, as it went out.
#[derive(Debug, Clone)]
struct Shown {
    up: Up,
}

/// What the spots' thread sends next, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    /// `spot_clear;`
    Clear,
    /// `spot_delete:<call>;`
    Delete(Call),
    /// `spot:…;`
    Put(Up),
}

impl Op {
    fn command(&self) -> Command {
        match self {
            Op::Clear => Command::SpotClear,
            Op::Delete(call) => Command::SpotDelete(call.clone()),
            Op::Put(up) => up.command(),
        }
    }
}

/// What the spots know: the radio loop's offer, ours on the panadapter, the clicks kept.
#[derive(Default)]
struct State {
    /// The radio loop's last offer (`None`: the switch is off), and when it last renewed it.
    offer: Option<PanadapterSpots>,
    renewed: Option<Instant>,
    /// Ours on the panadapter, by call.
    shown: BTreeMap<String, Shown>,
    /// `spot_clear;` has gone out since the spots were last turned on: nothing an earlier session
    /// put up is left.
    cleared: bool,
    /// The clicks kept for the radio loop: the call, the frequency clicked, and when.
    clicks: VecDeque<(String, i64, Instant)>,
    /// The client is closing: the thread sends nothing more.
    stopping: bool,
}

impl State {
    /// The offer stands: made, and renewed within the [`LEASE`].
    fn live(&self, now: Instant) -> bool {
        self.offer.is_some()
            && self
                .renewed
                .is_some_and(|at| now.saturating_duration_since(at) < LEASE)
    }

    /// Ours on the panadapter is as `op` left it, sent at `now`.
    fn sent(&mut self, op: &Op, now: Instant) {
        match op {
            Op::Clear => {
                self.shown.clear();
                self.cleared = self.live(now);
            }
            Op::Delete(call) => {
                self.shown.remove(call.as_str());
            }
            Op::Put(up) => {
                self.shown
                    .insert(up.call.as_str().to_string(), Shown { up: up.clone() });
            }
        }
    }
}

/// What to send to make the panadapter what the offer says, at `now` (`now_unix` on the wall
/// clock), in order: a lapsed or withdrawn offer clears ours; the first live offer clears first
/// (whatever an earlier session left), then puts up every spot; after that only the spots that
/// went (deleted first, so the panadapter never holds more than [`TCI_SPOTS_MAX`] of ours) or
/// changed are sent.
fn plan(s: &State, now: Instant, now_unix: i64) -> Vec<Op> {
    let Some(offer) = s.offer.as_ref().filter(|_| s.live(now)) else {
        return if s.cleared || !s.shown.is_empty() {
            vec![Op::Clear]
        } else {
            Vec::new()
        };
    };
    let want = wanted(&offer.spots, now_unix);
    if !s.cleared {
        return std::iter::once(Op::Clear)
            .chain(want.into_iter().map(Op::Put))
            .collect();
    }
    let mut ops: Vec<Op> = s
        .shown
        .values()
        .filter(|shown| !want.iter().any(|u| u.call == shown.up.call))
        .map(|shown| Op::Delete(shown.up.call.clone()))
        .collect();
    ops.extend(want.into_iter().filter_map(|u| {
        let due = s
            .shown
            .get(u.call.as_str())
            .is_none_or(|shown| shown.up != u);
        due.then_some(Op::Put(u))
    }));
    ops
}

/// The spots of one TCI client: shared by the radio loop, the client's watcher and the spots'
/// thread.
#[derive(Default)]
pub(crate) struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Shared {
    /// The radio loop's offer, renewed. Wakes the thread when the offer changed, or stood again
    /// after a lapse.
    fn offer(&self, offer: Option<PanadapterSpots>) {
        let now = Instant::now();
        let mut s = lock(&self.state);
        let was_live = s.live(now);
        let changed = match (&s.offer, &offer) {
            (None, None) => false,
            (Some(a), Some(b)) => a.generation != b.generation,
            _ => true,
        };
        s.offer = offer;
        s.renewed = Some(now);
        if changed || was_live != s.live(now) {
            self.wake.notify_all();
        }
    }

    /// A click the session reported, on this client's transceiver: kept for the radio loop when it
    /// is on one of ours. `Err` says why not.
    pub(crate) fn clicked(
        &self,
        call: &Call,
        hz: Hz,
        channel: Option<Channel>,
    ) -> Result<(), &'static str> {
        if channel.is_some_and(|c| c != Channel::A) {
            return Err(CLICK_ON_B);
        }
        let mut s = lock(&self.state);
        if !s.live(Instant::now()) {
            return Err(CLICK_SPOTS_OFF);
        }
        let ours = s
            .shown
            .get(call.as_str())
            .is_some_and(|shown| (shown.up.hz.get() - hz.get()).abs() <= CLICK_SLACK_HZ);
        if !ours {
            return Err(CLICK_NOT_OURS);
        }
        if s.clicks.len() >= CLICKS_KEPT {
            s.clicks.pop_front();
        }
        s.clicks
            .push_back((call.as_str().to_string(), hz.get(), Instant::now()));
        Ok(())
    }

    /// The next click kept, unless it has waited longer than [`CLICK_KEPT_FOR`].
    fn take_click(&self) -> Option<(String, i64)> {
        let mut s = lock(&self.state);
        while let Some((call, hz, at)) = s.clicks.pop_front() {
            if at.elapsed() <= CLICK_KEPT_FOR {
                return Some((call, hz));
            }
        }
        None
    }

    /// Stop the thread, then take ours off the panadapter in one `spot_clear;` while the session
    /// is still up: the client's close, after its transmitter's stops.
    pub(crate) fn close(&self, conn: &Connection) {
        lock(&self.state).stopping = true;
        self.wake.notify_all();
        let thread = lock(&self.thread).take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        let up = {
            let s = lock(&self.state);
            s.cleared || !s.shown.is_empty()
        };
        if up && conn.send(Command::SpotClear).is_ok() {
            let mut s = lock(&self.state);
            s.shown.clear();
            s.cleared = false;
        }
    }
}

/// The spots' thread: plans, sends, and records what went out, until the client closes or its
/// session goes. A command the session would not send (it is closing, or gone) ends the pass; the
/// next is tried a [`LOOK`] later.
fn put_up(shared: &Shared, conn: &Weak<Connection>, server: SocketAddr) {
    let mut not_before: Option<Instant> = None;
    loop {
        let ops = {
            let mut s = lock(&shared.state);
            loop {
                if s.stopping {
                    return;
                }
                let now = Instant::now();
                if not_before.is_none_or(|t| now >= t) {
                    let ops = plan(&s, now, unix_now());
                    if !ops.is_empty() {
                        break ops;
                    }
                }
                s = shared
                    .wake
                    .wait_timeout(s, LOOK)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        };
        let Some(conn) = conn.upgrade() else {
            return;
        };
        not_before = None;
        for op in ops {
            if lock(&shared.state).stopping {
                return;
            }
            if let Err(e) = conn.send(op.command()) {
                tempo_core::applog::info(
                    "cat",
                    &format!("TCI ({server}): spots not sent to the panadapter ({e:?})"),
                );
                not_before = Some(Instant::now() + LOOK);
                break;
            }
            lock(&shared.state).sent(&op, Instant::now());
        }
    }
}

impl TciDaemon {
    /// The radio loop's offer of Nexus's spots for this client's panadapter (`None`: the switch is
    /// off), renewed every tick ([`tick`]). The spots' thread starts the first time one is made.
    pub fn offer_spots(&self, offer: Option<PanadapterSpots>) {
        let spots = &self.watch.spots;
        let start = offer.is_some() && lock(&spots.thread).is_none();
        spots.offer(offer);
        if start {
            let shared = spots.clone();
            let conn = Arc::downgrade(self.conn.as_ref().expect("held until drop"));
            let server = self.server;
            let spawned = std::thread::Builder::new()
                .name("tci-spots".into())
                .spawn(move || put_up(&shared, &conn, server));
            match spawned {
                Ok(thread) => *lock(&spots.thread) = Some(thread),
                Err(e) => tempo_core::applog::warn(
                    "cat",
                    &format!("TCI ({}): spots not started ({e})", self.server),
                ),
            }
        }
    }

    /// The next click on one of ours that the radio loop has not taken: the call, and the
    /// frequency clicked.
    pub fn take_spot_click(&self) -> Option<(String, i64)> {
        self.watch.spots.take_click()
    }
}

/// One radio-loop tick for the spots of the radio in use: offer the engine's spots to its TCI
/// client, and hand each click on them to the engine, which only tunes. `daemon` is that client,
/// or `None` (another connection, or a radio switch the loop has not handed over).
pub fn tick(daemon: Option<&TciDaemon>, engine: &Arc<Mutex<Engine>>) {
    let Some(daemon) = daemon else {
        return;
    };
    let offer = {
        let eng = engine_lock(engine);
        eng.tci_spots_on().then(|| eng.tci_spots())
    };
    daemon.offer_spots(offer);
    while let Some((call, hz)) = daemon.take_spot_click() {
        let tuned = engine_lock(engine).tci_spot_clicked(&call, hz);
        if let Err(why) = tuned {
            tempo_core::applog::info(
                "cat",
                &format!("TCI ({}): {call} at {hz} Hz, {why}", daemon.server()),
            );
        }
    }
}

/// The words the watcher logs for a click: tuning to it, or why not.
pub(crate) fn click_words(call: &Call, hz: Hz, taken: Result<(), &str>) -> String {
    match taken {
        Ok(()) => format!(
            "spot {} at {} Hz clicked on the SDR program's panadapter; Nexus tunes to it",
            call.as_str(),
            hz.get()
        ),
        Err(why) => format!(
            "spot {} at {} Hz clicked on the SDR program's panadapter, ignored: {why}",
            call.as_str(),
            hz.get()
        ),
    }
}

#[cfg(test)]
mod tests;
