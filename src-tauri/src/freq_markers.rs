//! Band map frequency markers: a frequency the operator wants to come back to, with the call that
//! was in the contest entry when it was marked (or none), as N1MM Logger+'s Mark (Alt+M) "marks the
//! current frequency in the Bandmap as being in use".
//!
//! - **Marked at the station's own dial.** [`add_freq_marker`] reads the dial and the band from the
//!   station, never from the caller, so a marker is always where the rig was.
//! - **One per call per band.** Marking a call again on the same band moves its marker; a marker with
//!   no call replaces a call-less one within [`SAME_SPOT_HZ`] on that band.
//! - **Gone when worked.** A marker whose call is in the contest log on its band is dropped at the next
//!   read or write ([`Markers::prune_logged`]); Delete on the band map drops one by hand.
//! - **One contest session.** The file carries the session it belongs to (the contest and the start of
//!   its running window); a file from another session reads as no markers.
//! - **Clamped and atomic.** At most [`MAX_MARKERS`], each checked on load; written beside the file and
//!   renamed over it, so a crash leaves the old file or the new one.
//! - **Local only.** Nothing here is posted anywhere, and the snapshot does not carry markers: the band
//!   map asks for them by command.
//! - **Jumping back is an operator QSY** ([`freq_marker_qsy`]): the same retune as a dial change, and
//!   refused while anything is transmitting, since a QSY mid-over would move the signal on the air.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;
use tempo_app::dto::AppSnapshot;
use tempo_app::engine::Engine;

use crate::SharedEngine;

/// The most markers one session keeps; the oldest goes first.
pub const MAX_MARKERS: usize = 100;
/// The longest call a marker keeps (a long portable call fits).
const MAX_CALL: usize = 16;
/// Two call-less markers this close on one band are the same place.
pub const SAME_SPOT_HZ: u64 = 100;

/// The refusals, as tokens the UI turns into sentences.
pub const NO_CONTEST: &str = "markerNoContest";
pub const TRANSMITTING: &str = "markerTransmitting";
pub const NOT_FOUND: &str = "markerNotFound";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: u64,
    pub dial_hz: u64,
    pub band: String,
    /// Uppercase; empty for a marker made with an empty entry.
    pub call: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Markers {
    pub session: String,
    pub next_id: u64,
    pub markers: Vec<Marker>,
}

/// A call as a marker keeps it: uppercase letters, digits and `/`, at most [`MAX_CALL`].
fn clean_call(raw: &str) -> String {
    raw.trim()
        .to_ascii_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '/')
        .take(MAX_CALL)
        .collect()
}

/// A dial a marker can hold: inside an amateur band the band plan knows.
fn valid_dial(dial_hz: u64) -> bool {
    tempo_app::bandplan::band_for_dial(dial_hz as f64 / 1e6).is_some()
}

impl Markers {
    pub fn empty(session: &str) -> Self {
        Self {
            session: session.into(),
            next_id: 1,
            markers: Vec::new(),
        }
    }

    /// Mark `dial_hz` on `band` with `call` (may be empty). Returns the marker's id.
    pub fn add(&mut self, dial_hz: u64, band: &str, call: &str) -> u64 {
        let call = clean_call(call);
        let band = band.trim().to_ascii_uppercase();
        self.markers.retain(|m| {
            m.band != band
                || if call.is_empty() {
                    !m.call.is_empty() || m.dial_hz.abs_diff(dial_hz) > SAME_SPOT_HZ
                } else {
                    m.call != call
                }
        });
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.markers.push(Marker {
            id,
            dial_hz,
            band,
            call,
        });
        if self.markers.len() > MAX_MARKERS {
            let over = self.markers.len() - MAX_MARKERS;
            self.markers.drain(..over);
        }
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.markers.len();
        self.markers.retain(|m| m.id != id);
        self.markers.len() != before
    }

    /// Drop every marker whose call is logged on its band (`logged`: uppercase call, band).
    /// Returns whether anything went.
    pub fn prune_logged(&mut self, logged: &[(String, String)]) -> bool {
        let before = self.markers.len();
        self.markers.retain(|m| {
            m.call.is_empty()
                || !logged
                    .iter()
                    .any(|(c, b)| *c == m.call && b.eq_ignore_ascii_case(&m.band))
        });
        self.markers.len() != before
    }

    /// What a file holds, checked: another session's file, an unreadable one or a missing one
    /// reads as no markers; a marker off the band plan is dropped; the newest [`MAX_MARKERS`] stay.
    pub fn load(path: &Path, session: &str) -> Self {
        let Some(read) = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Markers>(&b).ok())
        else {
            return Self::empty(session);
        };
        if read.session != session {
            return Self::empty(session);
        }
        let mut markers: Vec<Marker> = read
            .markers
            .into_iter()
            .filter(|m| m.id > 0 && valid_dial(m.dial_hz))
            .map(|m| Marker {
                call: clean_call(&m.call),
                band: m.band.trim().to_ascii_uppercase(),
                ..m
            })
            .collect();
        if markers.len() > MAX_MARKERS {
            let over = markers.len() - MAX_MARKERS;
            markers.drain(..over);
        }
        let top = markers.iter().map(|m| m.id).max().unwrap_or(0);
        Self {
            session: session.into(),
            next_id: read.next_id.max(top + 1),
            markers,
        }
    }

    /// Written beside `path` and renamed over it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".part");
        let tmp = PathBuf::from(tmp);
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, &json)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    }
}

/// The contest session markers belong to: the contest's rules id and the start of its running (or
/// next) window, so the same contest next year starts with none. `None` with Contest mode off.
pub fn session_of(engine: &Engine, now_unix: u64) -> Option<String> {
    let s = engine.settings();
    if !s.fd_active {
        return None;
    }
    let event = s.fd_event.trim();
    let event = if event.is_empty() { "arrlfd" } else { event };
    let start =
        tempo_core::fd_rules::ruleset_by_id(event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .map(|rs| rs.next_or_running(now_unix).start_unix)
            .unwrap_or(0);
    Some(format!("{event}@{start}"))
}

/// `<config dir>/freq_markers.json`, beside settings.json.
fn markers_path() -> PathBuf {
    crate::settings_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("freq_markers.json")
}

/// The loaded markers, kept between calls (read from the file at the first call of a session).
static LOADED: Mutex<Option<(PathBuf, Markers)>> = Mutex::new(None);

/// Run `change` on the session's markers, pruned against the contest log first, and write the file
/// when anything changed. `Err(NO_CONTEST)` with Contest mode off.
pub fn with_markers<T>(
    engine: &Engine,
    path: &Path,
    now_unix: u64,
    change: impl FnOnce(&mut Markers, &Engine) -> Result<(T, bool), String>,
) -> Result<(T, Vec<Marker>), String> {
    let session = session_of(engine, now_unix).ok_or_else(|| NO_CONTEST.to_string())?;
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut markers = match loaded.take() {
        Some((p, m)) if p == path && m.session == session => m,
        _ => Markers::load(path, &session),
    };
    let pruned = markers.prune_logged(&engine.contest_logged_call_bands());
    let result = change(&mut markers, engine);
    if let Ok((_, changed)) = &result {
        if pruned || *changed {
            if let Err(e) = markers.save(path) {
                eprintln!("tempo: frequency markers not saved: {e}");
            }
        }
    }
    let list = markers.markers.clone();
    *loaded = Some((path.to_path_buf(), markers));
    result.map(|(t, _)| (t, list))
}

fn now() -> u64 {
    crate::now_unix().max(0) as u64
}

/// This contest session's markers. Empty with Contest mode off.
#[tauri::command(async)]
pub fn get_freq_markers(state: State<'_, SharedEngine>) -> Vec<Marker> {
    let eng = crate::engine_lock(&state);
    with_markers(&eng, &markers_path(), now(), |_, _| Ok(((), false)))
        .map(|(_, list)| list)
        .unwrap_or_default()
}

/// Mark the station's dial now, with `call` (the contest entry's call, or empty).
#[tauri::command(async)]
pub fn add_freq_marker(
    state: State<'_, SharedEngine>,
    call: String,
) -> Result<Vec<Marker>, String> {
    let eng = crate::engine_lock(&state);
    with_markers(&eng, &markers_path(), now(), |m, e| {
        add_at_dial(m, e, &call)
    })
    .map(|(_, list)| list)
}

/// [`add_freq_marker`]'s change: the station's own dial and band.
pub fn add_at_dial(m: &mut Markers, engine: &Engine, call: &str) -> Result<(u64, bool), String> {
    let s = engine.settings();
    let dial_hz = s.dial_hz();
    let Some(band) = tempo_app::bandplan::band_for_dial(dial_hz as f64 / 1e6) else {
        return Err("markerOffBand".into());
    };
    Ok((m.add(dial_hz, band, call), true))
}

/// Delete a marker.
#[tauri::command(async)]
pub fn remove_freq_marker(state: State<'_, SharedEngine>, id: u64) -> Result<Vec<Marker>, String> {
    let eng = crate::engine_lock(&state);
    with_markers(&eng, &markers_path(), now(), |m, _| Ok(((), m.remove(id)))).map(|(_, list)| list)
}

/// The marker to jump back to, or why not: refused while anything transmits.
pub fn qsy_target(engine: &Engine, markers: &Markers, id: u64) -> Result<Marker, String> {
    if engine.app.transmitting() || engine.tx_owner().is_some() {
        return Err(TRANSMITTING.into());
    }
    markers
        .markers
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .ok_or_else(|| NOT_FOUND.into())
}

/// Jump back to a marker: the operator's own QSY (`set_frequency`'s), keeping the sideband, and
/// refused while anything transmits.
#[tauri::command(async)]
pub fn freq_marker_qsy(state: State<'_, SharedEngine>, id: u64) -> Result<AppSnapshot, String> {
    let mut eng = crate::engine_lock(&state);
    let (target, _) = with_markers(&eng, &markers_path(), now(), |m, e| {
        Ok((qsy_target(e, m, id)?, false))
    })?;
    let mode = eng.settings().sideband.clone();
    let band = tempo_app::bandplan::band_for_dial(target.dial_hz as f64 / 1e6)
        .unwrap_or_default()
        .to_string();
    eng.set_frequency(target.dial_hz as f64 / 1e6, &band, &mode);
    crate::persist_settings(eng.settings().clone(), |e| {
        eprintln!("tempo: failed to persist frequency: {e}")
    });
    Ok(eng.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-freq-markers-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("freq_markers.json")
    }

    #[test]
    fn a_call_is_marked_once_per_band_and_a_bare_marker_once_per_spot() {
        let mut m = Markers::empty("s");
        m.add(14_025_000, "20m", " w1aw ");
        m.add(14_031_000, "20m", "W1AW");
        m.add(7_025_000, "40m", "W1AW");
        assert_eq!(
            m.markers
                .iter()
                .map(|x| (x.dial_hz, x.band.as_str(), x.call.as_str()))
                .collect::<Vec<_>>(),
            vec![(14_031_000, "20M", "W1AW"), (7_025_000, "40M", "W1AW")]
        );
        m.add(14_040_000, "20m", "");
        m.add(14_040_050, "20m", "");
        m.add(14_041_000, "20m", "");
        let bare: Vec<u64> = m
            .markers
            .iter()
            .filter(|x| x.call.is_empty())
            .map(|x| x.dial_hz)
            .collect();
        assert_eq!(bare, vec![14_040_050, 14_041_000]);
    }

    #[test]
    fn a_hostile_call_is_cleaned_and_clamped() {
        let mut m = Markers::empty("s");
        m.add(
            14_025_000,
            "20m",
            "k9abc\r\nDX 1 X\u{0}/P2345678901234567890",
        );
        assert_eq!(m.markers[0].call, "K9ABCDX1X/P23456");
    }

    #[test]
    fn the_oldest_marker_goes_past_the_limit() {
        let mut m = Markers::empty("s");
        for i in 0..(MAX_MARKERS as u64 + 5) {
            m.add(14_000_000 + i * 1000, "20m", &format!("K{i}AA"));
        }
        assert_eq!(m.markers.len(), MAX_MARKERS);
        assert_eq!(m.markers[0].call, "K5AA");
    }

    #[test]
    fn a_logged_call_clears_its_marker_on_that_band_only() {
        let mut m = Markers::empty("s");
        m.add(14_025_000, "20m", "W1AW");
        m.add(7_025_000, "40m", "W1AW");
        m.add(14_030_000, "20m", "");
        assert!(m.prune_logged(&[("W1AW".into(), "20M".into())]));
        assert_eq!(
            m.markers
                .iter()
                .map(|x| (x.band.as_str(), x.call.as_str()))
                .collect::<Vec<_>>(),
            vec![("40M", "W1AW"), ("20M", "")]
        );
        assert!(!m.prune_logged(&[("W1AW".into(), "20M".into())]));
    }

    #[test]
    fn markers_survive_a_restart_in_their_own_session_only() {
        let path = scratch("restart");
        let mut m = Markers::empty("ilqp@100");
        m.add(14_025_000, "20m", "W1AW");
        let id = m.add(7_030_000, "40m", "");
        m.save(&path).unwrap();
        assert!(!path.with_extension("json.part").exists());
        let back = Markers::load(&path, "ilqp@100");
        assert_eq!(back.markers, m.markers);
        // A new id never reuses one the file holds.
        assert!(back.next_id > id);
        assert_eq!(Markers::load(&path, "ilqp@200").markers, vec![]);
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Markers::load(&path, "ilqp@100").markers, vec![]);
    }

    #[test]
    fn a_file_is_checked_on_load() {
        let path = scratch("clamp");
        let mut list = Vec::new();
        for i in 0..(MAX_MARKERS as u64 + 10) {
            list.push(Marker {
                id: i + 1,
                dial_hz: 14_000_000 + i * 100,
                band: "20m".into(),
                call: format!("k{i}aa"),
            });
        }
        // Off every band, and an id of 0: dropped.
        list.push(Marker {
            id: 999,
            dial_hz: 99_000_000_000,
            band: "20m".into(),
            call: "X".into(),
        });
        list.push(Marker {
            id: 0,
            dial_hz: 14_000_000,
            band: "20m".into(),
            call: "Y".into(),
        });
        let file = Markers {
            session: "s".into(),
            next_id: 0,
            markers: list,
        };
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let back = Markers::load(&path, "s");
        assert_eq!(back.markers.len(), MAX_MARKERS);
        assert_eq!(back.markers[0].call, "K10AA");
        assert_eq!(back.markers[0].band, "20M");
        assert!(back.markers.iter().all(|m| m.id > 0 && m.id != 999));
        assert_eq!(back.next_id, MAX_MARKERS as u64 + 11);
    }

    fn contest_engine() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        e.apply_settings(s);
        e.set_mode("fieldday-sp").unwrap();
        e
    }

    #[test]
    fn markers_need_a_running_contest_and_take_the_stations_dial() {
        let path = scratch("engine");
        let off = Engine::new("W9XYZ", "EN61", 0);
        assert_eq!(
            with_markers(&off, &path, 1_000, |m, e| add_at_dial(m, e, "W1AW")).unwrap_err(),
            NO_CONTEST
        );
        let mut e = contest_engine();
        e.set_frequency(14.0255, "20m", "CW");
        let (_, list) = with_markers(&e, &path, 1_000, |m, e| add_at_dial(m, e, "w1aw")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].dial_hz, list[0].call.as_str()),
            (14_025_500, "W1AW")
        );
        // On disk, for the next run.
        let session = session_of(&e, 1_000).unwrap();
        assert_eq!(Markers::load(&path, &session).markers, list);
    }

    #[test]
    fn logging_the_call_on_that_band_clears_its_marker() {
        let path = scratch("logged");
        let mut e = contest_engine();
        e.set_frequency(14.0255, "20m", "CW");
        with_markers(&e, &path, 1_000, |m, e| add_at_dial(m, e, "W1AW")).unwrap();
        let logged = e
            .contest_log_manual(
                "W1AW",
                &[
                    ("class".into(), "2A".into()),
                    ("section".into(), "CT".into()),
                ],
                "CW",
                None,
            )
            .unwrap();
        assert!(logged, "premise: the contact is logged");
        let (_, list) = with_markers(&e, &path, 1_000, |_, _| Ok(((), false))).unwrap();
        assert_eq!(list, vec![]);
        let session = session_of(&e, 1_000).unwrap();
        assert_eq!(
            Markers::load(&path, &session).markers,
            vec![],
            "and on disk"
        );
    }

    #[test]
    fn jumping_back_is_refused_while_transmitting() {
        let mut e = contest_engine();
        let mut m = Markers::empty("s");
        let id = m.add(14_025_000, "20m", "W1AW");
        assert_eq!(qsy_target(&e, &m, id).unwrap().dial_hz, 14_025_000);
        e.app.set_transmitting(true);
        assert_eq!(qsy_target(&e, &m, id).unwrap_err(), TRANSMITTING);
        e.app.set_transmitting(false);
        assert_eq!(qsy_target(&e, &m, id + 1).unwrap_err(), NOT_FOUND);
    }
}
