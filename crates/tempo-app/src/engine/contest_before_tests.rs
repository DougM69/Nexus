//! The station as it was before the contest changed it, through the engine's own write paths — a
//! Settings save, the seat swap, the Unassisted switch, a restore — and through settings.json. Every
//! assertion is on the note's VALUE: when it is taken, what it holds, that the first one stands, and
//! when it is let go.
use super::*;
use crate::settings::ContestBefore;

/// A station on its everyday settings: its own call, nobody else at the key, no position, no
/// declarations, Unassisted off, the Illinois QSO Party picked but not running.
fn everyday() -> Engine {
    let mut e = Engine::new("K9ABC", "EN52", 0);
    let mut s = e.settings().clone();
    s.mycall = "K9ABC".into();
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "COOK".into();
    e.apply_settings(s);
    e
}

/// What [`everyday`] holds, as the note holds it.
fn everyday_values() -> ContestBefore {
    ContestBefore {
        mycall: "K9ABC".into(),
        ..ContestBefore::default()
    }
}

/// A Settings save of `edit` over what the engine holds now.
fn save(e: &mut Engine, edit: impl FnOnce(&mut Settings)) {
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

#[test]
fn an_everyday_station_holds_no_note() {
    assert_eq!(everyday().settings().contest_before, None);
}

#[test]
fn a_club_setup_saved_before_the_switch_is_noted_as_the_station_was_before_it() {
    let mut e = everyday();
    // Friday at the club: the club's call, a tent, MULTI-OP. Nobody has turned the mode on.
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_position_name = "CW tent".into();
        s.contest_category_operator = "MULTI-OP".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn turning_the_mode_on_takes_the_note_even_when_nothing_else_changed() {
    let mut e = everyday();
    save(&mut e, |s| s.fd_active = true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn the_first_note_stands_through_every_later_change_of_the_weekend() {
    let mut e = everyday();
    save(&mut e, |s| s.contest_category_power = "LOW".into());
    save(&mut e, |s| s.fd_active = true);
    save(&mut e, |s| s.contest_entry_class = "ILLINOIS FIXED".into());
    e.set_fd_operator("w9op".into());
    e.set_unassisted_mode(true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn a_seat_swap_or_the_unassisted_switch_takes_the_note_on_its_own() {
    let mut e = everyday();
    e.set_fd_operator("w9op".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    let mut e = everyday();
    e.set_unassisted_mode(true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn a_save_that_moves_none_of_the_seven_takes_no_note() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.contest_email = "k9abc@example.org".into();
        s.contest_qth_county = "WILL".into();
    });
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_stale_or_forged_payload_neither_writes_the_note_nor_wipes_it() {
    let mut e = everyday();
    // A surface that loaded before the weekend: its copy has no note.
    let stale = e.settings().clone();
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    let mut wipe = stale;
    wipe.mycall = "W9CLB".into();
    e.apply_settings(wipe);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "a payload without one wipes nothing"
    );
    let mut forged = e.settings().clone();
    forged.contest_before = Some(ContestBefore {
        mycall: "N0PE".into(),
        ..ContestBefore::default()
    });
    e.apply_settings(forged);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "nor does a payload write one"
    );
}

#[test]
fn turning_the_mode_off_lets_the_note_go_with_the_contest() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_active = true;
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    // The checklist's save: the mode off, the call put back.
    save(&mut e, |s| {
        s.fd_active = false;
        s.mycall = "K9ABC".into();
    });
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(e.settings().mycall, "K9ABC");
}

#[test]
fn turning_the_mode_off_with_nothing_put_back_still_lets_it_go() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_active = true;
    });
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(
        e.settings().mycall,
        "W9CLB",
        "what was not put back stays as it is"
    );
}

#[test]
fn unassisted_put_back_before_the_mode_goes_off_leaves_no_stray_note() {
    // The checklist's order: Unassisted through its own switch while the mode is still on, then the
    // save that turns the mode off. The other way round, the switch would find no note open and take
    // a new one holding the contest's "on" as "before".
    let mut e = everyday();
    e.set_unassisted_mode(true);
    save(&mut e, |s| s.fd_active = true);
    e.set_unassisted_mode(false);
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_restore_takes_the_backups_note_whole() {
    let mut e = everyday();
    let mut bundle = e.settings().clone();
    let noted = ContestBefore {
        mycall: "K9OLD".into(),
        contest_category_power: "HIGH".into(),
        ..ContestBefore::default()
    };
    bundle.contest_before = Some(noted.clone());
    e.apply_restored_settings(bundle);
    assert_eq!(e.settings().contest_before, Some(noted));
}

#[test]
fn the_note_round_trips_through_settings_json_and_an_older_file_reads_as_none() {
    let s = Settings {
        contest_before: Some(ContestBefore {
            mycall: "K9ABC".into(),
            fd_operator: "K9ZZZ".into(),
            fd_position_name: "CW tent".into(),
            contest_category_operator: "MULTI-OP".into(),
            contest_entry_class: "ILLINOIS FIXED".into(),
            contest_category_power: "LOW".into(),
            unassisted_mode: true,
        }),
        ..Settings::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    // The wire keys `ui/src/types.ts` writes by hand.
    for key in [
        "\"contestBefore\":{\"mycall\":\"K9ABC\"",
        "\"fdOperator\":\"K9ZZZ\"",
        "\"fdPositionName\":\"CW tent\"",
        "\"contestCategoryOperator\":\"MULTI-OP\"",
        "\"contestEntryClass\":\"ILLINOIS FIXED\"",
        "\"contestCategoryPower\":\"LOW\"",
        "\"unassistedMode\":true}",
    ] {
        assert!(json.contains(key), "missing {key} in the settings file");
    }
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
    older.as_object_mut().unwrap().remove("contestBefore");
    assert_eq!(
        serde_json::from_value::<Settings>(older)
            .unwrap()
            .contest_before,
        None
    );
}

/// A station that has never picked a contest: the picker's blank default, Contest mode off. One save
/// first, so the engine's own fill-ins (a fresh roster's default radio) are in before what a test
/// saves.
fn nothing_picked() -> Engine {
    let mut e = Engine::new("K9ABC", "EN52", 0);
    let s = e.settings().clone();
    e.apply_settings(s);
    assert_eq!(e.settings().fd_event, "", "premise: no contest picked");
    e
}

/// The settings.json keys whose values differ between `a` and `b`.
fn changed(a: &Settings, b: &Settings) -> Vec<String> {
    let (a, b) = (
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap(),
    );
    let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
    let mut keys: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

#[test]
fn with_no_contest_picked_a_save_holds_exactly_what_the_form_sent() {
    let mut e = nothing_picked();
    let mut form = e.settings().clone();
    form.mycall = "W9CLB".into();
    e.apply_settings(form.clone());
    assert_eq!(changed(&form, e.settings()), Vec::<String>::new());
    // A pick of nothing but spaces is the same blank.
    let mut e = nothing_picked();
    save(&mut e, |s| s.fd_event = "  ".into());
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn with_no_contest_picked_the_unassisted_switch_changes_only_unassisted() {
    let mut e = nothing_picked();
    let was = e.settings().clone();
    e.set_unassisted_mode(true);
    assert_eq!(changed(&was, e.settings()), vec!["unassistedMode"]);
}

#[test]
fn with_no_contest_picked_the_operator_box_changes_only_the_operator() {
    let mut e = nothing_picked();
    let was = e.settings().clone();
    e.set_fd_operator("w9op".into());
    assert_eq!(changed(&was, e.settings()), vec!["fdOperator"]);
}

#[test]
fn with_no_contest_picked_the_mode_going_on_still_takes_the_note_and_going_off_lets_it_go() {
    // The blank pick runs ARRL Field Day when the mode goes on: that is a contest running.
    let mut e = nothing_picked();
    save(&mut e, |s| {
        s.fd_active = true;
        s.fd_class = "1D".into();
        s.fd_section = "IL".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    save(&mut e, |s| s.mycall = "W9CLB".into());
    e.set_unassisted_mode(true);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "the first note stands while the contest runs"
    );
    e.set_unassisted_mode(false);
    save(&mut e, |s| {
        s.fd_active = false;
        s.mycall = "K9ABC".into();
    });
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_club_call_set_with_a_contest_picked_is_offered_back_when_the_contest_ends() {
    let mut e = nothing_picked();
    // Picking the contest moves none of the seven.
    save(&mut e, |s| {
        s.fd_event = "ilqp".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
    });
    assert_eq!(e.settings().contest_before, None);
    // Friday, the mode still off: the club's call.
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    // The weekend.
    save(&mut e, |s| s.fd_active = true);
    e.set_fd_operator("w9op".into());
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "what the checklist offers as the mode goes off: the station's own call"
    );
    // The checklist's save: the call put back, the mode off.
    save(&mut e, |s| {
        s.mycall = "K9ABC".into();
        s.fd_active = false;
    });
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(e.settings().mycall, "K9ABC");
}

#[test]
fn picking_the_contest_and_setting_the_club_call_in_one_save_takes_the_note() {
    let mut e = nothing_picked();
    save(&mut e, |s| {
        s.fd_event = "ilqp".into();
        s.mycall = "W9CLB".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn at_launch_a_note_with_no_contest_picked_and_the_mode_off_is_let_go_and_any_other_is_kept() {
    let noted = ContestBefore {
        mycall: "K9OLD".into(),
        ..ContestBefore::default()
    };
    let launched = |event: &str, active: bool| {
        let stored = Settings {
            mycall: "K9ABC".into(),
            fd_event: event.into(),
            fd_active: active,
            fd_class: "1D".into(),
            fd_section: "IL".into(),
            contest_before: Some(noted.clone()),
            ..Settings::default()
        };
        Engine::with_settings(stored)
            .settings()
            .contest_before
            .clone()
    };
    assert_eq!(
        launched("", false),
        None,
        "an earlier build's everyday note"
    );
    assert_eq!(launched("  ", false), None, "spaces are the same blank");
    assert_eq!(
        launched("ilqp", false),
        Some(noted.clone()),
        "a contest picked"
    );
    assert_eq!(
        launched("", true),
        Some(noted.clone()),
        "a restart mid-contest"
    );
    assert_eq!(launched("ilqp", true), Some(noted), "both");
}
