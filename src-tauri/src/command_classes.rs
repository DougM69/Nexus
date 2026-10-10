//! The class table's guard: every command `lib.rs` registers in `generate_handler!` has exactly one
//! row in the client core's table (`tempo_client::classes`, where each row and its reason are), and
//! every row there names a command registered here, so a new command cannot arrive unclassed and a
//! removed one cannot leave a row behind. The rest of this app reads the table through here, as
//! the other computer reads it there.
#[cfg(test)]
pub(crate) use tempo_client::classes::classes;
#[cfg(test)]
use tempo_client::classes::CLASSES;
pub(crate) use tempo_client::classes::{class_of, Class};

/// The command names `generate_handler!` registers, with any module path dropped
/// (`log_by_id::edit_qso_by_id` is `edit_qso_by_id`). It reads `lib.rs` as text, as the other
/// registration tests do, because the list exists nowhere else.
#[cfg(test)]
pub(crate) fn registered() -> Vec<&'static str> {
    include_str!("lib.rs")
        .split_once("tauri::generate_handler![")
        .and_then(|(_, rest)| rest.split_once("])"))
        .map(|(list, _)| list)
        .expect("lib.rs has a generate_handler! list")
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            // A path is all this reads. An attribute or a comment in the list fails here, by
            // name, instead of passing as a command that nothing registered.
            assert!(
                entry
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':'),
                "a handler entry this test cannot read: {entry:?}"
            );
            entry.rsplit("::").next().unwrap_or(entry)
        })
        .collect()
}

#[test]
fn every_registered_command_has_a_class() {
    let missing: Vec<&str> = registered()
        .into_iter()
        .filter(|name| !CLASSES.iter().any(|(row, ..)| row == name))
        .collect();
    assert!(
        missing.is_empty(),
        "no class for {missing:?}: add one row each to CLASSES in crates/tempo-client/src/classes.rs"
    );
}

#[test]
fn every_row_names_a_registered_command() {
    let registered = registered();
    let stale: Vec<&str> = CLASSES
        .iter()
        .map(|(row, ..)| *row)
        .filter(|row| !registered.contains(row))
        .collect();
    assert!(
        stale.is_empty(),
        "rows for commands lib.rs does not register: {stale:?}"
    );
}

/// Exactly one class each: a second row for a command fails here, as does a row out of place.
#[test]
fn one_row_per_command_in_name_order() {
    for pair in CLASSES.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "{:?} follows {:?}: one row per command, in name order",
            pair[1].0,
            pair[0].0
        );
    }
}

/// The lookup the dispatcher routes and admits by finds every row with its own class, and nothing
/// that has no row.
#[test]
fn class_of_finds_every_row_and_nothing_else() {
    for (name, class, _) in CLASSES {
        assert_eq!(class_of(name), Some(*class), "{name}");
    }
    for name in [
        "",
        "throwaway_unclassed_probe",
        "zzz",
        "log_by_id::edit_qso_by_id",
    ] {
        assert_eq!(class_of(name), None, "{name:?}");
    }
}
