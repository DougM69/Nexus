//! WSJT-X's FT8 47 step as a decode job ([`DecodePass::Residual`]): it subtracts the period's
//! early decodes for the full pass (`lib/ft8_decode.f90:124-149`), decodes nothing and folds
//! nothing, and the full pass after it hands on no early decode again (`:121`, `:152-162`).

use super::*;

/// A station on 20 m FT8.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.write_all_txt = true;
    e
}

/// The period from its start: one strong CQ at 1000 Hz, DT 0, in light noise; the first `n`
/// samples, zero after, as the radio loop takes each pass's frame.
fn period(n: usize) -> Vec<f32> {
    let mode = modes::make_mode(modes::ModeKind::Ft8);
    let wave = mode.gen_wave(&mode.encode("CQ K2DEF FN42"), 12_000.0, 1000.0);
    let mut seed = 0x0047_u64;
    (0..180_000)
        .map(|i| {
            if i >= n {
                return 0.0;
            }
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02
                + 0.05 * wave.get(i).copied().unwrap_or(0.0)
        })
        .collect()
}

/// What a pass hands on as it runs.
#[derive(Default)]
struct Collect(Vec<String>);
impl DecodeProgress for Collect {
    fn found(&mut self, d: StreamedDecode) {
        self.0.push(d.decode().message.trim().to_string());
    }
}

/// The 47 step decodes nothing and folds nothing, over audio full of a decode; the full pass after
/// it does not hand the early decode on again. Without the step, it does.
#[test]
fn the_47_step_decodes_nothing_and_the_full_pass_after_it_hands_on_no_early_decode() {
    for with_step in [true, false] {
        let mut e = station();
        let early = run_decode_job(e.build_decode_job(period(41 * 3456), 9, DecodePass::Early));
        assert_eq!(
            early
                .decodes()
                .iter()
                .map(|d| d.message.trim())
                .collect::<Vec<_>>(),
            vec!["CQ K2DEF FN42"],
            "premise: the early pass decodes the CQ"
        );
        assert!(matches!(
            e.apply_decode_result(early),
            DecodeApplied::Early { n: 1 }
        ));
        let rows = e.take_all_txt_pending().len();
        if with_step {
            let step =
                run_decode_job(e.build_decode_job(period(47 * 3456), 9, DecodePass::Residual));
            assert!(step.decodes().is_empty(), "the 47 step decodes nothing");
            assert!(
                matches!(e.apply_decode_result(step), DecodeApplied::Stale),
                "and folds nothing"
            );
            assert!(e.take_all_txt_pending().is_empty(), "no ALL.TXT row");
        }
        let mut full = Collect::default();
        let result = run_decode_job_streaming(
            e.build_decode_job(period(50 * 3456), 9, DecodePass::Boundary),
            &mut full,
        );
        if with_step {
            assert!(
                full.0.is_empty(),
                "after the 47 step the full pass hands on no early decode: {:?}",
                full.0
            );
            assert_eq!(
                result
                    .decodes()
                    .iter()
                    .map(|d| d.message.trim())
                    .collect::<Vec<_>>(),
                vec!["CQ K2DEF FN42"],
                "the early decode opens the full pass's list"
            );
        } else {
            assert_eq!(
                full.0,
                vec!["CQ K2DEF FN42".to_string()],
                "control: without the step the full pass hands it on again"
            );
        }
        // Either way the boundary folds it once: the early pass did.
        assert!(matches!(
            e.apply_decode_result(result),
            DecodeApplied::Boundary { n: 0, .. }
        ));
        assert_eq!(rows, 1, "one ALL.TXT row, from the early pass");
        assert!(e.take_all_txt_pending().is_empty(), "and no second one");
    }
}
