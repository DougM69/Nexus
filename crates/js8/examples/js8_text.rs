//! Nexus's JS8 TEXT layer for the parity lab — DEV-ONLY: an `example`, never shipped.
//!
//! The text twin of tempo-core's `encode_wav_js8` / `decode_wav_js8`, which carry the lab word
//! form (`"<12 sixbit> <i3>"`) and never text. The lab's JS8Call 3.0.3 tool prints the same two
//! line shapes for the same two questions, so a script compares the programs line for line:
//!
//!   frames — operator text → the frames Nexus sends (`compose::frames_with_grid`, the path the
//!            station transmits), each as its lab word and JS8Call's display line
//!            (`Frame::render`, what ALL.TXT prints):
//!              FRAME <12 sixbit> <i3> |<render>|
//!   render — one received word → its display line (`decode_word`, CRC-checked, then render):
//!              RENDER |<render>|
//!
//! Usage:
//!   cargo run -q -p js8 --example js8_text -- frames --speed ultra --mycall KD9TAW \
//!       --grid EN52 [--to W1AW] HELLO WORLD
//!   cargo run -q -p js8 --example js8_text -- render --speed ultra KD9TAWabcxyz 3
//! Speeds: slow|normal|fast|turbo|ultra or the ALL.TXT letters E|A|B|C|I.

use js8::proto::alphabet::sixbit_to_string;
use js8::proto::callsign::CallRef;
use js8::proto::compose::frames_with_grid;
use js8::proto::frame::{decode_word, encode_frame};
use js8::{Speed, Word87};

fn speed_arg(s: &str) -> Option<Speed> {
    match s.to_ascii_lowercase().as_str() {
        "slow" => Some(Speed::Slow),
        "normal" => Some(Speed::Normal),
        "fast" => Some(Speed::Fast),
        "turbo" => Some(Speed::Turbo),
        "ultra" => Some(Speed::Ultra),
        l if l.len() == 1 => Speed::from_letter(l.to_ascii_uppercase().chars().next()?),
        _ => None,
    }
}

fn fail(why: &str) -> ! {
    eprintln!("js8_text: {why}");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        fail("usage: js8_text frames|render ...")
    };
    let mut speed = None;
    let (mut mycall, mut grid, mut to) = (String::new(), String::new(), None::<String>);
    let mut rest = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--speed" => speed = it.next().and_then(|s| speed_arg(s)),
            "--mycall" => mycall = it.next().cloned().unwrap_or_default(),
            "--grid" => grid = it.next().cloned().unwrap_or_default(),
            "--to" => to = it.next().cloned(),
            _ => rest.push(a.clone()),
        }
    }
    let Some(speed) = speed else {
        fail("--speed slow|normal|fast|turbo|ultra")
    };
    match cmd.as_str() {
        "frames" => {
            let to = to.map(|c| CallRef::parse(&c).unwrap_or_else(|| fail("--to is not a call")));
            let text = rest.join(" ");
            let frames = frames_with_grid(&mycall, &grid, to.as_ref(), &text, speed)
                .unwrap_or_else(|e| fail(&format!("compose refused: {e:?}")));
            for (frame, i3) in frames {
                let w = encode_frame(&frame, i3, speed)
                    .unwrap_or_else(|e| fail(&format!("pack refused: {e:?}")));
                println!(
                    "FRAME {} {} |{}|",
                    sixbit_to_string(&w.payload72().chars12()),
                    w.i3().to_u8(),
                    frame.render()
                );
            }
        }
        "render" => {
            let w = Word87::from_lab(&rest.join(" "))
                .unwrap_or_else(|| fail("want \"<12 sixbit> <i3>\""));
            let (frame, _) = decode_word(&w, speed).unwrap_or_else(|e| fail(&format!("{e:?}")));
            println!("RENDER |{}|", frame.render());
        }
        other => fail(&format!("unknown command {other}")),
    }
}
