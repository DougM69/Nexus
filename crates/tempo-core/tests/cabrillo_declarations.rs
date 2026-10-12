//! ⭐ **The entry declarations a sponsor's Cabrillo template names** — `CATEGORY-TRANSMITTER`,
//! `CATEGORY-OVERLAY`, the `ADDRESS` lines and `SOAPBOX` — **and the proof that a log with none
//! of them set exports the file it exported before they existed, for every contest.**
//!
//! `tests/fixtures/cabrillo-declarations/unset.txt` was written by this test against the writer
//! as it stood before those declarations existed, with `NEXUS_WRITE_DECLARATION_GOLDENS=1`: all
//! eighteen shipped contests, each twice — the station's defaults, and a multi-operator entry
//! carrying every declaration that existed then (name, email, club, entry class, other operators,
//! a Winter Field Day objective, power, assistance, an operator at the key). The default mode
//! compares today's writer with it. Regenerating it against today's writer would make this test
//! prove nothing.
//!
//! Every callsign is an example call.
use tempo_core::contest::{
    install_call_resolver, CabrilloEntrant, CallLocation, ContestSession, StationData,
};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

const GOLDEN: &str = "tests/fixtures/cabrillo-declarations/unset.txt";

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let (entity, continent) = if up.starts_with("DL") {
        ("Fed. Rep. of Germany", "EU")
    } else if up.starts_with("JA") {
        ("Japan", "AS")
    } else if up.starts_with("VE") {
        ("Canada", "NA")
    } else if up.starts_with('W') || up.starts_with('K') || up.starts_with('N') {
        ("United States", "NA")
    } else {
        return None;
    };
    Some(CallLocation {
        entity,
        continent,
        cq_zone: None,
    })
}

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

/// One row's received exchange, as the entry strip hands it over: `(slot, raw)` pairs.
type Fields = &'static [(&'static str, &'static str)];

/// One contest's log: where the station is, how its contacts were made, and three rows.
struct Case {
    event: &'static str,
    mode: &'static str,
    submode: &'static str,
    /// `(state, county)` for the station data.
    qth: (&'static str, &'static str),
    rows: [(&'static str, &'static str, Fields); 3],
}

fn cases() -> Vec<Case> {
    const HF: [(&str, &str); 3] = [("K1ABC", "20m"), ("DL1ABC", "20m"), ("JA1ABC", "40m")];
    const VHF: [(&str, &str); 3] = [("K1ABC", "6m"), ("W2DEF", "6m"), ("N3GHI", "2m")];
    let hf = |f: [Fields; 3]| {
        [
            (HF[0].0, HF[0].1, f[0]),
            (HF[1].0, HF[1].1, f[1]),
            (HF[2].0, HF[2].1, f[2]),
        ]
    };
    let vhf = |f: [Fields; 3]| {
        [
            (VHF[0].0, VHF[0].1, f[0]),
            (VHF[1].0, VHF[1].1, f[1]),
            (VHF[2].0, VHF[2].1, f[2]),
        ]
    };
    let party: [Fields; 3] = [
        &[("RST", "599"), ("QTH", "IL")],
        &[("RST", "599"), ("QTH", "OH")],
        &[("RST", "599"), ("QTH", "CA")],
    ];
    let ss: [(&str, &str, Fields); 3] = [
        (
            "K1ABC",
            "20m",
            &[
                ("NR", "1"),
                ("PREC", "A"),
                ("CALL", "K1ABC"),
                ("CK", "74"),
                ("SEC", "CT"),
            ],
        ),
        (
            "W2DEF",
            "20m",
            &[
                ("NR", "22"),
                ("PREC", "B"),
                ("CALL", "W2DEF"),
                ("CK", "99"),
                ("SEC", "ENY"),
            ],
        ),
        (
            "N3GHI",
            "40m",
            &[
                ("NR", "5"),
                ("PREC", "Q"),
                ("CALL", "N3GHI"),
                ("CK", "05"),
                ("SEC", "EPA"),
            ],
        ),
    ];
    let grids: [Fields; 3] = [
        &[("GRID", "FN31")],
        &[("GRID", "EM12")],
        &[("GRID", "DM79")],
    ];
    let zones = |rst: &'static str| -> [Fields; 3] {
        match rst {
            "59" => [
                &[("RST", "59"), ("ZN", "5")],
                &[("RST", "59"), ("ZN", "14")],
                &[("RST", "59"), ("ZN", "25")],
            ],
            _ => [
                &[("RST", "599"), ("ZN", "5")],
                &[("RST", "599"), ("ZN", "14")],
                &[("RST", "599"), ("ZN", "25")],
            ],
        }
    };
    let serials = |rst: &'static str| -> [Fields; 3] {
        match rst {
            "59" => [
                &[("RST", "59"), ("NR", "1")],
                &[("RST", "59"), ("NR", "2")],
                &[("RST", "59"), ("NR", "3")],
            ],
            _ => [
                &[("RST", "599"), ("NR", "1")],
                &[("RST", "599"), ("NR", "2")],
                &[("RST", "599"), ("NR", "3")],
            ],
        }
    };
    let case = |event, mode, rows| Case {
        event,
        mode,
        submode: "",
        qth: ("IL", ""),
        rows,
    };
    vec![
        case(
            "arrlfd",
            "CW",
            hf([
                &[("CLASS", "2A"), ("SECTION", "IL")],
                &[("CLASS", "1D"), ("SECTION", "EMA")],
                &[("CLASS", "4F"), ("SECTION", "ONS")],
            ]),
        ),
        case(
            "wfd",
            "CW",
            hf([
                &[("CLASS", "2H"), ("SECTION", "IL")],
                &[("CLASS", "1O"), ("SECTION", "EMA")],
                &[("CLASS", "3M"), ("SECTION", "ONS")],
            ]),
        ),
        case("tnqp", "CW", hf(party)),
        case("ohqp", "CW", hf(party)),
        case(
            "cqp",
            "CW",
            hf([
                &[("NR", "1"), ("QTH", "IL")],
                &[("NR", "2"), ("QTH", "NV")],
                &[("NR", "3"), ("QTH", "DX")],
            ]),
        ),
        case("txqp", "CW", hf(party)),
        Case {
            event: "ilqp",
            mode: "CW",
            submode: "",
            qth: ("IL", "COOK"),
            rows: hf([
                &[("RST", "599"), ("QTH", "DUPG")],
                &[("RST", "599"), ("QTH", "MI")],
                &[("RST", "599"), ("QTH", "ON")],
            ]),
        },
        Case {
            event: "nyqp",
            mode: "CW",
            submode: "",
            qth: ("NY", "MON"),
            rows: hf([
                &[("RST", "599"), ("QTH", "ULS")],
                &[("RST", "599"), ("QTH", "OH")],
                &[("RST", "599"), ("QTH", "ON")],
            ]),
        },
        case("arrlss_cw", "CW", ss),
        case("arrlss_ssb", "PH", ss),
        case("cqww_cw", "CW", hf(zones("599"))),
        case("cqww_ssb", "PH", hf(zones("59"))),
        Case {
            event: "cqww_rtty",
            mode: "DIG",
            submode: "RTTY",
            qth: ("IL", ""),
            rows: hf([
                &[("RST", "599"), ("ZN", "5"), ("QTH", "MA")],
                &[("RST", "599"), ("ZN", "14"), ("QTH", "")],
                &[("RST", "599"), ("ZN", "25"), ("QTH", "")],
            ]),
        },
        case("cqwpx_cw", "CW", hf(serials("599"))),
        case("cqwpx_ssb", "PH", hf(serials("59"))),
        case("arrlvhf_jan", "PH", vhf(grids)),
        case("arrlvhf_jun", "PH", vhf(grids)),
        case("arrlvhf_sep", "PH", vhf(grids)),
    ]
}

/// The two entries every contest is exported as: the station's defaults, and a multi-operator
/// entry declaring everything that could be declared before this file's headers existed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Entry {
    Defaults,
    Declared,
}

fn station(case: &Case, entry: Entry) -> StationData {
    let declared = entry == Entry::Declared;
    StationData {
        mycall: "W9XYZ".into(),
        mygrid: "EN52".into(),
        fd_class: if case.event == "wfd" {
            "3H".into()
        } else {
            "3A".into()
        },
        fd_section: "WI".into(),
        contest_qth_state: case.qth.0.into(),
        contest_qth_county: case.qth.1.into(),
        contest_cq_zone: "4".into(),
        contest_check: "74".into(),
        contest_category_operator: if declared { "MULTI-OP" } else { "SINGLE-OP" }.into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: if declared { "ASSISTED" } else { "NON-ASSISTED" }.into(),
        ..Default::default()
    }
}

/// Every declaration an entrant could make before this file's headers existed.
fn declared_entrant() -> CabrilloEntrant {
    CabrilloEntrant {
        name: "A NAME".into(),
        email: "k1abc@example.com".into(),
        club: "EXAMPLE CONTEST CLUB".into(),
        entry_class: "UNLIMITED".into(),
        operators: "W2DEF, N3GHI".into(),
        objectives: vec!["wfd-alt-power-equipment".into()],
        ..Default::default()
    }
}

/// One contest's log, built and exported as the entry `entry` with the entrant `entrant`.
fn export(case: &Case, entry: Entry, entrant: &CabrilloEntrant) -> String {
    resolver();
    let rs = ruleset_by_id(case.event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, &station(case, entry))
        .unwrap_or_else(|e| panic!("{} refused: {e}", case.event));
    let mut log = FieldDayLog::new("W9XYZ", session, case.rows[0].1);
    if entry == Entry::Declared {
        log.operator = "K1ABC".into();
    }
    for (i, (call, band, fields)) in case.rows.iter().enumerate() {
        log.band = band.to_string();
        let fields: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert!(
            log.log_fields_at(
                call,
                &fields,
                case.mode,
                case.submode,
                0,
                1_790_380_800 + 60 * i as u64
            ),
            "{}: {call} logs",
            case.event
        );
    }
    log.cabrillo_with(14_000, entrant)
        .unwrap_or_else(|e| panic!("{} exports: {e}", case.event))
}

/// Every contest's two files, in one document: `### <event> <entry>` above each.
fn every_file(entrant_for: impl Fn(Entry) -> CabrilloEntrant) -> String {
    let mut out = String::new();
    for case in &cases() {
        for entry in [Entry::Defaults, Entry::Declared] {
            out.push_str(&format!("### {} {entry:?}\n", case.event));
            out.push_str(&export(case, entry, &entrant_for(entry)));
        }
    }
    out
}

/// The entrant each entry exports with when no new declaration is set.
fn existing_entrant(entry: Entry) -> CabrilloEntrant {
    match entry {
        Entry::Defaults => CabrilloEntrant::default(),
        Entry::Declared => declared_entrant(),
    }
}

/// ⭐ **No new declaration, no changed byte** — for all eighteen contests, as the station's
/// defaults and as a multi-operator entry declaring everything else.
#[test]
fn every_contests_file_is_unchanged_with_no_new_declaration() {
    assert_eq!(cases().len(), 18, "every shipped contest");
    let got = every_file(existing_entrant);
    if std::env::var_os("NEXUS_WRITE_DECLARATION_GOLDENS").is_some() {
        std::fs::create_dir_all("tests/fixtures/cabrillo-declarations").expect("golden dir");
        std::fs::write(GOLDEN, &got).expect("golden written");
        panic!("goldens written — this run proves nothing; run again without the variable");
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_else(|e| {
        panic!("{GOLDEN}: {e} (written by the writer before the declarations)")
    });
    // File by file, so a failure names the contest rather than one 40 KB string.
    let split = |s: &str| -> Vec<(String, String)> {
        s.split("### ")
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (head, body) = p.split_once('\n').expect("a heading line");
                (head.to_string(), body.to_string())
            })
            .collect()
    };
    let (got, want) = (split(&got), split(&want));
    assert_eq!(got.len(), 36, "eighteen contests, two entries each");
    assert_eq!(
        got.iter().map(|(h, _)| h).collect::<Vec<_>>(),
        want.iter().map(|(h, _)| h).collect::<Vec<_>>()
    );
    for ((head, g), (_, w)) in got.iter().zip(&want) {
        assert_eq!(g, w, "{head}: the Cabrillo file changed");
    }
}

// ---------------------------------------------------------------------------------------------
// The declarations themselves.
// ---------------------------------------------------------------------------------------------

use tempo_core::contest::CabrilloAddress;

/// A mailing address with every part filled in, two street lines first.
fn address() -> CabrilloAddress {
    CabrilloAddress {
        street: "1 Example Rd\nApt 2".into(),
        city: "Springfield".into(),
        state_province: "IL".into(),
        postal_code: "62701".into(),
        country: "USA".into(),
    }
}

/// The soapbox as typed: two lines with a blank one between them.
const SOAPBOX: &str = "A fine weekend.\n\nFirst licensed 2024-05-01.";

/// [`existing_entrant`] with every new declaration set as well: a transmitter class and an
/// overlay every sponsor that takes either has (`ONE`, `ROOKIE`), the address and the soapbox.
fn every_declaration(entry: Entry) -> CabrilloEntrant {
    CabrilloEntrant {
        transmitter: "ONE".into(),
        overlay: "ROOKIE".into(),
        address: address(),
        soapbox: SOAPBOX.into(),
        ..existing_entrant(entry)
    }
}

/// How one sponsor's template takes the address.
#[derive(Clone, Copy)]
enum Address {
    None,
    /// `ADDRESS` alone (the ARRL templates, Tennessee).
    Lines,
    /// Every part but the country on its own header (the Illinois QSO Party's sample).
    NoCountry,
    /// All five (CQ WW, CQ WPX, CQ WW RTTY, the New York QSO Party).
    All,
}

/// The lines every declaration adds to one contest's file, by value: what its sponsor names, in
/// the order the file carries them. `entry` decides the transmitter class (a multi-operator
/// entry's alone) and the OPERATORS line that comes with it.
fn added_lines(event: &str, entry: Entry) -> Vec<String> {
    // (transmitter classes, ROOKIE overlay, address, soapbox)
    let (tx, rookie, addr, soapbox) = match event {
        "arrlfd" | "wfd" | "ohqp" | "txqp" => (false, false, Address::None, false),
        "tnqp" => (false, false, Address::Lines, false),
        "cqp" => (false, false, Address::None, true),
        "ilqp" => (false, false, Address::NoCountry, true),
        "nyqp" => (true, true, Address::All, false),
        "arrlss_cw" | "arrlss_ssb" => (true, false, Address::Lines, false),
        "cqww_cw" | "cqww_ssb" | "cqww_rtty" | "cqwpx_cw" | "cqwpx_ssb" => {
            (true, true, Address::All, true)
        }
        "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => (true, false, Address::Lines, true),
        other => panic!("no row for {other}"),
    };
    let multi = entry == Entry::Declared;
    let mut lines: Vec<&str> = Vec::new();
    if tx && multi {
        lines.push("CATEGORY-TRANSMITTER: ONE");
    }
    if rookie {
        lines.push("CATEGORY-OVERLAY: ROOKIE");
    }
    match addr {
        Address::None => {}
        Address::Lines => lines.extend([
            "ADDRESS: 1 Example Rd",
            "ADDRESS: Apt 2",
            "ADDRESS: Springfield, IL 62701",
            "ADDRESS: USA",
        ]),
        Address::NoCountry => lines.extend([
            "ADDRESS: 1 Example Rd",
            "ADDRESS: Apt 2",
            "ADDRESS: USA",
            "ADDRESS-CITY: Springfield",
            "ADDRESS-STATE-PROVINCE: IL",
            "ADDRESS-POSTALCODE: 62701",
        ]),
        Address::All => lines.extend([
            "ADDRESS: 1 Example Rd",
            "ADDRESS: Apt 2",
            "ADDRESS-CITY: Springfield",
            "ADDRESS-STATE-PROVINCE: IL",
            "ADDRESS-POSTALCODE: 62701",
            "ADDRESS-COUNTRY: USA",
        ]),
    }
    if tx && multi {
        // Everyone the rows name at the key, then the typed operators.
        lines.push("OPERATORS: K1ABC W2DEF N3GHI");
    }
    if soapbox {
        lines.extend([
            "SOAPBOX: A fine weekend.",
            "SOAPBOX: First licensed 2024-05-01.",
        ]);
    }
    lines.into_iter().map(String::from).collect()
}

/// ⭐ **Every declaration set: each contest's file gains exactly the lines its sponsor names, and
/// not one other byte moves.** The file with those lines taken out is the golden above, and the
/// lines taken out are the sponsor's, by value and in order — so a contest whose sponsor names
/// none (both Field Days, Ohio, Texas) exports its unchanged file with every declaration set.
#[test]
fn every_declaration_adds_only_the_lines_each_sponsor_names() {
    let want = std::fs::read_to_string(GOLDEN).expect("golden");
    let mut changed = 0;
    for case in &cases() {
        for entry in [Entry::Defaults, Entry::Declared] {
            let got = export(case, entry, &every_declaration(entry));
            let golden = want
                .split("### ")
                .find_map(|p| {
                    let (head, body) = p.split_once('\n')?;
                    (head == format!("{} {entry:?}", case.event)).then(|| body.to_string())
                })
                .expect("a golden for every file");
            let expected = added_lines(case.event, entry);
            let added: Vec<String> = got
                .lines()
                .filter(|l| !golden.lines().any(|g| g == *l))
                .map(String::from)
                .collect();
            assert_eq!(added, expected, "{} {entry:?}: the added lines", case.event);
            let rest: String = got
                .lines()
                .filter(|l| !expected.iter().any(|e| e == l))
                .map(|l| format!("{l}\n"))
                .collect();
            assert_eq!(
                rest, golden,
                "{} {entry:?}: a byte other than the declarations moved",
                case.event
            );
            if !expected.is_empty() {
                changed += 1;
            }
        }
    }
    // POSITIVE CONTROL: the comparison saw declarations land — fourteen contests name at least
    // one, each in both entries.
    assert_eq!(changed, 28);
}

/// The full file of a CQ WW multi-operator entry, every line in its place, by value.
#[test]
fn a_cq_ww_multi_two_entry_heads_its_file_the_way_the_sponsor_shows() {
    let case = cases()
        .into_iter()
        .find(|c| c.event == "cqww_cw")
        .expect("cqww_cw");
    let me = CabrilloEntrant {
        transmitter: "two".into(),
        overlay: String::new(),
        ..every_declaration(Entry::Declared)
    };
    let got = export(&case, Entry::Declared, &me);
    let header: Vec<&str> = got.lines().take_while(|l| !l.starts_with("QSO:")).collect();
    assert_eq!(
        header,
        [
            "START-OF-LOG: 3.0",
            "CONTEST: CQ-WW-CW",
            "CALLSIGN: W9XYZ",
            "CATEGORY-OPERATOR: MULTI-OP",
            "CATEGORY-TRANSMITTER: TWO",
            "LOCATION: IL",
            "CREATED-BY: Nexus",
            "ADDRESS: 1 Example Rd",
            "ADDRESS: Apt 2",
            "ADDRESS-CITY: Springfield",
            "ADDRESS-STATE-PROVINCE: IL",
            "ADDRESS-POSTALCODE: 62701",
            "ADDRESS-COUNTRY: USA",
            "OPERATORS: K1ABC W2DEF N3GHI",
            "SOAPBOX: A fine weekend.",
            "SOAPBOX: First licensed 2024-05-01.",
            "X-NEXUS-RULES-YEAR: 2026",
        ]
    );
}

/// ⭐ **A class this contest does not have, or a single operator's, claims nothing** — and takes
/// no OPERATORS line with it. Sweepstakes' only multi-operator category is single-transmitter, so
/// `TWO` writes nothing there; a single operator writes no transmitter class at all, as CQ WW
/// *"required for multi-operator entries"* asks of nobody else.
#[test]
fn a_class_the_contest_lacks_or_a_single_operator_declares_writes_nothing() {
    let ss = cases()
        .into_iter()
        .find(|c| c.event == "arrlss_cw")
        .expect("ss");
    let cq = cases()
        .into_iter()
        .find(|c| c.event == "cqww_cw")
        .expect("cqww");
    let with = |tx: &str| CabrilloEntrant {
        transmitter: tx.into(),
        ..existing_entrant(Entry::Declared)
    };
    let transmitter_lines = |file: &str| -> Vec<String> {
        file.lines()
            .filter(|l| l.starts_with("CATEGORY-TRANSMITTER") || l.starts_with("OPERATORS"))
            .map(String::from)
            .collect()
    };
    assert_eq!(
        transmitter_lines(&export(&ss, Entry::Declared, &with("TWO"))),
        Vec::<String>::new()
    );
    assert_eq!(
        transmitter_lines(&export(&cq, Entry::Defaults, &with("TWO"))),
        Vec::<String>::new(),
        "a single operator"
    );
    // CONTROLS: the same contests, a class they have, a multi-operator entry.
    assert_eq!(
        transmitter_lines(&export(&ss, Entry::Declared, &with("ONE"))),
        ["CATEGORY-TRANSMITTER: ONE", "OPERATORS: K1ABC W2DEF N3GHI"]
    );
    assert_eq!(
        transmitter_lines(&export(&cq, Entry::Declared, &with("TWO"))),
        ["CATEGORY-TRANSMITTER: TWO", "OPERATORS: K1ABC W2DEF N3GHI"]
    );
}

/// An overlay this contest does not score claims nothing: CQ WPX's `TB-WIRES` is no CQ WW
/// overlay. CONTROL: WPX writes it.
#[test]
fn an_overlay_the_contest_does_not_score_writes_nothing() {
    let overlay = |event: &str| -> Vec<String> {
        let case = cases()
            .into_iter()
            .find(|c| c.event == event)
            .expect("shipped");
        let me = CabrilloEntrant {
            overlay: "TB-WIRES".into(),
            ..CabrilloEntrant::default()
        };
        export(&case, Entry::Defaults, &me)
            .lines()
            .filter(|l| l.starts_with("CATEGORY-OVERLAY"))
            .map(String::from)
            .collect()
    };
    assert_eq!(overlay("cqww_cw"), Vec::<String>::new());
    assert_eq!(overlay("cqwpx_cw"), ["CATEGORY-OVERLAY: TB-WIRES"]);
}
