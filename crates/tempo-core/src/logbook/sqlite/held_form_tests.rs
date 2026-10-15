//! A store written before contacts were held in one form reads back in that form, through every
//! decoder, and is not rewritten by being read.

use super::*;
use crate::logbook::query::{reference, LabelCount, LogStatCounter};
use crate::logbook::{adif_record, parse_adif};

/// The three contacts of the operator's report: one Nexus logged, one imported from another
/// logger, and a phone contact Nexus logged with its sideband.
fn contacts() -> Vec<QsoRecord> {
    let tag = |name: &str, value: &str| format!("<{name}:{}>{value}", value.len());
    let row = |call: &str, band: &str, mode: &str, time: &str, foreign: &str| {
        [
            tag("CALL", call),
            tag("QSO_DATE", "20261001"),
            tag("TIME_ON", time),
            tag("BAND", band),
            tag("MODE", mode),
        ]
        .concat()
            + foreign
            + "<EOR>\n"
    };
    // The phone contact carries two fields Nexus does not model, one either side of SUBMODE in
    // name order, as an N1MM log's do.
    let foreign = tag("APP_N1MM_EXCHANGE1", "WI") + &tag("VE_PROV", "ON");
    let mut rows = parse_adif(
        &(row("K2DEF", "20m", "FT8", "010000", "")
            + &row("K1ABC", "20m", "FT8", "020000", "")
            + &row("W9XYZ", "40m", "SSB", "030000", &foreign)
            + &row("W1AW", "", "FT8", "040000", "")),
    );
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = Some(RecordId::Provisional {
            hash: i as u64,
            ordinal: 0,
        });
    }
    rows
}

/// The store as a build before this one left it: the imported contact's band in its source's
/// capitals, and the phone contact's sideband in MODE, as the record held it.
fn store_from_before() -> LogDb {
    let mut db = LogDb::open_in_memory().expect("open");
    let rows = contacts();
    db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
        .expect("insert");
    db.conn
        .execute("UPDATE qso SET band = '20M' WHERE call = 'K1ABC'", [])
        .expect("an imported band, as stored before");
    db.conn
        .execute("UPDATE qso SET mode = 'LSB' WHERE call = 'W9XYZ'", [])
        .expect("a logged sideband, as stored before");
    db
}

fn stored(db: &LogDb, call: &str) -> (String, String) {
    db.conn
        .query_row("SELECT band, mode FROM qso WHERE call = ?1", [call], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("the row")
}

fn of<'a>(rows: &'a [QsoRecord], call: &str) -> &'a QsoRecord {
    rows.iter().find(|r| r.call == call).expect("the contact")
}

fn submode(r: &QsoRecord) -> Option<&str> {
    r.extra
        .iter()
        .find(|(k, _)| k == "SUBMODE")
        .map(|(_, v)| v.as_str())
}

/// ★ The whole read: each contact in the form it is held in, the sideband kept as its SUBMODE,
/// and written that way; a contact with no band keeps none.
#[test]
fn a_store_from_before_loads_in_one_form() {
    let db = store_from_before();
    assert_eq!(
        stored(&db, "K1ABC"),
        ("20M".into(), "FT8".into()),
        "premise"
    );
    assert_eq!(
        stored(&db, "W9XYZ"),
        ("40m".into(), "LSB".into()),
        "premise"
    );
    let all = db.load_all().expect("load");
    assert_eq!(of(&all, "K1ABC").band, "20m");
    let phone = of(&all, "W9XYZ");
    assert_eq!((phone.mode.as_str(), submode(phone)), ("SSB", Some("LSB")));
    assert!(
        adif_record(phone).contains("<MODE:3>SSB<SUBMODE:3>LSB"),
        "{}",
        adif_record(phone)
    );
    assert_eq!(
        of(&all, "W1AW").band,
        "",
        "a contact with no band keeps none"
    );
    // The same rows by id, and by rowid, read the same.
    let ids: Vec<RecordId> = all.iter().filter_map(|r| r.id).collect();
    assert_eq!(db.rows_by_ids(&ids).expect("by id"), all);
    let at: Vec<QsoRecord> = db
        .rows_at(&[1, 2, 3, 4])
        .expect("by rowid")
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(at, all);
}

/// The narrow reads the folds, the hot index and the Logbook's order are made from hold the same
/// form: the statistics count one 20 m and one SSB.
#[test]
fn every_narrow_read_of_a_store_from_before_holds_the_same_form() {
    let db = store_from_before();
    let narrow = Narrow {
        columns: &["call", "band", "mode", "when_unix"],
        uploads: false,
    };
    let mut seen = Vec::new();
    db.each_narrow(narrow, Scope::All, Order::Log, &mut |r| {
        seen.push(r.clone());
        ControlFlow::Continue(())
    })
    .expect("narrow");
    let counts = reference::fold(&seen, LogStatCounter::new(&|_: &QsoRecord| None));
    let label = |label: &str, count| LabelCount {
        label: label.into(),
        count,
    };
    assert_eq!(counts.by_band, vec![label("20m", 2), label("40m", 1)]);
    assert_eq!(counts.by_mode, vec![label("FT8", 3), label("SSB", 1)]);
    let mut after = Vec::new();
    db.each_narrow_after(&["call", "band", "mode"], 0, &mut |_, r| {
        after.push((r.call.clone(), r.band.clone(), r.mode.clone()));
    })
    .expect("narrow after");
    assert_eq!(
        after,
        [
            ("K2DEF".to_string(), "20m".to_string(), "FT8".to_string()),
            ("K1ABC".into(), "20m".into(), "FT8".into()),
            ("W9XYZ".into(), "40m".into(), "SSB".into()),
            ("W1AW".into(), "".into(), "FT8".into()),
        ]
    );
}

/// ⛔ Reading is not a migration: the rows on disk keep the spelling they were written in until a
/// change writes them, and an older build reads them exactly as it always did.
#[test]
fn reading_a_store_from_before_writes_nothing() {
    let db = store_from_before();
    let _ = db.load_all().expect("load");
    db.each_narrow(
        Narrow {
            columns: &["band", "mode"],
            uploads: false,
        },
        Scope::All,
        Order::Log,
        &mut |_| ControlFlow::Continue(()),
    )
    .expect("narrow");
    assert_eq!(stored(&db, "K1ABC"), ("20M".into(), "FT8".into()));
    assert_eq!(stored(&db, "W9XYZ"), ("40m".into(), "LSB".into()));
}

/// A change to a row from before — an upload stamp, an edit — is made on the row as it reads, the
/// store takes it as the row the change read (not as another window's), and writes it in the one
/// form; read again, it is the row the change wrote, its foreign fields and its sideband in place.
#[test]
fn a_change_to_a_row_from_before_writes_it_in_one_form() {
    let mut db = store_from_before();
    let read = db.load_all().expect("load");
    let mut changed = Vec::new();
    let mut expect = HashMap::new();
    for call in ["K1ABC", "W9XYZ"] {
        let was = of(&read, call).clone();
        let mut now = was.clone();
        now.name = Some("Frank".into());
        expect.insert(was.id.expect("an id"), Expect::Row(Arc::new(was)));
        changed.push(now);
    }
    let rows: Vec<RowWrite> = changed
        .iter()
        .map(|r| RowWrite::new(Arc::new(r.clone())))
        .collect();
    db.apply(Batch {
        upsert: &rows,
        expect: Some(&expect),
        ..Batch::default()
    })
    .expect("the store takes the change as made on the row it holds");
    assert_eq!(stored(&db, "K1ABC"), ("20m".into(), "FT8".into()));
    assert_eq!(stored(&db, "W9XYZ"), ("40m".into(), "SSB".into()));
    let again = db.load_all().expect("load");
    for now in &changed {
        assert_eq!(
            of(&again, &now.call),
            now,
            "{} as the change wrote it",
            now.call
        );
    }
    let phone = of(&again, "W9XYZ");
    assert_eq!(
        phone.extra,
        [
            ("APP_N1MM_EXCHANGE1".to_string(), "WI".to_string()),
            ("SUBMODE".into(), "LSB".into()),
            ("VE_PROV".into(), "ON".into()),
        ]
    );
}
