//! The dispatcher for another computer that drives this station: what that computer's Nexus runs
//! itself, what it sends here, and what the shack admits, each by the command's class in
//! `command_classes`. "Here", in this file as in the table, is the computer the operator sits at;
//! "the shack" is the one with the radio. Nothing outside the tests calls it yet: the command link
//! that carries each call between the two computers comes next, and hands every call it reads to
//! [`Dispatcher::admit`], and the other computer's handler asks [`route`] before it sends one.
//!
//! ## Deny by default, at both ends
//!
//! The other computer routes by [`route`]: a command classed "runs here" runs on that computer, a
//! station command goes to the shack, the ui-state pair splits by key, and a command with no class,
//! or one only the shack's own screen may give, is refused there and never sent. The shack does not
//! trust that routing. [`Dispatcher::admit`] looks the class up again and refuses a command with no
//! class, one that runs on the other computer, one that is refused from afar and, until the stream's
//! transmit rules are wired for this road, every command that starts a transmission.
//!
//! ## Stop is never refused
//!
//! A command that stops a transmission, and the off half of a transmit switch, is admitted before
//! anything else is looked at (the operator's ruling of 2026-10-07: "Never refused"). No lease, cap,
//! rate, journal or revision applies to it, and a repeat runs again: an unkey a guard can swallow is
//! a stuck transmitter. The off half is read off the command's own on/off argument ([`OFF_HALVES`]),
//! so the table keeps one class per command, and the dispatcher never takes a lock to admit it.
//!
//! ## Everything else
//!
//! - **The lease.** Only the computer holding station control runs anything else.
//! - **At most once.** Every call carries an id, rising, from one run of Nexus on the other
//!   computer. The journal answers a repeated id with what became of it and never runs it twice,
//!   and after a reconnect it says what became of each id ([`Dispatcher::outcomes`]). It keeps the
//!   newest [`JOURNAL`] outcomes for [`RESULT_AGE`]; an id older than those is refused, never run.
//! - **Caps.** [`IN_FLIGHT`] calls at once from one computer, [`PER_COMMAND`] of them one command,
//!   and [`PER_SECOND`] a second. The other computer holds its own calls back to stay under them.
//! - **A stale whole save is refused** (the operator's ruling of 2026-10-05). A command that replaces
//!   a store whole ([`STORES`]) names the revision of the copy it was made from, and is refused,
//!   "Changed at the shack. Reload and save again.", unless that is the store's revision now. A read
//!   of the store, and an accepted save, answer with the revision the other computer's copy now has.
//! - **The one mixed store**, `ui-state.json`, splits by key: the station's data ([`STATION_KEYS`])
//!   lives at the shack, every other key on the computer the operator sits at.
//! - **Events.** `pounce` is the one event the shack sends on, and the only one the other computer
//!   takes ([`FORWARDED_EVENTS`]).
//!
//! The operator's other rulings of 2026-10-07 live in the table and in [`argument_refusal`]: a whole
//! Settings save from afar saves every field, as the stream window's does; a settings restore, a
//! factory reset, a whole-log purge and the Connections log stay at the shack; the rig's VOX switch
//! runs at the shack, VOX on included; and a Winlink session runs at the shack with the password
//! stored there, a password typed on the other computer refused at both ends.
//!
//! ## What the revisions do not catch
//!
//! A whole save is checked when it is admitted, and the save itself takes the Engine lock again
//! when it runs, so a save made at the shack's own screen in the moment between the two is not seen;
//! nor is one between an accepted save and the revision it answers with, which is read after it. A
//! read answers with the revision taken before it ran, so a change landing during the read can only
//! cost a needless refusal later. Closing the two windows needs each save to check the revision under
//! its own lock, which changes the save commands themselves.

use crate::command_classes::{class_of, Class};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// Calls one run of Nexus on another computer may have running here at once: the design's eight.
/// The shack runs them on the same Engine lock as its own screen and its radio loop, so a flood
/// must not stack waiters until the runtime's pool is gone.
pub(crate) const IN_FLIGHT: usize = 8;
/// Of those, calls of any one command, so one poller cannot hold every slot.
pub(crate) const PER_COMMAND: usize = 4;
/// Calls one run of Nexus may make in a second. The desktop polls its waterfall up to once a frame,
/// so this leaves room for that, the snapshot and the rest.
pub(crate) const PER_SECOND: u32 = 200;
/// Outcomes kept for one run of Nexus. A reconnect asks after the calls in flight when the link
/// dropped, the newest few, so this is room to spare.
pub(crate) const JOURNAL: usize = 1024;
/// How long an outcome is kept: as long as the hosted road keeps a receipt.
pub(crate) const RESULT_AGE: Duration = Duration::from_secs(600);
/// Runs of Nexus, across all paired computers, whose journals are kept at once.
const CALLERS: usize = 16;
/// The longest command error a journal keeps, in bytes.
const ERROR_BYTES: usize = 512;

/// Why a call was refused. Its [`code`](Refusal::code) is what the other computer is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The command has no class. Refused at both ends.
    Unclassed,
    /// The command runs on the computer the operator sits at, never at the shack.
    RunsHere,
    /// The command is given only from the shack's own screen.
    ShackOnly,
    /// The command starts a transmission, which another computer may not do until the stream's
    /// transmit rules are wired for this road.
    TransmitNotYet,
    /// A password typed on the other computer: credentials are set and used at the shack.
    TypedPassword,
    /// Another device holds station control.
    NotController,
    /// Over a cap on calls in flight.
    Busy,
    /// Over the cap on calls a second.
    RateLimited,
    /// An id already used, whose outcome is no longer kept. It is never run again.
    Replayed,
    /// A whole save made from a copy older than the shack's, or naming no copy at all.
    ChangedAtShack,
    /// Arguments that are not what the command is invoked with.
    BadArguments,
}

impl Refusal {
    /// The refusal's name on the wire.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Refusal::Unclassed => "unclassed",
            Refusal::RunsHere => "runsHere",
            Refusal::ShackOnly => "shackOnly",
            Refusal::TransmitNotYet => "transmitNotYet",
            Refusal::TypedPassword => "typedPassword",
            Refusal::NotController => "notController",
            Refusal::Busy => "busy",
            Refusal::RateLimited => "rateLimited",
            Refusal::Replayed => "replayed",
            Refusal::ChangedAtShack => "changedAtShack",
            Refusal::BadArguments => "badArguments",
        }
    }
}

// --- The other computer's end ------------------------------------------------------------------

/// Where the computer the operator sits at sends a command its screens make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Run it on this computer.
    Here,
    /// Send it to the shack.
    Shack,
    /// The ui-state pair: the station's keys go to and come from the shack, every other key stays
    /// on this computer ([`split_ui_state`], [`join_ui_state`]).
    Split,
    /// Refuse it on this computer; nothing is sent.
    Refused(Refusal),
}

/// The route for `cmd`, invoked with `args`. A command with no class is refused, as is one only the
/// shack's own screen may give and an argument that may not leave this computer.
pub(crate) fn route(cmd: &str, args: &Value) -> Route {
    let Some(class) = class_of(cmd) else {
        return Route::Refused(Refusal::Unclassed);
    };
    match class {
        Class::RunsHere => Route::Here,
        Class::RefusedHere => Route::Refused(Refusal::ShackOnly),
        Class::RunsAtShack | Class::StartsTx | Class::StopsTx => {
            if let Some(refusal) = argument_refusal(cmd, args) {
                Route::Refused(refusal)
            } else if UI_STATE.contains(&cmd) {
                Route::Split
            } else {
                Route::Shack
            }
        }
    }
}

/// A refusal read off a command's arguments, made at both ends so that what it refuses never
/// leaves the other computer. One today: `winlink_connect` with a password typed on the other
/// computer (the operator's ruling of 2026-10-07, "Shack's stored password"). An empty password is
/// the command's own "use the one in the keychain", so the session still runs at the shack with the
/// password stored there; anything else is refused. The value is only tested for being empty:
/// never kept, logged, compared or echoed, and the refusal carries none of it.
pub(crate) fn argument_refusal(cmd: &str, args: &Value) -> Option<Refusal> {
    match cmd {
        "winlink_connect" => match args.get("password") {
            Some(Value::String(typed)) if typed.is_empty() => None,
            _ => Some(Refusal::TypedPassword),
        },
        _ => None,
    }
}

// --- Stop's rules --------------------------------------------------------------------------------

/// How the off half of a transmit switch reads in its argument.
#[derive(Clone, Copy, Debug)]
enum Off {
    /// A `bool` argument that is `false`.
    False,
    /// A string argument naming one of these values.
    OneOf(&'static [&'static str]),
}

/// The off half of each transmit switch: the command, the argument the app invokes it with that
/// says on or off, and how "off" reads in it. The off half runs under Stop's rules; anything else in
/// that argument, a missing one included, is the on half, under the command's class. A test holds
/// each argument to the command's own signature, and fails on a transmit command with an on/off
/// argument and no row here.
const OFF_HALVES: &[(&str, &str, Off)] = &[
    ("aprs_arm", "on", Off::False),
    ("js8_arm", "on", Off::False),
    ("js8_cq_repeat", "on", Off::False),
    ("psk_set_latched", "on", Off::False),
    // It runs at the shack either way: turning the sequencer on keys nothing, but turning it off
    // ends a live session, so the off half is a stop.
    ("rtty_set_auto", "on", Off::False),
    ("rtty_set_latched", "on", Off::False),
    ("set_beacon", "on", Off::False),
    ("set_chat_cq", "on", Off::False),
    // A run mode ("qso-run", "fieldday-run") calls CQ; these three are passive on entry, and
    // entering one ends a run and drops the queue.
    (
        "set_mode",
        "mode",
        Off::OneOf(&["chat", "qso-monitor", "fieldday-sp"]),
    ),
    ("set_ptt", "on", Off::False),
    ("set_tune", "on", Off::False),
    ("set_tx_enabled", "enabled", Off::False),
];

/// Whether a call runs under Stop's rules: its command stops a transmission, or it is the off half
/// of a transmit switch.
fn runs_as_stop(class: Option<Class>, cmd: &str, args: &Value) -> bool {
    class == Some(Class::StopsTx)
        || OFF_HALVES.iter().any(|(name, arg, off)| {
            *name == cmd
                && match (off, args.get(*arg)) {
                    (Off::False, Some(Value::Bool(false))) => true,
                    (Off::OneOf(values), Some(Value::String(value))) => {
                        values.contains(&value.as_str())
                    }
                    _ => false,
                }
        })
}

// --- Stores and their revisions ------------------------------------------------------------------

/// A store a whole save replaces, and whose reads answer with its revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store {
    /// The shack's settings.
    Settings,
    /// The station's keys of `ui-state.json` ([`STATION_KEYS`]).
    UiState,
}

/// How a command meets its store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    /// It reads the store whole.
    Read,
    /// It replaces the store, or a part of it, whole: the part it sends is what the other
    /// computer's copy held, so a copy older than the shack's would put back what changed since.
    WholeSave,
}

/// The commands that read a store whole or replace it whole. The list saves each replace one part
/// of the settings with a list the screen took from them.
const STORES: &[(&str, Store, Access)] = &[
    ("get_settings", Store::Settings, Access::Read),
    ("set_blocked_calls", Store::Settings, Access::WholeSave),
    ("set_psk_macros", Store::Settings, Access::WholeSave),
    ("set_radio_bands", Store::Settings, Access::WholeSave),
    ("set_routing_rules", Store::Settings, Access::WholeSave),
    ("set_rtty_macros", Store::Settings, Access::WholeSave),
    ("set_settings", Store::Settings, Access::WholeSave),
    ("ui_state_load", Store::UiState, Access::Read),
    ("ui_state_save", Store::UiState, Access::WholeSave),
    ("update_radio_profile", Store::Settings, Access::WholeSave),
];

/// The ui-state pair, which splits by key at both ends.
const UI_STATE: &[&str] = &["ui_state_load", "ui_state_save"];

/// A store's revision: the SHA-256 of its JSON, as the hosted road's Settings revision is. Two
/// copies have one revision exactly when their contents are the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Revision(String);

impl Revision {
    /// A revision as the other computer sent it back.
    pub(crate) fn new(text: String) -> Self {
        Self(text)
    }

    /// The revision as it goes on the wire.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The revision of `value`'s JSON. `None` when it does not serialize, which refuses a whole
    /// save rather than letting every copy match.
    fn of(value: &impl serde::Serialize) -> Option<Self> {
        let bytes = serde_json::to_vec(value).ok()?;
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        Some(Self(
            digest.as_ref().iter().map(|b| format!("{b:02x}")).collect(),
        ))
    }
}

/// What the dispatcher reads of the shack's stores. [`ShackStores`] is the shack's own.
pub(crate) trait Stores: Send + Sync + 'static {
    /// The store's revision now, or `None` when it cannot be read, which refuses a whole save.
    fn revision(&self, store: Store) -> Option<Revision>;
    /// The whole of `ui-state.json` now.
    fn ui_state(&self) -> BTreeMap<String, String>;
}

/// The shack's stores: its Engine's settings, and its `ui-state.json`.
pub(crate) struct ShackStores {
    pub(crate) engine: crate::SharedEngine,
}

impl Stores for ShackStores {
    fn revision(&self, store: Store) -> Option<Revision> {
        match store {
            // The Engine's own copy, every field of it. A field a read does not show (the Cloudlog
            // key waiting for the keychain) can only cost a needless refusal, never a missed change.
            Store::Settings => Revision::of(crate::engine_lock(&self.engine).settings()),
            Store::UiState => Revision::of(&split_ui_state(self.ui_state()).0),
        }
    }

    fn ui_state(&self) -> BTreeMap<String, String> {
        crate::ui_state_load()
    }
}

// --- The one mixed store -------------------------------------------------------------------------

/// The keys of `ui-state.json` that hold the station's own data: its memory channels, its watch
/// list, the satellite and DXpedition chase sets and their alarms, and its settings profiles. They
/// live at the shack. Every other key (the UI scale's cap, the rail's order, which modes the rail
/// shows, the main window's pane layout) belongs to the computer the operator sits at and stays
/// there. This is `DURABLE_KEYS` in `ui/src/features/durableStore.ts` split in two, and a test fails
/// on a durable key that is in neither half.
pub(crate) const STATION_KEYS: &[&str] = &[
    "nexus.dxped.alarms",
    "nexus.dxped.alarms.fired",
    "nexus.dxped.chasing",
    "nexus.memory.bank.v1",
    "nexus.memory.bank.v2",
    "nexus.profiles",
    "nexus.sats.alarms",
    "nexus.sats.alarms.fired",
    "nexus.sats.chasing",
    "nexus.watchlist",
];

/// A ui-state map split in two: the station's keys, and every other key.
pub(crate) fn split_ui_state(
    map: BTreeMap<String, String>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    map.into_iter()
        .partition(|(key, _)| STATION_KEYS.contains(&key.as_str()))
}

/// One map from two: the station's keys from `station`, every other key from `rest`. The other
/// computer's screen loads the shack's station keys joined to its own other keys, so a station key
/// in its own store, an old copy, gives way; and a save from afar writes the station keys it sent
/// joined to the shack's own other keys.
pub(crate) fn join_ui_state(
    station: BTreeMap<String, String>,
    rest: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut joined = split_ui_state(rest).1;
    joined.extend(split_ui_state(station).0);
    joined
}

/// The station's part of a `ui_state_load` answer, which is all the shack sends; a malformed answer
/// sends nothing.
fn station_answer(value: Value) -> Value {
    let map: BTreeMap<String, String> = serde_json::from_value(value).unwrap_or_default();
    serde_json::to_value(split_ui_state(map).0)
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

/// What a `ui_state_save` from afar writes at the shack: the shack's own keys as they are, and the
/// station's keys as the other computer sent them, so a key it removed is removed. `None` when the
/// arguments carry no map of strings.
fn merged_save(shack: BTreeMap<String, String>, args: &Value) -> Option<Value> {
    let sent: BTreeMap<String, String> = serde_json::from_value(args.get("state")?.clone()).ok()?;
    Some(serde_json::json!({ "state": join_ui_state(sent, shack) }))
}

// --- Events --------------------------------------------------------------------------------------

/// The shack's events that another computer is sent, and the only ones it takes in: `pounce`, the
/// alert the pounce detector raises, the app's one push (everything else polls). Any other event
/// stays where it was raised.
pub(crate) const FORWARDED_EVENTS: &[&str] = &["pounce"];

/// Whether the event `name` crosses between the two computers.
pub(crate) fn forwards(name: &str) -> bool {
    FORWARDED_EVENTS.contains(&name)
}

// --- The shack's end -----------------------------------------------------------------------------

/// One run of Nexus on one paired computer. Its journal follows it across a reconnect; a new run
/// starts a journal of its own.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CallerKey {
    /// The paired computer, as the shack names it from its key.
    pub(crate) device: String,
    /// That computer's run of Nexus, as its hello names it.
    pub(crate) run: u64,
}

/// Who is calling, and whether it holds the station's controller lease now.
#[derive(Clone, Debug)]
pub(crate) struct Caller {
    pub(crate) key: CallerKey,
    pub(crate) holds_lease: bool,
}

/// One call from the other computer. Deliberately not `Debug`: its arguments can hold anything the
/// screen sent, a typed password among them.
pub(crate) struct Call {
    /// Rising, within one run of Nexus.
    pub(crate) id: u64,
    pub(crate) cmd: String,
    /// The arguments as the app invokes the command with them: a JSON object.
    pub(crate) args: Value,
    /// For a whole save: the revision of the copy it was made from.
    pub(crate) base: Option<Revision>,
}

/// What became of a call, as the journal keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Admitted, and not finished yet.
    Running,
    /// It ran, and succeeded.
    Done,
    /// It ran, and failed with the command's own error.
    Failed(String),
    /// It was refused, and nothing ran.
    Refused(Refusal),
    /// It was admitted, and its result never came back.
    Lost,
    /// Nothing is known of it: never seen, no longer kept, or the shack restarted since. The
    /// screen says to check, and never guesses.
    Unknown,
}

/// The dispatcher's answer to a call.
pub(crate) enum Admission<S: Stores> {
    /// Run it at once, ahead of anything waiting: a Stop, or the off half of a transmit switch. It
    /// is not journaled, and a repeat runs again.
    Stop { args: Value },
    /// Run it with these arguments, then hand its result to [`Ticket::finish`].
    Run { args: Value, ticket: Ticket<S> },
    /// A repeated id: what became of it. It is not run again.
    Answered(Outcome),
    /// Refused; nothing ran.
    Refused(Refusal),
}

/// What a finished call sends back.
pub(crate) struct Reply {
    pub(crate) result: Result<Value, String>,
    /// The revision of the store the call read or saved: the one the other computer's copy now has.
    pub(crate) revision: Option<Revision>,
}

/// The shack's dispatcher: its stores, and a journal for each run of Nexus calling it.
pub(crate) struct Dispatcher<S: Stores> {
    inner: Arc<Inner<S>>,
}

impl<S: Stores> Clone for Dispatcher<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<S> {
    stores: S,
    journals: Mutex<HashMap<CallerKey, Journal>>,
}

impl<S: Stores> Dispatcher<S> {
    pub(crate) fn new(stores: S) -> Self {
        Self {
            inner: Arc::new(Inner {
                stores,
                journals: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Admit, answer or refuse one call. A Stop is answered without a lock or a store; any other
    /// call can wait on the Engine for a store's revision, so the link admits off its reading task.
    pub(crate) fn admit(&self, caller: &Caller, call: Call) -> Admission<S> {
        self.admit_at(caller, call, Instant::now())
    }

    fn admit_at(&self, caller: &Caller, call: Call, now: Instant) -> Admission<S> {
        let Call {
            id,
            cmd,
            args,
            base,
        } = call;
        let class = class_of(&cmd);
        if runs_as_stop(class, &cmd, &args) {
            return Admission::Stop { args };
        }
        // The journal, the rate, the class's reasons and the caps, in one hold of the lock.
        {
            let mut journals = lock(&self.inner.journals);
            let Some(journal) = journal_for(&mut journals, &caller.key, now) else {
                return Admission::Refused(Refusal::Busy);
            };
            match journal.seen(id) {
                Seen::New => {}
                Seen::Again(outcome) => return Admission::Answered(outcome),
                Seen::Forgotten => return Admission::Refused(Refusal::Replayed),
            }
            let refusal = if journal.over_rate(now) {
                Some(Refusal::RateLimited)
            } else if let Some(refusal) = class_refusal(class, &cmd, &args, caller.holds_lease) {
                Some(refusal)
            } else if journal.over_caps(&cmd) {
                Some(Refusal::Busy)
            } else {
                None
            };
            if let Some(refusal) = refusal {
                journal.record(id, Outcome::Refused(refusal), now);
                return Admission::Refused(refusal);
            }
            journal.running.insert(id, cmd.clone());
        }
        // Outside the lock: a store's revision can wait on the Engine.
        let mut ticket = Ticket {
            dispatcher: self.clone(),
            caller: caller.key.clone(),
            id,
            after: After::Nothing,
            open: true,
        };
        let Some(&(_, store, access)) = STORES.iter().find(|(name, ..)| *name == cmd) else {
            return Admission::Run { args, ticket };
        };
        match access {
            // Taken before the read runs: a change landing during it can only cost a needless
            // refusal later, never a stale save let through.
            Access::Read => {
                ticket.after = After::Read(store, self.inner.stores.revision(store));
                Admission::Run { args, ticket }
            }
            Access::WholeSave => {
                let now_is = self.inner.stores.revision(store);
                if base.is_none() || now_is.is_none() || base != now_is {
                    ticket.close(Outcome::Refused(Refusal::ChangedAtShack));
                    return Admission::Refused(Refusal::ChangedAtShack);
                }
                let args = match store {
                    Store::Settings => args,
                    Store::UiState => match merged_save(self.inner.stores.ui_state(), &args) {
                        Some(merged) => merged,
                        None => {
                            ticket.close(Outcome::Refused(Refusal::BadArguments));
                            return Admission::Refused(Refusal::BadArguments);
                        }
                    },
                };
                ticket.after = After::Saved(store);
                Admission::Run { args, ticket }
            }
        }
    }

    /// What became of each id, for a run of Nexus asking after a reconnect, in the order asked.
    pub(crate) fn outcomes(&self, caller: &CallerKey, ids: &[u64]) -> Vec<Outcome> {
        self.outcomes_at(caller, ids, Instant::now())
    }

    fn outcomes_at(&self, caller: &CallerKey, ids: &[u64], now: Instant) -> Vec<Outcome> {
        let mut journals = lock(&self.inner.journals);
        match journals.get_mut(caller) {
            Some(journal) => {
                journal.expire(now);
                ids.iter().map(|id| journal.outcome(*id)).collect()
            }
            None => vec![Outcome::Unknown; ids.len()],
        }
    }
}

/// The refusal a call's class gives it, its arguments and the lease. A Stop never gets here.
fn class_refusal(
    class: Option<Class>,
    cmd: &str,
    args: &Value,
    holds_lease: bool,
) -> Option<Refusal> {
    match class {
        None => Some(Refusal::Unclassed),
        Some(_) if !args.is_object() => Some(Refusal::BadArguments),
        Some(Class::RunsHere) => Some(Refusal::RunsHere),
        Some(Class::RefusedHere) => Some(Refusal::ShackOnly),
        // The plumbing only: transmitting from another computer waits for the stream's transmit
        // rules (one controller, presence, the held press) on this road.
        Some(Class::StartsTx) => Some(Refusal::TransmitNotYet),
        // Admitted before the journal, under Stop's rules.
        Some(Class::StopsTx) => None,
        Some(Class::RunsAtShack) => {
            argument_refusal(cmd, args).or_else(|| (!holds_lease).then_some(Refusal::NotController))
        }
    }
}

/// The journal for `key`, made if it is new. With [`CALLERS`] kept, the one idle longest with
/// nothing running gives way; `None` when every one has a call running.
fn journal_for<'a>(
    journals: &'a mut HashMap<CallerKey, Journal>,
    key: &CallerKey,
    now: Instant,
) -> Option<&'a mut Journal> {
    if !journals.contains_key(key) && journals.len() >= CALLERS {
        let idle = journals
            .iter()
            .filter(|(_, journal)| journal.running.is_empty())
            .min_by_key(|(_, journal)| journal.last_seen)
            .map(|(key, _)| key.clone())?;
        journals.remove(&idle);
    }
    let journal = journals
        .entry(key.clone())
        .or_insert_with(|| Journal::new(now));
    journal.last_seen = now;
    journal.expire(now);
    Some(journal)
}

/// A call's id, against the ids its run of Nexus used before.
enum Seen {
    /// Higher than every one before.
    New,
    /// Used before, and this is what became of it.
    Again(Outcome),
    /// Used before, and what became of it is no longer kept.
    Forgotten,
}

/// One run of Nexus's calls: the highest id it has used, the outcomes kept, and what is running.
struct Journal {
    /// An id at or below this one is a repeat.
    highest: Option<u64>,
    /// Outcomes by id, with when each was recorded.
    outcomes: HashMap<u64, (Instant, Outcome)>,
    /// The ids in `outcomes`, oldest record first.
    order: VecDeque<u64>,
    /// The calls running now, and their commands.
    running: HashMap<u64, String>,
    /// The start of the current one-second window, and the calls made in it.
    window: (Instant, u32),
    last_seen: Instant,
}

impl Journal {
    fn new(now: Instant) -> Self {
        Self {
            highest: None,
            outcomes: HashMap::new(),
            order: VecDeque::new(),
            running: HashMap::new(),
            window: (now, 0),
            last_seen: now,
        }
    }

    /// `id` against the ids used before; a new one becomes the highest.
    fn seen(&mut self, id: u64) -> Seen {
        if self.highest.is_some_and(|highest| id <= highest) {
            return match self.outcome(id) {
                Outcome::Unknown => Seen::Forgotten,
                known => Seen::Again(known),
            };
        }
        self.highest = Some(id);
        Seen::New
    }

    fn outcome(&self, id: u64) -> Outcome {
        if self.running.contains_key(&id) {
            Outcome::Running
        } else {
            self.outcomes
                .get(&id)
                .map_or(Outcome::Unknown, |(_, outcome)| outcome.clone())
        }
    }

    /// Counts this call in the current second, and says whether it is over the cap.
    fn over_rate(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.window.0) >= Duration::from_secs(1) {
            self.window = (now, 0);
        }
        self.window.1 = self.window.1.saturating_add(1);
        self.window.1 > PER_SECOND
    }

    fn over_caps(&self, cmd: &str) -> bool {
        self.running.len() >= IN_FLIGHT
            || self
                .running
                .values()
                .filter(|running| *running == cmd)
                .count()
                >= PER_COMMAND
    }

    /// Records what became of `id`, and keeps the newest [`JOURNAL`] outcomes.
    fn record(&mut self, id: u64, outcome: Outcome, now: Instant) {
        self.running.remove(&id);
        if self.outcomes.insert(id, (now, outcome)).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > JOURNAL {
            if let Some(old) = self.order.pop_front() {
                self.outcomes.remove(&old);
            }
        }
    }

    /// Forgets the outcomes older than [`RESULT_AGE`].
    fn expire(&mut self, now: Instant) {
        while let Some(&id) = self.order.front() {
            let fresh = self
                .outcomes
                .get(&id)
                .is_some_and(|(at, _)| now.saturating_duration_since(*at) < RESULT_AGE);
            if fresh {
                break;
            }
            self.order.pop_front();
            self.outcomes.remove(&id);
        }
    }
}

/// What finishing a call does beyond recording it.
enum After {
    Nothing,
    /// A read of the store: answer with the revision taken before it ran.
    Read(Store, Option<Revision>),
    /// An accepted whole save: answer with the store's revision after it.
    Saved(Store),
}

/// An admitted call's place in its journal and its slot under the caps. Dropping it without
/// [`finish`](Ticket::finish) records the call as lost and frees the slot.
pub(crate) struct Ticket<S: Stores> {
    dispatcher: Dispatcher<S>,
    caller: CallerKey,
    id: u64,
    after: After,
    open: bool,
}

impl<S: Stores> Ticket<S> {
    /// Records the call's result, frees its slot, and gives what goes back to the other computer.
    pub(crate) fn finish(mut self, result: Result<Value, String>) -> Reply {
        let stores = &self.dispatcher.inner.stores;
        let (result, revision) = match (&self.after, result) {
            (After::Read(Store::UiState, revision), Ok(value)) => {
                (Ok(station_answer(value)), revision.clone())
            }
            (After::Read(_, revision), Ok(value)) => (Ok(value), revision.clone()),
            (After::Saved(store), Ok(value)) => (Ok(value), stores.revision(*store)),
            (_, result) => (result, None),
        };
        self.close(match &result {
            Ok(_) => Outcome::Done,
            Err(error) => Outcome::Failed(bounded(error)),
        });
        Reply { result, revision }
    }

    fn close(&mut self, outcome: Outcome) {
        self.open = false;
        let mut journals = lock(&self.dispatcher.inner.journals);
        if let Some(journal) = journals.get_mut(&self.caller) {
            journal.record(self.id, outcome, Instant::now());
        }
    }
}

impl<S: Stores> Drop for Ticket<S> {
    fn drop(&mut self) {
        if self.open {
            self.close(Outcome::Lost);
        }
    }
}

/// A command's error, cut to [`ERROR_BYTES`] on a character boundary.
fn bounded(error: &str) -> String {
    let mut end = error.len().min(ERROR_BYTES);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error[..end].to_string()
}

/// The journals' lock, taken back from a panic that poisoned it: a journal is only ever updated
/// whole, so what a panic left is still a journal.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
