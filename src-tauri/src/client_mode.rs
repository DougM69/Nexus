//! Client mode: this computer's Nexus driving a paired station's over this network, with its own
//! station off. "Here" is the computer the operator sits at; "the shack" is the one with the radio.
//!
//! The operator picks a paired station in Stations on this network and presses Operate from this
//! computer. Nexus quits as itself, the way closing its window quits (the radio stopped first, the
//! logbook's last changes saved), and starts again with `--client <station id>` ([`ARG`]); Leave
//! starts it as itself again. A start, never a switch in place, so no half-stopped radio loop is
//! left behind.
//!
//! ## No station here
//!
//! A launch in client mode branches off in `run` (`lib.rs`) before anything of this computer's own
//! station is opened: no settings opened for writing, no logbook, no Engine, no radio loop, no
//! rigctld, no audio device, no CAT broker, no WSJT-X UDP, no cluster or spot feed, no upload and
//! no listener. The app [`run`] builds manages no Engine at all, so a command that reached this
//! computer's own handler could find none: it would fail, and never run on a station here. What it
//! does open is this computer's: its windows, the keys of its own `ui-state.json` that are not the
//! station's, and the two fields of its `settings.json` that belong to it, the update channel and
//! start at sign-in ([`ThisComputer`]). A test holds the module to the short list of the crate's
//! own functions it may call, each of which starts nothing.
//!
//! ## Every command by its class, failing closed
//!
//! The app's own handler is wrapped ([`Client::handle`]), so every IPC call the screens make passes
//! here, whichever window made it. [`way`] decides, from `command_dispatch::route` and the class
//! table:
//! - **here**: windows, links, notifications, the files this computer saves, its updater and street
//!   map, through the app's own handler;
//! - **the shack**: every station command, over the command link, with the revision of the copy a
//!   save was made from; at most [`IN_FLIGHT`] at once and [`PER_COMMAND`] of one command, the rest
//!   waiting here for a slot ([`Caps`]);
//! - **answered here**: the snapshot, the waterfall's rows and the meters, from what the shack
//!   pushes, so the screens' polls never cross the network; the update channel and start at
//!   sign-in, which are this computer's; and the launch picker's question, which is about which
//!   radio this Nexus drives, and so is never the shack's picker;
//! - **split**: the ui-state pair, the station's keys at the shack and every other key here (the
//!   operator's ruling of 2026-10-07: the rail, its order, the UI scale and the window layouts stay
//!   on the computer the operator sits at);
//! - **refused here**, with nothing sent: a command with no class, one only the shack's own screen
//!   may give, a password typed here, and, until the stream's transmit rules are wired for this
//!   road, every command that starts a transmission. The shack refuses each of those again itself.
//!
//! ## Stop is never held
//!
//! A Stop, and the off half of a transmit switch, takes no slot and waits for none, and the link
//! writes it ahead of everything waiting. With no link it cannot reach the shack, and says so at
//! once rather than waiting for one.
//!
//! A Stop press (a command of the stop class) also goes by Stop's second road
//! (`crate::lan_client::stop_road`): the handler writes it on that road's socket itself, before it
//! hands the call to the link, and returns. The road's own thread sends it again until the shack
//! acknowledges it. Each link's road outlives its link by a minute, so a Stop pressed as a link
//! drops still goes. The road's heartbeat names the last snapshot the main window fetched, kept here
//! per window, so a window that hangs lets presence lapse at the shack.
//!
//! ## The link and the lease
//!
//! [`Client::drive`] keeps the link: it connects, takes the lease when control is free, keeps it
//! with a heartbeat every [`HEARTBEAT`], watches the feeds, and hands the shack's events to the
//! screens. When the link drops, the screens keep the shack's last state ("Not live"), and it
//! connects again by itself. A call cut off by a drop is never sent again: the next link asks the
//! shack what became of it.
//!
//! ## What a refusal says
//!
//! A call refused here or at the shack fails with `nexusClient:<code>`, a refusal's code or one of
//! this module's (`notLive`, `gone`, `tooLarge`), and the screen words it in its own language
//! (`ui/src/api.ts`). A command's own error comes back as it would at the shack.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{Emitter, Manager, Runtime};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::command_classes::{class_of, Class};
use crate::command_dispatch::{
    is_stop, join_ui_state, route, split_ui_state, store_access, transmit_from_afar, Refusal,
    Route, Store, IN_FLIGHT, PER_COMMAND,
};
use crate::lan_client::link::{run_id, Answer, Link};
use crate::lan_client::stop_road::StopRoad;
use crate::remote_service::vault::PairedStation;

#[cfg(test)]
pub(crate) mod tests;

/// The argument that starts Nexus in client mode, with the paired station's id after it.
pub(crate) const ARG: &str = "--client";
/// How often the lease is renewed: well inside the shack's five seconds, so a heartbeat refused
/// once while the shack's engine is busy costs nothing.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// How long a link that dropped waits before it is tried again, the first time.
const AGAIN: Duration = Duration::from_secs(2);
/// …and every time after that, until it opens: the shack takes at most ten new connections a
/// minute from one address.
const AGAIN_AFTER: Duration = Duration::from_secs(7);
/// How long a station that refused this computer for a reason that will not pass by itself (the
/// other build, a key that changed, a pairing it no longer knows) waits before it is asked again.
const AGAIN_REFUSED: Duration = Duration::from_secs(30);
/// The reasons a link does not open that only the operator can change.
const LASTING: [&str; 7] = [
    "updateStation",
    "updateComputer",
    "otherBuild",
    "keyChanged",
    "notPaired",
    "notThisStation",
    "notStation",
];
/// How long the main window waits for the shack's state before it shows without it.
const SHOW_BY: Duration = Duration::from_secs(10);
/// The feeds the screens' polls are answered from.
const FEEDS: [&str; 3] = ["snapshot/1", "spectrumRow/1", "meters/1"];
/// The most ids one question after outcomes may carry (the shack's own bound).
const ASK_AT_ONCE: usize = 64;

/// The station this launch drives (`--client <id>` or `--client=<id>`), or none. Only an id a
/// station could have is taken: anything else starts Nexus as itself.
pub(crate) fn requested() -> Option<String> {
    requested_in(std::env::args().skip(1))
}

/// [`requested`], from the arguments given.
fn requested_in(mut args: impl Iterator<Item = String>) -> Option<String> {
    let mut found = None;
    while let Some(arg) = args.next() {
        if arg == ARG {
            found = args.next();
            break;
        } else if let Some(id) = arg.strip_prefix("--client=") {
            found = Some(id.to_string());
            break;
        }
    }
    found.filter(|id| tempo_stream::protocol::identifier(id))
}

// --- Where each command goes -------------------------------------------------------------------

/// Where a command goes from this computer in client mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// Run here, through the app's own handler.
    Here,
    /// Answered here by client mode itself.
    Local(Local),
    /// Sent to the shack. A Stop goes ahead of everything and takes no slot.
    Shack { stop: bool },
    /// The ui-state pair: the station's keys at the shack, every other key here.
    Split,
    /// Refused here, and nothing is sent.
    Refused(Refusal),
}

/// What client mode answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Local {
    /// The shack's newest snapshot, from its feed.
    Snapshot,
    /// A waterfall row from its feed, each one once to each window.
    SpectrumRow,
    /// The shack's meters, from their feed.
    Meters,
    /// This computer's update channel.
    BetaUpdates,
    /// Start this computer's Nexus at sign-in.
    LaunchAtLogin,
    /// Before this computer's updater hands over to its installer: nothing of a station's to
    /// save here, only this computer's window layouts and its diagnostic log.
    BeforeInstall,
    /// This computer's Nexus started again to finish its update, in client mode again.
    Restart,
    /// The launch picker's question: which radio this Nexus drives. A computer driving another
    /// station drives none of its own, so it never shows the shack's picker, whose choices would
    /// change the shack.
    RadioLaunchInfo,
}

/// The way for `cmd` invoked with `args`. `command_dispatch::route` first, deny by default: a
/// command it refuses never goes anywhere. Then what client mode does itself.
pub(crate) fn way(cmd: &str, args: &Value) -> Way {
    match route(cmd, args) {
        Route::Refused(refusal) => Way::Refused(refusal),
        Route::Split => Way::Split,
        Route::Here => match cmd {
            // This computer's, and kept in its own settings.json: the app's handler for them
            // reaches for an Engine, which client mode never has.
            "set_beta_updates" => Way::Local(Local::BetaUpdates),
            "set_launch_at_login" => Way::Local(Local::LaunchAtLogin),
            // The app's own handlers for these two save a logbook and stop a radio first.
            "prepare_update_install" => Way::Local(Local::BeforeInstall),
            "restart_app" => Way::Local(Local::Restart),
            _ => Way::Here,
        },
        Route::Shack => {
            let stop = is_stop(cmd, args);
            // Until the stream's transmit rules are wired for this road, a start never leaves this
            // computer; the shack refuses one too, by the same switch. The off half of a switch is
            // a Stop, and goes.
            if !stop && class_of(cmd) == Some(Class::StartsTx) {
                if let Err(refusal) = transmit_from_afar() {
                    return Way::Refused(refusal);
                }
            }
            match cmd {
                "get_snapshot" => Way::Local(Local::Snapshot),
                "get_spectrum_row" => Way::Local(Local::SpectrumRow),
                "get_meters" => Way::Local(Local::Meters),
                "radio_launch_info" => Way::Local(Local::RadioLaunchInfo),
                _ => Way::Shack { stop },
            }
        }
    }
}

/// A refusal, or another reason a call failed here, as the screen reads it.
pub(crate) fn word(code: &str) -> String {
    format!("nexusClient:{code}")
}

// --- The caps ----------------------------------------------------------------------------------

/// The caps on calls to the shack: [`IN_FLIGHT`] at once from this run, [`PER_COMMAND`] of one
/// command, as the shack's dispatcher counts them. A call over either waits here for a slot, in
/// the order the calls came, so the shack never has to answer `busy`. A Stop takes none.
pub(crate) struct Caps {
    in_flight: Arc<Semaphore>,
    per_command: Mutex<HashMap<String, Arc<Semaphore>>>,
}

/// A call's place under the caps, given back when it is dropped.
pub(crate) struct Slot {
    _of_command: OwnedSemaphorePermit,
    _in_flight: OwnedSemaphorePermit,
}

impl Caps {
    pub(crate) fn new() -> Self {
        Self {
            in_flight: Arc::new(Semaphore::new(IN_FLIGHT)),
            per_command: Mutex::new(HashMap::new()),
        }
    }

    /// A slot for one call of `cmd`, waited for: its command's first, so a call waiting behind
    /// others of its own command holds none of the slots every command shares.
    pub(crate) async fn slot(&self, cmd: &str) -> Slot {
        let mine = lock(&self.per_command)
            .entry(cmd.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_COMMAND)))
            .clone();
        let of_command = mine.acquire_owned().await.expect("a cap is never closed");
        let in_flight = Arc::clone(&self.in_flight)
            .acquire_owned()
            .await
            .expect("a cap is never closed");
        Slot {
            _of_command: of_command,
            _in_flight: in_flight,
        }
    }
}

// --- The shack, as client mode reaches it ------------------------------------------------------

/// A call's answer, on its way.
pub(crate) type Answering = Pin<Box<dyn Future<Output = (u64, Answer)> + Send>>;

/// Where client mode finds the paired station's record by its id: this computer's store in the
/// app, the test's own in a test. The store can block, so it is asked off the async threads.
pub(crate) type Find =
    Arc<dyn Fn(&str) -> Result<Option<PairedStation>, &'static str> + Send + Sync>;

/// The shack, as client mode reaches it: the command link in the app, a stand-in in a test.
pub(crate) trait Shack: Send + Sync + 'static {
    /// One call, with the revision of the copy a save was made from: its id, and what became of it.
    fn call(self: Arc<Self>, cmd: String, args: Value, base: Option<String>) -> Answering;
    /// A feed's newest value, with its number.
    fn latest(&self, feed: &str) -> Option<(u64, Value)>;
    /// This link's Stop road, when the shack opened one.
    fn stop_road(&self) -> Option<Arc<StopRoad>> {
        None
    }
}

impl Shack for Link {
    fn call(self: Arc<Self>, cmd: String, args: Value, base: Option<String>) -> Answering {
        Box::pin(async move { Link::call(&self, &cmd, args, base.as_deref()).await })
    }

    fn latest(&self, feed: &str) -> Option<(u64, Value)> {
        Link::latest(self, feed)
    }

    fn stop_road(&self) -> Option<Arc<StopRoad>> {
        Link::stop_road(self)
    }
}

// --- This computer's own ------------------------------------------------------------------------

/// What client mode keeps of this computer's own: the two settings that are this computer's
/// (`betaUpdates`, `launchAtLogin`), shown in place of the shack's and changed only here, and where
/// its own files are.
pub(crate) struct ThisComputer {
    /// This computer's `settings.json`. Only the two fields are ever written to it, and only when
    /// the operator changes one.
    settings: PathBuf,
    /// This computer's `ui-state.json`: the keys that are not the station's.
    ui_state: PathBuf,
    beta_updates: bool,
    launch_at_login: bool,
}

impl ThisComputer {
    /// This computer's, read from its own files. The settings are read as JSON and never loaded
    /// whole: a load could set a file it cannot read aside, and client mode writes nothing it was
    /// not asked to.
    pub(crate) fn read(settings: PathBuf, ui_state: PathBuf) -> Self {
        let read: Value = std::fs::read_to_string(&settings)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            beta_updates: read["betaUpdates"].as_bool().unwrap_or(false),
            launch_at_login: read["launchAtLogin"].as_bool().unwrap_or(false),
            settings,
            ui_state,
        }
    }

    /// The shack's settings with this computer's own two in place of the shack's.
    fn over(&self, settings: &mut Value) {
        if let Some(fields) = settings.as_object_mut() {
            fields.insert("betaUpdates".into(), json!(self.beta_updates));
            fields.insert("launchAtLogin".into(), json!(self.launch_at_login));
        }
    }

    /// Keeps one of the two in this computer's `settings.json`, through the file's own writer.
    fn keep(&mut self, which: Local, on: bool) -> Result<(), String> {
        tempo_app::settings::writer::SettingsWriter::of(&self.settings)
            .rewrite(|s| {
                match which {
                    Local::BetaUpdates => s.beta_updates = on,
                    _ => s.launch_at_login = on,
                }
                true
            })
            .map_err(|e| e.to_string())?;
        match which {
            Local::BetaUpdates => self.beta_updates = on,
            _ => self.launch_at_login = on,
        }
        Ok(())
    }
}

// --- The client ----------------------------------------------------------------------------------

/// How the link to the station stands, for the screens' banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    /// Not reached yet.
    Connecting,
    /// This computer holds control and the shack's state is arriving.
    Live,
    /// The link dropped; the screens hold the shack's last state while it connects again.
    NotLive,
    /// Another device holds control of the station.
    Busy,
    /// The station would not take this computer: the reason says why.
    Refused,
}

/// The link's state as the banner shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    /// Where the station answered, or was last known to: its address and port.
    pub(crate) station: String,
    pub(crate) phase: Phase,
    /// For a refusal, or a link that did not open: its code.
    pub(crate) reason: Option<String>,
}

/// Client mode's state: the link, the caps, each window's revisions and rows, and this computer's
/// own settings.
pub(crate) struct Client {
    station_id: String,
    status: Mutex<Status>,
    shack: Mutex<Option<Arc<dyn Shack>>>,
    caps: Caps,
    /// The revision of the copy each window holds of each store: the answer to its last read of
    /// it, or to its last save the shack took. A save goes with it. The screen does not say which
    /// read a save was made from, so a window that read the store again between making a copy and
    /// saving it sends the newer revision.
    revisions: Mutex<HashMap<(String, Store), String>>,
    /// The number of the last waterfall row each window was given, on this link.
    rows: Mutex<HashMap<String, u64>>,
    /// Each feed's last value, kept for the screens while the link is down.
    last: Mutex<HashMap<&'static str, Value>>,
    this: Mutex<ThisComputer>,
    /// Calls cut off by a link that dropped, asked after on the next one.
    gone: Mutex<Vec<u64>>,
    /// The shack's state has reached this computer since this run began.
    live_once: AtomicBool,
    /// The link and the lease this computer holds on it, handed back as Nexus quits.
    held: Mutex<Option<(Arc<Link>, String)>>,
    /// Stop's second roads: the link's now, and each earlier link's for its last minute.
    roads: Mutex<Vec<Arc<StopRoad>>>,
    /// The number of the last snapshot each window's poll fetched, on the link there is now.
    fetched: Mutex<HashMap<String, u64>>,
    /// What the update hand-off saves of this computer's own (its window layouts and log).
    before_install: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl Client {
    pub(crate) fn new(station_id: String, this: ThisComputer) -> Self {
        Self {
            status: Mutex::new(Status {
                station: station_id.clone(),
                phase: Phase::Connecting,
                reason: None,
            }),
            station_id,
            shack: Mutex::new(None),
            caps: Caps::new(),
            revisions: Mutex::new(HashMap::new()),
            rows: Mutex::new(HashMap::new()),
            last: Mutex::new(HashMap::new()),
            this: Mutex::new(this),
            gone: Mutex::new(Vec::new()),
            live_once: AtomicBool::new(false),
            held: Mutex::new(None),
            roads: Mutex::new(Vec::new()),
            fetched: Mutex::new(HashMap::new()),
            before_install: OnceLock::new(),
        }
    }

    /// The link's state, for the banner.
    pub(crate) fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    fn set(&self, phase: Phase, reason: Option<&str>) {
        let mut status = lock(&self.status);
        status.phase = phase;
        status.reason = reason.map(str::to_string);
    }

    fn at(&self, station: String) {
        lock(&self.status).station = station;
    }

    /// The shack as it is reached now, if it is.
    fn shack(&self) -> Option<Arc<dyn Shack>> {
        lock(&self.shack).clone()
    }

    /// A new link to the shack, or none: each window's rows and snapshot numbers start again with
    /// its numbering, and its Stop road joins those a Stop goes on. The roads past their minute go.
    pub(crate) fn install(&self, shack: Option<Arc<dyn Shack>>) {
        lock(&self.rows).clear();
        lock(&self.fetched).clear();
        let now = std::time::Instant::now();
        let mut roads = lock(&self.roads);
        roads.retain(|road| !road.gone(now));
        if let Some(road) = shack.as_ref().and_then(|shack| shack.stop_road()) {
            if !roads.iter().any(|kept| Arc::ptr_eq(kept, &road)) {
                roads.push(road);
            }
        }
        drop(roads);
        *lock(&self.shack) = shack;
    }

    /// A Stop press, on every Stop road that may still reach the shack: written now, on this
    /// thread, beside the Stop the command link carries. The roads past their minute go.
    fn stop_on_the_roads(&self) {
        let now = std::time::Instant::now();
        let mut roads = lock(&self.roads);
        roads.retain(|road| !road.gone(now));
        for road in roads.iter() {
            road.stop();
        }
    }

    /// One IPC call from a window: routed by [`way`], answered here, sent to the shack, or refused.
    /// `local` is the app's own handler.
    pub(crate) fn handle<R: Runtime>(
        self: &Arc<Self>,
        invoke: Invoke<R>,
        local: &dyn Fn(Invoke<R>) -> bool,
    ) -> bool {
        let cmd = invoke.message.command().to_string();
        let args = match invoke.message.payload() {
            InvokeBody::Json(args) => args.clone(),
            // A raw body cannot be routed by its arguments, so it goes nowhere.
            InvokeBody::Raw(_) => {
                invoke.resolver.reject(word(Refusal::BadArguments.code()));
                return true;
            }
        };
        let window = invoke.message.webview_ref().label().to_string();
        match way(&cmd, &args) {
            Way::Here => local(invoke),
            Way::Refused(refusal) => {
                invoke.resolver.reject(word(refusal.code()));
                true
            }
            Way::Local(Local::LaunchAtLogin) => {
                let app = invoke.message.webview_ref().app_handle().clone();
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.launch_at_login(&app, &args).map_err(InvokeError::from)
                });
                true
            }
            Way::Local(Local::Restart) => {
                // Tauri starts this same binary again with the arguments it was started with, so
                // in client mode again; the quit hands the lease back first.
                invoke.message.webview_ref().app_handle().request_restart();
                invoke.resolver.resolve(());
                true
            }
            Way::Local(Local::BeforeInstall) => {
                if let Some(save) = self.before_install.get() {
                    save();
                }
                invoke.resolver.resolve(());
                true
            }
            Way::Local(what @ Local::BetaUpdates) => {
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.local(what, &args, &window).map_err(InvokeError::from)
                });
                true
            }
            Way::Local(what) => {
                let answer = self.local(what, &args, &window);
                invoke
                    .resolver
                    .respond_async(async move { answer.map_err(InvokeError::from) });
                true
            }
            Way::Split => {
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.split(&cmd, &args, &window)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
            Way::Shack { stop } => {
                // A Stop press goes by the second road first, written here before the call is
                // handed to the link, so a link that is stuck or gone holds nothing back. The off
                // half of a switch is not a Stop press: it goes by the link alone.
                if stop && class_of(&cmd) == Some(Class::StopsTx) {
                    self.stop_on_the_roads();
                }
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.at_the_shack(&cmd, args, &window, stop)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
        }
    }

    /// What client mode answers itself, but start at sign-in ([`Client::launch_at_login`]).
    fn local(&self, what: Local, args: &Value, window: &str) -> Result<Value, String> {
        match what {
            Local::Snapshot => self.polled(window),
            Local::Meters => self.fed("meters/1"),
            Local::SpectrumRow => Ok(self.row(window)),
            Local::BetaUpdates | Local::LaunchAtLogin => {
                let on = args["on"]
                    .as_bool()
                    .ok_or_else(|| word(Refusal::BadArguments.code()))?;
                lock(&self.this).keep(what, on)?;
                // The screen reads a falsy answer as a write that failed.
                Ok(self.fed("snapshot/1").unwrap_or_else(|_| json!({})))
            }
            Local::RadioLaunchInfo => Ok(json!({ "showPicker": false, "radios": [] })),
            Local::BeforeInstall | Local::Restart => Ok(Value::Null),
        }
    }

    /// What the update hand-off saves of this computer's own, once the app is built.
    pub(crate) fn before_install(&self, save: Box<dyn Fn() + Send + Sync>) {
        let _ = self.before_install.set(save);
    }

    /// Start at sign-in, on this computer: the operating system's login entry first, and the
    /// choice kept only once it took, as the app's own command does.
    fn launch_at_login<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        args: &Value,
    ) -> Result<Value, String> {
        use tauri_plugin_autostart::ManagerExt;
        let on = args["on"]
            .as_bool()
            .ok_or_else(|| word(Refusal::BadArguments.code()))?;
        let launcher = app.autolaunch();
        let changed = if on {
            launcher.enable()
        } else {
            launcher.disable()
        };
        changed.map_err(|_| "launchAtLoginUnsupported".to_string())?;
        self.local(Local::LaunchAtLogin, args, "")
    }

    /// A feed's newest value, or its last while the link is down; nothing before it first came.
    fn fed(&self, feed: &'static str) -> Result<Value, String> {
        self.newest(self.shack().as_ref(), feed)
            .map(|(_, value)| value)
    }

    /// [`Client::fed`] from `shack`, with the value's number when it came from the link now.
    fn newest(
        &self,
        shack: Option<&Arc<dyn Shack>>,
        feed: &'static str,
    ) -> Result<(Option<u64>, Value), String> {
        let live = shack.and_then(|shack| shack.latest(feed));
        let mut last = lock(&self.last);
        if let Some((number, value)) = live {
            last.insert(feed, value.clone());
            return Ok((Some(number), value));
        }
        last.get(feed)
            .cloned()
            .map(|value| (None, value))
            .ok_or_else(|| word("notLive"))
    }

    /// A window's snapshot poll: [`Client::fed`], and the number of what it fetched kept for that
    /// window. The main window's goes to the link's Stop road, whose heartbeat names it: the screen's
    /// own poll, so a main window that hangs stops it moving.
    fn polled(&self, window: &str) -> Result<Value, String> {
        let shack = self.shack();
        let (number, value) = self.newest(shack.as_ref(), "snapshot/1")?;
        if let (Some(shack), Some(number)) = (&shack, number) {
            lock(&self.fetched).insert(window.to_string(), number);
            if window == "main" {
                if let Some(road) = shack.stop_road() {
                    road.fetched(number);
                }
            }
        }
        Ok(value)
    }

    /// The newest waterfall row, once to each window: a window that has it already, or a link
    /// with none, gets the empty row every display reads as "nothing new this tick", so the
    /// waterfall never scrolls a copy.
    fn row(&self, window: &str) -> Value {
        let latest = self.shack().and_then(|shack| shack.latest("spectrumRow/1"));
        let mut rows = lock(&self.rows);
        match latest {
            Some((number, row)) if rows.get(window).is_none_or(|seen| number > *seen) => {
                rows.insert(window.to_string(), number);
                row
            }
            _ => serde_json::to_value(tempo_app::dto::Spectrum {
                row: Vec::new(),
                lo_hz: 0.0,
                hi_hz: 0.0,
                source: String::new(),
            })
            .unwrap_or_default(),
        }
    }

    /// One call at the shack, under the caps unless it is a Stop.
    async fn at_the_shack(
        self: Arc<Self>,
        cmd: &str,
        args: Value,
        window: &str,
        stop: bool,
    ) -> Result<Value, String> {
        let _slot = if stop {
            None
        } else {
            Some(self.caps.slot(cmd).await)
        };
        let mut value = self.send(cmd, args, window).await?;
        if cmd == "get_settings" {
            lock(&self.this).over(&mut value);
        }
        Ok(value)
    }

    /// Sends one call on the link there is now, with the window's base revision when it saves a
    /// store, and keeps the revision the shack answers with for the store it read or saved.
    async fn send(&self, cmd: &str, args: Value, window: &str) -> Result<Value, String> {
        let shack = self.shack().ok_or_else(|| word("notLive"))?;
        let access = store_access(cmd);
        let base = match access {
            Some((store, true)) => lock(&self.revisions)
                .get(&(window.to_string(), store))
                .cloned(),
            _ => None,
        };
        let (id, answer) = shack.call(cmd.to_string(), args, base).await;
        match answer {
            Answer::Value { value, revision } => {
                if let (Some((store, _)), Some(revision)) = (access, revision) {
                    lock(&self.revisions).insert((window.to_string(), store), revision);
                }
                Ok(value)
            }
            Answer::Error(error) => Err(error),
            Answer::Refused(refusal) => Err(word(refusal.code())),
            // A fresh id is never a repeat; if one were, it did not run again.
            Answer::Before(_) => Err(word(Refusal::Replayed.code())),
            Answer::TooLarge => Err(word("tooLarge")),
            Answer::Gone => {
                lock(&self.gone).push(id);
                Err(word("gone"))
            }
        }
    }

    /// The ui-state pair, split by key: the station's keys from and to the shack, every other key
    /// from and to this computer's own file, where this computer's own station's keys stay as they
    /// are. Both halves are this window's, so the revision of the shack's half is kept for its
    /// next save as any store's is.
    async fn split(&self, cmd: &str, args: &Value, window: &str) -> Result<Value, String> {
        let path = lock(&self.this).ui_state.clone();
        match cmd {
            "ui_state_load" => {
                let strict = args["strict"].as_bool().unwrap_or(false);
                // The station's half strictly, always: a file at the shack that does not read is
                // no station data, never an empty list of it.
                let _slot = self.caps.slot(cmd).await;
                let station = self
                    .send("ui_state_load", json!({ "strict": true }), window)
                    .await?;
                let station: BTreeMap<String, String> = serde_json::from_value(station)
                    .map_err(|_| word(Refusal::BadArguments.code()))?;
                let here = crate::ui_state_read(&path, strict)?;
                serde_json::to_value(join_ui_state(station, here)).map_err(|e| e.to_string())
            }
            _ => {
                let sent: BTreeMap<String, String> = serde_json::from_value(args["state"].clone())
                    .map_err(|_| word(Refusal::BadArguments.code()))?;
                let (station, own) = split_ui_state(sent);
                let kept = crate::ui_state_read(&path, true).unwrap_or_default();
                let here = serde_json::to_string_pretty(&join_ui_state(kept, own))
                    .map(|text| crate::write_json_atomic(&path, &text))
                    .unwrap_or(false);
                let _slot = self.caps.slot(cmd).await;
                let there = matches!(
                    self.send("ui_state_save", json!({ "state": station }), window)
                        .await,
                    Ok(Value::Bool(true))
                );
                Ok(json!(here && there))
            }
        }
    }

    /// Keeps the link to the station `id` names: connects, takes and keeps the lease, watches the
    /// feeds, hands the shack's events to the screens, and connects again when it drops. Shows the
    /// main window once the shack's state is here, or after [`SHOW_BY`] without it; a window shown
    /// without it is loaded again the first time it comes, so every screen starts from the shack's.
    pub(crate) async fn drive<R: Runtime>(
        self: Arc<Self>,
        app: tauri::AppHandle<R>,
        find: Find,
        typed: Option<SocketAddrV4>,
    ) {
        let shown = Arc::new(AtomicBool::new(false));
        {
            let (app, me, shown) = (app.clone(), Arc::clone(&self), Arc::clone(&shown));
            tauri::async_runtime::spawn(async move {
                let deadline = tokio::time::Instant::now() + SHOW_BY;
                while !me.live_once.load(Ordering::SeqCst) && tokio::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                shown.store(!me.live_once.load(Ordering::SeqCst), Ordering::SeqCst);
                show_the_app(&app).await;
            });
        }
        loop {
            let id = self.station_id.clone();
            let found = find.clone();
            let record = tokio::task::spawn_blocking(move || found(&id))
                .await
                .unwrap_or(Err("storeUnavailable"));
            let record = match record {
                Ok(Some(record)) => record,
                Ok(None) => {
                    // Forgotten here, or never paired: nothing will change that but the operator.
                    self.set(Phase::Refused, Some("unknownStation"));
                    return;
                }
                Err(reason) => {
                    self.set(Phase::Refused, Some(reason));
                    tokio::time::sleep(AGAIN_AFTER).await;
                    continue;
                }
            };
            if let Some(first) = record.addresses.first() {
                self.at(first.clone());
            }
            let wait = match Link::connect(&record, typed, run_id()).await {
                // The station proved the key pinned for this pairing; one that then names itself
                // another station is not the one asked for, and runs nothing from here.
                Ok((link, _)) if link.ids.station != self.station_id => {
                    link.close();
                    self.set(Phase::Refused, Some("notThisStation"));
                    AGAIN_REFUSED
                }
                Ok((link, at)) => {
                    self.at(at.to_string());
                    self.on_the_link(&app, Arc::new(link), &shown).await;
                    self.set(Phase::NotLive, None);
                    AGAIN
                }
                Err(reason) => {
                    let lasting = LASTING.contains(&reason);
                    let phase = if !lasting && self.live_once.load(Ordering::SeqCst) {
                        Phase::NotLive
                    } else {
                        Phase::Refused
                    };
                    self.set(phase, Some(reason));
                    if lasting {
                        AGAIN_REFUSED
                    } else {
                        AGAIN_AFTER
                    }
                }
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// One link, open, until it ends: the lease, the feeds, the events, and the calls the last
    /// link left unanswered.
    async fn on_the_link<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        link: Arc<Link>,
        shown: &AtomicBool,
    ) {
        self.install(Some(link.clone() as Arc<dyn Shack>));
        let forwarding = {
            let (app, mut events) = (app.clone(), link.events());
            tauri::async_runtime::spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                // Only what the dispatcher lets cross ever arrives (`Link::events`). A screen that
                // fell behind misses the oldest, and goes on hearing the rest.
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = app.emit(&event.name, event.data);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            })
        };
        self.ask_after_the_gone(&link).await;
        let mut lease: Option<String> = None;
        let mut said_unwatched = false;
        let mut every = tokio::time::interval(HEARTBEAT);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = link.closed() => break,
                _ = every.tick() => {}
            }
            lease = self.keep_the_lease(&link, lease).await;
            *lock(&self.held) = lease.clone().map(|lease| (Arc::clone(&link), lease));
            // The Stop road's heartbeat names the lease held now, or none.
            if let Some(road) = link.stop_road() {
                road.lease(lease.clone());
            }
            if !said_unwatched {
                if let Some(watching) = link.watching() {
                    said_unwatched = true;
                    let unknown = &watching["unknown"];
                    if unknown.as_array().is_some_and(|names| !names.is_empty()) {
                        tempo_core::applog::warn(
                            "client",
                            &format!("the station has no feed for {unknown}; those screens wait"),
                        );
                    }
                }
            }
            let here = link.latest("snapshot/1").is_some();
            if here && !self.live_once.swap(true, Ordering::SeqCst) && shown.load(Ordering::SeqCst)
            {
                // The shack's state is here for the first time, and the main window was shown
                // without it: it starts again, so every screen starts from the shack's.
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.eval("window.location.reload()");
                }
            }
        }
        forwarding.abort();
        *lock(&self.held) = None;
        self.install(None);
    }

    /// One turn of the lease: renewed while held, taken when control is free, and the banner told
    /// how it stands. A shack whose engine is busy for a moment answers `stationBusy`; the lease
    /// outlasts several of those, so it is simply asked again next turn.
    async fn keep_the_lease(&self, link: &Link, lease: Option<String>) -> Option<String> {
        if let Some(held) = lease {
            return match operate(link, json!({"type":"heartbeat","leaseId":held})).await {
                Ok(_) => {
                    self.set(Phase::Live, None);
                    Some(held)
                }
                Err("stationBusy") => Some(held),
                // Gone, or taken back by the station: taken again next turn, if control is free.
                Err(_) => None,
            };
        }
        let state = match operate(link, json!({"type":"state"})).await {
            Ok(state) => state,
            Err(_) => return None,
        };
        match state["phase"].as_str() {
            Some("available") | Some("controlling") => {}
            Some("occupied") => {
                self.set(Phase::Busy, None);
                return None;
            }
            _ => {
                self.set(Phase::Refused, Some("localPermissionRequired"));
                return None;
            }
        }
        let acquire = json!({"type":"acquire","stationBootId":state["stationBootId"]});
        match operate(link, acquire).await {
            Ok(taken) => {
                let lease = taken["leaseId"].as_str().map(str::to_string);
                if lease.is_some() {
                    link.watch(&FEEDS);
                    self.set(Phase::Live, None);
                }
                lease
            }
            Err("controllerBusy") => {
                self.set(Phase::Busy, None);
                None
            }
            Err(_) => None,
        }
    }

    /// Asks the shack what became of the calls a dropped link left unanswered, and says so in the
    /// diagnostic log. They are never sent again.
    async fn ask_after_the_gone(&self, link: &Link) {
        let gone: Vec<u64> = std::mem::take(&mut *lock(&self.gone));
        for ids in gone.chunks(ASK_AT_ONCE) {
            let outcomes = link.outcomes(ids).await;
            tempo_core::applog::info(
                "client",
                &format!(
                    "calls cut off by the dropped link: {ids:?}, at the station: {outcomes:?}"
                ),
            );
        }
    }

    /// Lets the station go: the lease handed back, so control is free at once for the shack's own
    /// screen or another device, and the link closed. Bounded, since it runs as Nexus quits.
    pub(crate) async fn let_go(&self) {
        let held = lock(&self.held).take();
        if let Some((link, lease)) = held {
            let release = operate(&link, json!({"type":"release","leaseId":lease}));
            let _ = tokio::time::timeout(Duration::from_secs(1), release).await;
            link.close();
        }
        self.install(None);
    }
}

/// One of the lease's operation requests on `link`, with a fresh request id: its value, or the
/// station's error code (`linkEnded` once the link has gone).
async fn operate(link: &Link, mut request: Value) -> Result<Value, &'static str> {
    request["requestId"] = json!(request_id());
    let answer = link.operation(request).await.ok_or("linkEnded")?;
    match answer["error"].as_str() {
        Some(code) => Err(KNOWN
            .iter()
            .find(|known| **known == code)
            .copied()
            .unwrap_or("refused")),
        None => Ok(answer["value"].clone()),
    }
}

/// The operation errors client mode tells apart; any other reads as `refused`.
const KNOWN: [&str; 6] = [
    "stationBusy",
    "controllerBusy",
    "leaseExpired",
    "notController",
    "localPermissionRequired",
    "staleStation",
];

/// A request id: 36 lowercase characters in the station's identifier shape, at random.
fn request_id() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The main window, sent from the page it waits on to the app, then shown, and the splash closed.
async fn show_the_app<R: Runtime>(app: &tauri::AppHandle<R>) {
    if crate::send_main_window_to_the_app(app).is_err() {
        tempo_core::applog::error("startup", "client mode: the main window could not be shown");
        return;
    }
    // The time the station's own launch gives the app to draw behind the splash.
    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
}

// --- Starting again ------------------------------------------------------------------------------

/// The arguments to start Nexus with once this one has quit, when it is to start again.
static AGAIN_WITH: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Starts this Nexus again with `args` once it has quit, and quits it: the main window is closed,
/// so the quit is the one a window close makes (at the shack's own station: the radio stopped
/// first and the logbook's changes saved, held on screen while they are). The profile it runs in
/// goes along.
pub(crate) fn start_again<R: Runtime>(app: &tauri::AppHandle<R>, mut args: Vec<String>) {
    if let Some(profile) = crate::active_profile() {
        args.splice(0..0, ["--profile".to_string(), profile.to_string()]);
    }
    *lock(&AGAIN_WITH) = Some(args);
    match app.get_webview_window("main") {
        Some(main) => {
            let _ = main.close();
        }
        None => app.exit(0),
    }
}

/// As the process ends: starts the Nexus [`start_again`] asked for, if it did.
pub(crate) fn started_again() {
    let args = lock(&AGAIN_WITH).take();
    let Some(args) = args else { return };
    let started = std::env::current_exe()
        .map_err(|e| e.to_string())
        .and_then(|exe| {
            tempo_core::process::command(exe)
                .args(&args)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    if let Err(why) = started {
        tempo_core::applog::error("startup", &format!("could not start Nexus again: {why}"));
    }
}

/// Whether this computer drives a paired station, and how its link stands: what the screens'
/// banner shows. `None` on a computer running its own station.
#[tauri::command]
pub(crate) fn client_status(app: tauri::AppHandle) -> Option<Status> {
    app.try_state::<Arc<Client>>().map(|client| client.status())
}

/// Leave: this computer's Nexus starts again as itself, the station handed back first.
#[tauri::command]
pub(crate) fn leave_client_mode(app: tauri::AppHandle) -> Result<(), String> {
    if app.try_state::<Arc<Client>>().is_none() {
        return Err("notClient".to_string());
    }
    start_again(&app, Vec::new());
    Ok(())
}

/// This run in client mode: the app with no station of its own, driving the station `station`.
pub(crate) fn run(station: String) {
    tempo_core::applog::info(
        "startup",
        "client mode: driving a paired station; this computer's own station stays off",
    );
    let this = ThisComputer::read(crate::settings_path(), crate::ui_state_path());
    let client = Arc::new(Client::new(station, this));
    let local = crate::app_commands();
    let routed = Arc::clone(&client);
    let driving = Arc::clone(&client);
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::clone(&client))
        .manage(crate::BetaUpdateState::default())
        .manage(crate::street_map::StreetMapState::default())
        .manage(crate::remote_window::LanOrigin::default())
        .invoke_handler(move |invoke| routed.handle(invoke, &*local))
        .setup(move |app| {
            if let Ok(dir) = app.path().resource_dir() {
                let _ = crate::RESOURCE_DIR.set(dir);
            }
            let _ = crate::DISPLAY_DPI.set(crate::probe_physical_dpi());
            if let Ok(dir) = app.path().document_dir() {
                let _ = crate::DOCUMENTS_DIR.set(dir);
            }
            if let Ok(dir) = app.path().picture_dir() {
                let _ = crate::PICTURES_DIR.set(dir);
            }
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_min_size(Some(tauri::LogicalSize::new(900.0, 600.0)));
            }
            // This computer's window layouts, restored and kept here.
            crate::window_state::install(app.handle());
            let handle = app.handle().clone();
            driving.before_install(Box::new(move || {
                crate::capture_all_window_geometry(&handle);
                tempo_core::applog::flush();
            }));
            let find: Find = {
                let stations = Arc::new(crate::lan_client::Stations::new(Arc::new(
                    crate::remote_service::vault::SystemVault,
                )));
                Arc::new(move |id: &str| stations.get(id))
            };
            tauri::async_runtime::spawn(driving.drive(app.handle().clone(), find, None));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app = window.app_handle();
                if let Some(w) = app.get_webview_window(window.label()) {
                    crate::capture_bandmap_window(&w);
                }
                // Closing the main window ends the app, and its pop-outs with it.
                if window.label() == "main" {
                    for (label, w) in app.webview_windows() {
                        if label != "main" {
                            crate::capture_bandmap_window(&w);
                            let _ = w.close();
                        }
                    }
                }
            }
        })
        .build(crate::context());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            let why = format!("client mode could not start: {e}");
            tempo_core::applog::error("startup", &why);
            crate::show_startup_failure(&why);
            return;
        }
    };
    app.run(move |handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            crate::capture_all_window_geometry(handle);
            tauri::async_runtime::block_on(client.let_go());
        }
        tauri::RunEvent::Exit => {
            crate::capture_all_window_geometry(handle);
            started_again();
        }
        _ => {}
    });
}

/// A lock, taken back from a panic that poisoned it: every value here is only ever replaced whole.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
