//! The command lane at the shack (`super::channel`, step 5 of its ladder): one paired computer's
//! calls, run as the shack's own screen runs them, the feeds it watches and the events it is sent.
//! It opens only on a connection whose computer named this same build (`super::wire`).
//!
//! ## A call
//!
//! The reading task parses it and hands it on, and never runs a command or takes a lock itself.
//! - **A Stop**, or the off half of a transmit switch, is admitted there, with no lock
//!   (`command_dispatch::admit_stop`), and run at once on a thread of its own, behind no other
//!   call, its answer ahead of everything waiting to be written. It needs no lease and takes no slot.
//! - **Any other call** is admitted off the reading task, since a store's revision can wait on the
//!   Engine: the dispatcher's lease, class, journal, caps and revisions, with the lease read live
//!   from the operations authority. It then runs through the app's own handler, and its answer goes
//!   in its turn. At most [`PENDING`] wait at once from one computer; one more is refused `busy`
//!   on the reading task, before any of that.
//!
//! What a call answers: its value (with the revision of the store it read or saved), its own error,
//! the dispatcher's refusal, or, for an id already used, what became of it, and it is not run again.
//!
//! ## Feeds and events
//!
//! A watched feed runs its command here at its own pace, through the same handler, and pushes what
//! it answers, latest only (`super::outbox`). An event goes on when the dispatcher says it crosses
//! (`forwards`). Both only while this computer holds station control, as every call but a Stop:
//! nobody else is shown the station (the operator's ruling of 2026-09-27, "Controller only, for
//! now").
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::super::operations::Authority;
use super::outbox::Outbox;
use super::wire::{self, Assembly, Feed, Whole};
use crate::command_dispatch::{
    admit_stop, forwards, Admission, Answer, Call, Caller, CallerKey, Dispatcher, Outcome, Profile,
    Refusal, Reply, Runner, ShackStores,
};

/// Calls one computer may have waiting at once, admitted or not: the dispatcher's own cap on calls
/// in flight, and as many again being admitted. One more is refused `busy` without a thread.
pub(crate) const PENDING: usize = 2 * crate::command_dispatch::IN_FLIGHT;
/// The most ids one `outcomes` may ask after: the calls in flight when a link dropped.
pub(crate) const OUTCOMES: usize = 64;
/// The most feeds one `watch` may name.
pub(crate) const WATCH: usize = 8;
/// Events kept for a lane that has fallen behind; older ones are skipped.
const EVENTS: usize = 64;

/// An event raised at the shack, as the lane forwards it: its name, a number the shack gives it
/// (the same on every connection), when it was raised, and its payload as the app emitted it.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub(crate) name: &'static str,
    pub(crate) id: u64,
    pub(crate) at_ms: u64,
    pub(crate) data: String,
}

/// What the lane takes from the app: how a call runs, and the events raised.
#[derive(Clone)]
pub(crate) struct Commands {
    pub(crate) runner: Runner,
    pub(crate) events: broadcast::Sender<Event>,
}

impl Commands {
    /// The app's handler and a fresh event channel: what the app hands the service.
    pub(crate) fn new(runner: Runner) -> Self {
        Self {
            runner,
            events: broadcast::channel(EVENTS).0,
        }
    }

    /// An event the app raised, numbered and timed here, for every lane open now.
    pub(crate) fn raised(&self, name: &'static str, data: String) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let _ = self.events.send(Event {
            name,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            at_ms: super::super::now_ms(),
            data,
        });
    }
}

impl Default for Commands {
    /// No handler: every call is answered with an error, and no event is raised. A service built
    /// without the app (a test's) has nothing to run.
    fn default() -> Self {
        Self::new(Arc::new(|_: String, _: Value, answer: Answer| {
            answer(Err("This station runs no commands.".to_string()))
        }))
    }
}

/// What every call, feed and event on one lane needs.
struct Context {
    dispatcher: Dispatcher<ShackStores>,
    runner: Runner,
    authority: Arc<Authority>,
    connection: u64,
    device: String,
    outbox: Outbox,
}

impl Context {
    /// Whether this computer holds station control now. Never asked for a Stop.
    fn controls(&self) -> bool {
        self.authority
            .lan_controls(self.connection, &self.device, Instant::now())
    }
}

/// The open lane on one connection.
pub(super) struct Lane {
    context: Arc<Context>,
    key: CallerKey,
    profile: Profile,
    pending: Arc<AtomicUsize>,
    assembly: Assembly,
    feeds: Vec<(&'static Feed, tokio::task::JoinHandle<()>)>,
    events: tokio::task::JoinHandle<()>,
}

/// What opening the lane takes: the station's parts and this connection's.
pub(super) struct Opening {
    pub dispatcher: Dispatcher<ShackStores>,
    pub commands: Commands,
    pub authority: Arc<Authority>,
    pub connection: u64,
    pub device: String,
    pub run: u64,
    pub profile: Profile,
    pub outbox: Outbox,
}

impl Lane {
    /// The lane open, and forwarding events.
    pub(super) fn open(opening: Opening) -> Self {
        let context = Arc::new(Context {
            dispatcher: opening.dispatcher,
            runner: opening.commands.runner,
            authority: opening.authority,
            connection: opening.connection,
            device: opening.device.clone(),
            outbox: opening.outbox,
        });
        let events = tokio::spawn(forward(
            context.clone(),
            opening.commands.events.subscribe(),
        ));
        Self {
            key: CallerKey {
                device: opening.device,
                run: opening.run,
            },
            profile: opening.profile,
            pending: Arc::new(AtomicUsize::new(0)),
            assembly: Assembly::new(wire::CALL_BYTES),
            feeds: Vec::new(),
            events,
            context,
        }
    }

    /// One piece of a call: the call's JSON once its last piece is here. `Err` for a piece no
    /// computer of this protocol sends, which ends the connection.
    pub(super) fn piece(&mut self, frame: &[u8]) -> Result<Option<String>, ()> {
        match self.assembly.piece(frame)? {
            None => Ok(None),
            Some(Whole::Text(text)) => Ok(Some(text)),
            Some(Whole::Binary(_)) => Err(()),
        }
    }

    /// One call, on the reading task: a Stop admitted here and run at once, anything else handed
    /// off to be admitted.
    pub(super) fn call(&mut self, call: Call) {
        let id = call.id;
        let call = match admit_stop(call) {
            Ok(stop) => {
                let context = self.context.clone();
                tokio::task::spawn_blocking(move || {
                    let answer = answered(id, run(&context.runner, &stop.cmd, stop.args), None);
                    let _ = context.outbox.urgent(answer);
                });
                return;
            }
            Err(call) => call,
        };
        if self.pending.load(Ordering::SeqCst) >= PENDING {
            let _ = self.context.outbox.send(refused(id, Refusal::Busy));
            return;
        }
        let waiting = Waiting::new(&self.pending);
        let (context, key, profile) = (self.context.clone(), self.key.clone(), self.profile);
        tokio::task::spawn_blocking(move || {
            let _waiting = waiting;
            let caller = Caller {
                key,
                profile,
                holds_lease: context.controls(),
            };
            let cmd = call.cmd.clone();
            match context.dispatcher.admit(&caller, call) {
                // `admit_stop` took every Stop; one here still runs at once, as a Stop.
                Admission::Stop { args } => {
                    let _ =
                        context
                            .outbox
                            .urgent(answered(id, run(&context.runner, &cmd, args), None));
                }
                Admission::Run { args, ticket } => {
                    let answer = match run(&context.runner, &cmd, args) {
                        Some(result) => {
                            let Reply { result, revision } = ticket.finish(result);
                            answered(id, Some(result), revision.as_ref().map(|r| r.as_str()))
                        }
                        // Its answer never came: the ticket records it as lost.
                        None => {
                            drop(ticket);
                            repeated(id, &Outcome::Lost)
                        }
                    };
                    let _ = context.outbox.send(answer);
                }
                Admission::Answered(outcome) => {
                    let _ = context.outbox.send(repeated(id, &outcome));
                }
                Admission::Refused(refusal) => {
                    let _ = context.outbox.send(refused(id, refusal));
                }
            }
        });
    }

    /// What became of each of `ids`, asked after a reconnect, answered off the reading task.
    pub(super) fn outcomes(&self, ids: Vec<u64>) {
        let (context, key) = (self.context.clone(), self.key.clone());
        tokio::task::spawn_blocking(move || {
            let outcomes: Vec<Value> = context
                .dispatcher
                .outcomes(&key, &ids)
                .iter()
                .map(outcome)
                .collect();
            let answer = json!({"type":"outcomes","ids":ids,"outcomes":outcomes});
            let _ = context.outbox.text(answer.to_string());
        });
    }

    /// The feeds this computer watches from now on, in place of the ones before: each new one
    /// starts, each dropped one stops (its numbers and its deflate stream carry on if it is watched
    /// again), and the computer is told which names are no feed of this build's.
    pub(super) fn watch(&mut self, names: Vec<String>) {
        let mut watching: Vec<&'static Feed> = Vec::new();
        let mut unknown = Vec::new();
        for name in names {
            match wire::feed(&name) {
                Some(feed) if !watching.iter().any(|f| f.name == feed.name) => watching.push(feed),
                Some(_) => {}
                None => unknown.push(name),
            }
        }
        self.feeds.retain(|(feed, task)| {
            let kept = watching.iter().any(|f| f.name == feed.name);
            if !kept {
                task.abort();
            }
            kept
        });
        for feed in &watching {
            if !self.feeds.iter().any(|(f, _)| f.name == feed.name) {
                self.feeds
                    .push((feed, tokio::spawn(push(self.context.clone(), feed))));
            }
        }
        let names: Vec<&str> = watching.iter().map(|feed| feed.name).collect();
        let answer = json!({"type":"watching","feeds":names,"unknown":unknown});
        let _ = self.context.outbox.text(answer.to_string());
    }
}

impl Drop for Lane {
    /// The connection ended: its feeds and its events stop. A call still running finishes, and its
    /// outcome is kept for a reconnect to ask after.
    fn drop(&mut self) {
        self.events.abort();
        for (_, task) in &self.feeds {
            task.abort();
        }
    }
}

/// A call waiting, counted against [`PENDING`] until it is answered however it ends.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What the app's own handler answers `cmd` with `args`, on this thread, or `None` if it gave no
/// answer at all (it dropped it, or the command panicked).
fn run(runner: &Runner, cmd: &str, args: Value) -> Option<Result<Value, String>> {
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner(
            cmd.to_string(),
            args,
            Box::new(move |result| {
                let _ = answer.send(result);
            }),
        )
    }));
    ran.ok()?;
    answered.recv().ok()
}

/// A call's answer: its value or its error, with the revision of the store it read or saved; a
/// call whose answer never came is answered as lost. An answer over [`wire::ANSWER_BYTES`] goes as
/// that error instead.
fn answered(id: u64, result: Option<Result<Value, String>>, revision: Option<&str>) -> Whole {
    let mut answer = match result {
        Some(Ok(value)) => json!({"type":"result","id":id,"value":value}),
        Some(Err(error)) => json!({"type":"result","id":id,"error":error}),
        None => return repeated(id, &Outcome::Lost),
    };
    if let Some(revision) = revision {
        answer["revision"] = json!(revision);
    }
    let text = answer.to_string();
    if text.len() > wire::ANSWER_BYTES {
        let error = json!({"type":"result","id":id,"error":wire::ANSWER_TOO_LARGE});
        return Whole::Text(error.to_string());
    }
    Whole::Text(text)
}

/// A refused call: nothing ran.
fn refused(id: u64, refusal: Refusal) -> Whole {
    Whole::Text(json!({"type":"result","id":id,"refused":refusal.code()}).to_string())
}

/// A call whose id was used before: what became of it, and it did not run again.
fn repeated(id: u64, outcome_of: &Outcome) -> Whole {
    let mut answer = outcome(outcome_of);
    answer["type"] = json!("result");
    answer["id"] = json!(id);
    Whole::Text(answer.to_string())
}

/// An outcome as the wire says it.
fn outcome(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Running => json!({"outcome":"running"}),
        Outcome::Done => json!({"outcome":"done"}),
        Outcome::Failed(error) => json!({"outcome":"failed","error":error}),
        Outcome::Refused(refusal) => json!({"outcome":"refused","refused":refusal.code()}),
        Outcome::Lost => json!({"outcome":"lost"}),
        Outcome::Unknown => json!({"outcome":"unknown"}),
    }
}

/// One watched feed: its command run at its pace while this computer holds control, and each
/// value pushed (every one, or each change).
async fn push(context: Arc<Context>, feed: &'static Feed) {
    let mut tick = tokio::time::interval(feed.every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last: Option<Vec<u8>> = None;
    loop {
        tick.tick().await;
        // When this value's state began to be read: a value read slowly is the older for it.
        let taken = Instant::now();
        let ran = {
            let context = context.clone();
            tokio::task::spawn_blocking(move || {
                if !context.controls() {
                    return None;
                }
                run(&context.runner, feed.command, json!({}))
            })
            .await
        };
        let Ok(Some(Ok(value))) = ran else {
            continue;
        };
        let Ok(json) = serde_json::to_vec(&value) else {
            continue;
        };
        if feed.always || last.as_deref() != Some(json.as_slice()) {
            last = Some(json.clone());
            context.outbox.feed(feed.name, json, taken);
        }
    }
}

/// Events, on to this computer: each one that crosses, while it holds control.
async fn forward(context: Arc<Context>, mut events: broadcast::Receiver<Event>) {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if !forwards(event.name) {
            continue;
        }
        let asked = context.clone();
        if !tokio::task::spawn_blocking(move || asked.controls())
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        let message = json!({"type":"event","name":event.name,"id":event.id,"at":event.at_ms,
            "data":data});
        let _ = context.outbox.text(message.to_string());
    }
}
