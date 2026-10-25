//! The log's change history as the app keeps and uses it: where each change comes from, and the
//! undo of one change, of a deleted contact, or of a whole run.
//!
//! The history itself is the store's ([`tempo_core::logbook::history`]): written with the rows,
//! in the same transaction, from what the store held. What this module adds is the half only the
//! app knows — **who asked** — and the undo, which is a change like any other:
//!
//! - **Where a change comes from** ([`note`]). Every change the station makes is handed to the
//!   writer with a [`Note`]: its source, read off the context its maker names (an import, a
//!   download, the fill job, an edit), or the [`Origin`] its maker set in the same hold of the
//!   lock (an undo, the contest merge, a job's run spanning several changes). An append from a
//!   Remote client is the remote's; every other append is the operator's.
//! - **An undo is planned with the Engine lock released and made under it**, as every by-id
//!   change is ([`crate::logwrite`]): the entries and the contacts read from the store, the
//!   contacts' new state decided off the lock ([`tempo_core::logbook::history::undo`]), and the
//!   change made only while every contact it read is still the one it read. Made through the
//!   station's one commit, so the hot index, the store, and the `log.adi` mirror all follow it,
//!   and recorded as [`Source::Undo`] with a link to what it undid — itself undoable: a redo.
//! - **A run's undo is ONE transaction** ([`tempo_core::logbook::writer::Change::whole`]): every
//!   contact the run touched goes back, or, if the store refuses any of it, none does. A contact
//!   that cannot go back (taken out since, or every field it changed changed again) is left as it
//!   is and named in the answer; the rest still go back together.
//!
//! **An undo never sends anything to a service.** It calls no connector and queues no upload. It
//! does not touch an upload stamp either — except as the edit rule already does: an undo that
//! changes a contact's callsign clears its stamps, as a corrected call does, so the contact is
//! owed to the services under the call it now carries and goes with the next upload. A corrected
//! call made by an edit is queued to the connectors at once (`requeue_if_corrected`); an undo's is
//! not.
//!
//! Not undone here: the contest log's own row (Field Day's journal) — an undo changes the general
//! log only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tempo_core::logbook::history::{self, CannotUndo, Entry, Kind, Note, RunSummary, Source, Undo};
use tempo_core::logbook::hot::RowPair;
use tempo_core::logbook::{OpClass, QsoRecord, RecordId};

use crate::engine::{engine_lock, Engine};
use crate::logstore::{Durability, READ_WAIT};
use crate::station::PLANS;

/// Where a change comes from, when its maker says so: set on the station in the hold of the lock
/// that makes the change, and taken by it ([`crate::station::StationCore::origin`]).
#[derive(Debug, Clone)]
pub struct Origin {
    pub(crate) source: Source,
    /// The run it belongs to; a new one when `None`.
    pub(crate) run: Option<String>,
    /// For an undo: the entry it undoes, by contact.
    pub(crate) undoes: HashMap<RecordId, i64>,
    /// Written in one transaction ([`tempo_core::logbook::writer::Change::whole`]).
    pub(crate) whole: bool,
}

impl From<Source> for Origin {
    fn from(source: Source) -> Origin {
        Origin {
            source,
            run: None,
            undoes: HashMap::new(),
            whole: false,
        }
    }
}

impl Origin {
    /// A change of `source` in the run `run`: one of several changes a job makes, undone together.
    pub(crate) fn in_run(source: Source, run: &str) -> Origin {
        Origin {
            run: Some(run.to_string()),
            ..Origin::from(source)
        }
    }
}

/// The source a change's context names: the maker's own word for the change, which every commit
/// carries ([`crate::station::StationCore::commit`]). The operator's, unless it names a download,
/// an import, the fill job or another program's `log.adi`.
pub(crate) fn source_of(context: &str) -> Source {
    match context {
        "import_adif" => Source::Import,
        "merge_lotw_report" | "merge_lotw_own_echo" => Source::Lotw,
        "merge_eqsl_report" => Source::Eqsl,
        "merge_qrz_report" | "gain_qrz_confirmations" => Source::Qrz,
        "import_pota_log" => Source::Pota,
        "fill" => Source::Fill,
        "take in log.adi" => Source::File,
        _ => Source::Operator,
    }
}

/// What a change of `source` to a contact the log holds is called.
fn edit_kind(source: Source) -> Kind {
    match source {
        Source::Fill => Kind::Fill,
        Source::Import | Source::Lotw | Source::Eqsl | Source::Qrz | Source::Pota => Kind::Merge,
        _ => Kind::Edit,
    }
}

/// A new run id, unique across every window and session sharing the store: a number drawn once
/// per process, and a count.
pub fn new_run() -> String {
    static PROCESS: OnceLock<u64> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let process = *PROCESS.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        h.finish()
    });
    format!("{process:016x}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The note a change carries to the store: `origin` where its maker set one, else the source its
/// `context` names, in a run of its own.
pub(crate) fn note(origin: Option<Origin>, context: &str) -> Arc<Note> {
    let origin = origin.unwrap_or_else(|| Origin::from(source_of(context)));
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Arc::new(Note {
        run: origin.run.unwrap_or_else(new_run),
        at_ms,
        source: origin.source,
        edit: edit_kind(origin.source),
        undoes: origin.undoes,
    })
}

// ── reads ─────────────────────────────────────────────────────────────────────────────────────

/// `f` against the store, once it holds every change this process made before it was asked (or
/// as it stands after [`READ_WAIT`]). ⚠️ Never under the Engine lock.
fn read<T>(
    engine: &Mutex<Engine>,
    f: impl FnOnce(&tempo_core::logbook::sqlite::LogDb) -> tempo_core::logbook::sqlite::Result<T>,
) -> Result<T, String> {
    let reads = engine_lock(engine).log_store_reads();
    reads
        .read(READ_WAIT, f)
        .map(|(t, _)| t)
        .map_err(|e| e.to_string())
}

/// One contact's history, oldest change first.
pub fn history_of(engine: &Mutex<Engine>, id: RecordId) -> Result<Vec<Entry>, String> {
    read(engine, |db| db.history_of(id))
}

/// The contacts deleted and not put back, newest first, at most `limit`.
pub fn recently_deleted(engine: &Mutex<Engine>, limit: usize) -> Result<Vec<Entry>, String> {
    read(engine, |db| db.history_deleted(limit))
}

/// The latest run `source` made that changed a contact (any source's when `None`).
pub fn latest_run(
    engine: &Mutex<Engine>,
    source: Option<Source>,
) -> Result<Option<RunSummary>, String> {
    read(engine, |db| db.history_latest_run(source.map(Source::code)))
}

// ── undo ──────────────────────────────────────────────────────────────────────────────────────

/// What an undo made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Undone {
    /// How many contacts it changed.
    pub changed: usize,
    /// The undo's own run: undoing it is the redo. `None` when it changed nothing.
    pub run: Option<String>,
    /// Fields left as they are because a later change moved them, by contact.
    pub kept: Vec<(RecordId, String)>,
    /// Contacts it could not change, and why.
    pub refused: Vec<(RecordId, CannotUndo)>,
}

/// Undo the entry `seq`.
pub fn undo_entry(engine: &Mutex<Engine>, seq: i64) -> (Result<Undone, String>, Durability) {
    match read(engine, |db| db.history_entry(seq)) {
        Ok(Some(entry)) => undo_entries(engine, &[entry], false),
        Ok(None) => (
            Err("That change is not in the logbook's history.".into()),
            Durability::default(),
        ),
        Err(e) => (Err(e), Durability::default()),
    }
}

/// Put back the contact `id` as its latest delete took it out.
pub fn restore_deleted(
    engine: &Mutex<Engine>,
    id: RecordId,
) -> (Result<Undone, String>, Durability) {
    match history_of(engine, id) {
        Ok(entries) => match entries.into_iter().rev().find(|e| e.kind == Kind::Delete) {
            Some(entry) => undo_entries(engine, &[entry], false),
            None => (
                Err("That contact was never deleted from this logbook.".into()),
                Durability::default(),
            ),
        },
        Err(e) => (Err(e), Durability::default()),
    }
}

/// Undo every change of the run `run`, in one transaction.
pub fn undo_run(engine: &Mutex<Engine>, run: &str) -> (Result<Undone, String>, Durability) {
    match read(engine, |db| db.history_run(run)) {
        Ok(entries) if entries.is_empty() => (
            Err("That change is not in the logbook's history.".into()),
            Durability::default(),
        ),
        Ok(entries) => undo_entries(engine, &entries, true),
        Err(e) => (Err(e), Durability::default()),
    }
}

/// What undoing `entries` makes of the contacts they name, as `rows` holds them: a pair per
/// contact that changes, the widest class among them, and the answer.
fn plan_undo(
    entries: &[Entry],
    ids: &[RecordId],
    rows: &HashMap<RecordId, Arc<QsoRecord>>,
) -> (Vec<RowPair>, OpClass, HashMap<RecordId, i64>, Undone) {
    let mut changed: Vec<RowPair> = Vec::new();
    let mut put_back: Vec<RowPair> = Vec::new();
    let mut undoes = HashMap::new();
    let mut answer = Undone::default();
    let mut class = OpClass::Key;
    for id in ids {
        let before = rows.get(id).cloned();
        let mut now: Option<QsoRecord> = before.as_deref().cloned();
        let mut moved = false;
        // Latest first: the run's changes to one contact come off in the order they went on.
        for e in entries.iter().rev().filter(|e| e.qso_id == *id) {
            match history::undo(e, now.as_ref()) {
                Ok(Undo::Remove(_)) => now = None,
                Ok(Undo::Restore(row)) => now = Some(row),
                Ok(Undo::Revert { row, kept }) => {
                    answer.kept.extend(kept.into_iter().map(|tag| (*id, tag)));
                    now = Some(row);
                }
                Err(why) => {
                    answer.refused.push((*id, why));
                    continue;
                }
            }
            moved = true;
            undoes.insert(*id, e.seq);
        }
        if !moved {
            continue;
        }
        match (before, now) {
            (Some(b), Some(a)) => changed.push((Some(b), Some(Arc::new(a)))),
            (Some(b), None) => {
                class = OpClass::Structural;
                changed.push((Some(b), None));
            }
            (None, Some(a)) => put_back.push((None, Some(Arc::new(a)))),
            (None, None) => {}
        }
    }
    // The contacts put back go in after the ones changed, as an append's rows go in last.
    changed.extend(put_back);
    answer.changed = changed.len();
    (changed, class, undoes, answer)
}

/// Undo `entries` — one change's, or a run's, `whole` — planned with the Engine lock released and
/// made under it, planned again while a contact it read keeps changing ([`PLANS`] plans).
fn undo_entries(
    engine: &Mutex<Engine>,
    entries: &[Entry],
    whole: bool,
) -> (Result<Undone, String>, Durability) {
    let mut ids: Vec<RecordId> = Vec::new();
    for e in entries {
        if !ids.contains(&e.qso_id) {
            ids.push(e.qso_id);
        }
    }
    let run = new_run();
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let rows = match plan.rows(&ids) {
            Ok(rows) => rows,
            Err(e) => return (Err(e), Durability::default()),
        };
        let (pairs, class, undoes, mut answer) = plan_undo(entries, &ids, &rows);
        if pairs.is_empty() {
            return (Ok(answer), Durability::default());
        }
        #[cfg(test)]
        tests::race();
        let origin = Origin {
            source: Source::Undo,
            run: Some(run.clone()),
            undoes,
            whole,
        };
        let (made, durability) = engine_lock(engine).with_log_tickets(|e| {
            let made = e
                .station_mut()
                .commit_undo(&plan, class, pairs.clone(), origin.clone());
            // Taken by the change it was set for — or by none, when the plan went stale.
            e.station_mut().origin = None;
            made.is_ok()
        });
        if made {
            answer.run = Some(run);
            return (Ok(answer), durability);
        }
    }
    (Err(crate::station::LOG_BUSY.into()), Durability::default())
}

#[cfg(test)]
mod tests;
