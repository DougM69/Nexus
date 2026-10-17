//! The dispatcher for another computer that drives this station: what that computer's Nexus runs
//! itself, what it sends here, and what the shack admits, each by the command's class in
//! `command_classes`. "Here", in this file as in the table, is the computer the operator sits at;
//! "the shack" is the one with the radio. Nothing outside the tests calls it yet: the command link
//! that carries each call between the two computers hands every call it reads to
//! [`Dispatcher::admit`], and the other computer's handler asks [`route`] before it sends one.
//!
//! ## Deny by default, at both ends
//!
//! The other computer routes by [`route`]: a command classed "runs here" runs on that computer, a
//! station command goes to the shack, the ui-state pair splits by key, and a command with no class,
//! or one only the shack's own screen may give, is refused there and never sent. The shack does not
//! trust that routing. [`Dispatcher::admit`] looks the class up again and refuses a command with no
//! class, one that runs on the other computer, one that is refused from afar, and every command that
//! starts a transmission but the few wired for this road ([`transmit_from_afar`]).
//!
//! ## A transmission started from afar
//!
//! Only the starts in [`WIRED`] go, and each only under the stream's transmit rules, as the
//! operator signed them for this road: the computer holds the lease on the connection the call came
//! by, its Stop road holds live presence, and the newest heartbeat on that road named shack state
//! its screen fetched under two seconds before, by the shack's own clock ([`Caller::roads`]). The FT
//! starts are the desktop's own FT buttons, the JS8 starts the cockpit's sends, and the CW starts
//! keyboard CW and its macros as text; none of them carries a time, so the shack's slots, JS8
//! periods and keyer time each one. The PTT is never this command from afar but a held press on
//! Stop's road ([`Wired::HeldPress`]). Every other start is refused by name.
//!
//! Stop wins over a start that was in flight. The command link binds each start to the transmit
//! stand-down generation as it reads it ([`Afar`]), and its handler checks that generation under the
//! Engine lock it then runs in ([`still_bound`]): a Stop taken in between, by either road, or any
//! halt at the station, has moved it, and the start does not run ([`Refusal::Stopped`]). A Stop that
//! overtook the start on the way (by the second road, or ahead of it on the link) is caught by its
//! number: every call says how many Stops its computer had pressed when it was made ([`Call`]), and
//! a start made before a Stop the shack has taken is refused as it is read.
//!
//! From afar only those starts arm transmit. A section change, a spot worked or an SSTV channel
//! picked runs at the shack as its own screen's does, but the lane writes [`Afar`] into it too, and
//! its handler then enters the section as Remote's entry does, arming nothing ([`ENTRIES`]).
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
//! - **At most once.** Every call carries an id, unique within one run of Nexus on the other
//!   computer. The ids rise as that computer makes them, but calls admitted together, and a call
//!   sent in pieces, can come in another order. The journal answers a repeated id with what became
//!   of it and never runs it twice, and after a reconnect it says what became of each id
//!   ([`Dispatcher::outcomes`]). It keeps the newest [`JOURNAL`] outcomes for [`RESULT_AGE`]; an id
//!   at or below one whose outcome it no longer keeps is refused, never run.
//! - **Caps.** [`IN_FLIGHT`] calls at once from one computer, [`PER_COMMAND`] of them one command,
//!   and [`PER_SECOND`] a second. The other computer holds its own calls back to stay under them.
//! - **A stale whole save is refused** (the operator's ruling of 2026-10-05). A command that replaces
//!   a store whole ([`STORES`]) names the revision of the copy it was made from, and is refused,
//!   "Changed at the shack. Reload and save again.", unless that is the store's revision now. A read
//!   of the store, and an accepted save, answer with the revision the other computer's copy now has.
//! - **A list save is checked on its own list** (the operator's ruling of 2026-10-07, "Check only
//!   that list"). The macros, the blocked calls, the routing rules, and a radio's bands or its
//!   profile each replace one part of the settings ([`parts`]). The shack keeps the copies it gave
//!   out, by revision, and a list save made from an older copy is admitted when its part is the same
//!   in that copy as now: a QSY, or any other change elsewhere, refuses it no longer. The revision
//!   it answers with is that copy's with the list as saved, so a second list save from the same
//!   copy is checked on its own list too.
//! - **The one mixed store**, `ui-state.json`, splits by key: the station's data ([`STATION_KEYS`])
//!   lives at the shack, every other key on the computer the operator sits at.
//! - **Events.** An alert (`crate::alert`, Pounce's today) is the one event the shack sends on, and
//!   the only one the other computer takes ([`FORWARDED_EVENTS`]).
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

// The tables both ends route and admit by are the client core's (`tempo_client::dispatch`), read
// here as the other computer reads them there: the refusals, the routes, the starts wired from
// afar, Stop's rules, the stores, the ui-state split, the events that cross, the profiles and
// what became of a call. This module is the shack's dispatcher over them.
#[cfg(all(test, feature = "bench-road-switch"))]
pub(crate) use tempo_client::dispatch::contract_text;
use tempo_client::dispatch::{argument_refusal, runs_as_stop, Access, STORES};
pub(crate) use tempo_client::dispatch::{
    forwards, is_stop, join_ui_state, press_from_afar, route, split_ui_state, store_access,
    transmit_from_afar, Outcome, Profile, Refusal, Route, Store, Wired, FORWARDED_EVENTS,
};
#[cfg(test)]
pub(crate) use tempo_client::dispatch::{off_args, Off, OFF_HALVES, STATION_KEYS, WIRED};

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

// --- The other computer's end ------------------------------------------------------------------

/// What the shack's command lane writes into a call it admitted from another computer that a
/// handler must run as from afar, under the argument [`AFAR`]: the transmit stand-down generation
/// as the lane read the call off the link. A start runs only under it ([`still_bound`]); a section
/// entry arms nothing with it ([`ENTRIES`]). The lane alone writes it; anything the other computer
/// sends under that name goes before any call is admitted. The shack's own screen never sends it,
/// so its calls are as they always were.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Afar {
    pub(crate) stand_down: u64,
}

/// The argument [`Afar`] goes under: the name each handler that takes it gives its parameter.
pub(crate) const AFAR: &str = "afar";

/// ⛔ The section entries: from the shack's own screen each arms the Enable-TX latch on the way
/// into a manual section (Phone, CW, RTTY, Keyboard), so the manual keys there work. From afar each
/// arms nothing, as Remote's entry arms nothing: the latch is armed from afar only by a wired
/// start, under the stream's transmit rules, and with it armed the shack's own keys (its PTT, a CAT
/// client's) would be live with no presence behind them.
pub(crate) const ENTRIES: [&str; 3] = ["set_operating_mode", "sstv_tune", "work_spot"];

/// Whether the command lane writes [`Afar`] into an admitted call of `cmd`: each wired start but
/// the held press, which never runs as a command, and each section entry ([`ENTRIES`]). A test
/// holds this to the handlers that take it.
pub(crate) fn takes_afar(cmd: &str) -> bool {
    matches!(
        transmit_from_afar(cmd),
        Ok(Wired::Ft | Wired::VoiceKeyer | Wired::Js8 | Wired::Cw)
    ) || ENTRIES.contains(&cmd)
}

/// ⛔ STOP WINS OVER A START THAT WAS IN FLIGHT. A start from afar runs only while the transmit
/// stand-down generation is the one the command lane read it under ([`Afar`]). Asked by the
/// start's own handler, first, under the Engine lock the start then runs in: a Stop on either road,
/// the off half of a switch on the link, presence lapsing, or any halt at the station, taken after
/// the start was read, has moved it, so the start does not run at all. One taken once the start has
/// run waits for the same lock, and stops what it began. Asked before the start's own work, so a
/// start whose work halts (a QSY's change of context) never trips on itself. `None`, the shack's own
/// screen, runs as it always has.
pub(crate) fn still_bound(
    engine: &tempo_app::engine::Engine,
    afar: Option<&Afar>,
) -> Result<(), String> {
    match afar {
        Some(afar) if engine.remote_transmit_stand_down_generation() != afar.stand_down => {
            Err(STOPPED_SINCE.to_string())
        }
        _ => Ok(()),
    }
}

/// A start's own error when [`still_bound`] stopped it from running: the command lane answers it
/// as [`Refusal::Stopped`].
pub(crate) const STOPPED_SINCE: &str =
    "Transmit was stopped after this was pressed, so it did not run.";

// --- Stop's rules --------------------------------------------------------------------------------

/// A Stop, or the off half of a transmit switch, admitted at once with no lock taken: `Ok` with the
/// call to run as it came, or `Err` with the call back when it is anything else. The command link
/// asks this on its reading task before it hands a call anywhere, so a Stop never waits for another
/// call, the Engine or a journal; [`Dispatcher::admit`] admits a Stop the same way.
pub(crate) fn admit_stop(call: Call) -> Result<Call, Call> {
    if runs_as_stop(class_of(&call.cmd), &call.cmd, &call.args) {
        Ok(call)
    } else {
        Err(call)
    }
}

// --- Stores and their revisions ------------------------------------------------------------------

/// The settings copies kept, by revision, for list saves made from them.
const SERVED: usize = 32;

/// One step of a path into the settings' JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    /// A field, by its name in the JSON.
    Key(String),
    /// The radio in `radios` with this id.
    Radio(u64),
}

/// Where in the settings' JSON the part a list save replaces is: one path or more, the last step of
/// each a field. `None` for arguments that name no part (a radio save with no id), which is then
/// checked on the whole settings. A radio's profile save is checked on the fields its patch writes,
/// never the radio's tune memory, which the patch does not carry (`RadioProfilePatch`).
fn parts(cmd: &str, args: &Value) -> Option<Vec<Vec<Step>>> {
    let key = |name: &str| Step::Key(name.to_string());
    let radio = || Some(Step::Radio(args.get("id")?.as_u64()?));
    Some(match cmd {
        "set_blocked_calls" => vec![vec![key("blockedCalls")]],
        "set_routing_rules" => vec![vec![key("routingRules")]],
        "set_rtty_macros" => vec![
            vec![key("macros"), key("rttyProfiles")],
            vec![key("macros"), key("activeRttyProfile")],
        ],
        "set_psk_macros" => vec![
            vec![key("macros"), key("pskProfiles")],
            vec![key("macros"), key("activePskProfile")],
        ],
        "set_radio_bands" => vec![vec![key("radios"), radio()?, key("bands")]],
        "update_radio_profile" => {
            let radio = radio()?;
            args.get("patch")?
                .as_object()?
                .iter()
                // A field the patch leaves unset (`null`) is not written.
                .filter(|(_, value)| !value.is_null())
                .map(|(field, _)| vec![key("radios"), radio.clone(), key(field)])
                .collect()
        }
        _ => return None,
    })
}

/// What `settings` holds at `path`, if anything.
fn at<'a>(settings: &'a Value, path: &[Step]) -> Option<&'a Value> {
    path.iter().try_fold(settings, |value, step| match step {
        Step::Key(name) => value.get(name),
        Step::Radio(id) => value
            .as_array()?
            .iter()
            .find(|radio| radio.get("id").and_then(Value::as_u64) == Some(*id)),
    })
}

/// `copy` with what `saved` holds at each of `paths` put in its place: the other computer's copy
/// once its list save has run.
fn with_parts(mut copy: Value, saved: &Value, paths: &[Vec<Step>]) -> Value {
    for path in paths {
        let Some((Step::Key(field), within)) = path.split_last() else {
            continue;
        };
        let parent = within.iter().try_fold(&mut copy, |value, step| match step {
            Step::Key(name) => value.get_mut(name),
            Step::Radio(id) => value
                .as_array_mut()?
                .iter_mut()
                .find(|radio| radio.get("id").and_then(Value::as_u64) == Some(*id)),
        });
        let Some(Value::Object(parent)) = parent else {
            continue;
        };
        match at(saved, path) {
            Some(value) => {
                parent.insert(field.clone(), value.clone());
            }
            None => {
                parent.remove(field);
            }
        }
    }
    copy
}

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
    /// The whole of `ui-state.json` now, or `None` when the file is there and does not read: a
    /// save from afar merged into an empty map would remove every key of the shack's own.
    fn ui_state(&self) -> Option<BTreeMap<String, String>>;
    /// The store's contents as JSON with their revision, read together: what a list save's part is
    /// checked in. `None` where only the revision is kept, and a list save is then checked on the
    /// whole store, as a whole save is.
    fn copy(&self, _store: Store) -> Option<(Revision, Value)> {
        None
    }
}

/// The shack's stores: its Engine's settings, and its `ui-state.json`.
pub(crate) struct ShackStores {
    pub(crate) engine: crate::SharedEngine,
}

impl Stores for ShackStores {
    fn revision(&self, store: Store) -> Option<Revision> {
        match store {
            Store::Settings => self.copy(store).map(|(revision, _)| revision),
            Store::UiState => Revision::of(&split_ui_state(self.ui_state()?).0),
        }
    }

    fn ui_state(&self) -> Option<BTreeMap<String, String>> {
        crate::ui_state_read(&crate::ui_state_path(), true).ok()
    }

    fn copy(&self, store: Store) -> Option<(Revision, Value)> {
        match store {
            // The Engine's own copy, every field of it. A field a read does not show (the Cloudlog
            // key waiting for the keychain) can only cost a needless refusal, never a missed change.
            Store::Settings => {
                let copy =
                    serde_json::to_value(crate::engine_lock(&self.engine).settings()).ok()?;
                Some((Revision::of(&copy)?, copy))
            }
            // Saved whole, so checked on its revision alone.
            Store::UiState => None,
        }
    }
}

// --- The one mixed store -------------------------------------------------------------------------

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

/// Who is calling, the kind of client it is, whether it holds the station's controller lease
/// now, and whether its roads let a transmission start.
#[derive(Clone, Debug)]
pub(crate) struct Caller {
    pub(crate) key: CallerKey,
    pub(crate) profile: Profile,
    pub(crate) holds_lease: bool,
    /// For a start: the stream's transmit rules as this computer's Stop road reads them now (both
    /// roads up, live presence, fresh state), or the refusal of the first that does not hold, or
    /// [`Refusal::Stopped`] for a start its computer made before a Stop the shack has since taken.
    /// Asked by the command lane only for a command that starts a transmission.
    pub(crate) roads: Result<(), Refusal>,
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
    /// How many Stops the other computer had pressed when this call was made, a Stop counting
    /// itself: what a start is judged by against the Stops the shack has taken since (the command
    /// lane's, `remote_service::lan::stop_road::Ready`). Nothing else reads it, and it never
    /// refuses a Stop.
    pub(crate) stops: u64,
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
    /// The settings copies given out, by revision, newest last ([`SERVED`] of them): what a list
    /// save made from one of them is checked in. A copy no longer kept costs a needless refusal.
    served: Mutex<VecDeque<(Revision, Value)>>,
}

impl<S: Stores> Dispatcher<S> {
    pub(crate) fn new(stores: S) -> Self {
        Self {
            inner: Arc::new(Inner {
                stores,
                journals: Mutex::new(HashMap::new()),
                served: Mutex::new(VecDeque::new()),
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
            stops: _,
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
            } else if let Some(refusal) = class_refusal(caller, class, &cmd, &args) {
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
            // refusal later, never a stale save let through. The copy is kept for a list save.
            Access::Read => {
                let revision = match self.inner.stores.copy(store) {
                    Some((revision, copy)) => {
                        self.keep(revision.clone(), copy);
                        Some(revision)
                    }
                    None => self.inner.stores.revision(store),
                };
                ticket.after = After::Read(store, revision);
                Admission::Run { args, ticket }
            }
            Access::WholeSave | Access::PartSave => {
                let now = self.inner.stores.copy(store);
                let current = match &now {
                    Some((revision, _)) => Some(revision.clone()),
                    None => self.inner.stores.revision(store),
                };
                let fresh = base.is_some() && base == current;
                let paths = match access {
                    Access::PartSave => parts(&cmd, &args),
                    _ => None,
                };
                // A list save, and the copy it was made from: the shack's own when the save names
                // it, or else the kept copy it names, when its part is in that copy as it is now.
                let made_from = match (paths, &now) {
                    (Some(paths), Some((_, now))) if fresh => Some((now.clone(), paths)),
                    (Some(paths), Some((_, now))) => base
                        .as_ref()
                        .and_then(|base| self.served(base))
                        .filter(|copy| paths.iter().all(|path| at(copy, path) == at(now, path)))
                        .map(|copy| (copy, paths)),
                    _ => None,
                };
                if !fresh && made_from.is_none() {
                    ticket.close(Outcome::Refused(Refusal::ChangedAtShack));
                    return Admission::Refused(Refusal::ChangedAtShack);
                }
                let args = match store {
                    Store::Settings => args,
                    // The shack's own keys as the file holds them now; a file that stopped
                    // reading since its revision was taken refuses the save, as one that did not
                    // read then would have.
                    Store::UiState => match self.inner.stores.ui_state() {
                        None => {
                            ticket.close(Outcome::Refused(Refusal::ChangedAtShack));
                            return Admission::Refused(Refusal::ChangedAtShack);
                        }
                        Some(shack) => match merged_save(shack, &args) {
                            Some(merged) => merged,
                            None => {
                                ticket.close(Outcome::Refused(Refusal::BadArguments));
                                return Admission::Refused(Refusal::BadArguments);
                            }
                        },
                    },
                };
                ticket.after = match made_from {
                    Some((copy, paths)) => After::SavedPart(copy, paths),
                    None => After::Saved(store),
                };
                Admission::Run { args, ticket }
            }
        }
    }

    /// Keeps a settings copy given out under `revision`, newest last.
    fn keep(&self, revision: Revision, copy: Value) {
        let mut served = lock(&self.inner.served);
        served.retain(|(kept, _)| *kept != revision);
        served.push_back((revision, copy));
        while served.len() > SERVED {
            served.pop_front();
        }
    }

    /// The settings copy given out under `revision`, if it is still kept.
    fn served(&self, revision: &Revision) -> Option<Value> {
        lock(&self.inner.served)
            .iter()
            .rev()
            .find(|(kept, _)| kept == revision)
            .map(|(_, copy)| copy.clone())
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
    caller: &Caller,
    class: Option<Class>,
    cmd: &str,
    args: &Value,
) -> Option<Refusal> {
    // Every profile this build serves is admitted by class alone. A second one stops this binding
    // from compiling until its own rule is written here.
    let Profile::Desktop = caller.profile;
    match class {
        None => Some(Refusal::Unclassed),
        Some(_) if !args.is_object() => Some(Refusal::BadArguments),
        Some(Class::RunsHere) => Some(Refusal::RunsHere),
        Some(Class::RefusedHere) => Some(Refusal::ShackOnly),
        // A start from afar: only one wired for this road, and only under the stream's transmit
        // rules: the lease bound to this connection, then this computer's roads.
        Some(Class::StartsTx) => match transmit_from_afar(cmd) {
            Err(refusal) => Some(refusal),
            Ok(Wired::HeldPress) => Some(Refusal::HeldPress),
            Ok(Wired::Ft | Wired::VoiceKeyer | Wired::Js8 | Wired::Cw) => {
                argument_refusal(cmd, args)
                    .or_else(|| (!caller.holds_lease).then_some(Refusal::NotController))
                    .or_else(|| caller.roads.err())
            }
        },
        // Admitted before the journal, under Stop's rules.
        Some(Class::StopsTx) => None,
        Some(Class::RunsAtShack) => argument_refusal(cmd, args)
            .or_else(|| (!caller.holds_lease).then_some(Refusal::NotController)),
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
    /// Not used before.
    New,
    /// Used before, and this is what became of it.
    Again(Outcome),
    /// Used before, and what became of it is no longer kept.
    Forgotten,
}

/// One run of Nexus's calls: the outcomes kept, what is running, and the highest id whose outcome
/// is no longer kept.
struct Journal {
    /// An id at or below this one with no outcome kept may have run once already: it is refused.
    /// Above it, an id with no outcome kept and not running is new, in whatever order it comes.
    forgotten: Option<u64>,
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
            forgotten: None,
            outcomes: HashMap::new(),
            order: VecDeque::new(),
            running: HashMap::new(),
            window: (now, 0),
            last_seen: now,
        }
    }

    /// `id` against the ids used before.
    fn seen(&self, id: u64) -> Seen {
        match self.outcome(id) {
            Outcome::Unknown if self.forgotten.is_some_and(|forgotten| id <= forgotten) => {
                Seen::Forgotten
            }
            Outcome::Unknown => Seen::New,
            known => Seen::Again(known),
        }
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
                self.forget(old);
            }
        }
    }

    /// No longer keeps what became of `id`, and from now on refuses every id up to it that it has
    /// no outcome for.
    fn forget(&mut self, id: u64) {
        self.outcomes.remove(&id);
        self.forgotten = Some(self.forgotten.map_or(id, |forgotten| forgotten.max(id)));
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
            self.forget(id);
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
    /// An accepted list save of the settings, made from this copy: answer with that copy's
    /// revision once the parts at these paths are as saved.
    SavedPart(Value, Vec<Vec<Step>>),
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
            (After::Saved(store), Ok(value)) => match stores.copy(*store) {
                Some((revision, copy)) => {
                    self.dispatcher.keep(revision.clone(), copy);
                    (Ok(value), Some(revision))
                }
                None => (Ok(value), stores.revision(*store)),
            },
            (After::SavedPart(copy, paths), Ok(value)) => {
                let revision = stores.copy(Store::Settings).and_then(|(_, saved)| {
                    let mine = with_parts(copy.clone(), &saved, paths);
                    let revision = Revision::of(&mine)?;
                    self.dispatcher.keep(revision.clone(), mine);
                    Some(revision)
                });
                (Ok(value), revision)
            }
            (_, result) => (result, None),
        };
        self.close(match &result {
            Ok(_) => Outcome::Done,
            Err(error) => Outcome::Failed(bounded(error)),
        });
        Reply { result, revision }
    }

    /// Records the call as refused when it came to run, frees its slot: a start [`still_bound`]
    /// stopped. Nothing ran.
    pub(crate) fn refuse(mut self, refusal: Refusal) {
        self.close(Outcome::Refused(refusal));
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

// --- How the shack runs a call -------------------------------------------------------------------

/// Runs an admitted call at the shack, through the app's own command handler, and hands what it
/// answered to the [`Answer`], from whichever thread the command finished on. [`tauri_runner`] is
/// the shack's; a test hands in its own.
pub(crate) type Runner = Arc<dyn Fn(String, Value, Answer) + Send + Sync>;

/// Where a command's answer goes: its value, or its own error.
pub(crate) type Answer = Box<dyn FnOnce(Result<Value, String>) + Send>;

/// The shack's own command handler, reached through Tauri's public entry for a webview's IPC
/// (`Webview::on_message`) as the main window's screen reaches it: the same handler table, state
/// and permissions, and the invoke key the app was built with, which never leaves this process. A
/// sync command runs on the calling thread and an async one on Tauri's runtime, as from the screen.
/// The main window's address is read once, since reading it is a round trip to the main thread: it
/// is the local origin the permission check sees for the screen's own calls.
pub(crate) fn tauri_runner<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Runner {
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
    use tauri::Manager;
    let address: std::sync::OnceLock<tauri::Url> = std::sync::OnceLock::new();
    Arc::new(move |cmd: String, args: Value, answer: Answer| {
        let Some(window) = app.get_webview_window("main") else {
            return answer(Err("The station's main window is not open.".to_string()));
        };
        let webview: tauri::Webview<R> = window.as_ref().clone();
        let url = match address.get() {
            Some(url) => url.clone(),
            None => match webview.url() {
                Ok(url) => address.get_or_init(|| url).clone(),
                Err(_) => {
                    return answer(Err("The station's main window has no address.".to_string()))
                }
            },
        };
        let request = tauri::webview::InvokeRequest {
            cmd,
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url,
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: app.invoke_key().to_string(),
        };
        webview.on_message(
            request,
            Box::new(move |_webview, _cmd, response, _callback, _error| {
                answer(match response {
                    InvokeResponse::Ok(InvokeResponseBody::Json(json)) => {
                        serde_json::from_str(&json).map_err(|_| "unreadableAnswer".to_string())
                    }
                    // No command this app registers answers in raw bytes.
                    InvokeResponse::Ok(InvokeResponseBody::Raw(_)) => {
                        Err("unreadableAnswer".to_string())
                    }
                    InvokeResponse::Err(error) => Err(match error.0 {
                        Value::String(text) => text,
                        other => other.to_string(),
                    }),
                })
            }),
        );
    })
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
