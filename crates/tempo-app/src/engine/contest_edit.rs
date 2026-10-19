//! ⭐ **Editing a contest contact where it stands — the contest screen's Edit and the strip's
//! Ctrl+Y.**
//!
//! A busted call, a mistyped exchange, a contact logged on the wrong band or in the wrong mode,
//! fixed in place ([`FieldDayLog::edit_contact`](tempo_core::fieldday::FieldDayLog::edit_contact)):
//! the contact keeps its time, its serial and its seq, so an edit never spends a number, and the
//! journal holds the edit before the answer, so a restart keeps it. The score, every row's dupe
//! mark and the multipliers follow from the rows, the Cabrillo and the ADIF are written from them,
//! and a merge into the logbook made after the edit writes the contact as it now is. **Nothing
//! here keys, unkeys or stops a transmission.**
//!
//! ⛔ **Never at a club.** Club sync v4 names a row by its position and seq, and the host keeps
//! the row it merged first: the edited row sent again changes nothing there, nor does a take-back
//! and restore around it, so the club's file would keep the busted call with every chip saying
//! Synced. While club sync runs an edit is refused ([`ContestEditRefusal::Club`]), and the screen
//! says how a contact is fixed at a club today: taken back and logged again. Carrying an edit to
//! the club needs a line v4 does not have.
//!
//! **What an edit cannot reach, it names**, as a removal does: N3FJP, the N1MM broadcast and
//! WSJT-X listeners the contact had already gone to, and a copy merged into the logbook before
//! the edit, which keeps what it said until it is edited there.

use std::sync::Mutex;

use super::contest_removal::{holders, ContestSentTo};
use super::{engine_lock, Engine, Mode};
use crate::dto::FieldDayQso;
use tempo_core::fieldday::{ContactEdit, EditRefusal, LoggedQso};

/// Why an edit changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestEditRefusal {
    /// No contact in the log has that call and time: it was removed or edited since it was
    /// shown.
    Changed,
    /// The edit names no call, or a band no contest file can carry.
    Incomplete,
    /// The edit gives another number of rows than the contact has (a county line's counties).
    Rows,
    /// A contact worked through a satellite keeps the band of its pass.
    Satellite,
    /// Club sync runs, and an edit cannot reach the club.
    Club,
}

impl ContestEditRefusal {
    /// The refusal as the screen's catalog keys name it.
    pub fn code(self) -> &'static str {
        match self {
            ContestEditRefusal::Changed => "changed",
            ContestEditRefusal::Incomplete => "incomplete",
            ContestEditRefusal::Rows => "rows",
            ContestEditRefusal::Satellite => "satellite",
            ContestEditRefusal::Club => "club",
        }
    }
}

impl From<EditRefusal> for ContestEditRefusal {
    fn from(r: EditRefusal) -> Self {
        match r {
            EditRefusal::Changed => ContestEditRefusal::Changed,
            EditRefusal::Incomplete => ContestEditRefusal::Incomplete,
            EditRefusal::Rows => ContestEditRefusal::Rows,
            EditRefusal::Satellite => ContestEditRefusal::Satellite,
        }
    }
}

/// One edited contact, as the engine answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestEdit {
    /// Its rows as the log table showed them before the edit.
    pub before: Vec<FieldDayQso>,
    /// …and as it shows them now.
    pub after: Vec<FieldDayQso>,
    /// Where it had already gone, which the edit does not reach.
    pub sent: ContestSentTo,
    /// Each row's merge identity — what the logbook names its copy by, if it was merged.
    pub qids: Vec<tempo_core::contest::MergeIdentity>,
}

/// An edit as a command answers it: the engine's answer, plus the upload marks of a copy merged
/// into the logbook before the edit, read with the Engine lock released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestEdited {
    pub edit: ContestEdit,
    /// `Some` when the logbook holds a merged copy, which keeps what it said: the services that
    /// have taken it (or were sent it), by the copy's own upload marks. `None` when it was never
    /// merged, or when the logbook could not be read (the app log says so).
    pub logbook: Option<Vec<&'static str>>,
}

impl Engine {
    /// ⭐ **Edit a contest contact in place** — the one the screen named (`call`, `when_unix`,
    /// what its log table and the strip show), a county line as one contact. Refused at a club
    /// (see the module). `Err` only when no contest runs.
    pub fn contest_edit(
        &mut self,
        call: &str,
        when_unix: u64,
        edit: &ContactEdit,
    ) -> Result<Result<ContestEdit, ContestEditRefusal>, String> {
        let club = self.fd_sync_enabled();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        if club {
            return Ok(Err(ContestEditRefusal::Club));
        }
        let before = match station.log.edit_contact(call, when_unix, edit) {
            Ok(rows) => rows,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log(); // journal it — the edit must survive a crash too
        Ok(Ok(self.contest_edit_of(&before)))
    }

    /// The answer for a contact whose rows were `before`: both sides as the log table shows them,
    /// where it had gone, and the merge identities of its rows.
    fn contest_edit_of(&self, before: &[LoggedQso]) -> ContestEdit {
        let sent = self.contest_sent_to(before);
        let Mode::FieldDay { station, .. } = &self.mode else {
            return ContestEdit {
                before: Vec::new(),
                after: Vec::new(),
                sent,
                qids: Vec::new(),
            };
        };
        let log = &station.log;
        let (rs, spec, role) = (log.ruleset(), log.session.exchange, log.session.role());
        let numbers = log.qso_numbers();
        let shown = |q: &LoggedQso| {
            let nr = numbers.get(&q.seq).copied();
            super::field_day_display::field_day_qso(q, rs, spec, role, nr)
        };
        let after: Vec<&LoggedQso> = log
            .qsos()
            .iter()
            .filter(|q| before.iter().any(|b| b.seq == q.seq))
            .collect();
        ContestEdit {
            before: before.iter().map(shown).collect(),
            after: after.iter().map(|q| shown(q)).collect(),
            sent,
            qids: after
                .iter()
                .map(|q| {
                    tempo_core::contest::MergeIdentity::of(log, &self.settings.fd_position_id, q)
                })
                .collect(),
        }
    }
}

/// ★ [`Engine::contest_edit`] as a command makes it: the edit under the lock, its journal on
/// disk before the answer (the journal is the contact's only copy on disk until it is merged),
/// and a merged copy's upload marks read after — or, when the logbook cannot be read, the copy
/// left unnamed: the edit is made either way, and the answer must say so.
///
/// ⚠️ It waits and reads the store: never call it holding the Engine lock.
pub fn edit(
    engine: &Mutex<Engine>,
    call: &str,
    when_unix: u64,
    edit: &ContactEdit,
) -> Result<Result<ContestEdited, ContestEditRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_edit(call, when_unix, edit)?;
        (answer, e.journal_mark())
    };
    written.wait();
    let edit = match answer {
        Ok(edit) => edit,
        Err(refusal) => return Ok(Err(refusal)),
    };
    // The edit is made and on disk: a logbook that cannot be read now must not turn it into an
    // answer that says nothing was edited. The copy goes unnamed, and the log says why.
    let logbook = match crate::engine::log_plan(engine).merged_uploads(&edit.qids) {
        Ok(marks) => (!marks.is_empty()).then(|| holders(&marks)),
        Err(e) => {
            tempo_core::applog::error(
                "contest",
                &format!(
                    "a contest contact was edited, and the logbook could not be read to say \
                     whether it holds a copy merged before the edit: {e}"
                ),
            );
            None
        }
    };
    Ok(Ok(ContestEdited { edit, logbook }))
}
