//! Every place that decides which DXCC entity a LOGGED contact belongs to counts it as the awards
//! do ([`propagation::awards::counted_entity`]): by the DXCC number its record carries, LoTW's
//! first, else by cty.dat's reading of its call, a WAE area as the entity it belongs to (Sicily as
//! Italy), a deleted entity never as a current one. Worked All Provinces and the home entity
//! "First DX" is judged against follow it here.
//!
//! The report behind it: once the awards counted K1N (Navassa Island, 2015) by its record, the
//! Needed board still read the same contact from its call, as the United States, and went on
//! calling Navassa needed while the Awards view counted it confirmed.

use std::collections::BTreeSet;

use propagation::needalert::score_slots;
use propagation::stats::LogStatsAccumulator;
use propagation::{Awards, Band, LogNeeds, NeedKind, NeedTag, OperatorNeeds, RecordFields};

/// A record's DXCC number and nothing else.
fn dxcc_of(code: u32) -> RecordFields {
    RecordFields {
        dxcc: Some(code),
        ..RecordFields::default()
    }
}

/// One confirmed 20 m CW contact into `a`, as the app's award fold adds it.
fn add(a: &mut Awards, call: &str, state: Option<&str>, rec: RecordFields) {
    a.add_qso(
        call, "20m", "CW", true, false, false, state, None, None, false, rec,
    );
}

/// Worked All Provinces counts a province only for a contact in Canada — the entity the contact
/// counts toward, so its record's number decides where it has one.
#[test]
fn worked_all_provinces_counts_a_province_by_the_records_dxcc() {
    let provinces = |call: &str, rec: RecordFields| {
        let mut a = Awards::new();
        add(&mut a, call, Some("ON"), rec);
        a.summary().wap.worked
    };
    assert_eq!(
        [
            // A US call whose record says Canada (DXCC 1): Ontario counts.
            provinces("K1ABC", dxcc_of(1)),
            // A Canadian call whose record says the United States (291): it does not.
            provinces("VE3ABC", dxcc_of(291)),
            // Control: a Canadian call with no number counts by its call.
            provinces("VE3ABC", RecordFields::default()),
        ],
        [1, 0, 1]
    );
}

/// "First DX" counts an entity other than the operator's own, and the operator's own is judged
/// as a contact from the same call would count: a Sicilian call is Italy's, as the ARRL credits
/// it, so an Italian contact is not DX for it.
#[test]
fn first_dx_judges_the_home_entity_as_a_contact_from_it_counts() {
    let first_dx = |home: &str, call: &str| {
        let mut a = Awards::new();
        a.set_home_call(home);
        add(&mut a, call, None, RecordFields::default());
        a.summary()
            .achievements
            .iter()
            .find(|x| x.id == "dx-first")
            .expect("the First DX achievement")
            .current
    };
    assert_eq!(
        [
            // Italy, and Sicily, from Sicily: home.
            first_dx("IT9ABC", "I1ABC"),
            first_dx("IT9ABC", "IT9XYZ"),
            // Controls: Germany from Sicily is DX, and Italy from the United States.
            first_dx("IT9ABC", "DL1ABC"),
            first_dx("W1AW", "I1ABC"),
        ],
        [0, 0, 1, 1]
    );
}

// ── the Needed board's needs model ──────────────────────────────────────────────────────────────

/// One confirmed 20 m CW contact into the needs model, as the app's fold adds it.
fn need_add(n: &mut LogNeeds, call: &str, state: Option<&str>, rec: RecordFields) {
    n.add_qso(call, "20m", "CW", None, state, true, false, rec);
}

/// What the board says of a station heard on 20 m CW: the entity it names and its tags, or
/// nothing when it advances nothing.
fn heard(n: &LogNeeds, call: &str, state: Option<&str>) -> Option<(String, Vec<NeedTag>)> {
    score_slots(call, "20m", "CW", None, state, n, &n.slots()).map(|a| (a.entity, a.tags))
}

fn names(n: &LogNeeds) -> BTreeSet<&str> {
    n.worked_entity_names().iter().map(String::as_str).collect()
}

/// The record's number counts first, LoTW's over the log's: K1N in 2015 is Navassa, and K1B in
/// 2002 is Baker & Howland wherever LoTW says so, whatever the log or the call reads as today.
#[test]
fn the_needs_count_a_contact_by_its_records_dxcc_lotws_first() {
    let mut n = LogNeeds::new();
    need_add(&mut n, "K1N", None, dxcc_of(182));
    need_add(
        &mut n,
        "K1B",
        None,
        RecordFields {
            dxcc: Some(291),
            lotw_dxcc: Some(20),
            ..RecordFields::default()
        },
    );
    assert_eq!(
        names(&n),
        BTreeSet::from(["Baker & Howland Islands", "Navassa Island"])
    );
    assert_eq!(
        n.need("Navassa Island", Band::B20, "CW"),
        NeedKind::Satisfied
    );
    assert_eq!(n.need("United States", Band::B20, "CW"), NeedKind::Atno);
}

/// ADIF's DXCC 0 (a ship at sea) earns no entity slot; the zone its record carries still counts.
#[test]
fn dxcc_zero_fills_no_entity_slot_and_its_zone_still_counts() {
    let mut n = LogNeeds::new();
    need_add(
        &mut n,
        "W1AW/MM",
        None,
        RecordFields {
            dxcc: Some(propagation::dxcc::NO_DXCC_ENTITY),
            cqz: Some(8),
            ..RecordFields::default()
        },
    );
    assert!(n.worked_entity_names().is_empty());
    assert_eq!(
        n.worked_zones().iter().copied().collect::<Vec<_>>(),
        [(8, Band::B20)]
    );
}

/// A WAE area counts as the entity it belongs to, here as in the awards: a Sicilian contact is an
/// Italian one, and an Italian station heard is not needed. Heard, Sicily itself earns no DXCC tag,
/// as before.
#[test]
fn a_wae_contact_counts_toward_the_entity_it_belongs_to() {
    let mut n = LogNeeds::new();
    need_add(&mut n, "IT9ABC", None, RecordFields::default());
    assert_eq!(names(&n), BTreeSet::from(["Italy"]));
    assert_eq!(
        [heard(&n, "I1ABC", None), heard(&n, "IT9XYZ", None)],
        [None, None]
    );
}

/// A deleted entity is never a need, nor does its contact fill the slot of the current entity its
/// call reads as today: Y24UK was the German Democratic Republic, and the Federal Republic is
/// still a new one. Its zone counts, as WAZ counts it.
#[test]
fn a_deleted_entity_is_never_a_need_nor_its_calls_current_entity() {
    let mut n = LogNeeds::new();
    need_add(&mut n, "Y24UK", None, dxcc_of(229));
    assert!(n.worked_entity_names().is_empty());
    assert_eq!(
        heard(&n, "DL1ABC", None),
        Some(("Fed. Rep. of Germany".to_string(), vec![NeedTag::NewEntity]))
    );
}

/// The WAS gate is the record's entity where it carries one: a KP4 licensee in Florida whose
/// record says the United States (291) works Florida; the same call with no number is Puerto
/// Rico, and does not (the control).
#[test]
fn the_was_gate_is_the_records_entity() {
    let state_worked = |rec: RecordFields| {
        let mut n = LogNeeds::new();
        need_add(&mut n, "KP4ABC", Some("FL"), rec);
        n.worked_states().contains(&("FL".to_string(), Band::B20))
    };
    assert_eq!(
        [
            state_worked(dxcc_of(291)),
            state_worked(RecordFields::default())
        ],
        [true, false]
    );
}

/// The zone is the record's CQ zone where it carries one, else the call's: a W1 licensee in
/// California (zone 3), and the same call with no zone (5, the control).
#[test]
fn the_zone_is_the_records_cq_zone() {
    let zones = |rec: RecordFields| {
        let mut n = LogNeeds::new();
        need_add(&mut n, "K1ABC", None, rec);
        n.worked_zones().iter().copied().collect::<Vec<_>>()
    };
    let cqz_3 = RecordFields {
        cqz: Some(3),
        ..RecordFields::default()
    };
    assert_eq!(
        [zones(cqz_3), zones(RecordFields::default())],
        [vec![(3, Band::B20)], vec![(5, Band::B20)]]
    );
}

/// The control the change rests on: a record with no number, or a number the table does not
/// know, counts by its call exactly as before.
#[test]
fn a_record_without_a_known_number_counts_by_its_call() {
    for rec in [RecordFields::default(), dxcc_of(73), dxcc_of(523)] {
        let mut n = LogNeeds::new();
        need_add(&mut n, "JA1XYZ", None, rec);
        assert_eq!(names(&n), BTreeSet::from(["Japan"]), "{rec:?}");
    }
}

/// A station heard on the air has no record, so it is placed by its call alone: today's K1N is
/// the United States (cty.dat files it there), never Navassa — a new one on a log whose only
/// contact with that call was the 2015 Navassa operation. A Navassa station is not needed.
#[test]
fn a_station_heard_is_placed_by_its_call_alone() {
    let mut n = LogNeeds::new();
    need_add(
        &mut n,
        "K1N",
        None,
        RecordFields {
            dxcc: Some(182),
            cqz: Some(8),
            ..RecordFields::default()
        },
    );
    assert_eq!(
        [heard(&n, "K1N", None), heard(&n, "KP1AA", None)],
        [
            Some((
                "United States".to_string(),
                vec![NeedTag::NewEntity, NeedTag::NewZone]
            )),
            None
        ]
    );
}

// ── the log statistics' geography ───────────────────────────────────────────────────────────────

/// The geography places each contact where the awards count it: Navassa by K1N's record (zone 8),
/// Italy by a Sicilian call, the German Democratic Republic by Y24UK's record, nothing for a ship
/// at sea; DX or domestic against the operator's own entity.
#[test]
fn the_geography_places_each_contact_as_the_awards_count_it() {
    let mut g = LogStatsAccumulator::new("KD9TAW");
    g.add(
        "K1N",
        RecordFields {
            dxcc: Some(182),
            cqz: Some(8),
            ..RecordFields::default()
        },
    );
    g.add("IT9ABC", RecordFields::default());
    g.add("Y24UK", dxcc_of(229));
    g.add("W1AW", RecordFields::default());
    g.add("W1AW/MM", dxcc_of(propagation::dxcc::NO_DXCC_ENTITY));
    let s = g.summary();
    assert_eq!(
        (s.total, s.resolved, s.domestic, s.dx),
        (5, 4, 1, 3),
        "Navassa, Italy and the GDR are DX; the ship at sea is nowhere"
    );
    let continents: Vec<(&str, usize, usize)> = s
        .by_continent
        .iter()
        .map(|c| (c.continent.as_str(), c.qsos, c.entities))
        .collect();
    assert_eq!(continents, [("NA", 2, 2), ("EU", 2, 2)]);
    let zones: Vec<(u8, usize)> = s.by_zone.iter().map(|z| (z.zone, z.qsos)).collect();
    assert_eq!(zones, [(5, 1), (8, 1), (14, 1), (15, 1)]);
}

/// Home is the entity a contact from the operator's own call counts toward: from Sicily, an
/// Italian contact is domestic.
#[test]
fn the_geographys_home_is_the_entity_a_contact_from_it_counts_toward() {
    let mut g = LogStatsAccumulator::new("IT9ABC");
    g.add("I1ABC", RecordFields::default());
    g.add("DL1ABC", RecordFields::default());
    let s = g.summary();
    assert_eq!((s.domestic, s.dx), (1, 1));
}
