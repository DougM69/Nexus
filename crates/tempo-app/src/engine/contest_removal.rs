//! ⭐ **Removing the newest contest contact, and putting it back — never a delete.**
//!
//! The contest log keeps a removed contact as removed
//! ([`FieldDayLog::remove_last`](tempo_core::fieldday::FieldDayLog::remove_last)) and its
//! journal keeps it marked, so the serial and the club seq it was given stay spent and the
//! contest screen can restore it exactly. Clear test contacts takes every contact out the same
//! way, each its own removal ([`Engine::contest_clear`]). **Nothing here keys, unkeys or stops a
//! transmission.**
//!
//! ⭐ **At a club the club takes it back too**, at a position or at the host (a host is just
//! another position): the removed rows' seqs, and a restore's rows, are what this position owes
//! its host ([`Engine::fd_sync_marks`]), sent by the club sync pump as soon as this laptop is
//! with the host and kept as a tombstone there, never a deleted row. A laptop takes back only
//! its own contacts: its log holds no other position's, and the host takes a take-back only on
//! the connection of the position that logged the contact.
//!
//! **What a removal cannot reach, it names.** The contact may already have gone to N3FJP (whose
//! API has no delete), the N1MM broadcast, WSJT-X listeners, and — if it was merged — the
//! logbook and the services the logbook uploaded it to. Every answer says which, and nothing is
//! changed there.

use std::sync::Mutex;

use super::{engine_lock, now_unix_secs, Engine, Mode};
use crate::dto::FieldDayQso;
use tempo_core::fieldday::{LoggedQso, RemoveRefusal, RemovedQso};
use tempo_core::logbook::{QsoRecord, UploadOutcome, UploadState};

/// Why a removal or a restore changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestRefusal {
    /// The contest log holds no contact.
    Empty,
    /// The newest contact is not the one named — one was logged, removed or restored since it
    /// was shown.
    Changed,
    /// No removed contact has that id.
    NotRemoved,
    /// The same station was worked again since, on that band and mode.
    WorkedAgain,
}

impl ContestRefusal {
    /// The refusal as the screen's catalog keys name it.
    pub fn code(self) -> &'static str {
        match self {
            ContestRefusal::Empty => "empty",
            ContestRefusal::Changed => "changed",
            ContestRefusal::NotRemoved => "notRemoved",
            ContestRefusal::WorkedAgain => "workedAgain",
        }
    }
}

impl From<RemoveRefusal> for ContestRefusal {
    fn from(r: RemoveRefusal) -> Self {
        match r {
            RemoveRefusal::Empty => ContestRefusal::Empty,
            RemoveRefusal::Changed => ContestRefusal::Changed,
            RemoveRefusal::NotRemoved => ContestRefusal::NotRemoved,
            RemoveRefusal::WorkedAgain => ContestRefusal::WorkedAgain,
        }
    }
}

/// Where a contact had already gone when it was removed: the places a removal cannot reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContestSentTo {
    /// The N3FJP host it was pushed to.
    pub n3fjp: Option<String>,
    /// The address of the N1MM broadcast it went out on.
    pub n1mm: Option<String>,
    /// It went to WSJT-X listeners as a "QSO Logged" datagram.
    pub wsjtx: bool,
}

/// One removed contact, as the engine answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestRemoval {
    /// The entry the contest log keeps — the rows exactly as they stood.
    pub entry: RemovedQso,
    /// Its rows as the contest log table shows them.
    pub rows: Vec<FieldDayQso>,
    /// Where it had already gone (a removal only; empty for a restore and for the list).
    pub sent: ContestSentTo,
    /// Each row's merge identity — what the logbook names its copy by, if it was merged.
    pub qids: Vec<tempo_core::contest::MergeIdentity>,
    /// While club sync runs, a removal or restore goes to the club too: `Some(true)` when this
    /// laptop is with the club's host now (it goes at once), `Some(false)` when it is not (it goes
    /// when the link is back). `None` without club sync, and for the list.
    pub club: Option<bool>,
}

/// A removal or restore, as a command answers it: the engine's answer, plus — for a removal —
/// the upload marks on the logbook's copies of the contact, read with the Engine lock released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestRemoved {
    pub removal: ContestRemoval,
    /// `Some` when the logbook holds a merged copy: the services that have taken it (or were
    /// sent it), by the copy's own upload marks. `None` when it was never merged.
    pub logbook: Option<Vec<&'static str>>,
}

impl Engine {
    /// ⭐ **Remove the newest contest contact** — the strip's Ctrl+D pressed twice, its Remove
    /// last button, and the contest screen's Remove on the newest row — when it is still the
    /// contact the strip named (`call`, `when_unix`). A county line goes with all its rows.
    ///
    /// Kept, never deleted ([`FieldDayLog::remove_last`](tempo_core::fieldday::FieldDayLog::remove_last)):
    /// the journal keeps it marked, its serial and seq stay spent, and
    /// [`Self::contest_restore`] puts it back. At a club the club takes it back too, now or when
    /// this laptop is with the host again (the answer's `club`). `Err` only when no contest runs.
    pub fn contest_remove_last(
        &mut self,
        call: &str,
        when_unix: u64,
    ) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        let entry = match station.log.remove_last(call, when_unix, now_unix_secs()) {
            Ok(entry) => entry,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log(); // journal it — the removal must survive a crash too
        let mut removal = self.contest_removal_of(entry);
        removal.sent = self.contest_sent_to(&removal.entry.rows);
        removal.club = self.contest_club_route();
        Ok(Ok(removal))
    }

    /// ⭐ **Clear test contacts** — the contest screen's two-click action for contacts logged as
    /// a test before the start: every contact of this running leaves the contest log, each as
    /// [`Self::contest_remove_last`] takes the newest
    /// ([`FieldDayLog::remove_all`](tempo_core::fieldday::FieldDayLog::remove_all)): kept under
    /// Removed for Restore, its serial and seq spent, the journal keeping it marked. Only when the
    /// log is still what the screen named: `row_count` rows, the newest contact `call` at `when_unix`.
    /// At a club the club takes each back too, as a removal goes (each answer's `club`): this
    /// laptop's log holds only its own contacts, and every removed row is a take-back the club
    /// sync owes the host ([`Self::fd_sync_marks`]). `Err` only when no contest runs.
    pub fn contest_clear(
        &mut self,
        row_count: usize,
        call: &str,
        when_unix: u64,
    ) -> Result<Result<Vec<ContestRemoval>, ContestRefusal>, String> {
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        let entries = match station
            .log
            .remove_all(row_count, call, when_unix, now_unix_secs())
        {
            Ok(entries) => entries,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log(); // journal it — the removals must survive a crash too
        let club = self.contest_club_route();
        Ok(Ok(entries
            .into_iter()
            .map(|entry| {
                let mut removal = self.contest_removal_of(entry);
                removal.sent = self.contest_sent_to(&removal.entry.rows);
                removal.club = club;
                removal
            })
            .collect()))
    }

    /// ⭐ **Put a removed contact back exactly as it was** — same time, serial and seq — by its
    /// id ([`RemovedQso::id`]). At a club the club puts it back too, as a removal goes. Refused
    /// when the same station was worked again since on that band and mode. `Err` only when no
    /// contest runs.
    pub fn contest_restore(
        &mut self,
        id: u64,
    ) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        let entry = match station.log.restore(id) {
            Ok(entry) => entry,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log();
        let mut restored = self.contest_removal_of(entry);
        restored.club = self.contest_club_route();
        Ok(Ok(restored))
    }

    /// Where a removal or restore made now goes at a club: `Some(true)` with the host now,
    /// `Some(false)` when the link is back, `None` without club sync.
    fn contest_club_route(&self) -> Option<bool> {
        self.fd_sync_enabled().then_some(self.fd_mirror.connected)
    }

    /// The removed contacts, oldest removal first — the contest screen's Removed list. Empty
    /// outside a contest.
    pub fn contest_removed(&self) -> Vec<ContestRemoval> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return Vec::new();
        };
        station
            .log
            .removed()
            .iter()
            .map(|entry| self.contest_removal_of(entry.clone()))
            .collect()
    }

    /// The highest seq this contest log has given, its removed contacts' included — where the
    /// slot loop's forwarder starts when a session (re)starts, so nothing restored from the
    /// journal is pushed again, however it is later removed and restored. 0 outside a contest.
    pub fn contest_seq_high_water(&self) -> u64 {
        match &self.mode {
            Mode::FieldDay { station, .. } => station.log.max_seq(),
            _ => 0,
        }
    }

    /// The forwarder's cursor, each slot boundary: every live contest row with a seq at or
    /// below it has been handed to N3FJP, the N1MM broadcast and WSJT-X listeners, as
    /// configured then.
    pub fn note_fd_forwarded(&mut self, seq: u64) {
        self.fd_forwarded_seq = seq;
    }

    /// ⭐ **The contest half of a Logbook delete** — the twin of
    /// [`Self::correct_contest_row`]: a contact merged from the RUNNING contest leaves the
    /// contest log too, as a removal (kept, restorable), so the Cabrillo no longer carries a
    /// contact the operator deleted.
    ///
    /// ⛔ Only a row this session and this position minted: the deleted record's `qid` must name
    /// the live row ([`tempo_core::contest::merged_row`] — the merge's identity for it, or the
    /// name an earlier build gave it), so last weekend's contact, or another position's, never
    /// reaches this log's row with the same seq. At a club the club takes it back too, as a
    /// removal goes.
    ///
    /// ⛔ **And only the LAST copy.** An earlier build's second merge after a move wrote some
    /// contacts into the logbook twice, and deleting one of the two tidies the logbook; it does
    /// not say the contact was not made, so the contest row stays while another copy of it does.
    /// `copies` is every merged copy the logbook held before this delete, each once and this one
    /// among them, read with the lock released ([`crate::logwrite::change_row`], which every
    /// command's delete goes through); `None` when nothing read them.
    pub(crate) fn contest_row_deleted(
        &mut self,
        row: &QsoRecord,
        copies: Option<&[(String, u64)]>,
    ) {
        let Some(qid) = row.contest.as_deref().map(|c| c.qid.as_str()) else {
            return;
        };
        let posid = self.settings.fd_position_id.clone();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return;
        };
        let Some(seq) = tempo_core::contest::merged_row(&station.log, &posid, qid, row.when_unix)
        else {
            return;
        };
        let another = copies.is_some_and(|copies| {
            station
                .log
                .qsos()
                .iter()
                .find(|q| q.seq == seq)
                .is_some_and(|q| {
                    let id = tempo_core::contest::MergeIdentity::of(&station.log, &posid, q);
                    copies.iter().filter(|(c, w)| id.names(c, *w)).count() > 1
                })
        });
        if another {
            tempo_core::applog::info(
                "contest",
                &format!(
                    "a copy of {} was deleted from the logbook; another copy is still there, so \
                     the contest log keeps the contact",
                    row.call
                ),
            );
            return;
        }
        if station.log.remove_row(seq, now_unix_secs()).is_some() {
            self.persist_fd_log();
            tempo_core::applog::info(
                "contest",
                &format!(
                    "{} was deleted from the logbook, so it left the contest log too \
                     (restore it on the contest screen)",
                    row.call
                ),
            );
        }
    }

    /// An entry with its rows as the log table shows them and their merge identities.
    fn contest_removal_of(&self, entry: RemovedQso) -> ContestRemoval {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return ContestRemoval {
                entry,
                rows: Vec::new(),
                sent: ContestSentTo::default(),
                qids: Vec::new(),
                club: None,
            };
        };
        let log = &station.log;
        let (rs, spec, role) = (log.ruleset(), log.session.exchange, log.session.role());
        ContestRemoval {
            rows: entry
                .rows
                .iter()
                .map(|q| super::field_day_display::field_day_qso(q, rs, spec, role))
                .collect(),
            qids: entry
                .rows
                .iter()
                .map(|q| {
                    tempo_core::contest::MergeIdentity::of(log, &self.settings.fd_position_id, q)
                })
                .collect(),
            sent: ContestSentTo::default(),
            club: None,
            entry,
        }
    }

    /// Where a contact's `rows` had gone before it was removed or edited: each forwarder
    /// destination configured now, when a slot boundary had already passed it. A destination
    /// configured after it went out, or removed before, is answered as configured now — the
    /// forwarder keeps no per-contact record of which destinations it reached.
    pub(super) fn contest_sent_to(&self, rows: &[LoggedQso]) -> ContestSentTo {
        let forwarded = rows
            .iter()
            .any(|q| q.seq > 0 && q.seq <= self.fd_forwarded_seq);
        if !forwarded {
            return ContestSentTo::default();
        }
        let s = &self.settings;
        let set = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        ContestSentTo {
            n3fjp: set(&s.n3fjp_host),
            n1mm: set(&s.n1mm_addr),
            wsjtx: s.wsjtx_udp,
        }
    }
}

/// The services holding a merged copy, by its upload marks: one that was sent it (pending) or
/// took it (accepted, or already had it) — never one that refused it.
pub(super) fn holders(marks: &[UploadState]) -> Vec<&'static str> {
    let has = |m: &Option<tempo_core::logbook::UploadStatus>| {
        matches!(
            m.as_ref().map(|s| s.outcome),
            Some(UploadOutcome::Pending | UploadOutcome::Accepted | UploadOutcome::Duplicate)
        )
    };
    let mut out = Vec::new();
    for (name, held) in [
        ("lotw", marks.iter().any(|m| has(&m.lotw))),
        ("qrz", marks.iter().any(|m| has(&m.qrz))),
        ("clublog", marks.iter().any(|m| has(&m.clublog))),
        ("eqsl", marks.iter().any(|m| has(&m.eqsl))),
    ] {
        if held {
            out.push(name);
        }
    }
    out
}

/// What a command makes of an engine answer: for a removal with merge identities, the logbook's
/// copies read with the lock released (the store is never read under it).
fn with_logbook(
    engine: &Mutex<Engine>,
    answer: Result<ContestRemoval, ContestRefusal>,
) -> Result<Result<ContestRemoved, ContestRefusal>, String> {
    let removal = match answer {
        Ok(removal) => removal,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let marks = crate::engine::log_plan(engine).merged_uploads(&removal.qids)?;
    let logbook = (!marks.is_empty()).then(|| holders(&marks));
    Ok(Ok(ContestRemoved { removal, logbook }))
}

/// ★ [`Engine::contest_remove_last`] as a command makes it: the removal under the lock, its
/// journal on disk before the answer (a removal is a change to the contact's only copy on disk
/// until it is merged), and the logbook's copies read after.
///
/// ⚠️ It waits and reads the store: never call it holding the Engine lock.
pub fn remove_last(
    engine: &Mutex<Engine>,
    call: &str,
    when_unix: u64,
) -> Result<Result<ContestRemoved, ContestRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_remove_last(call, when_unix)?;
        (answer, e.journal_mark())
    };
    written.wait();
    with_logbook(engine, answer)
}

/// What a Clear test contacts came to, as a command answers it: the engine's removals, plus the
/// services holding the logbook's copies of any of them, read once with the Engine lock released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestCleared {
    pub removals: Vec<ContestRemoval>,
    /// `Some` when the logbook holds a merged copy of any of them: the services that have taken
    /// (or were sent) one, by the copies' own upload marks. `None` when none was merged.
    pub logbook: Option<Vec<&'static str>>,
}

/// ★ [`Engine::contest_clear`] as a command makes it: the removals under the lock, the journal on
/// disk before the answer, and the logbook's copies read after, in one read.
///
/// ⚠️ It waits and reads the store: never call it holding the Engine lock.
pub fn clear(
    engine: &Mutex<Engine>,
    row_count: usize,
    call: &str,
    when_unix: u64,
) -> Result<Result<ContestCleared, ContestRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_clear(row_count, call, when_unix)?;
        (answer, e.journal_mark())
    };
    written.wait();
    let removals = match answer {
        Ok(removals) => removals,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let qids: Vec<_> = removals.iter().flat_map(|r| r.qids.clone()).collect();
    let marks = crate::engine::log_plan(engine).merged_uploads(&qids)?;
    let logbook = (!marks.is_empty()).then(|| holders(&marks));
    Ok(Ok(ContestCleared { removals, logbook }))
}

/// ★ [`Engine::contest_restore`] as a command makes it: the journal on disk before the answer.
///
/// ⚠️ It waits: never call it holding the Engine lock.
pub fn restore(
    engine: &Mutex<Engine>,
    id: u64,
) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_restore(id)?;
        (answer, e.journal_mark())
    };
    written.wait();
    Ok(answer)
}
