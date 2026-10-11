//! ⭐ **A duplicate contact in a CROSS-CHECKED contest is LOGGED, MARKED and scored
//! ZERO — and Field Day keeps refusing one.**
//!
//! The sponsors say the same thing twice and mean it literally. ARRL LGCK.1 and CQ
//! XII.E.1: *"Duplicate contacts are removed with no additional penalty."* CQ's own
//! entrant FAQ is an instruction, not a permission: *"Yes! Please log all contacts that
//! you make even if they are duplicates"* and *"No, please do not remove any QSOs from
//! your log! This will cause the other station that worked you to lose credit for the
//! contact."* Dropping the row is what costs points — the other station's NIL is 1×
//! extra at ARRL, 2× extra at CQ — so the log carries the dupe and the checker zeroes it.
//!
//! ⚠️ **ARRL FIELD DAY IS NOT ONE OF THESE CONTESTS.** It has no log checking at all —
//! no cross-check, no NIL, and *"Complete station logs are NOT required for submission,
//! and ARRL does not use the logs."* The NIL justification has zero force there, Field
//! Day's deliverable is a dupe sheet plus **raw non-dupe** QSO counts, and so Field Day
//! keeps refusing the contact exactly as it always has. That split is the whole point of
//! this file, and `field_day_still_refuses_a_dupe` is the half that must never move.
//!
//! ⭐ **Except a dupe worked ON THE AIR.** A double-click on a CQ (or a logger's Reply, the
//! same pick from another program) answers a station the log already holds, and the
//! contact is made: the other station logs it. That contact is written in every contest,
//! Field Day included, marked and worth nothing, and the Cabrillo keeps it (the operator's
//! ruling, 2026-10-11: "the way N1MM does"). A TYPED dupe is still refused, and S&P's own
//! pounce still passes a worked station by. The last section of this file.
//!
//! ⚠️ **"Marked" means marked in OUR data model**, for scoring and display. Neither
//! sponsor asks for a marker, and CQ's worked example logs the dupe as a plain `QSO:`
//! line; `X-QSO` means "I want this EXCLUDED" and carries CQ's anti-abuse warning. So
//! `a_logged_dupe_is_a_plain_qso_line_in_cabrillo` pins that the mark stays inside.
//!
//! ⚠️ **The country file here is a STUB** — the real resolver lives in a crate that
//! depends on this one. Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog, FieldDayStation, FtMessages};

/// The stub country file: prefix → (entity, continent), longest prefix wins. Enough
/// entities that CQ WW's `ByRelation` point table prices these contacts at something
/// other than zero — a score of 0 either way would make "the dupe added nothing"
/// unfalsifiable.
const TABLE: &[(&str, &str, &str)] = &[
    ("JA", "Japan", "AS"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("VE", "Canada", "NA"),
    ("W", "United States", "NA"),
    ("K", "United States", "NA"),
];

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let mut best: Option<(usize, &'static str, &'static str)> = None;
    for (p, entity, cont) in TABLE {
        if up.starts_with(p) && best.is_none_or(|(n, _, _)| p.len() > n) {
            best = Some((p.len(), entity, cont));
        }
    }
    best.map(|(_, entity, continent)| CallLocation {
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

/// 2026-11-28 00:00:00Z — inside CQ WW CW's weekend. Only ordering matters here.
const START: u64 = 1_795_996_800;

/// A CQ WW CW log for a US station in zone 4 — one of the contests the sponsor
/// cross-checks, and therefore one that logs its dupes.
fn cqww_log() -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id("cqww_cw", CURRENT_RULES_YEAR).expect("cqww_cw is a shipped ruleset");
    let st = StationData {
        mycall: "W9XYZ".to_string(),
        contest_cq_zone: "4".to_string(),
        contest_qth_state: "IL".to_string(),
        contest_category_power: "LOW".to_string(),
        contest_category_assisted: "NON-ASSISTED".to_string(),
        ..Default::default()
    };
    let session = ContestSession::for_ruleset(rs, &st)
        .unwrap_or_else(|e| panic!("cqww_cw session refused: {e}"));
    FieldDayLog::new("W9XYZ", session, "20m")
}

/// An ARRL Field Day log — the contest with no log checking, which keeps refusing.
fn fd_log() -> FieldDayLog {
    resolver();
    FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
        "20m",
    )
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Work one CQ WW CW contact. `min` minutes in, so rows order deterministically.
fn work(log: &mut FieldDayLog, band: &str, call: &str, zone: &str, min: u64) -> bool {
    log.band = band.to_string();
    log.log_fields_at(
        call,
        &fields(&[("RST", "599"), ("ZN", zone)]),
        "CW",
        "",
        0,
        START + min * 60,
    )
}

/// `(qso points, powered, multipliers, total)` for a log, by its own ruleset.
fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

// ---------------------------------------------------------------------------
// The cross-checked contests: logged, marked, worth nothing
// ---------------------------------------------------------------------------

/// ⭐ **THE CHANGE.** CQ XII.E.1 + the entrant FAQ: the duplicate is written to the log
/// and the checker removes it. It must reach the log, say of itself that it is a dupe,
/// and move no number.
#[test]
fn a_cross_checked_dupe_is_logged_marked_and_scores_zero() {
    let mut log = cqww_log();
    assert!(
        work(&mut log, "20m", "JA1ABC", "25", 0),
        "the first contact"
    );
    let before = score(&log);

    // The same station, same band, same mode — a duplicate by CQ WW's own rule.
    assert!(
        work(&mut log, "20m", "JA1ABC", "25", 30),
        "a dupe in a cross-checked contest is ACCEPTED, not refused: dropping it costs \
         the other station a NIL at 2x the contact"
    );
    assert_eq!(log.qsos().len(), 2, "the duplicate row is in the log");
    assert!(log.qsos()[1].dupe, "…and it is MARKED as a duplicate");
    assert!(!log.qsos()[0].dupe, "…while the original is not");

    assert_eq!(
        score(&log),
        before,
        "XII.E.1 'duplicate contacts are removed': the dupe is worth zero points and \
         earns no multiplier"
    );
}

/// ⚠️ **A dupe earns no MULTIPLIER — and the case that proves it is a MIS-COPIED one.**
///
/// A dupe that repeats the exchange exactly cannot move a multiplier count whatever the
/// code does, because a multiplier is a set of distinct values and the original already
/// contributed this one; a test built on that shape passes no matter what and would say
/// nothing. So the second copy carries a DIFFERENT zone, which is what actually happens
/// when an operator re-works a station and mis-copies it. CQ WW keys a dupe on call and
/// band alone, so it is still a duplicate — and if it reached the multiplier board it
/// would claim a zone the station never worked.
#[test]
fn a_mis_copied_dupe_claims_no_multiplier_it_never_worked() {
    let mut log = cqww_log();
    let mults = |l: &FieldDayLog| l.ruleset().scoring.mult_counts(l.score_rows());
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    let before = mults(&log);

    // The same station, same band — a dupe — but copied as zone 26 this time.
    assert!(work(&mut log, "20m", "JA1ABC", "26", 30));
    assert!(log.qsos()[1].dupe, "call + band is CQ WW's whole dupe key");
    assert_eq!(
        mults(&log),
        before,
        "a zone that only a duplicate carried was never worked, and must not be claimed"
    );
    assert_eq!(
        log.worked_values("ZN"),
        vec!["25".to_string()],
        "…and the multiplier board does not colour it in either"
    );
}

/// ⚠️ **SCORING-GRADE.** Field Day's deliverable is a dupe sheet plus **raw non-dupe**
/// QSO counts, and `qso_count` is what every summary surface reads. A dupe entering the
/// log must not enter the count, or the submitted summary sheet overstates the score.
#[test]
fn the_count_excludes_a_logged_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "DL1ABC", "14", 10));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");

    assert_eq!(log.qsos().len(), 3, "three rows are in the log");
    assert_eq!(
        log.qso_count(),
        2,
        "…but the RAW NON-DUPE count is two — the number a summary sheet claims"
    );
}

/// The per-mode points sum is the other number a summary sheet reads, and it walks the
/// rows independently of `score_rows`. A dupe must be invisible to it too.
#[test]
fn the_points_sum_excludes_a_logged_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    let before = log.qso_points();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30));
    assert_eq!(log.qso_points(), before, "a dupe adds no QSO points");
}

/// ⚠️ **The mark must survive a restart**, and it is RE-DERIVED from the dupe index on
/// the way back in rather than carried as a tag — the same rule `restore_row` already
/// applies to the serial run. A journal reload that un-marked the row would restore a
/// log that scores higher than the one that was written.
#[test]
fn a_logged_dupe_survives_the_journal_round_trip_still_marked() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "DL1ABC", "14", 10));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");
    let journal = log.adif();

    let mut restored = cqww_log();
    restored.merge_adif(&journal, 0);

    assert_eq!(
        restored.qsos().len(),
        3,
        "every row came back, dupe included"
    );
    assert_eq!(restored.qso_count(), 2, "…and the count still excludes it");
    assert!(restored.qsos()[2].dupe, "…and it came back MARKED");
    assert_eq!(
        score(&restored),
        score(&log),
        "a restart rescores identically"
    );
}

/// ⚠️ **The mark stays INSIDE.** CQ's worked example logs a dupe as a plain `QSO:` line
/// and lets the checker zero it; `X-QSO` means "I want this EXCLUDED" and carries CQ's
/// own anti-abuse warning. Exporting our marker would be a different claim to the
/// sponsor than the one the operator made.
#[test]
fn a_logged_dupe_is_a_plain_qso_line_in_cabrillo() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");
    let file = log.cabrillo(14_030).expect("a one-mode log exports");

    assert_eq!(
        file.lines().filter(|l| l.starts_with("QSO:")).count(),
        2,
        "both contacts are plain QSO: lines"
    );
    assert!(
        !file.contains("X-QSO"),
        "never X-QSO: that asks the sponsor to EXCLUDE the contact"
    );
}

// ---------------------------------------------------------------------------
// Field Day: unchanged, because nothing checks its log
// ---------------------------------------------------------------------------

/// ⚠️ **THE HALF THAT MUST NOT MOVE.** ARRL Field Day has no log checking — no
/// cross-check, no NIL, and the logs are not used — so the reason to keep a dupe does
/// not exist there. It is refused and nothing is written, exactly as it shipped.
#[test]
fn field_day_still_refuses_a_dupe() {
    let mut log = fd_log();
    assert!(
        log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START),
        "the first contact"
    );
    assert!(
        !log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START + 1800),
        "Field Day REFUSES the dupe — no log checking means no NIL to avoid"
    );
    assert_eq!(log.qsos().len(), 1, "and writes nothing");
    assert_eq!(log.qso_count(), 1);
}

/// Winter Field Day is the same event family and the same ruling.
#[test]
fn winter_field_day_still_refuses_a_dupe() {
    resolver();
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::WinterFd, "2O", "WI"),
        "20m",
    );
    assert!(log.log_mode_at("K1ABC", "2O", "WI", "CW", 0, START));
    assert!(!log.log_mode_at("K1ABC", "2O", "WI", "CW", 0, START + 1800));
    assert_eq!(log.qsos().len(), 1);
}

/// A dupe is still a DUPE to the while-typing verdict in a contest that logs it — the
/// operator is told before they commit, which is the whole reason the check exists. What
/// changed is what happens when they commit anyway.
#[test]
fn the_pre_log_verdict_still_calls_it_a_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(
        log.is_dupe_mode("JA1ABC", "CW"),
        "the cockpit still warns before the operator commits"
    );
}

// ---------------------------------------------------------------------------
// The serial run, which a logged dupe now advances
// ---------------------------------------------------------------------------

/// ⚠️ **SCORING-GRADE, and a consequence rather than a goal.** In a serial contest the
/// number is SENT before the contact is logged, so working a dupe spends it — the other
/// operator copied it and it is in their log. Refusing the row used to leave that number
/// in flight for the NEXT station, which is the failure `restore_row` names in its own
/// words: *"a re-issued SERIAL is copied by the other operator and lands in their log, so
/// two of my QSO lines claim a number the checker can only match to one of them."*
/// Logging the dupe records the number against the contact that actually received it.
#[test]
fn a_logged_dupe_spends_the_serial_it_sent_and_does_not_re_issue_it() {
    resolver();
    let rs = ruleset_by_id("cqwpx_cw", CURRENT_RULES_YEAR).expect("cqwpx_cw is shipped");
    let st = StationData {
        mycall: "W9XYZ".to_string(),
        contest_cq_zone: "4".to_string(),
        ..Default::default()
    };
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::for_ruleset(rs, &st).expect("cqwpx_cw session"),
        "20m",
    );
    // Compose first, exactly as the engine does when the over goes out: that is where a
    // serial is ISSUED, and it is issued whether or not the station turns out to be a dupe.
    let work_wpx = |log: &mut FieldDayLog, call: &str, at: u64| {
        log.session.compose_for(call, at);
        log.log_fields_at(call, &fields(&[("RST", "599")]), "CW", "", 0, at)
    };
    assert!(work_wpx(&mut log, "JA1ABC", START));
    assert!(work_wpx(&mut log, "JA1ABC", START + 1800), "the dupe");
    assert!(work_wpx(&mut log, "DL1ABC", START + 3600));

    assert!(log.qsos()[1].dupe, "same call, same band");
    assert_eq!(
        log.qsos().iter().map(|q| q.sent("NR")).collect::<Vec<_>>(),
        vec!["1", "2", "3"],
        "three stations copied three different numbers; none may be issued twice"
    );
}

// ---------------------------------------------------------------------------
// The policy table — which contests report their duplicates, and which refuse
// ---------------------------------------------------------------------------

/// ⭐ **THE WHOLE RULING, one line per shipped ruleset.** A contest is here because a
/// sponsor's own words put it here, and the two tests below hold the data to it from
/// both ends: one reads it back through the loader, one reads the seed FILE, so a flip,
/// an addition or a removal is a red test rather than a silent rescore.
const DUPE_POLICY: &[(&str, bool)] = &[
    // ⚠️ NO LOG CHECKING AT ALL — no cross-check, no NIL, and "Complete station logs are
    // NOT required for submission, and ARRL does not use the logs." Nothing is spared by
    // keeping the row, and the deliverable wants RAW NON-DUPE counts. Refuse, as shipped.
    ("arrlfd", false),
    ("wfd", false),
    // ⚠️ FIVE SEPARATE SPONSORS, AND EVIDENCE FROM NONE OF THEM. ARRL's and CQ's
    // reasoning is not theirs to inherit; applying it to a sponsor who never stated it
    // is a guess about someone else's rules. These stay at the shipped behaviour until
    // somebody reads the actual rule and changes this line on purpose.
    ("tnqp", false),
    ("ohqp", false),
    ("cqp", false),
    ("txqp", false),
    ("ilqp", false),
    // ⭐ THE ONE PARTY WHOSE SPONSOR SAYS IT. The 2026 New York QSO Party rules (nyqp.org,
    // "v1.1 2026-09-25", QSO POINTS): "Do not remove duplicates from your log before
    // submission. They are used for cross-checking other logs." — the cross-check
    // reasoning ARRL and CQ give, stated by this sponsor for this contest.
    ("nyqp", true),
    // ARRL LGCK.1 — "Duplicate contacts are removed with no additional penalty." It is
    // written for ARRL contests generally, which is why the VHF three are here beside
    // Sweepstakes; Field Day is the exception above, and it is an exception because it
    // is not checked at all.
    ("arrlss_cw", true),
    ("arrlss_ssb", true),
    ("arrlvhf_jan", true),
    ("arrlvhf_jun", true),
    ("arrlvhf_sep", true),
    // CQ XII.E.1 — the same sentence, plus an entrant instruction in as many words:
    // "please do not remove any QSOs from your log! This will cause the other station
    // that worked you to lose credit for the contact."
    ("cqww_cw", true),
    ("cqww_ssb", true),
    ("cqww_rtty", true),
    ("cqwpx_cw", true),
    ("cqwpx_ssb", true),
];

/// The policy reaches `DupeRule` — the LOADER end. A key in the seed that never arrives
/// on the rule would leave the sponsor's ruling written down and not applied.
#[test]
fn every_shipped_ruleset_carries_the_dupe_policy_the_sponsor_backs() {
    for &(id, expected) in DUPE_POLICY {
        let rs = ruleset_by_id(id, CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{id} is a shipped ruleset"));
        assert_eq!(
            rs.dupe_rule.log_dupes, expected,
            "{id}: log_dupes should be {expected} — see the sponsor note in DUPE_POLICY"
        );
    }
}

/// …and the seed declares it for EVERY contest and no others — the DATA end, read from
/// the file the crate actually ships rather than from the table's own list of ids.
///
/// ⚠️ This is the half that catches an ADDED contest. The loader test above walks the
/// table, so a new ruleset that nobody made a decision about is invisible to it and
/// would quietly take the `#[serde(default)]`. Here the two sets must match exactly, so
/// adding, removing or flipping a ruleset all land as the same red.
#[test]
fn the_seed_declares_a_dupe_policy_for_every_contest_and_no_others() {
    let seed: serde_json::Value = serde_json::from_str(include_str!("../src/fd_rules.seed.json"))
        .expect("the bundled seed is valid JSON");
    let mut in_seed: Vec<(String, bool)> = seed["rulesets"]
        .as_array()
        .expect("rulesets is a list")
        .iter()
        .map(|r| {
            let id = r["event"].as_str().expect("every ruleset names its event");
            // Absent is a DECISION, not a hole: it means refuse, which is what every
            // contest written before this key existed meant.
            let logs = r["dupe"]
                .get("log_dupes")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            (id.to_string(), logs)
        })
        .collect();
    in_seed.sort();

    let mut expected: Vec<(String, bool)> = DUPE_POLICY
        .iter()
        .map(|&(id, b)| (id.to_string(), b))
        .collect();
    expected.sort();

    assert_eq!(
        in_seed, expected,
        "the seed and the ruling disagree — a contest was added, removed or flipped \
         without a line in DUPE_POLICY saying which sponsor's words justify it"
    );
}

/// ⚠️ **THE CLUB DUPE SHEET READS THE LOG'S RETURN BOOL AS "UNIQUE"**
/// (`tempo-app/src/fd_scoreboard.rs`, the replay that fills `unique[i]`), and that
/// reading is only true while the ruleset REFUSES duplicates — under a reporting
/// ruleset the bool means "written", which is a different question.
///
/// It holds, and it holds STRUCTURALLY rather than by the current seed: that replay
/// builds its log from `ContestSession::field_day(FdEvent, ..)`, so the rulesets it can
/// reach are exactly the arms of [`FdEvent`], and neither of them reports. The `match`
/// below is the tripwire — it is exhaustive, so adding an arm for a contest that
/// reports its dupes is a COMPILE error here rather than a silently wrong dupe sheet
/// over there.
#[test]
fn the_club_dupe_sheet_replay_can_only_reach_a_refusing_ruleset() {
    fn refuses_by_construction(e: FdEvent) -> bool {
        match e {
            // ⚠️ Adding an arm? The club dupe sheet's `unique[i]` assumes every event
            // here refuses duplicates. Check fd_scoreboard.rs before extending this.
            FdEvent::ArrlFd | FdEvent::WinterFd => true,
        }
    }
    for event in [FdEvent::ArrlFd, FdEvent::WinterFd] {
        assert!(refuses_by_construction(event));
        let log = FieldDayLog::new("W9XYZ", ContestSession::field_day(event, "2A", "WI"), "20m");
        assert!(
            !log.dupe_rule().log_dupes,
            "{event:?} is reachable from the club replay and must refuse duplicates"
        );
    }
}

// ---------------------------------------------------------------------------
// A dupe worked on the air: logged, marked and worth nothing wherever a typed one is refused
// ---------------------------------------------------------------------------

/// The contests that refuse a typed dupe and whose own messages an FT8 double-click on a CQ
/// follows: ARRL Field Day's exchange, and the standard messages in the four parties that allow
/// FT8. Winter Field Day and the Illinois party ban FT8 and FT4, and every other contest on the
/// list logs its dupes already.
const PICKED: [&str; 5] = ["arrlfd", "tnqp", "ohqp", "cqp", "txqp"];

fn dec(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.to_string(),
        sync: 1.0,
        snr: -5,
        dt: 0.0,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// An S&P station on FT8 in `event`, sending the contest's own messages: K1ABC in
/// Massachusetts, 2B EMA at Field Day.
fn ft8_sp(event: &str) -> FieldDayStation {
    resolver();
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = if event == "arrlfd" {
        ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA")
    } else {
        let st = StationData {
            mycall: "K1ABC".to_string(),
            mygrid: "FN42".to_string(),
            contest_qth_state: "MA".to_string(),
            ..Default::default()
        };
        ContestSession::for_ruleset(rs, &st).unwrap_or_else(|e| panic!("{event}: {e}"))
    };
    let mut st = FieldDayStation::search_and_pounce("K1ABC", "FN42", session, "20m");
    st.set_messages(FtMessages::for_contest(rs, "FT8"), false);
    st
}

/// What the station sends next, as it goes on the air.
fn next(st: &FieldDayStation) -> String {
    st.outgoing().map(|m| m.to_text()).unwrap_or_default()
}

/// The operator double-clicks W9XYZ's CQ ([`FieldDayStation::answer_cq`], which a logger's
/// Reply reaches too), and W9XYZ, running, finishes the contact as WSJT-X does: at Field Day
/// with `section` in its exchange. Every over this station sent, in order; the station is back
/// to listening after it.
fn picked_contact(st: &mut FieldDayStation, event: &str, section: &str, slot: u64) -> Vec<String> {
    st.answer_cq("W9XYZ", "EN37");
    finished(st, event, section, slot)
}

/// S&P's own pounce on W9XYZ's CQ, finished as [`picked_contact`] finishes.
fn pounced_contact(st: &mut FieldDayStation, event: &str, section: &str, slot: u64) -> Vec<String> {
    let cq = match event {
        "arrlfd" => "CQ FD W9XYZ EN37",
        "arrlvhf_jan" => "CQ TEST W9XYZ EN37",
        _ => "CQ W9XYZ EN37",
    };
    st.observe(&[dec(cq)], slot);
    finished(st, event, section, slot + 1)
}

/// The contact answered, run to its end by W9XYZ as WSJT-X runs it.
fn finished(st: &mut FieldDayStation, event: &str, section: &str, slot: u64) -> Vec<String> {
    let rogered = format!("K1ABC W9XYZ R 1D {section}");
    let replies = match event {
        "arrlfd" => vec![rogered.as_str()],
        "arrlvhf_jan" => vec!["K1ABC W9XYZ R EN37"],
        _ => vec!["K1ABC W9XYZ -07", "K1ABC W9XYZ RR73"],
    };
    let mut sent = vec![next(st)];
    for (k, reply) in replies.into_iter().enumerate() {
        st.after_tx();
        st.observe(&[dec(reply)], slot + 2 * k as u64 + 1);
        sent.push(next(st));
    }
    st.after_tx();
    assert!(st.done(), "{event}: the contact ran to its end");
    st.rearm(false);
    sent
}

/// Each row's call and dupe mark, in log order.
fn marks(log: &FieldDayLog) -> Vec<(String, bool)> {
    log.qsos()
        .iter()
        .map(|q| (q.call.clone(), q.dupe))
        .collect()
}

/// `n` rows of W9XYZ, the first the contact and every later one a dupe.
fn w9xyz(n: usize) -> Vec<(String, bool)> {
    (0..n).map(|i| ("W9XYZ".to_string(), i > 0)).collect()
}

/// The records in the contest ADIF the entry submits.
fn submitted(log: &FieldDayLog) -> usize {
    log.submission_adif()
        .to_ascii_uppercase()
        .matches("<EOR>")
        .count()
}

/// ⭐ **THE CHANGE: a dupe worked on the air is logged as one**, in every contest whose typed
/// dupe is refused. The double-click answers a station this running's log already holds and the
/// contact is made, so it is written and marked, and it moves no number: the same overs on the
/// air, the same count, points and multipliers (at Field Day a section only the dupe carried is
/// not claimed), and a Cabrillo file that is the one before with the dupe's QSO line added,
/// every header as it was. The contest ADIF keeps it too.
#[test]
fn a_dupe_the_operator_picked_off_the_air_is_logged_marked_and_moves_no_number() {
    for event in PICKED {
        let mut st = ft8_sp(event);
        let first = picked_contact(&mut st, event, "WI", 1);
        let numbers = |l: &FieldDayLog| (l.qso_count(), score(l), l.worked_values("SECTION"));
        let before = numbers(&st.log);
        let file = st.log.cabrillo(14_074).expect("a one-mode log exports");
        assert_eq!(submitted(&st.log), 1, "{event}: PREMISE, one contact");

        // W9XYZ again, picked off the air, and copied in another section this time.
        let again = picked_contact(&mut st, event, "IL", 11);
        assert_eq!(again, first, "{event}: the same overs on the air");
        assert_eq!(
            marks(&st.log),
            w9xyz(2),
            "{event}: logged, and marked a dupe"
        );
        assert_eq!(
            numbers(&st.log),
            before,
            "{event}: the dupe is worth nothing"
        );
        let after = st.log.cabrillo(14_074).expect("a one-mode log exports");
        let mut lines: Vec<&str> = after.lines().collect();
        let at = lines
            .iter()
            .rposition(|l| l.starts_with("QSO:"))
            .expect("QSO lines");
        let dupe = lines.remove(at);
        assert!(
            dupe.contains("W9XYZ"),
            "{event}: the dupe's QSO line: {dupe}"
        );
        assert_eq!(
            lines,
            file.lines().collect::<Vec<_>>(),
            "{event}: the file before, with the dupe's QSO line added"
        );
        assert_eq!(submitted(&st.log), 2, "{event}: the contest ADIF keeps it");
    }
}

/// ⭐ **It survives a restart, still marked**: the journal brings back a dupe the ruleset alone
/// would refuse, and so does the journal the restored log writes. Read twice into one log, the
/// journal keeps each contact once, as it always has.
#[test]
fn a_picked_dupe_survives_a_restart_still_marked_and_a_second_read_keeps_it_once() {
    for event in PICKED {
        let mut st = ft8_sp(event);
        picked_contact(&mut st, event, "WI", 1);
        picked_contact(&mut st, event, "WI", 11);
        let row = |q: &tempo_core::fieldday::LoggedQso| {
            let kv = |v: &[tempo_core::contest::FieldValue]| -> Vec<String> {
                v.iter().map(|f| format!("{}={}", f.key, f.raw)).collect()
            };
            (
                kv(&q.rx),
                kv(&q.tx),
                q.mode.clone(),
                q.submode.clone(),
                q.band.clone(),
            )
        };
        assert_eq!(marks(&st.log), w9xyz(2), "{event}: PREMISE, the dupe");
        let mut own = ft8_sp(event);
        pounced_contact(&mut own, event, "WI", 1);
        assert_eq!(
            row(&st.log.qsos()[1]),
            row(&own.log.qsos()[0]),
            "{event}: the dupe is the row S&P's own pounce writes for the same contact"
        );
        let journal = st.log.journal_adif();
        let now = st.log.qsos()[0].when_unix;
        let mut back = ft8_sp(event).log;
        back.restore_journal(&journal, 0, now);
        assert_eq!(marks(&back), w9xyz(2), "{event}: back, still marked");
        assert_eq!(
            (back.qso_count(), score(&back), back.cabrillo(14_074)),
            (st.log.qso_count(), score(&st.log), st.log.cabrillo(14_074)),
            "{event}: scored and filed the same"
        );
        back.restore_journal(&journal, 0, now);
        assert_eq!(
            marks(&back),
            w9xyz(2),
            "{event}: read twice, each contact once"
        );
        let mut again = ft8_sp(event).log;
        again.restore_journal(&back.journal_adif(), 0, now);
        assert_eq!(
            marks(&again),
            w9xyz(2),
            "{event}: and after a second restart"
        );
    }
}

/// A contest that logs its dupes logs a picked one as it always has: marked, and written with
/// nothing more in its journal or its files.
#[test]
fn a_contest_that_logs_its_dupes_writes_a_picked_one_as_it_always_has() {
    let mut st = ft8_sp("arrlvhf_jan");
    let first = picked_contact(&mut st, "arrlvhf_jan", "", 1);
    assert_eq!(picked_contact(&mut st, "arrlvhf_jan", "", 11), first);
    assert_eq!(marks(&st.log), w9xyz(2));
    for file in [
        st.log.journal_adif(),
        st.log.adif(),
        st.log.submission_adif(),
    ] {
        assert!(!file.contains("APP_NEXUS_KEPT"), "{file}");
    }
}

/// A typed write of the contact `call` with exactly the key the FT row carries: at Field Day
/// its class and section, in a party no exchange at all (the standard messages carry none).
fn typed(log: &mut FieldDayLog, event: &str, call: &str) -> bool {
    match event {
        "arrlfd" => log.log(call, "1D", "WI", 0),
        _ => log.log_exchange_at(call, Vec::new(), Vec::new(), "DIG", "FT8", 0, START),
    }
}

/// The controls, which hold before the change and after it: only the operator's pick keeps a
/// dupe. S&P's own pounce passes the worked station's CQ by and answers a new station's; a
/// contact S&P pounced on itself and that was typed into the log before it ended is not logged
/// a second time; a TYPED dupe is still refused; and on the run, a station already in the log
/// that calls is worked as WSJT-X works it and, in these contests, still not logged.
#[test]
fn only_the_operators_pick_keeps_a_dupe() {
    for event in PICKED {
        let mut st = ft8_sp(event);
        picked_contact(&mut st, event, "WI", 1);
        let cq = |call: &str| match event {
            "arrlfd" => format!("CQ FD {call} EN37"),
            _ => format!("CQ {call} EN37"),
        };
        st.observe(&[dec(&cq("W9XYZ"))], 3);
        assert_eq!(next(&st), "", "{event}: S&P passes a worked station by");
        let rows = st.log.qsos().len();
        assert!(!typed(&mut st.log, event, "W9XYZ"), "{event}: a typed dupe");
        assert_eq!(st.log.qsos().len(), rows, "{event}: is refused");

        // S&P answers W9ABC's CQ itself; W9ABC is typed in before the contact ends.
        st.observe(&[dec(&cq("W9ABC"))], 5);
        assert_ne!(next(&st), "", "{event}: S&P answers a new station");
        assert!(typed(&mut st.log, event, "W9ABC"), "{event}: typed in");
        let replies: &[&str] = match event {
            "arrlfd" => &["K1ABC W9ABC R 1D WI"],
            _ => &["K1ABC W9ABC -07", "K1ABC W9ABC RR73"],
        };
        for (k, reply) in replies.iter().enumerate() {
            st.after_tx();
            st.observe(&[dec(reply)], 7 + 2 * k as u64);
        }
        assert_eq!(st.log.qsos().len(), rows + 1, "{event}: S&P's own, once");

        // The run: W9XYZ, already in the log, calls and is worked to the end.
        let mut run = ft8_sp(event);
        run.set_messages(FtMessages::for_contest(run.log.ruleset(), "FT8"), true);
        run.rearm(true);
        assert!(typed(&mut run.log, event, "W9XYZ"), "{event}: PREMISE");
        let calls: &[&str] = match event {
            "arrlfd" => &["K1ABC W9XYZ 1D WI", "K1ABC W9XYZ RR73"],
            _ => &["K1ABC W9XYZ EN37", "K1ABC W9XYZ R-07"],
        };
        let mut sent = Vec::new();
        for (k, call) in calls.iter().enumerate() {
            run.after_tx();
            run.observe(&[dec(call)], 1 + 2 * k as u64);
            sent.push(next(&run));
        }
        run.after_tx();
        assert!(
            run.done() && sent[0].starts_with("W9XYZ K1ABC"),
            "{event}: worked on the run: {sent:?}"
        );
        assert_eq!(marks(&run.log), w9xyz(1), "{event}: and not logged");
    }
}
