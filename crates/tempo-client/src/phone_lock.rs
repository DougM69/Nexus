//! Phone's Lock (hands-free PTT) from a computer driving a station: a held press this computer's
//! screen keeps re-asserting with no finger on it. On the roads it is a held press and nothing
//! else, so every rule of one holds: the shack ends it 200 ms after its last hold or its last
//! frame of audio, when this computer's presence lapses, and at a Stop on either road.
//!
//! What a finger bounds, a cap bounds instead: [`CAP`] on the air from the press, or from the
//! operator's last answer to "Still talking?", which is asked [`ASK_BEFORE`] ahead of it. Nobody
//! answering, the press ends at the cap and never keys again.
//!
//! Measured, never counted: the watch reads the monotonic clock it is given each time it is asked.
use std::time::{Duration, Instant};

/// The longest Lock from afar stays on the air without an answer: three minutes, past any contest
/// over and most ragchew overs.
pub const CAP: Duration = Duration::from_secs(3 * 60);
/// How long before the cap "Still talking?" is asked.
pub const ASK_BEFORE: Duration = Duration::from_secs(20);

/// Where a Lock stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locked {
    /// On the air, with no question yet.
    Talking,
    /// "Still talking?" is asked: the cap is `left` away.
    Ask { left: Duration },
    /// The cap: the press ends.
    Capped,
}

/// Where a Lock whose count began `since` stands at `now`, under `cap`, asked `ask_before` it.
pub fn locked(since: Instant, cap: Duration, ask_before: Duration, now: Instant) -> Locked {
    let on = now.saturating_duration_since(since);
    if on >= cap {
        Locked::Capped
    } else if on + ask_before >= cap {
        Locked::Ask { left: cap - on }
    } else {
        Locked::Talking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ Talking until twenty seconds before three minutes, asked then with the time left, capped
    /// at three minutes. CONTROL: just under each bound, the state before.
    #[test]
    fn asked_twenty_seconds_before_the_three_minute_cap() {
        let start = Instant::now();
        let at = |d: Duration| locked(start, CAP, ASK_BEFORE, start + d);
        let ms = Duration::from_millis(1);
        assert_eq!(CAP, Duration::from_secs(180));
        assert_eq!(at(CAP - ASK_BEFORE - ms), Locked::Talking);
        assert_eq!(at(CAP - ASK_BEFORE), Locked::Ask { left: ASK_BEFORE });
        assert_eq!(at(CAP - ms), Locked::Ask { left: ms });
        assert_eq!(at(CAP), Locked::Capped);
        assert_eq!(at(CAP * 2), Locked::Capped);
    }
}
