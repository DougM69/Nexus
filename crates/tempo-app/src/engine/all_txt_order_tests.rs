//! ALL.TXT reads in the order things happened on the air.
//!
//! WSJT-X writes its `Tx` line when PTT goes up (`widgets/mainwindow.cpp` 8325-8331), after the
//! previous period's last decodes, so its file is a two-way conversation read top to bottom.
//! Nexus writes the `Tx` line when the over is planned, just before the boundary, and the previous
//! period's boundary-pass decodes are folded after it: the reply we answered sat BELOW our answer,
//! stamped 15 s earlier. The line is still made at plan time (nothing about when or what is
//! transmitted moves); it is held until the over ends and then goes into the file in time order.

use super::*;

const TX2: &str = "W1AW KD9TAW -10";

/// A station on 20 m FT8 at 1500 Hz with ALL.TXT on, working W1AW off its grid heard at slot 100.
fn working_w1aw() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.settings.write_all_txt = true;
    e.hear_for_test("KD9TAW W1AW FN31", -10, 100);
    e.call_station_ctx(
        "W1AW",
        Some("FN31"),
        Some("KD9TAW W1AW FN31"),
        Some(-10),
        None,
    )
    .expect("premise: W1AW worked");
    let _ = e.take_immediate_tx();
    e
}

/// Plan, build and commit `slot`'s over as the radio loop does, and say what went out.
fn key(e: &mut Engine, slot: u64) -> Option<String> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Native { text, .. } = &plan.waveform else {
        panic!("an FT8 plan is a native waveform");
    };
    let text = text.clone();
    let wave = plan.waveform.build();
    (!e.commit_tx(&plan, wave, slot).is_empty()).then_some(text)
}

fn position(file: &[String], needle: &str) -> usize {
    file.iter()
        .position(|l| l.ends_with(needle))
        .unwrap_or_else(|| panic!("{needle:?} in {file:#?}"))
}

/// ⭐ The previous period's decode folded after our over was planned still reads ABOVE our Tx
/// line, as it does in WSJT-X's file. The line is unchanged: stamped with the period it keys in.
#[test]
fn a_decode_folded_after_the_plan_reads_above_our_tx_line() {
    let mut e = working_w1aw();
    let _ = e.take_all_txt_pending();
    assert_eq!(
        key(&mut e, 102).as_deref(),
        Some(TX2),
        "premise: our report"
    );
    // The boundary pass of the period before ours, folded after the plan.
    e.fold_heard_for_test("KD9TAW W1AW -08", 102);
    let file = e.take_all_txt_pending();
    assert!(
        position(&file, "KD9TAW W1AW -08") < position(&file, TX2),
        "{file:#?}"
    );
    let tx = &file[position(&file, TX2)];
    assert_eq!(
        *tx,
        crate::alltxt::all_txt_line(102 * 15, 14.074, true, "FT8", 0, 0.0, 1500.0, TX2)
    );
}

/// The `Tx` line waits for the end of its over, and no longer: mid-over nothing of it is filed,
/// and at the over's end it is.
#[test]
fn our_tx_line_is_filed_when_its_over_ends() {
    let mut e = working_w1aw();
    let _ = e.take_all_txt_pending();
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2));
    let over_end = 103 * 15;
    assert!(
        e.take_all_txt_pending_at(over_end - 1).is_empty(),
        "held while the over is on the air"
    );
    let file = e.take_all_txt_pending_at(over_end);
    assert_eq!(file.len(), 1, "{file:#?}");
    assert!(file[0].ends_with(TX2), "{file:#?}");
}

/// A drain that comes late (the window was not polling) still files in time order: the decode of
/// the period after our over goes BELOW our Tx line, though it was buffered before the line was
/// released.
#[test]
fn a_late_drain_still_files_in_time_order() {
    let mut e = working_w1aw();
    let _ = e.take_all_txt_pending();
    assert_eq!(key(&mut e, 102).as_deref(), Some(TX2));
    e.fold_heard_for_test("KD9TAW W1AW -08", 102);
    e.fold_heard_for_test("KD9TAW W1AW R-12", 104);
    let file = e.take_all_txt_pending_at(200 * 15);
    let (before, ours, after) = (
        position(&file, "KD9TAW W1AW -08"),
        position(&file, TX2),
        position(&file, "KD9TAW W1AW R-12"),
    );
    assert!(before < ours && ours < after, "{file:#?}");
}

/// Q65's mode column names its period and submode, as WSJT-X's `write_all` writes it
/// (`"Q65-" + period + ('A' + submode)`): `Q65-30B`, not the bare `Q65`.
#[test]
fn a_q65_line_names_its_period_and_submode() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.q65_period_s = 30;
    e.settings.q65_submode = 1;
    e.set_tier(Tier::Q65);
    e.set_frequency(50.275, "6m", "USB");
    e.settings.write_all_txt = true;
    let _ = e.take_all_txt_pending();
    e.fold_heard_for_test("CQ W1AW FN31", 10);
    let file = e.take_all_txt_pending();
    assert!(
        file.iter()
            .any(|l| l.contains(" Rx Q65-30B -10 ") && l.ends_with("CQ W1AW FN31")),
        "{file:#?}"
    );
}
