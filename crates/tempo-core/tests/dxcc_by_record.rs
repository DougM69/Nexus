//! A contact's DXCC number and CQ zone survive the merges that bring them: the LoTW download
//! (LoTW's QSL detail carries both) and an ADIF import restating a contact the log already holds
//! (another logger's export carries both). A NEW row always kept them; a row that only upgrades
//! a held contact goes through `reconcile::apply_match`, which used to fill STATE and COUNTRY
//! when missing and nothing else. Where LoTW names another entity than the log holds, the log's
//! stays as entered and LoTW's is kept beside it — the awards count by it.

use tempo_core::logbook::{
    adif_record, record_cq_zone, record_lotw_dxcc, Logbook, QsoRecord, LOTW_DXCC_FIELD,
};

/// The contact as Nexus logged it: COUNTRY from cty.dat at log time, no DXCC number, no CQZ.
const HELD: &str = "<EOH>\n<CALL:3>K1N<COUNTRY:13>United States<BAND:3>20m<MODE:2>CW\
<QSO_DATE:8>20150210<TIME_ON:6>123400<EOR>\n";

/// The same contact as a logger that wrote the call's US number holds it.
const HELD_291: &str = "<EOH>\n<CALL:3>K1N<COUNTRY:13>United States<DXCC:3>291<CQZ:1>5\
<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20150210<TIME_ON:6>123400<EOR>\n";

/// LoTW's QSL detail for it (`qso_qsldetail=yes`), shaped like the existing LoTW fixtures.
fn lotw(dxcc: &str, cqz: &str) -> String {
    format!(
        "ARRL Logbook of the World Status Report\nfor n0call\n\
<PROGRAMID:4>LoTW\n<APP_LoTW_LASTQSL:19>2026-10-01 00:00:00\n<eoh>\n\
<STATION_CALLSIGN:6>N0CALL<CALL:3>K1N<BAND:3>20M<MODE:2>CW<QSO_DATE:8>20150210<TIME_ON:6>123400\
<QSL_RCVD:1>Y<QSLRDATE:8>20150301<CREDIT_GRANTED:9>DXCC:LOTW<DXCC:{}>{dxcc}\
<COUNTRY:14>NAVASSA ISLAND<CQZ:{}>{cqz}<ITUZ:2>11<eor>\n<APP_LoTW_EOF>\n",
        dxcc.len(),
        cqz.len()
    )
}

/// The same contact as a LOG4OM export carries it.
const LOG4OM: &str = "LOG4OM export\n<EOH>\n<CALL:3>K1N<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20150210\
<TIME_ON:6>123400<DXCC:3>182<COUNTRY:14>Navassa Island<CQZ:1>8<LOTW_QSL_RCVD:1>Y<EOR>\n";

fn cqz(r: &QsoRecord) -> Option<String> {
    r.extra
        .iter()
        .find(|(k, _)| k == "CQZ")
        .map(|(_, v)| v.clone())
}

fn held(text: &str) -> Logbook {
    let mut lb = Logbook::new();
    lb.import_adif(text);
    assert_eq!(lb.records().len(), 1, "premise: one held contact");
    lb
}

#[test]
fn control_a_new_row_keeps_its_dxcc_and_cqz() {
    let mut lb = Logbook::new();
    let (added, _, _) = lb.import_adif(LOG4OM);
    assert_eq!(added.len(), 1);
    let r = &lb.records()[0];
    println!(
        "new row: dxcc={:?} cqz={:?} award_confirmed={}",
        r.dxcc,
        cqz(r),
        r.award_confirmed
    );
    assert_eq!(r.dxcc, Some(182));
    assert_eq!(cqz(r).as_deref(), Some("8"));
}

#[test]
fn a_lotw_download_keeps_the_dxcc_it_carries_for_a_held_contact() {
    let mut lb = held(HELD);
    let s = lb.merge_lotw_report(&lotw("182", "8"), Some("N0CALL"));
    let r = &lb.records()[0];
    println!(
        "LoTW merge: matched={} newly_confirmed={} dxcc={:?} cqz={:?} country={:?}",
        s.matched,
        s.newly_confirmed,
        r.dxcc,
        cqz(r),
        r.country
    );
    assert_eq!(
        s.newly_confirmed, 1,
        "control: the row matched and confirmed the contact"
    );
    assert_eq!(r.dxcc, Some(182), "LoTW's DXCC on the confirmed contact");
    assert_eq!(
        record_lotw_dxcc(r),
        None,
        "nothing to keep beside a number the log lacked"
    );
    assert_eq!(
        r.country.as_deref(),
        Some("United States"),
        "the logged COUNTRY stays as entered"
    );
}

#[test]
fn a_lotw_download_keeps_the_cqz_it_carries_for_a_held_contact() {
    let mut lb = held(HELD);
    let s = lb.merge_lotw_report(&lotw("182", "8"), Some("N0CALL"));
    assert_eq!(s.newly_confirmed, 1, "control");
    assert_eq!(cqz(&lb.records()[0]).as_deref(), Some("8"), "LoTW's CQZ");
    assert_eq!(record_cq_zone(&lb.records()[0]), Some(8));
}

#[test]
fn an_adif_merge_keeps_the_dxcc_and_cqz_it_carries_for_a_held_contact() {
    let mut lb = held(HELD);
    let (added, skipped, merged) = lb.import_adif(LOG4OM);
    let r = &lb.records()[0];
    println!(
        "LOG4OM merge: added={} skipped={skipped} merged={merged} dxcc={:?} cqz={:?} award_confirmed={}",
        added.len(),
        r.dxcc,
        cqz(r),
        r.award_confirmed
    );
    assert_eq!(
        (added.len(), merged),
        (0, 1),
        "control: the held contact was upgraded"
    );
    assert_eq!(r.dxcc, Some(182), "LOG4OM's DXCC on the held contact");
    assert_eq!(
        cqz(r).as_deref(),
        Some("8"),
        "LOG4OM's CQZ on the held contact"
    );
}

/// Where the log holds another number, LoTW's is kept beside it and the log's stays as entered;
/// a later LoTW row that agrees with the log clears it. The log's CQZ is never replaced either.
#[test]
fn lotws_dxcc_is_kept_beside_a_logged_one_it_disagrees_with() {
    let mut lb = held(HELD_291);
    let s = lb.merge_lotw_report(&lotw("182", "8"), Some("N0CALL"));
    assert_eq!(s.newly_confirmed, 1, "control");
    let r = &lb.records()[0];
    assert_eq!(r.dxcc, Some(291), "the logged value stays as entered");
    assert_eq!(record_lotw_dxcc(r), Some(182), "LoTW's beside it");
    assert_eq!(cqz(r).as_deref(), Some("5"), "the logged CQZ stays too");

    // It rides the log's own ADIF, so it survives a restart and an export.
    let written = adif_record(r);
    assert!(
        written.contains(&format!("<{LOTW_DXCC_FIELD}:3>182")),
        "{written}"
    );
    let mut again = Logbook::new();
    again.import_adif(&format!("<EOH>\n{written}"));
    assert_eq!(record_lotw_dxcc(&again.records()[0]), Some(182));

    // LoTW, asked again after the operator corrected the log to 182 — or LoTW agreeing with 291
    // — keeps nothing beside it.
    let s = lb.merge_lotw_report(&lotw("291", "5"), Some("N0CALL"));
    assert_eq!(s.matched, 1, "control: the row matched again");
    assert_eq!(record_lotw_dxcc(&lb.records()[0]), None);
    assert_eq!(lb.records()[0].dxcc, Some(291));
}

/// Only LoTW's number is kept beside the log's: another logger's export restating the contact is
/// no authority over the log's own, so its different number changes nothing.
#[test]
fn an_adif_merge_never_overrides_a_logged_dxcc() {
    let mut lb = held(HELD_291);
    lb.import_adif(LOG4OM);
    let r = &lb.records()[0];
    assert!(r.award_confirmed, "control: the merge upgraded the contact");
    assert_eq!(r.dxcc, Some(291));
    assert_eq!(record_lotw_dxcc(r), None);
    assert_eq!(cqz(r).as_deref(), Some("5"));
}
