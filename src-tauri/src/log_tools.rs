//! The Logbook's clean-up tools as the UI asks for them: the duplicate finder and its merge, the
//! bulk edit with its preview, and the check with its fixes ([`tempo_app::logtools`]).
//!
//! Every read runs with the Engine lock released and changes nothing. Every change is planned
//! with the lock released and made under it, a chunk at a time, as ONE run of the log's history —
//! its `run` is what `undo_history_run` takes — and answers only once it is on disk
//! ([`crate::durable_command`]). None sends anything anywhere: no tool changes a callsign, so by
//! the edit rule every upload stamp and confirmation stays, and nothing is queued to a service.
//!
//! The changes are the shack's own (`command_classes`): like a purge, a bulk change can reach
//! every contact in the log.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tempo_app::dto::LoggedQso;
use tempo_app::logtools::{self, DupeTier, Fix, MergeAsk, ProblemKind, ToolRun};
use tempo_core::logbook::cleanup::{Change, Field};
use tempo_core::logbook::query::LogQuery;
use tempo_core::logbook::{QsoEdit, RecordId};

use crate::log_queries::LogQueries;
use crate::{durable_command, SharedEngine};

/// A malformed id names no contact: an id is only ever one the station handed out.
fn id_of(id: &str) -> Result<RecordId, String> {
    id.parse()
        .map_err(|()| format!("'{id}' is not a contact id."))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("logbook task failed: {e}"))?
}

/// What a tool's change made.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunDto {
    /// How many contacts it changed or deleted.
    pub changed: usize,
    /// Its run: what `undo_history_run` takes to undo the whole change. `None` when it changed
    /// nothing.
    pub run: Option<String>,
    /// What it left alone: contacts gone since they were shown, changed since, and (a merge) groups
    /// with a conflict that must be chosen.
    pub gone: usize,
    pub changed_since: usize,
    pub needs_choice: usize,
}

fn run_dto(r: ToolRun) -> ToolRunDto {
    let count = |why: &str| r.refused.iter().filter(|(_, w)| *w == why).count();
    ToolRunDto {
        changed: r.changed,
        gone: count("gone"),
        changed_since: count("changed"),
        needs_choice: count("needsChoice"),
        run: r.run,
    }
}

// ── duplicates ─────────────────────────────────────────────────────────────────────────────────

/// One value a copy holds for a conflicting field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictValueDto {
    pub id: String,
    pub value: String,
}

/// A field the copies disagree on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub tag: String,
    pub values: Vec<ConflictValueDto>,
    /// Must be chosen before the merge: a confirmed copy's value, the QSL-sent mark, the contest
    /// exchange.
    pub guarded: bool,
    /// The copy whose value the suggested merge takes.
    pub chosen: String,
}

/// One contact logged more than once.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroupDto {
    /// The copies, in log order.
    pub copies: Vec<LoggedQso>,
    /// Each copy's edit key, sent back with the merge: a copy changed since is not merged over.
    pub edit_keys: Vec<String>,
    /// The copy the suggested merge keeps.
    pub keep: usize,
    /// What the copies disagree on when that one is kept.
    pub conflicts: Vec<ConflictDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeScanDto {
    pub scanned: usize,
    pub groups: Vec<DupeGroupDto>,
}

/// The log's duplicates: `tier` `rule` — the log's own duplicate rule, the same station, band and
/// mode within 5 minutes — or `possible`, within an hour. Changes nothing.
#[tauri::command]
pub async fn find_log_duplicates(
    state: State<'_, SharedEngine>,
    tier: String,
) -> Result<DupeScanDto, String> {
    let tier = match tier.as_str() {
        "rule" => DupeTier::Rule,
        "possible" => DupeTier::Possible,
        other => return Err(format!("'{other}' is not a duplicate tier.")),
    };
    let engine = Arc::clone(&state);
    let scan = blocking(move || logtools::find_duplicates(&engine, tier)).await?;
    Ok(DupeScanDto {
        scanned: scan.scanned,
        groups: scan
            .groups
            .into_iter()
            .map(|g| DupeGroupDto {
                edit_keys: g.copies.iter().map(|r| QsoEdit::project(r).key()).collect(),
                conflicts: g
                    .conflicts
                    .into_iter()
                    .map(|c| ConflictDto {
                        tag: c.tag,
                        values: c
                            .values
                            .into_iter()
                            .map(|(id, value)| ConflictValueDto {
                                id: id.to_string(),
                                value,
                            })
                            .collect(),
                        guarded: c.guarded,
                        chosen: c.chosen.to_string(),
                    })
                    .collect(),
                keep: g.keep,
                copies: g.copies.into_iter().map(LoggedQso::from).collect(),
            })
            .collect(),
    })
}

/// One copy of a merge, as it was shown.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeCopyDto {
    pub id: String,
    pub edit_key: String,
}

/// One merge the operator asked for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeDto {
    pub keep: String,
    pub copies: Vec<MergeCopyDto>,
    /// For a conflicting field, the copy whose value the merged contact takes.
    #[serde(default)]
    pub choices: HashMap<String, String>,
}

fn merge_ask(m: MergeDto) -> Result<MergeAsk, String> {
    Ok(MergeAsk {
        keep: id_of(&m.keep)?,
        copies: m
            .copies
            .into_iter()
            .map(|c| Ok((id_of(&c.id)?, c.edit_key)))
            .collect::<Result<_, String>>()?,
        choices: m
            .choices
            .into_iter()
            .map(|(tag, id)| Ok((tag, id_of(&id)?)))
            .collect::<Result<_, String>>()?,
    })
}

/// Merge each group into its kept copy, as ONE run. Sends nothing.
#[tauri::command]
pub async fn merge_log_duplicates(
    state: State<'_, SharedEngine>,
    merges: Vec<MergeDto>,
) -> Result<ToolRunDto, String> {
    let asks: Vec<MergeAsk> = merges
        .into_iter()
        .map(merge_ask)
        .collect::<Result<_, _>>()?;
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = logtools::merge_duplicates(&engine, &asks);
        (made.map(run_dto), durability)
    })
    .await
}

// ── bulk edit ──────────────────────────────────────────────────────────────────────────────────

/// The contacts a bulk edit acts on: the ones ticked, or every one the Logbook's filter shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkTarget {
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub query: Option<LogQuery>,
}

fn target_ids(
    engine: &SharedEngine,
    queries: &LogQueries,
    target: &BulkTarget,
) -> Result<Vec<RecordId>, String> {
    match (&target.ids, &target.query) {
        (Some(ids), None) => ids.iter().map(|id| id_of(id)).collect(),
        (None, Some(query)) => queries.ids_of(engine, query),
        _ => Err("Name the contacts to change: those ticked, or those the filter shows.".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkPreviewDto {
    pub selected: usize,
    pub change: usize,
    pub unchanged: usize,
    pub missing: usize,
}

/// What a bulk edit would make of its contacts. Changes nothing.
#[tauri::command]
pub async fn bulk_edit_preview(
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    target: BulkTarget,
    changes: Vec<Change>,
) -> Result<BulkPreviewDto, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    blocking(move || {
        let ids = target_ids(&engine, &queries, &target)?;
        let p = logtools::bulk_preview(&engine, &ids, &changes)?;
        Ok(BulkPreviewDto {
            selected: p.selected,
            change: p.change,
            unchanged: p.unchanged,
            missing: p.missing,
        })
    })
    .await
}

/// Set or clear fields on every contact named, as ONE run. Never a call; sends nothing.
#[tauri::command]
pub async fn bulk_edit_apply(
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    target: BulkTarget,
    changes: Vec<Change>,
) -> Result<ToolRunDto, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    durable_command(move || {
        let ids = match target_ids(&engine, &queries, &target) {
            Ok(ids) => ids,
            Err(e) => return (Err(e), Default::default()),
        };
        let (made, durability) = logtools::bulk_edit(&engine, &ids, &changes);
        (made.map(run_dto), durability)
    })
    .await
}

// ── the check ──────────────────────────────────────────────────────────────────────────────────

/// The DXCC number the country file places `call` in, when the contact's own `logged` number is a
/// current entity and the call's is another. A deleted entity's number is never judged: the
/// prefix it was logged under may name another entity today.
fn dxcc_mismatch(call: &str, logged: u32) -> Option<u32> {
    let entity = propagation::dxcc::entity_by_dxcc(logged)?;
    if entity.deleted {
        return None;
    }
    let by_call = propagation::dxcc::resolve(call)?.dxcc?;
    (by_call != logged).then_some(by_call)
}

fn kind_word(k: ProblemKind) -> &'static str {
    match k {
        ProblemKind::Grid => "grid",
        ProblemKind::MyGrid => "myGrid",
        ProblemKind::Dxcc => "dxcc",
        ProblemKind::NoBand => "noBand",
        ProblemKind::BandFreq => "bandFreq",
        ProblemKind::Future => "future",
        ProblemKind::EndBeforeStart => "endBeforeStart",
        ProblemKind::Iota => "iota",
        ProblemKind::Pota => "pota",
        ProblemKind::Sota => "sota",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixValueDto {
    /// `null`: the fix clears the field.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDto {
    pub id: String,
    pub call: String,
    pub when_unix: u64,
    pub kind: &'static str,
    pub field: Option<Field>,
    pub current: Option<String>,
    /// The proposed fix; `null` when none can be proposed.
    pub fix: Option<FixValueDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDto {
    pub checked: usize,
    pub problems: Vec<ProblemDto>,
}

/// Check every contact for problems, each with its proposed fix. Changes nothing.
#[tauri::command]
pub async fn check_logbook(state: State<'_, SharedEngine>) -> Result<CheckDto, String> {
    let engine = Arc::clone(&state);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (checked, problems) =
        blocking(move || logtools::check(&engine, &dxcc_mismatch, now)).await?;
    Ok(CheckDto {
        checked,
        problems: problems
            .into_iter()
            .map(|p| ProblemDto {
                id: p.id.to_string(),
                call: p.call,
                when_unix: p.when_unix,
                kind: kind_word(p.kind),
                field: p.field,
                current: p.current,
                fix: p.fix.map(|c| FixValueDto { value: c.value }),
            })
            .collect(),
    })
}

/// One fix the operator ticked, with what the check found in its field.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixDto {
    pub id: String,
    pub field: Field,
    pub expect: Option<String>,
    pub value: Option<String>,
}

/// Make the fixes ticked, and only those, as ONE run. Sends nothing.
#[tauri::command]
pub async fn apply_log_fixes(
    state: State<'_, SharedEngine>,
    fixes: Vec<FixDto>,
) -> Result<ToolRunDto, String> {
    let fixes: Vec<Fix> = fixes
        .into_iter()
        .map(|f| {
            Ok(Fix {
                id: id_of(&f.id)?,
                field: f.field,
                expect: f.expect,
                value: f.value,
            })
        })
        .collect::<Result<_, String>>()?;
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = logtools::apply_fixes(&engine, &fixes);
        (made.map(run_dto), durability)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The country file's answer is asked only of a current entity's number: a deleted one's is
    /// never called a mismatch, and a call its own number agrees with is not either.
    #[test]
    fn a_dxcc_mismatch_is_asked_only_of_a_current_entity() {
        assert_eq!(dxcc_mismatch("W1AW", 291), None, "agrees");
        assert_eq!(
            dxcc_mismatch("W1AW", 1),
            Some(291),
            "Canada's number on a US call"
        );
        let deleted = propagation::dxcc::entities()
            .into_iter()
            .find(|e| e.deleted)
            .expect("a deleted entity");
        assert_eq!(dxcc_mismatch("W1AW", deleted.code), None);
    }

    /// A merge's ids that are not ids are refused before anything is read.
    #[test]
    fn a_merge_naming_no_contact_is_refused() {
        let m = MergeDto {
            keep: "nope".into(),
            copies: Vec::new(),
            choices: HashMap::new(),
        };
        assert!(merge_ask(m).is_err());
    }
}
