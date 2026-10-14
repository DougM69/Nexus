//! ⭐ **A CQ WW or WPX file carries the CATEGORY, NAME, EMAIL and CLUB lines its sponsor's Cabrillo
//! page asks for, written from the entry's own Settings — through the doors the operator uses.**
//!
//! The Settings save, the Contest mode switch, the contacts logged on the strip and the Export
//! Cabrillo button (`Engine::export_log`), with what the Contest screen names as missing read off the
//! same snapshot it renders (`FieldDayStatus::cabrillo_missing`). Nothing here is guessed: a
//! declaration left blank writes no line and is named instead.
//!
//! CQ prices every contact by the relation between two stations, so a session refuses to build
//! without a country file; `install_call_resolver` is process-wide, so it goes in once per test
//! binary. Every call is an example call.

use tempo_app::engine::Engine;
use tempo_app::settings::Settings;
use tempo_core::contest::{install_call_resolver, CallLocation};

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(|call| {
            let dx = call.trim().to_ascii_uppercase().starts_with("DL");
            Some(CallLocation {
                entity: if dx {
                    "Fed. Rep. of Germany"
                } else {
                    "United States"
                },
                continent: if dx { "EU" } else { "NA" },
                cq_zone: None,
            })
        })
        .expect("the first install in this test binary");
    });
}

/// An engine running `event`, its settings edited by `edit` first, with Contest mode on.
fn running(event: &str, edit: impl FnOnce(&mut Settings)) -> Engine {
    resolver();
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = event.into();
    s.contest_qth_state = "WI".into();
    s.contest_cq_zone = 4;
    edit(&mut s);
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("a CQ session builds");
    e
}

/// A Settings save, as the Save button sends the whole form.
fn save(e: &mut Engine, edit: impl FnOnce(&mut Settings)) {
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

fn log(e: &mut Engine, call: &str, fields: &[(&str, &str)], mode: &str) {
    let fields: Vec<(String, String)> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert!(e
        .contest_log_manual(call, &fields, mode, None)
        .expect("the contest session is running"));
}

fn header(cab: &str) -> Vec<String> {
    cab.lines()
        .take_while(|l| !l.starts_with("QSO:"))
        .map(String::from)
        .collect()
}

fn qso_lines(cab: &str) -> Vec<String> {
    cab.lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(String::from)
        .collect()
}

/// What the Contest screen names as missing, read off the snapshot it renders.
fn missing(e: &Engine) -> Vec<String> {
    e.snapshot()
        .field_day
        .expect("Contest mode is on")
        .cabrillo_missing
}

/// ⭐ **Every declaration made: the header is the sponsor's sample's, line for line, each value
/// from Settings, and nothing is named as missing.** CQ WW SSB, as cqww.com/cabrillo.htm heads its
/// own example: the category lines, CLAIMED-SCORE (the score the Contest screen shows), CLUB, NAME
/// and EMAIL (the operator name from the Station tab).
#[test]
fn every_declaration_made_heads_the_file_the_way_the_sponsor_shows() {
    let mut e = running("cqww_ssb", |s| {
        s.contest_category_power = "LOW".into();
        s.contest_category_assisted = "NON-ASSISTED".into();
        s.contest_category_band = "ALL".into();
        s.op_name = "A NAME".into();
        s.contest_email = "k1abc@example.com".into();
        s.contest_club = "EXAMPLE CONTEST CLUB".into();
    });
    e.set_frequency(14.250, "20m", "USB");
    log(&mut e, "K1ABC", &[("RST", "59"), ("ZN", "5")], "PH");
    log(&mut e, "DL1ABC", &[("RST", "59"), ("ZN", "14")], "PH");
    let cab = e.export_log("cabrillo").expect("one entry");
    assert_eq!(
        header(&cab),
        [
            "START-OF-LOG: 3.0",
            "CONTEST: CQ-WW-SSB",
            "CALLSIGN: W9XYZ",
            "CATEGORY-OPERATOR: SINGLE-OP",
            "CATEGORY-ASSISTED: NON-ASSISTED",
            "CATEGORY-BAND: ALL",
            "CATEGORY-MODE: SSB",
            "CATEGORY-POWER: LOW",
            "LOCATION: WI",
            "CLAIMED-SCORE: 12",
            "CLUB: EXAMPLE CONTEST CLUB",
            "CREATED-BY: Nexus",
            "EMAIL: k1abc@example.com",
            "NAME: A NAME",
            "X-NEXUS-RULES-YEAR: 2026",
        ],
        "{cab}"
    );
    // The claimed score is the one on the screen: K1ABC 0 points (same country), DL1ABC 3, times
    // zones 5 and 14 and countries US and DE.
    let fd = e.snapshot().field_day.expect("Contest mode is on");
    assert_eq!(fd.total_score, 3 * 4);
    assert_eq!(qso_lines(&cab).len(), 2, "{cab}");
    assert_eq!(missing(&e), Vec::<String>::new());
}

/// ⭐ **A declaration left blank writes no line and is named; a save fills what the export reads.**
///
/// CQ WPX CW with nothing declared: no CATEGORY-ASSISTED, -BAND or -POWER line, and those three
/// named. The band is read when the file is written, so declaring it clears it at once. Power and
/// assistance are the session's, read when Contest mode turned on, as Sweepstakes' precedence is:
/// saved mid-contest they stay named — the export would still leave them out — until Contest mode
/// is turned off and on, and the contacts come back with it.
#[test]
fn a_blank_declaration_is_named_until_the_file_would_carry_it() {
    let dir = std::env::temp_dir().join(format!("nexus-cq-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut e = running("cqwpx_cw", |_| {});
    e.set_fd_log_path(dir.join("fd_log.jsonl"));
    e.set_frequency(14.025, "20m", "CW");
    log(&mut e, "K1ABC", &[("RST", "599"), ("NR", "7")], "CW");
    log(&mut e, "DL1ABC", &[("RST", "599"), ("NR", "12")], "CW");

    let cab = e.export_log("cabrillo").expect("one entry");
    let before = qso_lines(&cab);
    let categories: Vec<String> = header(&cab)
        .into_iter()
        .filter(|l| l.starts_with("CATEGORY-"))
        .collect();
    assert_eq!(
        categories,
        ["CATEGORY-OPERATOR: SINGLE-OP", "CATEGORY-MODE: CW"],
        "nothing declared, nothing claimed: {cab}"
    );
    assert_eq!(
        missing(&e),
        ["CATEGORY-ASSISTED", "CATEGORY-BAND", "CATEGORY-POWER"]
    );

    save(&mut e, |s| s.contest_category_band = "20M".into());
    assert_eq!(missing(&e), ["CATEGORY-ASSISTED", "CATEGORY-POWER"]);
    assert!(e
        .export_log("cabrillo")
        .unwrap()
        .contains("\nCATEGORY-BAND: 20M\n"));

    save(&mut e, |s| {
        s.contest_category_power = "LOW".into();
        s.contest_category_assisted = "NON-ASSISTED".into();
    });
    assert_eq!(
        missing(&e),
        ["CATEGORY-ASSISTED", "CATEGORY-POWER"],
        "the running session still writes neither"
    );

    // Contest mode off and on again, as the Contest screen's switch saves it.
    save(&mut e, |s| s.fd_active = false);
    assert!(e.snapshot().field_day.is_none(), "Contest mode is off");
    save(&mut e, |s| s.fd_active = true);
    e.set_mode("fieldday-sp").expect("the session builds again");
    assert_eq!(missing(&e), Vec::<String>::new());
    let cab = e.export_log("cabrillo").expect("one entry");
    for line in [
        "CATEGORY-ASSISTED: NON-ASSISTED",
        "CATEGORY-BAND: 20M",
        "CATEGORY-POWER: LOW",
    ] {
        assert!(header(&cab).iter().any(|l| l == line), "{line}: {cab}");
    }
    // …each QSO line with the serial that contact was sent, never the composing "0": the header
    // lines are the only bytes the declarations move.
    let lines = qso_lines(&cab);
    assert_eq!(
        lines, before,
        "the contacts came back, every QSO line as it was: {cab}"
    );
    for (line, (serial, call, rcvd)) in lines.iter().zip([(1, "K1ABC", 7), (2, "DL1ABC", 12)]) {
        assert!(
            line.contains(&format!("W9XYZ 599 {serial} {call} 599 {rcvd}")),
            "{line}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A multi-operator entry is told about its class and its operators; a checklog about
/// nothing.** CQ WW CW: *"The category-transmitter is required for multi-operator entries"* and
/// *"If you select a multi-operator category and do not enter operator callsigns in this field,
/// the robot will reject your log"*. The class brings the OPERATORS line, with the operator at the
/// key on each contact.
#[test]
fn a_multi_operator_entry_is_told_about_its_class_and_a_checklog_about_nothing() {
    let declared = |s: &mut Settings| {
        s.contest_category_power = "HIGH".into();
        s.contest_category_assisted = "ASSISTED".into();
        s.contest_category_band = "ALL".into();
    };
    let mut e = running("cqww_cw", |s| {
        declared(s);
        s.contest_category_operator = "MULTI-OP".into();
        s.fd_operator = "K1ABC".into();
    });
    e.set_frequency(14.025, "20m", "CW");
    log(&mut e, "W2DEF", &[("RST", "599"), ("ZN", "5")], "CW");
    assert_eq!(missing(&e), ["CATEGORY-TRANSMITTER", "OPERATORS"]);
    save(&mut e, |s| s.contest_category_transmitter = "TWO".into());
    assert_eq!(missing(&e), Vec::<String>::new());
    let cab = e.export_log("cabrillo").expect("one entry");
    for line in ["CATEGORY-TRANSMITTER: TWO", "OPERATORS: K1ABC"] {
        assert!(header(&cab).iter().any(|l| l == line), "{line}: {cab}");
    }

    let checklog = running("cqww_cw", |s| {
        s.contest_category_operator = "CHECKLOG".into()
    });
    assert_eq!(
        missing(&checklog),
        Vec::<String>::new(),
        "a checklog is placed in no category"
    );
}
