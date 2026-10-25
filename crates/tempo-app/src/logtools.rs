//! The Logbook's clean-up tools as the app makes them: the duplicate finder and its merge, the bulk
//! edit, and the check with its fixes. The rules are the store's ([`tempo_core::logbook::cleanup`]);
//! this module reads the rows, decides with those rules, and makes each change through the log's
//! one write path.
//!
//! # Every change is the log's own
//!
//! Each tool's change is planned with the Engine lock released and made under it, as every planned
//! change is ([`crate::logwrite`]): the rows read from the store, the change decided off the lock,
//! and made only while every row it read is still the row the log holds — planned again otherwise,
//! [`PLANS`] times, then the tool stops and says how far it got. Made through the station's one
//! commit, so the hot index, the store and the `log.adi` mirror all follow it, and recorded in the
//! history.
//!
//! **A tool's whole change is ONE run** ([`crate::loghistory::Origin::in_run`]), handed back in
//! [`ToolRun::run`]: Undo is `undo_history_run` on it, which puts back every contact the tool
//! changed or deleted, as one transaction. The change itself is made [`CHUNK`] contacts at a time,
//! so no hold of the Engine lock grows with the run; each chunk is in the store before the next is
//! planned.
//!
//! **A run of more than [`tempo_core::logbook::writer::CHUNK_ROWS`] contacts is refused while the
//! radio transmits** ([`MUST_WAIT_FOR_TX`]), as a large undo is — asked under the lock before each
//! chunk. Refused before the first chunk it changes nothing; refused later, the chunks made stand,
//! and the answer says how far it got (they are in the run, and its undo takes them back).
//!
//! # What each tool does to uploads — nothing of its own
//!
//! **No tool here calls a connector or queues an upload.** What a contact is owed to a service
//! follows from its stamps, and:
//!
//! - **a merge** gives the kept contact no stamp of another copy's but one only a copy a portable
//!   `/P` apart reached (`cleanup`'s header): the kept copy's stamps are its own, so the merged contact is owed to every service the kept copy did
//!   not reach, and the next upload sends it there. A service holding a deleted copy at the same
//!   time answers it Duplicate; one holding a copy logged minutes apart may keep both. Its call is
//!   the kept copy's, unchanged: no correction — and copies under different calls, or different
//!   station calls, are never merged at all.
//! - **a bulk edit** never changes the worked call (`cleanup::BULK_FIELDS`). Every stamp and
//!   confirmation stays, but on a contact whose **STATION_CALLSIGN** it replaces with another
//!   call: that is the call the contact is uploaded under, so its stamps are cleared as a corrected
//!   call's are, and the next upload sends it under the new call ([`cleanup::resends`]; the
//!   preview counts them, [`BulkPreview::recall`] and [`BulkPreview::resend`]). Filling in a blank
//!   station call, clearing one, or adding or taking off a portable `/P` changes the field and
//!   nothing else, whatever the call in Settings is; but a contact naming none is uploaded under
//!   the Settings call, so the preview counts those a fill or a clear sends under another call to
//!   a service not yet sent them ([`BulkPreview::moves`]), and the edit is refused, changing
//!   nothing, unless they are still the ones the operator acknowledged ([`MovesAck`]). It is not queued here. The LoTW, eQSL and QRZ confirmations matched
//!   under the old call go too; a paper card and the credit codes stay (`cleanup`'s header; the
//!   preview counts them, [`BulkPreview::unconfirm`]), and the run's undo puts them back. A
//!   push answered after the change is not stamped on it, unless only a `/P` changed
//!   ([`station::push_target`]).
//! - **a fix** changes a grid, a band, a DXCC number, a reference or an end time, never a call:
//!   every stamp stays.
//!
//! An undo of any of them sends nothing either ([`crate::loghistory`]). A stamp is never history:
//! the undo of a merge leaves the kept contact the stamps it had before the merge, because the
//! merge gave it none; the undo of a station call change clears the stamps of uploads made under
//! the call it takes off, so the contact is owed again under the call it gets back; and it never
//! puts back a blank station call that would move a contact under another call
//! ([`crate::loghistory::Undone::held`]).
//!
//! # What a merge does not touch
//!
//! The contest log's own row (Field Day's journal). A merge deletes the general log's copies it
//! does not keep and nothing else, as an undo changes the general log only; the contest log is
//! the contest's record of what went over the air.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_core::logbook::cleanup::{self, Change, Conflict, DupeKey, Field};
use tempo_core::logbook::history::{content_key, Source};
use tempo_core::logbook::sqlite::{Narrow, Order, Scope};
use tempo_core::logbook::{OpClass, QsoRecord, RecordId};
use tempo_core::reconcile::lotw_own_call;

use crate::engine::{engine_lock, Engine};
use crate::loghistory::{new_run, Origin};
use crate::logstore::{Durability, DURABLE_WAIT};
use crate::logwrite::settings_call;
use crate::station::{self, MadeRow, PLANS};

/// How many contacts one change of a tool carries: the fill job's bound, for its reason
/// ([`crate::logwrite::FILL_CHUNK`]) — a bound on what one plan reads and one commit holds the
/// Engine lock for, whatever the log's size.
pub const CHUNK: usize = crate::logwrite::FILL_CHUNK;

/// What a tool's run too large to make in a moment answers while the radio transmits or the FT
/// sequencer is armed ([`Engine::transmit_needs_the_lock`]).
pub const MUST_WAIT_FOR_TX: &str =
    "Stop transmitting first: changing this many contacts holds up the radio while it is made.";

/// What a tool's change made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolRun {
    /// How many contacts it changed or deleted.
    pub changed: usize,
    /// Its run in the history: undoing it undoes the whole change. `None` when it changed nothing.
    pub run: Option<String>,
    /// What it left alone, and why: `gone` (no longer in the log), `changed` (changed since it was
    /// shown), `needsChoice` (a guarded conflict with no choice, or a choice of a value no copy
    /// holds), `distinct` (a merge of copies that are two contacts: [`cleanup::distinct`]) — by
    /// contact; for a merge, by the contact it would have kept.
    pub refused: Vec<(RecordId, &'static str)>,
    /// The longest hold of the Engine lock one of its changes took (the bench's number).
    pub longest_hold: Duration,
}

/// What a tool that stopped part-way answers: what it made stands, and is undone with its run.
fn stopped(made: &ToolRun, why: &str) -> String {
    if made.changed == 0 {
        return why.to_string();
    }
    format!(
        "{} contacts were changed, then it stopped: {why} What was changed is one change in the \
         history and can be undone.",
        made.changed
    )
}

/// ★ Make a tool's change, `parts` at a time: each part's rows read from the store with the Engine
/// lock released, `plan_part` handed them (with the part's index) to answer the rows it changes
/// and what it leaves alone, and the change made under the lock — planned again while a row it
/// read keeps changing, [`PLANS`] plans. Every part's change is one run in the history. The run's
/// answer and the durability of every change made.
fn run_in_parts(
    engine: &Mutex<Engine>,
    parts: &[Vec<RecordId>],
    context: &str,
    mut plan_part: impl FnMut(
        usize,
        &HashMap<RecordId, Arc<QsoRecord>>,
    ) -> (Vec<MadeRow>, Vec<(RecordId, &'static str)>),
) -> (Result<ToolRun, String>, Durability) {
    let total: usize = parts.iter().map(Vec::len).sum();
    let large = total > tempo_core::logbook::writer::CHUNK_ROWS;
    let run = new_run();
    let mut made = ToolRun::default();
    let mut durability = Durability::default();
    for (k, part) in parts.iter().enumerate() {
        let mut done = false;
        for _ in 0..PLANS {
            let plan = crate::engine::log_plan(engine);
            let rows = match plan.rows(part) {
                Ok(rows) => rows,
                Err(e) => return (Err(stopped(&made, &e)), durability),
            };
            #[cfg(test)]
            tests::race();
            let (pairs, refused) = plan_part(k, &rows);
            if pairs.is_empty() {
                made.refused.extend(refused);
                done = true;
                break;
            }
            let n = pairs.len();
            let class = if pairs.iter().any(|(_, after)| after.is_none()) {
                OpClass::Structural
            } else {
                OpClass::Key
            };
            let mut guard = engine_lock(engine);
            let held = Instant::now();
            let (ok, d) = guard.with_log_tickets(|e| {
                // Before any of the work, in the same hold: the radio loop must not wait it out.
                if large && e.transmit_needs_the_lock() {
                    return Err(MUST_WAIT_FOR_TX);
                }
                e.station_mut().origin = Some(Origin::in_run(Source::Operator, &run));
                let ok = e
                    .station_mut()
                    .commit_planned(&plan, class, pairs, true, Vec::new(), context)
                    .is_ok();
                // Taken by the change it was set for — or by none, when the plan went stale.
                e.station_mut().origin = None;
                Ok(ok)
            });
            drop(guard);
            made.longest_hold = made.longest_hold.max(held.elapsed());
            match ok {
                Err(why) => return (Err(stopped(&made, why)), durability),
                Ok(false) => continue,
                // Turned back by the store: another window changed a row it read before the
                // store wrote it. Planned again on the rows as they now stand.
                Ok(true) if d.turned_back(DURABLE_WAIT) => continue,
                Ok(true) => {
                    made.changed += n;
                    made.run = Some(run.clone());
                    made.refused.extend(refused);
                    // In the store before the next part is planned: ahead of the store's
                    // writer the parts would pile up in flight, and each plan's read looks
                    // through what is in flight row by row.
                    let stored = d.wait_stored(DURABLE_WAIT);
                    durability = durability.and(d);
                    if let Err(why) = stored {
                        return (Err(stopped(&made, &why)), durability);
                    }
                    done = true;
                    break;
                }
            }
        }
        if !done {
            return (Err(stopped(&made, station::LOG_BUSY)), durability);
        }
    }
    (Ok(made), durability)
}

/// The station's country rule for a DXCC number, taken with the lock held for a moment: the name
/// the number gives a contact, else its call's entity ([`station::country_from`]).
fn country_rule(engine: &Mutex<Engine>) -> impl Fn(&str, Option<u32>) -> Option<String> {
    let (number, by_call) = {
        let e = engine_lock(engine);
        (
            e.station().dxcc_country.clone(),
            e.station().dxcc_resolve.clone(),
        )
    };
    move |call: &str, dxcc: Option<u32>| {
        station::country_from(number.as_deref(), by_call.as_deref(), call, dxcc)
    }
}

/// Every contact the log holds, `narrow`'s fields read, in log order. ⚠️ Never under the lock.
fn each_row(
    engine: &Mutex<Engine>,
    narrow: Narrow,
    each: &mut dyn FnMut(&QsoRecord),
) -> Result<(), String> {
    let rows = engine_lock(engine).log_rows();
    rows.each(narrow, Scope::All, Order::Log, &mut |r| {
        each(r);
        ControlFlow::Continue(())
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

// ── duplicates ─────────────────────────────────────────────────────────────────────────────────

/// Which duplicates to find: the log's own rule, or the wider "possible" window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupeTier {
    Rule,
    Possible,
}

impl DupeTier {
    pub fn window_secs(self) -> u64 {
        match self {
            DupeTier::Rule => tempo_core::logbook::dedup::DEDUP_WINDOW_SECS,
            DupeTier::Possible => cleanup::POSSIBLE_WINDOW_SECS,
        }
    }
}

/// One contact logged more than once: its copies in log order, the copy the merge keeps unless
/// the operator picks another, and what the copies disagree on when that one is kept.
#[derive(Debug, Clone, PartialEq)]
pub struct DupeGroup {
    pub copies: Vec<QsoRecord>,
    pub keep: usize,
    pub conflicts: Vec<Conflict>,
}

/// What the duplicate finder found.
#[derive(Debug, Clone, PartialEq)]
pub struct DupeScan {
    /// How many contacts it looked at.
    pub scanned: usize,
    pub groups: Vec<DupeGroup>,
}

const DUPE_KEYS: Narrow = Narrow {
    columns: &["call", "band", "mode", "when_unix"],
    uploads: false,
};

/// ★ The log's duplicates by `tier`: one narrow pass over the log for the station, band, mode and
/// time of every contact ([`cleanup::dupe_groups`]), then the copies of each group read whole.
/// Read with the Engine lock released; it changes nothing.
pub fn find_duplicates(engine: &Mutex<Engine>, tier: DupeTier) -> Result<DupeScan, String> {
    let mut keys = Vec::new();
    each_row(engine, DUPE_KEYS, &mut |r| {
        if let Some(id) = r.id {
            keys.push(DupeKey {
                id,
                call: r.call.clone(),
                band: r.band.clone(),
                mode: r.mode.clone(),
                when_unix: r.when_unix,
            });
        }
    })?;
    let scanned = keys.len();
    let settings_call = settings_call(engine);
    let grouped = cleanup::dupe_groups(keys, tier.window_secs());
    let ids: Vec<RecordId> = grouped.iter().flatten().copied().collect();
    let rows = engine_lock(engine).log_rows();
    let (found, _) = rows.rows_by_ids(&ids).map_err(|e| e.to_string())?;
    let at: HashMap<RecordId, usize> = found
        .iter()
        .enumerate()
        .filter_map(|(i, r)| Some((r.id?, i)))
        .collect();
    let mut groups = Vec::with_capacity(grouped.len());
    for group in grouped {
        // In log order; a copy deleted between the two reads is simply not in it.
        let mut places: Vec<usize> = group.iter().filter_map(|id| at.get(id).copied()).collect();
        places.sort_unstable();
        let copies: Vec<QsoRecord> = places.iter().map(|&i| found[i].clone()).collect();
        // Copies that differ in an identity field are two contacts, not one.
        for copies in cleanup::split_distinct(copies, Some(&settings_call)) {
            let keep = cleanup::suggested_keep(&copies);
            let conflicts = cleanup::merge(&copies, keep, &HashMap::new()).conflicts;
            groups.push(DupeGroup {
                copies,
                keep,
                conflicts,
            });
        }
    }
    Ok(DupeScan { scanned, groups })
}

/// One merge the operator asked for: the copy to keep, every copy with the revision it had when
/// it was shown ([`content_key`]: every field but its stamps, so a contest exchange, a station or
/// a passthrough field changed since is seen), and the copy whose value each conflicting field
/// takes.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeAsk {
    pub keep: RecordId,
    pub copies: Vec<(RecordId, String)>,
    pub choices: HashMap<String, RecordId>,
}

/// ★ Merge each group `asks` names into its kept copy ([`cleanup::merge`]): the kept copy takes
/// the union, the others are deleted — every group in ONE run of the history. A group is left as
/// it is when a copy is gone or changed since it was shown, when its copies are two contacts, or
/// when a guarded conflict has no choice; the rest are merged. Refused whole, changing nothing,
/// when a contact is named twice — in one merge or in two.
pub fn merge_duplicates(
    engine: &Mutex<Engine>,
    asks: &[MergeAsk],
) -> (Result<ToolRun, String>, Durability) {
    let mut named = HashSet::new();
    if !asks
        .iter()
        .flat_map(|a| &a.copies)
        .all(|(id, _)| named.insert(*id))
    {
        return (
            Err("A contact is named twice in the merge: nothing was changed.".into()),
            Durability::default(),
        );
    }
    // Whole groups to a part, about [`CHUNK`] contacts each.
    let mut parts: Vec<Vec<RecordId>> = Vec::new();
    let mut part_asks: Vec<Vec<&MergeAsk>> = Vec::new();
    for ask in asks {
        if parts
            .last()
            .is_none_or(|p| p.len() + ask.copies.len() > CHUNK)
        {
            parts.push(Vec::new());
            part_asks.push(Vec::new());
        }
        parts
            .last_mut()
            .expect("a part")
            .extend(ask.copies.iter().map(|(id, _)| *id));
        part_asks.last_mut().expect("a part").push(ask);
    }
    let settings_call = settings_call(engine);
    run_in_parts(engine, &parts, "merge_duplicates", |k, rows| {
        let mut pairs = Vec::new();
        let mut refused = Vec::new();
        for ask in &part_asks[k] {
            match merged_pairs(ask, rows, &settings_call) {
                Ok(made) => pairs.extend(made),
                Err(why) => refused.push((ask.keep, why)),
            }
        }
        (pairs, refused)
    })
}

/// The rows one merge makes on `rows` as the plan read them, or why it is not made.
/// `settings_call`: the call in Settings, the one a contact naming no station call is under.
fn merged_pairs(
    ask: &MergeAsk,
    rows: &HashMap<RecordId, Arc<QsoRecord>>,
    settings_call: &str,
) -> Result<Vec<MadeRow>, &'static str> {
    let mut copies: Vec<&Arc<QsoRecord>> = Vec::with_capacity(ask.copies.len());
    for (id, revision) in &ask.copies {
        let row = rows.get(id).ok_or("gone")?;
        if content_key(row) != *revision {
            return Err("changed");
        }
        copies.push(row);
    }
    let keep = ask
        .copies
        .iter()
        .position(|(id, _)| *id == ask.keep)
        .ok_or("gone")?;
    if copies.len() < 2 {
        return Err("gone");
    }
    // ★ One contact, whatever was asked: the same call, band and mode, no identity field held
    // differently, and all within the widest window the finder groups by.
    for (i, a) in copies.iter().enumerate() {
        if copies[i + 1..]
            .iter()
            .any(|b| cleanup::distinct(a, b, Some(settings_call)).is_some())
        {
            return Err("distinct");
        }
    }
    let (first, last) = copies.iter().fold((u64::MAX, 0), |(lo, hi), r| {
        (lo.min(r.when_unix), hi.max(r.when_unix))
    });
    if last - first > cleanup::POSSIBLE_WINDOW_SECS {
        return Err("distinct");
    }
    let owned: Vec<QsoRecord> = copies.iter().map(|r| QsoRecord::clone(r)).collect();
    let merged = cleanup::merge(&owned, keep, &ask.choices);
    if !merged.unchosen(&ask.choices).is_empty() {
        return Err("needsChoice");
    }
    // Each choice names a conflict there is and a copy holding one of its values.
    let offered = |tag: &String, id: &RecordId| {
        merged
            .conflicts
            .iter()
            .any(|c| c.tag == *tag && c.values.iter().any(|(v, _)| v == id))
    };
    if !ask.choices.iter().all(|(tag, id)| offered(tag, id)) {
        return Err("needsChoice");
    }
    let mut pairs: Vec<MadeRow> = Vec::with_capacity(copies.len());
    if merged.record != **copies[keep] {
        pairs.push((Arc::clone(copies[keep]), Some(Arc::new(merged.record))));
    }
    for (i, row) in copies.iter().enumerate() {
        if i != keep {
            pairs.push((Arc::clone(row), None));
        }
    }
    Ok(pairs)
}

// ── bulk edit ──────────────────────────────────────────────────────────────────────────────────

/// What a bulk edit would make of the contacts it was handed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkPreview {
    /// The contacts named.
    pub selected: usize,
    /// Those the edit changes.
    pub change: usize,
    /// Those it leaves as they are: already so, or a side holding another programme's reference.
    pub unchanged: usize,
    /// Those no longer in the log.
    pub missing: usize,
    /// Of those it changes, the ones whose station call it replaces with another
    /// ([`cleanup::resends`]): owed to every service under the new call.
    pub recall: usize,
    /// Of those, the ones uploaded somewhere under the station call it changes: their stamps are
    /// cleared, and the next upload sends them again under the new call.
    pub resend: usize,
    /// The LoTW, eQSL and QRZ confirmations a changed station call takes off those it changes.
    pub unconfirm: usize,
    /// Of those it changes, the ones a filled or cleared station call sends to a service not yet
    /// sent them under another call than before ([`cleanup::moves`]): a contact naming none goes
    /// under the call in Settings. Their stamps stay.
    pub moves: usize,
    /// The call those go under now, as the upload sends it, a portable `/P` kept
    /// ([`tempo_core::reconcile::lotw_own_call`]). `None` when none moves.
    pub move_to: Option<String>,
    /// The calls they went under before, as sent, sorted, each once.
    pub move_from: Vec<String>,
}

/// The contacts a preview said a fill or clear sends under another call ([`BulkPreview::moves`],
/// [`BulkPreview::move_to`]), as the operator acknowledged them: [`bulk_edit`] changes nothing
/// unless they are still what it finds. `Default` acknowledges none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovesAck {
    pub moves: usize,
    pub move_to: Option<String>,
}

/// Only fields a bulk edit offers, each value of its field's form.
fn bulk_changes(changes: &[Change]) -> Result<Vec<Change>, String> {
    if changes.is_empty() {
        return Err("Choose a field to set or clear.".into());
    }
    if let Some(c) = changes
        .iter()
        .find(|c| !cleanup::BULK_FIELDS.contains(&c.field))
    {
        return Err(format!("{:?} cannot be changed in bulk.", c.field));
    }
    cleanup::checked(changes)
}

/// ★ What [`bulk_edit`] would make of `ids` — read with the Engine lock released, changing
/// nothing: the preview the confirm names its count from.
pub fn bulk_preview(
    engine: &Mutex<Engine>,
    ids: &[RecordId],
    changes: &[Change],
) -> Result<BulkPreview, String> {
    let changes = bulk_changes(changes)?;
    let country = country_rule(engine);
    let settings_call = settings_call(engine);
    let settings_call = Some(settings_call.as_str());
    let mut from = BTreeSet::new();
    let mut seen = HashSet::new();
    let ids: Vec<RecordId> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    let mut out = BulkPreview {
        selected: ids.len(),
        ..BulkPreview::default()
    };
    for part in ids.chunks(CHUNK) {
        let plan = crate::engine::log_plan(engine);
        let rows = plan.rows(part)?;
        out.missing += part.len() - rows.len();
        for row in rows.values() {
            match cleanup::changed(row, &changes, &country) {
                Some(after) => {
                    out.change += 1;
                    if cleanup::resends(row, &after) {
                        out.recall += 1;
                        if row.upload != Default::default() {
                            out.resend += 1;
                        }
                    }
                    out.unconfirm += cleanup::confirmations_cleared(row, &after);
                    // Decided /P aside, named as sent: LoTW holds KD9TAW/P and KD9TAW apart.
                    if cleanup::moves(row, &after, settings_call) {
                        out.moves += 1;
                        out.move_to = lotw_own_call(&after, settings_call);
                        from.extend(lotw_own_call(row, settings_call));
                    }
                }
                None => out.unchanged += 1,
            }
        }
    }
    out.move_from = from.into_iter().collect();
    Ok(out)
}

/// What a bulk edit answers when the contacts it is handed are not the ones the operator confirmed.
pub const NOT_AS_PREVIEWED: &str =
    "The contacts to change are not the ones previewed: preview again. Nothing was changed.";

/// What a bulk edit answers when the contacts it would send under another call are not the ones
/// the operator acknowledged — the call in Settings changed since the preview, say.
pub const MOVES_NOT_AS_ACKNOWLEDGED: &str =
    "The contacts this change would send under another call are not the ones you \
     acknowledged (has the call in Settings changed?): preview again. Nothing was changed.";

/// ★ Set or clear `changes` on every contact `ids` names — the ids the preview was made of — as
/// ONE run of the history. Refused, changing nothing, unless they are `expect` contacts: the count
/// the operator confirmed, and unless the contacts a fill or clear sends under another call are
/// still the ones `acknowledged` names ([`MovesAck`]). Never the worked call: every confirmation and upload stamp stays but a
/// changed station call's (a paper card stays even then), and nothing is sent (see the module header). A contact no
/// longer in the log is named `gone`.
pub fn bulk_edit(
    engine: &Mutex<Engine>,
    ids: &[RecordId],
    expect: usize,
    acknowledged: &MovesAck,
    changes: &[Change],
) -> (Result<ToolRun, String>, Durability) {
    let raw = changes;
    let changes = match bulk_changes(changes) {
        Ok(c) => c,
        Err(e) => return (Err(e), Durability::default()),
    };
    let mut seen = HashSet::new();
    let ids: Vec<RecordId> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    if ids.len() != expect {
        return (Err(NOT_AS_PREVIEWED.into()), Durability::default());
    }
    // The contacts it sends under another call, read again now: never ones the operator did not
    // acknowledge.
    match bulk_preview(engine, &ids, raw) {
        Ok(p) if p.moves == acknowledged.moves && p.move_to == acknowledged.move_to => {}
        Ok(_) => return (Err(MOVES_NOT_AS_ACKNOWLEDGED.into()), Durability::default()),
        Err(e) => return (Err(e), Durability::default()),
    }
    let country = country_rule(engine);
    let parts: Vec<Vec<RecordId>> = ids.chunks(CHUNK).map(<[RecordId]>::to_vec).collect();
    run_in_parts(engine, &parts, "bulk_edit", |k, rows| {
        let mut pairs = Vec::new();
        let mut refused = Vec::new();
        for id in &parts[k] {
            match rows.get(id) {
                None => refused.push((*id, "gone")),
                Some(row) => {
                    if let Some(after) = cleanup::changed(row, &changes, &country) {
                        pairs.push((Arc::clone(row), Some(Arc::new(after))));
                    }
                }
            }
        }
        (pairs, refused)
    })
}

// ── the check ──────────────────────────────────────────────────────────────────────────────────

/// What is wrong with a contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// The worked station's grid is not a Maidenhead locator.
    Grid,
    /// The operator's own grid is not a Maidenhead locator.
    MyGrid,
    /// The contact's DXCC number is not the entity its call's prefix places it in.
    Dxcc,
    /// A frequency and no band.
    NoBand,
    /// A band its frequency is not in.
    BandFreq,
    /// Logged in the future.
    Future,
    /// Ended before it started.
    EndBeforeStart,
    /// An IOTA, POTA or SOTA reference not of its format.
    Iota,
    Pota,
    Sota,
}

/// One problem the check found, and the fix it proposes.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub id: RecordId,
    pub call: String,
    pub when_unix: u64,
    pub kind: ProblemKind,
    /// The field it is in, as the fix would find it ([`cleanup::value_of`]); `None` for a problem
    /// with no one field (a date in the future).
    pub field: Option<Field>,
    pub current: Option<String>,
    /// The proposed fix: the field set to this (`Some`) or cleared (`None`). `None` here when no
    /// fix can be proposed — a date only the operator can know.
    pub fix: Option<Change>,
}

/// What the check reads of every contact.
const CHECKED: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "freq_mhz",
        "when_unix",
        "time_off_unix",
        "grid",
        "my_grid",
        "dxcc",
        "ota_my_program",
        "ota_my_ref",
        "ota_their_program",
        "ota_their_ref",
        "ota_iota",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "credit_granted",
    ],
    uploads: false,
};

/// A grid's fix: trimmed, or cut back to the longest locator it starts with; cleared when it
/// starts with none. `None` — no fix — for a grid that is more than one locator: a list
/// (`FN31,FN32`, `FN31/FN32`, a VUCC credit each) or a 10-character one, whose cut would lose a
/// grid or the extension.
fn grid_fix(g: &str) -> Option<Option<String>> {
    let g = g.trim();
    if g.contains([',', '/', ' ']) || g.len() == 10 {
        return None;
    }
    let cut = [8, 6, 4, 2]
        .into_iter()
        .filter_map(|n| g.get(..n))
        .find(|p| cleanup::valid_grid(p))
        .map(str::to_string);
    Some(cut)
}

/// ★ The problems of one contact. `dxcc_mismatch` answers, for a call and the DXCC number the
/// contact carries, whether the country file places the call in a different entity (`None` when
/// they agree or either is unknown) and, when it does, the number to propose — `None` inside where
/// the call's prefix cannot decide it (a mainland call logged as one of its own block's islands,
/// a DXpedition's call, an exact-call exception): listed for the operator, never fixed.
/// `now_unix`: a contact more than a day after it is in the future.
///
/// **A confirmed contact's DXCC and band are listed, never fixed**: a paper card, a LoTW
/// confirmation or granted credit carries the entity and band the other station's record matched,
/// which outrank the call's prefix and the logged frequency. Nor is an end time ever cleared: an
/// end before the start that is no midnight wrap is listed for the operator.
pub fn problems_of(
    r: &QsoRecord,
    dxcc_mismatch: &dyn Fn(&str, u32) -> Option<Option<u32>>,
    now_unix: u64,
) -> Vec<Problem> {
    let Some(id) = r.id else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut add = |kind, field: Option<Field>, fix: Option<Option<String>>| {
        out.push(Problem {
            id,
            call: r.call.clone(),
            when_unix: r.when_unix,
            kind,
            field,
            current: field.and_then(|f| cleanup::value_of(r, f)),
            fix: field.zip(fix).map(|(field, value)| Change { field, value }),
        });
    };
    for (kind, field, grid) in [
        (ProblemKind::Grid, Field::Gridsquare, &r.grid),
        (ProblemKind::MyGrid, Field::MyGridsquare, &r.my_grid),
    ] {
        if let Some(g) = grid.as_deref().filter(|g| !g.is_empty()) {
            if !cleanup::valid_grid(g) {
                add(kind, Some(field), grid_fix(g));
            }
        }
    }
    let confirmed = r.qsl_rcvd.card || r.qsl_rcvd.lotw || !r.credit_granted.is_empty();
    if let Some(want) = r.dxcc.and_then(|n| dxcc_mismatch(&r.call, n)) {
        add(
            ProblemKind::Dxcc,
            Some(Field::Dxcc),
            want.filter(|_| !confirmed).map(|n| Some(n.to_string())),
        );
    }
    if r.freq_mhz.is_finite() && r.freq_mhz > 0.0 {
        let by_freq = crate::bandplan::band_for_dial(r.freq_mhz);
        if r.band.trim().is_empty() {
            add(
                ProblemKind::NoBand,
                Some(Field::Band),
                by_freq.map(|b| Some(b.to_string())),
            );
        } else if let Some(b) = by_freq.filter(|b| !b.eq_ignore_ascii_case(r.band.trim())) {
            add(
                ProblemKind::BandFreq,
                Some(Field::Band),
                (!confirmed).then(|| Some(b.to_string())),
            );
        }
    }
    if r.when_unix > now_unix + 86_400 {
        add(ProblemKind::Future, None, None);
    }
    if let Some(off) = r.time_off_unix.filter(|&off| off < r.when_unix) {
        // An end time past midnight written on the start's date: the next day's.
        let next = off + 86_400;
        let fix = (next >= r.when_unix && next - r.when_unix < 86_400).then(|| next.to_string());
        add(
            ProblemKind::EndBeforeStart,
            Some(Field::TimeOff),
            fix.map(Some),
        );
    }
    if let Some(iota) = r.ota.iota.as_deref().filter(|v| !v.is_empty()) {
        if !cleanup::valid_iota(iota) {
            let fix = cleanup::respelled(iota, cleanup::valid_iota);
            add(ProblemKind::Iota, Some(Field::Iota), fix.map(Some));
        }
    }
    for (program, reference, pota, sota) in [
        (
            &r.ota.my_program,
            &r.ota.my_ref,
            Field::MyPotaRef,
            Field::MySotaRef,
        ),
        (
            &r.ota.their_program,
            &r.ota.their_ref,
            Field::PotaRef,
            Field::SotaRef,
        ),
    ] {
        let Some(reference) = reference.as_deref().filter(|v| !v.is_empty()) else {
            continue;
        };
        let (kind, field, valid): (_, _, fn(&str) -> bool) = match program.as_deref() {
            Some(p) if p.eq_ignore_ascii_case("POTA") => {
                (ProblemKind::Pota, pota, cleanup::valid_pota)
            }
            Some(p) if p.eq_ignore_ascii_case("SOTA") => {
                (ProblemKind::Sota, sota, cleanup::valid_sota)
            }
            _ => continue,
        };
        if !valid(reference) {
            add(
                kind,
                Some(field),
                cleanup::respelled(reference, valid).map(Some),
            );
        }
    }
    out
}

/// ★ Check every contact the log holds ([`problems_of`]): one narrow pass, with the Engine lock
/// released, changing nothing. How many contacts it checked, and what it found.
pub fn check(
    engine: &Mutex<Engine>,
    dxcc_mismatch: &dyn Fn(&str, u32) -> Option<Option<u32>>,
    now_unix: u64,
) -> Result<(usize, Vec<Problem>), String> {
    let (mut checked, mut problems) = (0, Vec::new());
    each_row(engine, CHECKED, &mut |r| {
        checked += 1;
        problems.extend(problems_of(r, dxcc_mismatch, now_unix));
    })?;
    Ok((checked, problems))
}

/// One fix the operator ticked: `field` of the contact `id` set to `value` (cleared on `None`),
/// only while it still holds `expect` — what the check found there.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub id: RecordId,
    pub field: Field,
    pub expect: Option<String>,
    pub value: Option<String>,
}

/// ★ Make the fixes the operator ticked, and no other, as ONE run of the history. A fix whose
/// field has changed since the check is left out (`changed`), and so is a contact gone since.
pub fn apply_fixes(engine: &Mutex<Engine>, fixes: &[Fix]) -> (Result<ToolRun, String>, Durability) {
    let checked = cleanup::checked(
        &fixes
            .iter()
            .map(|f| Change {
                field: f.field,
                value: f.value.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let values = match checked {
        Ok(v) => v,
        Err(e) => return (Err(e), Durability::default()),
    };
    let mut by_id: HashMap<RecordId, Vec<(&Fix, Change)>> = HashMap::new();
    let mut ids = Vec::new();
    for (fix, change) in fixes.iter().zip(values) {
        let held = by_id.entry(fix.id).or_default();
        if held.is_empty() {
            ids.push(fix.id);
        }
        held.push((fix, change));
    }
    let country = country_rule(engine);
    let parts: Vec<Vec<RecordId>> = ids.chunks(CHUNK).map(<[RecordId]>::to_vec).collect();
    run_in_parts(engine, &parts, "log_fixes", |k, rows| {
        let mut pairs = Vec::new();
        let mut refused = Vec::new();
        for id in &parts[k] {
            let Some(row) = rows.get(id) else {
                refused.push((*id, "gone"));
                continue;
            };
            let mut still = Vec::new();
            for (fix, change) in &by_id[id] {
                let now = cleanup::value_of(row, fix.field);
                let expect = fix.expect.clone().filter(|v| !v.is_empty());
                if now == expect {
                    still.push(change.clone());
                } else {
                    refused.push((*id, "changed"));
                }
            }
            if let Some(after) = cleanup::changed(row, &still, &country) {
                pairs.push((Arc::clone(row), Some(Arc::new(after))));
            }
        }
        (pairs, refused)
    })
}

#[cfg(test)]
mod tests;
