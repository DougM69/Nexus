//! The history's proofs at the store: what each kind of change records, that a change and its
//! history land together or not at all, what an undo makes of a contact, and that a store from
//! before the history opens, gains it, and stays readable to the build that made it.

use super::*;
use crate::logbook::sqlite::{Batch, Expect, LogDb};
use crate::logbook::{adif_record_own_log, parse_adif, UploadOutcome, UploadState, UploadStatus};
use std::sync::Arc;

/// A plain contact with id `n`.
fn rec(call: &str, n: u64) -> QsoRecord {
    let mut r = parse_adif(&format!(
        "<CALL:{}>{call}<QSO_DATE:8>20260701<TIME_ON:6>010203<BAND:3>20m<MODE:3>FT8\
         <GRIDSQUARE:4>EN52<NAME:3>Bob<APP_OTHERLOG_X:5>kept!<EOR>",
        call.len()
    ))
    .remove(0);
    r.id = Some(RecordId::Provisional {
        hash: n,
        ordinal: 0,
    });
    r
}

fn note(run: &str, source: Source, edit: Kind) -> Note {
    Note {
        run: run.into(),
        at_ms: 1_700_000_000_000,
        source,
        edit,
        undoes: HashMap::new(),
    }
}

fn write(db: &mut LogDb, rows: &[QsoRecord], note: Option<&Note>) -> Result<()> {
    let upsert: Vec<RowWrite> = rows
        .iter()
        .map(|r| RowWrite::new(Arc::new(r.clone())))
        .collect();
    db.apply(Batch {
        upsert: &upsert,
        history: note,
        ..Batch::default()
    })
}

fn remove(db: &mut LogDb, ids: &[RecordId], note: Option<&Note>) -> Result<()> {
    db.apply(Batch {
        remove: ids,
        history: note,
        ..Batch::default()
    })
}

fn id(n: u64) -> RecordId {
    RecordId::Provisional {
        hash: n,
        ordinal: 0,
    }
}

fn count(db: &LogDb, table: &str) -> i64 {
    db.conn_for_tests()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// ★ Each kind of change is recorded, under the source and run its note names: a new contact,
/// an edit (its fields, before and after, and nothing else), a merge and a fill (their own kinds),
/// and a delete (the contact whole).
#[test]
fn each_kind_of_change_is_recorded_with_its_source() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("r1", Source::Operator, Kind::Edit)),
    )
    .unwrap();

    let mut edited = a.clone();
    edited.name = Some("Robert".into());
    edited.comment = Some("new".into());
    write(
        &mut db,
        &[edited.clone()],
        Some(&note("r2", Source::Operator, Kind::Edit)),
    )
    .unwrap();

    let mut merged = edited.clone();
    merged.qsl_rcvd.lotw = true;
    write(
        &mut db,
        &[merged.clone()],
        Some(&note("r3", Source::Lotw, Kind::Merge)),
    )
    .unwrap();

    let mut filled = merged.clone();
    filled.state = Some("WI".into());
    write(
        &mut db,
        &[filled.clone()],
        Some(&note("r4", Source::Fill, Kind::Fill)),
    )
    .unwrap();

    remove(
        &mut db,
        &[id(1)],
        Some(&note("r5", Source::Import, Kind::Edit)),
    )
    .unwrap();

    let h = db.history_of(id(1)).unwrap();
    let seen: Vec<(Kind, &str, &str)> = h
        .iter()
        .map(|e| (e.kind, e.source.as_str(), e.run.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (Kind::Create, "operator", "r1"),
            (Kind::Edit, "operator", "r2"),
            (Kind::Merge, "lotw", "r3"),
            (Kind::Fill, "fill", "r4"),
            (Kind::Delete, "import", "r5"),
        ]
    );
    assert_eq!(h[0].before, None, "a new contact stores the event alone");
    assert_eq!(
        h[1].changes(),
        vec![
            FieldChange {
                tag: "COMMENT".into(),
                old: None,
                new: Some("new".into()),
            },
            FieldChange {
                tag: "NAME".into(),
                old: Some("Bob".into()),
                new: Some("Robert".into()),
            },
        ],
        "an edit stores the fields it changed, and only those"
    );
    assert_eq!(
        h[3].changes(),
        vec![FieldChange {
            tag: "STATE".into(),
            old: None,
            new: Some("WI".into()),
        }]
    );
    let gone = h[4].deleted_contact().expect("a delete keeps the contact");
    assert_eq!(
        adif_record_own_log(&gone),
        adif_record_own_log(&filled),
        "the contact as it was, every field — a foreign APP_ tag included"
    );
    assert_eq!(h[0].at_ms, 1_700_000_000_000);
}

/// A change that moves only an upload stamp is not history: nothing is written for it, not even
/// its run. And a change with no note — the conversion of `log.adi` — writes none.
#[test]
fn a_stamp_and_a_change_without_a_note_write_no_history() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(&mut db, std::slice::from_ref(&a), None).unwrap();
    assert_eq!(count(&db, "qso_history"), 0, "no note, no history");
    let mut stamped = a.clone();
    stamped.upload.lotw = Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 5,
        detail: None,
    });
    write(
        &mut db,
        &[stamped],
        Some(&note("s", Source::Lotw, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(count(&db, "qso_history"), 0, "a stamp is not history");
    assert_eq!(count(&db, "history_run"), 0, "nor is its run");
}

/// ★ A change and its history land together, or neither does: a batch the store refuses part-way
/// (a row with no id, after a good one) leaves no row and no history; one refused by its check
/// (another window changed a row) leaves none either.
#[test]
fn a_refused_batch_leaves_neither_rows_nor_history() {
    let mut db = LogDb::open_in_memory().unwrap();
    let good = rec("K1ABC", 1);
    let mut bad = rec("K2XYZ", 2);
    bad.id = None;
    assert!(write(
        &mut db,
        &[good.clone(), bad],
        Some(&note("r", Source::Import, Kind::Merge))
    )
    .is_err());
    assert_eq!(db.row_count().unwrap(), 0);
    assert_eq!(count(&db, "qso_history"), 0);
    assert_eq!(count(&db, "history_run"), 0);

    write(
        &mut db,
        std::slice::from_ref(&good),
        Some(&note("a", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let mut theirs = good.clone();
    theirs.comment = Some("theirs".into());
    let mut ours = good.clone();
    ours.comment = Some("ours".into());
    write(&mut db, &[theirs], None).unwrap();
    let expect = HashMap::from([(id(1), Expect::Row(Arc::new(good.clone())))]);
    let upsert = [RowWrite::new(Arc::new(ours))];
    let refused = db.apply(Batch {
        upsert: &upsert,
        expect: Some(&expect),
        history: Some(&note("b", Source::Operator, Kind::Edit)),
        ..Batch::default()
    });
    assert!(refused.is_err(), "the check refuses it");
    assert_eq!(
        db.history_of(id(1)).unwrap().len(),
        1,
        "only the create: the refused edit wrote no history"
    );
}

/// A purge that names no row records a delete of every row it took out.
#[test]
fn a_purge_records_every_contact_it_took_out() {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    db.apply(Batch {
        clear: true,
        history: Some(&note("p", Source::Operator, Kind::Edit)),
        ..Batch::default()
    })
    .unwrap();
    let run = db.history_run("p").unwrap();
    assert_eq!(run.len(), 2);
    assert!(run
        .iter()
        .all(|e| e.kind == Kind::Delete && e.before.is_some()));
    assert_eq!(db.history_deleted(10).unwrap().len(), 2);
}

/// "Recently deleted" lists a contact once, by its latest delete, and stops listing it once it
/// is back in the log.
#[test]
fn recently_deleted_lists_what_is_gone_and_only_that() {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d1", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    write(
        &mut db,
        &[rec("K1ABC", 1)],
        Some(&note("back", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d2", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(2)],
        Some(&note("d3", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let gone: Vec<(String, RecordId)> = db
        .history_deleted(10)
        .unwrap()
        .into_iter()
        .map(|e| (e.run, e.qso_id))
        .collect();
    assert_eq!(gone, vec![("d3".into(), id(2)), ("d2".into(), id(1))]);
    write(
        &mut db,
        &[rec("K2XYZ", 2)],
        Some(&note("r", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(db.history_deleted(10).unwrap().len(), 1);
    let latest = db.history_latest_run(Some("operator")).unwrap().unwrap();
    assert_eq!((latest.run.as_str(), latest.contacts), ("d3", 1));
}

// ── undo, as a function of the entry and the contact as it now stands ─────────────────────────

fn entries_after(changes: &[(&QsoRecord, &QsoRecord)]) -> (LogDb, Vec<Entry>) {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[changes[0].0.clone()], None).unwrap();
    for (i, (_, after)) in changes.iter().enumerate() {
        write(
            &mut db,
            &[(*after).clone()],
            Some(&note(&format!("e{i}"), Source::Operator, Kind::Edit)),
        )
        .unwrap();
    }
    let h = db.history_of(changes[0].0.id.unwrap()).unwrap();
    (db, h)
}

/// An edit's undo puts back each field it changed — and keeps a field changed again since.
#[test]
fn an_undo_puts_back_what_the_edit_changed_and_keeps_what_changed_since() {
    let a = rec("K1ABC", 1);
    let mut b = a.clone();
    b.name = Some("Robert".into());
    b.grid = Some("FN31".into());
    let mut c = b.clone();
    c.grid = Some("FN42".into());
    let (_, h) = entries_after(&[(&a, &b), (&b, &c)]);
    match undo(&h[0], Some(&c)).unwrap() {
        Undo::Revert { row, kept } => {
            assert_eq!(row.name.as_deref(), Some("Bob"), "the name goes back");
            assert_eq!(row.grid.as_deref(), Some("FN42"), "the later grid stays");
            assert_eq!(kept, vec!["GRIDSQUARE".to_string()]);
            assert_eq!(row.id, a.id);
            assert_eq!(row.extra, c.extra, "a foreign field survives the undo");
        }
        other => panic!("{other:?}"),
    }
    // Every field it changed has moved since: nothing to undo.
    let mut d = c.clone();
    d.grid = Some("FN53".into());
    assert_eq!(undo(&h[1], Some(&d)).err(), Some(CannotUndo::ChangedSince));
}

/// An undo keeps the stamps the contact has — unless it corrects the call, which owes the
/// contact to every service again, as an edit of the call does. It never sends anything.
#[test]
fn an_undo_keeps_the_stamps_unless_it_changes_the_call() {
    let stamp = UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 9,
        detail: None,
    };
    let a = rec("K1ABC", 1);
    let mut b = a.clone();
    b.name = Some("Robert".into());
    let mut b_now = b.clone();
    b_now.upload.qrz = Some(stamp.clone());
    let (_, h) = entries_after(&[(&a, &b)]);
    let Undo::Revert { row, .. } = undo(&h[0], Some(&b_now)).unwrap() else {
        panic!()
    };
    assert_eq!(
        row.upload.qrz,
        Some(stamp.clone()),
        "a stamp is never undone"
    );

    let mut corrected = a.clone();
    corrected.call = "K1ABD".into();
    let mut corrected_now = corrected.clone();
    corrected_now.upload.qrz = Some(stamp.clone());
    let (_, h) = entries_after(&[(&a, &corrected)]);
    let Undo::Revert { row, .. } = undo(&h[0], Some(&corrected_now)).unwrap() else {
        panic!()
    };
    assert_eq!(row.call, "K1ABC");
    assert_eq!(
        row.upload,
        UploadState::default(),
        "a corrected call is owed again"
    );
}

/// A new contact's undo takes it out; a delete's puts it back whole; neither is made on a log
/// where it would mean nothing.
#[test]
fn a_create_undoes_to_a_removal_and_a_delete_to_a_restore() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("c", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let h = db.history_of(id(1)).unwrap();
    assert_eq!(undo(&h[0], Some(&a)), Ok(Undo::Remove(id(1))));
    assert_eq!(undo(&h[0], None), Err(CannotUndo::Gone));
    match undo(&h[1], None).unwrap() {
        Undo::Restore(back) => assert_eq!(adif_record_own_log(&back), adif_record_own_log(&a)),
        other => panic!("{other:?}"),
    }
    assert_eq!(undo(&h[1], Some(&a)), Err(CannotUndo::AlreadyThere));
}

// ── the schema: a store from before the history ─────────────────────────────────────────────

/// ★ A store written before the history existed opens, gains its two tables, keeps every
/// contact and its version; and the history it gains never stops the build that wrote it from
/// reading it — the version is unchanged, the old tables are as they were.
#[test]
fn a_store_from_before_the_history_gains_it_and_stays_readable() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-history-schema-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.db");
    {
        let mut db = LogDb::open(&path).unwrap();
        write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    }
    // Today's store: exactly this one without the history's objects.
    let old_schema = {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE qso_history; DROP TABLE history_run;")
            .unwrap();
        schema(&conn)
    };
    assert!(old_schema.iter().all(|(_, name, _)| !ours(name)));

    let mut db = LogDb::open(&path).expect("today's store opens");
    assert_eq!(
        db.meta("schema_version").unwrap(),
        Some(1),
        "the version does not move"
    );
    assert_eq!(db.load_all().unwrap().len(), 2, "every contact is there");
    remove(
        &mut db,
        &[id(2)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    drop(db);
    let conn = Connection::open(&path).unwrap();
    let now: Vec<_> = schema(&conn)
        .into_iter()
        .filter(|(_, name, _)| !ours(name))
        .collect();
    assert_eq!(
        now, old_schema,
        "the log's own tables and indexes are as they were"
    );
    // An older build's view: its version check passes and its tables read as before.
    let version: i64 = conn
        .query_row(
            "SELECT v FROM log_meta WHERE k = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, crate::logbook::sqlite::SCHEMA_VERSION);
    // A contact an older build deletes leaves the history alone, and the restore still reads it.
    conn.execute("DELETE FROM qso WHERE id = ?1", [id(1).to_string()])
        .unwrap();
    drop(conn);
    let db = LogDb::open(&path).unwrap();
    let h = db.history_of(id(2)).unwrap();
    assert!(
        matches!(undo(&h[0], None), Ok(Undo::Restore(_))),
        "restorable after a restart"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The history's own tables and indexes.
fn ours(name: &str) -> bool {
    name.contains("history_run") || name.starts_with("qso_history")
}

fn schema(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

// ── the cost, measured ──────────────────────────────────────────────────────────────────────

/// What the history costs a big log: the same writes — a log of `HISTORY_BENCH_ROWS` contacts
/// (150,000 by default) put in 256-row chunks as the writer does, then 1,000 one-contact edits and
/// 1,000 one-contact deletes — made with no note and with one, timed, and the store's size on disk
/// after each; then the reads the UI makes. A measurement, not a check: run it in release with
/// `cargo test -p tempo-core --release --lib the_history_at_scale -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn the_history_at_scale() {
    use std::time::Instant;
    let n: u64 = std::env::var("HISTORY_BENCH_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150_000);
    let base = rec("K1ABC", 0);
    let row = |i: u64| {
        let mut r = base.clone();
        r.call = format!("K{}AB{}", i % 10, i);
        r.when_unix += i;
        r.id = Some(id(i + 1));
        r
    };
    for noted in [false, true] {
        let dir = std::env::temp_dir().join(format!(
            "nexus-history-scale-{}-{noted}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.db");
        let mut db = LogDb::open(&path).unwrap();
        let mut k = 0u64;
        let mut note_for = |src: Source| {
            k += 1;
            Note {
                run: format!("run-{k}"),
                at_ms: 1_700_000_000_000,
                source: src,
                edit: Kind::Edit,
                undoes: HashMap::new(),
            }
        };
        let import = note_for(Source::Import);
        let started = Instant::now();
        let mut i = 0;
        while i < n {
            let rows: Vec<QsoRecord> = (i..(i + 256).min(n)).map(row).collect();
            write(&mut db, &rows, noted.then_some(&import)).unwrap();
            i += 256;
        }
        let load = started.elapsed();
        let started = Instant::now();
        for j in 0..1_000u64 {
            let mut r = row(j * 97 % n);
            r.comment = Some(format!("edit {j}"));
            let note = note_for(Source::Operator);
            write(&mut db, &[r], noted.then_some(&note)).unwrap();
        }
        let edits = started.elapsed();
        let started = Instant::now();
        for j in 0..1_000u64 {
            let note = note_for(Source::Operator);
            remove(&mut db, &[id(n - j)], noted.then_some(&note)).unwrap();
        }
        let deletes = started.elapsed();
        db.conn_for_tests()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let bytes = std::fs::metadata(&path).unwrap().len();
        println!(
            "HISTORY {} rows, history {}: load {:.2} s ({:.1} µs/row); 1,000 edits {:.1} ms \
             ({:.0} µs each); 1,000 deletes {:.1} ms ({:.0} µs each); store {:.1} MiB",
            n,
            if noted { "ON " } else { "OFF" },
            load.as_secs_f64(),
            load.as_secs_f64() * 1e6 / n as f64,
            edits.as_secs_f64() * 1e3,
            edits.as_secs_f64() * 1e3,
            deletes.as_secs_f64() * 1e3,
            deletes.as_secs_f64() * 1e3,
            bytes as f64 / 1_048_576.0
        );
        if noted {
            let count = |t: &str| count(&db, t);
            println!(
                "HISTORY entries {} in {} runs",
                count("qso_history"),
                count("history_run")
            );
            let started = Instant::now();
            let h = db.history_of(id(97)).unwrap();
            let one = started.elapsed();
            let started = Instant::now();
            let gone = db.history_deleted(200).unwrap();
            let deleted = started.elapsed();
            let started = Instant::now();
            let latest = db.history_latest_run(Some("import")).unwrap();
            let latest_t = started.elapsed();
            println!(
                "HISTORY reads: one contact's ({} entries) {:.3} ms; recently deleted ({}) {:.3} \
                 ms; latest import run ({} contacts) {:.3} ms",
                h.len(),
                one.as_secs_f64() * 1e3,
                gone.len(),
                deleted.as_secs_f64() * 1e3,
                latest.map_or(0, |r| r.contacts),
                latest_t.as_secs_f64() * 1e3
            );
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
