//! The Logbook's "Update from QRZ": many calls looked up in the callbook, slowly, and their
//! answers handed to the fill ([`crate::logtools::callbook_fill`]), which writes each into the
//! EMPTY fields of the contacts logged with that call and nowhere else.
//!
//! This module is the lookups' loop and nothing more: which call next, when the next network
//! lookup may start, and when to stop. The lookup itself is the caller's ([`Callbook`]) — the
//! shell hands in the same cached lookup the callsign card makes, with the account it already
//! holds, so nothing here touches a password, a session or the network.
//!
//! # Paced, and capped per day
//!
//! - **A kept answer costs nothing.** A call the lookup cache answers fresh ([`Callbook::kept`])
//!   is taken at once: no wait, and not counted against the cap.
//! - **Every network lookup waits its turn first** ([`Limits::pace`]), the first one too, so `N`
//!   network lookups take at least `N × pace` and a run started straight after another cannot
//!   burst. The wait is cancellable.
//! - **The cap is per 24 hours, across runs** ([`Limits::daily_cap`]). Before each network
//!   lookup, what it would cost ([`Callbook::cost`]: the stroked call and its base are two) is
//!   added to what was spent — the larger of the station's own tally ([`Callbook::spent`], kept on
//!   disk) and QRZ's own count of the account's lookups, when an answer this run carried one — and
//!   a lookup that would pass the cap is not made: the run stops, [`Stop::Limit`]. QRZ publishes
//!   no daily number, so the cap is the operator's setting, 250 by default.
//! - **A call the callbook does not know** is reported ([`Lookups::unknown`]) and its contacts left
//!   alone. **A lookup that fails** (no sign-in, the network down) stops the run, [`Stop::Failed`]:
//!   the next lookup would only fail the same way, and each failure is still a request to QRZ.
//!
//! Whatever stops the run, the answers already in hand are the run's: the caller fills them as
//! ONE change, and a later run picks the rest up from the lookup cache at no cost.

use std::collections::HashMap;
use std::time::Duration;

use tempo_core::logbook::callbook::CallbookAnswer;

/// How fast and how far a run may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The least time before each network lookup.
    pub pace: Duration,
    /// The most network lookups in 24 hours, across runs.
    pub daily_cap: usize,
}

/// What one network lookup answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Looked {
    Found(Box<CallbookAnswer>),
    /// The callbook was asked and holds no such call.
    Unknown,
    /// The lookup could not be made: the reason, as the operator reads it.
    Failed(String),
}

/// The callbook as the loop uses it. The shell's is the cached lookup the callsign card makes;
/// the tests' a script with a fake clock.
pub trait Callbook {
    /// The answer the lookup cache holds for `call`, while it is fresh. Never the network.
    fn kept(&mut self, call: &str) -> Option<CallbookAnswer>;
    /// How many lookups a network lookup of `call` may make: the call, then its base for a
    /// stroked call. Counted against the cap before it is made.
    fn cost(&self, call: &str) -> usize {
        let _ = call;
        1
    }
    /// The network lookups this station noted in the last 24 hours.
    fn spent(&mut self) -> usize;
    /// Note `n` network lookups, made now.
    fn note_spent(&mut self, n: usize);
    /// Look `call` up over the network, and QRZ's own count of the account's lookups in the last
    /// 24 hours when its answer carried one.
    fn live(&mut self, call: &str) -> (Looked, Option<usize>);
    /// Wait `d`. `false`: the run was cancelled while it waited.
    fn wait(&mut self, d: Duration) -> bool;
    /// Whether the operator cancelled the run.
    fn cancelled(&self) -> bool;
}

/// Why a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Every call was looked up.
    Done,
    Cancelled,
    /// The next network lookup would have passed the daily cap.
    Limit,
    /// A lookup failed: the reason.
    Failed(String),
}

/// How far a run has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Calls answered, from the cache or the network, or found unknown.
    pub done: usize,
    /// The calls the run was handed.
    pub total: usize,
    /// Of `done`, those the lookup cache answered.
    pub from_cache: usize,
    /// Network lookups made.
    pub looked_up: usize,
    /// Network lookups spent in the last 24 hours, as the cap reads them.
    pub spent: usize,
}

/// What a run found.
#[derive(Debug, Clone, PartialEq)]
pub struct Lookups {
    /// Each call answered, by its key ([`tempo_core::logbook::cleanup::call_key`]).
    pub answers: HashMap<String, CallbookAnswer>,
    /// The calls the callbook does not know.
    pub unknown: Vec<String>,
    pub stop: Stop,
    pub progress: Progress,
}

/// ★ Look `calls` up, kept answers first-hand and the rest over the network at `limits`' pace,
/// until every call is answered, the operator cancels, the cap is reached or a lookup fails.
/// `progress` is told after each call. See the module header.
pub fn look_up(
    calls: &[String],
    limits: Limits,
    callbook: &mut impl Callbook,
    progress: &mut dyn FnMut(&Progress),
) -> Lookups {
    let mut out = Lookups {
        answers: HashMap::new(),
        unknown: Vec::new(),
        stop: Stop::Done,
        progress: Progress {
            total: calls.len(),
            spent: callbook.spent(),
            ..Progress::default()
        },
    };
    // QRZ's own count, once an answer this run carried one.
    let mut qrz_count: Option<usize> = None;
    for call in calls {
        if callbook.cancelled() {
            out.stop = Stop::Cancelled;
            break;
        }
        if let Some(answer) = callbook.kept(call) {
            out.answers.insert(call.clone(), answer);
            out.progress.from_cache += 1;
            out.progress.done += 1;
            progress(&out.progress);
            continue;
        }
        let spent = callbook.spent().max(qrz_count.unwrap_or(0));
        out.progress.spent = spent;
        let cost = callbook.cost(call);
        if spent + cost > limits.daily_cap {
            out.stop = Stop::Limit;
            break;
        }
        if !callbook.wait(limits.pace) {
            out.stop = Stop::Cancelled;
            break;
        }
        // Noted before it is made: a lookup that fails was still a request.
        callbook.note_spent(cost);
        let (looked, count) = callbook.live(call);
        out.progress.looked_up += 1;
        if count.is_some() {
            qrz_count = count;
        }
        out.progress.spent = callbook.spent().max(qrz_count.unwrap_or(0));
        match looked {
            Looked::Found(answer) => {
                out.answers.insert(call.clone(), *answer);
            }
            Looked::Unknown => out.unknown.push(call.clone()),
            Looked::Failed(why) => {
                out.stop = Stop::Failed(why);
                break;
            }
        }
        out.progress.done += 1;
        progress(&out.progress);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A scripted callbook on a fake clock: `kept` answers from the cache, `known` over the
    /// network, `failing` fails; every other call is unknown.
    #[derive(Default)]
    struct Script {
        kept: Vec<&'static str>,
        known: Vec<&'static str>,
        failing: Vec<&'static str>,
        /// The fake clock, in milliseconds.
        now_ms: u64,
        /// When each network lookup started, by call.
        lookups: Vec<(String, u64)>,
        /// The station's tally: the time of each lookup noted.
        tally: Vec<u64>,
        /// Spent before the run, in the last day.
        spent_before: usize,
        /// QRZ's count, as its answers carry it: `Some(n)` reports `n + lookups made`.
        qrz_base: Option<usize>,
        /// Cancel once this many network lookups have been made.
        cancel_after: Option<usize>,
        cancelled: Cell<bool>,
    }

    fn answer(call: &str) -> CallbookAnswer {
        CallbookAnswer {
            call: call.into(),
            name: Some("Op".into()),
            ..CallbookAnswer::default()
        }
    }

    impl Callbook for Script {
        fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
            self.kept.contains(&call).then(|| answer(call))
        }
        fn cost(&self, call: &str) -> usize {
            if call.contains('/') {
                2
            } else {
                1
            }
        }
        fn spent(&mut self) -> usize {
            self.spent_before + self.tally.len()
        }
        fn note_spent(&mut self, n: usize) {
            for _ in 0..n {
                self.tally.push(self.now_ms);
            }
        }
        fn live(&mut self, call: &str) -> (Looked, Option<usize>) {
            self.lookups.push((call.to_string(), self.now_ms));
            if self.cancel_after.is_some_and(|n| self.lookups.len() >= n) {
                self.cancelled.set(true);
            }
            let count = self.qrz_base.map(|b| b + self.lookups.len());
            let looked = if self.failing.contains(&call) {
                Looked::Failed("QRZ could not be reached".into())
            } else if self.known.contains(&call) {
                Looked::Found(Box::new(answer(call)))
            } else {
                Looked::Unknown
            };
            (looked, count)
        }
        fn wait(&mut self, d: Duration) -> bool {
            if self.cancelled.get() {
                return false;
            }
            self.now_ms += d.as_millis() as u64;
            true
        }
        fn cancelled(&self) -> bool {
            self.cancelled.get()
        }
    }

    fn calls(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| s.to_string()).collect()
    }

    const PACE: Duration = Duration::from_millis(1_500);

    fn limits(cap: usize) -> Limits {
        Limits {
            pace: PACE,
            daily_cap: cap,
        }
    }

    /// ★ PACING. `N` network lookups take at least `N × pace` on the clock, each starting at
    /// least `pace` after the one before (and the first `pace` after the start).
    #[test]
    fn network_lookups_are_paced() {
        let mut cb = Script {
            known: vec!["W1AW", "K1ABC", "N0XYZ", "G4ABC"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["W1AW", "K1ABC", "N0XYZ", "G4ABC"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers.len(), 4);
        let starts: Vec<u64> = cb.lookups.iter().map(|(_, t)| *t).collect();
        assert_eq!(starts, vec![1_500, 3_000, 4_500, 6_000]);
        assert!(cb.now_ms >= 4 * PACE.as_millis() as u64);
    }

    /// ★ THE CAP. A run never makes a lookup that would pass the day's cap, counting what was
    /// spent before it; it stops with what it has, and a stroked call costs two.
    #[test]
    fn the_cap_is_never_passed() {
        let all = calls(&["A1A", "B1B", "C1C", "D1D", "E1E"]);
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D", "E1E"],
            spent_before: 2,
            ..Script::default()
        };
        let run = look_up(&all, limits(5), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(cb.lookups.len(), 3, "2 spent before + 3 = the cap of 5");
        assert_eq!(cb.spent(), 5);
        assert_eq!(run.answers.len(), 3);
        assert_eq!(run.progress.done, 3);

        // One left under the cap, and the next call is stroked: it would cost two. Not made.
        let mut cb = Script {
            known: vec!["A1A", "W1AW/P"],
            spent_before: 3,
            ..Script::default()
        };
        let run = look_up(&calls(&["A1A", "W1AW/P"]), limits(5), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(cb.spent(), 4, "never past the cap");

        // At the cap already: nothing is asked at all.
        let mut cb = Script {
            known: vec!["A1A"],
            spent_before: 5,
            ..Script::default()
        };
        let run = look_up(&calls(&["A1A"]), limits(5), &mut cb, &mut |_| {});
        assert_eq!((run.stop, cb.lookups.len()), (Stop::Limit, 0));
    }

    /// QRZ's own count, when its answers carry one, holds the cap too: lookups made elsewhere
    /// (another program on the same account) count.
    #[test]
    fn qrzs_own_count_holds_the_cap() {
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D"],
            qrz_base: Some(8),
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(10),
            &mut cb,
            &mut |_| {},
        );
        // First lookup: QRZ says 9. Second: 10. The third would be 11.
        assert_eq!(run.stop, Stop::Limit);
        assert_eq!(cb.lookups.len(), 2);
        assert_eq!(run.progress.spent, 10);
    }

    /// ★ Cache hits cost nothing: no wait and no count, whatever the cap.
    #[test]
    fn kept_answers_neither_wait_nor_count() {
        let mut cb = Script {
            kept: vec!["A1A", "B1B", "C1C"],
            known: vec!["D1D"],
            spent_before: 0,
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(1),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers.len(), 4);
        assert_eq!((run.progress.from_cache, run.progress.looked_up), (3, 1));
        assert_eq!(cb.spent(), 1, "only the network lookup counted");
        assert_eq!(cb.now_ms, PACE.as_millis() as u64, "one wait, for D1D");
    }

    /// ★ Cancel: the run stops where it is, keeping every answer it had.
    #[test]
    fn cancel_keeps_what_was_found() {
        let mut cb = Script {
            known: vec!["A1A", "B1B", "C1C", "D1D"],
            cancel_after: Some(2),
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C", "D1D"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Cancelled);
        assert_eq!(cb.lookups.len(), 2);
        let mut got: Vec<&String> = run.answers.keys().collect();
        got.sort();
        assert_eq!(got, ["A1A", "B1B"]);
    }

    /// ★ A call the callbook does not know is reported and left alone; the run goes on.
    #[test]
    fn an_unknown_call_is_reported_and_the_run_goes_on() {
        let mut cb = Script {
            known: vec!["A1A", "C1C"],
            ..Script::default()
        };
        let run = look_up(
            &calls(&["A1A", "Q9QQQ", "C1C"]),
            limits(250),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.unknown, ["Q9QQQ"]);
        assert!(!run.answers.contains_key("Q9QQQ"));
        assert_eq!(run.answers.len(), 2);
        assert_eq!(run.progress.done, 3);
    }

    /// A lookup that fails stops the run: the next would fail the same way. What it found
    /// stands, and the failure was still a request, so it is counted.
    #[test]
    fn a_failed_lookup_stops_the_run() {
        let mut cb = Script {
            known: vec!["A1A", "C1C"],
            failing: vec!["B1B"],
            ..Script::default()
        };
        let mut seen = Vec::new();
        let run = look_up(
            &calls(&["A1A", "B1B", "C1C"]),
            limits(250),
            &mut cb,
            &mut |p| seen.push(p.done),
        );
        assert_eq!(run.stop, Stop::Failed("QRZ could not be reached".into()));
        assert_eq!(cb.lookups.len(), 2, "C1C never asked");
        assert_eq!(cb.spent(), 2);
        assert_eq!(run.answers.len(), 1);
        assert_eq!(seen, [1], "progress after each call answered");
    }
}
