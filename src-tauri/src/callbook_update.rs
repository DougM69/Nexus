//! The Logbook's "Update from callbook" as the UI asks for it: a preview, a start, the progress, and a
//! cancel ([`tempo_app::callbook_update`], [`tempo_app::logtools::callbook_fill`]).
//!
//! **The same lookup the callsign card makes.** A call the lookup cache answers fresh is taken from
//! it; any other is asked of the callbooks ([`crate::callbook_lookup_live`]) — QRZ with the account
//! and stored password the card uses, then HamQTH and callook.info as the card falls through to
//! them — and the answer kept in the cache. Nothing here reads, stores or asks for a credential.
//! Unlike the card it does not hand its answers to the engine's in-memory list for the contacts
//! being logged now: a few hundred old calls would push out the ones the operator just looked up.
//!
//! ⚠️ **A failed lookup is a failure, never an older answer** ([`ask`]). The card answers from an
//! expired kept answer when the network fails; the update must not, or a QRZ that turns the
//! sign-in down would be asked (and turn it down) for every call to the cap, each "found" in data
//! older than the operator's expiry.
//!
//! **Paced and capped** by the operator's settings (`callbook_update_pace_secs`,
//! `callbook_update_daily_cap`). The fallbacks are paced the same: each call is ONE paced turn, in
//! which QRZ is asked first and HamQTH, then callook.info (a US call), only when the one before
//! has no record, so no service is asked more often than one call per pace (the call and then its
//! base for a stroked call, which the cap counts as two). The cap counts every turn's lookups,
//! whichever service answered. The station's own tally of the day's network lookups lives in the
//! cache file ([`tempo_core::callbook_cache::CallbookCache::note_lookups`]), and QRZ's own count of
//! the account's lookups ([`note_qrz_count`]) holds the cap too when an answer carries it; the
//! last one is kept in the same file with its time, so the next preview and run start from it. A
//! sign-in to QRZ or HamQTH is a request too ([`note_login`]), and counts.
//!
//! **What the services said** is noted by the lookups themselves ([`note_qrz_answer`],
//! [`note_login`], [`note_hamqth_refused`]) and taken after each of the update's lookups, as
//! QRZ's count is. They are the station's own account's, so a card lookup made at the same time
//! says the same about it.
//!
//! **One run.** The lookups run on a thread of their own; when they stop — every call answered,
//! cancelled, the cap reached, QRZ refusing or a lookup failed — the answers in hand are filled as
//! ONE run of the history, which the toast's Undo takes back. A later run takes the calls already
//! answered from the cache at no cost. A large fill waits while the radio transmits (shown as
//! waiting, and Stop ends the wait), and a fill stopped part-way still hands back its run.
//! The fill happens only once the lookups end: if the app quits mid-run nothing is filled, which
//! loses no contact data, and the answers already fetched are in the cache, so a re-run fills them
//! at no network cost.
//!
//! **Its end is reported once, wherever the operator is.** The end is kept until the Logbook takes
//! it ([`callbook_update_take_end`]): the dialog may have been closed long before the run ended,
//! and the Logbook, always there, shows the toast and its Undo and reloads the list.
//!
//! **Where the station is, only on the last 12 months' contacts** (operator ruling): the fill
//! gives an older contact the answer without its location ([`logtools::LOCATION_FIELDS`]), and the
//! preview counts the contacts that leaves without one.
//!
//! **Where it runs.** The preview, the progress and the cancel run at the station from either
//! computer; the start is the shack's own, as a bulk edit's is (`command_classes`): it changes
//! many contacts at once, and the lookups always run at the station with its own account.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tempo_app::callbook_update::{self, Asked, Callbook, Limits, Looked, Progress, Stop};
use tempo_app::engine::engine_lock;
use tempo_app::logtools;
use tempo_core::logbook::callbook::CallbookAnswer;
use tempo_core::logbook::RecordId;

use crate::log_queries::LogQueries;
use crate::log_tools::{id_of, run_dto, target_ids, BulkTarget, ToolRunDto};
use crate::{SharedCallbookCache, SharedEngine, SharedHamQthSession, SharedQrzSession};

/// What the callbooks said since it was last taken: noted by the lookups, taken by the update after
/// each of its lookups.
#[derive(Debug, Default, PartialEq)]
struct Heard {
    /// QRZ's own count of the account's lookups in the last 24 hours, from the last answer that
    /// carried one (`<Session><Count>`).
    qrz_count: Option<u32>,
    /// QRZ answered with a record.
    qrz_record: bool,
    /// QRZ answered with none: an unknown call, or a refusal.
    qrz_no_record: bool,
    /// Sign-ins to QRZ or HamQTH.
    logins: usize,
    /// HamQTH would not take a session it had just issued.
    hamqth_refused: bool,
}

static HEARD: Mutex<Heard> = Mutex::new(Heard {
    qrz_count: None,
    qrz_record: false,
    qrz_no_record: false,
    logins: 0,
    hamqth_refused: false,
});

fn heard() -> std::sync::MutexGuard<'static, Heard> {
    lock(&HEARD)
}

/// Note the count a QRZ lookup answer carried ([`crate::qrz_try_lookup`]).
pub(crate) fn note_qrz_count(count: Option<u32>) {
    if let Some(c) = count {
        heard().qrz_count = Some(c);
    }
}

/// Note what QRZ answered a lookup on a live session: a record, or none.
pub(crate) fn note_qrz_answer(record: bool) {
    let mut h = heard();
    if record {
        h.qrz_record = true;
    } else {
        h.qrz_no_record = true;
    }
}

/// Note a sign-in request to QRZ or HamQTH.
pub(crate) fn note_login() {
    heard().logins += 1;
}

/// Note HamQTH turning down a session it had just issued.
pub(crate) fn note_hamqth_refused() {
    heard().hamqth_refused = true;
}

fn take_heard() -> Heard {
    std::mem::take(&mut *heard())
}

/// What the update says when HamQTH turns down its own session: asked again, it would sign in
/// again for every call.
const HAMQTH_REFUSED: &str =
    "HamQTH would not accept a session it had just issued. Check the HamQTH username and password.";

/// The update in progress, or the last one's outcome. Managed state.
#[derive(Default)]
pub struct CallbookUpdate {
    status: Mutex<UpdateStatusDto>,
    /// The last update's end until the Logbook takes it ([`callbook_update_take_end`]): reported
    /// once.
    unreported: Mutex<Option<UpdateEndDto>>,
    cancel: AtomicBool,
    /// The day's network lookups, when the cache file cannot be opened: kept for the session.
    fallback_tally: Mutex<Vec<i64>>,
}

/// How the last update ended.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEndDto {
    /// `done`, `cancelled`, `limit`, `refused` (QRZ refusing lookups) or `failed`.
    pub stop: String,
    /// Why a lookup failed, or why the fill could not be made.
    pub error: Option<String>,
    /// The calls the callbook does not know.
    pub unknown: Vec<String>,
    /// Calls not reached: the rest, to be picked up by another run.
    pub not_looked_up: usize,
    /// The fill's run: what the toast's Undo takes. `None` when nothing was filled.
    pub made: Option<ToolRunDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusDto {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub from_cache: usize,
    pub looked_up: usize,
    /// Network lookups in the last 24 hours, as the cap reads them, and the cap.
    pub spent: usize,
    pub cap: usize,
    /// The lookups are done, and the fill waits for the radio to stop transmitting.
    pub waiting_for_tx: bool,
    /// The last update's outcome, once it has ended.
    pub ended: Option<UpdateEndDto>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The kept answer for `call` while it is fresh, as the log takes it. Never the network.
fn kept(cache: &SharedCallbookCache, call: &str, max_age: i64) -> Option<CallbookAnswer> {
    let (dto, fetched_at) = cache.get(call)?;
    tempo_core::callbook_cache::CachedAnswer {
        answer: String::new(),
        fetched_at,
    }
    .is_fresh(crate::now_unix(), max_age)
    .then(|| crate::callbook_answer(&dto))
}

/// The day's network lookups as the cap reads them: the cache file's tally, or the session's —
/// or QRZ's own count when it kept a larger one in the last day.
fn spent(app: &AppHandle) -> usize {
    let now = crate::now_unix();
    let cache = app.state::<SharedCallbookCache>();
    let qrz = cache
        .store()
        .and_then(|s| s.lock().ok().and_then(|s| s.qrz_count(now).ok().flatten()))
        .unwrap_or(0);
    if let Some(n) = cache
        .store()
        .and_then(|s| s.lock().ok().and_then(|s| s.lookups_spent(now).ok()))
    {
        return n.max(qrz);
    }
    let job = app.state::<CallbookUpdate>();
    let tally = lock(&job.fallback_tally);
    tally.iter().filter(|&&at| at > now - 86_400).count()
}

/// ★ One network lookup of `call` for the update: asked of the callbooks (`live`) whatever the
/// cache holds, and the answer kept in the cache. Any failure but "not in the callbook" is the
/// failure: an older kept answer never stands in for it (see the module header).
async fn ask<F, Fut>(cache: &SharedCallbookCache, call: &str, now: i64, live: F) -> Looked
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<tempo_app::dto::QrzLookupDto, String>>,
{
    match live().await {
        Ok(dto) => {
            cache.put(call, &dto, now);
            Looked::Found(Box::new(crate::callbook_answer(&dto)))
        }
        Err(e) if e == crate::callbook_not_found(call) => Looked::Unknown,
        Err(e) => Looked::Failed(e),
    }
}

/// One lookup's answer with what the services said while it was made: HamQTH turning down its own
/// session stops the run, as it would sign in again for every call.
fn asked(looked: Looked, heard: Heard) -> Asked {
    Asked {
        looked: if heard.hamqth_refused {
            Looked::Failed(HAMQTH_REFUSED.into())
        } else {
            looked
        },
        qrz_count: heard.qrz_count.map(|c| c as usize),
        qrz_without_record: heard.qrz_no_record && !heard.qrz_record,
        logins: heard.logins,
    }
}

/// The station's callbook as the update's loop asks it.
struct StationCallbook {
    app: AppHandle,
    max_age: i64,
}

impl Callbook for StationCallbook {
    fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
        kept(&self.app.state::<SharedCallbookCache>(), call, self.max_age)
    }

    /// The call, then its base for a stroked call: what the lookup may ask.
    fn cost(&self, call: &str) -> usize {
        crate::callbook_candidates(call).len()
    }

    fn spent(&mut self) -> usize {
        spent(&self.app)
    }

    fn note_spent(&mut self, n: usize) {
        let now = crate::now_unix();
        let cache = self.app.state::<SharedCallbookCache>();
        let noted = cache
            .store()
            .and_then(|s| s.lock().ok().map(|s| s.note_lookups(n, now).is_ok()))
            .unwrap_or(false);
        if !noted {
            let job = self.app.state::<CallbookUpdate>();
            lock(&job.fallback_tally).extend(std::iter::repeat_n(now, n));
        }
    }

    fn live(&mut self, call: &str) -> Asked {
        let _ = take_heard();
        let engine = self.app.state::<SharedEngine>().inner().clone();
        let qrz = self.app.state::<SharedQrzSession>().inner().clone();
        let hamqth = SharedHamQthSession(self.app.state::<SharedHamQthSession>().0.clone());
        let cache = self.app.state::<SharedCallbookCache>();
        let now = crate::now_unix();
        let looked = tauri::async_runtime::block_on(ask(&cache, call, now, || {
            crate::callbook_lookup_live(call, &engine, &qrz, &hamqth)
        }));
        let heard = take_heard();
        if let (Some(store), Some(count)) = (cache.store(), heard.qrz_count) {
            if let Ok(store) = store.lock() {
                let _ = store.note_qrz_count(count as usize, now);
            }
        }
        asked(looked, heard)
    }

    fn wait(&mut self, d: Duration) -> bool {
        let job = self.app.state::<CallbookUpdate>();
        let step = Duration::from_millis(100);
        let mut left = d;
        while !left.is_zero() {
            if job.cancel.load(Ordering::SeqCst) {
                return false;
            }
            let nap = left.min(step);
            std::thread::sleep(nap);
            left -= nap;
        }
        !job.cancel.load(Ordering::SeqCst)
    }

    fn cancelled(&self) -> bool {
        self.app
            .state::<CallbookUpdate>()
            .cancel
            .load(Ordering::SeqCst)
    }
}

/// What an update of the contacts would do: the calls, those already kept, the time the rest would
/// take at the pace, the day's lookups left under the cap, and per field how many contacts the fill
/// could give it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreviewDto {
    /// The contacts previewed: what `callbook_update_start` is handed back.
    pub ids: Vec<String>,
    pub selected: usize,
    pub missing: usize,
    pub calls: usize,
    pub kept: usize,
    /// Calls the cache does not answer: each a network lookup.
    pub to_look_up: usize,
    /// What those may cost against the cap: a stroked call is asked again for its base.
    pub lookups_needed: usize,
    pub pace_secs: u64,
    /// The least time the lookups take at the pace, in seconds.
    pub estimate_secs: u64,
    pub spent: usize,
    pub cap: usize,
    /// QRZ's own count of the account's lookups, from its last answer in the last day.
    pub qrz_count: Option<usize>,
    pub could_change: usize,
    /// `[ADIF field, contacts]`, most first: exact for kept answers, at most for the rest.
    pub fields: Vec<(String, usize)>,
    /// Whether a callbook is set up at all (a QRZ or HamQTH username, or callook.info on).
    pub configured: bool,
    /// The services each call is asked of, in order, as set up: `qrz`, `hamqth`, `callook`
    /// (US calls only).
    pub services: Vec<&'static str>,
    /// Contacts older than 12 months left without a location field the answer would have given.
    pub location_too_old: usize,
}

/// The callbooks [`crate::callbook_lookup_live`] asks, in its order, as the settings set them up.
fn services(s: &tempo_app::settings::Settings) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !s.qrz_username.trim().is_empty() {
        out.push("qrz");
    }
    if !s.hamqth_username.trim().is_empty() {
        out.push("hamqth");
    }
    if s.callook_lookup {
        out.push("callook");
    }
    out
}

/// Now, as the fill's 12-month location gate reads it.
fn now_secs() -> u64 {
    u64::try_from(crate::now_unix()).unwrap_or(0)
}

/// What "Update from callbook" would do with its contacts. Changes nothing and asks no callbook.
#[tauri::command]
pub async fn callbook_update_preview(
    app: AppHandle,
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    target: BulkTarget,
) -> Result<UpdatePreviewDto, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    tokio::task::spawn_blocking(move || {
        let ids = target_ids(&engine, &queries, &target)?;
        let (max_age, pace, cap, services) = {
            let e = engine_lock(&engine);
            let s = e.settings();
            (
                s.callbook_cache_max_age_secs(),
                s.callbook_update_pace(),
                s.callbook_update_daily_cap(),
                services(s),
            )
        };
        let cache = app.state::<SharedCallbookCache>();
        let mut lookups_needed = 0;
        let p = logtools::callbook_preview(&engine, &ids, now_secs(), &mut |call| {
            let hit = kept(&cache, call, max_age);
            if hit.is_none() {
                lookups_needed += crate::callbook_candidates(call).len();
            }
            hit
        })?;
        let qrz_count = cache.store().and_then(|s| {
            s.lock()
                .ok()
                .and_then(|s| s.qrz_count(crate::now_unix()).ok().flatten())
        });
        let to_look_up = p.calls.len() - p.kept;
        let mut fields: Vec<(String, usize)> = p.fields.into_iter().collect();
        fields.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(UpdatePreviewDto {
            ids: ids.iter().map(ToString::to_string).collect(),
            selected: p.selected,
            missing: p.missing,
            calls: p.calls.len(),
            kept: p.kept,
            to_look_up,
            lookups_needed,
            pace_secs: pace.as_secs(),
            estimate_secs: pace.as_secs() * to_look_up as u64,
            spent: spent(&app),
            cap,
            qrz_count,
            could_change: p.could_change,
            fields,
            configured: !services.is_empty(),
            services,
            location_too_old: p.location_too_old,
        })
    })
    .await
    .map_err(|e| format!("logbook task failed: {e}"))?
}

/// The word for why an update ended.
fn stop_word(stop: &Stop) -> (&'static str, Option<String>) {
    match stop {
        Stop::Done => ("done", None),
        Stop::Cancelled => ("cancelled", None),
        Stop::Limit => ("limit", None),
        Stop::Refused => ("refused", None),
        Stop::Failed(why) => ("failed", Some(why.clone())),
    }
}

/// Start an update of the contacts the preview counted — `ids`, as the preview answered them —
/// only while they are the `expect_count` the operator confirmed. Answers at once; the progress
/// is `callbook_update_status`. Refused while one is running.
#[tauri::command]
pub async fn callbook_update_start(
    app: AppHandle,
    state: State<'_, SharedEngine>,
    job: State<'_, CallbookUpdate>,
    ids: Vec<String>,
    expect_count: usize,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    let ids: Vec<RecordId> = ids
        .iter()
        .map(|id| id_of(id))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect();
    if ids.len() != expect_count {
        return Err(logtools::NOT_AS_PREVIEWED.into());
    }
    let engine = Arc::clone(&state);
    let (max_age, limits) = {
        let e = engine_lock(&engine);
        let s = e.settings();
        (
            s.callbook_cache_max_age_secs(),
            Limits {
                pace: s.callbook_update_pace(),
                daily_cap: s.callbook_update_daily_cap(),
            },
        )
    };
    {
        let mut status = lock(&job.status);
        if status.running {
            return Err("An update from the callbook is already running.".into());
        }
        *status = UpdateStatusDto {
            running: true,
            cap: limits.daily_cap,
            ..UpdateStatusDto::default()
        };
    }
    job.cancel.store(false, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("callbook-update".into())
        .spawn(move || {
            let ended = run(&app, &engine, &ids, expect_count, max_age, limits);
            app.state::<CallbookUpdate>().end(ended);
        })
        .map(|_| ())
        .map_err(|e| {
            lock(&job.status).running = false;
            format!("The update could not be started: {e}")
        })
}

/// The update itself, on its own thread: the calls looked up, then what they answered filled as
/// one run.
fn run(
    app: &AppHandle,
    engine: &SharedEngine,
    ids: &[RecordId],
    expect: usize,
    max_age: i64,
    limits: Limits,
) -> UpdateEndDto {
    let calls = match logtools::callbook_preview(engine, ids, now_secs(), &mut |_| None) {
        Ok(p) => p.calls,
        Err(e) => {
            return UpdateEndDto {
                stop: "failed".into(),
                error: Some(e),
                ..UpdateEndDto::default()
            }
        }
    };
    let job = app.state::<CallbookUpdate>();
    lock(&job.status).total = calls.len();
    let mut callbook = StationCallbook {
        app: app.clone(),
        max_age,
    };
    let found = callbook_update::look_up(&calls, limits, &mut callbook, &mut |p: &Progress| {
        let mut status = lock(&job.status);
        status.done = p.done;
        status.from_cache = p.from_cache;
        status.looked_up = p.looked_up;
        status.spent = p.spent;
    });
    {
        let mut status = lock(&job.status);
        status.done = found.progress.done;
        status.spent = found.progress.spent;
    }
    let (stop, error) = stop_word(&found.stop);
    let mut end = UpdateEndDto {
        stop: stop.into(),
        error,
        not_looked_up: calls.len() - found.progress.done,
        unknown: found.unknown,
        made: None,
    };
    if found.answers.is_empty() {
        return end;
    }
    let (made, why, durability) =
        logtools::callbook_fill(engine, ids, expect, &found.answers, now_secs(), &mut || {
            wait_for_tx(&job, engine)
        });
    // A fill stopped part-way hands back its run too: the toast's Undo takes what it made.
    if why.is_none() || made.run.is_some() {
        let wait = if durability.turned_back(tempo_app::logstore::DURABLE_WAIT) {
            Err(tempo_app::station::LOG_BUSY.to_string())
        } else {
            durability
                .wait(tempo_app::logstore::DURABLE_WAIT)
                .map_err(crate::durability_failed)
        };
        if let Err(why) = wait {
            end.error = Some(why);
        }
        end.made = Some(run_dto(made));
    }
    if why.is_some() {
        end.error = why;
    }
    end
}

/// The fill's wait while the radio transmits: until transmit is off (`true`), or the operator
/// stops the update (`false`). The status says it waits.
fn wait_for_tx(job: &CallbookUpdate, engine: &SharedEngine) -> bool {
    lock(&job.status).waiting_for_tx = true;
    let off = loop {
        if job.cancel.load(Ordering::SeqCst) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(250));
        if !engine_lock(engine).transmit_needs_the_lock() {
            break true;
        }
    };
    lock(&job.status).waiting_for_tx = false;
    off
}

impl CallbookUpdate {
    /// The update has ended: its status says so, and the end is kept until the Logbook takes it.
    fn end(&self, ended: UpdateEndDto) {
        *lock(&self.unreported) = Some(ended.clone());
        let mut status = lock(&self.status);
        status.running = false;
        status.waiting_for_tx = false;
        status.ended = Some(ended);
    }

    /// The end not reported yet, once: a second take has none.
    fn take_end(&self) -> Option<UpdateEndDto> {
        lock(&self.unreported).take()
    }
}

/// The last update's end, if the Logbook has not reported it yet; once.
#[tauri::command]
pub fn callbook_update_take_end(job: State<'_, CallbookUpdate>) -> Option<UpdateEndDto> {
    job.take_end()
}

/// How the update is getting on, or how the last one ended.
#[tauri::command]
pub fn callbook_update_status(job: State<'_, CallbookUpdate>) -> UpdateStatusDto {
    lock(&job.status).clone()
}

/// Stop the update after the lookup in hand: what it found so far is filled as one run.
#[tauri::command]
pub fn callbook_update_cancel(job: State<'_, CallbookUpdate>) {
    job.cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kept answer counts only while fresh by the operator's expiry, and comes back as the log
    /// takes it (the name by the log strip's rule).
    #[test]
    fn a_kept_answer_counts_only_while_fresh() {
        let cache = SharedCallbookCache::with(
            tempo_core::callbook_cache::CallbookCache::open_in_memory(10).unwrap(),
        );
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            nickname: Some("Hiram".into()),
            ..Default::default()
        };
        let now = crate::now_unix();
        cache.put("W1AW", &dto, now - 100);
        let a = kept(&cache, "w1aw", 1_000).expect("fresh");
        assert_eq!(
            (a.call.as_str(), a.name.as_deref()),
            ("W1AW", Some("Hiram"))
        );
        assert_eq!(kept(&cache, "W1AW", 50), None, "older than the expiry");
        assert_eq!(kept(&cache, "K1ABC", 1_000), None);
    }

    /// What the lookups note is taken once: a later take without a new lookup has nothing.
    #[test]
    fn what_the_lookups_note_is_taken_once() {
        let _ = take_heard();
        note_qrz_count(Some(42));
        note_qrz_count(None);
        note_qrz_answer(false);
        note_login();
        note_login();
        note_hamqth_refused();
        assert_eq!(
            take_heard(),
            Heard {
                qrz_count: Some(42),
                qrz_record: false,
                qrz_no_record: true,
                logins: 2,
                hamqth_refused: true,
            },
            "an answer with no count keeps the last"
        );
        assert_eq!(take_heard(), Heard::default());
    }

    /// The update's callbook over a real cache, its network a script: `answer` is what every
    /// lookup of the callbooks answers.
    struct OverCache {
        cache: SharedCallbookCache,
        max_age: i64,
        answer: Result<tempo_app::dto::QrzLookupDto, String>,
        asked: Vec<String>,
        tally: usize,
    }

    impl Callbook for OverCache {
        fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
            kept(&self.cache, call, self.max_age)
        }
        fn spent(&mut self) -> usize {
            self.tally
        }
        fn note_spent(&mut self, n: usize) {
            self.tally += n;
        }
        fn live(&mut self, call: &str) -> Asked {
            self.asked.push(call.to_string());
            let answer = self.answer.clone();
            Asked::plain(tauri::async_runtime::block_on(ask(
                &self.cache,
                call,
                crate::now_unix(),
                || async move { answer },
            )))
        }
        fn wait(&mut self, _: Duration) -> bool {
            true
        }
        fn cancelled(&self) -> bool {
            false
        }
    }

    fn over_cache(answer: Result<tempo_app::dto::QrzLookupDto, String>) -> OverCache {
        OverCache {
            cache: SharedCallbookCache::with(
                tempo_core::callbook_cache::CallbookCache::open_in_memory(10).unwrap(),
            ),
            max_age: 1_000,
            answer,
            asked: Vec::new(),
            tally: 0,
        }
    }

    fn limits() -> Limits {
        Limits {
            pace: Duration::ZERO,
            daily_cap: 250,
        }
    }

    /// ★ AN EXPIRED ANSWER NEVER HIDES A FAILED LOOKUP. W1AW's kept answer is past the expiry, so
    /// it is asked of the callbooks, and the sign-in is turned down: the run stops on that first
    /// failure (K1ABC is never asked, so QRZ is not asked to sign in again), and nothing is
    /// filled from the expired answer.
    #[test]
    fn an_expired_answer_never_stands_in_for_a_failed_lookup() {
        let refused = "QRZ login failed: incorrect password".to_string();
        let mut cb = over_cache(Err(refused.clone()));
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            ..Default::default()
        };
        cb.cache.put("W1AW", &dto, crate::now_unix() - 10_000);
        let run = callbook_update::look_up(
            &["W1AW".to_string(), "K1ABC".to_string()],
            limits(),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Failed(refused));
        assert_eq!(cb.asked, ["W1AW"], "stopped at the first failure");
        assert!(run.answers.is_empty(), "nothing from the expired answer");
        assert_eq!(cb.tally, 1);
    }

    /// The control: a lookup that answers is kept in the cache, fresh, and "not in the callbook"
    /// is an unknown call, not a failure.
    #[test]
    fn an_answer_is_kept_and_not_found_is_unknown() {
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            ..Default::default()
        };
        let mut cb = over_cache(Ok(dto));
        let run = callbook_update::look_up(&["W1AW".to_string()], limits(), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers["W1AW"].name.as_deref(), Some("Hiram Maxim"));
        assert!(kept(&cb.cache, "W1AW", 1_000).is_some(), "kept, fresh");

        let mut cb = over_cache(Err(crate::callbook_not_found("Q9QQQ")));
        let run = callbook_update::look_up(&["Q9QQQ".to_string()], limits(), &mut cb, &mut |_| {});
        assert_eq!(
            (run.stop, run.unknown),
            (Stop::Done, vec!["Q9QQQ".to_string()])
        );
    }

    /// What the services said, as the loop reads it: QRZ with no record for the call (and none
    /// for its base) is a no-record answer, a record for either is not; HamQTH turning down its
    /// own session fails the lookup; the sign-ins are counted.
    #[test]
    fn what_the_services_said_reaches_the_loop() {
        let no_record = Heard {
            qrz_no_record: true,
            logins: 1,
            ..Heard::default()
        };
        let a = asked(Looked::Unknown, no_record);
        assert_eq!(a.looked, Looked::Unknown);
        assert!(a.qrz_without_record);
        assert_eq!(a.logins, 1);

        let base_found = Heard {
            qrz_no_record: true,
            qrz_record: true,
            ..Heard::default()
        };
        assert!(!asked(Looked::Unknown, base_found).qrz_without_record);
        assert!(
            !asked(Looked::Unknown, Heard::default()).qrz_without_record,
            "QRZ not asked"
        );

        let hamqth = Heard {
            hamqth_refused: true,
            ..Heard::default()
        };
        assert_eq!(
            asked(Looked::Unknown, hamqth).looked,
            Looked::Failed(HAMQTH_REFUSED.into())
        );
    }

    /// ★ THE END IS REPORTED ONCE, WHENEVER IT IS ASKED. The run ends with no one watching: the
    /// end is kept, the first take has it and the second has nothing, and the status still
    /// shows it for the dialog opened again.
    #[test]
    fn the_end_is_taken_once() {
        let job = CallbookUpdate::default();
        lock(&job.status).running = true;
        assert_eq!(job.take_end(), None, "nothing ended yet");
        let end = UpdateEndDto {
            stop: "limit".into(),
            not_looked_up: 4,
            ..UpdateEndDto::default()
        };
        job.end(end.clone());
        assert_eq!(job.take_end(), Some(end.clone()));
        assert_eq!(job.take_end(), None, "once");
        let status = lock(&job.status).clone();
        assert!(!status.running);
        assert_eq!(status.ended, Some(end));
    }

    /// The one failure the update goes on after is the lookup's own "not in the callbook", word
    /// for word, for the call as asked.
    #[test]
    fn not_found_is_the_lookups_own_words() {
        assert_eq!(
            crate::callbook_not_found("w1aw/p"),
            "W1AW/P is not in the callbook."
        );
    }
}
