//! A press's end reaches its computer though a datagram is lost, on the roadside the road tests use:
//! the shack's real channel, command lane, Stop road and engine, over loopback, the app's handlers
//! as `held_on_the_engine` stands them in.
use super::*;

/// ⛔ A press's end reaches the computer even when the one datagram saying it is lost. A press keyed
/// by its voice stops being held; the station ends it 200 ms later and says so on the road. That
/// datagram is lost here (the test reads it and throws it away), so the computer, which never heard
/// it, goes on re-asserting the press. Each of those holds is answered with the press's end, for the
/// reason the station said first, one datagram for each, so the computer's next word from the
/// station is that the over ended. CONTROL: the station said the end once, before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_for_a_press_the_station_ended_is_answered_with_its_end() {
    let app = App::new();
    let mut r = roadside(&app);
    let _record = held_on_the_engine(&mut r, gate(false), "none");
    r.s.shared
        .engine
        .lock()
        .unwrap()
        .set_operating_mode("phone", false);
    let mut seat = seated(&r).await;
    seat.renewed().await;
    let done = Arc::new(AtomicBool::new(false));
    let radio = radio_loop(
        r.s.shared.engine.clone(),
        Over::Voice,
        done.clone(),
        Instant::now() + 2 * PRESENCE,
    );
    let mut encoder = MicEncoder::new().unwrap();
    speak(&seat, &mut encoder, 1, 1, 20, |_| true).await;
    assert!(
        r.s.shared.engine.lock().unwrap().mic_keyed(),
        "premise: on the air"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let lost = seat.road_says(Duration::from_millis(200)).await;
    let end = presses(&lost).iter().find_map(|down| match down {
        Down::Press {
            press: 1,
            end: Some(end),
            ..
        } => Some(*end),
        _ => None,
    });
    assert!(
        end.is_some(),
        "premise: the station said the press ended: {lost:?}"
    );
    for _ in 0..5 {
        seat.road.say(&Up::Hold { press: 1 }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let said = seat.road_says(Duration::from_millis(300)).await;
    done.store(true, Ordering::SeqCst);
    drop(tokio::task::spawn_blocking(move || radio.join().unwrap()).await);
    let ends = presses(&said)
        .into_iter()
        .filter(|down| {
            **down
                == Down::Press {
                    press: 1,
                    keyed: false,
                    end,
                }
        })
        .count();
    assert_eq!(
        ends, 5,
        "five holds for a press the station had ended ({end:?}), and what it said: {said:?}"
    );
}

/// ⛔ The microphone over's state comes with every presence answer, so a computer that lost the one
/// datagram saying its over ended hears it at the next renewal, and its screen and its muted
/// receive follow. Two renewals, the over's state the same across them: each answer is followed by
/// that state. CONTROL: both renewals were answered, and the first state came (new to the road).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_microphones_state_comes_again_with_each_presence_answer() {
    let app = App::new();
    let mut r = roadside(&app);
    let _record = held_on_the_engine(&mut r, gate(false), "none");
    let mut seat = seated(&r).await;
    let mut answers = Vec::new();
    for _ in 0..2 {
        let (state, _) = seat.fresh_snapshot().await;
        seat.heartbeat(state).await;
        let said = seat.road_says(Duration::from_millis(300)).await;
        answers.push((
            said.iter()
                .any(|down| matches!(down, Down::Presence { .. })),
            said.iter().any(|down| matches!(down, Down::Mic(_))),
        ));
    }
    assert_eq!(
        answers,
        [(true, true), (true, true)],
        "(presence renewed, the microphone's state said) at each renewal"
    );
}
