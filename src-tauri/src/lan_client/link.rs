//! The command link, this computer's end: the road to a paired station at protocol 2, for client
//! mode, which hands every station command its screens make to [`Link::call`] and answers their
//! polls from what the station pushes ([`Link::latest`]). The other end is the shack's command lane
//! (`remote_service::lan::commands`); the wire is `remote_service::lan::wire`'s.
//!
//! 1. **The road, and the same build** (the operator's ruling of 2026-10-05, "the same version at
//!    both ends"). The window's road reaches the station ([`super::road::reach`]: TLS 1.3 with the
//!    station's key pinned, the hello, the welcome). The welcome names the station's app and
//!    contract, and anything but this build's is refused here, saying which side to update, before
//!    this computer opens anything; the station checks the `open` the same way.
//! 2. **Calls.** Each has an id that rises across every link of this run of Nexus ([`run_id`]), so
//!    the station's journal runs each at most once whichever link carried it. The dispatcher routes
//!    each first (`command_dispatch::route`): one that runs on this computer, or is refused here,
//!    never leaves it. A Stop, or the off half of a transmit switch, goes ahead of everything
//!    waiting to be written, and a call bigger than a frame goes in pieces, so a Stop goes between
//!    them. A link that ends with calls in flight answers them [`Answer::Gone`]; the next one asks
//!    after them ([`Link::outcomes`]), and the station says what became of each.
//! 3. **Feeds** arrive numbered, and each value must carry the next number of its feed: a gap or a
//!    step back means this computer's copy is no longer the station's, and the link ends.
//! 4. **Events** that cross, for the screens ([`Link::events`]).
//! 5. **The lease, and Stop's first road**: protocol 1's operation requests ([`Link::operation`]),
//!    a `stopTransmit` ahead of everything.
//! 6. **Stop's second road** ([`Link::stop_road`]), when the welcome names one: a road of its own
//!    beside this link (`super::stop_road`), keyed from this link's TLS session. It is told when the
//!    link ends, and outlives it by a minute for a Stop pressed as it drops.
//! 7. **The audio road** ([`Link::audio_road`]), when the welcome names one and this computer has a
//!    headset: the station's audio and this computer's microphone (`super::audio_road`), keyed
//!    under its own label. It ends with the link. The station's status line says whether its rig is
//!    keyed, which mutes the station's audio here (M9), kept with the over's state ([`OnAir`]).
//!
//! Anything the station sends that no station of this build sends ends the link: it fails closed.
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use futures_util::stream::SplitStream;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tempo_stream::protocol;
use tokio::sync::{broadcast, oneshot, Notify};
use tokio_tungstenite::tungstenite::Message;

use super::audio_road::{AudioRoad, Headset};
use super::road::{self, Ids, Socket};
use super::stop_road::{OnAir, StopRoad};
use crate::command_dispatch::{forwards, is_stop, route, Outcome, Profile, Refusal, Route};
use crate::remote_service::lan::commands::OUTCOMES;
use crate::remote_service::lan::datagram;
use crate::remote_service::lan::outbox::Outbox;
use crate::remote_service::lan::wire::{self, Assembly, End, Inflate, Whole};
use crate::remote_service::vault::PairedStation;

/// The operation requests the link carries, as the window's road does.
const OPERATIONS: [&str; 5] = ["state", "acquire", "heartbeat", "release", "stopTransmit"];
/// Events kept for a screen that has fallen behind.
const EVENTS: usize = 64;

/// This run of Nexus, as the station's journal knows it: made once, at random, never 0.
pub(crate) fn run_id() -> u64 {
    static RUN: OnceLock<u64> = OnceLock::new();
    *RUN.get_or_init(|| {
        use ring::rand::SecureRandom;
        let mut bytes = [0; 8];
        if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
            // No randomness: the clock, which no other run of this computer's shares either.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            bytes = (now.as_nanos() as u64).to_le_bytes();
        }
        u64::from_le_bytes(bytes).max(1)
    })
}

/// The next call id of this run: rising across every link it opens.
fn next_id() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::SeqCst) + 1
}

/// What became of a call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Answer {
    /// It ran, and this is what it answered, with the revision of the store it read or saved.
    Value {
        value: Value,
        revision: Option<String>,
    },
    /// It ran, and failed with the command's own error.
    Error(String),
    /// It was refused, at the station or on this computer, and nothing ran.
    Refused(Refusal),
    /// Its id was used before, and this is what became of it then; it did not run again.
    Before(Outcome),
    /// Bigger than the station takes ([`wire::CALL_BYTES`]): nothing was sent.
    TooLarge,
    /// The link ended before its answer came: ask after it on the next link.
    Gone,
}

/// An event raised at the station, as it arrived.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub(crate) name: String,
    pub(crate) id: u64,
    pub(crate) at_ms: u64,
    pub(crate) data: Value,
}

/// One feed as this computer holds it: the number of the last value, its deflate stream, and the
/// value with its number.
#[derive(Default)]
struct Feed {
    last: u64,
    inflate: Inflate,
    latest: Option<(u64, Value)>,
}

/// A question after outcomes, waiting: the ids asked, and where the answer goes.
type Asked = (Vec<u64>, oneshot::Sender<Vec<Outcome>>);

/// What the link's tasks and its callers share.
struct Shared {
    calls: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    operations: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    asked: Mutex<VecDeque<Asked>>,
    feeds: Mutex<HashMap<&'static str, Feed>>,
    watching: Mutex<Option<Value>>,
    events: broadcast::Sender<Event>,
    ended: AtomicBool,
    end: Notify,
    /// This link's Stop road, told when the link ends.
    road: Option<Arc<StopRoad>>,
    /// This link's audio road, told when the link ends.
    audio: Option<Arc<AudioRoad>>,
    /// Whether this computer's over may be on the air: the status line's rig keyed goes here.
    on_air: Arc<OnAir>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A paired station's command link, open.
pub(crate) struct Link {
    outbox: Outbox,
    shared: Arc<Shared>,
    tasks: Vec<tokio::task::AbortHandle>,
    pub(crate) ids: Ids,
}

impl Link {
    /// The link to the station `record` names, at `typed` first when the operator gave an address,
    /// for the run `run` ([`run_id`] in the app), its audio on `headset` when this computer has one:
    /// the link and the address that answered, or the most telling reason it did not open (the
    /// road's, `updateStation`, `updateComputer`, `otherBuild`, or `notStation` for a station that
    /// said something no station says).
    pub(crate) async fn connect(
        record: &PairedStation,
        typed: Option<SocketAddrV4>,
        run: u64,
        headset: Option<Arc<dyn Headset>>,
    ) -> Result<(Self, SocketAddrV4), &'static str> {
        let road::Reached {
            mut socket,
            ids,
            app,
            contract,
            stop_road,
            audio_road,
            at,
        } = road::reach(record, &road::order(record, typed)).await?;
        let on_air = Arc::new(OnAir::default());
        let ours = (wire::APP, wire::contract());
        if let Some(reason) = wire::refusal(End::Computer, ours, (&app, &contract)) {
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
            return Err(reason);
        }
        let open = json!({"type":"open","profile":Profile::Desktop.name(),"app":ours.0,
            "contract":ours.1,"run":run});
        road::say(&mut socket, open.to_string())
            .await
            .map_err(|_| "unreachable")?;
        let said = tokio::time::timeout(road::HANDSHAKE, first_word(&mut socket, &on_air))
            .await
            .ok()
            .flatten();
        match said.as_ref().and_then(|said| said["type"].as_str()) {
            Some("opened") => {}
            Some("refused") => {
                return Err(
                    match said.as_ref().and_then(|said| said["reason"].as_str()) {
                        Some("updateStation") => "updateStation",
                        Some("updateComputer") => "updateComputer",
                        Some("otherBuild") => "otherBuild",
                        _ => "notStation",
                    },
                )
            }
            _ => return Err("notStation"),
        }
        // Stop's second road, from this session's own keys: at the address that welcomed this
        // computer, at the port and under the tag the welcome named. Without one, Stop has this link.
        let stop_road = stop_road.and_then(|offer| {
            let tag = offer.tag()?;
            let exported = socket
                .get_ref()
                .get_ref()
                .1
                .export_keying_material(
                    [0; datagram::EXPORTED],
                    datagram::STOP_LABEL,
                    Some(ids.session.as_bytes()),
                )
                .ok()?;
            StopRoad::open(
                SocketAddrV4::new(*at.ip(), offer.port).into(),
                tag,
                &exported,
                on_air.clone(),
            )
        });
        // The audio road the same way, under its own label, when this computer has a headset.
        let audio_road = audio_road.zip(headset).and_then(|(offer, headset)| {
            let tag = offer.tag()?;
            let exported = socket
                .get_ref()
                .get_ref()
                .1
                .export_keying_material(
                    [0; datagram::EXPORTED],
                    datagram::AUDIO_LABEL,
                    Some(ids.session.as_bytes()),
                )
                .ok()?;
            AudioRoad::open(
                SocketAddrV4::new(*at.ip(), offer.port).into(),
                tag,
                &exported,
                headset,
                on_air.clone(),
            )
        });
        let (sink, stream) = socket.split();
        let (outbox, writer) = Outbox::spawn(sink);
        let shared = Arc::new(Shared::new(stop_road, audio_road, on_air));
        let writing = writer.abort_handle();
        let reader = tokio::spawn(read(stream, shared.clone()));
        let keeper = tokio::spawn(keep(outbox.clone(), writer, shared.clone()));
        let tasks = vec![writing, reader.abort_handle(), keeper.abort_handle()];
        Ok((
            Self {
                outbox,
                shared,
                tasks,
                ids,
            },
            at,
        ))
    }

    /// One station command, with the revision of the copy a whole save was made from: its id in
    /// this run, and what became of it. Routed first, deny by default: a command that runs on this
    /// computer, or is refused here, is answered here and never sent. The ui-state pair goes as
    /// given, already split (`command_dispatch::split_ui_state`).
    pub(crate) async fn call(&self, cmd: &str, args: Value, base: Option<&str>) -> (u64, Answer) {
        let id = next_id();
        match route(cmd, &args) {
            Route::Shack | Route::Split => {}
            Route::Here => return (id, Answer::Refused(Refusal::RunsHere)),
            Route::Refused(refusal) => return (id, Answer::Refused(refusal)),
        }
        let urgent = is_stop(cmd, &args);
        let mut message = json!({"type":"call","id":id,"cmd":cmd,"args":args});
        if let Some(base) = base {
            message["base"] = json!(base);
        }
        let text = message.to_string();
        if text.len() > wire::CALL_BYTES {
            return (id, Answer::TooLarge);
        }
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.calls).insert(id, answer);
        let sent = if self.shared.ended.load(Ordering::SeqCst) {
            Err(())
        } else if urgent {
            self.outbox.urgent(Whole::Text(text))
        } else {
            self.outbox.send(Whole::Text(text))
        };
        if sent.is_err() {
            lock(&self.shared.calls).remove(&id);
            return (id, Answer::Gone);
        }
        (id, answered.await.unwrap_or(Answer::Gone))
    }

    /// What became of each of `ids` (calls of this run, from a link that ended), in the order
    /// asked; `None` once this link has ended too, or for more ids than one question may ask.
    pub(crate) async fn outcomes(&self, ids: &[u64]) -> Option<Vec<Outcome>> {
        if ids.is_empty() || ids.len() > OUTCOMES {
            return None;
        }
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.asked).push_back((ids.to_vec(), answer));
        if self.shared.ended.load(Ordering::SeqCst) {
            return None;
        }
        let asked = json!({"type":"outcomes","ids":ids});
        self.outbox.text(asked.to_string()).ok()?;
        answered.await.ok()
    }

    /// One of protocol 1's operation requests (the lease's, and Stop): the station's whole
    /// `operationResponse`, or `None` for a request the link does not carry or once it has ended.
    /// A Stop goes ahead of everything waiting.
    pub(crate) async fn operation(&self, request: Value) -> Option<Value> {
        let kind = request["type"]
            .as_str()
            .filter(|kind| OPERATIONS.contains(kind))?;
        let urgent = kind == "stopTransmit";
        let request_id = request["requestId"]
            .as_str()
            .filter(|id| protocol::identifier(id))?
            .to_string();
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.operations).insert(request_id.clone(), answer);
        let text = json!({"type":"operationRequest","request":request}).to_string();
        let sent = if self.shared.ended.load(Ordering::SeqCst) {
            Err(())
        } else if urgent {
            self.outbox.urgent(Whole::Text(text))
        } else {
            self.outbox.text(text)
        };
        if sent.is_err() {
            lock(&self.shared.operations).remove(&request_id);
            return None;
        }
        answered.await.ok()
    }

    /// Watch these feeds from now on, in place of the ones before. The station answers with the
    /// ones it has ([`Link::watching`]).
    pub(crate) fn watch(&self, feeds: &[&str]) -> bool {
        let watch = json!({"type":"watch","feeds":feeds});
        self.outbox.text(watch.to_string()).is_ok()
    }

    /// The station's last answer to a watch: the feeds it pushes, and the names it has no feed for.
    pub(crate) fn watching(&self) -> Option<Value> {
        lock(&self.shared.watching).clone()
    }

    /// The newest value of a feed, with its number.
    pub(crate) fn latest(&self, feed: &str) -> Option<(u64, Value)> {
        lock(&self.shared.feeds)
            .get(feed)
            .and_then(|feed| feed.latest.clone())
    }

    /// The station's events from now on.
    pub(crate) fn events(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    /// This link's Stop road, when the station opened one.
    pub(crate) fn stop_road(&self) -> Option<Arc<StopRoad>> {
        self.shared.road.clone()
    }

    /// This link's audio road, when the station opened one and this computer has a headset.
    pub(crate) fn audio_road(&self) -> Option<Arc<AudioRoad>> {
        self.shared.audio.clone()
    }

    /// Whether the link has ended.
    pub(crate) fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::SeqCst)
    }

    /// Waits until the link has ended.
    pub(crate) async fn closed(&self) {
        loop {
            let ending = self.shared.end.notified();
            if self.ended() {
                return;
            }
            ending.await;
        }
    }
}

impl Link {
    /// Closes the link: its socket goes with its tasks, and every call still waiting is answered
    /// gone.
    pub(crate) fn close(&self) {
        for task in &self.tasks {
            task.abort();
        }
        self.shared.end();
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

/// The station's first word after the `open`, the status line kept aside (`on_air`): `None` once
/// it has gone.
async fn first_word(socket: &mut Socket, on_air: &OnAir) -> Option<Value> {
    loop {
        match socket.next().await? {
            Ok(Message::Text(text)) => {
                let said: Value = serde_json::from_str(&text).ok()?;
                if said["type"] != "status" {
                    return Some(said);
                }
                on_air.rig(said["rigKeyed"].as_bool());
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            _ => return None,
        }
    }
}

/// Everything the station sends, until the link ends.
async fn read(mut stream: SplitStream<Socket>, shared: Arc<Shared>) {
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    loop {
        let whole = match stream.next().await {
            Some(Ok(Message::Text(text))) => Whole::Text(text.to_string()),
            Some(Ok(Message::Binary(frame))) if wire::is_piece(&frame) => {
                match pieces.piece(&frame) {
                    Ok(Some(whole)) => whole,
                    Ok(None) => continue,
                    Err(()) => break,
                }
            }
            Some(Ok(Message::Binary(frame))) => Whole::Binary(frame.to_vec()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => break,
        };
        if shared.heard(whole).is_err() {
            break;
        }
    }
    shared.end();
}

/// The link's keeper: a ping every [`road::PING_EVERY`], so the station never takes this computer
/// for gone, and the link's end when its writing ends.
async fn keep(outbox: Outbox, mut writer: tokio::task::JoinHandle<()>, shared: Arc<Shared>) {
    let mut every = tokio::time::interval(road::PING_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    every.tick().await;
    loop {
        tokio::select! {
            _ = &mut writer => break,
            _ = every.tick() => if outbox.ping().is_err() { break },
        }
    }
    shared.end();
}

impl Shared {
    fn new(road: Option<Arc<StopRoad>>, audio: Option<Arc<AudioRoad>>, on_air: Arc<OnAir>) -> Self {
        Self {
            calls: Default::default(),
            operations: Default::default(),
            asked: Default::default(),
            feeds: Default::default(),
            watching: Default::default(),
            events: broadcast::channel(EVENTS).0,
            ended: AtomicBool::new(false),
            end: Notify::new(),
            road,
            audio,
            on_air,
        }
    }

    /// The link has ended: every call still waiting is answered gone, and every other question
    /// is dropped, which its asker reads the same way. Its Stop road stays a minute more; its audio
    /// road goes now.
    fn end(&self) {
        self.ended.store(true, Ordering::SeqCst);
        if let Some(road) = &self.road {
            road.link_ended();
        }
        if let Some(audio) = &self.audio {
            audio.link_ended();
        }
        for (_, waiting) in lock(&self.calls).drain() {
            let _ = waiting.send(Answer::Gone);
        }
        lock(&self.operations).clear();
        lock(&self.asked).clear();
        self.end.notify_waiters();
    }

    /// One message from the station. `Err` for anything no station of this build sends.
    fn heard(&self, whole: Whole) -> Result<(), ()> {
        let text = match whole {
            Whole::Binary(frame) => return self.feed(&frame),
            Whole::Text(text) => text,
        };
        let message: Value = serde_json::from_str(&text).map_err(|_| ())?;
        match message["type"].as_str().ok_or(())? {
            "result" => {
                let id = message["id"].as_u64().ok_or(())?;
                let answer = answer_of(&message).ok_or(())?;
                // An answer nobody waits for any more is let go.
                let waiting = lock(&self.calls).remove(&id);
                if let Some(waiting) = waiting {
                    let _ = waiting.send(answer);
                }
            }
            "outcomes" => {
                let ids: Vec<u64> =
                    serde_json::from_value(message["ids"].clone()).map_err(|_| ())?;
                let outcomes = message["outcomes"]
                    .as_array()
                    .ok_or(())?
                    .iter()
                    .map(outcome_of)
                    .collect::<Option<Vec<_>>>()
                    .filter(|outcomes| outcomes.len() == ids.len())
                    .ok_or(())?;
                let mut asked = lock(&self.asked);
                if let Some(at) = asked.iter().position(|(asked, _)| *asked == ids) {
                    if let Some((_, waiting)) = asked.remove(at) {
                        let _ = waiting.send(outcomes);
                    }
                }
            }
            "operationResponse" => {
                let id = message["requestId"].as_str().ok_or(())?;
                let waiting = lock(&self.operations).remove(id);
                if let Some(waiting) = waiting {
                    let _ = waiting.send(message);
                }
            }
            "watching" => *lock(&self.watching) = Some(message),
            "event" => {
                let name = message["name"]
                    .as_str()
                    .filter(|name| forwards(name))
                    .ok_or(())?;
                let event = Event {
                    name: name.to_string(),
                    id: message["id"].as_u64().ok_or(())?,
                    at_ms: message["at"].as_u64().ok_or(())?,
                    data: message["data"].clone(),
                };
                let _ = self.events.send(event);
            }
            // The rig keyed mutes the station's audio here (M9); `null` when it cannot tell.
            "status" => self.on_air.rig(message["rigKeyed"].as_bool()),
            _ => return Err(()),
        }
        Ok(())
    }

    /// A feed's value: kept, if it carries the next number of its feed, and the link ended
    /// otherwise, since a gap or a step back is a copy that is no longer the station's.
    fn feed(&self, frame: &[u8]) -> Result<(), ()> {
        let (name, number, deflated) = wire::read_feed(frame).ok_or(())?;
        let feed = wire::feed(name).ok_or(())?;
        let mut feeds = lock(&self.feeds);
        let held = feeds.entry(feed.name).or_default();
        if number != held.last + 1 {
            return Err(());
        }
        let json = held.inflate.value(deflated).ok_or(())?;
        let value: Value = serde_json::from_slice(&json).map_err(|_| ())?;
        held.last = number;
        held.latest = Some((number, value));
        Ok(())
    }
}

/// A call's answer as the station wrote it.
fn answer_of(message: &Value) -> Option<Answer> {
    if message.get("outcome").is_some() {
        return outcome_of(message).map(Answer::Before);
    }
    if let Some(value) = message.get("value") {
        let revision = message["revision"].as_str().map(str::to_string);
        return Some(Answer::Value {
            value: value.clone(),
            revision,
        });
    }
    if let Some(error) = message["error"].as_str() {
        return Some(Answer::Error(error.to_string()));
    }
    Refusal::named(message["refused"].as_str()?).map(Answer::Refused)
}

/// An outcome as the station wrote it.
fn outcome_of(message: &Value) -> Option<Outcome> {
    Some(match message["outcome"].as_str()? {
        "running" => Outcome::Running,
        "done" => Outcome::Done,
        "failed" => Outcome::Failed(message["error"].as_str()?.to_string()),
        "refused" => Outcome::Refused(Refusal::named(message["refused"].as_str()?)?),
        "lost" => Outcome::Lost,
        "unknown" => Outcome::Unknown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::lan::wire::Deflate;

    /// The station's frames for values of `feed` numbered as given, from one deflate stream.
    fn frames(feed: &str, numbers: &[u64]) -> Vec<Vec<u8>> {
        let mut deflate = Deflate::default();
        numbers
            .iter()
            .map(|number| {
                let json = format!("{{\"n\":{number}}}");
                wire::feed_frame(feed, *number, &deflate.value(json.as_bytes()).unwrap())
            })
            .collect()
    }

    /// ★ The feeds' numbers, at this end: each value is kept only when it carries the next number
    /// of its feed, from 1. A gap, a repeat or a step back, a first value numbered past 1, a feed
    /// this build has not, and a value that does not inflate each end the link. CONTROL: 1, 2, 3
    /// are each kept, the newest with its number.
    #[test]
    fn a_feed_value_out_of_its_order_ends_the_link() {
        let shared = Shared::new(None, None, Arc::default());
        for frame in frames("snapshot/1", &[1, 2, 3]) {
            assert_eq!(shared.heard(Whole::Binary(frame)), Ok(()));
        }
        assert_eq!(
            lock(&shared.feeds)["snapshot/1"].latest,
            Some((3, json!({"n": 3})))
        );
        for (numbers, why) in [
            (&[1, 3][..], "a gap"),
            (&[1, 2, 2][..], "a repeat"),
            (&[1, 2, 1][..], "a step back"),
            (&[2][..], "a first value past 1"),
        ] {
            let shared = Shared::new(None, None, Arc::default());
            let mut heard: Vec<Result<(), ()>> = frames("meters/1", numbers)
                .into_iter()
                .map(|frame| shared.heard(Whole::Binary(frame)))
                .collect();
            assert_eq!(heard.pop(), Some(Err(())), "{why}");
            assert!(heard.iter().all(Result::is_ok), "{why}");
        }
        let shared = Shared::new(None, None, Arc::default());
        let unknown = frames("noSuchFeed/1", &[1]).remove(0);
        assert_eq!(
            shared.heard(Whole::Binary(unknown)),
            Err(()),
            "no such feed"
        );
        let garbage = wire::feed_frame("snapshot/1", 1, &[0xff; 16]);
        assert_eq!(
            shared.heard(Whole::Binary(garbage)),
            Err(()),
            "no deflate stream"
        );
    }

    /// ★ Only what a station of this build says is taken: a result of no known shape, a refusal
    /// with an unknown code, an event the dispatcher does not forward and a message of no known
    /// type each end the link. CONTROL: an answer, a repeat's outcome, a forwarded event and the
    /// status line are taken.
    #[test]
    fn a_message_no_station_sends_ends_the_link() {
        let shared = Shared::new(None, None, Arc::default());
        let said = |value: Value| shared.heard(Whole::Text(value.to_string()));
        assert_eq!(
            said(json!({"type":"result","id":1,"value":{"ok":true}})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"result","id":2,"outcome":"done"})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"result","id":3,"refused":"busy"})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"event","name":"pounce","id":1,"at":5,"data":{}})),
            Ok(())
        );
        assert_eq!(said(json!({"type":"status","rigKeyed":null})), Ok(()));
        for wrong in [
            json!({"type":"result","id":4}),
            json!({"type":"result","id":5,"refused":"noSuchRefusal"}),
            json!({"type":"result","id":6,"outcome":"maybe"}),
            json!({"type":"event","name":"update-progress","id":1,"at":5,"data":{}}),
            json!({"type":"somethingElse"}),
        ] {
            assert_eq!(said(wrong.clone()), Err(()), "{wrong}");
        }
    }
}
