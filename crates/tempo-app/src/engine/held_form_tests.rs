//! Every contact the station takes in is held in one form — the band in the ADIF enumeration's
//! lower case, a phone sideband as SSB's SUBMODE — so the log's statistics and its duplicate
//! guard see one band and one mode, whichever way a contact came in.

use super::*;
use crate::test_util::StoredLog;
use tempo_core::logbook::query::{reference, LabelCount, LogStatCounter};

/// A contact as a log path hands it to the station: the band and the mode exactly as given.
fn contact(call: &str, band: &str, mode: &str, when_unix: u64) -> QsoRecord {
    let mut r = tempo_core::logbook::parse_adif(&format!(
        "<CALL:{}>{call}<QSO_DATE:8>20261001<TIME_ON:6>120000<TIME_OFF:6>120100<EOR>",
        call.len()
    ))
    .remove(0);
    r.band = band.into();
    r.mode = mode.into();
    r.freq_mhz = 14.2;
    r.when_unix = when_unix;
    r
}

fn submode(r: &QsoRecord) -> Option<&str> {
    r.extra
        .iter()
        .find(|(k, _)| k == "SUBMODE")
        .map(|(_, v)| v.as_str())
}

/// ★ The operator's report end to end: a contact imported from another logger's `20M` and one
/// Nexus logs on `20m` are one band in the statistics, both counted.
#[test]
fn an_imported_20m_and_a_logged_20m_are_one_band() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let (added, ..) = e.import_adif(
        "<CALL:6>VE3ABC<QSO_DATE:8>20261001<TIME_ON:6>141500<BAND:3>20M<MODE:3>FT8\
         <COUNTRY:13>UNITED STATES<QSL_RCVD:1>Y<EQSL_QSL_RCVD:1>Y<EOR>\n",
    );
    assert_eq!(added, 1, "premise: the import added the contact");
    e.log_qso(contact("W9XYZ", "20m", "FT8", 1_791_000_000));
    let log = e.stored_records();
    let counts = reference::fold(&log, LogStatCounter::new(&|_: &str| None));
    assert_eq!(
        counts.by_band,
        vec![LabelCount {
            label: "20m".into(),
            count: 2
        }]
    );
}

/// Whatever spelling a log path hands the station, the contact is held in the enumeration's.
#[test]
fn a_contact_logged_with_its_band_in_capitals_is_held_in_lower_case() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.log_qso(contact("W1AW", "20M", "FT8", 1_791_000_000));
    let log = e.stored_records();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].band, "20m");
}

/// A phone contact logged on its sideband is held as SSB with the sideband as its SUBMODE, and
/// the duplicate guard sees one mode: the same contact logged again within the guard's window —
/// on the same sideband, or as plain SSB from the form — is not logged twice. A contact on
/// another band is (the control).
#[test]
fn a_phone_contact_is_held_as_ssb_and_logged_once() {
    let at = 1_791_000_000;
    let held = |again: &str| -> Vec<(String, String, Option<String>)> {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.log_qso(contact("K1ABC", "20m", "USB", at));
        e.log_qso(contact("K1ABC", "20m", again, at + 60));
        e.log_qso(contact("K1ABC", "40m", "LSB", at + 120));
        e.stored_records()
            .iter()
            .map(|r| {
                (
                    r.band.clone(),
                    r.mode.clone(),
                    submode(r).map(str::to_string),
                )
            })
            .collect()
    };
    let once = vec![
        (
            "20m".to_string(),
            "SSB".to_string(),
            Some("USB".to_string()),
        ),
        ("40m".into(), "SSB".into(), Some("LSB".into())),
    ];
    assert_eq!(
        [held("USB"), held("SSB")],
        [once.clone(), once],
        "logged again on the sideband, then as plain SSB"
    );
}
