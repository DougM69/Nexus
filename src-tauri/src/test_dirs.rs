//! Test builds only: no test reads or writes the operator's own Nexus folders.
//!
//! The app keeps an operator's files in three folders, each found from the platform's own
//! variables: the config folder (`config_base`: `settings.json` and everything beside it, the
//! journals, the window layouts, the Remote and LAN files), the local data folder
//! (`local_data_dir`: `ALL.TXT`, the diagnostic log) and the shared data folder
//! (`shared_data_dir`: the logbook, the data tables, the Winlink mailbox). A test that drives a
//! command handler saves through those same functions, so a `cargo test` of this crate wrote the
//! `settings.json` of whoever ran it (`$XDG_CONFIG_HOME/tempo`, `~/.config/tempo`,
//! `%APPDATA%\tempo`), and every test that saved settings shared that one file with every other
//! test running beside it.
//!
//! In a test build the three answer with [`root`] instead: a folder of the calling thread's own,
//! made by whatever first writes into it and removed when the thread ends. libtest runs every test
//! on a thread of its own, so that is one folder per test, and nothing for a test to remember. The
//! shipped resolvers are `cfg(not(test))`, so no thread of a test build can reach the operator's
//! folders, and the shared folder's twin does not read `NEXUS_DATA_DIR`. Pointing
//! `XDG_CONFIG_HOME` or `HOME` elsewhere from inside a test would not do: the environment belongs
//! to the whole process, and tests run in parallel.
//!
//! ⚠️ **The unit is the thread, not the test.** A `#[tauri::command(async)]` handler runs on the
//! async runtime's worker threads even when a test calls it, so what it saves lands in that
//! worker's folder: never the operator's, but shared by every test whose calls that worker
//! happens to run, and never removed (a worker outlives every test). No test reads such a save
//! back from disk; one that needs to should read the Engine instead.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::settings::writer::{Flushed, SettingsWriter};
use tempo_app::settings::Settings;

/// The calling thread's folder: `nexus-test-dirs-<pid>-<thread id>` in the temp directory. The
/// process id keeps two test binaries apart, the thread id two tests in one.
pub(crate) fn root() -> PathBuf {
    FOLDER.with(|folder| folder.0.clone())
}

/// A thread's folder, removed when the thread ends: the test that owned it is over by then.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

thread_local! {
    static FOLDER: Folder = Folder(std::env::temp_dir().join(format!(
        "nexus-test-dirs-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )));
}

/// The folders the SHIPPED app keeps an operator's files in on this computer, worked out here from
/// the platform's own variables the way the release build's resolvers work them out (`config_base`,
/// `local_data_dir`, and `shared_data_dir`'s `NEXUS_DATA_DIR`), so the guards below do not take the
/// resolvers under test at their word.
fn operators_folders() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let (config, data) = if cfg!(windows) {
        (var("APPDATA"), var("LOCALAPPDATA"))
    } else {
        (
            var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))),
            var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share"))),
        )
    };
    let mut folders = vec![
        config.unwrap_or_else(|| PathBuf::from(".")),
        data.unwrap_or_else(|| PathBuf::from(".")).join("Nexus"),
    ];
    folders.extend(var("NEXUS_DATA_DIR").filter(|d| !d.as_os_str().is_empty()));
    folders
}

/// ⛔ The assertion every guard here ends on: `written` is in no folder of the operator's.
fn assert_not_the_operators(written: &Path, what: &str) {
    for theirs in operators_folders() {
        assert!(
            !written.starts_with(&theirs),
            "a test wrote the operator's {what}: {} is inside {}",
            written.display(),
            theirs.display()
        );
    }
}

/// Text no earlier run can have left in a file: this process, this thread, this instant.
fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{}-{:?}-{nanos}",
        std::process::id(),
        std::thread::current().id()
    )
}

/// Save a configuration whose call is `call` the way every command saves one (the snapshot taken
/// and handed to the file's writer under the Engine lock), wait for it to reach the disk, and
/// answer with the file the writer wrote.
fn save_settings_as(call: &str) -> PathBuf {
    let engine = std::sync::Mutex::new(Engine::new(call, "EN52", 0));
    let eng = engine_lock(&engine);
    crate::persist_settings(eng.settings().clone(), |_| {});
    drop(eng);
    let written = crate::settings_path();
    assert_eq!(
        SettingsWriter::of(&written).flush(Duration::from_secs(30)),
        Flushed::Saved,
        "control: the writer saved {}",
        written.display()
    );
    written
}

/// ⛔ A setting a test saves lands in that test's own folder, never in the operator's
/// settings.json. Saved as a command saves one, so this finds out where the bytes land rather
/// than asking a path function where they would.
#[test]
fn a_setting_a_test_saves_never_reaches_the_operators_settings_file() {
    let written = save_settings_as("K9TST");
    assert_eq!(
        Settings::load(&written).mycall,
        "K9TST",
        "control: the save reached {}, or this says nothing about where",
        written.display()
    );
    assert_not_the_operators(&written, "settings.json");
}

/// ⛔ A decode line a test logs lands in that test's own `ALL.TXT`, never the operator's: the
/// local data folder (`local_data_dir`), appended to by the app's own writer.
#[test]
fn a_decode_line_a_test_logs_never_reaches_the_operators_all_txt() {
    let line = format!("{} a test's own decode line", unique());
    crate::flush_all_txt(std::slice::from_ref(&line));
    let written = crate::all_txt_path();
    assert!(
        std::fs::read_to_string(&written)
            .unwrap_or_default()
            .contains(&line),
        "control: the line reached {}, or this says nothing about where",
        written.display()
    );
    assert_not_the_operators(&written, "ALL.TXT");
}

/// ⛔ A data table a test refreshes lands in that test's own shared data folder, never the
/// operator's: the folder that holds the logbook (`shared_data_dir`), written as the satellite
/// elements cache writes it.
#[test]
fn a_data_table_a_test_refreshes_never_reaches_the_operators_data_folder() {
    let marker = unique();
    let written = crate::shared_tles_path();
    assert!(
        crate::write_json_atomic(&written, &format!("{{\"marker\":\"{marker}\"}}")),
        "control: the table was written to {}",
        written.display()
    );
    assert!(
        std::fs::read_to_string(&written)
            .unwrap_or_default()
            .contains(&marker),
        "control: and it is this test's: {}",
        written.display()
    );
    assert_eq!(
        crate::logbook_path().parent(),
        written.parent(),
        "the logbook is in the folder this test just wrote"
    );
    assert_not_the_operators(&written, "shared data folder");
}

/// ⛔ Two tests saving settings at once each read back their own. libtest runs every test on a
/// thread of its own, so the two tests here are two threads: the first saves, then the second,
/// then each reads its file back. With one folder for every test (the operator's, as it was), the
/// second save lands over the first and the first reads the second's call.
#[test]
fn two_tests_saving_settings_at_once_each_read_back_their_own() {
    let (first_saved, after_first) = mpsc::channel::<()>();
    let (second_saved, after_second) = mpsc::channel::<()>();
    let first = std::thread::spawn(move || {
        let written = save_settings_as("K1AAA");
        first_saved.send(()).expect("the second test is waiting");
        after_second
            .recv_timeout(Duration::from_secs(60))
            .expect("the second test saved");
        (Settings::load(&written).mycall, written)
    });
    let second = std::thread::spawn(move || {
        after_first
            .recv_timeout(Duration::from_secs(60))
            .expect("the first test saved");
        let written = save_settings_as("K2BBB");
        second_saved.send(()).expect("the first test is waiting");
        (Settings::load(&written).mycall, written)
    });
    let first = first.join().expect("the first test ran");
    let second = second.join().expect("the second test ran");
    assert_eq!(
        first.0,
        "K1AAA",
        "the first test read back another test's settings from {}",
        first.1.display()
    );
    assert_eq!(
        second.0,
        "K2BBB",
        "the second test read back another test's settings from {}",
        second.1.display()
    );
}

/// A test's folder goes with the test, so a run leaves nothing of it in the temp directory.
/// CONTROL: the save was in that folder before the test ended.
#[test]
fn a_tests_folder_goes_with_the_test() {
    let (folder, written) = std::thread::spawn(|| (root(), save_settings_as("K3CCC")))
        .join()
        .expect("the test ran");
    assert!(
        written.starts_with(&folder),
        "control: the save went into the test's own folder: {}",
        written.display()
    );
    assert!(
        !folder.exists(),
        "a finished test's folder was left behind: {}",
        folder.display()
    );
}
