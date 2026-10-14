//! DXCC, WAS and WAZ count each contact by the numbers its own record carries — the DXCC entity
//! number (ADIF `DXCC`, which LoTW's QSL detail and other loggers' exports write, and which the
//! ARRL credits by), LoTW's DXCC where it differs from the log's, and the CQ zone (ADIF `CQZ`) —
//! and fall back to cty.dat's reading of the call only where the record carries none.
//!
//! The reports behind it: a log whose LoTW account shows 340 current / 349 total confirmed was
//! counted 336, and a 52,521-contact DX4WIN import counted 337 where the ARRL counts 340. Each
//! missing entity was a DXpedition on a US-style call (K1N Navassa 2015, K1B Baker & Howland
//! 2002, K3J Johnston 2001…), which cty.dat files under the United States, since the same 1×1
//! and 1×2 calls are reissued for US special events. Each row below goes through
//! `Awards::add_qso` as the app's fold calls it, with what the record carries.

use std::collections::BTreeSet;

use propagation::{dxcc, AwardSummary, Awards, RecordFields};

/// A record's DXCC number and nothing else.
fn dxcc_of(code: u32) -> RecordFields {
    RecordFields {
        dxcc: Some(code),
        ..RecordFields::default()
    }
}

/// One contact into `a` as the app's fold adds it: confirmed via LoTW, 20 m CW, terrestrial.
fn add(a: &mut Awards, call: &str, confirmed: bool, state: Option<&str>, rec: RecordFields) {
    a.add_qso(
        call, "20m", "CW", confirmed, false, false, state, None, None, false, rec,
    );
}

/// (call, the ADIF DXCC code its LoTW QSL / logger's export carries, deleted entity?, what it is)
const DISAGREE: &[(&str, u32, bool, &str)] = &[
    ("W1AW", 291, false, "United States (control row)"),
    ("K1N", 182, false, "Navassa Island, Feb 2015"),
    ("K5D", 43, false, "Desecheo Island, Feb 2009"),
    ("K4M", 174, false, "Midway Island, Oct 2009"),
    ("K5P", 197, false, "Palmyra & Jarvis, Jan 2016"),
    ("N8S", 515, false, "Swains Island, Apr 2007"),
    ("IT9ABC", 248, false, "Sicily: ARRL credits Italy"),
    (
        "Y24UK",
        229,
        true,
        "German Democratic Republic, deleted 1990",
    ),
];

/// The control: the same entity codes, each on a call cty.dat files under that entity — the
/// last field is the entity cty.dat must name, asserted row by row (a count alone matched once
/// by coincidence: KH4AA is an exact call under Puerto Rico).
const AGREE: &[(&str, u32, bool, &str)] = &[
    ("W1AW", 291, false, "United States"),
    ("KP1AA", 182, false, "Navassa Island"),
    ("KP5AA", 43, false, "Desecheo Island"),
    ("KH4XYZ", 174, false, "Midway Island"),
    ("KH5AA", 197, false, "Palmyra & Jarvis Islands"),
    ("KH8SI", 515, false, "Swains Island"),
    ("I1ABC", 248, false, "Italy"),
    ("OE1ABC", 206, false, "Austria"),
    ("4U1VIC", 206, false, "Austria"),
];

/// Fold `rows` with their numbers (or without, `by_number = false`): Nexus's current and total
/// confirmed counts, and the current and total by the numbers.
fn fold(rows: &[(&str, u32, bool, &str)], by_number: bool) -> (usize, usize, usize, usize) {
    let mut a = Awards::new();
    for (call, code, _, what) in rows {
        let r = dxcc::resolve(call);
        println!(
            "{call:8} record DXCC {code:3}  cty.dat -> {:28} is_dxcc={:5}  ({what})",
            r.map_or("(none)", |i| i.entity),
            r.is_some_and(|i| i.is_dxcc)
        );
        let rec = if by_number {
            dxcc_of(*code)
        } else {
            RecordFields::default()
        };
        add(&mut a, call, true, None, rec);
    }
    let current: BTreeSet<u32> = rows.iter().filter(|r| !r.2).map(|r| r.1).collect();
    let total: BTreeSet<u32> = rows.iter().map(|r| r.1).collect();
    let s = a.summary();
    (
        s.dxcc_confirmed,
        s.dxcc_confirmed_total,
        current.len(),
        total.len(),
    )
}

#[test]
fn control_the_count_matches_the_numbers_when_cty_dat_agrees_with_them() {
    for (call, _, _, entity) in AGREE {
        assert_eq!(
            dxcc::resolve(call).map(|i| i.entity),
            Some(*entity),
            "{call}"
        );
    }
    for by_number in [true, false] {
        let (nexus, nexus_total, current, total) = fold(AGREE, by_number);
        println!("control (by number: {by_number}): Nexus {nexus}/{nexus_total}, by DXCC number {current}/{total}");
        assert_eq!((nexus, nexus_total), (current, total));
    }
}

#[test]
fn the_confirmed_count_equals_the_count_by_dxcc_number() {
    let (nexus, nexus_total, current, total) = fold(DISAGREE, true);
    println!("Nexus {nexus} current / {nexus_total} total, by DXCC number {current} / {total}");
    assert_eq!(
        nexus, current,
        "confirmed DXCC (current) by the records' own numbers"
    );
    assert_eq!(nexus, 7);
    assert_eq!(
        nexus_total, total,
        "confirmed DXCC (total, deleted included) by the records' own numbers"
    );
    assert_eq!(nexus_total, 8);
}

/// The DX4WIN import's three, as reported — "K1B – Baker & Howland Islands (2002), K1N – Navassa
/// Island (2015), K3J – Johnston Island (2001)" — each with the number DX4WIN wrote for it. The
/// numbers are cty.csv's and TrustedQSL's (KH1 → 20, KP1 → 182, KH3 → 123).
#[test]
fn three_dxpeditions_on_us_calls_count_as_the_entities_they_operated_from() {
    let rows = [
        ("K1B", 20, "Baker & Howland Islands"),
        ("K1N", 182, "Navassa Island"),
        ("K3J", 123, "Johnston Island"),
    ];
    for (call, code, entity) in rows {
        assert_eq!(
            dxcc::resolve(call).map(|i| i.entity),
            Some("United States"),
            "premise: cty.dat files {call} under the United States"
        );
        assert_eq!(dxcc::entity_by_dxcc(code).map(|e| e.name), Some(entity));
    }

    // Worked, not yet confirmed: the "new one" chase names each entity, never the United States.
    let mut a = Awards::new();
    for (call, code, _) in rows {
        add(&mut a, call, false, None, dxcc_of(code));
    }
    let s = a.summary();
    let chased: BTreeSet<&str> = s.needed.iter().map(|n| n.entity.as_str()).collect();
    assert_eq!(
        chased,
        BTreeSet::from([
            "Baker & Howland Islands",
            "Johnston Island",
            "Navassa Island"
        ])
    );
    assert_eq!(s.dxcc_worked, 3);

    // Confirmed, and a United States contact beside them: four entities, not one.
    let mut a = Awards::new();
    for (call, code, _) in rows {
        add(&mut a, call, true, None, dxcc_of(code));
    }
    add(&mut a, "W1AW", true, None, RecordFields::default());
    assert_eq!(a.summary().dxcc_confirmed, 4);
    assert_eq!(a.summary().honor_roll.confirmed, 4);
}

/// The first report's four: Navassa, Desecheo, Midway, Palmyra & Jarvis, each confirmed by LoTW
/// on a US call that cty.dat reads as the United States — with LoTW's number on the record.
#[test]
fn the_first_reports_four_entities_count_by_lotws_number() {
    let rows = [("K1N", 182), ("K5D", 43), ("K4M", 174), ("K5P", 197)];
    let mut before = Awards::new();
    let mut after = Awards::new();
    for (call, code) in rows {
        assert_eq!(dxcc::resolve(call).map(|i| i.entity), Some("United States"));
        add(&mut before, call, true, None, RecordFields::default());
        add(&mut after, call, true, None, dxcc_of(code));
    }
    add(&mut before, "W1AW", true, None, RecordFields::default());
    add(&mut after, "W1AW", true, None, RecordFields::default());
    assert_eq!(
        before.summary().dxcc_confirmed,
        1,
        "control: by call alone all five are the United States"
    );
    assert_eq!(after.summary().dxcc_confirmed, 5);
}

/// A deleted entity counts in the total and never in the current count, Honor Roll or the
/// Challenge — and cty.dat alone would have made it a current one (Y2 → Fed. Rep. of Germany).
#[test]
fn a_deleted_entity_counts_in_the_total_and_not_in_current() {
    assert_eq!(
        dxcc::resolve("Y24UK").map(|i| i.entity),
        Some("Fed. Rep. of Germany"),
        "premise: cty.dat reads the call as a current entity"
    );
    let mut a = Awards::new();
    add(&mut a, "W1AW", true, None, RecordFields::default());
    add(&mut a, "Y24UK", true, None, dxcc_of(229));
    let s: AwardSummary = a.summary();
    assert_eq!(
        (s.dxcc_worked, s.dxcc_confirmed),
        (1, 1),
        "current: the United States"
    );
    assert_eq!(
        (s.dxcc_worked_total, s.dxcc_confirmed_total),
        (2, 2),
        "total: and the German Democratic Republic"
    );
    assert_eq!(s.honor_roll.confirmed, 1);
    assert_eq!(
        s.slots_confirmed, 1,
        "the Challenge counts current entities"
    );
    assert!(
        s.needed.is_empty() && s.slot_needed.is_empty(),
        "nothing to chase"
    );

    // Control: the same call with no number is the current entity cty.dat names.
    let mut b = Awards::new();
    add(&mut b, "Y24UK", true, None, RecordFields::default());
    let s = b.summary();
    assert_eq!((s.dxcc_confirmed, s.dxcc_confirmed_total), (1, 1));
}

/// The control the whole fix rests on: a record that carries no number counts exactly as before,
/// by cty.dat — and so does one whose number names nothing the table knows.
#[test]
fn a_record_without_a_known_number_counts_by_its_call() {
    for rec in [RecordFields::default(), dxcc_of(73), dxcc_of(523)] {
        let mut a = Awards::new();
        a.add_qso(
            "JA1XYZ", "20m", "FT8", false, false, false, None, None, None, false, rec,
        );
        let s = a.summary();
        assert_eq!(s.dxcc_worked, 1, "{rec:?}");
        assert_eq!(
            s.needed
                .iter()
                .map(|n| n.entity.as_str())
                .collect::<Vec<_>>(),
            ["Japan"],
            "{rec:?}"
        );
    }
}

/// ADIF's DXCC 0 says the station was in no DXCC entity (a ship at sea): no entity, whatever the
/// call's prefix — while the contact still counts as a contact and for its zone.
#[test]
fn dxcc_zero_earns_no_entity() {
    let mut a = Awards::new();
    add(&mut a, "W1AW/MM", true, None, dxcc_of(dxcc::NO_DXCC_ENTITY));
    let s = a.summary();
    assert_eq!(s.qsos, 1);
    assert_eq!((s.dxcc_worked, s.dxcc_worked_total), (0, 0));
    assert_eq!(s.waz_worked, 1, "its zone still counts");

    // Control: without the number, cty.dat keeps the home entity.
    let mut b = Awards::new();
    add(&mut b, "W1AW/MM", true, None, RecordFields::default());
    assert_eq!(b.summary().dxcc_worked, 1);
}

/// LoTW's DXCC wins over the log's for counting; the log's stays as entered; the difference is
/// listed, with both entities named.
#[test]
fn lotws_dxcc_wins_and_the_disagreement_is_listed() {
    let mut a = Awards::new();
    a.add_qso(
        "K1N",
        "20m",
        "CW",
        true,
        false,
        false,
        None,
        None,
        None,
        false,
        RecordFields {
            dxcc: Some(291),
            lotw_dxcc: Some(182),
            cqz: None,
            when_unix: 1_423_571_640,
        },
    );
    // Agreeing numbers, and a LoTW number with no log number beside it, list nothing.
    a.add_qso(
        "K5D",
        "20m",
        "CW",
        true,
        false,
        false,
        None,
        None,
        None,
        false,
        RecordFields {
            dxcc: Some(43),
            lotw_dxcc: Some(43),
            ..RecordFields::default()
        },
    );
    a.add_qso(
        "K4M",
        "20m",
        "CW",
        true,
        false,
        false,
        None,
        None,
        None,
        false,
        RecordFields {
            lotw_dxcc: Some(174),
            ..RecordFields::default()
        },
    );
    // And a United States contact: K1N is never the second United States one.
    add(&mut a, "W1AW", true, None, RecordFields::default());
    let s = a.summary();
    assert_eq!(
        s.dxcc_confirmed, 4,
        "Navassa (LoTW's), Desecheo, Midway and the United States"
    );
    assert_eq!(
        s.dxcc_disagreements,
        vec![propagation::DxccDisagreement {
            call: "K1N".into(),
            when_unix: 1_423_571_640,
            band: "20m".into(),
            mode: "CW".into(),
            logged_dxcc: 291,
            logged_entity: Some("United States".into()),
            lotw_dxcc: 182,
            lotw_entity: Some("Navassa Island".into()),
        }]
    );
}

/// WAS: a KP4 licensee signing LoTW from Florida (DXCC 291, STATE FL) — ARRL credits Florida.
/// The gate is the record's DXCC; the call's entity decides only where the record has none.
#[test]
fn was_credits_a_state_the_record_places_in_the_united_states() {
    let mut control = Awards::new();
    add(
        &mut control,
        "K4ABC",
        true,
        Some("FL"),
        RecordFields::default(),
    );
    assert_eq!(control.summary().was.confirmed, 1, "control");

    println!(
        "KP4ABC: record DXCC 291 STATE FL; cty.dat -> {:?}",
        dxcc::resolve("KP4ABC").map(|i| i.entity)
    );
    let mut a = Awards::new();
    add(&mut a, "KP4ABC", true, Some("FL"), dxcc_of(291));
    assert_eq!(
        a.summary().was.confirmed,
        1,
        "the record says DXCC 291, STATE FL"
    );

    // …and by the call alone, Puerto Rico has no state.
    let mut b = Awards::new();
    add(&mut b, "KP4ABC", true, Some("FL"), RecordFields::default());
    assert_eq!(b.summary().was.confirmed, 0);

    // The record's number gates the other way too: a US call whose record says Puerto Rico.
    let mut c = Awards::new();
    add(&mut c, "K4ABC", true, Some("FL"), dxcc_of(202));
    assert_eq!(c.summary().was.confirmed, 0, "DXCC 202 is no WAS state");

    // Alaska and Hawaii by number, and a deleted entity never.
    let mut d = Awards::new();
    add(&mut d, "W1XYZ", true, Some("AK"), dxcc_of(6));
    add(&mut d, "W2XYZ", true, Some("HI"), dxcc_of(110));
    add(&mut d, "W3XYZ", true, Some("NY"), dxcc_of(229));
    let s = d.summary();
    assert_eq!(s.was.confirmed, 2);
    assert!(s.was.needed.contains(&"NY".to_string()));
}

/// WAZ: a W1 licensee living in California is in CQ zone 3, and LoTW writes CQZ 3; cty.dat's W1
/// prefix says zone 5.
#[test]
fn waz_counts_the_zone_the_record_carries() {
    // Control: where cty.dat's zone agrees with the record's, the counts agree.
    let mut c = Awards::new();
    for (call, cqz) in [("K6ABC", 3u8), ("W1XYZ", 5u8)] {
        assert_eq!(
            dxcc::resolve(call).map(|i| i.cq_zone),
            Some(cqz),
            "control {call}"
        );
        add(&mut c, call, true, None, RecordFields::default());
    }
    assert_eq!(c.summary().waz_confirmed, 2, "control");

    // (call, record CQZ)
    let rows = [("K1ABC", 3u8), ("W1XYZ", 5u8)];
    let mut a = Awards::new();
    for (call, cqz) in rows {
        println!(
            "{call}: record CQZ {cqz}; cty.dat zone {:?}",
            dxcc::resolve(call).map(|i| i.cq_zone)
        );
        add(
            &mut a,
            call,
            true,
            None,
            RecordFields {
                cqz: Some(cqz),
                ..RecordFields::default()
            },
        );
    }
    let by_record: BTreeSet<u8> = rows.iter().map(|r| r.1).collect();
    println!(
        "WAZ: Nexus {}, by record CQZ {}",
        a.summary().waz_confirmed,
        by_record.len()
    );
    assert_eq!(a.summary().waz_confirmed, by_record.len());
}

/// A CQZ that is no zone (0, 41) is not a zone: the call's zone counts instead. And a record's
/// zone counts even where cty.dat cannot place the call at all.
#[test]
fn a_cqz_out_of_range_falls_back_and_a_known_zone_needs_no_call() {
    for cqz in [0u8, 41] {
        let mut a = Awards::new();
        add(
            &mut a,
            "W1XYZ",
            true,
            None,
            RecordFields {
                cqz: Some(cqz),
                ..RecordFields::default()
            },
        );
        let s = a.summary();
        assert_eq!(s.waz_confirmed, 1, "CQZ {cqz}");
        // Zone 5 is the one W1 resolves to, so a second W1 contact adds nothing.
        let mut b = a.clone();
        add(&mut b, "W1ABC", true, None, RecordFields::default());
        assert_eq!(
            b.summary().waz_confirmed,
            1,
            "CQZ {cqz} fell back to zone 5"
        );
    }
    assert_eq!(dxcc::resolve("Q0ZZZ"), None, "premise: no entity");
    let mut a = Awards::new();
    add(
        &mut a,
        "Q0ZZZ",
        true,
        None,
        RecordFields {
            cqz: Some(17),
            ..RecordFields::default()
        },
    );
    assert_eq!(a.summary().waz_confirmed, 1);
}
