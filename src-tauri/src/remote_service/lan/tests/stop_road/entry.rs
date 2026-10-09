//! A section change from afar arms nothing, on the roadside the road tests use: the shack's real
//! channel, command lane, Stop road and engine, over loopback, the app's handlers as
//! `held_on_the_engine` stands them in.
use super::*;

/// ⛔ From afar, only the wired starts arm transmit, each under the stream's rules. A computer holds
/// the lease with no presence on its road, so its Enable TX is refused (`noPresence`). Its section
/// change into Phone (`set_operating_mode`), a Phone spot worked (`work_spot`) and an SSTV channel
/// picked (`sstv_tune`, which takes the Phone section) each run at the shack, through the section
/// entry the shack's own screen uses, which arms the Enable-TX latch for the manual sections. From
/// afar they arm nothing, as the hosted road's entry arms nothing. CONTROL: the refusal shows the
/// rules were in force; the first two into Digital, which arms nothing anywhere, leave TX off; and
/// at the shack's own screen each of the three still arms.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_section_change_from_afar_arms_nothing() {
    let mut armed = Vec::new();
    for (cmd, args) in [
        (
            "set_operating_mode",
            json!({"mode": "phone", "followFreq": false}),
        ),
        (
            "work_spot",
            json!({"mode": "phone", "freqMhz": 14.25, "band": "20m"}),
        ),
        (
            "sstv_tune",
            json!({"dialMhz": 14.23, "band": "20m", "mode": "USB"}),
        ),
    ] {
        let app = App::new();
        let mut r = roadside(&app);
        let record = held_on_the_engine(&mut r, gate(false), "none");
        let mut seat = seated(&r).await;
        seat.keep_the_lease().await;
        let said = seat
            .call(1, "set_tx_enabled", json!({"enabled": true}))
            .await;
        assert_eq!(said["refused"], "noPresence", "CONTROL {cmd}: {said}");
        assert!(!r.s.shared.engine.lock().unwrap().tx_enabled(), "CONTROL");
        if cmd != "sstv_tune" {
            let mut digital = args.clone();
            digital["mode"] = json!("digital");
            let said = seat.call(2, cmd, digital).await;
            assert_eq!(said["value"]["ran"], cmd, "{said}");
            assert!(
                !r.s.shared.engine.lock().unwrap().tx_enabled(),
                "CONTROL {cmd}: digital armed"
            );
        }
        let said = seat.call(3, cmd, args).await;
        assert_eq!(said["value"]["ran"], cmd, "{said}");
        let mut e = r.s.shared.engine.lock().unwrap();
        if e.tx_enabled() {
            armed.push(format!("{cmd}, ran {:?}", ran(&record)));
        }
        // CONTROL: the shack's own screen's entry still arms, from Digital.
        e.set_operating_mode("digital", false);
        assert!(!e.tx_enabled(), "CONTROL {cmd}: back in Digital");
        match cmd {
            "set_operating_mode" => e.set_operating_mode("phone", false),
            "work_spot" => e.work_spot_tiered(None, "phone", 14.25, "20m", None),
            _ => e.sstv_tune(14.23, "20m", "USB"),
        }
        assert!(e.tx_enabled(), "CONTROL {cmd}: the shack's own entry arms");
    }
    assert!(
        armed.is_empty(),
        "Enable TX armed from afar with no presence on the road: {armed:#?}"
    );
}
