//! ⭐ **The entry strip's optional "RST sent" box** (N1MM's Snt) — an operator's own sent report
//! for ONE contact, in place of the constant the session sends.
//!
//! Without the box nothing changes: a CW row carries the session's `599`, a phone row `59`
//! (`ContestSession::fit_sent_rst`). With a typed report the row, the Cabrillo line and the ADIF
//! record carry what was typed, and the next contact goes back to the constant. Illinois QSO Party
//! here: its exchange sends a constant RST and a county.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

const T0: u64 = 1_792_342_800;

fn ilqp_log() -> FieldDayLog {
    let station = StationData {
        contest_qth_state: "IL".to_string(),
        contest_qth_county: "ADAM".to_string(),
        ..Default::default()
    };
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("ilqp is a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, &station).expect("ilqp selects");
    FieldDayLog::new("W9XYZ", session, "40m")
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The sent RST on the newest row for `call`.
fn sent_rst(log: &FieldDayLog, call: &str) -> String {
    let q = log
        .qsos()
        .iter()
        .rev()
        .find(|q| q.call == call)
        .expect("the row is in the log");
    q.tx.iter()
        .find(|v| v.key == "RST")
        .map(|v| v.raw.clone())
        .expect("the row carries a sent RST")
}

#[test]
fn the_session_offers_its_own_constant_and_none_where_it_sends_no_report() {
    let log = ilqp_log();
    assert_eq!(log.session.sent_rst_constant(), Some("599"));
    // CONTROL: Field Day sends no RST, so there is nothing to override and no box.
    let fd = ContestSession::field_day(tempo_core::fieldday::FdEvent::ArrlFd, "3A", "IL");
    assert_eq!(fd.sent_rst_constant(), None);
}

#[test]
fn a_typed_report_is_logged_exported_and_the_next_contact_goes_back_to_the_constant() {
    let mut log = ilqp_log();
    let ex = fields(&[("RST", "599"), ("QTH", "KANK")]);
    assert!(log.log_entry_at("K9NR", &ex, "CW", "", T0, None, Some("579")));
    assert_eq!(
        sent_rst(&log, "K9NR"),
        "579",
        "the typed report is the row's"
    );
    // The next contact, no report typed: the constant again, not the last one typed.
    assert!(log.log_entry_at("K9ABC", &ex, "CW", "", T0 + 60, None, None));
    assert_eq!(sent_rst(&log, "K9ABC"), "599");

    let cab = log
        .cabrillo_with(7_000, &Default::default())
        .expect("one entry");
    assert!(cab.contains(" W9XYZ 579 ADAM K9NR 599 KANK"), "{cab}");
    assert!(cab.contains(" W9XYZ 599 ADAM K9ABC 599 KANK"), "{cab}");
    let adif = log.submission_adif();
    let k9nr = adif
        .split("<EOR>")
        .find(|r| r.contains("<CALL:4>K9NR"))
        .expect("K9NR is in the ADIF");
    assert!(k9nr.contains("<RST_SENT:3>579"), "{k9nr}");
}

#[test]
fn a_phone_report_overrides_the_59_and_without_one_the_row_is_unchanged() {
    let mut log = ilqp_log();
    let ex = fields(&[("RST", "59"), ("QTH", "KANK")]);
    assert!(log.log_entry_at("K9NR", &ex, "PH", "SSB", T0, None, Some("57")));
    assert_eq!(sent_rst(&log, "K9NR"), "57");
    // The entry write with no report is byte for byte the ordinary write.
    let mut a = ilqp_log();
    let mut b = ilqp_log();
    assert!(a.log_entry_at("K9AB", &ex, "PH", "SSB", T0, None, None));
    assert!(b.log_fields_at("K9AB", &ex, "PH", "SSB", 0, T0));
    assert_eq!(a.adif(), b.adif());
    assert_eq!(sent_rst(&a, "K9AB"), "59");
}

#[test]
fn the_report_replaces_only_the_constant_rst_slot() {
    let log = ilqp_log();
    let mut tx = log.session.my_exchange.clone();
    let before: Vec<(String, String)> = tx
        .iter()
        .map(|v| (v.key.to_string(), v.raw.clone()))
        .collect();
    log.session.override_sent_rst(&mut tx, "449");
    for (v, (k, raw)) in tx.iter().zip(&before) {
        if v.key == "RST" {
            assert_eq!(v.raw, "449");
        } else {
            assert_eq!(&v.raw, raw, "{k} must not move");
        }
    }
    assert!(tx.iter().any(|v| v.key == "RST"));
}

#[test]
fn a_sent_report_has_the_digit_count_of_its_mode() {
    for ok in ["599", "579", "449", "111"] {
        assert!(ContestSession::sent_rst_ok(ok, "CW"), "{ok} on CW");
        assert!(ContestSession::sent_rst_ok(ok, "DIG"), "{ok} on DIG");
    }
    for ok in ["59", "57", "11"] {
        assert!(ContestSession::sent_rst_ok(ok, "PH"), "{ok} on PH");
    }
    for bad in ["59", "5999", "5NN", "609", "509", "590", "", " 599", "59a"] {
        assert!(!ContestSession::sent_rst_ok(bad, "CW"), "{bad:?} on CW");
    }
    for bad in ["599", "5", "69", "50", "05", "5N", ""] {
        assert!(!ContestSession::sent_rst_ok(bad, "PH"), "{bad:?} on PH");
    }
}
