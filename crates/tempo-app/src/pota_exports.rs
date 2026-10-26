//! The activation files already exported — park × UTC day × callsign, the unit the
//! per-activation export writes ([`crate::logexport::export_for_activation`]) — so a later change
//! to those contacts can say that a file already made, and perhaps already uploaded at pota.app,
//! no longer matches the log.
//!
//! POTA has no upload API: the operator uploads the file by hand at pota.app, and nothing comes
//! back that Nexus could read. The export is the last thing Nexus sees, so it is what is recorded.
//! Only the key is kept, never a contact, and the record changes no contact: it is read by the
//! Logbook's bulk edit preview ([`crate::logtools::BulkPreview::activation_exported`]), which
//! names the files a change would put out of date. Files exported before this record existed are
//! not in it.
//!
//! Kept in its own file beside settings.json (`pota_exports.json`), written whole on each new
//! entry (temporary file, fsync, rename), never a Settings field — the same reason as the running
//! activation's file: the Settings screen saves the whole of what it loaded.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tempo_core::logbook::ActivationKey;

/// The exported activation files, and the file they are kept in.
#[derive(Debug, Default)]
pub struct ExportedActivations {
    path: Option<PathBuf>,
    keys: BTreeSet<ActivationKey>,
}

impl ExportedActivations {
    /// Keep the record in `path`, taking up what it already holds. A missing file is an empty
    /// record; one this build cannot read is left as it is and said in the diagnostic log.
    pub fn open(path: PathBuf) -> Self {
        let keys = match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str::<BTreeSet<ActivationKey>>(&text).unwrap_or_else(|e| {
                    tempo_core::applog::warn(
                        "pota exports",
                        &format!("{} could not be read: {e}", path.display()),
                    );
                    BTreeSet::new()
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => {
                tempo_core::applog::warn("pota exports", &format!("{}: {e}", path.display()));
                BTreeSet::new()
            }
        };
        ExportedActivations {
            path: Some(path),
            keys,
        }
    }

    /// The file for `reference` (upper-cased), the UTC day holding `day_unix` and `callsign` was
    /// exported. Kept at once when it is new.
    pub fn note(&mut self, reference: &str, day_unix: u64, callsign: Option<&str>) {
        let key = (
            reference.trim().to_ascii_uppercase(),
            day_unix - day_unix % 86_400,
            callsign
                .map(|c| c.trim().to_ascii_uppercase())
                .filter(|c| !c.is_empty()),
        );
        if self.keys.insert(key) {
            self.persist();
        }
    }

    /// Whether the file `key` names was exported.
    pub fn contains(&self, key: &ActivationKey) -> bool {
        self.keys.contains(key)
    }

    fn persist(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let Ok(text) = serde_json::to_string(&self.keys) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let res = std::fs::File::create(&tmp)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all()
            })
            .and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            tempo_core::applog::warn("pota exports", &format!("not kept: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An exported file is remembered across a restart, keyed as the export keys it: the UTC
    /// day of any second in it, the reference and call in upper case.
    #[test]
    fn an_exported_file_is_kept_across_a_restart() {
        let dir = std::env::temp_dir().join(format!("pota-exports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("pota_exports.json");
        let mut rec = ExportedActivations::open(path.clone());
        let day = 1_767_225_600;
        rec.note("us-1234", day + 3_600, Some("kd9taw"));
        let again = ExportedActivations::open(path);
        let key = ("US-1234".to_string(), day, Some("KD9TAW".to_string()));
        assert!(again.contains(&key));
        assert!(!again.contains(&("US-1234".to_string(), day + 86_400, Some("KD9TAW".into()))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
