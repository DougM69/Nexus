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

use std::sync::Arc;

use super::Engine;

/// One spot for the SDR program's panadapter, as the station chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanadapterSpot {
    /// The station, as the spot named it.
    pub call: String,
    /// Where it was heard, in Hz.
    pub hz: i64,
    /// Its mode, as the panadapter labels it ("CW", "SSB", "FT8").
    pub mode: String,
    /// The colour of the need it is for, ARGB (`0xAARRGGBB`): the Needed board's colour for it.
    pub argb: u32,
    /// Why it is needed, in words ("New one", "New band / POTA").
    pub text: String,
    /// When it was last heard, in Unix seconds: the panadapter loses it once that is old.
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
