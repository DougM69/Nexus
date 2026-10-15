//! The ALC meter of a Yaesu FT-991/991A (Hamlib model 1035) and FT-710 (1049), on the radio's own
//! scale.
//!
//! Both radios give their ALC meter as 0 to 255. Yaesu's FT-991A CAT manual, `RM` READ METER:
//! `RM4;` is answered `RM4` and three digits. Its FT-710 CAT manual: the same three digits, then a
//! fixed `000`. Hamlib 4.7.1's `l ALC` sends that same `RM4;`, but neither radio's backend has an
//! ALC table, so it scales with newcat's default, raw 64 = 1.0 and clamped there. The meter showed
//! four times the radio's reading, and full from a quarter of the radio's bar up.
//!
//! The rigctld here answers both ways of asking as Hamlib and the radio do (see [`alc_rigctld`]).
//! The loop is the real `RadioLoop::step`, with the radio keyed at the rig, the mic-PTT scene of
//! `a_rig_that_answers_its_transmit_meters_keeps_being_polled`. What is asserted is what the meter
//! is given: the engine's `tx_alc`, by value.
use super::*;

/// What Hamlib 4.7.1 answers `l ALC` with on a Yaesu whose backend has no ALC table: newcat's
/// `yaesu_default_alc_cal`, raw 0 = 0.0 and raw 64 = 1.0, clamped past its last point.
fn hamlib_alc(raw: u16) -> f32 {
    (f32::from(raw) / 64.0).min(1.0)
}

/// A rigctld for a radio keyed at the rig (`t` answers 1) whose ALC meter reads `raw`, 0 to 255.
///
/// - `w RM4;` gets the radio's own reply: `RM4`, `raw` in three digits, `p3` (the FT-710's fixed
///   `000`, empty for an FT-991A) and `;`, then a NUL and no newline. That is the shape rigctld's
///   send_cmd hands back (`a_nul_terminated_raw_cat_reply_is_read_instead_of_timing_out`).
/// - `l ALC` gets [`hamlib_alc`] of the same reading, six decimals as rigctld prints a level.
/// - Every other line gets `RPRT 0`, no reading, so the other meters drop out after their three
///   strikes and the rest of the over is ALC reads.
fn alc_rigctld(raw: u16, p3: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let reply = match l.as_str() {
                    "f" => "14074000\n".to_string(),
                    "t" => "1\n".to_string(), // the operator is holding the mic key
                    "w RM4;" => format!("RM4{raw:03}{p3};\0"),
                    "l ALC" => format!("{:.6}\n", hamlib_alc(raw)),
                    _ => "RPRT 0\n".to_string(),
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// Four seconds of an over on a radio of Hamlib `model` whose ALC reads `raw`: what the meter is
/// given, and every line the radio's rigctld was sent.
fn alc_after_an_over(model: u32, raw: u16, p3: &'static str) -> (Option<f32>, Vec<String>) {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        e.apply_settings(s);
    }
    let (addr, log) = alc_rigctld(raw, p3);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let mut t = 0.0;
    for _ in 0..80 {
        t += 50.0;
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
    assert!(
        state.rig_keyed,
        "model {model}: control: the radio must be keyed, or no meter is read at all"
    );
    let alc = engine.lock().unwrap().snapshot().radio.tx_alc;
    let sent = log.lock().unwrap().clone();
    (alc, sent)
}

/// ⭐ THE FT-991A AND THE FT-710 SHOW THE RADIO'S OWN BAR. Raw 64 is a quarter of the radio's bar
/// and 128 half of it; Nexus showed both as 100 %, red. Asserted by value first, every reading
/// against the bar position, raw / 255, for each radio's own reply shape; then how it was asked.
#[test]
fn an_ft991a_and_an_ft710_show_alc_where_the_radios_own_bar_is() {
    for (model, name, p3) in [(1035u32, "FT-991A", ""), (1049, "FT-710", "000")] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, p3);
                (raw, alc, sent)
            })
            .collect();
        let table: Vec<String> = overs
            .iter()
            .map(|(raw, alc, _)| {
                let shown = alc.map_or("nothing".to_string(), |a| format!("{:.1} %", a * 100.0));
                format!(
                    "raw {raw}: bar {:.1} %, meter {shown}",
                    f32::from(*raw) / 2.55
                )
            })
            .collect();
        for (raw, alc, _) in &overs {
            let bar = f32::from(*raw) / 255.0;
            assert!(
                alc.is_some_and(|a| (a - bar).abs() < 1e-6),
                "{name}, raw {raw}: the meter must show where the radio's bar is — {table:?}"
            );
        }
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "w RM4;"),
                "{name}: the radio's own ALC value is read — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "l ALC"),
                "{name}: Hamlib's scaled ALC is not asked for — {sent:?}"
            );
            for meter in ["SWR", "RFPOWER_METER_WATTS", "COMP_METER"] {
                assert!(
                    sent.iter().any(|l| *l == format!("l {meter}")),
                    "{name}: {meter} is still read through Hamlib — {sent:?}"
                );
            }
        }
    }
}

/// …and every other radio reads its ALC as it did: another Yaesu on the same Hamlib table (the
/// FTDX10) and an Icom (the IC-7300 through Hamlib). Whatever `l ALC` answers is what the meter
/// is given, and the radio is sent no `RM4;`, which on an Icom's CI-V bus is junk. By value
/// first, every reading, as above: raw 0 reads 0 on either scale, so it proves nothing alone.
#[test]
fn every_other_radios_alc_still_reads_through_hamlib() {
    for (model, name) in [(1042u32, "FTDX10"), (3073, "IC-7300")] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, "000");
                (raw, alc, sent)
            })
            .collect();
        let got: Vec<(u16, Option<f32>)> = overs.iter().map(|(raw, alc, _)| (*raw, *alc)).collect();
        let want: Vec<(u16, Option<f32>)> = overs
            .iter()
            .map(|(raw, _, _)| (*raw, Some(hamlib_alc(*raw))))
            .collect();
        assert_eq!(
            got, want,
            "{name}: unchanged, the meter shows Hamlib's own answer (raw, meter)"
        );
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "l ALC"),
                "{name}: ALC is still read through Hamlib — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "w RM4;"),
                "{name}: no raw RM4 is sent to it — {sent:?}"
            );
        }
    }
}

/// A rigctld whose radio answers every line with `RM4064;` (and the NUL), but only after `late`.
fn late_rm4_rigctld(late: std::time::Duration) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                std::thread::sleep(late);
                if stream.write_all(b"RM4064;\0").is_err() {
                    break;
                }
            }
        }
    });
    addr
}

/// A tune-up's ALC read is bounded on these radios too. A meter read blocks the loop while the
/// tune carrier plays out of a short lead, so the poll passes the tune's deadline
/// (`TUNE_METER_DEADLINE_MS`) and an answer later than that is given up on. The same late answer
/// IS read with the normal deadline, the control that the fake radio answers at all. A slow
/// transport, so the normal deadline (2.5 s) is far from the late answer (1 s).
#[test]
fn a_tune_ups_alc_read_gives_up_at_the_tune_deadline() {
    let mut rig = Rig::rigctld(&late_rm4_rigctld(std::time::Duration::from_millis(1_000)));
    rig.set_slow_transport(true);
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, None),
        Some(64.0 / 255.0),
        "control: with the normal deadline the late answer is read"
    );
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, Some(TUNE_METER_DEADLINE_MS)),
        None,
        "a tune-up does not wait a second for its ALC"
    );
}
