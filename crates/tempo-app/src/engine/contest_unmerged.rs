//! ⭐ **The contest contacts no session loads any more, and the Logbook does not hold — offered for
//! the merge, never dropped.**
//!
//! A contest journal restore loads the session's own rows whatever their age, and keeps every
//! record of another contest, or of another running, older than its four days
//! ([`FieldDayLog::aged`](tempo_core::fieldday::FieldDayLog::aged)): it used to let them go,
//! and the next contact rewrote the journal without them, so a contact never merged into the
//! Logbook was then in no file Nexus keeps. The journal cannot see the Logbook, so this is where
//! the two meet ([`tempo_core::contest::settle_aged`]): a contact whose copy is on disk is let go
//! once its contest's running is over, as a merged contact always aged out; one with no copy at
//! all is answered, for the contest screen
//! to say how many, from which contest and when, and to offer the merge
//! ([`tempo_core::contest::plan_aged_merge`]) — the one merge there is, which never writes a
//! contact twice.
//!
//! Both read the Logbook's merge identities with the Engine lock released and act under it only
//! while the Logbook has not changed since, as [`crate::logwrite::fd_merge_to_general`] does.
//! **Nothing here keys, unkeys or stops a transmission**, and nothing in the Logbook is rewritten
//! or deleted: the merge appends.

use std::sync::Mutex;

use super::{engine_lock, sync_shared_log, Engine, Mode};
use crate::logstore::Durability;
use crate::station::{LogPlan, LOG_BUSY, PLANS};
use tempo_core::contest::{ContestSession, MergeReport, Unmerged};

const NOT_ACTIVE: &str = "Contest mode is not active";

#[cfg(test)]
thread_local! {
    // A change to the Logbook made after a plan's read and before the plan is used, on this
    // thread: a test's. (A plain comment: a doc comment cannot attach through `thread_local!`.)
    pub(crate) static RACE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The racing change, if a test set one: made once, with no lock held.
#[cfg(test)]
fn race() {
    let hook = RACE.with(|r| r.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

impl Engine {
    /// [`unmerged`] under the lock: the contacts the journal keeps past its window asked of the
    /// Logbook as `plan` read it — `copies` every merge identity it holds, `on_disk` those of them
    /// on disk — while it has not changed since. `Ok(None)` when it has, changing nothing.
    pub(crate) fn contest_unmerged_planned(
        &mut self,
        plan: &LogPlan,
        copies: &[(String, u64)],
        on_disk: &[(String, u64)],
    ) -> Result<Option<Vec<Unmerged>>, String> {
        let posid = self.settings.fd_position_id.clone();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err(NOT_ACTIVE.into());
        };
        if !self.station.unchanged_at_all_since(plan) {
            return Ok(None);
        }
        Ok(Some(tempo_core::contest::settle_aged(
            &mut station.log,
            &posid,
            copies,
            on_disk,
            super::now_unix_secs(),
        )))
    }

    /// [`merge_unmerged`] under the lock: the contacts of contest `event` the journal keeps past
    /// its window that no copy in the Logbook names, merged as `plan` read the Logbook (`seen`,
    /// as [`Engine::fd_merge_planned`] takes it) while it has not changed since. `Ok(None)` when it
    /// has, changing nothing.
    ///
    /// They are read back in a session of their own contest: this one for its own contacts, and
    /// for another contest's the session entering it would build, from Settings — refused, in
    /// that session's own words, where entering it would be. Nothing is queued for upload: the
    /// session whose upload switch could have said so is long gone, and the switch is off until
    /// an operator turns it on.
    pub(crate) fn contest_merge_unmerged_planned(
        &mut self,
        plan: &LogPlan,
        event: &str,
        seen: Vec<(String, u64)>,
    ) -> Result<Option<MergeReport>, String> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return Err(NOT_ACTIVE.into());
        };
        if !self.station.unchanged_at_all_since(plan) {
            return Ok(None);
        }
        let session = if station.log.ruleset().event == event {
            station.log.session.clone()
        } else {
            let rs = tempo_core::fd_rules::ruleset_by_id(
                event,
                tempo_core::fd_rules::CURRENT_RULES_YEAR,
            )
            .ok_or_else(|| format!("{event} is not a contest this build's rules hold."))?;
            ContestSession::for_ruleset(rs, &self.contest_station_data())?
        };
        let mut report = tempo_core::contest::plan_aged_merge(
            &station.log,
            event,
            &session,
            &self.settings.fd_position_id,
            seen,
        );
        if !report.written.is_empty() {
            // Filled and appended as the end-of-contest merge's are (`fd_merge_planned`).
            let (country, number, state) = (
                self.station.dxcc_resolve.as_deref(),
                self.station.dxcc_country.as_deref(),
                self.station.state_resolve.as_deref(),
            );
            for rec in &mut report.written {
                crate::station::fill_with(rec, country, number, state);
            }
            let (written, _) = self
                .station
                .append(std::mem::take(&mut report.written), false);
            report.written = written;
        }
        tempo_core::applog::info(
            "contest",
            &format!(
                "merged {} {event} contest QSOs the journal kept past its window into the \
                 logbook ({} already there, {} refused)",
                report.added(),
                report.already,
                report.refused
            ),
        );
        Ok(Some(report))
    }
}

/// ★ **The contacts the contest journal keeps past its window that the Logbook does not hold**, by
/// contest — what the contest screen offers to merge — after letting go of every one whose copy is
/// on disk. Nothing to ask when the journal keeps none. The Logbook is read with the Engine lock
/// released, and asked again when it changed meanwhile.
///
/// ⚠️ It reads the store: never call it holding the Engine lock.
pub fn unmerged(engine: &Mutex<Engine>) -> Result<Vec<Unmerged>, String> {
    for _ in 0..PLANS {
        sync_shared_log(engine);
        let plan = {
            let mut e = engine_lock(engine);
            let Mode::FieldDay { station, .. } = &e.mode else {
                return Err(NOT_ACTIVE.into());
            };
            if station.log.aged() == 0 {
                return Ok(Vec::new());
            }
            e.station_mut().log_plan()
        };
        let copies = plan.merge_identities()?;
        let on_disk = plan.durable_merge_identities()?;
        #[cfg(test)]
        race();
        let made = engine_lock(engine).contest_unmerged_planned(&plan, &copies, &on_disk)?;
        if let Some(offer) = made {
            return Ok(offer);
        }
    }
    Err(LOG_BUSY.into())
}

/// ★ **The merge the contest screen offers for contest `event`'s contacts the journal keeps past
/// its window** ([`unmerged`]): the Logbook's merge identities read with the Engine lock released,
/// the merge made under it while the Logbook has not changed since, and planned again otherwise.
/// What it merged, and the durability of its writes for the command to wait on.
///
/// ⚠️ It reads the store: never call it holding the Engine lock.
pub fn merge_unmerged(
    engine: &Mutex<Engine>,
    event: &str,
) -> (Result<MergeReport, String>, Durability) {
    for _ in 0..PLANS {
        sync_shared_log(engine);
        let plan = {
            let mut e = engine_lock(engine);
            if !e.in_field_day() {
                return (Err(NOT_ACTIVE.into()), Durability::default());
            }
            e.station_mut().log_plan()
        };
        let seen = match plan.merge_identities() {
            Ok(seen) => seen,
            Err(e) => return (Err(e), Durability::default()),
        };
        #[cfg(test)]
        race();
        let (made, durability) = engine_lock(engine)
            .with_log_tickets(|e| e.contest_merge_unmerged_planned(&plan, event, seen));
        match made {
            Ok(Some(report)) => return (Ok(report), durability),
            Ok(None) => {}
            Err(e) => return (Err(e), durability),
        }
    }
    (Err(LOG_BUSY.into()), Durability::default())
}
