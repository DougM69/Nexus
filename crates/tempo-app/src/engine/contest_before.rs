//! ⭐ **The station as it was before the contest changed it** — the note the contest-over
//! checklist offers back when Contest mode goes off: the station callsign a club set, the operator
//! at the key, the position, the entry category (MULTI-OP), the entry class, the power category and
//! Unassisted ([`ContestBefore`]).
//!
//! **When it is taken.** Only for a contest: while one is picked in Settings or Contest mode is on.
//! Then, the first time since the last contest ended that one of those seven changes — a Settings
//! save, a seat swap, the Unassisted switch — or Contest mode goes on: the seven as they were just
//! BEFORE that change. A club picks its contest and types its call, its position and MULTI-OP into
//! Settings on the Friday, long before anyone turns the mode on, so a note taken only at the switch
//! would hold the club's call as "before" and offer nothing back; taken at the first change it
//! holds the station's own. The pick is read as the change leaves it, so a save that picks the
//! contest and sets the club's call together takes the note. With no contest picked (the picker's
//! blank default) and the mode off, a change is the station's everyday business: no note is taken
//! and none is stored, so no later contest's checklist offers back a callsign changed months ago.
//!
//! **What the pick cannot tell.** The picker has no "none": a contest once picked stays picked after
//! it ends, so a station that has run one takes the note at an everyday change too, and its next
//! contest's checklist offers that back. And the blank default runs ARRL Field Day when the mode
//! goes on (the picker shows that contest lit), yet it is no pick here: a club that sets up for
//! Field Day without touching the picker has its note taken when the mode goes on, holding the
//! club's call.
//!
//! **When it is let go.** When Contest mode goes off: the contest is over, the checklist has
//! offered it, and whatever was not put back stays as it is. (The checklist puts Unassisted back
//! through its own switch BEFORE the save that turns the mode off, so that change finds the note
//! still open rather than starting a new one.) And at launch, when no contest is picked and the mode
//! is off: only an earlier build took a note then, at an everyday change, and one left standing
//! would be the first note of the next contest, keeping out the station as it really was. A note
//! with a contest picked or the mode on is the contest's, so a restart mid-weekend keeps it.
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
use crate::settings::{ContestBefore, Settings};

/// Is a contest picked in Settings? Anything but the picker's blank default, which spaces alone
/// still are.
fn contest_picked(s: &Settings) -> bool {
    !s.fd_event.trim().is_empty()
}

impl Engine {
    /// After a change to the settings: `was` is the seven as they were before it, `was_active`
    /// whether Contest mode was on. Ends the note when the mode went off; otherwise, while a
    /// contest is picked or the mode is on, takes it, if none is open, when the mode went on or one
    /// of the seven moved.
    pub(super) fn note_contest_before(&mut self, was: ContestBefore, was_active: bool) {
        let active = self.settings.fd_active;
        if was_active && !active {
            // Contest mode went off: the contest is over, and the note goes with it.
            self.settings.contest_before = None;
            return;
        }
        if !active && !contest_picked(&self.settings) {
            // No contest picked and none running: an everyday change, which takes no note.
            return;
        }
        // None open, and this change is the contest's first: the mode went on, or one of the seven
        // moved. What they were just before it is the station's own.
        let moved = ContestBefore::of(&self.settings) != was;
        if self.settings.contest_before.is_none() && ((active && !was_active) || moved) {
            self.settings.contest_before = Some(was);
        }
    }

    /// At launch, on the stored settings: lets go of a note held with no contest picked and the
    /// mode off, which only an earlier build took. One with a contest picked or the mode on is
    /// kept.
    pub(super) fn forget_an_everyday_note(s: &mut Settings) {
        if !s.fd_active && !contest_picked(s) {
            s.contest_before = None;
        }
    }
}
