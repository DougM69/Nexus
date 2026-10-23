//! "Still there?" on the internet road (N5, the stream's own rule): what counts is the operator's own
//! input, a click, a key or a turn of the wheel in any window, and a held PTT for as long as it is
//! re-asserted. The station listening, the audio playing, the waterfall moving and the rig
//! transmitting never count, so a running FT sequence does not hold a link open. Fifteen minutes
//! without any of it brings "Still there?"; a minute more, unanswered, ends the link the way Leave
//! lets the station go, and nothing connects again until the operator answers.
//!
//! The bill tracks connected hours, so this is the lever on it. On this network it never applies:
//! the LAN road costs nothing.
//!
//! Measured, never counted: the watch reads the monotonic clock it is given each time it is asked.
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Fifteen minutes with no click, key, wheel or held PTT: "Still there?".
pub const PROMPT: Duration = Duration::from_secs(15 * 60);
/// A minute more, unanswered: the link ends.
pub const END: Duration = Duration::from_secs(16 * 60);

/// Where an internet link stands by the operator's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idle {
    Active,
    /// "Still there?" is shown.
    Prompt,
    /// Nobody answered: the link ends.
    End,
}

/// The time since the operator last did anything, against the prompt's and the end's bounds.
pub struct IdleWatch {
    last: Mutex<Instant>,
    prompt: Duration,
    end: Duration,
}

impl IdleWatch {
    /// The stream's bounds, [`PROMPT`] and [`END`].
    pub fn new(now: Instant) -> Self {
        Self::bounded(PROMPT, END, now)
    }

    /// Other bounds: for a test that cannot wait a quarter of an hour.
    pub fn bounded(prompt: Duration, end: Duration, now: Instant) -> Self {
        Self {
            last: Mutex::new(now),
            prompt,
            end,
        }
    }

    /// The operator did something at `now`.
    pub fn active(&self, now: Instant) {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        *last = (*last).max(now);
    }

    /// Where the link stands at `now`.
    pub fn state(&self, now: Instant) -> Idle {
        let last = *self.last.lock().unwrap_or_else(|p| p.into_inner());
        let idle = now.saturating_duration_since(last);
        if idle >= self.end {
            Idle::End
        } else if idle >= self.prompt {
            Idle::Prompt
        } else {
            Idle::Active
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ Fifteen minutes with nothing brings the prompt, sixteen the end; any input starts the count
    /// again, an earlier one never winds it back. CONTROL: just under each bound, the state before.
    #[test]
    fn fifteen_minutes_asks_and_one_more_ends() {
        let start = Instant::now();
        let watch = IdleWatch::new(start);
        let at = |d: Duration| start + d;
        let second = Duration::from_secs(1);
        assert_eq!(watch.state(at(PROMPT - second)), Idle::Active);
        assert_eq!(watch.state(at(PROMPT)), Idle::Prompt);
        assert_eq!(watch.state(at(END - second)), Idle::Prompt);
        assert_eq!(watch.state(at(END)), Idle::End);
        watch.active(at(END - second));
        assert_eq!(watch.state(at(END)), Idle::Active, "answered");
        watch.active(at(second));
        assert_eq!(
            watch.state(at(END - second + PROMPT)),
            Idle::Prompt,
            "an older input does not wind the count back"
        );
    }
}
