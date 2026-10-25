//! The Logbook's change history and its undo, as the UI asks for them: a contact's history, the
//! contacts deleted and not put back, and the undo of one change, of a run, or of a delete
//! ([`tempo_app::loghistory`]). A run is undone by the id the change that made it handed back
//! (an import's, a purge's, an undo's own for its redo) — never by "the latest of a kind", which
//! a contact logged a moment later would be.
//!
//! Every read runs with the Engine lock released. Every undo is planned with the lock released and
//! made under it, made again when the store turns it back ([`tempo_app::logwrite::until_written`]),
//! and answers only once it is on disk ([`crate::durable_command`]). None sends anything anywhere.

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tempo_core::logbook::history::{CannotUndo, Entry, Kind};
use tempo_core::logbook::RecordId;

use crate::{durable_command, SharedEngine};

/// One field a change changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChangeDto {
    pub tag: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// One entry of a contact's history.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryDto {
    pub seq: i64,
    /// `create`, `edit`, `delete`, `merge` or `fill` — or `unknown`, a kind a later build
    /// recorded, which is never undone.
    pub kind: &'static str,
    /// Who made it: `operator`, `import`, `fill`, `lotw`, `eqsl`, `qrz`, `pota`, `contest`,
    /// `remote`, `file` or `undo` (or a word a later build wrote).
    pub source: String,
    pub run: String,
    pub at_ms: i64,
    /// The entry this one undid.
    pub undoes: Option<i64>,
    /// The latest entry that undid this one.
    pub undone_by: Option<i64>,
    /// The fields it changed. Empty for a new contact and a delete (a delete keeps the contact
    /// whole, for the restore).
    pub changes: Vec<FieldChangeDto>,
}

fn entries_dto(entries: Vec<Entry>) -> Vec<HistoryEntryDto> {
    let undone_by = |seq: i64| {
        entries
            .iter()
            .filter(|e| e.undoes == Some(seq))
            .map(|e| e.seq)
            .max()
    };
    entries
        .iter()
        .map(|e| HistoryEntryDto {
            seq: e.seq,
            kind: e.kind.word(),
            source: e.source.clone(),
            run: e.run.clone(),
            at_ms: e.at_ms,
            undoes: e.undoes,
            undone_by: undone_by(e.seq),
            changes: match e.kind {
                Kind::Delete | Kind::Create => Vec::new(),
                _ => e
                    .changes()
                    .into_iter()
                    .map(|c| FieldChangeDto {
                        tag: c.tag,
                        old: c.old,
                        new: c.new,
                    })
                    .collect(),
            },
        })
        .collect()
}

/// A malformed id names no contact: an id is only ever one the station handed out.
fn id_of(id: &str) -> Result<RecordId, String> {
    id.parse()
        .map_err(|()| format!("'{id}' is not a contact id."))
}

/// One contact's history, oldest change first.
#[tauri::command]
pub async fn qso_history(
    state: State<'_, SharedEngine>,
    id: String,
) -> Result<Vec<HistoryEntryDto>, String> {
    let engine = Arc::clone(&state);
    let id = id_of(&id)?;
    tokio::task::spawn_blocking(move || tempo_app::loghistory::history_of(&engine, id))
        .await
        .map_err(|e| format!("logbook task failed: {e}"))?
        .map(entries_dto)
}

/// A contact deleted and not put back.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedQsoDto {
    pub id: String,
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
    pub deleted_at_ms: i64,
    pub source: String,
}

/// The contacts deleted and not put back, newest delete first.
#[tauri::command]
pub async fn recently_deleted_qsos(
    state: State<'_, SharedEngine>,
    limit: Option<u32>,
) -> Result<Vec<DeletedQsoDto>, String> {
    let engine = Arc::clone(&state);
    let limit = limit.unwrap_or(200).clamp(1, 5_000) as usize;
    let entries = tokio::task::spawn_blocking(move || {
        tempo_app::loghistory::recently_deleted(&engine, limit)
    })
    .await
    .map_err(|e| format!("logbook task failed: {e}"))??;
    Ok(entries
        .iter()
        .filter_map(|e| {
            let rec = e.deleted_contact()?;
            Some(DeletedQsoDto {
                id: e.qso_id.to_string(),
                call: rec.call,
                band: rec.band,
                mode: rec.mode,
                when_unix: rec.when_unix,
                deleted_at_ms: e.at_ms,
                source: e.source.clone(),
            })
        })
        .collect())
}

/// What an undo made.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoAnswer {
    /// How many contacts it changed.
    pub changed: usize,
    /// The undo's own run — undoing it is the redo. `None` when it changed nothing.
    pub run: Option<String>,
    /// The fields a later change moved, which the undo left as they are.
    pub kept: Vec<String>,
    /// How many contacts it could not change, by why: `gone` (no longer in the log),
    /// `alreadyThere` (nothing to put back), `changedSince` (every field changed again since, or
    /// a contact it would take out edited or confirmed since, which stays), `duplicate` (the log
    /// holds it already under another id), `unknownKind` (a change a later build recorded).
    pub gone: usize,
    pub already_there: usize,
    pub changed_since: usize,
    pub duplicate: usize,
    pub unknown_kind: usize,
    /// The calls of the contacts left as they are because they changed since, and of those not
    /// put back because the log holds them already: sorted, each once.
    pub changed_since_calls: Vec<String>,
    pub duplicate_calls: Vec<String>,
}

fn answer(undone: tempo_app::loghistory::Undone) -> UndoAnswer {
    let count = |why: CannotUndo| undone.refused.iter().filter(|(_, w)| *w == why).count();
    let mut kept: Vec<String> = undone.kept.iter().map(|(_, tag)| tag.clone()).collect();
    kept.sort();
    kept.dedup();
    let calls = |why: CannotUndo| {
        let mut calls: Vec<String> = undone
            .refused
            .iter()
            .filter(|(_, w)| *w == why)
            .filter_map(|(id, _)| undone.calls.get(id).cloned())
            .collect();
        calls.sort();
        calls.dedup();
        calls
    };
    UndoAnswer {
        changed: undone.changed,
        run: undone.run.clone(),
        kept,
        gone: count(CannotUndo::Gone),
        already_there: count(CannotUndo::AlreadyThere),
        changed_since: count(CannotUndo::ChangedSince),
        duplicate: count(CannotUndo::Duplicate),
        unknown_kind: count(CannotUndo::UnknownKind),
        changed_since_calls: calls(CannotUndo::ChangedSince),
        duplicate_calls: calls(CannotUndo::Duplicate),
    }
}

/// Undo one change of a contact's history.
#[tauri::command]
pub async fn undo_qso_change(
    state: State<'_, SharedEngine>,
    seq: i64,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) =
            tempo_app::logwrite::until_written(|| tempo_app::loghistory::undo_entry(&engine, seq));
        (made.map(answer), durability)
    })
    .await
}

/// Undo every change one run made, as one transaction.
#[tauri::command]
pub async fn undo_history_run(
    state: State<'_, SharedEngine>,
    run: String,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) =
            tempo_app::logwrite::until_written(|| tempo_app::loghistory::undo_run(&engine, &run));
        (made.map(answer), durability)
    })
    .await
}

/// Put a deleted contact back, as its latest delete took it out.
#[tauri::command]
pub async fn restore_deleted_qso(
    state: State<'_, SharedEngine>,
    id: String,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    let id = id_of(&id)?;
    durable_command(move || {
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            tempo_app::loghistory::restore_deleted(&engine, id)
        });
        (made.map(answer), durability)
    })
    .await
}
