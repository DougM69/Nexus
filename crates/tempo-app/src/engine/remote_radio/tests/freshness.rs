//! THE ONE-SECOND RULE, BY VALUE. A Remote radio action takes a reading less than a second old and
//! refuses one that is not. Every other case in this module holds the station's readings clock
//! still (`Station::new`); these move it on to 999 ms and to 1000 ms past the reading each check
//! reads, and keep every other reading fresh, so the check under test is the one that answers.
use super::*;

/// The station's readings clock, `ms` past the instant of the readings taken so far.
fn later(s: &mut Station, ms: u64) {
    let taken = s.engine.remote_readings.now();
    s.engine.remote_readings.still = Some(taken + Duration::from_millis(ms));
}

/// A PTT reading taken at the station's clock as it stands, so only the readings before it age.
fn fresh_ptt(s: &mut Station) {
    let now = s.engine.remote_readings.now();
    let read = s.engine.remote_radio_read(&s.connection, now).unwrap();
    s.engine.remote_observe_ptt(Some(&read), Some(false));
}

/// The queue's link check reads the PTT reading.
#[test]
fn a_remote_radio_action_takes_a_ptt_reading_999_ms_old_and_refuses_one_1000_ms_old() {
    let queued = [999, 1000].map(|ms| {
        let mut s = Station::new(OperatingMode::Phone);
        later(&mut s, ms);
        (ms, s.queue().map(|_| ()))
    });
    assert_eq!(
        queued,
        [(999, Ok(())), (1000, Err(Reason::ReadingUnavailable))]
    );
}

/// The commit reads the dial and mode readback, with the PTT reading fresh.
#[test]
fn a_commit_takes_a_readback_999_ms_old_and_refuses_one_1000_ms_old() {
    let committed = [999, 1000].map(|ms| {
        let mut s = Station::new(OperatingMode::Phone);
        let receipt = s.queue().unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        s.sample(request.target_hz, &request.target_mode);
        later(&mut s, ms);
        fresh_ptt(&mut s);
        (ms, request.commit(&mut s.engine), receipt.outcome())
    });
    assert_eq!(
        committed,
        [
            (
                999,
                true,
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            ),
            (
                1000,
                false,
                Outcome::Rejected {
                    reason: Reason::HardwareUnconfirmed
                }
            ),
        ]
    );
}

/// A filter width reads the mode reading, with the PTT reading fresh.
#[test]
fn a_filter_width_takes_a_mode_reading_999_ms_old_and_refuses_one_1000_ms_old() {
    let queued = [999, 1000].map(|ms| {
        let mut s = Station::new(OperatingMode::Cw);
        s.engine.observe_rig_passband(Some(500));
        later(&mut s, ms);
        fresh_ptt(&mut s);
        let connection = s
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        let permit = s.authority.permit(unexpired_deadline()).unwrap();
        let queued = s
            .engine
            .queue_remote_filter_width("cw", 500, 550, connection, permit);
        (ms, queued.map(|_| ()))
    });
    assert_eq!(
        queued,
        [(999, Ok(())), (1000, Err(Reason::ReadingUnavailable))]
    );
}
