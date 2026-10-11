//! ⭐ **The station as it was before the contest changed it** — the note the contest-over
//! checklist offers back when Contest mode goes off: the station callsign a club set, the operator
//! at the key, the position, the entry category (MULTI-OP), the entry class, the power category and
//! Unassisted ([`ContestBefore`]).
//!
//! **When it is taken.** The first time, since the last contest ended, that one of those seven
//! changes — a Settings save, a seat swap, the Unassisted switch — or Contest mode goes on: the
//! seven as they were just BEFORE that change. A club types its call, its position and MULTI-OP
//! into Settings on the Friday, long before anyone turns the mode on, so a note taken only at the
//! switch would hold the club's call as "before" and offer nothing back; taken at the first change
//! it holds the station's own.
//!
//! **When it is let go.** When Contest mode goes off: the contest is over, the checklist has
//! offered it, and whatever was not put back stays as it is. (The checklist puts Unassisted back
//! through its own switch BEFORE the save that turns the mode off, so that change finds the note
//! still open rather than starting a new one.)
//!
//! **Who writes it.** Only this. A Settings save carries a copy of every field, and a copy is a
//! snapshot from whenever its surface loaded, so `apply_settings` keeps the engine's own note
//! whatever the payload says: a stale panel can neither write one nor wipe one. A RESTORE takes the
//! backup's, as it takes everything else.
//!
//! **What it never does.** Put anything back. It is a note; the checklist shows it beside today's
//! values and the operator ticks what goes back, through the ordinary Settings save and the
//! Unassisted switch. Nothing here keys, unkeys or stops a transmission.
use super::Engine;
use crate::settings::ContestBefore;

impl Engine {
    /// After a change to the settings: `was` is the seven as they were before it, `was_active`
    /// whether Contest mode was on. Ends the note when the mode went off; otherwise takes it, if
    /// none is open, when the mode went on or one of the seven moved.
    pub(super) fn note_contest_before(&mut self, was: ContestBefore, was_active: bool) {
        let active = self.settings.fd_active;
        if was_active && !active {
            // Contest mode went off: the contest is over, and the note goes with it.
            self.settings.contest_before = None;
            return;
        }
        // None open, and this change is the contest's first: the mode went on, or one of the seven
        // moved. What they were just before it is the station's own.
        let moved = ContestBefore::of(&self.settings) != was;
        if self.settings.contest_before.is_none() && ((active && !was_active) || moved) {
            self.settings.contest_before = Some(was);
        }
    }
}
