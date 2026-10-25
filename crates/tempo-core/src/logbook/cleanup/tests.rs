//! The clean-up tools' rules, on records: which contacts are one logged twice, that a merge loses
//! no field any copy held, and what a bulk edit and a fix change — and leave alone.

use super::*;
use crate::logbook::{
    adif_record_own_log, parse_adif, ContestFields, QslRcvd, QslVia, UploadOutcome, UploadStatus,
};

const T0: u64 = 1_788_000_000;

fn contact(call: &str, when: u64, seq: u64) -> QsoRecord {
    let mut r = parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>").remove(0);
    r.call = call.to_string();
    r.when_unix = when;
    r.freq_mhz = 0.0;
    r.id = Some(format!("~{seq:016x}").parse().expect("an id"));
    r
}

fn key(r: &QsoRecord) -> DupeKey {
    DupeKey {
        id: r.id.unwrap(),
        call: r.call.clone(),
        band: r.band.clone(),
        mode: r.mode.clone(),
        when_unix: r.when_unix,
    }
}

fn stamp(when_unix: i64) -> Option<UploadStatus> {
    Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix,
        detail: None,
    })
}

/// The log's own rule groups what its guard would have refused — 300 s in, 301 s out, a portable
/// and a lower-case spelling of one station, case aside on band and mode — and nothing across a
/// band, a mode or another station. Three copies a few minutes apart chain into one group; the
/// possible tier's hour takes in a pair the rule leaves apart.
#[test]
fn the_duplicate_groups_are_the_logs_own_rule_and_the_possible_tier_is_wider() {
    let mut rows = vec![
        contact("W1AW", T0, 1),
        contact("w1aw/p", T0 + 300, 2),
        contact("W1AW", T0 + 600, 3),
        contact("W1AW", T0 + 901, 4),
        contact("K1ABC", T0, 5),
        contact("K1ABC", T0 + 10, 6),
        contact("K1ABC", T0 + 20, 7),
        contact("N0XYZ", T0, 8),
        contact("N0XYZ", T0 + 2_000, 9),
    ];
    rows[5].band = "40m".into();
    rows[6].mode = "FT4".into();
    let ids = |rs: &[usize]| rs.iter().map(|&i| rows[i].id.unwrap()).collect::<Vec<_>>();

    let rule = dupe_groups(
        rows.iter().map(key).collect(),
        crate::logbook::dedup::DEDUP_WINDOW_SECS,
    );
    assert_eq!(rule, vec![ids(&[0, 1, 2])], "the rule's groups");

    let possible = dupe_groups(rows.iter().map(key).collect(), POSSIBLE_WINDOW_SECS);
    assert_eq!(
        possible,
        vec![ids(&[7, 8]), ids(&[0, 1, 2, 3])],
        "newest first"
    );

    // Each pair the rule groups is one the guard calls a duplicate, and the 301 s one is not.
    use crate::logbook::dedup::is_recent_duplicate;
    assert!(is_recent_duplicate(&rows[0], &rows[1]));
    assert!(!is_recent_duplicate(&rows[2], &rows[3]));
}

/// Two copies of one contact, holding between them every kind of field there is, and agreeing
/// wherever both hold one.
fn two_copies() -> (QsoRecord, QsoRecord) {
    let mut a = contact("W1AW", T0, 1);
    a.grid = Some("FN31pr".into());
    a.rst_sent = Some("-10".into());
    a.qsl_rcvd = QslRcvd {
        lotw: true,
        ..QslRcvd::default()
    };
    a.confirmed = true;
    a.award_confirmed = true;
    a.credit_granted = vec!["DXCC".into()];
    a.upload.lotw = stamp(100);
    a.dxcc = Some(291);
    a.country = Some("United States".into());
    a.extra = vec![("CQZ".into(), "5".into())];
    a.qsl_sent = crate::logbook::QslSent {
        sent: true,
        via: Some(QslVia::Bureau),
        date_unix: Some(T0 - T0 % 86_400),
        cleared_unix: None,
    };

    let mut b = contact("W1AW", T0 + 60, 2);
    b.grid = Some("FN31PR".into());
    b.name = Some("Hiram".into());
    b.qth = Some("Newington".into());
    b.state = Some("CT".into());
    b.rst_rcvd = Some("-07".into());
    b.notes = Some("first QSO on the new antenna".into());
    b.comment = Some("tnx".into());
    b.tx_power = Some(50.0);
    b.freq_mhz = 14.074_5;
    b.time_off_unix = Some(T0 + 120);
    b.prop_mode = Some("F2".into());
    b.operator = Some("K2DEF".into());
    b.station_callsign = Some("K2DEF".into());
    b.my_grid = Some("FN20".into());
    b.my_rig = Some("IC-7300".into());
    b.ota.their_program = Some("POTA".into());
    b.ota.their_ref = Some("US-1234".into());
    b.ota.my_program = Some("SOTA".into());
    b.ota.my_ref = Some("W2/GC-001".into());
    b.ota.iota = Some("NA-001".into());
    b.qsl_rcvd = QslRcvd {
        eqsl: true,
        card: true,
        ..QslRcvd::default()
    };
    b.confirmed = true;
    b.award_confirmed = true;
    b.credit_submitted = vec!["WAS".into()];
    b.upload.qrz = stamp(200);
    b.upload.eqsl = stamp(201);
    b.upload.clublog = stamp(202);
    b.extra = vec![
        ("CQZ".into(), "5".into()),
        ("ITUZ".into(), "8".into()),
        ("SUBMODE".into(), "".into()),
    ];
    b.contest = Some(Box::new(ContestFields {
        contest_id: "ARRL-FIELD-DAY".into(),
        ..ContestFields::default()
    }));
    (a, b)
}

/// The ADIF tags of a record as the operator's own log writes it, with their values.
fn tags(r: &QsoRecord) -> Vec<(String, String)> {
    let text = adif_record_own_log(r);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let head = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let mut it = head.split(':');
        let name = it.next().unwrap_or("").to_ascii_uppercase();
        let Some(len) = it.next().and_then(|l| l.parse::<usize>().ok()) else {
            continue;
        };
        out.push((name, rest[..len].to_string()));
        rest = &rest[len..];
    }
    out
}

/// ★ NO FIELD LOST, at the record. Each copy holds what the other lacks; the merge keeps the
/// first and the result is asserted WHOLE against the record built by hand: every field either
/// copy held is in it, confirmations and credit unioned, each upload stamp kept. Then, as a
/// second witness over every field the log writes, each tag either copy carries is in the
/// merged contact's own-log record with its value — case aside, as a grid is compared — the
/// second copy's time and id aside, the kept contact's own.
#[test]
fn a_merge_keeps_every_field_either_copy_held() {
    let (a, b) = two_copies();
    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);

    let mut want = a.clone();
    want.name = b.name.clone();
    want.qth = b.qth.clone();
    want.state = b.state.clone();
    want.rst_rcvd = b.rst_rcvd.clone();
    want.notes = b.notes.clone();
    want.comment = b.comment.clone();
    want.tx_power = b.tx_power;
    want.freq_mhz = b.freq_mhz;
    want.time_off_unix = b.time_off_unix;
    want.prop_mode = b.prop_mode.clone();
    want.operator = b.operator.clone();
    want.station_callsign = b.station_callsign.clone();
    want.my_grid = b.my_grid.clone();
    want.my_rig = b.my_rig.clone();
    want.ota = b.ota.clone();
    want.qsl_rcvd = QslRcvd {
        lotw: true,
        eqsl: true,
        card: true,
        qrz: false,
    };
    want.credit_submitted = vec!["WAS".into()];
    want.upload.qrz = stamp(200);
    want.upload.eqsl = stamp(201);
    want.upload.clublog = stamp(202);
    want.extra = vec![
        ("CQZ".into(), "5".into()),
        ("ITUZ".into(), "8".into()),
        ("SUBMODE".into(), "".into()),
    ];
    want.contest = b.contest.clone();
    assert_eq!(merged.record, want);

    let held = tags(&merged.record);
    for copy in [&a, &b] {
        for (tag, value) in tags(copy) {
            let kept_identity =
                copy.id != a.id && matches!(tag.as_str(), "APP_NEXUS_ID" | "QSO_DATE" | "TIME_ON");
            if kept_identity {
                continue;
            }
            assert!(
                held.iter()
                    .any(|(t, v)| *t == tag && v.eq_ignore_ascii_case(&value)),
                "{tag}={value} from {:?} is not in the merged contact",
                copy.id
            );
        }
    }
}

/// A field the copies disagree on is a conflict, the kept copy's value standing unless the caller
/// chose another; the one a confirmed copy holds, the QSL-sent mark and the contest exchange are
/// guarded, and a guarded conflict left unchosen is named.
#[test]
fn a_disagreement_is_a_conflict_and_a_confirmed_copys_is_guarded() {
    let (mut a, mut b) = two_copies();
    b.grid = Some("FN42".into());
    b.qsl_sent = crate::logbook::QslSent {
        sent: true,
        via: Some(QslVia::Direct),
        date_unix: None,
        cleared_unix: None,
    };
    a.contest = Some(Box::new(ContestFields {
        contest_id: "CQ-WW-CW".into(),
        ..ContestFields::default()
    }));
    a.qsl_rcvd = QslRcvd::default();
    a.confirmed = false;
    a.award_confirmed = false;
    b.qsl_rcvd = QslRcvd::default();
    b.confirmed = false;
    b.award_confirmed = false;
    a.comment = Some("one".into());
    b.comment = Some("two".into());

    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    let by_tag = |tag: &str| merged.conflicts.iter().find(|c| c.tag == tag).unwrap();
    assert_eq!(
        merged.record.comment.as_deref(),
        Some("one"),
        "the kept copy's"
    );
    assert!(!by_tag("COMMENT").guarded);
    assert!(!by_tag("GRIDSQUARE").guarded, "no copy is confirmed");
    assert!(by_tag("QSL_SENT").guarded && by_tag("CONTEST").guarded);
    let mut unchosen = merged.unchosen(&HashMap::new());
    unchosen.sort();
    assert_eq!(unchosen, vec!["CONTEST", "QSL_SENT"]);

    let choices: HashMap<String, RecordId> = [
        ("COMMENT".to_string(), b.id.unwrap()),
        ("CONTEST".to_string(), b.id.unwrap()),
        ("QSL_SENT".to_string(), a.id.unwrap()),
    ]
    .into();
    let chosen = merge(&[a.clone(), b.clone()], 0, &choices);
    assert!(chosen.unchosen(&choices).is_empty());
    assert_eq!(chosen.record.comment.as_deref(), Some("two"));
    assert_eq!(chosen.record.contest, b.contest);
    assert_eq!(chosen.record.qsl_sent, a.qsl_sent);

    // Confirmed, the copy's grid is a confirmation's detail: guarded.
    b.qsl_rcvd.lotw = true;
    let guarded = merge(&[a, b], 0, &HashMap::new());
    assert!(guarded
        .conflicts
        .iter()
        .any(|c| c.tag == "GRIDSQUARE" && c.guarded));
}

/// The fullest copy is the suggestion; of equally full ones, the first.
#[test]
fn the_suggested_copy_is_the_fullest() {
    let (a, b) = two_copies();
    assert_eq!(suggested_keep(&[a.clone(), b.clone()]), 1);
    let bare = contact("W1AW", T0, 3);
    assert_eq!(suggested_keep(&[bare.clone(), bare]), 0);
}

fn no_country(_: &str, _: Option<u32>) -> Option<String> {
    None
}

/// A bulk edit sets and clears what it names and nothing else — never a confirmation, never an
/// upload stamp — and takes the "Nexus filled this" mark off a grid it changes. A park goes only
/// on a side with no reference or one of its own programme.
#[test]
fn a_bulk_edit_changes_only_its_fields_and_keeps_every_stamp() {
    let (_, mut b) = two_copies();
    crate::logbook::mark_self_filled(&mut b, "MY_GRIDSQUARE");
    let changes = checked(&[
        Change {
            field: Field::MyGridsquare,
            value: Some("em10".into()),
        },
        Change {
            field: Field::Operator,
            value: None,
        },
        Change {
            field: Field::MyPotaRef,
            value: Some("us-0001".into()),
        },
    ])
    .unwrap();
    let after = changed(&b, &changes, &no_country).expect("a change");
    let mut want = b.clone();
    want.my_grid = Some("em10".into());
    want.operator = None;
    crate::logbook::set_extra(&mut want, SELF_FILLED_FIELD, None);
    assert_eq!(
        after, want,
        "the summit side is SOTA's: the park is not put over it"
    );
    assert_eq!(after.upload, b.upload);
    assert_eq!(after.qsl_rcvd, b.qsl_rcvd);

    b.ota.my_program = None;
    b.ota.my_ref = None;
    let parked = changed(&b, &changes, &no_country).unwrap();
    assert_eq!(parked.ota.my_program.as_deref(), Some("POTA"));
    assert_eq!(parked.ota.my_ref.as_deref(), Some("US-0001"));

    assert!(
        changed(&parked, &changes, &no_country).is_none(),
        "already so"
    );
    assert!(checked(&[Change {
        field: Field::MyGridsquare,
        value: Some("ZZ99".into())
    }])
    .is_err());
}

#[test]
fn the_formats_a_check_holds_references_to() {
    for g in ["FN", "FN31", "fn31pr", "FN31PR45"] {
        assert!(valid_grid(g), "{g}");
    }
    for g in ["", "F", "FN3", "SZ31", "FN31PZ", "FN31PR4", "FN31 "] {
        assert!(!valid_grid(g), "{g}");
    }
    assert!(valid_pota("US-1234") && valid_pota("K-0001,VE-12345"));
    assert!(!valid_pota("US1234") && !valid_pota("US-12") && !valid_pota("1234-0001"));
    assert!(valid_sota("W7W/LC-001") && valid_sota("G/LD-001"));
    assert!(!valid_sota("W7W/LC-1") && !valid_sota("W7WLC-001"));
    assert!(valid_iota("NA-001") && !valid_iota("XX-001") && !valid_iota("na-001"));
    assert_eq!(respelled("us1234", valid_pota).as_deref(), Some("US-1234"));
    assert_eq!(
        respelled(" us 1234 ", valid_pota).as_deref(),
        Some("US-1234")
    );
    assert_eq!(
        respelled("w7w/lc-001", valid_sota).as_deref(),
        Some("W7W/LC-001")
    );
    assert_eq!(respelled("park", valid_pota), None);
}
