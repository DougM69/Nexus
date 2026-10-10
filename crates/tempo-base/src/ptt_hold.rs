//! Why a held press ended, as the engine's held PTT (tempo-app's `remote_control::ptt_hold`) says
//! it and Stop's road carries it back to the computer that pressed.

/// Why a press ended, or was never keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldEnd {
    /// The station would not key it.
    Refused,
    /// No hold arrived for the held PTT's `GAP`.
    Lapsed,
    /// The page released it.
    Released,
    /// Something at the station unkeyed it: a Stop, a halt, the stream ending.
    Stopped,
}
