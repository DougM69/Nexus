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
    logged(case, &station(case, entry), entry == Entry::Declared)
        .cabrillo_with(14_000, entrant)
        .unwrap_or_else(|e| panic!("{} exports: {e}", case.event))
}

/// One contest's log of its three rows, for the station `station`, with `K1ABC` at the key on
/// every row when `at_key`.
fn logged(case: &Case, station: &StationData, at_key: bool) -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id(case.event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, station)
        .unwrap_or_else(|e| panic!("{} refused: {e}", case.event));
    let mut log = FieldDayLog::new("W9XYZ", session, case.rows[0].1);
    if at_key {
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
    log
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

/// ⭐ **What a CQ file carries from declarations that already existed**, over the golden: the
/// CATEGORY lines CQ asks every entry for — the declared assistance, the declared `band` (`""`
/// for none), the contest's own mode and the declared power — then the entrant's CLUB, EMAIL
/// and NAME. `(category lines, entrant lines)`, each in file order; both empty for every
/// contest that is not CQ's. CQ WW RTTY wrote its CATEGORY lines before (its log decides the
/// band), so it gains CLUB alone.
fn cq_lines(event: &str, entry: Entry, band: &str) -> (Vec<String>, Vec<String>) {
    let declared = entry == Entry::Declared;
    let who = |lines: &[&str]| -> Vec<String> {
        if declared {
            lines.iter().map(|l| l.to_string()).collect()
        } else {
            Vec::new()
        }
    };
    let mode = match event {
        "cqww_cw" | "cqwpx_cw" => "CW",
        "cqww_ssb" | "cqwpx_ssb" => "SSB",
        "cqww_rtty" => return (Vec::new(), who(&["CLUB: EXAMPLE CONTEST CLUB"])),
        _ => return (Vec::new(), Vec::new()),
    };
    let mut categories = vec![format!(
        "CATEGORY-ASSISTED: {}",
        if declared { "ASSISTED" } else { "NON-ASSISTED" }
    )];
    if !band.is_empty() {
        categories.push(format!("CATEGORY-BAND: {band}"));
    }
    categories.push(format!("CATEGORY-MODE: {mode}"));
    categories.push("CATEGORY-POWER: LOW".to_string());
    (
        categories,
        who(&[
            "CLUB: EXAMPLE CONTEST CLUB",
            "EMAIL: k1abc@example.com",
            "NAME: A NAME",
        ]),
    )
}

/// ⭐ **`got` is `golden` with exactly `added` put in** — those lines, in that order — **and not
/// one other byte moved**, the QSO lines included.
fn assert_golden_plus(head: &str, got: &str, golden: &str, added: &[String]) {
    let new: Vec<&str> = got
        .lines()
        .filter(|l| !golden.lines().any(|g| g == *l))
        .collect();
    assert_eq!(new, added, "{head}: the added lines");
    assert!(
        added.iter().all(|l| !l.starts_with("QSO:")),
        "{head}: a QSO line is among the added ones"
    );
    let rest: String = got
        .lines()
        .filter(|l| !added.iter().any(|a| a == l))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        rest, golden,
        "{head}: a byte other than the added lines moved"
    );
}

/// ⭐ **No new declaration, no changed byte, but in a CQ file** — for all eighteen contests, as
/// the station's defaults and as a multi-operator entry declaring everything else. Thirteen
/// export their golden byte for byte; the five CQ contests export it with their category lines
/// and the entrant's lines put in ([`cq_lines`]), and nothing else moved.
#[test]
fn every_contests_file_is_its_golden_and_a_cq_file_gains_only_its_category_lines() {
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
    let mut cq = 0;
    for ((head, g), (_, w)) in got.iter().zip(&want) {
        let (event, entry) = head.split_once(' ').expect("`<event> <entry>`");
        let entry = if entry == "Declared" {
            Entry::Declared
        } else {
            Entry::Defaults
        };
        let (categories, who) = cq_lines(event, entry, "");
        let added: Vec<String> = categories.into_iter().chain(who).collect();
        if added.is_empty() {
            assert_eq!(g, w, "{head}: the Cabrillo file changed");
        } else {
            assert_golden_plus(head, g, w, &added);
            cq += 1;
        }
    }
    // POSITIVE CONTROL: the CQ files were the ones compared by their added lines — four
    // contests in both entries, and CQ WW RTTY's declared entry (its club).
    assert_eq!(cq, 9);
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
/// overlay every sponsor that takes either has (`ONE`, `ROOKIE`), the address, the soapbox, and
/// the band category every CQ contest takes (`ALL`, which CQ WW RTTY's log already says).
fn every_declaration(entry: Entry) -> CabrilloEntrant {
    CabrilloEntrant {
        transmitter: "ONE".into(),
        overlay: "ROOKIE".into(),
        address: address(),
        soapbox: SOAPBOX.into(),
        band: "ALL".into(),
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
    // A CQ contest's category lines come first and its entrant's CLUB, EMAIL and NAME after
    // the overlay, where the file carries them ([`cq_lines`]).
    let (categories, who) = cq_lines(event, entry, "ALL");
    let mut lines: Vec<&str> = categories.iter().map(String::as_str).collect();
    if tx && multi {
        lines.push("CATEGORY-TRANSMITTER: ONE");
    }
    if rookie {
        lines.push("CATEGORY-OVERLAY: ROOKIE");
    }
    lines.extend(who.iter().map(String::as_str));
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

/// The full file of a CQ WW multi-operator entry, every line in its place, by value: the lines of
/// the sponsor's own sample header (cqww.com/cabrillo.htm), each from a declaration.
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
            "CATEGORY-ASSISTED: ASSISTED",
            "CATEGORY-BAND: ALL",
            "CATEGORY-MODE: CW",
            "CATEGORY-POWER: LOW",
            "CATEGORY-TRANSMITTER: TWO",
            "LOCATION: IL",
            "CLUB: EXAMPLE CONTEST CLUB",
            "CREATED-BY: Nexus",
            "EMAIL: k1abc@example.com",
            "NAME: A NAME",
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

// ---------------------------------------------------------------------------------------------
// A contact edited after it was logged.
// ---------------------------------------------------------------------------------------------

use tempo_core::fieldday::ContactEdit;

/// The second contact's call as it was first logged: its last letter busted to an `X`.
fn busted(call: &str) -> String {
    format!("{}X", &call[..call.len() - 1])
}

/// One contest's log as [`export`] builds it, but with its second contact logged under a
/// [`busted`] call and then edited, as Ctrl+Y or the Contest screen's Edit sends it, to the call
/// the golden logged: the file before the edit, and the file after it.
fn export_edited(case: &Case, entry: Entry, entrant: &CabrilloEntrant) -> (String, String) {
    resolver();
    let rs = ruleset_by_id(case.event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, &station(case, entry))
        .unwrap_or_else(|e| panic!("{} refused: {e}", case.event));
    let mut log = FieldDayLog::new("W9XYZ", session, case.rows[0].1);
    if entry == Entry::Declared {
        log.operator = "K1ABC".into();
    }
    let pairs = |fields: Fields| -> Vec<(String, String)> {
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let when = |i: usize| 1_790_380_800 + 60 * i as u64;
    for (i, &(call, band, fields)) in case.rows.iter().enumerate() {
        log.band = band.to_string();
        let call = if i == 1 {
            busted(call)
        } else {
            call.to_string()
        };
        assert!(
            log.log_fields_at(&call, &pairs(fields), case.mode, case.submode, 0, when(i)),
            "{}: {call} logs",
            case.event
        );
    }
    let file = |log: &FieldDayLog| {
        log.cabrillo_with(14_000, entrant)
            .unwrap_or_else(|e| panic!("{} exports: {e}", case.event))
    };
    let before = file(&log);
    let (call, band, fields) = case.rows[1];
    let edit = ContactEdit {
        call: call.into(),
        rows: vec![pairs(fields)],
        band: band.into(),
        mode: String::new(),
    };
    log.edit_contact(&busted(call), when(1), &edit)
        .unwrap_or_else(|e| panic!("{}: the edit was refused: {e:?}", case.event));
    (before, file(&log))
}

/// ⭐ **An edited call reaches the file, and nothing else moves** — all eighteen contests, both
/// entries. The second contact is logged under a busted call and edited to the call the golden
/// logged, and the file is then the golden, byte for byte (a CQ file with its category lines
/// put in, [`cq_lines`]): an edit keeps the contact's time, serial, operator and what was sent,
/// so only its call may change, and it changes to the right one. CONTROL: before the edit every
/// file differs from the edited one on that one QSO line, by the call alone.
#[test]
fn an_edited_call_exports_the_golden_of_the_call_logged_right() {
    let golden = std::fs::read_to_string(GOLDEN).unwrap_or_else(|e| panic!("{GOLDEN}: {e}"));
    let goldens: std::collections::HashMap<&str, &str> = golden
        .split("### ")
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('\n').expect("a heading line"))
        .collect();
    let mut files = 0;
    for case in &cases() {
        let right = case.rows[1].0;
        let wrong = busted(right);
        for entry in [Entry::Defaults, Entry::Declared] {
            let head = format!("{} {entry:?}", case.event);
            let want = goldens[head.as_str()];
            let (before, after) = export_edited(case, entry, &existing_entrant(entry));
            let (categories, who) = cq_lines(case.event, entry, "");
            let added: Vec<String> = categories.into_iter().chain(who).collect();
            assert_golden_plus(&head, &after, want, &added);
            assert!(!after.contains(&wrong), "{head}: the busted call stayed");
            // CONTROL: the busted call was in the file until the edit, on one QSO line only.
            assert_eq!(
                before.lines().count(),
                after.lines().count(),
                "{head}: line count"
            );
            let differ: Vec<(&str, &str)> = before
                .lines()
                .zip(after.lines())
                .filter(|(b, w)| b != w)
                .collect();
            assert_eq!(differ.len(), 1, "{head}: before the edit: {differ:?}");
            assert!(differ[0].0.starts_with("QSO: "), "{head}: {differ:?}");
            assert_eq!(
                differ[0].0.replace(&wrong, right),
                differ[0].1,
                "{head}: by the call alone"
            );
            files += 1;
        }
    }
    assert_eq!(files, 36, "eighteen contests, two entries each");
}

// ---------------------------------------------------------------------------------------------
// The CQ contests' CATEGORY lines: the band an entry declares, and what is missing.
// ---------------------------------------------------------------------------------------------

/// ⭐ **A declared band is written where the sponsor lists it, and nowhere else.** CQ's pages:
/// *"If you are entering an all band category, use ALL, otherwise select one of the individual
/// bands shown"*. CQ WW and WPX gain the line; CQ WW RTTY's log said `ALL` (20 m and 40 m) and
/// the declaration says `20M`, which is what a single-band entrant who also logged other bands
/// needs (*"Single band entrants are required to include all contacts made during the contest
/// period, even if made on other bands"*); every other contest's file does not move.
#[test]
fn a_declared_band_is_written_only_where_the_sponsor_lists_it() {
    let with_band = |band: &str| CabrilloEntrant {
        band: band.into(),
        ..CabrilloEntrant::default()
    };
    let band_lines = |file: &str| -> Vec<String> {
        file.lines()
            .filter(|l| l.starts_with("CATEGORY-BAND"))
            .map(String::from)
            .collect()
    };
    let mut written = 0;
    for case in &cases() {
        let none = export(case, Entry::Defaults, &CabrilloEntrant::default());
        let got = export(case, Entry::Defaults, &with_band("20M"));
        match case.event {
            "cqww_cw" | "cqww_ssb" | "cqwpx_cw" | "cqwpx_ssb" => {
                assert_golden_plus(case.event, &got, &none, &["CATEGORY-BAND: 20M".into()]);
                written += 1;
            }
            "cqww_rtty" => {
                assert_eq!(band_lines(&none), ["CATEGORY-BAND: ALL"], "the log's own");
                assert_eq!(
                    got,
                    none.replace("CATEGORY-BAND: ALL\n", "CATEGORY-BAND: 20M\n")
                );
                written += 1;
            }
            _ => assert_eq!(got, none, "{}: no sponsor of its lists a band", case.event),
        }
    }
    assert_eq!(written, 5, "the five CQ contests");
    // A band the sponsor does not list claims nothing: 6 m is no CQ WW band, and 160 m is no CQ
    // WW RTTY band, whose log decides as it did before. A typed token is the sponsor's own.
    let case = |event: &str| -> Case {
        cases()
            .into_iter()
            .find(|c| c.event == event)
            .expect("shipped")
    };
    let cqww = case("cqww_cw");
    let rtty = case("cqww_rtty");
    assert_eq!(
        band_lines(&export(&cqww, Entry::Defaults, &with_band("6M"))),
        Vec::<String>::new()
    );
    assert_eq!(
        band_lines(&export(&rtty, Entry::Defaults, &with_band("160M"))),
        ["CATEGORY-BAND: ALL"]
    );
    assert_eq!(
        band_lines(&export(&cqww, Entry::Defaults, &with_band(" 160m "))),
        ["CATEGORY-BAND: 160M"]
    );
}

/// ⭐ **What CQ requires and the entry left blank is named, before the export** — and only that.
///
/// CQ's rules: *"Failure to fill out the header correctly may result in the entry being placed
/// in the wrong category or reclassified as a Checklog"*; its pages: the category-transmitter
/// *"is required for multi-operator entries"*, and *"If you select a multi-operator category and
/// do not enter operator callsigns in this field, the robot will reject your log"*. So a scored
/// CQ entry's file needs its assistance, band, mode and power, and a multi-operator one its
/// class and operators. A checklog is not scored, and no other contest's sponsor is read as
/// requiring any of them; the Illinois QSO Party requires its own two, the entrant's ENTRY-CLASS and
/// EMAIL. Each name is a line the export leaves out, by value.
#[test]
fn the_lines_cq_requires_and_the_entry_left_blank_are_named() {
    let ssb = cases()
        .into_iter()
        .find(|c| c.event == "cqww_ssb")
        .expect("shipped");
    // `(operator category, power, assisted)` as declared, and whether K1ABC was at the key.
    let missing = |declared: (&str, &str, &str), at_key: bool, entrant: &CabrilloEntrant| {
        let (category, power, assisted) = declared;
        let entry = StationData {
            contest_category_operator: category.into(),
            contest_category_power: power.into(),
            contest_category_assisted: assisted.into(),
            ..station(&ssb, Entry::Defaults)
        };
        let log = logged(&ssb, &entry, at_key);
        let file = log.cabrillo_with(14_000, entrant).expect("exports");
        let named = log.cabrillo_missing(entrant);
        // CONTROL: every line named is one the file leaves out.
        for tag in &named {
            assert!(
                !file.lines().any(|l| l.starts_with(&format!("{tag}: "))),
                "{tag} is named and in the file:\n{file}"
            );
        }
        named
    };
    let nothing = CabrilloEntrant::default();
    let band = CabrilloEntrant {
        band: "ALL".into(),
        ..CabrilloEntrant::default()
    };
    let class = |tx: &str, typed: &str| CabrilloEntrant {
        band: "ALL".into(),
        transmitter: tx.into(),
        operators: typed.into(),
        ..CabrilloEntrant::default()
    };
    assert_eq!(
        missing(("SINGLE-OP", "", ""), false, &nothing),
        ["CATEGORY-ASSISTED", "CATEGORY-BAND", "CATEGORY-POWER"],
        "nothing declared"
    );
    assert_eq!(
        missing(("SINGLE-OP", "LOW", ""), false, &band),
        ["CATEGORY-ASSISTED"]
    );
    assert_eq!(
        missing(("SINGLE-OP", "LOW", "NON-ASSISTED"), false, &band),
        Vec::<&str>::new(),
        "everything a single operator's file needs"
    );
    assert_eq!(
        missing(("MULTI-OP", "LOW", "ASSISTED"), false, &band),
        ["CATEGORY-TRANSMITTER", "OPERATORS"],
        "a multi-operator entry with no class"
    );
    assert_eq!(
        missing(("MULTI-OP", "LOW", "ASSISTED"), false, &class("TWO", "")),
        ["OPERATORS"],
        "a class, and nobody named at the key or typed"
    );
    assert_eq!(
        missing(("MULTI-OP", "LOW", "ASSISTED"), true, &class("TWO", "")),
        Vec::<&str>::new(),
        "the operator at the key names one"
    );
    assert_eq!(
        missing(
            ("MULTI-OP", "LOW", "ASSISTED"),
            false,
            &class("TWO", "W2DEF")
        ),
        Vec::<&str>::new(),
        "a typed operator names one"
    );
    assert_eq!(
        missing(("CHECKLOG", "", ""), false, &nothing),
        Vec::<&str>::new(),
        "a checklog is not placed in a category"
    );
    // Every other contest: nothing is named, whatever is blank. The CQ ones name what these
    // entries leave out (no band declared; a multi-operator entry with no class, whose OPERATORS
    // line comes with the class), which is the control on this loop, and the Illinois QSO Party
    // the entrant's class and email, left blank here.
    let mut named = Vec::new();
    for case in &cases() {
        for entry in [Entry::Defaults, Entry::Declared] {
            let log = logged(case, &station(case, entry), entry == Entry::Declared);
            let m = log.cabrillo_missing(&CabrilloEntrant::default());
            if !m.is_empty() {
                named.push(format!("{} {entry:?}: {}", case.event, m.join(" ")));
            }
        }
    }
    assert_eq!(
        named,
        [
            "ilqp Defaults: ENTRY-CLASS EMAIL",
            "ilqp Declared: ENTRY-CLASS EMAIL",
            "cqww_cw Defaults: CATEGORY-BAND",
            "cqww_cw Declared: CATEGORY-BAND CATEGORY-TRANSMITTER OPERATORS",
            "cqww_ssb Defaults: CATEGORY-BAND",
            "cqww_ssb Declared: CATEGORY-BAND CATEGORY-TRANSMITTER OPERATORS",
            "cqww_rtty Declared: CATEGORY-TRANSMITTER OPERATORS",
            "cqwpx_cw Defaults: CATEGORY-BAND",
            "cqwpx_cw Declared: CATEGORY-BAND CATEGORY-TRANSMITTER OPERATORS",
            "cqwpx_ssb Defaults: CATEGORY-BAND",
            "cqwpx_ssb Declared: CATEGORY-BAND CATEGORY-TRANSMITTER OPERATORS",
        ]
    );
}
