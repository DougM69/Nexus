//! A Cabrillo contest log read as the ADIF fields of its contacts.
//!
//! # The line
//!
//! Cabrillo 3.0's QSO data (WWROF, <https://wwrof.org/cabrillo/cabrillo-qso-data/>):
//!
//! ```text
//! QSO: freq mo date time call rst exch call rst exch t
//! ```
//!
//! *"freq: 1800 or actual frequency in kHz"*, and for 50 MHz and up a band token (`50`, `70`,
//! `144`, `222`, `432`, `902`, `1.2G` … `241G`, `LIGHT`); *"mo: CW, PH, FM, RY, DG"*; the date
//! `yyyy-mm-dd` and the time `nnnn`, both UTC; the exchange *"contest-specific"*; `t` the
//! transmitter id of a multi-transmitter entry. `X-QSO:` lines are QSOs the entrant left out of
//! the score.
//!
//! # The exchange: the contest's own template, from the rules table
//!
//! The columns between the two calls are read with the template of the contest the `CONTEST:`
//! header names — the SAME ruleset Nexus writes that contest's Cabrillo file with
//! ([`crate::fieldday::FieldDayLog::cabrillo_with`]): its own fixed columns where the sponsor's
//! template declares them (`cabrillo_columns`), else each role's send order, the callsign slot
//! left out because the call is the line's structural column
//! ([`crate::contest::side_declares_call`]). Each value goes to the ADIF field that slot carries in
//! that direction ([`crate::contest::directed_columns`]'s tags: `SRX`/`STX`, `ARRL_SECT`/
//! `MY_ARRL_SECT`, `CQZ`/`MY_CQ_ZONE`, `RST_RCVD`/`RST_SENT`, `CLASS`…). The templates those
//! rulesets were checked against:
//!
//! * ARRL Sweepstakes — ARRL's Cabrillo Format & Tutorial,
//!   <https://www.arrl.org/cabrillo-format-tutorial> (`… W1AW 1 M 38 CT K8MM 1 Q 92 MI`);
//! * CQ World Wide DX — <https://www.cqww.com/cabrillo.htm>
//!   (`QSO: freq mo date time call rst exch call rst exch t`, the exchange the CQ zone);
//! * CQ WPX — <https://www.cqwpx.com/cabrillo.htm> (the same columns, the exchange a serial);
//! * ARRL Field Day and Winter Field Day — class and section (`… W9XYZ 3A WI K1ABC 2A CT`);
//! * and every other contest in the loaded rules table (the QSO parties, ARRL VHF).
//!
//! The whole of each side's exchange is also kept as text, in ADIF's `STX_STRING` and
//! `SRX_STRING` ("contest QSO transmitted / received information"), so a column with no field of
//! its own (Sweepstakes' precedence and check, Field Day's sent class) is still in the log; each
//! such column is listed ([`UnmappedReason::ExchangeText`]).
//!
//! # A contest with no template: what can be known, and nothing guessed
//!
//! The call, band, mode and time, and an RST where the first column of a side looks like one
//! (it is listed as read that way, [`NoteKind::UnknownContest`]); the rest kept as the exchange
//! text. Which column is the other station's call is found by the template's own symmetry (each
//! side the same width, a trailing transmitter id or not), checked against a call's shape. A line
//! that fits no reading is not imported, and is listed ([`NoteKind::UnreadableLine`]).

use super::{Note, NoteKind, SourceRecord, Unmapped, UnmappedReason};
use crate::contest::spec::ExchangeSpec;
use crate::contest::{cabrillo_contest_token, is_call_slot};
use crate::fd_rules::{ruleset_by_contest_id, FdRuleset};

/// A Cabrillo log, read.
pub(crate) struct Read {
    pub(crate) records: Vec<SourceRecord>,
    pub(crate) notes: Vec<Note>,
    pub(crate) unmapped: Vec<Unmapped>,
    pub(crate) template: Option<String>,
}

/// The ADIF contest ids whose Cabrillo `CONTEST` token is spelled otherwise
/// ([`cabrillo_contest_token`] maps each), so a file carrying the token finds its ruleset.
const RESPELLED: &[&str] = &[
    "ARRL-FIELD-DAY",
    "OH-QSO-PARTY",
    "TX-QSO-PARTY",
    "IL QSO Party",
];

/// The ruleset a Cabrillo `CONTEST` token names, for a log of `year`.
pub(super) fn ruleset_for(token: &str, year: u16) -> Option<&'static FdRuleset> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    ruleset_by_contest_id(token, year).or_else(|| {
        RESPELLED
            .iter()
            .find(|id| {
                cabrillo_contest_token(id).eq_ignore_ascii_case(token)
                    // The hyphenated Illinois token other loggers write, which the sponsor takes.
                    || (**id == "IL QSO Party" && token.eq_ignore_ascii_case("IL-QSO-PARTY"))
            })
            .and_then(|id| ruleset_by_contest_id(id, year))
    })
}

/// The header lines a log's contacts take a field from.
const MAPPED_HEADERS: &[&str] = &["CONTEST", "GRID-LOCATOR", "START-OF-LOG", "END-OF-LOG"];

/// A Cabrillo band token as an ADIF band, or a frequency in kHz as the band it is in and the
/// frequency in MHz.
fn band_and_freq(token: &str) -> Option<(String, Option<String>)> {
    let t = token.trim().to_ascii_uppercase();
    let band = match t.as_str() {
        "50" => "6m",
        "70" => "4m",
        "144" => "2m",
        "222" => "1.25m",
        "432" => "70cm",
        "902" => "33cm",
        "1.2G" => "23cm",
        "2.3G" => "13cm",
        "3.4G" => "9cm",
        "5.7G" => "6cm",
        "10G" => "3cm",
        "24G" => "1.25cm",
        "47G" => "6mm",
        "75G" => "4mm",
        "122G" => "2.5mm",
        "134G" => "2mm",
        "241G" => "1mm",
        "LIGHT" => "submm",
        _ => {
            let khz: u32 = t.parse().ok().filter(|k| *k >= 1000)?;
            let mhz = f64::from(khz) / 1000.0;
            let band = super::super::query::band_key("", mhz)?.to_ascii_lowercase();
            return Some((band, Some(format!("{mhz:.3}"))));
        }
    };
    Some((band.to_string(), None))
}

/// The ADIF mode a Cabrillo mode token is, `None` for `DG` when the contest names no one digital
/// mode.
fn mode_for(mo: &str, contest: &str) -> Option<&'static str> {
    let c = contest.to_ascii_uppercase();
    Some(match mo.to_ascii_uppercase().as_str() {
        "CW" => "CW",
        "PH" => "SSB",
        "FM" => "FM",
        "RY" => "RTTY",
        "DG" if c.contains("RTTY") => "RTTY",
        "DG" if c.contains("FT8") => "FT8",
        "DG" if c.contains("FT4") => "FT4",
        _ => return None,
    })
}

/// Shaped like a call: letters and digits (and `/`), a digit followed somewhere by a letter, and
/// not two leading digits (no prefix has them; a Field Day class like `10A` does).
fn call_shaped(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() >= 3
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'/')
        && !(b[0].is_ascii_digit() && b[1].is_ascii_digit())
        && b.iter()
            .position(u8::is_ascii_digit)
            .is_some_and(|d| b[d..].iter().any(u8::is_ascii_alphabetic))
}

/// Shaped like a signal report: two or three digits, readability 1–5.
fn rst_shaped(s: &str) -> bool {
    (2..=3).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_digit())
        && (b'1'..=b'5').contains(&s.as_bytes()[0])
}

/// The template's columns for one line: the slot keys each side carries, the line's columns
/// after the date and time being `mycall + sent + call + rcvd [+ t]`. The first role whose width
/// fits; `None` when none does.
fn columns(
    spec: &ExchangeSpec,
    rs: &FdRuleset,
    n: usize,
) -> Option<(Vec<&'static str>, Vec<&'static str>, bool)> {
    let fits = |s: usize, r: usize| {
        if 2 + s + r == n {
            Some(false)
        } else if 3 + s + r == n {
            Some(true)
        } else {
            None
        }
    };
    if !rs.cabrillo_columns.is_empty() {
        let keys: Vec<&'static str> = rs.cabrillo_columns.iter().map(|c| c.key).collect();
        return fits(keys.len(), keys.len()).map(|t| (keys.clone(), keys, t));
    }
    spec.roles.iter().find_map(|role| {
        let sent: Vec<&'static str> = role
            .sends
            .iter()
            .copied()
            .filter(|k| !is_call_slot(spec, k))
            .collect();
        let rcvd: Vec<&'static str> = role
            .receives
            .iter()
            .copied()
            .filter(|k| !is_call_slot(spec, k))
            .collect();
        fits(sent.len(), rcvd.len()).map(|t| (sent, rcvd, t))
    })
}

/// Read a Cabrillo log.
pub(crate) fn read(text: &str) -> Read {
    let mut notes = Vec::new();
    let mut unmapped: Vec<Unmapped> = Vec::new();
    let mut list = |what: &str, side: Option<&'static str>, reason: UnmappedReason| match unmapped
        .iter_mut()
        .find(|u| u.what == what && u.side == side && u.reason == reason)
    {
        Some(u) => u.count += 1,
        None => unmapped.push(Unmapped {
            what: what.to_string(),
            side,
            count: 1,
            reason,
        }),
    };
    // The header first: the contest and the grid apply to every line, wherever they sit.
    let mut contest = String::new();
    let mut grid = String::new();
    let mut qsos: Vec<(usize, &str, bool)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let Some((tag, value)) = line.split_once(':') else {
            notes.push(Note {
                kind: NoteKind::UnreadableLine,
                at: i + 1,
                text: super::clip(line),
            });
            continue;
        };
        let tag = tag.trim().to_ascii_uppercase();
        match tag.as_str() {
            "QSO" => qsos.push((i + 1, value, false)),
            "X-QSO" => qsos.push((i + 1, value, true)),
            "CONTEST" => contest = value.trim().to_string(),
            "GRID-LOCATOR" => grid = value.trim().to_ascii_uppercase(),
            _ => {}
        }
        if tag != "QSO" && tag != "X-QSO" && !MAPPED_HEADERS.contains(&tag.as_str()) {
            list(&tag, None, UnmappedReason::Header);
        }
    }
    let year = qsos
        .iter()
        .find_map(|(_, v, _)| v.split_whitespace().nth(2)?.get(0..4)?.parse().ok())
        .unwrap_or(0);
    let ruleset = ruleset_for(&contest, year);
    let contest_id = ruleset.map_or(contest.clone(), |r| r.contest_id.to_string());
    if ruleset.is_none() && !qsos.is_empty() {
        notes.push(Note {
            kind: NoteKind::UnknownContest,
            at: 0,
            text: format!("CONTEST: {}", super::clip(&contest)),
        });
    }
    let mut records = Vec::new();
    for (at, value, excluded) in qsos {
        let mut note = |kind, text: String| notes.push(Note { kind, at, text });
        let tokens: Vec<&str> = value.split_whitespace().collect();
        let (Some(&freq), Some(&mo), Some(&date), Some(&time)) =
            (tokens.first(), tokens.get(1), tokens.get(2), tokens.get(3))
        else {
            note(NoteKind::UnreadableLine, super::clip(value.trim()));
            continue;
        };
        let a = &tokens[4.min(tokens.len())..];
        // Read with the template, or as an unknown contest's line.
        let templated = ruleset.and_then(|rs| {
            let spec = rs.exchange;
            let (sent, rcvd, t) = columns(spec, rs, a.len())?;
            Some((spec, sent, rcvd, t))
        });
        if ruleset.is_some() && templated.is_none() {
            note(NoteKind::TemplateMismatch, super::clip(value.trim()));
        }
        let (their, rcvd_cols, t) = match &templated {
            Some((_, sent, rcvd, t)) => (1 + sent.len(), rcvd.len(), *t),
            None => match unknown_split(a) {
                Some((their, rcvd, t)) => (their, rcvd, t),
                None => {
                    note(NoteKind::UnreadableLine, super::clip(value.trim()));
                    continue;
                }
            },
        };
        let mine = a[0];
        let call = a[their];
        let sent = &a[1..their];
        let rcvd = &a[their + 1..their + 1 + rcvd_cols];
        let mut fields: Vec<(String, String)> = Vec::new();
        put(&mut fields, "CALL", &call.to_ascii_uppercase());
        put(&mut fields, "QSO_DATE", &date.replace('-', ""));
        put(&mut fields, "TIME_ON", &format!("{time}00"));
        match band_and_freq(freq) {
            Some((band, mhz)) => {
                put(&mut fields, "BAND", &band);
                if let Some(mhz) = mhz {
                    put(&mut fields, "FREQ", &mhz);
                }
            }
            None => note(NoteKind::UnknownBand, format!("freq={freq}")),
        }
        match mode_for(mo, &contest) {
            Some(m) => put(&mut fields, "MODE", m),
            None => note(NoteKind::ModeUnresolved, format!("mo={mo}")),
        }
        put(&mut fields, "STATION_CALLSIGN", &mine.to_ascii_uppercase());
        // The exchange: to its fields by the template, or an RST read off an unknown one.
        match &templated {
            Some((spec, sent_keys, rcvd_keys, _)) => {
                let blank = |rs: &FdRuleset, k: &str, v: &str| {
                    rs.cabrillo_columns
                        .iter()
                        .any(|c| c.key == k && !c.blank.is_empty() && c.blank == v)
                };
                for (side, keys, values) in [("sent", sent_keys, sent), ("rcvd", rcvd_keys, rcvd)] {
                    for (k, v) in keys.iter().zip(values.iter()) {
                        let rs = ruleset.expect("a template comes from a ruleset");
                        if blank(rs, k, v) {
                            continue;
                        }
                        let tags = spec
                            .copied(k, v)
                            .map(|fv| crate::contest::adif::tags_for(spec, &fv));
                        let tag = tags.and_then(|t| if side == "sent" { t.sent } else { t.rcvd });
                        match tag {
                            Some(tag) => put(&mut fields, tag, v),
                            None => list(k, Some(side), UnmappedReason::ExchangeText),
                        }
                    }
                }
            }
            None => {
                if let (Some(s), Some(r)) = (sent.first(), rcvd.first()) {
                    if rst_shaped(s) && rst_shaped(r) {
                        put(&mut fields, "RST_SENT", s);
                        put(&mut fields, "RST_RCVD", r);
                        note(NoteKind::UnknownContest, format!("RST {s} {r}"));
                    }
                }
            }
        }
        put(&mut fields, "STX_STRING", &sent.join(" "));
        put(&mut fields, "SRX_STRING", &rcvd.join(" "));
        put(&mut fields, "CONTEST_ID", &contest_id);
        put(&mut fields, "MY_GRIDSQUARE", &grid);
        if t {
            list("t", None, UnmappedReason::TransmitterId);
        }
        if excluded {
            note(NoteKind::ExcludedQso, format!("{call} {date} {time}"));
        }
        records.push(SourceRecord { at, fields });
    }
    Read {
        records,
        notes,
        unmapped,
        template: ruleset.map(|r| r.contest_id.to_string()),
    }
}

/// A field once, and only with a value: the first a line names wins, as in
/// [`crate::contest::directed_columns`].
fn put(fields: &mut Vec<(String, String)>, k: &str, v: &str) {
    if !v.is_empty() && !fields.iter().any(|(f, _)| f == k) {
        fields.push((k.to_string(), v.to_string()));
    }
}

/// An unknown contest's line after the date and time, `a` (`a[0]` the sending call): where the
/// other call is, how wide the received side is, and whether a transmitter id ends the line.
/// (The sent side is everything between the two calls.)
/// Each side the same width first, as every template is, the transmitter id assumed only when the
/// line is otherwise odd; then the first call-shaped column after the sender's.
fn unknown_split(a: &[&str]) -> Option<(usize, usize, bool)> {
    let n = a.len();
    if n < 2 || !call_shaped(a[0]) {
        return None;
    }
    for t in [!n.is_multiple_of(2), n.is_multiple_of(2)] {
        let m = n - usize::from(t);
        if m >= 2 && m.is_multiple_of(2) {
            let k = (m - 2) / 2;
            if call_shaped(a[1 + k]) {
                return Some((1 + k, k, t));
            }
        }
    }
    let at = (1..n).find(|&i| call_shaped(a[i]))?;
    Some((at, n - at - 1, false))
}
