//! One contact, one spelling of its band and of its phone mode, wherever it came from.
//!
//! ADIF's `BAND` and `MODE` are enumerations, and the specification is plain about how they
//! compare: *"Enumeration values are case insensitive"* (ADIF 3.1.6, III.B). So `20M` written by
//! another logger, `20m` written by Nexus and a LoTW download's `20M` are one band — but a log
//! that keeps each source's spelling hands every reader two strings for it, and every reader that
//! groups on the exact string (the statistics' BY BAND, the globe's band list and its per-band
//! squares, one call's history) shows the band twice. The Band enumeration spells every band in
//! lower case (III.B.4: `160m`, `20m`, `2m`, `70cm`), which is Nexus's own spelling, so that is
//! the one the log holds.
//!
//! The Mode enumeration lists `SSB` with its submodes `LSB, USB` (III.B.10), and `SUBMODE` is the
//! field for them. A contact whose MODE names the sideband — another logger's `MODE=USB`, or
//! Nexus's own phone contact, logged on the sideband the radio was on — is held as `SSB` with the
//! sideband as its SUBMODE: the same contact Log4OM writes as `MODE=SSB SUBMODE=USB`, in the same
//! shape, and the sideband is kept. The writer puts that SUBMODE beside its MODE
//! ([`super::adif_record`]).
//!
//! # Held where a contact enters the log, not folded by each reader
//!
//! A contact enters the model in a few places, and each one calls [`contact`]:
//!
//! - every ADIF read ([`super::parse_adif`]'s `record_from`) — an import, a confirmation download
//!   (LoTW, eQSL, a QRZ logbook fetch), the companion's logged-contact datagram, a `log.adi`
//!   another program wrote, the conversion of a `log.adi` into the store;
//! - every read of the store ([`super::sqlite`]'s decoder, whole rows and narrow ones), which is
//!   what makes a log stored before this hold the same form — read in it, without a migration:
//!   a stored row keeps the spelling it was written in until a change writes it again;
//! - the station's log path (`Engine::log_qso`, every contact Nexus logs, BEFORE its duplicate
//!   guard compares the band and the mode), the station's append of any row it adds
//!   (`StationCore::append`), an edit (`edited`), and a contest contact merged into the lifetime
//!   log.
//!
//! The readers that fold the case themselves (the worked-before sets, the dupe and confirmation
//! keys, the award bands) keep their folds: they are right on any input, and a fold costs nothing
//! on a value already in its form.
//!
//! # What it never touches
//!
//! The call, the times, the confirmations and credit, and the country text — `COUNTRY` is free
//! text (*"the contacted station's DXCC entity name"*, a String), spelled as its source spelled
//! it. Of the band only the case and the padding change, and of the mode only a sideband in it.
//! A record with no band keeps none.

use super::QsoRecord;

/// A band as the log holds it: trimmed, in the Band enumeration's own lower case (`20M` →
/// `20m`, `70CM` → `70cm`). `""` stays `""`, and a value the enumeration does not know keeps its
/// letters, only their case changes.
pub fn band(band: &str) -> String {
    band.trim().to_ascii_lowercase()
}

/// ★ Hold `rec` in the log's one form: its band as [`band`] spells it, and a sideband written as
/// its mode moved to `SSB` with the sideband as its SUBMODE. Nothing else changes.
///
/// A row that already carries a SUBMODE keeps it as it came, and only its MODE becomes `SSB`: the
/// row said what its submode is. So the mode a row is held under depends on its MODE alone, and a
/// narrow read of the store (which reads no SUBMODE) holds the same mode as a whole one — the hot
/// index compares the two.
pub fn contact(rec: &mut QsoRecord) {
    if rec.band.trim().len() != rec.band.len() || rec.band.bytes().any(|b| b.is_ascii_uppercase()) {
        rec.band = band(&rec.band);
    }
    let sideband = match rec.mode.trim() {
        m if m.eq_ignore_ascii_case("USB") => "USB",
        m if m.eq_ignore_ascii_case("LSB") => "LSB",
        _ => return,
    };
    rec.mode = "SSB".to_string();
    if !rec.extra.iter().any(|(k, _)| k == "SUBMODE") {
        // In name order, as the parser and the store both keep `extra`.
        let at = rec.extra.partition_point(|(k, _)| k.as_str() < "SUBMODE");
        rec.extra
            .insert(at, ("SUBMODE".to_string(), sideband.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::query::{reference, BandsInLog, GridCounts, LabelCount, LogStatCounter};
    use super::super::{adif_record, edited, parse_adif, QsoRecord};

    fn tag(name: &str, value: &str) -> String {
        format!("<{name}:{}>{value}", value.len())
    }

    /// A contact as another logger exports it: the band and the country in its own spelling, a
    /// paper card and an eQSL, and no signal reports.
    fn imported(call: &str, band: &str, mode: &str, grid: &str) -> String {
        [
            tag("CALL", call),
            tag("QSO_DATE", "20261001"),
            tag("TIME_ON", "141500"),
            tag("BAND", band),
            tag("FREQ", "14.074000"),
            tag("MODE", mode),
            tag("GRIDSQUARE", grid),
            tag("COUNTRY", "UNITED STATES"),
            tag("QSL_RCVD", "Y"),
            tag("EQSL_QSL_RCVD", "Y"),
        ]
        .concat()
            + "<EOR>\n"
    }

    /// A contact as Nexus logs it: its own spelling of the band, the country from cty.dat.
    fn logged(call: &str, band: &str, mode: &str, grid: &str) -> String {
        [
            tag("CALL", call),
            tag("GRIDSQUARE", grid),
            tag("COUNTRY", "United States"),
            tag("BAND", band),
            tag("FREQ", "14.075500"),
            tag("MODE", mode),
            tag("QSO_DATE", "20261002"),
            tag("TIME_ON", "010203"),
            tag("TIME_OFF", "010300"),
            tag("RST_SENT", "+11"),
            tag("RST_RCVD", "+09"),
        ]
        .concat()
            + "<EOR>\n"
    }

    fn one(text: &str) -> QsoRecord {
        let mut rows = parse_adif(text);
        assert_eq!(rows.len(), 1, "fixture: one record in {text}");
        rows.remove(0)
    }

    fn submode(r: &QsoRecord) -> Option<&str> {
        r.extra
            .iter()
            .find(|(k, _)| k == "SUBMODE")
            .map(|(_, v)| v.as_str())
    }

    fn submodes(adif: &str) -> usize {
        adif.matches("<SUBMODE:").count()
    }

    /// ★ The operator's report: an imported `20M` contact and a Nexus-logged `20m` one are one
    /// band in BY BAND, both counted — not a `20M` row and a `20m` row.
    #[test]
    fn an_imported_20m_and_a_logged_20m_are_one_band_in_the_statistics() {
        let log = vec![
            one(&imported("K1ABC", "20M", "FT8", "EN52")),
            one(&logged("K2DEF", "20m", "FT8", "FN20")),
        ];
        let counts = reference::fold(&log, LogStatCounter::new(&|_: &QsoRecord| None));
        assert_eq!(
            counts.by_band,
            vec![LabelCount {
                label: "20m".into(),
                count: 2
            }]
        );
    }

    /// The globe's band list, its squares on one band, and one call's history of bands: a filter
    /// on 20 m finds the imported contact too.
    #[test]
    fn the_globe_and_a_calls_history_see_one_20m() {
        let log = vec![
            one(&imported("K1ABC", "20M", "FT8", "EN52")),
            one(&logged("K1ABC", "20m", "FT8", "EN52")),
            one(&logged("K2DEF", "20m", "FT8", "FN20")),
        ];
        assert_eq!(reference::fold(&log, BandsInLog::default()), ["20m"]);
        let squares: Vec<(String, usize)> = reference::fold(&log, GridCounts::new("20m"))
            .into_iter()
            .map(|s| (s.grid, s.n))
            .collect();
        assert_eq!(squares, [("EN52".to_string(), 2), ("FN20".to_string(), 1)]);
        let h = reference::call_history(&log, "K1ABC", "20m", "FT8", false);
        assert_eq!(h.bands, ["20m"]);
    }

    /// Every case a source writes a band in reads as the ADIF Band enumeration's own spelling,
    /// and is written back that way; a contact with no band keeps none.
    #[test]
    fn every_spelling_of_a_band_is_held_in_the_enumerations_own() {
        for (written, held) in [
            ("20M", "20m"),
            ("20m", "20m"),
            ("70CM", "70cm"),
            ("2M", "2m"),
            ("6M", "6m"),
            ("160M", "160m"),
            ("1.25M", "1.25m"),
            (" 40m ", "40m"),
        ] {
            let r = one(&imported("W1AW", written, "FT8", "FN31"));
            assert_eq!(r.band, held, "{written:?}");
            assert!(
                adif_record(&r).contains(&tag("BAND", held)),
                "{written:?} is written back as {held}: {}",
                adif_record(&r)
            );
        }
        // CONTROL: no band stays no band — the record carries none and writes none.
        let none = one(&(tag("CALL", "W1AW")
            + &tag("QSO_DATE", "20261001")
            + &tag("MODE", "FT8")
            + "<EOR>"));
        assert_eq!(none.band, "");
        assert!(!adif_record(&none).contains("<BAND:"));
    }

    /// ADIF's Mode enumeration lists `SSB` with the submodes `LSB, USB`: a contact that arrives
    /// with the sideband in MODE is held as SSB with the sideband as its SUBMODE, and written so,
    /// once. The forms already right are left as they are.
    #[test]
    fn a_sideband_in_mode_is_held_as_ssb_with_the_sideband_as_its_submode() {
        let ours_before =
            tag("MODE", "SSB") + &tag("SUBMODE", "USB") + &tag("APP_TEMPO_MODE", "USB");
        for (mode_tags, held, sub) in [
            (tag("MODE", "USB"), "SSB", Some("USB")),
            (tag("MODE", "LSB"), "SSB", Some("LSB")),
            (tag("MODE", "lsb"), "SSB", Some("LSB")),
            // Nexus's own file since the sideband was recorded.
            (ours_before.clone(), "SSB", Some("USB")),
            // A row that says both: its own SUBMODE stands, and is the one written.
            (
                tag("MODE", "LSB") + &tag("SUBMODE", "USB"),
                "SSB",
                Some("USB"),
            ),
            // CONTROLS: Log4OM's shape, a phone contact with no sideband, and other modes.
            (
                tag("MODE", "SSB") + &tag("SUBMODE", "LSB"),
                "SSB",
                Some("LSB"),
            ),
            (tag("MODE", "SSB"), "SSB", None),
            (tag("MODE", "FM"), "FM", None),
            (tag("MODE", "FT8"), "FT8", None),
        ] {
            let text = tag("CALL", "W9XYZ")
                + &tag("QSO_DATE", "20261001")
                + &tag("BAND", "40m")
                + &mode_tags
                + "<EOR>";
            let r = one(&text);
            assert_eq!((r.mode.as_str(), submode(&r)), (held, sub), "{mode_tags}");
            let out = adif_record(&r);
            assert!(out.contains(&tag("MODE", held)), "{mode_tags}: {out}");
            assert_eq!(
                submodes(&out),
                usize::from(sub.is_some()),
                "{mode_tags}: {out}"
            );
            if let Some(s) = sub {
                assert!(
                    out.contains(&(tag("MODE", "SSB") + &tag("SUBMODE", s))),
                    "{mode_tags}: the sideband rides beside its mode: {out}"
                );
            }
            assert!(
                !out.contains("<MODE:3>USB") && !out.contains("<MODE:3>LSB"),
                "{out}"
            );
        }
    }

    /// Phone groups as SSB by value: the statistics' mode breakdown, and one call's history of
    /// modes, whichever side of the sideband a contact was worked on and wherever it came from.
    #[test]
    fn phone_contacts_group_as_ssb_whatever_their_sideband() {
        let log = vec![
            one(&imported("VE3ABC", "40M", "USB", "EN61")),
            one(&logged("VE3ABC", "40m", "LSB", "EN61")),
            one(&(tag("CALL", "VE3ABC")
                + &tag("QSO_DATE", "20261003")
                + &tag("BAND", "40m")
                + &tag("MODE", "SSB")
                + &tag("SUBMODE", "LSB")
                + "<EOR>")),
        ];
        let counts = reference::fold(&log, LogStatCounter::new(&|_: &QsoRecord| None));
        assert_eq!(
            counts.by_mode,
            vec![LabelCount {
                label: "SSB".into(),
                count: 3
            }]
        );
        let h = reference::call_history(&log, "VE3ABC", "40m", "SSB", true);
        assert_eq!(
            (h.bands, h.modes),
            (vec!["40m".to_string()], vec!["SSB".to_string()])
        );
    }

    /// A hand edit holds what it writes in the same form as every other way in: a band typed in
    /// capitals, and a corrected sideband.
    #[test]
    fn an_edit_is_held_in_the_same_form() {
        let old = one(&(tag("CALL", "W9XYZ")
            + &tag("QSO_DATE", "20261001")
            + &tag("BAND", "40m")
            + &tag("MODE", "SSB")
            + &tag("SUBMODE", "LSB")
            + "<EOR>"));
        let mut form = old.clone();
        form.extra = Vec::new();
        form.band = "20M".into();
        form.mode = "USB".into();
        let now = edited(&old, form);
        assert_eq!(
            (now.band.as_str(), now.mode.as_str(), submode(&now)),
            ("20m", "SSB", Some("USB"))
        );
        assert_eq!(submodes(&adif_record(&now)), 1);
    }
}
