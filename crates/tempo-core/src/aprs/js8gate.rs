//! The JS8 `@APRSIS` gateway's APRS-IS lines: the position a heard `GRID` becomes, the raw packet a
//! heard `CMD` becomes, the acknowledgement for a message relayed to a JS8 station, and the one
//! kind of line that comes DOWN — an APRS message for a station heard on JS8.
//!
//! JS8Call 3.0.3 is the protocol, read as the specification: `JS8_Main/APRSISClient.cpp`
//! (`enqueueSpot`, `enqueueThirdParty`, `enqueueMessageAck`, `grid2deg`, `grid2aprs`,
//! `replaceCallsignSuffixWithSSID`, the inbound message pattern in `onSocketReadyRead`),
//! `JS8_Main/AprsInboundRelay.cpp` and `JS8_UI/mainwindow.cpp` `spotAprsGrid`/`spotAprsCmd`
//! (:2400-2454). Nothing here is its code. Pure: no socket and no clock. The engine decides WHEN
//! (its switches, the never-spot list, the heard list); `tempo_net::aprsis` carries the bytes on
//! the session the APRS screen and the iGate already use.
//!
//! Deliberate differences from JS8Call, each stated where it lives:
//! * **The TOCALL is Nexus's** ([`NEXUS_TOCALL`]), not JS8Call's `APJ8CL`. The destination names
//!   the software that built the packet, and APRS clients label the packet by it. The same rule
//!   as the PSK Reporter software name.
//! * **The frequency in a position's comment always uses a decimal point.** JS8Call formats it in
//!   the computer's locale, so a German JS8Call writes `14,078500MHz`.
//! * **Inbound lines are parsed strictly** (they are untrusted internet input that can end up
//!   keyed on the air): printable ASCII only, an addressee that is a bare callsign, the APRS
//!   message limits (67 characters of text, an id of at most 5), and none of the characters APRS
//!   reserves in message text (`{`, `|`, `~`). JS8Call takes any `(.*)`. A `rej` is protocol, like
//!   an `ack`, and is never relayed as text.

use super::packet::NEXUS_TOCALL;

/// JS8Call trims a position comment to 42 characters (`comment.left(42)`).
pub const MAX_COMMENT: usize = 42;
/// APRS 1.0.1: "The message text may be up to 67 characters long".
pub const MAX_MESSAGE_TEXT: usize = 67;
/// APRS 1.0.1: a message identifier is 1–5 alphanumerics.
pub const MAX_MESSAGE_ID: usize = 5;
/// The longest raw packet a heard `CMD` may become: the APRS information field's own limit.
pub const MAX_RAW_TEXT: usize = 256;

/// JS8Call's `Radio::base_callsign` (Radio.cpp:106-117): the longer side of the first `/`, the
/// right one on a tie, upper-cased.
pub fn base_callsign(call: &str) -> String {
    let call = call.trim();
    match call.find('/') {
        Some(slash) if call.len() - slash > slash => call[slash + 1..].to_ascii_uppercase(),
        Some(slash) => call[..slash].to_ascii_uppercase(),
        None => call.to_ascii_uppercase(),
    }
}

/// A JS8 callsign as an APRS source (`replaceCallsignSuffixWithSSID(call, base_callsign(call))`):
/// a numeric `/` suffix becomes the SSID (`KN4CRD/7` → `KN4CRD-7`), any other prefix or suffix is
/// dropped (`KN4CRD/P` → `KN4CRD`, `VE3/KN4CRD` → `KN4CRD`).
pub fn aprs_call(call: &str) -> String {
    let call = call.trim().to_ascii_uppercase();
    let base = base_callsign(&call);
    if call == base {
        return call;
    }
    // The first `/` followed by digits names the SSID; the digits run as far as they go.
    let bytes = call.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        let digits: String = call[i + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            return format!("{base}-{digits}");
        }
    }
    base
}

/// The locator as JS8Call's `grid_pattern` accepts it: two letters A–X, two digits, then any run of
/// letter pairs each optionally followed by a digit pair. `None` for anything else.
fn locator_pairs(grid: &str) -> Option<Vec<(u8, u8)>> {
    let g = grid.trim().as_bytes();
    let letters =
        |i: usize| g.len() >= i + 2 && g[i..i + 2].iter().all(|c| (b'A'..=b'X').contains(c));
    let digits = |i: usize| g.len() >= i + 2 && g[i..i + 2].iter().all(u8::is_ascii_digit);
    if !(letters(0) && digits(2)) {
        return None;
    }
    let mut pairs = vec![(g[0] - b'A', g[1] - b'A'), (g[2] - b'0', g[3] - b'0')];
    let mut i = 4;
    while i < g.len() {
        if !letters(i) {
            return None;
        }
        pairs.push((g[i] - b'A', g[i + 1] - b'A'));
        i += 2;
        if digits(i) {
            pairs.push((g[i] - b'0', g[i + 1] - b'0'));
            i += 2;
        }
    }
    Some(pairs)
}

/// JS8Call's `grid2deg` for a locator of any length: the SOUTH-WEST corner of the smallest square
/// it names (no half-square is added), accumulated in single precision as JS8Call accumulates it.
/// Each pair `i` weighs `10^⌊(1−(i−1))/2⌋ · 24^⌊−(i−1)/2⌋`: 10, 1, 1/24, 1/240, 1/5760, ….
/// Returns (latitude, longitude) in degrees.
fn locator_corner(pairs: &[(u8, u8)]) -> (f32, f32) {
    let mut lat: f32 = -90.0;
    let mut lon: f32 = -90.0;
    for (i, &(x, y)) in pairs.iter().enumerate() {
        let z = i as i32 - 1;
        let scale = (10f64.powi((1 - z).div_euclid(2)) * 24f64.powi((-z).div_euclid(2))) as f32;
        lon += scale * f32::from(x);
        lat += scale * f32::from(y);
    }
    (lat, lon * 2.0)
}

/// One coordinate as APRS writes it, JS8Call's `grid2aprs`: whole degrees, minutes and seconds
/// (seconds rounded, carrying into minutes and degrees), printed as `DDMM.mm` (latitude, 7 wide)
/// or `DDDMM.mm` (longitude, 8 wide) — the seconds are hundredths of a minute after rounding to
/// whole seconds, not before.
fn aprs_coordinate(value: f32, width: usize, pos: char, neg: char) -> String {
    let (v, dir) = if value < 0.0 {
        (-value, neg)
    } else {
        (value, pos)
    };
    let v = f64::from(v);
    let mut deg = v.trunc();
    let min_f = (v - deg) * 60.0;
    let mut min = min_f.trunc();
    let mut sec = ((min_f - min) * 60.0).round();
    if sec == 60.0 {
        min += 1.0;
        sec = 0.0;
    }
    if min == 60.0 {
        deg += 1.0;
        min = 0.0;
    }
    let aprs = deg * 100.0 + min + sec / 60.0;
    format!("{aprs:0width$.2}{dir}")
}

/// A locator as an APRS position: (`DDMM.mmN`, `DDDMM.mmW`). `None` when it is not a locator.
pub fn grid_to_aprs(grid: &str) -> Option<(String, String)> {
    let pairs = locator_pairs(grid)?;
    let (lat, lon) = locator_corner(&pairs);
    Some((
        aprs_coordinate(lat, 7, 'N', 'S'),
        aprs_coordinate(lon, 8, 'E', 'W'),
    ))
}

/// JS8Call's `Varicode::formatSNR`: a sign and two digits, empty outside ±60.
fn format_snr(snr: i32) -> String {
    if !(-60..=60).contains(&snr) {
        String::new()
    } else if snr < 0 {
        format!("-{:02}", -snr)
    } else {
        format!("+{snr:02}")
    }
}

/// Printable ASCII, no character a TNC2 line could be split or extended by.
fn printable(s: &str) -> bool {
    s.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

/// An APRS-IS source as a station names itself: letters and digits, then an optional `-` and a
/// one- or two-digit SSID, nine characters at most. A JS8 call whose base is longer than that, or
/// that keeps a `/` after [`aprs_call`], cannot be one.
fn aprs_source_ok(s: &str) -> bool {
    let (call, ssid) = match s.split_once('-') {
        Some((c, n)) => (c, Some(n)),
        None => (s, None),
    };
    !call.is_empty()
        && s.len() <= 9
        && call
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && ssid.is_none_or(|n| (1..=2).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The position a heard `@APRSIS GRID` becomes (`spotAprsGrid` → `enqueueSpot`):
/// `FROM>TOCALL,qAS,BY:=LAT/LONG#JS8 COMMENT`, symbol `/G`. The comment is the frequency the
/// station was heard on (dial + offset) and its SNR, `14.078500MHz -10dB`, after the station's
/// call when it has a `/` in it, trimmed to [`MAX_COMMENT`]. `call` is the station as heard,
/// `mycall` this station. `None` when the grid is not a locator or a call cannot be an APRS
/// source.
pub fn position_line(
    call: &str,
    mycall: &str,
    grid: &str,
    freq_hz: u64,
    snr: i32,
) -> Option<String> {
    let (lat, lon) = grid_to_aprs(grid)?;
    let (from, by) = (aprs_call(call), aprs_call(mycall));
    if !(aprs_source_ok(&from) && aprs_source_ok(&by)) {
        return None;
    }
    let mut comment = format!(
        "{}.{:06}MHz {}dB",
        freq_hz / 1_000_000,
        freq_hz % 1_000_000,
        format_snr(snr)
    );
    if call.contains('/') {
        comment = format!("{} {comment}", call.trim().to_ascii_uppercase());
    }
    let comment: String = comment.chars().take(MAX_COMMENT).collect();
    if !printable(&comment) {
        return None;
    }
    Some(format!(
        "{from}>{NEXUS_TOCALL},qAS,{by}:={lat}/{lon}G#JS8 {comment}"
    ))
}

/// The raw packet a heard `@APRSIS CMD` becomes (`spotAprsCmd` → `enqueueThirdParty`): the text
/// after `CMD` is the information field, `FROM>TOCALL,qAS,BY:TEXT`. The text came over the air
/// from a stranger, so it must be printable and at most [`MAX_RAW_TEXT`]; `None` otherwise.
pub fn raw_line(call: &str, mycall: &str, text: &str) -> Option<String> {
    let (from, by) = (aprs_call(call), aprs_call(mycall));
    if !(aprs_source_ok(&from) && aprs_source_ok(&by)) {
        return None;
    }
    if text.is_empty() || text.len() > MAX_RAW_TEXT || !printable(text) {
        return None;
    }
    Some(format!("{from}>{NEXUS_TOCALL},qAS,{by}:{text}"))
}

/// The acknowledgement for an APRS message taken for relay (`enqueueMessageAck`): sent AS the JS8
/// station it is addressed to, `FROM>APRS,TCPIP*::TO_______:ackID`, the addressee padded to nine.
/// `None` when a call cannot be an APRS source or the id is not one.
pub fn ack_line(from: &str, to: &str, id: &str) -> Option<String> {
    if !(aprs_source_ok(from) && aprs_source_ok(to)) {
        return None;
    }
    if id.is_empty() || id.len() > MAX_MESSAGE_ID || !id.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    let to: String = to.chars().take(9).collect();
    Some(format!("{from}>APRS,TCPIP*::{to:<9}:ack{id}"))
}

/// An APRS message from APRS-IS, as the inbound relay reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    /// The APRS source, SSID included (`N0CALL-9`).
    pub from: String,
    /// The addressee, padding removed (`W1AW`).
    pub to: String,
    /// The message text, its `{id` removed.
    pub text: String,
    /// The message identifier, when it carries one.
    pub id: Option<String>,
}

/// `ack…` or `rej…` followed by alphanumerics: protocol, never text to relay. (JS8Call skips the
/// `ack` form, `isAckMessage`; a `rej` is the same kind of line.)
pub fn is_ack_or_rej(text: &str) -> bool {
    let t = text.trim();
    t.len() > 3
        && (t[..3].eq_ignore_ascii_case("ack") || t[..3].eq_ignore_ascii_case("rej"))
        && t[3..].bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Parse one APRS-IS line as a message for the inbound relay: JS8Call's
/// `^([^>]+)>[^:]+::([A-Z0-9 ]{9}):(.*)$`, then its message id (`{` + alphanumerics, an optional
/// closing `}`), held to the strict rules in the module header. `None` for anything else — a
/// position, a bulletin, a server line, or a message that breaks a rule.
pub fn parse_inbound_message(line: &[u8]) -> Option<InboundMessage> {
    let line = std::str::from_utf8(line)
        .ok()?
        .trim_end_matches(['\r', '\n']);
    if !printable(line) {
        return None;
    }
    let (from, rest) = line.split_once('>')?;
    let (path, rest) = rest.split_once("::")?;
    if path.is_empty() || path.contains(':') {
        return None;
    }
    // The source: a callsign with an optional SSID, as an APRS station names itself.
    if from.is_empty()
        || from.len() > 9
        || !from
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
    {
        return None;
    }
    // The addressee: exactly nine characters, left-justified, space-padded, a bare call.
    if rest.len() < 10 || rest.as_bytes()[9] != b':' {
        return None;
    }
    let (field, text) = (&rest[..9], &rest[10..]);
    let to = field.trim_end_matches(' ');
    if to.is_empty()
        || !to
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return None;
    }
    let (text, id) = match text.trim().rfind('{') {
        Some(brace) => {
            let trimmed = text.trim();
            let suffix = trimmed[brace + 1..]
                .strip_suffix('}')
                .unwrap_or(&trimmed[brace + 1..]);
            if suffix.is_empty()
                || suffix.len() > MAX_MESSAGE_ID
                || !suffix.bytes().all(|b| b.is_ascii_alphanumeric())
            {
                return None;
            }
            (
                trimmed[..brace].trim().to_string(),
                Some(suffix.to_string()),
            )
        }
        None => (text.trim().to_string(), None),
    };
    if text.is_empty() || text.len() > MAX_MESSAGE_TEXT || text.contains(['{', '|', '~']) {
        return None;
    }
    Some(InboundMessage {
        from: from.to_string(),
        to: to.to_string(),
        text,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_js8_call_becomes_an_aprs_source_as_js8call_makes_it() {
        assert_eq!(aprs_call("KN4CRD"), "KN4CRD");
        assert_eq!(
            aprs_call("kn4crd/7"),
            "KN4CRD-7",
            "a numeric suffix is the SSID"
        );
        assert_eq!(
            aprs_call("KN4CRD/P"),
            "KN4CRD",
            "any other suffix is dropped"
        );
        assert_eq!(aprs_call("VE3/KN4CRD"), "KN4CRD", "a prefix is dropped");
        assert_eq!(aprs_call("KN4CRD/12"), "KN4CRD-12");
    }

    #[test]
    fn a_locator_becomes_the_south_west_corner_in_aprs_form() {
        // Four characters: FN04 is 44°N 80°W exactly.
        assert_eq!(
            grid_to_aprs("FN04"),
            Some(("4400.00N".to_string(), "08000.00W".to_string()))
        );
        // Six: TV adds 21/24° of latitude (52.5') and 2·19/24° of longitude (1°35'): 44°52'30"N
        // and 78°25'W. 19/24 is inexact in single precision, so the longitude lands a few
        // millionths of a degree off 25': below it, the seconds round to 60 and carry into the
        // minutes; above it they round to 0. Either way the line reads 25.00.
        assert_eq!(
            grid_to_aprs("FN04TV"),
            Some(("4452.50N".to_string(), "07825.00W".to_string()))
        );
        // The user guide's example, eight characters: 3/240° of latitude (45") and 2·5/240° of
        // longitude (2'30") more: 44°53'15"N 78°22'30"W.
        assert_eq!(
            grid_to_aprs("FN04TV53"),
            Some(("4453.25N".to_string(), "07822.50W".to_string()))
        );
        // The southern and eastern hemispheres: QF56 is 34°S 150°E.
        assert_eq!(
            grid_to_aprs("QF56"),
            Some(("3400.00S".to_string(), "15000.00E".to_string()))
        );
        // CONTROLS: not locators.
        assert_eq!(grid_to_aprs("FN0"), None);
        assert_eq!(grid_to_aprs("FN04T"), None);
        assert_eq!(
            grid_to_aprs("ZZ04"),
            None,
            "Y and Z are outside JS8Call's [A-X]"
        );
        assert_eq!(
            grid_to_aprs("fn04"),
            None,
            "JS8Call's pattern is upper-case"
        );
    }

    #[test]
    fn a_heard_grid_becomes_the_position_line_js8call_sends() {
        assert_eq!(
            position_line("KN4CRD", "KD9TAW", "FN04TV53", 14_078_500, -10).as_deref(),
            Some("KN4CRD>APZNEX,qAS,KD9TAW:=4453.25N/07822.50WG#JS8 14.078500MHz -10dB")
        );
        // A call with a `/` leads its comment, and its SSID comes from the suffix.
        assert_eq!(
            position_line("KN4CRD/7", "KD9TAW/P", "FN04", 7_078_000, 3).as_deref(),
            Some("KN4CRD-7>APZNEX,qAS,KD9TAW:=4400.00N/08000.00WG#JS8 KN4CRD/7 7.078000MHz +03dB")
        );
        // The comment is cut at 42 characters. No real frequency reaches the cut, so this one is
        // absurd on purpose: 13 + 1 + 24 + 1 + 5 = 44 characters, so the cut takes the `dB`.
        let long = position_line("VE3XYZ/KN4CRD", "KD9TAW", "FN04", u64::MAX, -10).unwrap();
        assert_eq!(
            long.split("#JS8 ").nth(1),
            Some("VE3XYZ/KN4CRD 18446744073709.551615MHz -10")
        );
        // CONTROLS: no position from a non-locator, or for a call that cannot be an APRS source.
        assert_eq!(
            position_line("KN4CRD", "KD9TAW", "HELLO", 14_078_500, -10),
            None
        );
        assert_eq!(
            position_line("VE3/KN4CRD/12", "KD9TAW", "FN04", 14_078_500, -10),
            None
        );
        assert_eq!(position_line("KN4CRD", "", "FN04", 14_078_500, -10), None);
    }

    #[test]
    fn a_heard_cmd_becomes_its_raw_packet_and_nothing_unprintable_goes_up() {
        assert_eq!(
            raw_line("KN4CRD", "KD9TAW", ":SMSGTE   :@5551212 TEST").as_deref(),
            Some("KN4CRD>APZNEX,qAS,KD9TAW::SMSGTE   :@5551212 TEST")
        );
        assert_eq!(raw_line("KN4CRD", "KD9TAW", "BAD\r\nW1AW>APRS:X"), None);
        assert_eq!(raw_line("KN4CRD", "KD9TAW", ""), None);
        assert_eq!(
            raw_line("KN4CRD", "KD9TAW", &"X".repeat(MAX_RAW_TEXT + 1)),
            None
        );
        assert!(raw_line("KN4CRD", "KD9TAW", &"X".repeat(MAX_RAW_TEXT)).is_some());
    }

    #[test]
    fn the_ack_goes_up_as_the_js8_station_with_a_nine_character_addressee() {
        assert_eq!(
            ack_line("W1AW", "N0CALL-9", "12").as_deref(),
            Some("W1AW>APRS,TCPIP*::N0CALL-9 :ack12")
        );
        assert_eq!(ack_line("W1AW", "N0CALL", "").as_deref(), None);
        assert_eq!(ack_line("W1AW", "N0CALL", "123456").as_deref(), None);
        assert_eq!(ack_line("W1AW/P", "N0CALL", "1").as_deref(), None);
    }

    #[test]
    fn an_inbound_message_is_parsed_strictly() {
        let m =
            parse_inbound_message(b"N0CALL-9>APDR16,TCPIP*,qAC,T2TEXAS::W1AW     :HELLO THERE{12")
                .unwrap();
        assert_eq!(
            m,
            InboundMessage {
                from: "N0CALL-9".into(),
                to: "W1AW".into(),
                text: "HELLO THERE".into(),
                id: Some("12".into()),
            }
        );
        // The APRS 1.1 closing brace, and no id at all.
        assert_eq!(
            parse_inbound_message(b"N0CALL>APRS::W1AW     :HI{AB}")
                .unwrap()
                .id
                .as_deref(),
            Some("AB")
        );
        assert_eq!(
            parse_inbound_message(b"N0CALL>APRS::W1AW     :HI")
                .unwrap()
                .id,
            None
        );
        // Every rule, by a line that breaks only it.
        for bad in [
            &b"N0CALL>APRS::W1AW     :HI\x07"[..],   // a control character
            b"N0CALL>APRS::W1AW     :H\xC3\xA9",     // not ASCII
            b"n0call>APRS::W1AW     :HI",            // a lower-case source
            b"N0CALL-TOOLONG>APRS::W1AW     :HI",    // a source over nine
            b"N0CALL>::W1AW     :HI",                // no path
            b"N0CALL>APRS::W1AW    :HI",             // an addressee of eight
            b"N0CALL>APRS:: W1AW    :HI",            // not left-justified
            b"N0CALL>APRS::W1AW-7   :HI",            // an SSID is not a JS8 call
            b"N0CALL>APRS::W1AW     :",              // no text
            b"N0CALL>APRS::W1AW     :HI{123456",     // an id over five
            b"N0CALL>APRS::W1AW     :HI{1-2",        // an id that is not alphanumeric
            b"N0CALL>APRS::W1AW     :A|B",           // a reserved character
            b"N0CALL>APRS:=4453.25N/07822.50WG#JS8", // not a message
            b"# logresp N0CALL verified, server T2TEXAS", // a server line
        ] {
            assert_eq!(
                parse_inbound_message(bad),
                None,
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        let max = format!("N0CALL>APRS::W1AW     :{}", "X".repeat(MAX_MESSAGE_TEXT));
        assert!(
            parse_inbound_message(max.as_bytes()).is_some(),
            "67 characters is the limit"
        );
        let over = format!(
            "N0CALL>APRS::W1AW     :{}",
            "X".repeat(MAX_MESSAGE_TEXT + 1)
        );
        assert_eq!(parse_inbound_message(over.as_bytes()), None);
    }

    #[test]
    fn acks_and_rejects_are_protocol_not_text() {
        assert!(is_ack_or_rej("ack12"));
        assert!(is_ack_or_rej("REJ7"));
        assert!(!is_ack_or_rej("ACKNOWLEDGED, THANKS"));
        assert!(!is_ack_or_rej("ack"));
        assert!(!is_ack_or_rej("HELLO"));
    }
}
