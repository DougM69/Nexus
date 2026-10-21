//! What the recorder removes from a server's messages before anything is written.
//!
//! A TCI session carries little that identifies anyone: frequencies, modes, levels, the device's
//! model. What could: an address (the server's own, or one a server puts in a vendor command) or a
//! serial number. So, **replaced**, each distinct original by its own stable placeholder (two
//! fields that held the same address still match after scrubbing):
//!
//! - **IPv4 addresses** anywhere in a message: `192.0.2.x`, then `198.51.100.x` and `203.0.113.x`
//!   (the RFC 5737 documentation ranges). A dotted quad in the version of a `protocol` line, or of
//!   a command whose name holds `version`, is kept: Thetis versions have four parts.
//! - **IPv6 addresses**: `2001:db8::x` (RFC 3849).
//! - **MAC addresses** (`hh:hh:hh:hh:hh:hh` or with `-`): `02:00:00:00:xx:xx`.
//! - **Serial numbers**: every argument of a command whose name holds `serial`: `serial-N`.
//!
//! **Kept** as recorded: everything else, the device name, frequencies, modes, spots and the
//! callsigns a server sends (which are other stations', from spots and CW messages). The tester
//! reads the file before sending it.

use std::collections::HashMap;
use std::net::Ipv6Addr;

/// The kinds of thing replaced, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Ipv4,
    Ipv6,
    Mac,
    Serial,
}

const KINDS: [Kind; 4] = [Kind::Ipv4, Kind::Ipv6, Kind::Mac, Kind::Serial];

impl Kind {
    /// How the recorder's summary names this kind.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Ipv4 => "IPv4 addresses",
            Kind::Ipv6 => "IPv6 addresses",
            Kind::Mac => "MAC addresses",
            Kind::Serial => "serial numbers",
        }
    }
}

/// Replaces what [`Kind`] lists, consistently across a whole recording.
#[derive(Debug, Default)]
pub struct Scrubber {
    seen: HashMap<(Kind, String), String>,
    counts: HashMap<Kind, usize>,
}

impl Scrubber {
    pub fn new() -> Scrubber {
        Scrubber::default()
    }

    /// The placeholder for `original` of `kind`, the same one every time.
    fn placeholder(&mut self, kind: Kind, original: &str) -> String {
        if let Some(p) = self.seen.get(&(kind, original.to_string())) {
            return p.clone();
        }
        let n = self.counts.entry(kind).or_insert(0);
        *n += 1;
        let n = *n;
        let p = match kind {
            Kind::Ipv4 => {
                let (net, host) = ((n - 1) / 254, (n - 1) % 254 + 1);
                match net {
                    0 => format!("192.0.2.{host}"),
                    1 => format!("198.51.100.{host}"),
                    _ => format!("203.0.113.{host}"),
                }
            }
            Kind::Ipv6 => format!("2001:db8::{n:x}"),
            Kind::Mac => format!("02:00:00:00:{:02x}:{:02x}", (n >> 8) & 0xFF, n & 0xFF),
            Kind::Serial => format!("serial-{n}"),
        };
        self.seen.insert((kind, original.to_string()), p.clone());
        p
    }

    /// Each kind replaced so far, with its count of distinct originals.
    pub fn report(&self) -> Vec<(Kind, usize)> {
        KINDS
            .iter()
            .filter_map(|k| self.counts.get(k).map(|n| (*k, *n)))
            .collect()
    }

    /// One text message, scrubbed. The message is cut at `;` into commands, so a command's name
    /// decides what its arguments are; text that is not a command is scanned for addresses all
    /// the same.
    pub fn text(&mut self, message: &str) -> String {
        let mut out = String::with_capacity(message.len());
        for piece in message.split_inclusive(';') {
            let (body, end) = match piece.strip_suffix(';') {
                Some(body) => (body, ";"),
                None => (piece, ""),
            };
            match body.split_once(':') {
                Some((name, args)) => {
                    let lower = name.trim().to_ascii_lowercase();
                    let serial = lower.contains("serial");
                    let versioned = lower == "protocol" || lower.contains("version");
                    out.push_str(&self.addresses(name, false));
                    out.push(':');
                    let args: Vec<String> = args
                        .split(',')
                        .enumerate()
                        .map(|(i, arg)| {
                            if serial && !arg.trim().is_empty() {
                                self.placeholder(Kind::Serial, arg.trim())
                            } else {
                                // `protocol:<program>,<version>`: the version is a version.
                                self.addresses(arg, versioned && i >= 1)
                            }
                        })
                        .collect();
                    out.push_str(&args.join(","));
                }
                None => out.push_str(&self.addresses(body, false)),
            }
            out.push_str(end);
        }
        out
    }

    /// `text` with every address replaced; with `keep_quads`, IPv4-shaped quads are versions.
    fn addresses(&mut self, text: &str, keep_quads: bool) -> String {
        let text = self.replace_runs(
            text,
            Kind::Mac,
            |c| c.is_ascii_hexdigit() || c == ':' || c == '-',
            is_mac,
        );
        let text = self.replace_runs(
            &text,
            Kind::Ipv6,
            |c| c.is_ascii_hexdigit() || c == ':' || c == '.',
            is_ipv6,
        );
        if keep_quads {
            return text;
        }
        self.replace_runs(
            &text,
            Kind::Ipv4,
            |c| c.is_ascii_digit() || c == '.',
            is_ipv4,
        )
    }

    /// Replace each maximal run of `part` characters that `matches` as an address of `kind`.
    fn replace_runs(
        &mut self,
        text: &str,
        kind: Kind,
        part: impl Fn(char) -> bool,
        matches: impl Fn(&str) -> bool,
    ) -> String {
        let mut out = String::with_capacity(text.len());
        let mut run = String::new();
        for c in text.chars().chain(std::iter::once('\u{0}')) {
            if c != '\u{0}' && part(c) {
                run.push(c);
                continue;
            }
            if !run.is_empty() {
                let trimmed = run.trim_matches(|c| c == '.' || c == ':' || c == '-');
                if !trimmed.is_empty() && matches(trimmed) {
                    let start = run.find(trimmed).unwrap_or(0);
                    out.push_str(&run[..start]);
                    let p = self.placeholder(kind, trimmed);
                    out.push_str(&p);
                    out.push_str(&run[start + trimmed.len()..]);
                } else {
                    out.push_str(&run);
                }
                run.clear();
            }
            if c != '\u{0}' {
                out.push(c);
            }
        }
        out
    }
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| (1..=3).contains(&p.len()) && p.parse::<u16>().is_ok_and(|v| v <= 255))
}

fn is_ipv6(s: &str) -> bool {
    s.matches(':').count() >= 2 && s.parse::<Ipv6Addr>().is_ok_and(|a| !a.is_unspecified())
}

fn is_mac(s: &str) -> bool {
    let sep = if s.contains(':') { ':' } else { '-' };
    let parts: Vec<&str> = s.split(sep).collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_serials_are_replaced_consistently() {
        let mut s = Scrubber::new();
        assert_eq!(
            s.text("x_vendor:192.168.1.20,fe80::1c2:3ff:fe4d:5e6f,00:1C:2D:00:00:01;"),
            "x_vendor:192.0.2.1,2001:db8::1,02:00:00:00:00:01;"
        );
        // The same address again gets the same placeholder; a new one the next.
        assert_eq!(
            s.text("note:192.168.1.20 and 10.0.0.7;"),
            "note:192.0.2.1 and 192.0.2.2;"
        );
        assert_eq!(s.text("device_serial:SN-12345;"), "device_serial:serial-1;");
        assert_eq!(
            s.text("x_serial_no:SN-12345,AB;"),
            "x_serial_no:serial-1,serial-2;"
        );
        let kinds: Vec<Kind> = s.report().into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, [Kind::Ipv4, Kind::Ipv6, Kind::Mac, Kind::Serial]);
    }

    #[test]
    fn what_tci_normally_says_passes_untouched() {
        let mut s = Scrubber::new();
        for line in [
            "vfo:0,0,14074000;",
            "protocol:Thetis,2.10.3.14;",
            "protocol:ExpertSDR3,2.0;",
            "rx_channel_sensors:0,0,-71.5;",
            "if_limits:-48000,48000;",
            "modulations_list:LSB,USB,DSB,CW,FMN,AM,DIGU,SPEC,DIGL,SAM,DRM;",
            "audio_stream_channels: 2",
            "spot:K1ABC,CW,14025000,4294901760,needed;",
            "callsign_send:RA6LH;",
            "tx_sensors:0,-27.2,47.4,67.5,1.7;",
            "vfo:0,0,14074000;dds:0,14080000;ready;start;",
            "",
        ] {
            assert_eq!(s.text(line), line);
        }
        assert!(s.report().is_empty());
    }
}
