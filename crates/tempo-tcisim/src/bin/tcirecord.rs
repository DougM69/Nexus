//! `tcirecord`: record a session from a TCI server (an SDR program: ExpertSDR3, Thetis,
//! piHPSDR …) to attach to a Nexus tester report.
//!
//! ```text
//! cargo run -p tempo-tcisim --bin tcirecord -- <server>[:<port>] [--seconds N] [--audio TRX]
//!     [--iq TRX] [--payload N] [--out FILE]
//! ```
//!
//! Observe-only. It connects as one more TCI client, records the server's init burst and every
//! message after it, turns on its own meter pushes and asks six reads that carry no argument. It
//! can send nothing else: the command list is printed when it starts, and
//! [`tempo_tcisim::record`] explains how that is enforced. It never transmits, never changes a
//! setting and never sends audio.
//!
//! IP and MAC addresses and serial numbers are replaced before anything is written. The file stays
//! on this computer and is never sent anywhere: read it, then attach it to your report if you
//! choose. It refuses to overwrite an existing file.
//!
//! Exit 0 = the session was recorded. Exit 1 = it was not, with the reason.

use std::fs::OpenOptions;
use std::io::BufWriter;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempo_tcisim::record::{self, Options, ALLOWED, DEFAULT_PORT};

const USAGE: &str = "usage: tcirecord <server>[:<port>] [--seconds N] [--audio TRX] [--iq TRX]
                 [--payload N] [--out FILE]

Records a TCI server's session (ExpertSDR3, Thetis, piHPSDR …) for a Nexus tester report.
Observe-only: it never transmits, never changes a setting and never sends audio. Addresses and
serial numbers are replaced before anything is written.
  <server>      the SDR program's address, with its TCI port (default 50001; piHPSDR uses 40001)
  --seconds N   how long to observe after connecting (default 60, at most 3600)
  --audio TRX   also record the headers of transceiver TRX's receive audio frames (0, 1 …).
                Some servers give a receiver's audio to one program only: close other programs'
                audio first, or leave this out
  --iq TRX      also record the headers of transceiver TRX's IQ frames (the same caution)
  --payload N   also write the samples of the first N frames of each kind (default 0, at most 50)
  --out FILE    where to write the session (default tci-session-<time>.tcisession);
                an existing file is never overwritten";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tcirecord: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The server's address: `ip`, `ip:port`, `[ipv6]:port`, `host` or `host:port`.
fn server(text: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Ok(addr);
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_PORT));
    }
    let with_port = if text.contains(':') {
        text.to_string()
    } else {
        format!("{text}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| format!("{text:?} is not an address this computer can reach\n\n{USAGE}"))
}

fn run(args: &[String]) -> Result<(), String> {
    let mut target: Option<SocketAddr> = None;
    let mut options = Options::default();
    let mut out: Option<String> = None;
    let number =
        |it: &mut std::slice::Iter<'_, String>, flag: &str, max: u64| -> Result<u64, String> {
            it.next()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n <= max)
                .ok_or(format!("{flag} needs a number from 0 to {max}"))
        };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--seconds" => {
                let n = number(&mut it, "--seconds", 3600)?.max(1);
                options.duration = Duration::from_secs(n);
            }
            "--audio" => options.audio = Some(number(&mut it, "--audio", 7)? as u8),
            "--iq" => options.iq = Some(number(&mut it, "--iq", 7)? as u8),
            "--payload" => options.payload_frames = number(&mut it, "--payload", 50)? as usize,
            "--out" => out = Some(it.next().ok_or("--out needs a file name")?.clone()),
            other if target.is_none() && !other.starts_with('-') => target = Some(server(other)?),
            other => return Err(format!("unexpected argument {other:?}\n\n{USAGE}")),
        }
    }
    let target = target.ok_or(USAGE)?;
    let path = out.unwrap_or_else(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("tci-session-{now}.tcisession")
    });
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("cannot create {path}: {e}"))?;

    println!(
        "Recording the TCI server on port {} for {} s into {path}.",
        target.port(),
        options.duration.as_secs()
    );
    println!("Observe-only: the only commands it can send are {ALLOWED:?}.");
    let summary = record::record(target, &options, BufWriter::new(file))
        .map_err(|e| format!("recording failed: {e} (anything recorded so far is in {path})"))?;

    println!(
        "Recorded {} messages and {} frame headers; sent {} observe-only commands.",
        summary.messages, summary.frames, summary.commands
    );
    if !summary.ready {
        println!("The server never said `ready;`: the start of the file is all it sent.");
    }
    if summary.chronos > 0 {
        println!(
            "The server asked this recorder for transmit audio {} times (another program was \
             transmitting); it sent nothing back.",
            summary.chronos
        );
    }
    if summary.server_closed {
        println!("The server closed the connection before the time was up.");
    }
    if summary.scrubbed.is_empty() {
        println!("Nothing needed replacing.");
    } else {
        let replaced: Vec<String> = summary
            .scrubbed
            .iter()
            .map(|(kind, n)| format!("{n} {}", kind.label()))
            .collect();
        println!("Replaced: {}.", replaced.join(", "));
    }
    println!(
        "Nothing was sent anywhere. Read {path}, then attach it to your report if you choose."
    );
    Ok(())
}
