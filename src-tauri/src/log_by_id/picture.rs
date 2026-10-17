//! One picture of the log — SPEC-2 v3's **C18** (§4.8): the search the log commands make for the
//! row the log view showed ([`super::find`]). ONE read transaction is one picture of the log,
//! whatever commits while it runs.
//!
//! # What every read of the log here keeps to
//!
//! 1. **Handles under the lock, the read after it.** Under the Engine lock a reader takes the
//!    log's rows (`Engine::log_rows` — the store's handles, or a copy of pointers on the 1.13
//!    path) and the settings it answers for, nothing else; the pass runs once the lock is
//!    released. A debug build panics if it does not (`tempo_core::logbook::io_fence`).
//! 2. **One picture.** A pass over every contact and the whole records of the contacts it picked
//!    come from ONE read transaction ([`read`]), never from two reads a change could land
//!    between.
//! 3. **Never an answer older than the question (P4).** The read first waits, bounded, for the
//!    store to hold every change made before its rows were taken. If the writer is still behind
//!    when the wait runs out, the read is REFUSED as busy rather than served: the log command says
//!    the log could not be read, and changes nothing.
//!
//! SQL narrows, Rust decides (SPEC-2 v3 P2): a reader names the columns it reads ([`Narrow`])
//! and runs its own test on every contact it is handed, so its answer is the one the log in
//! memory gave.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::ControlFlow;

use tempo_app::logstore::{Freshness, LogRows, READ_WAIT};
use tempo_core::logbook::sqlite::{LogDb, Narrow, Order, Scope};
use tempo_core::logbook::{QsoRecord, RecordId};

/// What a read says when the store could not be read. Never an empty answer, which would call
/// every station on the air a new one.
const UNREADABLE: &str = "applicationUnavailable";

/// The log as one read sees it: the store inside one read transaction, on either path — the log is
/// only ever in a store (SPEC-2 v3 C19). [`read`] makes it, for the length of one read.
pub(super) enum Picture<'a> {
    Store(&'a LogDb),
}

/// A contact a pass handed over: its place in log order in this picture (among the contacts its
/// pass visited), and its id — what its whole record is fetched by ([`Picture::whole`]). Two
/// picks of one pass are the same pick when they are the same place, which is the same contact.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pick {
    pub(super) at: usize,
    id: Option<RecordId>,
}

impl PartialEq for Pick {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at
    }
}
impl Eq for Pick {}
impl PartialOrd for Pick {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pick {
    fn cmp(&self, other: &Self) -> Ordering {
        self.at.cmp(&other.at)
    }
}

impl Picture<'_> {
    /// Hand `each` every contact `scope` holds, in log order, with (at least) `narrow`'s fields
    /// filled, and its [`Pick`]; the first refusal `each` answers ends the pass, and is the answer.
    /// On the store it goes through the index that
    /// holds them, on the 1.13 path by the same test made here. A scope narrows, it never decides
    /// (P2): a reader still tests every contact it is handed.
    pub(super) fn each_in(
        &self,
        scope: Scope<'_>,
        narrow: Narrow,
        each: &mut dyn FnMut(Pick, &QsoRecord) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        self.pass(scope, narrow, false, &[], each)
    }

    /// The pass [`Self::each_in`] makes; `submode` reads each contact's SUBMODE as well, and
    /// `extras` the passthrough fields it names.
    fn pass(
        &self,
        scope: Scope<'_>,
        narrow: Narrow,
        submode: bool,
        extras: &[&str],
        each: &mut dyn FnMut(Pick, &QsoRecord) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        match self {
            Picture::Store(db) => {
                let (mut at, mut answer) = (0, Ok(()));
                let mut visit = |q: &QsoRecord| {
                    let pick = Pick { at, id: q.id };
                    at += 1;
                    match each(pick, q) {
                        Ok(()) => ControlFlow::Continue(()),
                        Err(refused) => {
                            answer = Err(refused);
                            ControlFlow::Break(())
                        }
                    }
                };
                if submode {
                    db.each_narrow_with_submode(narrow, scope, Order::Log, &mut visit)
                } else {
                    db.each_narrow_with(narrow, extras, scope, Order::Log, &mut visit)
                }
                .map_err(|_| UNREADABLE)?;
                answer
            }
        }
    }

    /// The whole records of `picks`, in the order given — from this same picture, so a contact
    /// edited or deleted since the pass that picked it is still the contact it picked.
    pub(super) fn whole(&self, picks: &[Pick]) -> Result<Vec<QsoRecord>, &'static str> {
        match self {
            Picture::Store(db) => {
                let ids = picks
                    .iter()
                    .map(|p| p.id.ok_or(UNREADABLE))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut found: HashMap<RecordId, QsoRecord> = db
                    .rows_by_ids(&ids)
                    .map_err(|_| UNREADABLE)?
                    .into_iter()
                    .filter_map(|r| Some((r.id?, r)))
                    .collect();
                // In one picture every contact a pass picked is there to be read.
                ids.iter()
                    .map(|id| found.remove(id).ok_or(UNREADABLE))
                    .collect()
            }
        }
    }

    /// How many contacts the log holds.
    #[cfg(test)]
    pub(super) fn count(&self) -> Result<usize, &'static str> {
        match self {
            Picture::Store(db) => db
                .row_count()
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .map_err(|_| UNREADABLE),
        }
    }
}

/// Run `f` against ONE picture of the log `rows` names — taken under the Engine lock by the
/// caller (`Engine::log_rows`), read here with the lock released. On the store it first waits for
/// every change made before `rows` was taken (P4), and refuses as busy if the writer is still
/// behind; then everything `f` reads comes from one read transaction.
///
/// ⚠️ Never call it holding the Engine lock; a debug build panics.
pub(super) fn read<T>(
    rows: &LogRows,
    f: impl FnOnce(&Picture<'_>) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("a Remote read of the log");
    match rows {
        LogRows::Store(reads) => {
            let (answer, fresh) = reads
                .read(READ_WAIT, |db| Ok(f(&Picture::Store(db))))
                .map_err(|_| UNREADABLE)?;
            match fresh {
                Freshness::Current => answer,
                Freshness::Stale(_) => Err("applicationBusy"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The three rules, each shown on a count of the log.
    use super::*;
    use crate::remote_service::log_tests::{launch, memory, parse_one, settle, synthetic_log, Dir};
    use std::time::Duration;
    use tempo_core::logbook::sqlite::WriteHold;

    /// A contact to log: ZD7AA, now, which no synthetic log holds.
    fn zd7() -> QsoRecord {
        let mut r = parse_one(
            "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
        );
        r.when_unix = 1_788_000_000;
        r
    }

    /// How many contacts the log holds, read as every reader reads it: the rows under the lock,
    /// the read after it is released.
    fn window(e: &crate::SharedEngine) -> Result<usize, &'static str> {
        let rows = e.lock().unwrap().log_rows();
        read(&rows, |log| log.count())
    }

    /// ★ P4, AND THE ENGINE FREE: asked straight after a contact is logged — while its write is
    /// held up, as another process's write holds it — the read waits for it with the Engine lock
    /// free (checked from this thread while it waits), and then has it.
    #[test]
    fn a_read_waits_for_a_contact_just_logged_with_the_engine_lock_free() {
        let d = Dir::new("p4");
        std::fs::write(d.log(), synthetic_log(200, 0x000C_18A4)).unwrap();
        let e = launch(&d);
        let before = window(&e).expect("premise: the log reads");
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(zd7());
        let found = std::thread::scope(|s| {
            // The page's part too: a read refused busy because the write had still not landed
            // when its wait ran out (a loaded machine) is asked again, as the page asks again.
            let asked = s.spawn(|| {
                let deadline = std::time::Instant::now() + tempo_app::test_util::TEST_WAIT;
                loop {
                    match window(&e) {
                        Err("applicationBusy") if std::time::Instant::now() < deadline => {}
                        answer => break answer,
                    }
                }
            });
            std::thread::sleep(Duration::from_millis(250));
            assert!(
                !asked.is_finished(),
                "the read waits for the contact's write"
            );
            for _ in 0..5 {
                assert!(
                    e.try_lock().is_ok(),
                    "the Engine lock is free while it waits"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(hold);
            asked.join().unwrap()
        });
        assert_eq!(
            found,
            Ok(before + 1),
            "the read has the contact logged before it was asked"
        );
        settle(&e);
    }

    /// ★ NEVER OLDER THAN THE QUESTION: a writer still behind when the wait runs out — the
    /// store's lock held past it — makes the read a refusal the page retries, not an answer
    /// missing the contact. The control: the same read once the write has landed answers, with
    /// the contact.
    #[test]
    fn a_read_the_writer_has_not_caught_up_with_is_refused_as_busy() {
        let d = Dir::new("stale");
        std::fs::write(d.log(), synthetic_log(50, 0x000C_18A5)).unwrap();
        let e = launch(&d);
        let before = window(&e).expect("premise: the log reads");
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(zd7());
        let stale = window(&e);
        drop(hold);
        assert_eq!(stale, Err("applicationBusy"));
        // "Once written" is the control's premise, so wait for the write: releasing the hold only
        // lets the writer start, and on a loaded machine its commit can outlast the read's own
        // wait (`READ_WAIT`), which made the control a refusal too.
        settle(&e);
        assert_eq!(
            window(&e),
            Ok(before + 1),
            "control: once written, the read answers"
        );
    }

    /// ★ POSITIVE CONTROL for the fence every read of the log passes: made while this
    /// thread holds an Engine guard, it is a panic in a debug build.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "io_fence: a Remote read of the log is a pass over the whole log")]
    fn a_read_under_the_engine_lock_is_refused() {
        let d = Dir::new("fence");
        let e = memory(&d, &synthetic_log(5, 0x000C_18A6));
        let eng = tempo_app::engine::engine_lock(&e);
        let rows = eng.log_rows();
        let _ = read(&rows, |log| log.count());
    }
}
