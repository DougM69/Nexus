//! Every change to a contact, kept beside the log: when, what it changed (each field, before and
//! after), and where the change came from — and what it takes to undo one.
//!
//! # Where it is written, and why there
//!
//! In the store, by [`super::sqlite::LogDb::apply`], **in the transaction that writes the rows**.
//! Every change any part of the app makes reaches the store through that one statement path (the
//! writer thread's chunks), so nothing can change a contact without its history, and a crash
//! leaves both or neither: a chunk's rows and its history commit together. The row as it stood
//! is read there too — the store's own row, in the same transaction — so the record says what the
//! change actually did to what the store held, never what a caller believed it held.
//!
//! The change's origin ([`Note`]) is decided by the app, which knows who asked; it rides the
//! change to the writer. A change with no note writes no history: the one-time conversion of
//! `log.adi` (that is the log's starting point, not a change to it) and a store with no app
//! attached (tests that drive the writer directly).
//!
//! # What is kept, and how compactly
//!
//! One row per changed contact per change ([`qso_history`](DDL)), and one row per change
//! ([`history_run`](DDL)) holding what every contact it touched shares: its run id, its time and
//! its source.
//!
//! - **An edit** (and a merge, a fill): only the fields that changed, as two ADIF fragments —
//!   the values before and after, each `<TAG:n>value` in the log's own spelling. A field absent
//!   from one side was absent from the contact on that side.
//! - **A delete**: the contact whole, as the operator's own log writes it
//!   ([`super::adif_record_own_log`]), so it can be put back exactly — its id, its stamps and
//!   every field another logger left in it.
//! - **A new contact**: the event alone. Its content is the contact itself.
//!
//! **Upload stamps are not history** ([`UPLOAD_TAGS`]). A stamp is the app's own bookkeeping of
//! what a service holds, not something the operator wrote; recording one would make "mark all
//! as uploaded" a 150,000-row history entry, and undoing one would quietly owe the contact to the
//! service again. A change that moves only stamps writes no history at all.
//!
//! # Undo
//!
//! An undo is a change like any other, made through the same write path and recorded with source
//! [`Source::Undo`] and a link to the entry it undoes, so it can itself be undone (a redo).
//! What it does to the contact, as it stands now ([`undo`]):
//!
//! - a new contact: removed;
//! - a deleted one: put back, whole, under its own id;
//! - an edit: each field it changed goes back, **but only a field still holding what the change
//!   wrote**. A field changed again since keeps the later value; an undo whose every field was
//!   changed since is refused rather than made as nothing.
//!
//! An undo never touches an upload stamp, with the one exception the edit rule already makes: an
//! undo that changes the callsign clears the stamps, as a corrected call does
//! ([`super::edited`]), so the contact is owed to the services under the call it now carries.
//! Nothing here sends anything anywhere.
//!
//! # Older builds
//!
//! Both tables are new and the store's `schema_version` does not move, so a build from before
//! them opens the store as it always did and never reads them. What that build changes is not
//! recorded, and an undo made afterwards meets the contact as that build left it — which the
//! field-by-field rule above already answers.

use std::collections::{BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use super::sqlite::{Result, RowWrite};
use super::{QsoRecord, RecordId};

/// The two tables, applied by every open after the store's own. `IF NOT EXISTS`, so an existing
/// store gains them on its first open by this build and nothing else about it changes.
///
/// `qso_history.run_id` names its `history_run` row without a foreign key, and `qso_id` names a
/// contact without one: a history row outlives the contact it describes (that is what a restore
/// reads), and a store written by an older build may hold contacts whose history never began.
pub(crate) const DDL: &str = "
CREATE TABLE IF NOT EXISTS history_run (
  id      INTEGER PRIMARY KEY,
  run     TEXT    NOT NULL UNIQUE,  -- the run id: one change, or one job's changes
  at_ms   INTEGER NOT NULL,         -- when it was made, Unix milliseconds
  source  TEXT    NOT NULL          -- who made it: Source::code
);
CREATE TABLE IF NOT EXISTS qso_history (
  seq     INTEGER PRIMARY KEY,      -- order of changes, across every contact
  run_id  INTEGER NOT NULL,         -- history_run.id
  qso_id  TEXT    NOT NULL,         -- the contact's id (APP_NEXUS_ID)
  kind    INTEGER NOT NULL,         -- Kind::code
  undoes  INTEGER,                  -- the seq this change undid, if it was an undo
  before  TEXT,                     -- edit: the changed fields as they were; delete: the contact
  after   TEXT                      -- edit: the changed fields as they became
);
";

/// The history's indexes, built with the store's own ([`super::sqlite::LogDb::build_indexes`]),
/// never by the tables' open: the one-time conversion opens a store with its tables only, and a
/// check made of a store another window is still converting must write nothing to it.
pub(crate) const INDEXES: &str = "
CREATE INDEX IF NOT EXISTS qso_history_qso ON qso_history(qso_id);
CREATE INDEX IF NOT EXISTS qso_history_run ON qso_history(run_id);
CREATE INDEX IF NOT EXISTS qso_history_deleted ON qso_history(seq) WHERE kind = 2;
";

/// The tags an upload stamp is written as. Never compared, never recorded, never undone.
pub const UPLOAD_TAGS: [&str; 4] = [
    "APP_TEMPO_UL_LOTW",
    "APP_TEMPO_UL_EQSL",
    "APP_TEMPO_UL_QRZ",
    "APP_TEMPO_UL_CLUBLOG",
];

/// Who made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The operator, at this station: a logged contact, an edit, a mark, a delete, a Logbook tool.
    Operator,
    /// An ADIF import (the Logbook's, or a companion program's logged contact).
    Import,
    /// The background fill of country and state.
    Fill,
    /// A LoTW confirmation download.
    Lotw,
    /// An eQSL InBox download.
    Eqsl,
    /// QRZ's logbook download.
    Qrz,
    /// A pota.app export's park references.
    Pota,
    /// The contest log's merge into the general log.
    Contest,
    /// A contact logged from a Remote client.
    Remote,
    /// Another program writing `log.adi` beside a session that keeps it.
    File,
    /// An undo (or a redo) made from the history.
    Undo,
}

impl Source {
    /// The word stored for it — a token, never translated.
    pub fn code(self) -> &'static str {
        match self {
            Source::Operator => "operator",
            Source::Import => "import",
            Source::Fill => "fill",
            Source::Lotw => "lotw",
            Source::Eqsl => "eqsl",
            Source::Qrz => "qrz",
            Source::Pota => "pota",
            Source::Contest => "contest",
            Source::Remote => "remote",
            Source::File => "file",
            Source::Undo => "undo",
        }
    }
}

/// What a change did to one contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// It was added.
    Create,
    /// Fields of it changed, by the operator or an undo.
    Edit,
    /// It was taken out of the log.
    Delete,
    /// Fields of it changed by a download or an import that matched it.
    Merge,
    /// An empty field of it was filled.
    Fill,
}

impl Kind {
    fn code(self) -> i64 {
        match self {
            Kind::Create => 0,
            Kind::Edit => 1,
            Kind::Delete => 2,
            Kind::Merge => 3,
            Kind::Fill => 4,
        }
    }

    fn of_code(code: i64) -> Kind {
        match code {
            0 => Kind::Create,
            2 => Kind::Delete,
            3 => Kind::Merge,
            4 => Kind::Fill,
            _ => Kind::Edit,
        }
    }

    /// The word for it the UI keys its labels on.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Create => "create",
            Kind::Edit => "edit",
            Kind::Delete => "delete",
            Kind::Merge => "merge",
            Kind::Fill => "fill",
        }
    }
}

/// Where a change came from: what every contact it touches shares. The app makes one per
/// change (or one per job, for a job written in several changes) and it rides the change to
/// the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The run this change belongs to — unique across every window and session sharing the
    /// store, so a job's changes can be undone together.
    pub run: String,
    /// When, Unix milliseconds.
    pub at_ms: i64,
    /// Who.
    pub source: Source,
    /// What a change to a contact the log already holds is called: [`Kind::Edit`],
    /// [`Kind::Merge`] or [`Kind::Fill`]. A contact added or taken out is that, whatever this is.
    pub edit: Kind,
    /// For an undo: the entry it undoes, by the contact it changes.
    pub undoes: HashMap<RecordId, i64>,
}

/// One entry of a contact's history, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its place in the order of every change.
    pub seq: i64,
    /// The contact.
    pub qso_id: RecordId,
    /// What the change did to it.
    pub kind: Kind,
    /// The change's run.
    pub run: String,
    /// When, Unix milliseconds.
    pub at_ms: i64,
    /// Who: a [`Source::code`] (or a word a later build wrote).
    pub source: String,
    /// The entry this one undid, if it was an undo.
    pub undoes: Option<i64>,
    /// An edit: the changed fields as they were. A delete: the contact, whole.
    pub before: Option<String>,
    /// An edit: the changed fields as they became.
    pub after: Option<String>,
}

/// One field an entry changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// The ADIF tag.
    pub tag: String,
    /// Its value before, `None` where the contact did not carry it.
    pub old: Option<String>,
    /// Its value after, `None` where the change took it away.
    pub new: Option<String>,
}

impl Entry {
    /// The fields this entry changed, in tag order. A delete lists every field of the contact it
    /// took out, with no new value; a new contact lists nothing.
    pub fn changes(&self) -> Vec<FieldChange> {
        let before: HashMap<String, String> = fields(self.before.as_deref().unwrap_or(""))
            .into_iter()
            .collect();
        let after: HashMap<String, String> = fields(self.after.as_deref().unwrap_or(""))
            .into_iter()
            .collect();
        let tags: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        tags.into_iter()
            .map(|tag| FieldChange {
                tag: tag.clone(),
                old: before.get(tag).cloned(),
                new: after.get(tag).cloned(),
            })
            .collect()
    }

    /// The contact a delete took out, as it was. `None` for any other entry.
    pub fn deleted_contact(&self) -> Option<QsoRecord> {
        if self.kind != Kind::Delete {
            return None;
        }
        let text = self.before.as_deref()?;
        let mut rec = super::parse_adif(text).into_iter().next()?;
        rec.id = Some(self.qso_id);
        Some(rec)
    }
}

/// An ADIF record's fields, in order, as the importer reads them: the tag upper-cased, the value
/// the `len` bytes after it. Up to `<EOR>`; a header is not expected (a fragment has none).
pub fn fields(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(end) = text[i..].find('>').map(|e| i + e) else {
            break;
        };
        let tag = &text[i + 1..end];
        i = end + 1;
        if tag.eq_ignore_ascii_case("EOR") {
            break;
        }
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let len: usize = parts
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        let stop = i.saturating_add(len);
        out.push((name, text.get(i..stop).unwrap_or("").to_string()));
        i = stop;
    }
    out
}

/// A contact's fields as the operator's own log writes it, upload stamps left out.
fn content_fields(r: &QsoRecord) -> Vec<(String, String)> {
    let mut f = fields(&super::adif_record_own_log(r));
    f.retain(|(tag, _)| !UPLOAD_TAGS.contains(&tag.as_str()));
    f
}

/// What changed between two versions of a contact, as the two fragments an edit stores — or
/// `None` when no field but an upload stamp did.
pub fn diff(old: &QsoRecord, new: &QsoRecord) -> Option<(String, String)> {
    let a = content_fields(old);
    let b = content_fields(new);
    let am: HashMap<&str, &str> = a.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let bm: HashMap<&str, &str> = b.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut before = String::new();
    let mut after = String::new();
    let tags: BTreeSet<&str> = am.keys().chain(bm.keys()).copied().collect();
    for tag in tags {
        let (x, y) = (am.get(tag), bm.get(tag));
        if x == y {
            continue;
        }
        if let Some(v) = x {
            before.push_str(&super::field(tag, v));
        }
        if let Some(v) = y {
            after.push_str(&super::field(tag, v));
        }
    }
    (!before.is_empty() || !after.is_empty()).then_some((before, after))
}

/// Write the history of one chunk of a change, inside the chunk's transaction.
///
/// `stored` holds the rows as the store held them before the chunk, by id — every row the chunk
/// takes out or writes that the store held. `cleared` is every row a purge that named none took
/// out. A row both taken out and written is an edit.
pub(crate) fn record(
    conn: &Connection,
    note: &Note,
    stored: &HashMap<RecordId, QsoRecord>,
    cleared: &[QsoRecord],
    remove: &[RecordId],
    upsert: &[RowWrite],
) -> Result<()> {
    let written: HashMap<RecordId, &QsoRecord> = upsert
        .iter()
        .filter_map(|w| Some((w.rec.id?, &*w.rec)))
        .collect();
    let mut rows: Vec<(RecordId, Kind, Option<String>, Option<String>)> = Vec::new();
    let mut gone = |id: RecordId, old: &QsoRecord| {
        if !written.contains_key(&id) {
            rows.push((
                id,
                Kind::Delete,
                Some(super::adif_record_own_log(old)),
                None,
            ));
        }
    };
    for old in cleared {
        if let Some(id) = old.id {
            gone(id, old);
        }
    }
    for id in remove {
        if let Some(old) = stored.get(id) {
            gone(*id, old);
        }
    }
    for w in upsert {
        let Some(id) = w.rec.id else { continue };
        match stored.get(&id) {
            None => rows.push((id, Kind::Create, None, None)),
            Some(old) => {
                if let Some((before, after)) = diff(old, &w.rec) {
                    rows.push((id, note.edit, Some(before), Some(after)));
                }
            }
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO history_run (run, at_ms, source) VALUES (?1, ?2, ?3)
         ON CONFLICT(run) DO NOTHING",
        params![note.run, note.at_ms, note.source.code()],
    )?;
    let run_id: i64 = conn.query_row(
        "SELECT id FROM history_run WHERE run = ?1",
        [&note.run],
        |r| r.get(0),
    )?;
    let mut ins = conn.prepare_cached(
        "INSERT INTO qso_history (run_id, qso_id, kind, undoes, before, after)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (id, kind, before, after) in rows {
        ins.execute(params![
            run_id,
            id.to_string(),
            kind.code(),
            note.undoes.get(&id),
            before,
            after
        ])?;
    }
    Ok(())
}

const SELECT: &str = "SELECT h.seq, h.qso_id, h.kind, r.run, r.at_ms, r.source, h.undoes, \
                      h.before, h.after FROM qso_history h JOIN history_run r ON r.id = h.run_id";

fn entry_of(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<Entry>> {
    let id: String = row.get(1)?;
    let Ok(qso_id) = id.parse::<RecordId>() else {
        return Ok(None);
    };
    Ok(Some(Entry {
        seq: row.get(0)?,
        qso_id,
        kind: Kind::of_code(row.get(2)?),
        run: row.get(3)?,
        at_ms: row.get(4)?,
        source: row.get(5)?,
        undoes: row.get(6)?,
        before: row.get(7)?,
        after: row.get(8)?,
    }))
}

fn entries(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Entry>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, entry_of)?;
    let mut out = Vec::new();
    for r in rows {
        out.extend(r?);
    }
    Ok(out)
}

/// One contact's history, oldest first.
pub(crate) fn of_qso(conn: &Connection, id: RecordId) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!("{SELECT} WHERE h.qso_id = ?1 ORDER BY h.seq"),
        [id.to_string()],
    )
}

/// One entry.
pub(crate) fn entry(conn: &Connection, seq: i64) -> Result<Option<Entry>> {
    Ok(entries(conn, &format!("{SELECT} WHERE h.seq = ?1"), [seq])?
        .into_iter()
        .next())
}

/// Every entry of one run, in the order they were made.
pub(crate) fn of_run(conn: &Connection, run: &str) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!("{SELECT} WHERE r.run = ?1 ORDER BY h.seq"),
        [run],
    )
}

/// The contacts deleted and not put back since, newest delete first: for each, its latest delete.
pub(crate) fn deleted(conn: &Connection, limit: usize) -> Result<Vec<Entry>> {
    entries(
        conn,
        &format!(
            "{SELECT} WHERE h.kind = 2 \
             AND NOT EXISTS (SELECT 1 FROM qso q WHERE q.id = h.qso_id) \
             AND h.seq = (SELECT MAX(x.seq) FROM qso_history x \
                          WHERE x.qso_id = h.qso_id AND x.kind = 2) \
             ORDER BY h.seq DESC LIMIT ?1"
        ),
        [i64::try_from(limit).unwrap_or(i64::MAX)],
    )
}

/// A run, as a toast or a menu names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// Its id.
    pub run: String,
    /// Who made it.
    pub source: String,
    /// When.
    pub at_ms: i64,
    /// How many contacts it changed.
    pub contacts: u64,
}

/// The latest run made by `source` (any source when `None`) that changed a contact.
pub(crate) fn latest_run(conn: &Connection, source: Option<&str>) -> Result<Option<RunSummary>> {
    let found = conn
        .query_row(
            "SELECT r.run, r.source, r.at_ms, \
                    (SELECT COUNT(*) FROM qso_history h WHERE h.run_id = r.id) \
             FROM history_run r \
             WHERE (?1 IS NULL OR r.source = ?1) \
               AND EXISTS (SELECT 1 FROM qso_history h WHERE h.run_id = r.id) \
             ORDER BY r.id DESC LIMIT 1",
            [source],
            |r| {
                Ok(RunSummary {
                    run: r.get(0)?,
                    source: r.get(1)?,
                    at_ms: r.get(2)?,
                    contacts: r.get::<_, i64>(3)?.max(0) as u64,
                })
            },
        )
        .optional()?;
    Ok(found)
}

/// What undoing an entry does to its contact, as it now stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    /// Take the contact out: the entry added it.
    Remove(RecordId),
    /// Put the contact back, whole: the entry deleted it.
    Restore(QsoRecord),
    /// The contact, with the entry's fields put back where they still hold what it wrote.
    /// `kept` names the fields changed again since, which keep the later value.
    Revert {
        /// The contact as the undo leaves it.
        row: QsoRecord,
        /// Fields left as they are, because a later change moved them.
        kept: Vec<String>,
    },
}

/// Why an entry cannot be undone now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotUndo {
    /// The contact is no longer in the log.
    Gone,
    /// The contact is in the log already: there is nothing to put back.
    AlreadyThere,
    /// Every field the entry changed has been changed again since.
    ChangedSince,
}

/// What undoing `entry` does to its contact, `current` — the contact as the log now holds it, or
/// `None` where it holds none.
pub fn undo(entry: &Entry, current: Option<&QsoRecord>) -> std::result::Result<Undo, CannotUndo> {
    match entry.kind {
        Kind::Create => match current {
            Some(_) => Ok(Undo::Remove(entry.qso_id)),
            None => Err(CannotUndo::Gone),
        },
        Kind::Delete => match current {
            Some(_) => Err(CannotUndo::AlreadyThere),
            None => entry
                .deleted_contact()
                .map(Undo::Restore)
                .ok_or(CannotUndo::Gone),
        },
        Kind::Edit | Kind::Merge | Kind::Fill => {
            let current = current.ok_or(CannotUndo::Gone)?;
            revert(entry, current)
        }
    }
}

/// [`undo`] of an edit: each changed field back where it still holds what the edit wrote.
fn revert(entry: &Entry, current: &QsoRecord) -> std::result::Result<Undo, CannotUndo> {
    let mut now = fields(&super::adif_record_own_log(current));
    let mut kept = Vec::new();
    let mut moved = false;
    for change in entry.changes() {
        let at = now.iter().position(|(tag, _)| *tag == change.tag);
        let holds = at.map(|i| now[i].1.clone());
        if holds != change.new {
            kept.push(change.tag);
            continue;
        }
        moved = true;
        match (at, change.old) {
            (Some(i), Some(old)) => now[i].1 = old,
            (Some(i), None) => {
                now.remove(i);
            }
            (None, Some(old)) => now.push((change.tag, old)),
            (None, None) => {}
        }
    }
    if !moved {
        return Err(CannotUndo::ChangedSince);
    }
    let mut text = String::new();
    for (tag, value) in &now {
        text.push_str(&super::field(tag, value));
    }
    text.push_str("<EOR>\n");
    let mut row = super::parse_adif(&text)
        .into_iter()
        .next()
        .ok_or(CannotUndo::ChangedSince)?;
    row.id = current.id;
    // A stamp is never history, so it is never undone: the contact keeps the stamps it has —
    // unless the undo corrects its call, which owes it to every service again, as a corrected
    // call always has (`edited`), with the LoTW mark another logger left in it.
    if row.call.trim().eq_ignore_ascii_case(current.call.trim()) {
        row.upload = current.upload.clone();
    } else {
        row.upload = super::UploadState::default();
        row.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
    }
    Ok(Undo::Revert { row, kept })
}

#[cfg(test)]
mod tests;
