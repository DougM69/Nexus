//! The Logbook's exports, read from the logbook store — SPEC-2 v3's **C15**.
//!
//! The ADIF and CSV export (whole, or a date range, #98), one operator's contacts (#25), one
//! activation's (POTA's unit of credit), and the two lists those splits are chosen from. Until
//! C15 each was a pass over the log in memory made UNDER the Engine lock — hundreds of
//! milliseconds at 150,000 contacts, with the radio loop waiting on the same lock every 20 ms.
//! Now each reads the store with the lock released, a chunk of whole records at a time
//! ([`LogRows::each_record`]), and runs the export rule it always ran over what it reads.
//!
//! # Byte-identical, by construction and by test
//!
//! The standing ruling is that an export does not change. The rules are not copied here: they
//! are [`Export`], [`Operators`] and [`Activations`] in tempo-core, the very code
//! `Logbook::adif_in_range` and its siblings run over the log in memory. What reaches them is
//! each record as the store's one decoder reads it back, in log order — and the store holds
//! exactly what memory holds (SPEC-2 v3 D2-A saved the country and state fills too).
//!
//! # A change the database does not hold yet: the file says what it is, the screen says what it lacks
//!
//! The operator's ruling (SPEC-2 v3 C15): *export with a warning* — "Write what the database
//! holds and say how many recent changes aren't in the file yet (they keep retrying). Export still
//! works as a rescue when the disk is failing."
//!
//! So an export first waits, up to [`EXPORT_WAIT`] (about ten seconds), for this process's
//! changes made before it was asked for — taken as the quit takes them ([`Unsaved`]: the tickets
//! of the changes still on their way, and the ones the writer gave up on). Then it writes the
//! store as it stands, whatever that wait found, and counts the changes still not in it for the
//! screen to say so, in two counts because they are two different news: [`Exported::saving`],
//! still on their way or being sent again, which will land; [`Exported::held`], refused by the
//! store for what they are, which will not. Nothing missing: nothing counted, nothing said.
//!
//! ⚠️ Why the changes' own tickets and not the read's freshness
//! ([`crate::logstore::Freshness`]): a stale read says only that something is missing. The
//! tickets say how many, and which of the two kinds.
//!
//! # A contact's COUNTRY follows its DXCC number
//!
//! An export writes each record as the store holds it but for one field: where the record carries
//! a DXCC number that names an entity, its ADIF `COUNTRY` is the name that number gives it
//! ([`crate::station::StationCore::dxcc_country`]), so a file never says one entity in `DXCC` and
//! another in `COUNTRY`. The record itself is not rewritten — a contact imported with a `COUNTRY`
//! its number contradicts keeps it on disk until the operator changes the contact. With no
//! resolver (a headless test's station) the file is the store's, byte for byte.
//!
//! ⚠️ Every function here reads the store: never call one under the Engine lock (a debug build
//! panics). Take the [`Source`] under it, release it, then call.

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use tempo_core::logbook::{
    Activations, Export, ExportKind, LoggedActivation, Operators, QsoRecord, RecordId,
};

use crate::logstore::{LogRows, Standing, Unsaved, READ_WAIT};
use crate::station::DxccCountry;

/// How long an export waits for the changes made before it was asked for to reach the store,
/// before it writes what the store holds and says what the file lacks — the operator's ruling,
/// "about 10 seconds": time enough for a change on its way to land, and a rescue from a failing
/// disk is not a minute's wait. (It was [`crate::logstore::DURABLE_WAIT`], a minute.)
pub const EXPORT_WAIT: Duration = Duration::from_secs(10);

/// What an export reads, taken under the Engine lock — handles, no I/O: the log's rows, this
/// process's changes that were not in the store yet when the export was asked for, and the
/// station's resolver of the country a record's DXCC number gives it.
pub struct Source {
    rows: LogRows,
    unsaved: Unsaved,
    country: Option<Arc<DxccCountry>>,
}

impl Source {
    /// The export source of the log `engine` holds. Take it under the Engine lock, and read it
    /// with the lock released.
    pub fn of(engine: &crate::engine::Engine) -> Source {
        Source {
            rows: engine.log_rows(),
            unsaved: engine.log_unsaved(),
            country: engine.station().dxcc_country.clone(),
        }
    }

    /// The export source of a store a test opened, and of the changes submitted to it.
    #[cfg(test)]
    pub(crate) fn of_store(store: &crate::logstore::LogStore) -> Source {
        Source {
            rows: LogRows::Store(store.reads()),
            unsaved: store.unsaved(),
            country: None,
        }
    }

    /// `r` as an export writes it, where that is not the record as stored: its `COUNTRY` the name
    /// its own DXCC number gives it. `None` — write the record as it is — where it carries no
    /// number, one that names no entity, or already says that name.
    fn as_written(&self, r: &QsoRecord) -> Option<QsoRecord> {
        let country = self.country.as_ref()?(r.dxcc?)?;
        if r.country.as_deref() == Some(country.as_str()) {
            return None;
        }
        let mut written = r.clone();
        written.country = Some(country);
        Some(written)
    }
}

/// The general logbook as `format` — `"csv"` (any case) for CSV, anything else ADIF — bounded to
/// the QSOs starting in `[from_unix, to_unix]` (inclusive; absent = unbounded). The Logbook's
/// Export button (`Logbook::adif_in_range` / `csv_in_range`).
pub fn export_logbook(
    from: &Source,
    format: &str,
    from_unix: Option<u64>,
    to_unix: Option<u64>,
) -> Result<Exported, String> {
    let kind = match format.to_ascii_lowercase().as_str() {
        "csv" => ExportKind::Csv {
            from: from_unix,
            to: to_unix,
        },
        _ => ExportKind::Adif {
            from: from_unix,
            to: to_unix,
        },
    };
    export_waiting(from, kind, EXPORT_WAIT)
}

/// The export range the Logbook's two date boxes give: `from` and `to` are `"YYYY-MM-DD"` UTC
/// dates, `from` the start of its day and `to` the end of its own (both inclusive). Absent or
/// empty = unbounded on that side. A malformed date is an ERROR, never silently ignored: dropping
/// a bound would ship a full log the operator believes is filtered.
pub fn day_range(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Option<u64>, Option<u64>), String> {
    let parse = |d: Option<&str>| -> Result<Option<(u64, u64)>, String> {
        match d.map(str::trim) {
            None | Some("") => Ok(None),
            Some(s) => tempo_core::logbook::day_bounds_utc(s)
                .map(Some)
                .ok_or_else(|| format!("bad export date {s:?} (expected YYYY-MM-DD)")),
        }
    };
    Ok((parse(from)?.map(|b| b.0), parse(to)?.map(|b| b.1)))
}

/// ADIF of `operator`'s contacts (`Logbook::adif_for_operator`).
pub fn export_for_operator(from: &Source, operator: &str) -> Result<Exported, String> {
    export_waiting(
        from,
        ExportKind::Operator(operator.to_string()),
        EXPORT_WAIT,
    )
}

/// ADIF of ONE activation: `reference`, on the UTC day containing `day_start_unix`, worked under
/// `callsign` (`Logbook::adif_for_activation`).
pub fn export_for_activation(
    from: &Source,
    reference: &str,
    day_start_unix: u64,
    callsign: Option<&str>,
) -> Result<Exported, String> {
    export_waiting(
        from,
        ExportKind::Activation {
            reference: reference.to_string(),
            day_start_unix,
            callsign: callsign.map(str::to_string),
        },
        EXPORT_WAIT,
    )
}

/// ADIF of exactly the contacts whose ids are `ids` — the rows the operator selected in the
/// Logbook, as the ids the Logbook holds (`LoggedQso.id`) — in log order, each record as the
/// whole-log export writes it. An id that does not parse is an ERROR, never silently dropped: a
/// file short of a contact the operator picked is a wrong file.
pub fn export_selected(from: &Source, ids: &[String]) -> Result<Exported, String> {
    let ids = ids
        .iter()
        .map(|id| {
            id.parse::<RecordId>()
                .map_err(|()| format!("bad contact id {id:?}"))
        })
        .collect::<Result<HashSet<_>, String>>()?;
    export_waiting(from, ExportKind::Selected(ids), EXPORT_WAIT)
}

/// Every distinct operator in the log, uppercased and sorted (`Logbook::operators`) — the store
/// as it stands once this process's changes have had the ordinary read's wait to land.
pub fn operators(from: &Source) -> Result<Vec<String>, String> {
    let mut found = Operators::default();
    pass(from, READ_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

/// Every distinct activation in the log, newest first (`Logbook::activations`), as
/// [`operators`].
pub fn activations(from: &Source) -> Result<Vec<LoggedActivation>, String> {
    let mut found = Activations::default();
    pass(from, READ_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

/// An export's file — the store as it stood when it was read — and this process's changes made
/// before it was asked for that were not in the store then, so are not in the file. Both counts
/// `0`: nothing to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The file.
    pub text: String,
    /// Changes the file lacks that are still being saved: on their way to the store, or refused
    /// for a reason that can pass and sent again from memory until they land.
    pub saving: usize,
    /// Changes the file lacks that the store refused for what they are: kept in memory for the
    /// session, and asked about when Nexus quits. Never in the store.
    pub held: usize,
}

/// The export `kind`, waiting at most `wait` for this process's changes to reach the store —
/// what a test shortens.
pub(crate) fn export_waiting(
    from: &Source,
    kind: ExportKind,
    wait: Duration,
) -> Result<Exported, String> {
    let mut e = Export::new(kind);
    let lacking = pass(from, wait, &mut |r| match from.as_written(r) {
        Some(written) => e.add(&written),
        None => e.add(r),
    })?;
    Ok(Exported {
        text: e.finish(),
        saving: lacking.pending + lacking.retryable,
        held: lacking.refused,
    })
}

/// Every record of the log, whole and in log order, through `take`, once this process's changes
/// have had `wait` to reach the store — and where the ones that had not stand.
fn pass(
    from: &Source,
    wait: Duration,
    take: &mut dyn FnMut(&tempo_core::logbook::QsoRecord),
) -> Result<Standing, String> {
    // What the STORE lacks, which is what the read below can lack: on the 1.13 path a change
    // still on its way to `log.adi` is in the store already, and in the file this writes.
    let lacking = from.unsaved.wait_stored(wait);
    // The wait for them is over: the read takes the store as it stands, without a wait of its
    // own for changes this one has already waited on.
    from.rows
        .each_record(Duration::ZERO, &mut |r| {
            take(r);
            ControlFlow::Continue(())
        })
        .map_err(|e| format!("The logbook could not be read for the export: {e}"))?;
    Ok(lacking)
}
