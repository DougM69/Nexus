//! ⭐ **Every save of the contest journal keeps rolling copies beside it**, in
//! `fieldday_history/`: the newest saves and one per hour. The journal is the only copy of a
//! contest's contacts on disk until they are merged into the Logbook, so a journal lost or
//! damaged later has a fallback. A copy that fails is reported in the diagnostic log and never
//! fails the journal, or the Log press that waits for it.

use super::*;

/// ARRL Field Day, 3A from Wisconsin, journalled at `<dir>/fieldday_backup_7.adi`.
fn contest_on(dir: &std::path::Path) -> Engine {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("Wisconsin is a current section");
    e.set_fd_log_path(dir.join("fieldday_backup_7.adi"));
    e
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nexus-journal-copies-{tag}-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// After a Log press the newest copy in `fieldday_history/` is the journal, byte for byte.
#[test]
fn after_a_log_press_the_newest_copy_is_the_journal() {
    let dir = scratch("press");
    let mut e = contest_on(&dir);
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "CW").unwrap());
    e.journal_mark().wait();

    let journal = std::fs::read(dir.join("fieldday_backup_7.adi")).expect("the journal");
    let history = dir.join("fieldday_history");
    let mut saves: Vec<std::path::PathBuf> = std::fs::read_dir(&history)
        .expect("the history folder")
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fieldday_backup_7.save-")
        })
        .collect();
    saves.sort();
    let newest = saves.last().expect("a save copy");
    assert_eq!(std::fs::read(newest).unwrap(), journal);
    assert!(
        String::from_utf8_lossy(&journal).contains("<CALL:4>W1AW"),
        "premise: the journal holds the last contact"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A history folder that cannot be made (a file in its place) leaves the Log press succeeding
/// and the contact in the journal, with no "not being saved" trouble on it.
#[test]
fn a_copy_that_fails_leaves_the_log_press_and_the_journal_good() {
    let dir = scratch("blocked");
    std::fs::write(dir.join("fieldday_history"), "a file where the folder goes").unwrap();
    let mut e = contest_on(&dir);
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    e.journal_mark().wait();

    let path = dir.join("fieldday_backup_7.adi");
    let journal = std::fs::read_to_string(&path).expect("the journal was written");
    assert!(journal.contains("<CALL:5>K1ABC"));
    assert_eq!(e.station.journals.trouble(&path), None);
    let _ = std::fs::remove_dir_all(&dir);
}
