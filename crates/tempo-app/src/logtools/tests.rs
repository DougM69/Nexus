//! The clean-up tools through the engine and the real store: a merge, a bulk edit and the check's
//! fixes each change the log as one run of the history, the `log.adi` mirror follows, the run's
//! undo puts every contact back, nothing is queued to a service, and a large run waits while the
//! radio transmits.

use super::*;
use crate::loghistory::undo_run;
use crate::logstore::tests::{engine_on_store, flush, Dir};
use crate::logwrite;
use crate::test_util::StoredLog;
use std::cell::RefCell;
use tempo_core::logbook::{adif_header, UploadOutcome, UploadService, UploadStatus};

thread_local! {
    // A writer racing the change planned on this thread: run after its plan's read, with no lock
    // held, before the change is made. (A plain comment: a doc comment cannot attach through
    // `thread_local!`.)
    static RACE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// [`run_in_parts`]'s seam: the racing writer, if a test set one.
pub(super) fn race() {
    let hook = RACE.with(|r| r.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook();
        RACE.with(|r| *r.borrow_mut() = Some(hook));
    }
}

fn racing<T>(hook: impl FnMut() + 'static, f: impl FnOnce() -> T) -> T {
    RACE.with(|r| *r.borrow_mut() = Some(Box::new(hook)));
    let out = f();
    RACE.with(|r| *r.borrow_mut() = None);
    out
}

/// One ADIF record: `call` on 20 m FT8 on 1 January 2026 at `time`, with `more` fields.
fn adif(call: &str, time: &str, more: &str) -> String {
    format!(
        "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>{time}\
         {more}<EOR>\n",
        call.len()
    )
}

/// A shared engine on a store converted from a `log.adi` of `records`.
fn shared(d: &Dir, records: &[String]) -> Arc<Mutex<Engine>> {
    std::fs::write(d.log(), adif_header() + &records.concat()).unwrap();
    let e = engine_on_store(d);
    flush(&e);
    Arc::new(Mutex::new(e))
}

fn rows(engine: &Mutex<Engine>) -> Vec<QsoRecord> {
    engine
        .stored_log()
        .iter()
        .map(|r| QsoRecord::clone(r))
        .collect()
}

fn by_call(engine: &Mutex<Engine>, call: &str) -> Vec<QsoRecord> {
    rows(engine)
        .into_iter()
        .filter(|r| r.call == call)
        .collect()
}

fn mirror(d: &Dir) -> String {
    std::fs::read_to_string(d.log()).unwrap()
}

fn settle(engine: &Mutex<Engine>) {
    flush(&engine_lock(engine));
}

fn stamp(engine: &Mutex<Engine>, r: &QsoRecord, service: UploadService, when_unix: i64) {
    let (_, d) = logwrite::stamp_push(
        engine,
        r,
        service,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix,
            detail: None,
        },
    );
    d.wait(DURABLE_WAIT).unwrap();
}

/// The merge the finder suggests for its group `g`, as the window sends it back.
fn ask(g: &DupeGroup, choices: &[(&str, usize)]) -> MergeAsk {
    MergeAsk {
        keep: g.copies[g.keep].id.unwrap(),
        copies: g
            .copies
            .iter()
            .map(|r| (r.id.unwrap(), QsoEdit::project(r).key()))
            .collect(),
        choices: choices
            .iter()
            .map(|(tag, i)| (tag.to_string(), g.copies[*i].id.unwrap()))
            .collect(),
    }
}

/// Two copies of W1AW, two minutes apart: one LoTW-confirmed and sent to LoTW, one with a name,
/// a grid and a QRZ stamp — and a contact logged an hour later, which only the possible tier
/// groups with them.
fn dupes(d: &Dir) -> Arc<Mutex<Engine>> {
    let engine = shared(
        d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y<RST_SENT:3>-10"),
            adif("K1ABC", "120100", ""),
            adif(
                "W1AW",
                "120200",
                "<NAME:5>Hiram<QTH:9>Newington<GRIDSQUARE:4>FN31<CQZ:1>5",
            ),
            adif("W1AW", "125900", "<COMMENT:5>later"),
        ],
    );
    let w = by_call(&engine, "W1AW");
    stamp(&engine, &w[0], UploadService::Lotw, 100);
    stamp(&engine, &w[1], UploadService::Qrz, 200);
    engine_lock(&engine).take_pending_uploads();
    engine
}

/// ★ THE MERGE, END TO END. The finder groups the two copies by the log's rule (the contact an
/// hour later only in the possible tier); the merge leaves ONE contact holding the union — the
/// record asserted whole against the rule's own answer on the copies as stored, both stamps on
/// it — deletes the other, and the mirror follows. It queues nothing. Its run's undo puts both
/// copies back, each field for field as it was, the kept one's stamps aside: a stamp is never
/// history, so the kept contact keeps the QRZ stamp the merge gave it.
#[test]
fn a_merge_keeps_the_union_and_its_undo_puts_both_copies_back() {
    let d = Dir::new("tools-merge");
    let engine = dupes(&d);
    let before = by_call(&engine, "W1AW");

    let possible = find_duplicates(&engine, DupeTier::Possible).unwrap();
    assert_eq!(possible.scanned, 4);
    assert_eq!(possible.groups.len(), 1);
    assert_eq!(possible.groups[0].copies.len(), 3);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    assert_eq!(scan.groups.len(), 1, "{:?}", scan.groups);
    let g = &scan.groups[0];
    assert_eq!(
        g.copies,
        before[..2].to_vec(),
        "the two copies, whole, in log order"
    );
    assert_eq!(g.keep, 1, "the fuller copy is suggested");

    let want = cleanup::merge(&g.copies, g.keep, &HashMap::new()).record;
    assert_eq!(want.upload.lotw.as_ref().map(|s| s.when_unix), Some(100));
    assert_eq!(want.upload.qrz.as_ref().map(|s| s.when_unix), Some(200));
    assert!(want.qsl_rcvd.lotw && want.name.as_deref() == Some("Hiram"));
    assert_eq!(want.rst_sent.as_deref(), Some("-10"));

    let (made, dur) = merge_duplicates(&engine, &[ask(g, &[])]);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 2, "one kept and changed, one deleted");
    assert!(made.refused.is_empty());
    let run = made.run.clone().expect("a run");
    let after = by_call(&engine, "W1AW");
    assert_eq!(
        after.len(),
        2,
        "the copy merged in is gone; the later contact stays"
    );
    assert_eq!(after[0], want, "the merged contact, whole");
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "a merge queues nothing"
    );
    settle(&engine);
    assert_eq!(
        mirror(&d).matches("<CALL:4>W1AW").count(),
        2,
        "log.adi follows"
    );

    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let back = by_call(&engine, "W1AW");
    assert_eq!(back.len(), 3);
    for was in &before {
        let now = back.iter().find(|r| r.id == was.id).expect("back by id");
        if was.id == want.id {
            assert_eq!(
                QsoRecord {
                    upload: was.upload.clone(),
                    ..now.clone()
                },
                *was,
                "the kept copy, every field but its stamps"
            );
            assert_eq!(now.upload, want.upload, "the stamps the merge gave it stay");
        } else {
            assert_eq!(now, was, "the copy merged in, whole");
        }
    }
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());
    settle(&engine);
    assert_eq!(mirror(&d).matches("<CALL:4>W1AW").count(), 3);
}

/// A merge whose copies disagree on a field a confirmed copy holds waits for the operator's
/// choice, changing nothing; chosen, it is made with the value chosen.
#[test]
fn a_guarded_conflict_waits_for_a_choice() {
    let d = Dir::new("tools-merge-guarded");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y<GRIDSQUARE:4>FN31"),
            adif("W1AW", "120100", "<GRIDSQUARE:4>FN42<NAME:5>Hiram"),
        ],
    );
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let g = &scan.groups[0];
    let grid = g.conflicts.iter().find(|c| c.tag == "GRIDSQUARE").unwrap();
    assert!(grid.guarded);
    let keep_other = MergeAsk {
        keep: g.copies[1].id.unwrap(),
        ..ask(g, &[])
    };

    let (made, _) = merge_duplicates(&engine, std::slice::from_ref(&keep_other));
    let made = made.unwrap();
    assert_eq!(made.changed, 0);
    assert_eq!(made.run, None);
    assert_eq!(made.refused, vec![(keep_other.keep, "needsChoice")]);
    assert_eq!(rows(&engine).len(), 2, "nothing was merged");

    let chosen = MergeAsk {
        choices: [("GRIDSQUARE".to_string(), g.copies[0].id.unwrap())].into(),
        ..keep_other
    };
    let (made, dur) = merge_duplicates(&engine, &[chosen]);
    assert_eq!(made.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let left = rows(&engine);
    assert_eq!(left.len(), 1);
    assert_eq!(
        left[0].grid.as_deref(),
        Some("FN31"),
        "the confirmed copy's, as chosen"
    );
    assert!(left[0].qsl_rcvd.lotw);
    assert_eq!(left[0].name.as_deref(), Some("Hiram"));
}

/// A copy another writer edits after the merge was shown — even between the plan's read and the
/// change — is never merged over: the merge plans again, finds the copy changed, and leaves the
/// group as it is.
#[test]
fn a_copy_changed_since_it_was_shown_is_not_merged_over() {
    let d = Dir::new("tools-merge-changed");
    let engine = dupes(&d);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let the_ask = ask(&scan.groups[0], &[]);
    let target = scan.groups[0].copies[0].clone();
    let once = std::rc::Rc::new(std::cell::Cell::new(false));
    let hook = {
        let (engine, once) = (Arc::clone(&engine), once.clone());
        move || {
            if !once.replace(true) {
                let key = QsoEdit::project(&target).key();
                let mut edit = QsoEdit::project(&target);
                edit.comment = Some("theirs".into());
                let (made, d) = logwrite::edit_row(&engine, target.id.unwrap(), &key, &edit);
                made.unwrap().unwrap();
                d.wait(DURABLE_WAIT).unwrap();
            }
        }
    };
    let (made, _) = racing(hook, || {
        merge_duplicates(&engine, std::slice::from_ref(&the_ask))
    });
    let made = made.unwrap();
    assert_eq!(made.changed, 0);
    assert_eq!(made.refused, vec![(the_ask.keep, "changed")]);
    assert_eq!(by_call(&engine, "W1AW").len(), 3, "both copies stand");
    assert!(by_call(&engine, "W1AW")
        .iter()
        .any(|r| r.comment.as_deref() == Some("theirs")));
}

/// ★ BULK EDIT. The preview counts what the edit would change; the edit makes exactly that, as
/// one run, keeping every confirmation and upload stamp and queueing nothing — a call is not a
/// field it offers — and the run's undo puts every contact back whole.
#[test]
fn a_bulk_edit_previews_applies_and_undoes_and_keeps_every_stamp() {
    let d = Dir::new("tools-bulk");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<MY_GRIDSQUARE:4>FN20<LOTW_QSL_RCVD:1>Y"),
            adif("K1ABC", "120100", "<MY_GRIDSQUARE:4>EM10"),
            adif("N0XYZ", "120200", ""),
        ],
    );
    let before = rows(&engine);
    stamp(&engine, &before[1], UploadService::Qrz, 300);
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let changes = [
        Change {
            field: Field::MyGridsquare,
            value: Some("EM10".into()),
        },
        Change {
            field: Field::Operator,
            value: Some("k2def".into()),
        },
    ];

    let preview = bulk_preview(&engine, &ids, &changes).unwrap();
    assert_eq!(
        preview,
        BulkPreview {
            selected: 3,
            change: 3,
            unchanged: 0,
            missing: 0
        }
    );
    let only_grid = &changes[..1];
    let preview = bulk_preview(&engine, &ids, only_grid).unwrap();
    assert_eq!(
        (preview.change, preview.unchanged),
        (2, 1),
        "K1ABC is already EM10"
    );

    let (made, dur) = bulk_edit(&engine, &ids, &changes);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    let after = rows(&engine);
    for (was, now) in before.iter().zip(&after) {
        let mut want = was.clone();
        want.my_grid = Some("EM10".into());
        want.operator = Some("K2DEF".into());
        assert_eq!(
            *now, want,
            "only the two fields, every stamp and confirmation kept"
        );
    }
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "a bulk edit queues nothing"
    );
    settle(&engine);
    assert_eq!(mirror(&d).matches("<OPERATOR:5>K2DEF").count(), 3);

    let call = [Change {
        field: Field::Gridsquare,
        value: Some("FN31".into()),
    }];
    assert!(
        bulk_edit(&engine, &ids, &call).0.is_err(),
        "not a bulk field"
    );

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before, "every contact back, whole");
}

/// ★ THE CHECK. Each kind of problem is found with its proposed fix; only the fixes ticked are
/// made, as one run; a fix whose field changed since the check is left out; the run's undo puts
/// the contacts back.
#[test]
fn the_check_finds_each_problem_and_makes_only_the_ticked_fixes() {
    let d = Dir::new("tools-check");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<GRIDSQUARE:7>FN31PRX<DXCC:3>291"),
            adif("K1ABC", "120100", "<MY_GRIDSQUARE:4>ZZ99"),
            "<CALL:5>N0XYZ<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>120200<EOR>\n"
                .into(),
            adif("VE3AA", "120300", "").replace("<BAND:3>20m", "<BAND:3>40m"),
            adif(
                "G4AAA",
                "235000",
                "<QSO_DATE_OFF:8>20260101<TIME_OFF:6>000500",
            ),
            adif("JA1AA", "120500", "<SIG:4>POTA<SIG_INFO:6>us1234"),
            adif("W2AA", "120600", "<MY_SIG:4>SOTA<MY_SIG_INFO:6>W2/GC1"),
            "<CALL:4>W3AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20990101<TIME_ON:6>120000<EOR>\n".into(),
        ],
    );
    // The country file's answer, as the command layer gives it: W1AW is 291, never 1.
    let mismatch = |call: &str, n: u32| (call == "W1AW" && n != 1).then_some(1);
    let now = 1_790_000_000;
    let (checked, problems) = check(&engine, &mismatch, now).unwrap();
    assert_eq!(checked, 8);
    let found: Vec<(String, ProblemKind, Option<Option<String>>)> = problems
        .iter()
        .map(|p| {
            (
                p.call.clone(),
                p.kind,
                p.fix.as_ref().map(|f| f.value.clone()),
            )
        })
        .collect();
    let s = |v: &str| Some(Some(v.to_string()));
    assert_eq!(
        found,
        vec![
            ("W1AW".into(), ProblemKind::Grid, s("FN31PR")),
            ("W1AW".into(), ProblemKind::Dxcc, s("1")),
            ("K1ABC".into(), ProblemKind::MyGrid, Some(None)),
            ("N0XYZ".into(), ProblemKind::NoBand, s("20m")),
            ("VE3AA".into(), ProblemKind::BandFreq, s("20m")),
            ("G4AAA".into(), ProblemKind::EndBeforeStart, s("1767312300")),
            ("JA1AA".into(), ProblemKind::Pota, s("US-1234")),
            ("W2AA".into(), ProblemKind::Sota, None),
            ("W3AA".into(), ProblemKind::Future, None),
        ],
    );
    let before = rows(&engine);

    // Tick the grid, the band and the park — not the DXCC number. K1ABC's grid is edited in
    // between: its fix is left out.
    let fix = |p: &Problem| Fix {
        id: p.id,
        field: p.field.unwrap(),
        expect: p.current.clone(),
        value: p.fix.clone().unwrap().value,
    };
    let ticked: Vec<Fix> = problems
        .iter()
        .filter(|p| {
            matches!(
                p.kind,
                ProblemKind::Grid | ProblemKind::NoBand | ProblemKind::Pota | ProblemKind::MyGrid
            )
        })
        .map(fix)
        .collect();
    let k1 = before.iter().find(|r| r.call == "K1ABC").unwrap();
    let mut edit = QsoEdit::project(k1);
    edit.my_grid = Some("EM10".into());
    let (m, dd) = logwrite::edit_row(&engine, k1.id.unwrap(), &QsoEdit::project(k1).key(), &edit);
    m.unwrap().unwrap();
    dd.wait(DURABLE_WAIT).unwrap();
    let before = rows(&engine);

    let (made, dur) = apply_fixes(&engine, &ticked);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    assert_eq!(made.refused, vec![(k1.id.unwrap(), "changed")]);
    let after = rows(&engine);
    let of = |call: &str| after.iter().find(|r| r.call == call).unwrap().clone();
    assert_eq!(of("W1AW").grid.as_deref(), Some("FN31PR"));
    assert_eq!(of("W1AW").dxcc, Some(291), "not ticked: not made");
    assert_eq!(of("N0XYZ").band, "20m");
    assert_eq!(of("JA1AA").ota.their_ref.as_deref(), Some("US-1234"));
    assert_eq!(
        of("K1ABC").my_grid.as_deref(),
        Some("EM10"),
        "the operator's edit stands"
    );
    for (was, now) in before.iter().zip(&after) {
        if !["W1AW", "N0XYZ", "JA1AA"].contains(&was.call.as_str()) {
            assert_eq!(was, now, "{} untouched", was.call);
        }
    }
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// ⛔ A run of more than the writer's chunk is refused while the FT sequencer is armed and changes
/// nothing — as a large undo is — while a small one is made; disarmed, the large one is made.
#[test]
fn a_large_run_waits_until_transmit_is_off() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 20;
    let d = Dir::new("tools-large-tx");
    let records: Vec<String> = (0..n)
        .map(|i| {
            adif(
                &format!("K{i}ABC"),
                &format!("12{:02}{:02}", i / 60 % 60, i % 60),
                "",
            )
        })
        .collect();
    let engine = shared(&d, &records);
    let ids: Vec<RecordId> = rows(&engine).iter().map(|r| r.id.unwrap()).collect();
    let changes = [Change {
        field: Field::MyRig,
        value: Some("IC-7300".into()),
    }];
    engine_lock(&engine).set_tx_enabled(true);
    assert!(
        engine_lock(&engine).transmit_needs_the_lock(),
        "premise: armed for FT"
    );

    let (refused, _) = bulk_edit(&engine, &ids, &changes);
    assert_eq!(refused, Err(MUST_WAIT_FOR_TX.to_string()));
    assert!(
        rows(&engine).iter().all(|r| r.my_rig.is_none()),
        "nothing was made"
    );
    let (small, d1) = bulk_edit(&engine, &ids[..3], &changes);
    assert_eq!(small.unwrap().changed, 3, "a small run is made");
    d1.wait(DURABLE_WAIT).unwrap();

    engine_lock(&engine).set_tx_enabled(false);
    let (made, dur) = bulk_edit(&engine, &ids, &changes);
    assert_eq!(made.unwrap().changed, n - 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(rows(&engine)
        .iter()
        .all(|r| r.my_rig.as_deref() == Some("IC-7300")));
}

/// ★ ONE RUN ACROSS PARTS. A bulk edit of more contacts than one change carries ([`CHUNK`]) is
/// made in several changes, and they are one run: its undo puts every contact back, the ones of
/// the first change and of the last alike.
#[test]
fn a_run_made_in_several_changes_is_undone_whole() {
    let n = CHUNK + 6;
    let d = Dir::new("tools-parts");
    let records: Vec<String> = (0..n)
        .map(|i| {
            adif(
                &format!("K{i}ABC"),
                &format!("{:02}{:02}{:02}", i / 3600, i / 60 % 60, i % 60),
                "",
            )
        })
        .collect();
    let engine = shared(&d, &records);
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let (made, dur) = bulk_edit(
        &engine,
        &ids,
        &[Change {
            field: Field::StationCallsign,
            value: Some("K2DEF".into()),
        }],
    );
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, n);
    assert!(rows(&engine)
        .iter()
        .all(|r| r.station_callsign.as_deref() == Some("K2DEF")));
    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, n, "both changes' contacts");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// The bench: at 150,000 contacts, the duplicate scan's time, and the longest hold of the Engine
/// lock one chunk of a bulk edit of every contact takes. Run in release:
/// `cargo test -p tempo-app --release --lib logtools -- --ignored --nocapture`.
#[test]
#[ignore = "the 150k bench: run in release"]
fn bench_150k() {
    let n = 150_000;
    let d = Dir::new("tools-bench");
    let mut s = adif_header();
    for i in 0..n {
        let call = format!("K{}A{}", i % 10, i / 10);
        let (day, sec) = (1 + i / 86_400 % 28, i % 86_400);
        s.push_str(&format!(
            "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>202601{day:02}\
             <TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
            call.len(),
            sec / 3600,
            sec / 60 % 60,
            sec % 60,
        ));
        // Every hundredth contact logged twice, a minute later.
        if i % 100 == 0 {
            let sec = sec.min(86_340) + 60;
            s.push_str(&format!(
                "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>202601{day:02}\
                 <TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
                call.len(),
                sec / 3600,
                sec / 60 % 60,
                sec % 60,
            ));
        }
    }
    std::fs::write(d.log(), s).unwrap();
    let e = engine_on_store(&d);
    flush(&e);
    let engine = Arc::new(Mutex::new(e));

    let t = Instant::now();
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let scan_time = t.elapsed();
    assert_eq!(scan.scanned, n + n / 100);
    assert_eq!(scan.groups.len(), n / 100);
    let t = Instant::now();
    let possible = find_duplicates(&engine, DupeTier::Possible).unwrap();
    let possible_time = t.elapsed();

    let ids: Vec<RecordId> = rows(&engine).iter().map(|r| r.id.unwrap()).collect();
    let t = Instant::now();
    let (made, dur) = bulk_edit(
        &engine,
        &ids,
        &[Change {
            field: Field::MyRig,
            value: Some("IC-7300".into()),
        }],
    );
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let edit_time = t.elapsed();

    let asks: Vec<MergeAsk> = scan.groups.iter().map(|g| ask(g, &[])).collect();
    let fresh = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let asks_now: Vec<MergeAsk> = fresh.groups.iter().map(|g| ask(g, &[])).collect();
    assert_eq!(asks.len(), asks_now.len());
    let t = Instant::now();
    let (merged, dur) = merge_duplicates(&engine, &asks_now);
    let merged = merged.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let merge_time = t.elapsed();

    println!(
        "150k bench: {} contacts; rule scan {:?} ({} groups); possible scan {:?} ({} groups); \
         bulk edit of all {:?}, longest lock hold {:?}; merge of {} groups {:?}, longest lock \
         hold {:?}",
        scan.scanned,
        scan_time,
        scan.groups.len(),
        possible_time,
        possible.groups.len(),
        edit_time,
        made.longest_hold,
        asks_now.len(),
        merge_time,
        merged.longest_hold,
    );
    assert_eq!(made.changed, ids.len());
    // Each pair's copies differ only in their time, so the kept one already holds the union:
    // the merge deletes the other and changes nothing else.
    assert_eq!(merged.changed, asks_now.len());
    assert!(scan_time < Duration::from_secs(10), "{scan_time:?}");
    assert!(
        made.longest_hold < Duration::from_millis(50),
        "{:?}",
        made.longest_hold
    );
}
