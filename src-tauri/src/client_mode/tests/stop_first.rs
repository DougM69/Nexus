//! Stop wins over a start that was in flight, at this computer: a start still waiting here for a
//! slot under the caps when a Stop is pressed never leaves after it.
use super::*;

/// ⛔ A start pressed before a Stop never leaves after it. Call CQ is pressed five times while the
/// shack is slow to answer: four go, and one waits here for a slot (`PER_COMMAND`); which one is the
/// runtime's to say, since the five start on its threads in no set order. Stop is pressed: it takes
/// no slot and goes at once. When the shack answers, the CQ that waited, pressed before the Stop, is
/// refused here, `stopped`, and never sent. CONTROL: the premises are asserted (four in flight, the
/// fifth held back, the Stop sent at once).
#[test]
fn a_start_pressed_before_a_stop_never_leaves_after_it() {
    let (shack, _end, _client, s, dir) = present_for(Duration::from_secs(10));
    shack.hold();
    let pressed: Vec<_> = (0..5)
        .map(|_| send(&s.main, "call_cq", json!({})))
        .collect();
    eventually("four CQs in flight", || shack.sent().len() == 4);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(shack.sent().len(), 4, "premise: the fifth waits for a slot");
    let stop = send(&s.main, "halt_tx", json!({}));
    eventually("the Stop sent at once", || shack.sent().len() == 5);
    assert_eq!(shack.sent()[4], "halt_tx", "premise: {:?}", shack.sent());
    shack.let_go();
    let answers: Vec<_> = pressed
        .into_iter()
        .map(|answer| answer.recv_timeout(Duration::from_secs(5)).ok())
        .collect();
    let _ = stop.recv_timeout(Duration::from_secs(5));
    let sent = shack.sent();
    let stopped_at = sent.iter().position(|cmd| cmd == "halt_tx").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let stopped = answers
        .iter()
        .filter(|answer| **answer == Some(refused("stopped")))
        .count();
    assert!(
        sent[stopped_at + 1..].iter().all(|cmd| cmd != "call_cq") && stopped == 1,
        "a Call CQ pressed before Stop was sent after it: {sent:?}, the answers {answers:?}"
    );
}
