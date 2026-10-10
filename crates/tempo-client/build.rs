//! The app's version for the command link's build check (`wire::APP`): src-tauri's own package
//! version, the number the installer and the updater show, read from its manifest's first `version`
//! line (the one `scripts/release-prep` sets), so this crate and the app it is built into never name
//! two versions. src-tauri's tests hold the two equal.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"))
        .join("../../src-tauri/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|error| {
        panic!(
            "the app's version is read from {}: {error}",
            manifest.display()
        )
    });
    let version = text
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{} names no version", manifest.display()));
    println!("cargo:rustc-env=NEXUS_APP_VERSION={version}");
}
