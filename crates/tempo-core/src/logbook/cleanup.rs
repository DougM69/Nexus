//! The Logbook's clean-up tools, as rules on records: which contacts are one contact logged twice,
//! what merging them keeps, what a bulk edit sets or clears, and the formats a check holds a
//! contact's grid and references to. Pure — the app reads the rows, decides with these, and
//! makes the change through the log's one write path, recorded in the history
//! (`tempo_app::logtools`).
//!
//! # Duplicates ([`dupe_groups`])
//!
//! Two contacts are one logged twice by **the log's own duplicate rule**
//! ([`super::dedup::is_recent_duplicate`]): the same station (base call, case aside), band and
//! mode, within [`super::dedup::DEDUP_WINDOW_SECS`]. The wider "possible" tier asks the same
//! question within [`POSSIBLE_WINDOW_SECS`]. A group is every contact chained to another by the
//! window, so three copies a few minutes apart are one group.
//!
//! # A merge keeps everything it can ([`merge`])
//!
//! The kept contact is one of the group, its identity (id, call, band, mode, time) as it is. Every
//! other field is the union of the copies:
//!
//! - a field one copy holds and the kept one lacks is taken;
//! - **confirmations are unioned** — each channel's, `confirmed`, `award_confirmed`, credit granted
//!   and submitted — never taken off;
//! - **upload stamps are unioned**: a service only one copy was sent to keeps that stamp; where
//!   both were, the later attempt stands ([`super::UploadState::merge_recent`], the rule two
//!   windows sharing one log already use);
//! - a field two copies hold DIFFERENT values for is a [`Conflict`]: the merged contact takes the
//!   value the caller chose, else the kept copy's (else the first copy's that has one). A conflict
//!   is **guarded** — it must be chosen, never defaulted — when it is the contest exchange, the
//!   QSL-sent mark, or a value a confirmed copy carries: a confirmation's own detail (a grid, a
//!   state, a zone LoTW sent) is never overwritten without the operator's say.
//!
//! The copies that are not kept are deleted by the merge, whole, into the history: nothing a
//! copy held is lost, and the merge is undone as one run.
//!
//! [`merge`] names every field of [`QsoRecord`] in one destructuring, without `..`: a field added
//! to the record does not compile here until it is given a rule, so no field is merged by being
//! forgotten.
//!
//! # A bulk edit ([`changed`])
//!
//! Sets or clears the fields in [`BULK_FIELDS`] — the operator's own station details, the
//! propagation mode, and the operator's park or summit. **Never the call**: a bulk edit changes no
//! callsign, so by the edit rule ([`super::edited`]) it keeps every confirmation and every upload
//! stamp, and queues nothing to any service. The marks that say Nexus or a callbook filled a field
//! come off a field it changes, as an edit takes them off. A park or summit is set only on a
//! contact whose side holds no reference or one of the same programme: another programme's
//! reference is never replaced by it, and such a contact is left as it is.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::callbook::{self, CALLBOOK_FILLED_FIELD};
use super::{
    canonical, extra_value, self_filled, set_extra, set_self_filled, QsoRecord, RecordId,
    SELF_FILLED_FIELD,
};
use crate::message::base_call;

// ── duplicates ─────────────────────────────────────────────────────────────────────────────────

/// The "possible duplicate" tier's window: the same station, band and mode within an hour.
pub const POSSIBLE_WINDOW_SECS: u64 = 3_600;

/// What the duplicate scan reads of a contact.
#[derive(Debug, Clone)]
pub struct DupeKey {
    pub id: RecordId,
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
}

/// The groups of contacts that are one contact logged more than once: the same station (base
/// call, case aside — [`crate::message::same_call`]), band and mode (case aside), each within
/// `window_secs` of the one before it. Each group in time order, two or more contacts; the groups
/// newest first. A contact with no call is in none.
pub fn dupe_groups(keys: Vec<DupeKey>, window_secs: u64) -> Vec<Vec<RecordId>> {
    type Slot = (String, String, String);
    let mut keyed: Vec<(Slot, u64, RecordId)> = keys
        .into_iter()
        .filter(|k| !k.call.trim().is_empty())
        .map(|k| {
            let slot = (
                base_call(&k.call),
                k.band.to_ascii_lowercase(),
                k.mode.to_ascii_lowercase(),
            );
            (slot, k.when_unix, k.id)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut groups: Vec<(u64, Vec<RecordId>)> = Vec::new();
    let mut current: Vec<RecordId> = Vec::new();
    let mut latest = 0;
    for (i, (slot, when, id)) in keyed.iter().enumerate() {
        let joins = i > 0 && keyed[i - 1].0 == *slot && when - keyed[i - 1].1 <= window_secs;
        if !joins {
            if current.len() > 1 {
                groups.push((latest, std::mem::take(&mut current)));
            }
            current.clear();
        }
        current.push(*id);
        latest = *when;
    }
    if current.len() > 1 {
        groups.push((latest, current));
    }
    groups.sort_by(|a, b| b.0.cmp(&a.0));
    groups.into_iter().map(|(_, g)| g).collect()
}

// ── merge ──────────────────────────────────────────────────────────────────────────────────────

/// A field the copies of a contact hold different values for.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// The field: its ADIF name, or for a pair that goes together the name of its first —
    /// `DXCC` (with `COUNTRY`), `FREQ` (with `FREQ_RX`), `PROP_MODE` (with `SAT_NAME`), `MY_SIG`
    /// and `SIG` (a side's programme with its reference), `QSL_SENT`, `CONTEST` (the whole
    /// exchange).
    pub tag: String,
    /// Each copy holding a value, and the value as shown.
    pub values: Vec<(RecordId, String)>,
    /// Must be chosen: never defaulted (see the module header).
    pub guarded: bool,
    /// The copy whose value the merged contact takes.
    pub chosen: RecordId,
}

/// What merging the copies of a contact makes: the contact, and the fields they disagreed on.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub record: QsoRecord,
    pub conflicts: Vec<Conflict>,
}

impl Merged {
    /// The guarded conflicts `choices` does not choose for: the merge must not be made until
    /// each is chosen.
    pub fn unchosen(&self, choices: &HashMap<String, RecordId>) -> Vec<&str> {
        self.conflicts
            .iter()
            .filter(|c| c.guarded && !choices.contains_key(&c.tag))
            .map(|c| c.tag.as_str())
            .collect()
    }
}

/// A field's text, trimmed; `None` when blank.
fn text(v: &Option<String>) -> Option<String> {
    v.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// A field — or a pair that goes together — as the merge compares and copies it: its value as
/// compared and as shown (`None` when the copy holds none), and the copy of it from one record
/// into another.
struct Part {
    tag: &'static str,
    get: fn(&QsoRecord) -> Option<(String, String)>,
    put: fn(&mut QsoRecord, &QsoRecord),
}

/// A value compared as it is shown.
fn same(v: Option<String>) -> Option<(String, String)> {
    v.map(|v| (v.clone(), v))
}

/// A value compared case aside — a grid, a call.
fn caseless(v: Option<String>) -> Option<(String, String)> {
    v.map(|v| (v.to_ascii_uppercase(), v))
}

fn parts() -> Vec<Part> {
    vec![
        Part {
            tag: "GRIDSQUARE",
            get: |r| caseless(text(&r.grid)),
            put: |o, s| o.grid = s.grid.clone(),
        },
        Part {
            tag: "DXCC",
            get: |r| {
                let country = text(&r.country);
                (r.dxcc.is_some() || country.is_some()).then(|| {
                    let shown = match (r.dxcc, &country) {
                        (Some(n), Some(c)) => format!("{n} {c}"),
                        (Some(n), None) => n.to_string(),
                        (None, Some(c)) => c.clone(),
                        (None, None) => String::new(),
                    };
                    (shown.to_ascii_uppercase(), shown)
                })
            },
            put: |o, s| {
                o.dxcc = s.dxcc;
                o.country = s.country.clone();
            },
        },
        Part {
            tag: "STATE",
            get: |r| caseless(text(&r.state)),
            put: |o, s| o.state = s.state.clone(),
        },
        Part {
            tag: "NAME",
            get: |r| same(text(&r.name)),
            put: |o, s| o.name = s.name.clone(),
        },
        Part {
            tag: "QTH",
            get: |r| same(text(&r.qth)),
            put: |o, s| o.qth = s.qth.clone(),
        },
        Part {
            tag: "COMMENT",
            get: |r| same(text(&r.comment)),
            put: |o, s| o.comment = s.comment.clone(),
        },
        Part {
            tag: "NOTES",
            get: |r| same(text(&r.notes)),
            put: |o, s| o.notes = s.notes.clone(),
        },
        Part {
            tag: "RST_SENT",
            get: |r| same(text(&r.rst_sent)),
            put: |o, s| o.rst_sent = s.rst_sent.clone(),
        },
        Part {
            tag: "RST_RCVD",
            get: |r| same(text(&r.rst_rcvd)),
            put: |o, s| o.rst_rcvd = s.rst_rcvd.clone(),
        },
        Part {
            tag: "TX_PWR",
            get: |r| same(r.tx_power.map(|p| p.to_string())),
            put: |o, s| o.tx_power = s.tx_power,
        },
        Part {
            tag: "FREQ",
            get: |r| {
                let rx = r
                    .freq_rx_mhz
                    .map(|f| format!(" / {f:.6}"))
                    .unwrap_or_default();
                (r.freq_mhz > 0.0 || r.freq_rx_mhz.is_some())
                    .then(|| format!("{:.6}{rx}", r.freq_mhz))
                    .map(|v| (v.clone(), v))
            },
            put: |o, s| {
                o.freq_mhz = s.freq_mhz;
                o.freq_rx_mhz = s.freq_rx_mhz;
            },
        },
        Part {
            tag: "TIME_OFF",
            get: |r| same(r.time_off_unix.map(|t| t.to_string())),
            // An end time before the kept contact's start is no end time of it.
            put: |o, s| {
                o.time_off_unix = s.time_off_unix.filter(|&off| off >= o.when_unix);
            },
        },
        Part {
            tag: "PROP_MODE",
            get: |r| {
                let (p, s) = (text(&r.prop_mode), text(&r.sat_name));
                (p.is_some() || s.is_some()).then(|| {
                    let shown = format!("{} {}", p.unwrap_or_default(), s.unwrap_or_default())
                        .trim()
                        .to_string();
                    (shown.to_ascii_uppercase(), shown)
                })
            },
            put: |o, s| {
                o.prop_mode = s.prop_mode.clone();
                o.sat_name = s.sat_name.clone();
            },
        },
        Part {
            tag: "OPERATOR",
            get: |r| caseless(text(&r.operator)),
            put: |o, s| o.operator = s.operator.clone(),
        },
        Part {
            tag: "STATION_CALLSIGN",
            get: |r| caseless(text(&r.station_callsign)),
            put: |o, s| o.station_callsign = s.station_callsign.clone(),
        },
        Part {
            tag: "MY_GRIDSQUARE",
            get: |r| caseless(text(&r.my_grid)),
            put: |o, s| o.my_grid = s.my_grid.clone(),
        },
        Part {
            tag: "MY_RIG",
            get: |r| same(text(&r.my_rig)),
            put: |o, s| o.my_rig = s.my_rig.clone(),
        },
        Part {
            tag: "MY_SIG",
            get: |r| side(&r.ota.my_program, &r.ota.my_ref),
            put: |o, s| {
                o.ota.my_program = s.ota.my_program.clone();
                o.ota.my_ref = s.ota.my_ref.clone();
            },
        },
        Part {
            tag: "SIG",
            get: |r| side(&r.ota.their_program, &r.ota.their_ref),
            put: |o, s| {
                o.ota.their_program = s.ota.their_program.clone();
                o.ota.their_ref = s.ota.their_ref.clone();
            },
        },
        Part {
            tag: "IOTA",
            get: |r| caseless(text(&r.ota.iota)),
            put: |o, s| o.ota.iota = s.ota.iota.clone(),
        },
        Part {
            tag: "QSL_SENT",
            get: |r| {
                r.qsl_sent.sent.then(|| {
                    let via = r
                        .qsl_sent
                        .via
                        .map(|v| format!(" {v:?}"))
                        .unwrap_or_default();
                    let date = r
                        .qsl_sent
                        .date_unix
                        .map(|d| format!(" {}", super::query::fmt_utc(d)))
                        .unwrap_or_default();
                    let shown = format!("Y{via}{date}");
                    (shown.clone(), shown)
                })
            },
            put: |o, s| o.qsl_sent = s.qsl_sent,
        },
        Part {
            tag: "CONTEST",
            get: |r| {
                r.contest.as_deref().map(|c| {
                    let sent = c
                        .stx_string
                        .clone()
                        .or(c.stx.map(|n| n.to_string()))
                        .unwrap_or_else(|| exchange(&c.sent));
                    let rcvd = c
                        .srx_string
                        .clone()
                        .or(c.srx.map(|n| n.to_string()))
                        .unwrap_or_else(|| exchange(&c.rcvd));
                    (
                        format!("{c:?}"),
                        format!("{} {sent} / {rcvd}", c.contest_id)
                            .trim()
                            .to_string(),
                    )
                })
            },
            put: |o, s| o.contest = s.contest.clone(),
        },
    ]
}

/// One side's park, summit or other programme, as compared and shown.
fn side(program: &Option<String>, reference: &Option<String>) -> Option<(String, String)> {
    let (p, r) = (text(program), text(reference));
    (p.is_some() || r.is_some()).then(|| {
        let shown = format!("{} {}", p.unwrap_or_default(), r.unwrap_or_default())
            .trim()
            .to_string();
        (shown.to_ascii_uppercase(), shown)
    })
}

/// A contest exchange's slots, as shown.
fn exchange(slots: &[(String, String)]) -> String {
    slots
        .iter()
        .map(|(_, v)| v.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The passthrough fields that are marks about OTHER fields: they follow the values they mark,
/// never merged as values of their own.
fn is_mark(name: &str) -> bool {
    name == SELF_FILLED_FIELD || name == CALLBOOK_FILLED_FIELD
}

/// How much a copy holds: the default copy to keep is the fullest.
fn fullness(r: &QsoRecord) -> usize {
    let held = parts().iter().filter(|p| (p.get)(r).is_some()).count();
    let confirmed = [
        r.qsl_rcvd.card,
        r.qsl_rcvd.lotw,
        r.qsl_rcvd.eqsl,
        r.qsl_rcvd.qrz,
    ]
    .into_iter()
    .filter(|c| *c)
    .count();
    held + r.extra.iter().filter(|(k, _)| !is_mark(k)).count() + 2 * confirmed
}

/// The copy a merge keeps unless the operator picks another: the fullest — the most fields,
/// confirmations counted twice — and of equally full ones the first in log order.
pub fn suggested_keep(copies: &[QsoRecord]) -> usize {
    let mut best = 0;
    for (i, r) in copies.iter().enumerate() {
        if fullness(r) > fullness(&copies[best]) {
            best = i;
        }
    }
    best
}

/// Whether a copy carries a confirmation: what it holds is a confirmation's detail.
fn confirmed(r: &QsoRecord) -> bool {
    r.confirmed || r.award_confirmed || r.qsl_rcvd.any()
}

/// ★ The copies of one contact merged into the copy `keep` (see the module header). `copies` in
/// log order; `choices` names, for a conflicting field, the copy whose value the merged contact
/// takes. The merged contact keeps `copies[keep]`'s id.
pub fn merge(copies: &[QsoRecord], keep: usize, choices: &HashMap<String, RecordId>) -> Merged {
    let kept = &copies[keep];
    // The kept copy first: its value is the default wherever it has one.
    let order: Vec<&QsoRecord> = std::iter::once(kept)
        .chain(
            copies
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != keep)
                .map(|(_, r)| r),
        )
        .collect();
    let id_of = |r: &QsoRecord| r.id.expect("a contact the log holds carries an id");
    let pick = |tag: &str, holding: &[&QsoRecord]| -> usize {
        choices
            .get(tag)
            .and_then(|want| holding.iter().position(|r| id_of(r) == *want))
            .unwrap_or(0)
    };
    let mut out = kept.clone();
    let mut conflicts = Vec::new();

    for part in parts() {
        let holding: Vec<(&QsoRecord, (String, String))> = order
            .iter()
            .filter_map(|r| (part.get)(r).map(|v| (*r, v)))
            .collect();
        let Some((_, first)) = holding.first() else {
            continue;
        };
        let differ = holding.iter().any(|(_, v)| v.0 != first.0);
        let records: Vec<&QsoRecord> = holding.iter().map(|(r, _)| *r).collect();
        let from = if differ { pick(part.tag, &records) } else { 0 };
        (part.put)(&mut out, records[from]);
        if differ {
            let guarded =
                matches!(part.tag, "CONTEST" | "QSL_SENT") || records.iter().any(|r| confirmed(r));
            conflicts.push(Conflict {
                tag: part.tag.to_string(),
                values: holding
                    .iter()
                    .map(|(r, v)| (id_of(r), v.1.clone()))
                    .collect(),
                guarded,
                chosen: id_of(records[from]),
            });
        }
    }

    // The passthrough fields, one by one, as the parts are: the marks follow the values below.
    let mut names: Vec<&str> = order
        .iter()
        .flat_map(|r| r.extra.iter().map(|(k, _)| k.as_str()))
        .filter(|k| !is_mark(k))
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut extra: Vec<(String, String)> = Vec::new();
    for name in names {
        let holding: Vec<(&QsoRecord, &str)> = order
            .iter()
            .filter_map(|r| extra_value(r, name).map(|v| (*r, v)))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let Some(&(_, first)) = holding.first() else {
            // Held empty by every copy that holds it: kept as the kept copy holds it, or as the
            // first holds it.
            if let Some((k, v)) = order
                .iter()
                .find_map(|r| r.extra.iter().find(|(k, _)| k == name))
            {
                extra.push((k.clone(), v.clone()));
            }
            continue;
        };
        let differ = holding.iter().any(|(_, v)| *v != first);
        let records: Vec<&QsoRecord> = holding.iter().map(|(r, _)| *r).collect();
        let from = if differ { pick(name, &records) } else { 0 };
        let value = holding[from].1;
        extra.push((name.to_string(), value.to_string()));
        if differ {
            conflicts.push(Conflict {
                tag: name.to_string(),
                values: holding
                    .iter()
                    .map(|(r, v)| (id_of(r), v.to_string()))
                    .collect(),
                guarded: records.iter().any(|r| confirmed(r)),
                chosen: id_of(records[from]),
            });
        }
    }
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    out.extra = extra;
    // The marks: a field is marked filled by Nexus or by a callbook when a copy that marked it
    // holds the value the merged contact now holds.
    let mut own: Vec<String> = Vec::new();
    let mut looked: Vec<String> = Vec::new();
    for r in &order {
        for tag in self_filled(r) {
            if mark_value(r, tag) == mark_value(&out, tag) && mark_value(r, tag).is_some() {
                own.push(tag.to_string());
            }
        }
        for tag in callbook::callbook_filled(r) {
            if mark_value(r, tag) == mark_value(&out, tag) && mark_value(r, tag).is_some() {
                looked.push(tag.to_string());
            }
        }
    }
    set_self_filled(&mut out, own);
    looked.sort();
    looked.dedup();
    set_extra(
        &mut out,
        CALLBOOK_FILLED_FIELD,
        (!looked.is_empty()).then(|| looked.join(",")),
    );

    // ★ Every field of the record, named — no `..`: a field added to `QsoRecord` does not compile
    // here until it has a rule. The rules, field by field:
    let QsoRecord {
        // The kept copy's identity: the contact the merge keeps.
        id: _,
        call: _,
        band: _,
        mode: _,
        when_unix: _,
        time_known: _,
        // The parts above.
        grid: _,
        country: _,
        state: _,
        freq_mhz: _,
        freq_rx_mhz: _,
        rst_sent: _,
        rst_rcvd: _,
        name: _,
        qth: _,
        comment: _,
        notes: _,
        tx_power: _,
        time_off_unix: _,
        qsl_sent: _,
        ota: _,
        dxcc: _,
        prop_mode: _,
        sat_name: _,
        operator: _,
        station_callsign: _,
        my_grid: _,
        my_rig: _,
        contest: _,
        // The passthrough, above.
        extra: _,
        // Unioned, below.
        confirmed: _,
        award_confirmed: _,
        qsl_rcvd: _,
        credit_granted: _,
        credit_submitted: _,
        upload: _,
    } = &out;
    for r in &order[1..] {
        out.confirmed |= r.confirmed;
        out.award_confirmed |= r.award_confirmed;
        out.qsl_rcvd.merge(r.qsl_rcvd);
        out.credit_granted.extend(r.credit_granted.iter().cloned());
        out.credit_submitted
            .extend(r.credit_submitted.iter().cloned());
        out.upload.merge_recent(&r.upload);
    }
    for list in [&mut out.credit_granted, &mut out.credit_submitted] {
        list.sort();
        list.dedup();
    }
    Merged {
        record: out,
        conflicts,
    }
}

/// The value a fill mark names, as [`callbook`] and the self-filled list read it.
fn mark_value(r: &QsoRecord, tag: &str) -> Option<String> {
    let v = match tag {
        "NAME" => r.name.as_deref(),
        "QTH" => r.qth.as_deref(),
        "STATE" => r.state.as_deref(),
        "GRIDSQUARE" => r.grid.as_deref(),
        "MY_GRIDSQUARE" => r.my_grid.as_deref(),
        "IOTA" => r.ota.iota.as_deref(),
        _ => extra_value(r, tag),
    };
    v.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_ascii_uppercase)
}

// ── bulk edit and fixes ────────────────────────────────────────────────────────────────────────

/// A field the clean-up tools set or clear, by its ADIF name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Field {
    MyGridsquare,
    MyRig,
    Operator,
    StationCallsign,
    PropMode,
    SatName,
    TxPwr,
    MyPotaRef,
    MySotaRef,
    // The check's fixes, beside the above.
    Gridsquare,
    Band,
    Dxcc,
    Iota,
    PotaRef,
    SotaRef,
    TimeOff,
}

/// The fields a bulk edit offers. Never the call: see the module header.
pub const BULK_FIELDS: [Field; 9] = [
    Field::MyGridsquare,
    Field::MyRig,
    Field::Operator,
    Field::StationCallsign,
    Field::PropMode,
    Field::SatName,
    Field::TxPwr,
    Field::MyPotaRef,
    Field::MySotaRef,
];

/// One field set (`Some`) or cleared (`None`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub field: Field,
    pub value: Option<String>,
}

/// A field's value on a contact as the tools compare it — what a fix expects to find.
pub fn value_of(r: &QsoRecord, field: Field) -> Option<String> {
    let of_side = |program: &Option<String>, reference: &Option<String>, want: &str| {
        program
            .as_deref()
            .filter(|p| p.eq_ignore_ascii_case(want))
            .and(reference.clone())
    };
    let v = match field {
        Field::MyGridsquare => r.my_grid.clone(),
        Field::MyRig => r.my_rig.clone(),
        Field::Operator => r.operator.clone(),
        Field::StationCallsign => r.station_callsign.clone(),
        Field::PropMode => r.prop_mode.clone(),
        Field::SatName => r.sat_name.clone(),
        Field::TxPwr => r.tx_power.map(|p| p.to_string()),
        Field::MyPotaRef => of_side(&r.ota.my_program, &r.ota.my_ref, "POTA"),
        Field::MySotaRef => of_side(&r.ota.my_program, &r.ota.my_ref, "SOTA"),
        Field::Gridsquare => r.grid.clone(),
        Field::Band => Some(r.band.clone()),
        Field::Dxcc => r.dxcc.map(|n| n.to_string()),
        Field::Iota => r.ota.iota.clone(),
        Field::PotaRef => of_side(&r.ota.their_program, &r.ota.their_ref, "POTA"),
        Field::SotaRef => of_side(&r.ota.their_program, &r.ota.their_ref, "SOTA"),
        Field::TimeOff => r.time_off_unix.map(|t| t.to_string()),
    };
    v.filter(|v| !v.is_empty())
}

/// A Maidenhead locator of 2, 4, 6 or 8 characters, case aside: field letters A–R, square
/// digits, subsquare letters A–X, extended square digits.
pub fn valid_grid(g: &str) -> bool {
    let b = g.as_bytes();
    if !matches!(b.len(), 2 | 4 | 6 | 8) {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        0 | 1 => matches!(c.to_ascii_uppercase(), b'A'..=b'R'),
        2 | 3 | 6 | 7 => c.is_ascii_digit(),
        _ => matches!(c.to_ascii_uppercase(), b'A'..=b'X'),
    })
}

/// A POTA park reference: a 1–4 character prefix with a letter, a hyphen, 4 or 5 digits
/// (`US-1234`, `K-0001`, `VE-12345`). Several, comma-separated, for a two-fer.
pub fn valid_pota(r: &str) -> bool {
    each_ref(r, |one| {
        let Some((prefix, num)) = one.split_once('-') else {
            return false;
        };
        (1..=4).contains(&prefix.len())
            && prefix
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && prefix.bytes().any(|c| c.is_ascii_uppercase())
            && (4..=5).contains(&num.len())
            && num.bytes().all(|c| c.is_ascii_digit())
    })
}

/// A SOTA summit reference: association, `/`, a two-character region, a hyphen, three digits
/// (`W7W/LC-001`, `G/LD-001`, `VK3/VE-001`).
pub fn valid_sota(r: &str) -> bool {
    each_ref(r, |one| {
        let Some((assoc, rest)) = one.split_once('/') else {
            return false;
        };
        let Some((region, num)) = rest.split_once('-') else {
            return false;
        };
        (1..=4).contains(&assoc.len())
            && assoc
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && region.len() == 2
            && region
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && num.len() == 3
            && num.bytes().all(|c| c.is_ascii_digit())
    })
}

/// An IOTA reference: a continent (AF, AN, AS, EU, NA, OC, SA), a hyphen, three digits.
pub fn valid_iota(r: &str) -> bool {
    super::valid_iota(r).is_some_and(|v| v == r)
}

fn each_ref(r: &str, one: impl Fn(&str) -> bool) -> bool {
    !r.is_empty() && r.split(',').all(|p| one(p.trim()))
}

/// A reference spelled the way its format wants it, where that is all that is wrong: trimmed,
/// upper case, a space or underscore read as the hyphen, and a POTA reference missing its hyphen
/// given one (`us1234` → `US-1234`). `None` when even that is not a valid reference.
pub fn respelled(r: &str, valid: fn(&str) -> bool) -> Option<String> {
    let parts: Vec<String> = r
        .split(',')
        .map(|p| {
            let p = p.trim().to_ascii_uppercase().replace([' ', '_'], "-");
            if valid(&p) {
                return p;
            }
            // A park missing its hyphen: the letters, then the digits.
            let at = p.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            let (prefix, num) = p.split_at(at);
            if !prefix.is_empty()
                && prefix.bytes().all(|c| c.is_ascii_uppercase())
                && num.bytes().all(|c| c.is_ascii_digit())
            {
                format!("{prefix}-{num}")
            } else {
                p
            }
        })
        .collect();
    let out = parts.join(",");
    valid(&out).then_some(out)
}

/// Why a change cannot be made as asked, before any contact is read: a value of the wrong form.
fn checked_value(field: Field, value: &str) -> Result<String, String> {
    let v = value.trim();
    let bad = |what: &str| Err(format!("'{v}' is not {what}."));
    match field {
        Field::MyGridsquare | Field::Gridsquare if !valid_grid(v) => bad("a grid square"),
        Field::TxPwr if !v.parse::<f64>().is_ok_and(|w| w.is_finite() && w >= 0.0) => {
            bad("a power in watts")
        }
        Field::Dxcc if v.parse::<u32>().is_err() => bad("a DXCC entity number"),
        Field::TimeOff if v.parse::<u64>().is_err() => bad("a time"),
        Field::MyPotaRef | Field::PotaRef if !valid_pota(&v.to_ascii_uppercase()) => {
            bad("a POTA park reference")
        }
        Field::MySotaRef | Field::SotaRef if !valid_sota(&v.to_ascii_uppercase()) => {
            bad("a SOTA summit reference")
        }
        Field::Iota if !valid_iota(&v.to_ascii_uppercase()) => bad("an IOTA reference"),
        Field::Band if v.is_empty() => bad("a band"),
        _ if v.is_empty() => Err("A value to set cannot be blank; clear the field instead.".into()),
        Field::Operator | Field::StationCallsign | Field::PropMode => Ok(v.to_ascii_uppercase()),
        Field::MyPotaRef | Field::PotaRef | Field::MySotaRef | Field::SotaRef | Field::Iota => {
            Ok(v.to_ascii_uppercase())
        }
        _ => Ok(v.to_string()),
    }
}

/// `changes` in the form they are made: each value checked and spelled as stored. `Err` names the
/// first value that is not of its field's form.
pub fn checked(changes: &[Change]) -> Result<Vec<Change>, String> {
    changes
        .iter()
        .map(|c| {
            Ok(Change {
                field: c.field,
                value: c
                    .value
                    .as_deref()
                    .map(|v| checked_value(c.field, v))
                    .transpose()?,
            })
        })
        .collect()
}

/// Set one side's programme reference: on a side holding none, or one of the same programme;
/// another programme's is left as it is. A clear takes only that programme's reference.
fn set_side(
    program: &mut Option<String>,
    reference: &mut Option<String>,
    want: &str,
    value: &Option<String>,
) {
    let mine = program
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case(want));
    let empty = text(program).is_none() && text(reference).is_none();
    match value {
        Some(v) if mine || empty => {
            *program = Some(want.to_string());
            *reference = Some(v.clone());
        }
        None if mine => {
            *program = None;
            *reference = None;
        }
        _ => {}
    }
}

/// ★ The contact `row` with `changes` made (each already [`checked`]), as an edit makes them: the
/// fill marks off a field changed, a side's left-over references gone with the side, the band in
/// the log's one form. `country` names the `COUNTRY` a DXCC number gives the contact, as every
/// change of the number writes it. `None` when nothing about the contact changes.
pub fn changed(
    row: &QsoRecord,
    changes: &[Change],
    country: &dyn Fn(&str, Option<u32>) -> Option<String>,
) -> Option<QsoRecord> {
    let mut rec = row.clone();
    for c in changes {
        let v = c.value.clone();
        match c.field {
            Field::MyGridsquare => rec.my_grid = v,
            Field::MyRig => rec.my_rig = v,
            Field::Operator => rec.operator = v,
            Field::StationCallsign => rec.station_callsign = v,
            Field::PropMode => rec.prop_mode = v,
            Field::SatName => rec.sat_name = v,
            Field::TxPwr => rec.tx_power = v.and_then(|w| w.parse().ok()),
            Field::MyPotaRef => set_side(&mut rec.ota.my_program, &mut rec.ota.my_ref, "POTA", &v),
            Field::MySotaRef => set_side(&mut rec.ota.my_program, &mut rec.ota.my_ref, "SOTA", &v),
            Field::PotaRef => set_side(
                &mut rec.ota.their_program,
                &mut rec.ota.their_ref,
                "POTA",
                &v,
            ),
            Field::SotaRef => set_side(
                &mut rec.ota.their_program,
                &mut rec.ota.their_ref,
                "SOTA",
                &v,
            ),
            Field::Gridsquare => rec.grid = v,
            Field::Band => rec.band = v.unwrap_or_default(),
            Field::Dxcc => {
                rec.dxcc = v.and_then(|n| n.parse().ok());
                rec.country = country(&rec.call, rec.dxcc).or(rec.country.clone());
            }
            Field::Iota => rec.ota.iota = v,
            Field::TimeOff => rec.time_off_unix = v.and_then(|t| t.parse().ok()),
        }
    }
    // What an edit does beside its fields (`super::edited`), for the fields these change.
    let grid_of = |g: &Option<String>| g.as_deref().map(|g| g.trim().to_ascii_uppercase());
    if grid_of(&rec.my_grid) != grid_of(&row.my_grid) {
        let tags = self_filled(&rec)
            .into_iter()
            .filter(|t| *t != "MY_GRIDSQUARE")
            .map(str::to_string)
            .collect();
        set_self_filled(&mut rec, tags);
    }
    callbook::unmark_edited(row, &mut rec);
    if (&rec.ota.my_program, &rec.ota.my_ref) != (&row.ota.my_program, &row.ota.my_ref) {
        rec.extra.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "MY_SIG" | "MY_SIG_INFO" | "MY_SOTA_REF" | "MY_POTA_REF"
            )
        });
    }
    if (&rec.ota.their_program, &rec.ota.their_ref) != (&row.ota.their_program, &row.ota.their_ref)
    {
        rec.extra
            .retain(|(k, _)| !matches!(k.as_str(), "SIG" | "SIG_INFO" | "SOTA_REF" | "POTA_REF"));
    }
    canonical::contact(&mut rec);
    (rec != *row).then_some(rec)
}

#[cfg(test)]
mod tests;
