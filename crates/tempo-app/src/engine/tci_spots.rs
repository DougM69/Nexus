//! Nexus's spots for an SDR program's panadapter: what the station hands the TCI client of the
//! radio in use when that radio's "Spots on the SDR panadapter" is on (`Settings::tci_spots`, per
//! radio, off by default).
//!
//! The spots are the Needed board's own, scored where the board's are (the app shell), each with
//! the colour of its need; the shell hands them here every few seconds while the radio in use
//! wants them ([`Engine::tci_spots_on`], [`Engine::set_tci_spots`]). The radio loop reads them
//! each tick ([`Engine::tci_spots`]) and gives them to that radio's TCI client
//! (`tempo_audio::tci::spots`), which puts them on the panadapter, takes them off as they age,
//! and hands the operator's clicks on them back to [`Engine::tci_spot_clicked`], which only tunes.
//! Nothing here writes to a radio.
//!
//! ## Programs' spots, the other way round
//!
//! A program connected to Nexus's own TCI server (a logger such as Log4OM, a skimmer) sends its
//! spots (`spot:<call>,<mode>,<hz>,<argb>,<text>;`), and Nexus draws them on its band map with the
//! program that sent them ([`Engine::tci_program_spot`]). They are kept apart from the cluster's
//! spots, in [`ProgramSpots`], and bounded: [`PROGRAM_SPOTS_EACH`] a program and
//! [`PROGRAM_SPOTS_IN_ALL`] in all, the oldest going first, each gone [`PROGRAM_SPOT_MAX_AGE_SECS`]
//! after it was sent. A program's `spot_delete` and `spot_clear` touch its own spots only. Its
//! spots go when it disconnects, and every program's go when the server stops, when "Spots from
//! programs" (`Settings::tci_server_spots`) is off, and in an Unassisted contest entry, where a
//! program's spots are as much help as the cluster's ([`Engine::tci_server_spots_on`]).
//!
//! What a program sends is kept as data, never as markup: the callsign 1 to
//! [`PROGRAM_SPOT_CALL_MAX`] letters, digits and `/`, shaped like a callsign; the frequency inside
//! the range programs are told (`vfo_limits`); the mode at most a few letters and digits; the text
//! printable ASCII, cut to [`PROGRAM_SPOT_TEXT_MAX`] characters. Spots refused and texts cut are
//! counted ([`ProgramSpots::refused`], [`ProgramSpots::cut`]).
//!
//! ⛔ A SPOT NEVER ACTS, AND A CLICK ON ONE ONLY TUNES. Keeping a program's spot never tunes, keys
//! or arms anything. The operator's click on one ([`Engine::tci_program_spot_clicked`]) tunes the
//! way a click in an SDR program's panadapter does, with the same rules (never while anything is on
//! the air; in soundcard CW the dial a pitch off the spot), and nothing else: it never changes the
//! section, never calls the station or starts a QSO, never arms transmit, never clears a Stop's
//! hold or a watchdog trip, and never asks the window to open a cockpit, whose entry would arm.
//! Every program is then told of the click (`clicked_on_spot`, both spellings), as it is told of
//! a click on any spot in Nexus; telling is all that does.

use std::collections::VecDeque;
use std::sync::Arc;

use tempo_net::tci::server::{Refusal, Spot};

use super::Engine;

/// One spot for the SDR program's panadapter, as the station chose it; and one spot a program
/// sent Nexus's TCI server ([`ProgramSpot`]), in the same shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanadapterSpot {
    /// The station, as the spot named it.
    pub call: String,
    /// Where it was heard, in Hz.
    pub hz: i64,
    /// Its mode, as the panadapter labels it ("CW", "SSB", "FT8").
    pub mode: String,
    /// The colour of the need it is for, ARGB (`0xAARRGGBB`): the Needed board's colour for it.
    /// A program's spot keeps the colour the program gave it.
    pub argb: u32,
    /// Why it is needed, in words ("New one", "New band / POTA"); a program's spot, its comment.
    pub text: String,
    /// When it was last heard, in Unix seconds: the panadapter loses it once that is old. A
    /// program's spot, when the program sent it.
    pub heard_unix: i64,
}

/// The spots the station last chose for the panadapter.
#[derive(Debug, Clone, Default)]
pub struct PanadapterSpots {
    /// Moves on with each change of the spots, so a reader can tell a new list from the last one
    /// without comparing them.
    pub generation: u64,
    pub spots: Arc<[PanadapterSpot]>,
}

impl Engine {
    /// Whether the radio in use puts Nexus's spots on its SDR program's panadapter: it is on the
    /// TCI connection, and its "Spots on the SDR panadapter" is on.
    pub fn tci_spots_on(&self) -> bool {
        self.settings.tci_spots && crate::settings::rig_conn_is_tci(&self.settings.rig_conn)
    }

    /// The spots for the panadapter, best first, as the station chose them just now. A list equal
    /// to the last one changes nothing.
    pub fn set_tci_spots(&mut self, spots: Vec<PanadapterSpot>) {
        if *self.tci_spots.spots != spots[..] {
            self.tci_spots = PanadapterSpots {
                generation: self.tci_spots.generation.wrapping_add(1),
                spots: spots.into(),
            };
        }
    }

    /// The spots for the panadapter, as last chosen ([`Self::set_tci_spots`]).
    pub fn tci_spots(&self) -> PanadapterSpots {
        self.tci_spots.clone()
    }
}

/// The most spots one program keeps on the band map at once.
pub const PROGRAM_SPOTS_EACH: usize = 200;
/// The most spots all programs together keep on the band map.
pub const PROGRAM_SPOTS_IN_ALL: usize = 500;
/// A program's spot goes this long after the program sent it (or sent it again).
pub const PROGRAM_SPOT_MAX_AGE_SECS: i64 = 30 * 60;
/// The longest callsign a program's spot may name.
pub const PROGRAM_SPOT_CALL_MAX: usize = 16;
/// A program's spot text is cut to this many characters.
pub const PROGRAM_SPOT_TEXT_MAX: usize = 64;
/// A program's spot mode keeps at most this many letters and digits.
const PROGRAM_SPOT_MODE_MAX: usize = 8;
/// The operator's clicks waiting to be told to programs, at most: the TCI server's manager tells
/// them on its next tick, 50 ms away, so more than this is never waiting but in a burst.
const SPOT_CLICKS_WAITING: usize = 8;

/// A spot a program on Nexus's TCI server sent, as Nexus keeps it for its band map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSpot {
    /// The program that sent it: its connection's number on the server (`ClientId::get`), the
    /// number Settings lists it by.
    pub program: u64,
    /// The spot, as kept: callsign in upper case, mode and text cleaned, sent at `heard_unix`.
    pub spot: PanadapterSpot,
}

/// Why a program's spot was not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotKept {
    /// Its callsign is not 1 to [`PROGRAM_SPOT_CALL_MAX`] letters, digits and `/` shaped like one.
    Call,
    /// Its frequency is outside the range programs are told.
    Frequency,
}

/// The programs' spots, oldest first: bounded per program and in all, aged out, and counted; and
/// the operator's clicks on spots waiting to be told to the programs.
#[derive(Debug, Clone, Default)]
pub struct ProgramSpots {
    spots: VecDeque<ProgramSpot>,
    generation: u64,
    refused: u64,
    cut: u64,
    /// `(callsign, Hz)` of each click, oldest first, at most [`SPOT_CLICKS_WAITING`].
    clicks: VecDeque<(String, i64)>,
}

/// A program's callsign as kept: upper case, 1 to [`PROGRAM_SPOT_CALL_MAX`] letters, digits and
/// `/`, and shaped like a callsign (as a click on it must be, to tune to it). `None` otherwise.
pub fn program_spot_call(call: &str) -> Option<String> {
    let call = call.trim().to_ascii_uppercase();
    let chars_ok = (1..=PROGRAM_SPOT_CALL_MAX).contains(&call.len())
        && call
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/');
    (chars_ok && tempo_core::message::looks_like_call(&call)).then_some(call)
}

/// A program's spot text as kept: printable ASCII only, trimmed, at most
/// [`PROGRAM_SPOT_TEXT_MAX`] characters. `true` when anything was dropped or cut.
fn program_spot_text(text: &str) -> (String, bool) {
    let printable: String = text.chars().filter(|c| (' '..='~').contains(c)).collect();
    let dropped = printable.chars().count() != text.chars().count();
    let trimmed = printable.trim();
    let kept: String = trimmed.chars().take(PROGRAM_SPOT_TEXT_MAX).collect();
    let cut = kept.len() != trimmed.len();
    (kept, dropped || cut)
}

/// A program's spot mode as kept: its letters and digits, upper case, at most
/// [`PROGRAM_SPOT_MODE_MAX`].
fn program_spot_mode(mode: &str) -> String {
    mode.chars()
        .filter(char::is_ascii_alphanumeric)
        .take(PROGRAM_SPOT_MODE_MAX)
        .collect::<String>()
        .to_ascii_uppercase()
}

impl ProgramSpots {
    /// Keep `program`'s spot `raw`, cleaned, as its newest: its older spot of the same callsign
    /// goes, and past the bounds the oldest go first, the program's own past
    /// [`PROGRAM_SPOTS_EACH`], then anyone's past [`PROGRAM_SPOTS_IN_ALL`]. `limits` is the range
    /// of frequencies programs are told, ends included.
    pub fn put(
        &mut self,
        program: u64,
        raw: PanadapterSpot,
        limits: (i64, i64),
    ) -> Result<(), NotKept> {
        let Some(call) = program_spot_call(&raw.call) else {
            self.refused += 1;
            return Err(NotKept::Call);
        };
        if raw.hz < limits.0 || raw.hz > limits.1 {
            self.refused += 1;
            return Err(NotKept::Frequency);
        }
        let (text, cut) = program_spot_text(&raw.text);
        if cut {
            self.cut += 1;
        }
        self.spots
            .retain(|s| s.program != program || s.spot.call != call);
        self.spots.push_back(ProgramSpot {
            program,
            spot: PanadapterSpot {
                call,
                hz: raw.hz,
                mode: program_spot_mode(&raw.mode),
                argb: raw.argb,
                text,
                heard_unix: raw.heard_unix,
            },
        });
        if self.spots.iter().filter(|s| s.program == program).count() > PROGRAM_SPOTS_EACH {
            if let Some(oldest) = self.spots.iter().position(|s| s.program == program) {
                self.spots.remove(oldest);
            }
        }
        while self.spots.len() > PROGRAM_SPOTS_IN_ALL {
            self.spots.pop_front();
        }
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// Keep only the spots `keep` says, moving the generation on if any went.
    fn keep(&mut self, keep: impl FnMut(&ProgramSpot) -> bool) {
        let before = self.spots.len();
        self.spots.retain(keep);
        if self.spots.len() != before {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// `program`'s spot of `call` goes (`spot_delete`); no one else's.
    pub fn delete(&mut self, program: u64, call: &str) {
        let call = call.trim().to_ascii_uppercase();
        self.keep(|s| s.program != program || s.spot.call != call);
    }

    /// Every spot of `program` goes (`spot_clear`, or it disconnected); no one else's.
    pub fn clear_program(&mut self, program: u64) {
        self.keep(|s| s.program != program);
    }

    /// Every program's spots go, and the clicks waiting with them.
    pub fn clear(&mut self) {
        self.keep(|_| false);
        self.clicks.clear();
    }

    /// The spots [`PROGRAM_SPOT_MAX_AGE_SECS`] old at `now_unix` go.
    pub fn age(&mut self, now_unix: i64) {
        self.keep(|s| now_unix - s.spot.heard_unix < PROGRAM_SPOT_MAX_AGE_SECS);
    }

    /// The spots kept, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &ProgramSpot> {
        self.spots.iter()
    }

    /// Moves on with every change, so a reader can tell the spots changed without comparing them.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Spots refused since launch: a callsign that is not one, a frequency outside the range.
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// Spot texts cut to [`PROGRAM_SPOT_TEXT_MAX`] or stripped of what is not printable.
    pub fn cut(&self) -> u64 {
        self.cut
    }

    /// The operator clicked the spot `call` at `hz`: waiting to be told to the programs, the
    /// oldest dropped past [`SPOT_CLICKS_WAITING`]. Only a callsign a program's spot could carry.
    fn clicked(&mut self, call: &str, hz: i64) {
        let Some(call) = program_spot_call(call) else {
            return;
        };
        if self.clicks.len() == SPOT_CLICKS_WAITING {
            self.clicks.pop_front();
        }
        self.clicks.push_back((call, hz));
    }
}

impl Engine {
    /// Whether programs' spots are taken in, drawn, and told of the operator's clicks: Nexus's TCI
    /// server and its "Spots from programs" are on, and the operator is not in an Unassisted
    /// contest entry, where a program's spots are as much help as the cluster's.
    pub fn tci_server_spots_on(&self) -> bool {
        let s = &self.settings;
        s.tci_server && s.tci_server_spots && !s.unassisted_mode
    }

    /// `program`'s spot, for the band map: kept as data, never tuned to and never keyed (the
    /// module header). `limits` is the range of frequencies programs are told (`vfo_limits`). The
    /// refusal is the connection log's: [`Refusal::Tier`] while programs' spots are off.
    pub fn tci_program_spot(
        &mut self,
        program: u64,
        spot: &Spot,
        limits: (i64, i64),
    ) -> Result<(), Refusal> {
        if !self.tci_server_spots_on() {
            return Err(Refusal::Tier);
        }
        let raw = PanadapterSpot {
            call: spot.call.as_str().to_string(),
            hz: spot.hz.get(),
            mode: spot.mode.as_str().to_string(),
            argb: spot.argb,
            text: spot.text.as_str().to_string(),
            heard_unix: super::now_unix_secs() as i64,
        };
        self.tci_program_spots
            .put(program, raw, limits)
            .map_err(|why| match why {
                NotKept::Call => Refusal::SpotCall,
                NotKept::Frequency => Refusal::Limits,
            })
    }

    /// `spot_delete:<call>;` from `program`: its own spot of `call` goes, no one else's.
    pub fn tci_program_spot_delete(&mut self, program: u64, call: &str) {
        self.tci_program_spots.delete(program, call);
    }

    /// `spot_clear;` from `program`, or its disconnect: its own spots go, no one else's.
    pub fn tci_program_spot_clear(&mut self, program: u64) {
        self.tci_program_spots.clear_program(program);
    }

    /// The TCI server's manager, on each of its ticks: programs' spots age out, and every one goes
    /// while they are off ([`Self::tci_server_spots_on`]), the clicks waiting with them.
    pub fn tci_program_spots_tick(&mut self, now_unix: i64) {
        if self.tci_server_spots_on() {
            self.tci_program_spots.age(now_unix);
        } else {
            self.tci_program_spots.clear();
        }
    }

    /// The programs' spots for the band map at `now_unix`, oldest first: none while they are off,
    /// and none [`PROGRAM_SPOT_MAX_AGE_SECS`] old, whether or not a tick has taken it away yet.
    pub fn tci_program_spots(&self, now_unix: i64) -> Vec<ProgramSpot> {
        if !self.tci_server_spots_on() {
            return Vec::new();
        }
        self.tci_program_spots
            .iter()
            .filter(|s| now_unix - s.spot.heard_unix < PROGRAM_SPOT_MAX_AGE_SECS)
            .cloned()
            .collect()
    }

    /// The programs' spots as kept, for their counts and generation.
    pub fn tci_program_spot_store(&self) -> &ProgramSpots {
        &self.tci_program_spots
    }

    /// The operator clicked a program's spot, `call` at `hz`, on Nexus's band map: tune to it, as
    /// a click in an SDR program's panadapter tunes, and nothing else (the module header); then
    /// the programs are told, as of a click on any spot. `Err` says why the click changed nothing,
    /// and nothing is told then.
    pub fn tci_program_spot_clicked(&mut self, call: &str, hz: i64) -> Result<(), &'static str> {
        self.tune_to_clicked_spot(call, hz, "a program's spot clicked on Nexus's band map")?;
        self.note_tci_spot_click(call, hz);
        Ok(())
    }

    /// The operator clicked the spot `call` at `hz` (its own frequency) in Nexus: every program on
    /// Nexus's TCI server is told, by its manager on its next tick, while programs' spots are on.
    /// A cluster spot worked comes here from [`Self::note_work_call`], a program's spot from
    /// [`Self::tci_program_spot_clicked`]. Telling is all it does.
    pub(super) fn note_tci_spot_click(&mut self, call: &str, hz: i64) {
        if self.tci_server_spots_on() {
            self.tci_program_spots.clicked(call, hz);
        }
    }

    /// The clicks to tell the programs, oldest first, taken: the TCI server's manager tells each,
    /// and with no server running drops them, so a click is never told late.
    pub fn take_tci_spot_clicks(&mut self) -> Vec<(String, i64)> {
        self.tci_program_spots.clicks.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::now_unix_secs;
    use crate::settings::Settings;

    fn spot(call: &str, hz: i64) -> PanadapterSpot {
        PanadapterSpot {
            call: call.into(),
            hz,
            mode: "CW".into(),
            argb: 0xFFF2_3EC0,
            text: "New one".into(),
            heard_unix: 1_000,
        }
    }

    fn station(conn: &str, spots: bool) -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.apply_restored_settings(Settings {
            rig_model: 7,
            rig_conn: conn.into(),
            tci_spots: spots,
            ..Settings::default()
        });
        e
    }

    /// The spots go to the panadapter only from a radio on the TCI connection with the switch on.
    #[test]
    fn only_a_tci_radio_with_its_switch_on_wants_its_spots() {
        let cases = [
            ("tci", true, true),
            ("tci", false, false),
            ("serial", true, false),
            ("network", true, false),
        ];
        let got: Vec<_> = cases
            .iter()
            .map(|&(conn, on, _)| (conn, on, station(conn, on).tci_spots_on()))
            .collect();
        let want: Vec<_> = cases.iter().map(|&(c, on, w)| (c, on, w)).collect();
        assert_eq!(got, want);
    }

    /// A new list moves the generation on; the same list again does not.
    #[test]
    fn the_generation_moves_only_when_the_spots_change() {
        let mut e = station("tci", true);
        let first = e.tci_spots();
        assert!(first.spots.is_empty(), "none at launch");
        e.set_tci_spots(vec![spot("K1ABC", 14_025_000)]);
        let second = e.tci_spots();
        assert_eq!(second.generation, first.generation + 1);
        assert_eq!(&*second.spots, &[spot("K1ABC", 14_025_000)]);
        e.set_tci_spots(vec![spot("K1ABC", 14_025_000)]);
        assert_eq!(e.tci_spots().generation, second.generation, "the same list");
        e.set_tci_spots(vec![spot("K1ABC", 14_026_000)]);
        assert_eq!(e.tci_spots().generation, second.generation + 1, "moved");
        e.set_tci_spots(Vec::new());
        assert_eq!(e.tci_spots().generation, second.generation + 2, "emptied");
        assert!(e.tci_spots().spots.is_empty());
    }

    // ── Programs' spots ──────────────────────────────────────────────────────────────────────

    /// The range programs are told (`vfo_limits`), as the app tells it.
    const LIMITS: (i64, i64) = (30_000, 1_300_000_000);
    /// A made-up clock: the spots' send time.
    const T0: i64 = 1_800_000_000;

    fn raw(call: &str, hz: i64, text: &str, at: i64) -> PanadapterSpot {
        PanadapterSpot {
            call: call.into(),
            hz,
            mode: "cw".into(),
            argb: 0xFF00_FF00,
            text: text.into(),
            heard_unix: at,
        }
    }

    /// `(program, call)` of each spot kept, oldest first.
    fn kept(store: &ProgramSpots) -> Vec<(u64, String)> {
        store
            .iter()
            .map(|s| (s.program, s.spot.call.clone()))
            .collect()
    }

    /// A callsign `n` (`K1A`, `K1B`, … then `K2A`…), distinct for every `n` below 2000.
    fn call_n(n: usize) -> String {
        let letters = |mut i: usize| {
            let mut out = String::new();
            for _ in 0..2 {
                out.push(char::from(b'A' + (i % 26) as u8));
                i /= 26;
            }
            out
        };
        format!("K{}{}", n % 10, letters(n / 10))
    }

    /// A spot is kept as data with the program that sent it: the callsign in upper case, the mode
    /// cut to its letters and digits, the colour as sent.
    #[test]
    fn a_programs_spot_is_kept_with_the_program_that_sent_it() {
        let mut store = ProgramSpots::default();
        let mut sent = raw("k1abc", 14_025_000, "CQ NA", T0);
        sent.mode = "c.w;FT8-x".into();
        assert_eq!(store.put(3, sent, LIMITS), Ok(()));
        let got: Vec<_> = store.iter().cloned().collect();
        assert_eq!(
            got,
            [ProgramSpot {
                program: 3,
                spot: PanadapterSpot {
                    call: "K1ABC".into(),
                    hz: 14_025_000,
                    mode: "CWFT8X".into(),
                    argb: 0xFF00_FF00,
                    text: "CQ NA".into(),
                    heard_unix: T0,
                },
            }]
        );
    }

    /// ⛔ A CALLSIGN OR NOTHING: 1 to 16 letters, digits and `/`, shaped like a callsign. Each other
    /// is refused and counted. CONTROL: the callsigns a cluster carries are kept.
    #[test]
    fn a_spot_whose_callsign_is_not_one_is_refused_and_counted() {
        let bad = [
            "",
            " ",
            "K1ABC!",
            "K1ABC<B>",
            "<K1ABC>",
            "K1 ABC",
            "K1ABC/AAAAAAAAAAA",
            "HELLO",
            "73",
            "TRX:0,TRUE",
            "K1ABC\u{7}",
        ];
        let mut store = ProgramSpots::default();
        let refused: Vec<_> = bad
            .iter()
            .map(|c| store.put(1, raw(c, 14_025_000, "", T0), LIMITS))
            .collect();
        assert_eq!(refused, vec![Err(NotKept::Call); bad.len()]);
        assert_eq!(store.refused(), bad.len() as u64, "each refusal counted");
        assert!(kept(&store).is_empty());
        for good in [
            "W1AW",
            "k1abc",
            "VP2V/K1ABC/P",
            "3Y0J",
            "4U1UN",
            "K1ABC/AAAAAAAAAA",
        ] {
            assert_eq!(
                store.put(1, raw(good, 14_025_000, "", T0), LIMITS),
                Ok(()),
                "control: {good}"
            );
        }
        assert_eq!(store.refused(), bad.len() as u64, "control: none counted");
    }

    /// ⛔ INSIDE THE RANGE PROGRAMS ARE TOLD, ends included: outside it, refused and counted.
    #[test]
    fn a_spot_outside_the_range_programs_are_told_is_refused_and_counted() {
        let mut store = ProgramSpots::default();
        for hz in [LIMITS.0 - 1, LIMITS.1 + 1, 0, -14_025_000] {
            assert_eq!(
                store.put(1, raw("K1ABC", hz, "", T0), LIMITS),
                Err(NotKept::Frequency),
                "{hz} Hz"
            );
        }
        assert_eq!(store.refused(), 4);
        for hz in [LIMITS.0, LIMITS.1] {
            assert_eq!(
                store.put(1, raw("K1ABC", hz, "", T0), LIMITS),
                Ok(()),
                "control: {hz} Hz"
            );
        }
        assert_eq!(store.refused(), 4, "control: none counted");
    }

    /// ⛔ TEXT IS DATA: printable ASCII only, at most 64 characters; what is dropped or cut is
    /// counted. Markup is kept as the characters it is, for the band map to show as text.
    /// CONTROL: a clean 64-character text is kept whole and not counted.
    #[test]
    fn a_spots_text_is_printable_ascii_cut_to_64_characters_and_counted() {
        let mut store = ProgramSpots::default();
        let long = "X".repeat(100);
        let cases = [
            ("\u{7}BEL\u{1b}[31mred\r\n", "BEL[31mred"),
            ("caf\u{e9} \u{2603} ok", "caf  ok"),
            (long.as_str(), &long[..64]),
        ];
        for (n, (sent, want)) in cases.iter().enumerate() {
            let call = call_n(n);
            assert_eq!(
                store.put(1, raw(&call, 14_025_000, sent, T0), LIMITS),
                Ok(())
            );
            let text = store.iter().last().map(|s| s.spot.text.clone());
            assert_eq!(text.as_deref(), Some(*want), "{sent:?}");
        }
        assert_eq!(store.cut(), 3, "each cut counted");
        let markup = "<img src=x onerror=alert(1)>&amp;\"'";
        let exact = "Y".repeat(64);
        for (n, text) in [markup, exact.as_str(), "  padded  "].iter().enumerate() {
            store
                .put(2, raw(&call_n(n), 14_025_000, text, T0), LIMITS)
                .expect("kept");
        }
        let texts: Vec<_> = store
            .iter()
            .filter(|s| s.program == 2)
            .map(|s| s.spot.text.clone())
            .collect();
        assert_eq!(texts, [markup, exact.as_str(), "padded"], "kept as sent");
        assert_eq!(store.cut(), 3, "control: none counted");
    }

    /// ⛔ 200 A PROGRAM, OLDEST OUT FIRST, and another program's spots untouched by it.
    #[test]
    fn a_program_keeps_200_spots_and_its_oldest_goes_first() {
        let mut store = ProgramSpots::default();
        store
            .put(2, raw("W1AW", 7_030_000, "", T0), LIMITS)
            .expect("kept");
        for n in 0..=PROGRAM_SPOTS_EACH {
            store
                .put(1, raw(&call_n(n), 14_025_000, "", T0 + n as i64), LIMITS)
                .expect("kept");
        }
        let ones: Vec<_> = kept(&store)
            .into_iter()
            .filter(|(p, _)| *p == 1)
            .map(|(_, c)| c)
            .collect();
        assert_eq!(ones.len(), PROGRAM_SPOTS_EACH);
        assert_eq!(ones.first(), Some(&call_n(1)), "the first one went");
        assert_eq!(ones.last(), Some(&call_n(PROGRAM_SPOTS_EACH)));
        assert!(
            kept(&store).contains(&(2, "W1AW".into())),
            "another program's spot stays"
        );
    }

    /// ⛔ 500 IN ALL, OLDEST OUT FIRST, whoever sent it. CONTROL: 500 are all kept.
    #[test]
    fn all_programs_keep_500_spots_and_the_oldest_goes_first() {
        let mut store = ProgramSpots::default();
        let mut n = 0;
        for program in [1, 2] {
            for _ in 0..PROGRAM_SPOTS_EACH {
                store
                    .put(program, raw(&call_n(n), 14_025_000, "", T0), LIMITS)
                    .expect("kept");
                n += 1;
            }
        }
        for _ in 0..(PROGRAM_SPOTS_IN_ALL - 2 * PROGRAM_SPOTS_EACH) {
            store
                .put(3, raw(&call_n(n), 14_025_000, "", T0), LIMITS)
                .expect("kept");
            n += 1;
        }
        assert_eq!(
            store.iter().count(),
            PROGRAM_SPOTS_IN_ALL,
            "control: 500 kept"
        );
        assert_eq!(kept(&store).first(), Some(&(1, call_n(0))));
        store
            .put(3, raw(&call_n(n), 14_025_000, "", T0), LIMITS)
            .expect("kept");
        assert_eq!(store.iter().count(), PROGRAM_SPOTS_IN_ALL);
        assert_eq!(
            kept(&store).first(),
            Some(&(1, call_n(1))),
            "the oldest of all went"
        );
        assert_eq!(kept(&store).last(), Some(&(3, call_n(n))));
    }

    /// ⛔ 30 MINUTES: a spot is kept at 29:59 and gone at 30:00. A spot sent again starts its 30
    /// minutes again.
    #[test]
    fn a_spot_goes_30_minutes_after_it_was_sent() {
        let mut store = ProgramSpots::default();
        store
            .put(1, raw("K1ABC", 14_025_000, "", T0), LIMITS)
            .expect("kept");
        store
            .put(1, raw("W1AW", 14_030_000, "", T0), LIMITS)
            .expect("kept");
        store
            .put(1, raw("W1AW", 14_030_000, "", T0 + 600), LIMITS)
            .expect("sent again");
        store.age(T0 + PROGRAM_SPOT_MAX_AGE_SECS - 1);
        assert_eq!(kept(&store).len(), 2, "both at 29:59");
        store.age(T0 + PROGRAM_SPOT_MAX_AGE_SECS);
        assert_eq!(kept(&store), [(1, "W1AW".into())], "K1ABC gone at 30:00");
        store.age(T0 + 600 + PROGRAM_SPOT_MAX_AGE_SECS);
        assert!(kept(&store).is_empty());
    }

    /// ⛔ `spot_delete` AND `spot_clear` ARE THE SENDER'S: another program's spots of the same
    /// callsign survive both. CONTROL: the sender's own go.
    #[test]
    fn delete_and_clear_take_only_the_senders_own_spots() {
        let mut store = ProgramSpots::default();
        for program in [1, 2] {
            for call in ["K1ABC", "W1AW"] {
                store
                    .put(program, raw(call, 14_025_000, "", T0), LIMITS)
                    .expect("kept");
            }
        }
        store.delete(2, "k1abc");
        assert_eq!(
            kept(&store),
            [(1, "K1ABC".into()), (1, "W1AW".into()), (2, "W1AW".into())]
        );
        store.clear_program(2);
        assert_eq!(kept(&store), [(1, "K1ABC".into()), (1, "W1AW".into())]);
        store.delete(1, "W1AW");
        assert_eq!(kept(&store), [(1, "K1ABC".into())], "control");
    }

    /// A program's newer spot of a callsign takes the place of its older one, as the newest;
    /// another program's spot of it stays.
    #[test]
    fn a_newer_spot_of_a_callsign_replaces_the_programs_older_one() {
        let mut store = ProgramSpots::default();
        store
            .put(1, raw("K1ABC", 14_025_000, "", T0), LIMITS)
            .expect("kept");
        store
            .put(2, raw("K1ABC", 14_025_000, "", T0), LIMITS)
            .expect("kept");
        store
            .put(1, raw("W1AW", 7_030_000, "", T0), LIMITS)
            .expect("kept");
        store
            .put(1, raw("K1ABC", 14_026_000, "up", T0 + 5), LIMITS)
            .expect("kept");
        let got: Vec<_> = store
            .iter()
            .map(|s| (s.program, s.spot.call.as_str(), s.spot.hz))
            .collect();
        assert_eq!(
            got,
            [
                (2, "K1ABC", 14_025_000),
                (1, "W1AW", 7_030_000),
                (1, "K1ABC", 14_026_000)
            ]
        );
    }

    /// The generation moves with each change and only then.
    #[test]
    fn the_generation_moves_only_when_the_programs_spots_change() {
        let mut store = ProgramSpots::default();
        let g0 = store.generation();
        store
            .put(1, raw("K1ABC", 14_025_000, "", T0), LIMITS)
            .expect("kept");
        let g1 = store.generation();
        assert_ne!(g1, g0, "a spot kept");
        let _ = store.put(1, raw("HELLO", 14_025_000, "", T0), LIMITS);
        store.delete(1, "W1AW");
        store.clear_program(9);
        store.age(T0);
        assert_eq!(store.generation(), g1, "nothing changed");
        store.clear();
        assert_ne!(store.generation(), g1, "cleared");
    }

    /// A station whose TCI server serves programs' spots: the server and its spots tier on, an
    /// assisted entry.
    fn serving_spots() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.apply_restored_settings(Settings {
            tci_server: true,
            tci_server_spots: true,
            ..Settings::default()
        });
        e
    }

    fn sent(call: &str, hz: i64, text: &str) -> Spot {
        use tempo_net::tci::wire::{Call, Hz, Text, Token};
        Spot {
            call: Call::new(call).expect("a call"),
            mode: Token::new("CW").expect("a mode"),
            hz: Hz::new(hz).expect("a frequency"),
            argb: 0xFFFF_0000,
            text: Text::new(text).expect("a text"),
        }
    }

    /// ⛔ ONLY WHILE THEY ARE ON: the server, its "Spots from programs", and not an Unassisted
    /// entry, where a program's spots are help as the cluster's are. Off, a spot is refused as not
    /// offered, the band map reads none, and the next tick takes away what was kept. CONTROL: on,
    /// the same spot is kept and read.
    #[test]
    fn programs_spots_are_kept_only_while_the_server_its_spots_and_the_entry_allow() {
        type Off = fn(&mut Settings);
        let offs: [(&str, Off); 3] = [
            ("the server off", |s| s.tci_server = false),
            ("its spots off", |s| s.tci_server_spots = false),
            ("an Unassisted entry", |s| s.unassisted_mode = true),
        ];
        let now = now_unix_secs() as i64;
        for (what, off) in offs {
            let mut e = serving_spots();
            assert_eq!(
                e.tci_program_spot(1, &sent("K1ABC", 14_025_000, "CQ"), LIMITS),
                Ok(()),
                "control: on"
            );
            assert_eq!(e.tci_program_spots(now).len(), 1, "control: read");
            assert!(
                e.tci_program_spots(now + PROGRAM_SPOT_MAX_AGE_SECS + 1)
                    .is_empty(),
                "30 minutes on, the band map reads none before any tick"
            );
            let mut s = e.settings().clone();
            off(&mut s);
            e.apply_restored_settings(s);
            assert!(!e.tci_server_spots_on(), "{what}");
            assert_eq!(
                e.tci_program_spot(1, &sent("W1AW", 14_030_000, ""), LIMITS),
                Err(Refusal::Tier),
                "{what}: refused"
            );
            assert!(e.tci_program_spots(now).is_empty(), "{what}: none read");
            e.tci_program_spots_tick(now);
            assert_eq!(
                e.tci_program_spot_store().iter().count(),
                0,
                "{what}: the tick took them away"
            );
        }
    }

    /// ⛔ A SPOT WORKED IN NEXUS WITH ITS CALL IS TOLD TO THE PROGRAMS, ONCE, AND TELLING IS ALL IT
    /// DOES: from each state, the station after the Work is the station after the same Work with
    /// programs' spots off (whatever the Work itself does, it does either way), but for the click
    /// waiting to be told. CONTROL: off, nothing is told; a Work with no call tells nothing.
    #[test]
    fn a_spot_worked_with_its_call_is_told_to_the_programs_and_nothing_else_changes() {
        type Setup = fn(&mut Engine);
        let states: [(&str, Setup); 5] = [
            ("launch, TX off", |_| {}),
            ("Phone, armed", |e| {
                e.set_operating_mode("phone", false);
                e.set_tx_enabled_by_operator(true);
            }),
            ("Phone, after Stop TX", |e| {
                e.set_operating_mode("phone", false);
                e.set_tx_enabled_by_operator(true);
                e.halt_tx();
            }),
            ("CW, TX off", |e| {
                e.set_operating_mode("cw", false);
                e.set_tx_enabled(false);
            }),
            // The dial sits a pitch off the spot here: the programs are told the spot's own.
            ("CW, the soundcard keyer", |e| {
                e.set_operating_mode("cw", false);
                e.settings.cw_keyer = crate::settings::CwKeyerBackend::Soundcard;
            }),
        ];
        type Seen = (
            bool,
            bool,
            bool,
            Option<u64>,
            String,
            String,
            Option<String>,
            u64,
        );
        let seen = |e: &Engine| -> Seen {
            (
                e.tx_enabled,
                e.tx_stop_hold,
                e.tx_watchdog,
                e.tx_watchdog_start,
                format!("{:?}", e.settings.operating_mode),
                format!("{:.6}", e.settings.dial_mhz),
                e.qso_dxcall().map(str::to_string),
                e.snapshot().work_tick,
            )
        };
        for (state, setup) in states {
            let work = |spots: bool| {
                let mut e = serving_spots();
                e.settings.tci_server_spots = spots;
                setup(&mut e);
                e.work_spot_tiered(None, "cw", 14.025, "20m", None);
                e.note_work_call(Some("k1abc".into()));
                let told = e.take_tci_spot_clicks();
                (seen(&e), told)
            };
            let (on, told) = work(true);
            let (off, untold) = work(false);
            assert_eq!(on, off, "{state}: telling changed the station");
            assert_eq!(
                told,
                [("K1ABC".to_string(), 14_025_000)],
                "{state}: told once"
            );
            assert!(untold.is_empty(), "{state}: control, spots off");
        }
        let mut e = serving_spots();
        e.work_spot_tiered(None, "cw", 14.025, "20m", None);
        e.note_work_call(None);
        e.note_work_call(Some("K1ABC".into()));
        assert!(
            e.take_tci_spot_clicks().is_empty(),
            "control: a Work with no call tells nothing, and a call noted after it is no click"
        );
    }

    /// A click on a program's spot is told once it has tuned, and not when it was refused; the
    /// clicks waiting are bounded, the oldest dropped, and go with programs' spots.
    #[test]
    fn a_click_on_a_programs_spot_is_told_once_it_tuned_and_the_wait_is_bounded() {
        let mut e = serving_spots();
        assert_eq!(e.tci_program_spot_clicked("K1ABC", 14_025_000), Ok(()));
        assert_eq!(
            e.take_tci_spot_clicks(),
            [("K1ABC".to_string(), 14_025_000)]
        );
        assert!(e.take_tci_spot_clicks().is_empty(), "told once");
        assert!(e.tci_program_spot_clicked("HELLO", 14_025_000).is_err());
        assert!(e.take_tci_spot_clicks().is_empty(), "refused: nothing told");
        for n in 0..=SPOT_CLICKS_WAITING {
            e.note_tci_spot_click(&call_n(n), 14_025_000 + n as i64);
        }
        let waiting = e.take_tci_spot_clicks();
        assert_eq!(waiting.len(), SPOT_CLICKS_WAITING);
        assert_eq!(waiting[0].0, call_n(1), "the oldest went");
        e.note_tci_spot_click("K1ABC", 14_025_000);
        e.settings.tci_server_spots = false;
        e.tci_program_spots_tick(now_unix_secs() as i64);
        e.settings.tci_server_spots = true;
        assert!(e.take_tci_spot_clicks().is_empty(), "gone with the spots");
    }

    /// The engine's refusals are the connection log's: a callsign that is not one, a frequency
    /// outside the range; the store counts both.
    #[test]
    fn the_engine_refuses_a_bad_spot_in_the_connection_logs_words() {
        let mut e = serving_spots();
        assert_eq!(
            e.tci_program_spot(1, &sent("HELLO", 14_025_000, ""), LIMITS),
            Err(Refusal::SpotCall)
        );
        assert_eq!(
            e.tci_program_spot(1, &sent("K1ABC", 1_300_000_001, ""), LIMITS),
            Err(Refusal::Limits)
        );
        assert_eq!(e.tci_program_spot_store().refused(), 2);
        assert_eq!(
            e.tci_program_spot(1, &sent("K1ABC", 14_025_000, ""), LIMITS),
            Ok(()),
            "control"
        );
    }
}
