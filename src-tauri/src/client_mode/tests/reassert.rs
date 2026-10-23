//! The held press here, against presence as this computer's Stop road last heard it.
use super::*;
use crate::remote_service::lan::stop_road::Up;

/// ⛔ A re-assertion holds the press only while presence is live here. A press starts under live
/// presence; the station's renewals stop coming, and presence as this computer heard it lapses
/// (its Stop road gone quiet, say). The screen's next re-assertion is refused, the press ends here
/// (`pressEnded`: the screen reads "Over ended — release PTT"), and no other hold goes on the road;
/// the press is let go there. CONTROL: the first press went, and presence was heard lapsed before
/// the re-assertion.
#[test]
fn a_reassertion_holds_the_press_only_while_presence_is_live_here() {
    let (shack, end, _client, s, dir) = present_for(Duration::from_millis(600));
    let road = lock(&shack.road).clone().unwrap();
    assert!(
        ask(&s.main, "set_ptt", json!({"on": true})).is_ok(),
        "CONTROL: the press starts under live presence"
    );
    eventually("its first hold", || end.presses().len() == 1);
    eventually("presence lapsed here", || {
        !road.presence_live(std::time::Instant::now())
    });
    let again = ask(&s.main, "set_ptt", json!({"on": true}));
    std::thread::sleep(Duration::from_millis(100));
    let presses = end.presses();
    std::fs::remove_dir_all(&dir).unwrap();
    let holds = presses
        .iter()
        .filter(|up| matches!(up, Up::Hold { .. }))
        .count();
    assert!(
        again == refused("pressEnded") && holds == 1,
        "re-asserted with presence lapsed here: {again:?}, {holds} holds on the road"
    );
    assert_eq!(
        presses.last(),
        Some(&Up::Release { press: 1 }),
        "the press let go on its road"
    );
}
