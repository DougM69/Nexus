use super::*;
use crate::amplifier::AmpIntent;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tempo_app::{engine::Engine, settings::Settings};

// These tests exercise the process-local native queue through its real consumer.
// Every test cleans it, even after a failed assertion, without racing another case.
static QUEUE_TEST: Mutex<()> = Mutex::new(());
struct QueueGuard(std::sync::MutexGuard<'static, ()>);
impl QueueGuard {
    fn new() -> Self {
        let guard = Self(QUEUE_TEST.lock().unwrap_or_else(|e| e.into_inner()));
        drop_pending();
        guard
    }
}
impl Drop for QueueGuard {
    fn drop(&mut self) {
        let _ = &self.0;
        drop_pending();
    }
}

fn station(
    family: &str,
    follow: bool,
) -> (
    Arc<Mutex<Engine>>,
    AmpStatusDto,
    tempo_app::remote_monitor::provenance::Read,
) {
    let mut settings = Settings {
        amp_model: family.into(),
        amp_port: "native-amp-test".into(),
        amp_follow_band: follow,
        band: "20m".into(),
        dial_mhz: 14.074,
        ..Default::default()
    };
    settings.ensure_radio_profiles();
    let mut e = Engine::with_settings(settings);
    e.set_tx_enabled(false);
    let radio = e.remote_open_radio().unwrap();
    let read = e.remote_radio_read(&radio, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_ptt(Some(&read), Some(false));
    let amp = e.remote_open_amp().unwrap();
    let read = e.remote_amp_read(&amp, Instant::now()).unwrap();
    let dto = AmpStatusDto {
        family: family.into(),
        model: if family == FAMILY_SPE {
            "15K"
        } else {
            "KPA500"
        }
        .into(),
        linked: true,
        operate: Some(false),
        transmitting: (family == FAMILY_SPE).then_some(false),
        output_watts: Some(0),
        band_label: Some("80m".into()),
        ..Default::default()
    };
    e.remote_observe_amp(Some(&read), dto.clone());
    (Arc::new(Mutex::new(e)), dto, read)
}

fn dispatch(
    e: &Arc<Mutex<Engine>>,
    dto: &AmpStatusDto,
    read: &tempo_app::remote_monitor::provenance::Read,
) -> Vec<AmpIntent> {
    let mut writes = Vec::new();
    assert!(!dispatch_native(e, dto, Some(read), false, |intent| {
        assert!(e.try_lock().is_ok(), "serial writes must release Engine");
        writes.push(intent);
        Ok(())
    }));
    writes
}

fn fresh_amp(
    e: &Arc<Mutex<Engine>>,
    dto: &AmpStatusDto,
) -> tempo_app::remote_monitor::provenance::Read {
    let mut native = e.lock().unwrap();
    let amp = native.remote_open_amp().unwrap();
    let read = native.remote_amp_read(&amp, Instant::now()).unwrap();
    native.remote_observe_amp(Some(&read), dto.clone());
    read
}

#[test]
fn failed_native_amp_write_clears_both_displays_and_retires_the_completed_poll() {
    let _guard = QueueGuard::new();
    let (e, dto, read) = station(FAMILY_SPE, true);
    let id = e.lock().unwrap().settings().active_radio;
    finish_poll(&mut e.lock().unwrap(), id, Some(&read), dto.clone(), true);
    assert_eq!(
        e.lock().unwrap().amp_live(id).unwrap().output_watts,
        Some(0)
    );
    let mut attempted = 0;
    assert!(dispatch_native(&e, &dto, Some(&read), false, |_| {
        attempted += 1;
        Err(std::io::Error::other("test wire failure"))
    }));
    assert_eq!(attempted, 1);
    let mut native = e.lock().unwrap();
    finish_poll(&mut native, id, Some(&read), dto.clone(), false);
    assert_eq!(native.amp_live(id).unwrap().output_watts, None);
    assert!(!native.amp_live(id).unwrap().linked);
    assert!(native
        .remote_monitor_observation()
        .amplifier
        .unwrap()
        .reading
        .is_none());
    native.remote_observe_amp(Some(&read), dto.clone());
    finish_poll(&mut native, id, Some(&read), dto, true);
    assert_eq!(native.amp_live(id).unwrap().output_watts, None);
    assert!(native
        .remote_monitor_observation()
        .amplifier
        .unwrap()
        .reading
        .is_none());
}

#[test]
fn retired_amp_completion_cannot_replace_a_reconnected_amplifiers_current_display() {
    let (e, dto, old) = station(FAMILY_SPE, false);
    let current = AmpStatusDto {
        operate: Some(true),
        band_label: Some("40m".into()),
        ..dto.clone()
    };
    let fresh = fresh_amp(&e, &current);
    let mut native = e.lock().unwrap();
    let id = native.settings().active_radio;
    finish_poll(&mut native, id, Some(&fresh), current, true);
    for linked in [false, true] {
        finish_poll(&mut native, id, Some(&old), dto.clone(), linked);
        assert_eq!(native.amp_live(id).unwrap().operate, Some(true));
        assert_eq!(
            native
                .remote_monitor_observation()
                .amplifier
                .unwrap()
                .operate,
            Some(true)
        );
    }
}

#[test]
fn native_amp_refuses_physical_ptt_even_when_nexus_did_not_key_the_radio() {
    let _guard = QueueGuard::new();
    for family in [FAMILY_KPA, FAMILY_SPE] {
        let (e, dto, read) = station(family, true);
        assert_eq!(dispatch(&e, &dto, &read), [AmpIntent::BandUp]);
        {
            let mut native = e.lock().unwrap();
            native.observe_rig_ptt(true);
            let radio = native.remote_open_radio().unwrap();
            let read = native.remote_radio_read(&radio, Instant::now()).unwrap();
            native.remote_observe_cat(Some(&read), Some(true));
            native.remote_observe_ptt(Some(&read), Some(true));
            assert!(
                !native.snapshot().radio.transmitting,
                "this is physical PTT, not Nexus TX"
            );
        }
        assert!(
            dispatch(&e, &dto, &read).is_empty(),
            "{family} must not step under drive"
        );
    }
}

#[test]
fn native_amp_follow_off_during_poll_cancels_the_captured_follow_step() {
    let _guard = QueueGuard::new();
    let (e, dto, read) = station(FAMILY_SPE, true);
    assert_eq!(dispatch(&e, &dto, &read), [AmpIntent::BandUp]);
    {
        let mut native = e.lock().unwrap();
        let mut patch: tempo_app::settings::RadioProfilePatch = serde_json::from_value(
            serde_json::to_value(native.settings().active_profile().unwrap()).unwrap(),
        )
        .unwrap();
        patch.amp_follow_band = false;
        let radio = native.settings().active_radio;
        native.update_radio_profile(radio, patch).unwrap();
        assert!(!native.settings().active_profile().unwrap().amp_follow_band);
    }
    assert!(
        dispatch(&e, &dto, &read).is_empty(),
        "the serial poll captured follow before it was disabled"
    );
    let current = fresh_amp(&e, &dto);
    assert!(
        dispatch(&e, &dto, &current).is_empty(),
        "follow stays off after a fresh poll"
    );
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert_eq!(dispatch(&e, &dto, &current), [AmpIntent::ToggleOperate]);
}

#[test]
fn native_amp_port_change_during_poll_cannot_operate_the_old_link() {
    let _guard = QueueGuard::new();
    let (e, dto, read) = station(FAMILY_SPE, false);
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert_eq!(dispatch(&e, &dto, &read), [AmpIntent::ToggleOperate]);
    {
        let mut native = e.lock().unwrap();
        let mut patch: tempo_app::settings::RadioProfilePatch = serde_json::from_value(
            serde_json::to_value(native.settings().active_profile().unwrap()).unwrap(),
        )
        .unwrap();
        patch.amp_port = "different-native-amp".into();
        let radio = native.settings().active_radio;
        native.update_radio_profile(radio, patch).unwrap();
        assert_eq!(
            native.settings().active_profile().unwrap().amp_port,
            "different-native-amp"
        );
    }
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert!(
        dispatch(&e, &dto, &read).is_empty(),
        "a new-port gesture must not reach the old polled link"
    );
}

#[test]
fn native_amp_follow_respects_the_model_band_limit() {
    for (family, model) in [
        (FAMILY_KPA, "KPA500"),
        (FAMILY_KPA, "KPA1500"),
        (FAMILY_SPE, "15K"),
        (FAMILY_SPE, "unknown"),
    ] {
        let dto = AmpStatusDto {
            family: family.into(),
            model: model.into(),
            band_label: Some("6m".into()),
            ..Default::default()
        };
        assert_eq!(follow_step(&dto, "20m"), Some(AmpIntent::BandDown));
        assert_eq!(
            follow_step(&dto, "4m"),
            None,
            "{family}/{model} has no established 4 m step"
        );
    }
    let dto = AmpStatusDto {
        family: FAMILY_SPE.into(),
        model: "13K".into(),
        band_label: Some("6m".into()),
        ..Default::default()
    };
    assert_eq!(follow_step(&dto, "4m"), Some(AmpIntent::BandUp));
}

#[test]
fn native_amp_write_permit_cannot_outlive_its_physical_idle_reading() {
    let (e, _, amp_read) = station(FAMILY_KPA, false);
    let mut native = e.lock().unwrap();
    let radio = native.remote_open_radio().unwrap();
    let sampled = Instant::now() - Duration::from_millis(750);
    let read = native.remote_radio_read(&radio, sampled).unwrap();
    native.remote_observe_cat(Some(&read), Some(true));
    native.remote_observe_ptt(Some(&read), Some(false));
    let permit = native
        .native_amplifier_permit(&amp_read, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(permit.valid(Instant::now()));
    assert!(
        permit.deadline() <= sampled + Duration::from_secs(1),
        "a descheduled worker must not send after the idle evidence expires"
    );
}

#[test]
fn native_amp_write_permit_cannot_outlive_its_amplifier_poll() {
    let (e, dto, _) = station(FAMILY_SPE, false);
    let mut native = e.lock().unwrap();
    let amp = native.remote_open_amp().unwrap();
    let sampled = Instant::now() - Duration::from_millis(4500);
    let read = native.remote_amp_read(&amp, sampled).unwrap();
    native.remote_observe_amp(Some(&read), dto);
    let permit = native
        .native_amplifier_permit(&read, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(permit.valid(Instant::now()));
    assert!(permit.deadline() <= sampled + Duration::from_secs(5));
}

#[test]
fn native_amp_uses_spe_idle_flag_but_never_invents_a_missing_kpa_ptt_reading() {
    let _guard = QueueGuard::new();
    for family in [FAMILY_SPE, FAMILY_KPA] {
        let (e, dto, read) = station(family, true);
        assert_eq!(dispatch(&e, &dto, &read), [AmpIntent::BandUp]);
        e.lock().unwrap().remote_open_radio().unwrap(); // new link, no CAT/PTT reading yet
        let writes = dispatch(&e, &dto, &read);
        assert_eq!(
            writes,
            if family == FAMILY_SPE {
                vec![AmpIntent::BandUp]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn native_amp_keeps_manual_priority_and_drops_a_batch_when_physical_ptt_arrives() {
    let _guard = QueueGuard::new();
    let (e, dto, read) = station(FAMILY_SPE, true);
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert_eq!(
        dispatch(&e, &dto, &read),
        [AmpIntent::ToggleOperate],
        "follow must not add a competing step"
    );
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert!(queue_amp_command(AmpIntent::BandUp));
    let mut writes = Vec::new();
    assert!(!dispatch_native(&e, &dto, Some(&read), false, |intent| {
        writes.push(intent);
        e.lock().unwrap().observe_rig_ptt(true);
        Ok(())
    }));
    assert_eq!(writes, [AmpIntent::ToggleOperate]);
    assert!(dispatch(&e, &dto, &read).is_empty());
    assert!(
        e.lock().unwrap().settings().amp_follow_band,
        "cancellation must preserve the saved choice"
    );
}

#[test]
fn native_amp_retired_reads_stay_retired_after_the_same_port_is_restored() {
    let _guard = QueueGuard::new();
    let (e, dto, read) = station(FAMILY_SPE, true);
    assert_eq!(dispatch(&e, &dto, &read), [AmpIntent::BandUp]);
    {
        let mut native = e.lock().unwrap();
        let mut patch: tempo_app::settings::RadioProfilePatch = serde_json::from_value(
            serde_json::to_value(native.settings().active_profile().unwrap()).unwrap(),
        )
        .unwrap();
        let radio = native.settings().active_radio;
        patch.amp_port = "temporarily-different".into();
        native.update_radio_profile(radio, patch.clone()).unwrap();
        patch.amp_port = "native-amp-test".into();
        native.update_radio_profile(radio, patch).unwrap();
        native.remote_observe_amp(Some(&read), dto.clone());
    }
    assert!(dispatch(&e, &dto, &read).is_empty());
    let current = fresh_amp(&e, &dto);
    assert_eq!(dispatch(&e, &dto, &current), [AmpIntent::BandUp]);
}

#[test]
fn native_amp_ptt_revocation_uses_current_transitions_without_repeated_revision_churn() {
    let (e, _, amp_read) = station(FAMILY_KPA, false);
    let mut native = e.lock().unwrap();
    let old = native.remote_open_radio().unwrap();
    let retired = native.remote_radio_read(&old, Instant::now()).unwrap();
    let current = native.remote_open_radio().unwrap();
    let read = native.remote_radio_read(&current, Instant::now()).unwrap();
    native.remote_observe_cat(Some(&read), Some(true));
    native.remote_observe_ptt(Some(&read), Some(false));
    let permit = native
        .native_amplifier_permit(&amp_read, Instant::now() + Duration::from_secs(5))
        .unwrap();
    native.remote_observe_ptt(Some(&retired), Some(true));
    assert!(
        permit.valid(Instant::now()),
        "a retired read must not cancel current work"
    );
    let read = native.remote_radio_read(&current, Instant::now()).unwrap();
    native.remote_observe_ptt(Some(&read), Some(true));
    assert!(!permit.valid(Instant::now()));
    let generation = native.remote_actuation_context_generation();
    let read = native.remote_radio_read(&current, Instant::now()).unwrap();
    native.remote_observe_ptt(Some(&read), Some(true));
    assert_eq!(
        native.remote_actuation_context_generation(),
        generation,
        "steady PTT polling is not a new station edit"
    );
}

/// ⛔ A KPA1500 over the network hears nothing from Nexus while the station transmits, exactly as
/// over serial: the not-transmitting checks run before any write, whatever the wire. Against the
/// fake KPA1500 UDP server, through the real `KpaLink`: while the radio is keyed a band-follow
/// step, an Operate/Standby press and a band press each reach nothing, and PTT arriving mid-batch
/// stops the rest of the batch. CONTROL: once unkeyed, the same press and the follow step arrive.
#[cfg(feature = "serial")]
#[test]
fn a_kpa1500_over_udp_hears_nothing_from_nexus_while_the_station_transmits() {
    use crate::amplifier::{FakeKpa, KpaLink};
    use std::time::Duration;
    let _guard = QueueGuard::new();
    let fake = FakeKpa::start();
    let (mut link, _) = KpaLink::open(&fake.address()).expect("the fake answers");
    // The amplifier's own state, which `send_intent` steps from: standby, band 5 (20 m).
    let now = link.poll().unwrap();
    let (e, dto, read) = station(FAMILY_KPA, true);
    let key = |on: bool| {
        let mut native = e.lock().unwrap();
        native.observe_rig_ptt(on);
        let radio = native.remote_open_radio().unwrap();
        let read = native.remote_radio_read(&radio, Instant::now()).unwrap();
        native.remote_observe_cat(Some(&read), Some(true));
        native.remote_observe_ptt(Some(&read), Some(on));
    };
    // What the fake hears after `from`, once nothing more is in flight (UDP is not instant):
    // up to `want` commands, or whatever arrived within the wait.
    let heard_after = |from: usize, want: usize| -> Vec<String> {
        let deadline = Instant::now() + Duration::from_millis(if want == 0 { 300 } else { 2_000 });
        while fake.heard().len() < from + want.max(1) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(50));
        fake.heard()[from..].to_vec()
    };

    key(true);
    let base = fake.heard().len();
    let mut writes = Vec::new();
    // The follow step (the amplifier on 80 m, the radio on 20 m), then each press on its own.
    for press in [
        None,
        Some(AmpIntent::ToggleOperate),
        Some(AmpIntent::BandUp),
        Some(AmpIntent::BandDown),
    ] {
        if let Some(press) = press {
            assert!(queue_amp_command(press), "the press queued");
        }
        assert!(!dispatch_native(&e, &dto, Some(&read), false, |intent| {
            writes.push(intent);
            link.send_intent(intent, &now)
        }));
    }
    assert_eq!(
        heard_after(base, 0),
        Vec::<String>::new(),
        "the KPA1500 heard nothing while keyed"
    );
    assert_eq!(writes, [], "nothing handed to the wire while keyed");

    // PTT arrives after the first write of a batch: the rest never reaches the amplifier.
    key(false);
    let read = fresh_amp(&e, &dto);
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    assert!(queue_amp_command(AmpIntent::BandUp));
    let base = fake.heard().len();
    assert!(!dispatch_native(&e, &dto, Some(&read), false, |intent| {
        let sent = link.send_intent(intent, &now);
        key(true);
        sent
    }));
    assert_eq!(
        heard_after(base, 1),
        ["^OS1;"],
        "only the write before PTT arrived"
    );

    // CONTROL: unkeyed, the press and then the follow step arrive, by value.
    key(false);
    let read = fresh_amp(&e, &dto);
    assert!(queue_amp_command(AmpIntent::ToggleOperate));
    let base = fake.heard().len();
    assert!(!dispatch_native(&e, &dto, Some(&read), false, |intent| {
        link.send_intent(intent, &now)
    }));
    assert!(!dispatch_native(&e, &dto, Some(&read), false, |intent| {
        link.send_intent(intent, &now)
    }));
    assert_eq!(
        heard_after(base, 2),
        ["^OS1;", "^BN06;"],
        "CONTROL: unkeyed, Operate and the follow step reach the KPA1500"
    );
}
