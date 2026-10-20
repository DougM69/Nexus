//! The session file the recorder writes, and how the simulator replays one.
//!
//! ```text
//! tcisession 1                         the format line, first
//! # a comment                          whole-line comments
//! port 50001                           the server's port (its address is never written)
//! recv 12 vfo_limits:10000,30000000;   a text message from the server: ms since connect, the text
//! sent 250 rx_sensors_enable:true,200; a command the recorder sent
//! frame 300 1 0 48000 3 0 0 2048 2 8192
//!                                      a frame's header: ms, type, receiver, sample_rate, format,
//!                                      codec, crc, length, channels, then the payload's size in bytes
//! payload 300 0000803f...              that frame's payload in hex, only when the tester asked
//! end 60000 the time was up            why the recording ended
//! ```
//!
//! Text is written as received, with `\` written `\\` and control characters as `\n`, `\r`, `\t`
//! or `\xHH`, so one message is one line.
//!
//! **Replay** ([`Recording::dialect`]). A recording cannot show how a server behaves when a client
//! keys (the recorder never keys), only what it announces and how. So a replay takes the bundled
//! profile its protocol line names (ExpertSDR3, Thetis, AetherSDR by its device name, otherwise the
//! 1.x profile) for behaviour, and the recorded burst for everything the burst says: the identity
//! lines in their order, whether the burst came as one message, where `start;` falls, the lines
//! that are not commands, the stream settings, the radio's state, and every other state line,
//! replayed as written. The simulator then sends a burst made from that: the same identity, in the
//! same order, with the radio's state lines in the simulator's own order.

use std::fmt;

use crate::dialect::{AudioFormat, Dialect, DriveForm, Ident, Initial, Spelling, TrxInit};
use crate::frame::{Header, SampleType};
use crate::wire;

/// The format line every session starts with.
pub const FORMAT_LINE: &str = "tcisession 1";

/// One line of a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A text message from the server.
    Recv { ms: u64, text: String },
    /// A command the recorder sent.
    Sent { ms: u64, text: String },
    /// A frame from the server: its header and its payload's size.
    Frame {
        ms: u64,
        header: Header,
        payload: usize,
    },
    /// The payload of the frame before it.
    Payload { ms: u64, bytes: Vec<u8> },
    /// Why the recording ended.
    End { ms: u64, why: String },
}

/// A recorded session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub port: Option<u16>,
    pub entries: Vec<Entry>,
}

/// A line the parser could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub reason: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

impl std::error::Error for ParseError {}

/// `text` as one line: `\` doubled, control characters escaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() && (c as u32) < 0x100 => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// The text [`escape`] wrote.
pub fn unescape(line: &str) -> Result<String, String> {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                let v =
                    u32::from_str_radix(&hex, 16).map_err(|_| format!("bad escape \\x{hex}"))?;
                out.push(char::from_u32(v).ok_or("bad escape")?);
            }
            other => {
                return Err(format!(
                    "bad escape \\{}",
                    other.map_or(String::new(), String::from)
                ))
            }
        }
    }
    Ok(out)
}

/// Bytes as lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Entry {
    /// The entry as one line, without its newline.
    pub fn line(&self) -> String {
        match self {
            Entry::Recv { ms, text } => format!("recv {ms} {}", escape(text)),
            Entry::Sent { ms, text } => format!("sent {ms} {}", escape(text)),
            Entry::Frame {
                ms,
                header: h,
                payload,
            } => format!(
                "frame {ms} {} {} {} {} {} {} {} {} {payload}",
                h.kind, h.receiver, h.sample_rate, h.format, h.codec, h.crc, h.length, h.channels
            ),
            Entry::Payload { ms, bytes } => format!("payload {ms} {}", hex(bytes)),
            Entry::End { ms, why } => format!("end {ms} {}", escape(why)),
        }
    }
}

impl Recording {
    /// Read a recording.
    pub fn parse(text: &str) -> Result<Recording, ParseError> {
        let mut rec = Recording::default();
        let mut lines = text.lines().enumerate();
        match lines.next() {
            Some((_, l)) if l.trim_end() == FORMAT_LINE => {}
            _ => {
                return Err(ParseError {
                    line: 1,
                    reason: format!("not a session: the first line must be `{FORMAT_LINE}`"),
                })
            }
        }
        for (i, line) in lines {
            let err = |reason: &str| ParseError {
                line: i + 1,
                reason: reason.to_string(),
            };
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            if word == "port" {
                rec.port = Some(rest.trim().parse().map_err(|_| err("a bad port"))?);
                continue;
            }
            let (ms, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            let ms: u64 = ms.parse().map_err(|_| err("a bad time"))?;
            let entry = match word {
                "recv" => Entry::Recv {
                    ms,
                    text: unescape(rest).map_err(|e| err(&e))?,
                },
                "sent" => Entry::Sent {
                    ms,
                    text: unescape(rest).map_err(|e| err(&e))?,
                },
                "end" => Entry::End {
                    ms,
                    why: unescape(rest).map_err(|e| err(&e))?,
                },
                "payload" => Entry::Payload {
                    ms,
                    bytes: unhex(rest.trim()).ok_or_else(|| err("bad hex"))?,
                },
                "frame" => {
                    let v: Vec<u64> = rest
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .map_err(|_| err("a bad frame line"))?;
                    let [kind, receiver, rate, format, codec, crc, length, channels, payload] =
                        v[..]
                    else {
                        return Err(err("a frame line has nine numbers"));
                    };
                    let w =
                        |x: u64| u32::try_from(x).map_err(|_| err("a header field over 32 bits"));
                    Entry::Frame {
                        ms,
                        header: Header {
                            receiver: w(receiver)?,
                            sample_rate: w(rate)?,
                            format: w(format)?,
                            codec: w(codec)?,
                            crc: w(crc)?,
                            length: w(length)?,
                            kind: w(kind)?,
                            channels: w(channels)?,
                            reserved: [0; 8],
                        },
                        payload: usize::try_from(payload).map_err(|_| err("a bad size"))?,
                    }
                }
                other => return Err(err(&format!("unknown line `{other}`"))),
            };
            rec.entries.push(entry);
        }
        Ok(rec)
    }

    /// The init burst: the text messages received up to and including the first that holds
    /// `ready;`, then any message holding only `start;` or `stop;` right after it.
    pub fn burst(&self) -> Vec<String> {
        let received: Vec<&str> = self
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Recv { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let names = |t: &str| -> Vec<String> {
            wire::parse(t)
                .commands
                .into_iter()
                .map(|c| c.name)
                .collect()
        };
        let Some(ready) = received
            .iter()
            .position(|t| names(t).iter().any(|n| n == "ready"))
        else {
            return received.iter().map(|t| t.to_string()).collect();
        };
        let mut burst: Vec<String> = received[..=ready].iter().map(|t| t.to_string()).collect();
        if let Some(next) = received.get(ready + 1) {
            if matches!(names(next).as_slice(), [n] if n == "start" || n == "stop") {
                burst.push(next.to_string());
            }
        }
        burst
    }

    /// A dialect that announces what this recording's server announced (see the module header).
    pub fn dialect(&self) -> Dialect {
        let burst = self.burst();
        let commands: Vec<wire::Command> =
            burst.iter().flat_map(|t| wire::parse(t).commands).collect();
        let find = |name: &str| commands.iter().find(|c| c.name == name);
        let program = find("protocol")
            .map(|c| c.arg(0).to_string())
            .unwrap_or_default();
        let device = find("device")
            .map(|c| c.arg(0).to_string())
            .unwrap_or_default();
        let mut d = if device == "AetherSDR" {
            Dialect::aether_sdr()
        } else if program.eq_ignore_ascii_case("thetis") {
            Dialect::thetis()
        } else if program.eq_ignore_ascii_case("expertsdr3") {
            Dialect::expert_sdr3()
        } else {
            Dialect::expert_sdr2()
        };
        let version = find("protocol")
            .map(|c| c.arg(1).to_string())
            .unwrap_or_default();
        d.name = format!("Recorded: {program} {version} on {device}");
        d.program = program;
        d.version = version;
        d.device = device;

        // The identity, in the order it came.
        let mut order = Vec::new();
        let mut spelling = None;
        for c in &commands {
            let ident = match c.name.as_str() {
                "vfo_limits" => Ident::VfoLimits,
                "if_limits" => Ident::IfLimits,
                "trx_count" => Ident::TrxCount,
                "channel_count" | "channels_count" => {
                    let plural = c.name == "channels_count";
                    spelling = Some(match (spelling, plural) {
                        (None, true) => Spelling::Plural,
                        (None, false) => Spelling::Singular,
                        (Some(s), p) if (s == Spelling::Plural) == p => s,
                        _ => Spelling::Both,
                    });
                    Ident::ChannelCount
                }
                "device" => Ident::Device,
                "receive_only" => Ident::ReceiveOnly,
                "modulations_list" => Ident::Modulations,
                "protocol" => Ident::Protocol,
                _ => continue,
            };
            if !order.contains(&ident) {
                order.push(ident);
            }
        }
        d.identity_order = order;
        if let Some(s) = spelling {
            d.channel_count = s;
        }
        let number = |name: &str, i: usize| find(name).and_then(|c| c.arg(i).parse::<i64>().ok());
        if let (Some(lo), Some(hi)) = (number("vfo_limits", 0), number("vfo_limits", 1)) {
            d.vfo_limits = (lo.max(0) as u64, hi.max(0) as u64);
        }
        if let (Some(lo), Some(hi)) = (number("if_limits", 0), number("if_limits", 1)) {
            d.if_limits = (lo, hi);
        }
        if let Some(n) = number("trx_count", 0) {
            d.trx_count = n.clamp(1, 8) as u32;
        }
        if let Some(n) = number("channels_count", 0).or_else(|| number("channel_count", 0)) {
            d.channels = n.clamp(1, 2) as u32;
        }
        if let Some(c) = find("modulations_list") {
            d.modulations = c.args.clone();
        }
        d.receive_only =
            find("receive_only").is_some_and(|c| wire::boolean(c.arg(0)) == Some(true));

        // How it was sent.
        d.burst_in_one_message =
            burst.len() == 1 || (burst.len() == 2 && wire::parse(&burst[1]).commands.len() == 1);
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        let ready = names.iter().position(|n| *n == "ready");
        d.run_state_after_ready = ready.is_some_and(|r| {
            names[r + 1..]
                .iter()
                .any(|n| matches!(*n, "start" | "stop"))
        });
        d.quirks = burst
            .iter()
            .filter(|t| {
                let p = wire::parse(t);
                p.commands.is_empty() && !p.flaws.is_empty()
            })
            .cloned()
            .collect();

        // The stream settings it announced.
        d.burst_stream_settings = find("audio_samplerate").is_some();
        let mut audio = AudioFormat::SPEC;
        if let Some(r) = number("audio_samplerate", 0) {
            audio.rate = r as u32;
        }
        if let Some(t) =
            find("audio_stream_sample_type").and_then(|c| SampleType::from_name(c.arg(0)))
        {
            audio.sample_type = t;
        }
        if let Some(n) = number("audio_stream_channels", 0) {
            audio.channels = n.clamp(1, 2) as u32;
        }
        if let Some(n) = number("audio_stream_samples", 0) {
            audio.samples = Some(n.clamp(100, 2048) as u32);
        }
        d.audio = audio;
        if let Some(r) = number("iq_samplerate", 0) {
            d.iq_rate = r as u32;
        }

        // The radio's state.
        if commands
            .iter()
            .any(|c| c.name == "drive" && c.args.len() == 1)
        {
            d.drive = DriveForm::Bare;
        } else if commands
            .iter()
            .any(|c| c.name == "drive" && c.args.len() == 2)
        {
            d.drive = DriveForm::PerTrx;
        }
        let count = d.trx_count as usize;
        let mut trx: Vec<TrxInit> = (0..count).map(|t| d.initial_trx(t)).collect();
        let mut initial = Initial {
            trx: Vec::new(),
            running: d.initial.running,
            cw_speed: d.initial.cw_speed,
            cw_delay: d.initial.cw_delay,
            extra: Vec::new(),
        };
        for c in &commands {
            let t = c.arg(0).parse::<usize>().ok().filter(|t| *t < count);
            let ch = c.arg(1).parse::<usize>().ok().filter(|c| *c < 2);
            let int = |i: usize| c.arg(i).parse::<i64>().ok();
            let flag = |i: usize| wire::boolean(c.arg(i));
            let mut used = true;
            match (c.name.as_str(), t) {
                ("vfo", Some(t)) => match (ch, int(2)) {
                    (Some(ch), Some(hz)) => trx[t].vfo[ch] = hz.max(0) as u64,
                    _ => used = false,
                },
                ("dds", Some(t)) => match int(1) {
                    Some(hz) => trx[t].dds = hz.max(0) as u64,
                    None => used = false,
                },
                ("if", Some(t)) => match (ch, int(2)) {
                    (Some(ch), Some(hz)) => trx[t].if_hz[ch] = hz,
                    _ => used = false,
                },
                ("modulation", Some(t)) if c.args.len() == 2 => {
                    trx[t].modulation = c.arg(1).to_string()
                }
                ("rx_filter_band", Some(t)) => match (int(1), int(2)) {
                    (Some(lo), Some(hi)) => trx[t].filter = (lo, hi),
                    _ => used = false,
                },
                ("split_enable", Some(t)) => trx[t].split = flag(1).unwrap_or(false),
                ("rit_enable", Some(t)) => trx[t].rit = flag(1).unwrap_or(false),
                ("xit_enable", Some(t)) => trx[t].xit = flag(1).unwrap_or(false),
                ("rit_offset", Some(t)) => trx[t].rit_offset = int(1).unwrap_or(0),
                ("xit_offset", Some(t)) => trx[t].xit_offset = int(1).unwrap_or(0),
                ("drive", _) if c.args.len() == 1 => {
                    for s in &mut trx {
                        s.drive = int(0).unwrap_or(0).clamp(0, 100) as u32;
                    }
                }
                ("drive", Some(t)) => trx[t].drive = int(1).unwrap_or(0).clamp(0, 100) as u32,
                ("tune_drive", Some(t)) => {
                    trx[t].tune_drive = int(1).unwrap_or(0).clamp(0, 100) as u32
                }
                ("tx_enable", Some(t)) => trx[t].tx_enable = flag(1).unwrap_or(true),
                ("cw_macros_speed", _) => {
                    initial.cw_speed = int(0).unwrap_or(25).clamp(1, 100) as u32
                }
                ("cw_macros_delay", _) => {
                    initial.cw_delay = int(0).unwrap_or(0).clamp(0, 5_000) as u32
                }
                ("start", _) => initial.running = true,
                ("stop", _) => initial.running = false,
                (
                    "vfo_limits"
                    | "if_limits"
                    | "trx_count"
                    | "channel_count"
                    | "channels_count"
                    | "device"
                    | "receive_only"
                    | "modulations_list"
                    | "protocol"
                    | "ready"
                    | "trx"
                    | "tune"
                    | "audio_samplerate"
                    | "audio_stream_sample_type"
                    | "audio_stream_channels"
                    | "audio_stream_samples"
                    | "tx_stream_audio_buffering"
                    | "iq_samplerate",
                    _,
                ) => {}
                _ => used = false,
            }
            if !used {
                // Everything else is replayed as it was written.
                initial.extra.push(format!("{};", c.text));
            }
        }
        // A mode the server lists in another spelling is still its mode.
        for s in &mut trx {
            if let Some(m) = d.modulation(&s.modulation) {
                s.modulation = m.to_string();
            }
        }
        initial.trx = trx;
        d.initial = initial;
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_round_trips_through_its_line() {
        let entries = vec![
            Entry::Recv {
                ms: 0,
                text: "vfo_limits:10000,30000000;".into(),
            },
            Entry::Recv {
                ms: 3,
                text: "odd\\text\nwith\ttabs\u{1}and ünïcödé".into(),
            },
            Entry::Recv {
                ms: 4,
                text: String::new(),
            },
            Entry::Sent {
                ms: 250,
                text: "rx_sensors_enable:true,200;".into(),
            },
            Entry::Frame {
                ms: 300,
                header: Header {
                    receiver: 1,
                    sample_rate: 48_000,
                    format: 3,
                    length: 2048,
                    kind: 1,
                    channels: 2,
                    ..Header::default()
                },
                payload: 8192,
            },
            Entry::Payload {
                ms: 300,
                bytes: vec![0, 0, 0x80, 0x3f],
            },
            Entry::End {
                ms: 60_000,
                why: "the time was up".into(),
            },
        ];
        let mut text = format!("{FORMAT_LINE}\n# a comment\nport 50001\n");
        for e in &entries {
            text.push_str(&e.line());
            text.push('\n');
        }
        let rec = Recording::parse(&text).unwrap();
        assert_eq!(rec.port, Some(50001));
        assert_eq!(rec.entries, entries);
        assert!(Recording::parse("flexsession 1\n").is_err());
        assert_eq!(
            Recording::parse(&format!("{FORMAT_LINE}\nfrobnicate 1 x\n"))
                .unwrap_err()
                .line,
            2
        );
    }
}
