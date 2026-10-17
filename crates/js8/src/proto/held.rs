//! Held messages, looked up as JS8Call 3.0.3 looks them up: `Inbox.cpp` (:298-355, :418-720) and
//! the facades over it (`mainwindow.cpp`:6388-6501), read as the spec. They say what a heartbeat's
//! acknowledgement and `QUERY MSGS` offer, what comes after a `QUERY MSG n` delivery (`NEXT MSG
//! ID`), and how many more wait (the `+k` 3.0 added), for one station's messages and for a
//! group's.
//!
//! Pure functions over the inbox and over who has been handed a group message (JS8Call's
//! `inbox_group_recip_v1`, here `(id, call)` pairs); the station gates and schedules.
//!
//! JS8Call's rules, as these keep them:
//! - A held message is a `STORE` row with text. Its recipient is compared with SQLite's `LIKE`,
//!   which for a callsign is a comparison blind to case.
//! - After a delivery, the next one for a station is the oldest other one held for the call as
//!   heard, else for its base call (`getLookaheadMessageIdForCallsign`, :6416-6435). Each lookup
//!   reads the oldest ten rows and takes the first with text (`LIMIT 10`, Inbox.cpp:318), as the
//!   station's own offer does (`Station::stored_for`).
//! - How many wait for a station counts the messages held for the call AS HEARD, with text, and
//!   not those held for its base call (`countUnreadForCallsign`, Inbox.cpp:611-654).
//! - A group's message is offered for two days after it was stored (`addDays(-2)`,
//!   Inbox.cpp:503-507), to a station it has not been handed to yet under the call as heard
//!   (`getNextGroupMessageIdForCallsign`, `countGroupUnreadForCallsign` and
//!   `getLookaheadGroupMessageIdForCallsign`, Inbox.cpp:468-720; the lookahead tries the base call
//!   too, mainwindow.cpp:6449-6470).
use crate::proto::station::{has_text, radio_base_callsign, InboxEntry, InboxState};

/// How long a group's message is offered, and held: two days.
pub const GROUP_HOLD_MS: u64 = 48 * 60 * 60 * 1000;

/// JS8Call reads ten rows a lookup.
const PAGE: usize = 10;

/// A message held for a group (`@…`), handed to each station that asks for it.
pub fn is_group(to: &str) -> bool {
    to.starts_with('@')
}

/// Whether `id` has been handed to `who` (the station as heard).
pub fn handed(recipients: &[(u32, String)], id: u32, who: &str) -> bool {
    recipients.iter().any(|(i, c)| *i == id && c == who)
}

fn held_under(e: &InboxEntry, call: &str) -> bool {
    e.state == InboxState::Store && e.to.eq_ignore_ascii_case(call)
}

/// Held for `group`, stored in the last two days, and not yet handed to `who`.
fn open_for(
    e: &InboxEntry,
    group: &str,
    who: &str,
    recipients: &[(u32, String)],
    now_ms: u64,
) -> bool {
    held_under(e, group)
        && now_ms.saturating_sub(e.at_ms) < GROUP_HOLD_MS
        && !handed(recipients, e.id, who)
}

/// The oldest ten that `keep` passes, the first of them with text.
fn first_of_ten(inbox: &[InboxEntry], keep: impl Fn(&InboxEntry) -> bool) -> Option<u32> {
    let mut rows: Vec<&InboxEntry> = inbox.iter().filter(|e| keep(e)).collect();
    rows.sort_by_key(|e| e.id);
    rows.into_iter()
        .take(PAGE)
        .find(|e| has_text(e))
        .map(|e| e.id)
}

/// The messages held for `who` as heard, with text.
pub fn count_for(inbox: &[InboxEntry], who: &str) -> usize {
    inbox
        .iter()
        .filter(|e| held_under(e, who) && has_text(e))
        .count()
}

/// The next message held for `who` after `after`.
pub fn lookahead_for(inbox: &[InboxEntry], who: &str, after: u32) -> Option<u32> {
    first_of_ten(inbox, |e| e.id != after && held_under(e, who)).or_else(|| {
        let base = radio_base_callsign(who);
        first_of_ten(inbox, |e| e.id != after && held_under(e, &base))
    })
}

/// The next message of `group` for `who`.
pub fn group_next_for(
    inbox: &[InboxEntry],
    recipients: &[(u32, String)],
    group: &str,
    who: &str,
    now_ms: u64,
) -> Option<u32> {
    first_of_ten(inbox, |e| open_for(e, group, who, recipients, now_ms))
}

/// The messages of `group` still to be handed to `who`, with text.
pub fn group_count_for(
    inbox: &[InboxEntry],
    recipients: &[(u32, String)],
    group: &str,
    who: &str,
    now_ms: u64,
) -> usize {
    inbox
        .iter()
        .filter(|e| open_for(e, group, who, recipients, now_ms) && has_text(e))
        .count()
}

/// The next message of `group` for `who` after `after`.
pub fn group_lookahead_for(
    inbox: &[InboxEntry],
    recipients: &[(u32, String)],
    group: &str,
    who: &str,
    after: u32,
    now_ms: u64,
) -> Option<u32> {
    first_of_ten(inbox, |e| {
        e.id != after && open_for(e, group, who, recipients, now_ms)
    })
    .or_else(|| {
        let base = radio_base_callsign(who);
        first_of_ten(inbox, |e| {
            e.id != after && open_for(e, group, &base, recipients, now_ms)
        })
    })
}

/// JS8Call 3.0's `pendingCount` (processCommandActivity.cpp:668-674, :938-947, :1000-1007): the
/// messages held for `who` as heard, plus, when the heartbeat or query was addressed to a group I
/// joined, that group's still to be handed to `who`, less `taken`: 1 past an offer's `MSG ID n`,
/// 2 past a delivery, whose message is still held until its reply goes and whose `NEXT MSG ID`
/// is named. Said as `+k` only when above zero.
pub fn pending(
    inbox: &[InboxEntry],
    recipients: &[(u32, String)],
    who: &str,
    group: Option<&str>,
    now_ms: u64,
    taken: usize,
) -> i64 {
    let mut n = count_for(inbox, who);
    if let Some(g) = group {
        n += group_count_for(inbox, recipients, g, who, now_ms);
    }
    n as i64 - taken as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(id: u32, to: &str, text: &str, at_ms: u64) -> InboxEntry {
        InboxEntry {
            id,
            from: "W1AW".into(),
            to: to.into(),
            text: text.into(),
            path: Vec::new(),
            state: InboxState::Store,
            at_ms,
            freq_hz: 1500.0,
            snr_db: -5,
        }
    }

    /// JS8Call counts what waits for the call as heard, never for its base call: a station heard
    /// as K1ABC/P is told of no more than the one it is offered, whatever K1ABC holds.
    #[test]
    fn the_count_is_of_the_call_as_heard_with_text_held() {
        let mut delivered = held(4, "K1ABC", "OLD", 0);
        delivered.state = InboxState::Delivered;
        let inbox = vec![
            held(1, "K1ABC", "ONE", 0),
            held(2, "k1abc", "TWO", 0),
            held(3, "K1ABC", "  ", 0),
            delivered,
            held(5, "W9XYZ", "ELSE", 0),
        ];
        assert_eq!(count_for(&inbox, "K1ABC"), 2);
        assert_eq!(count_for(&inbox, "K1ABC/P"), 0);
    }

    /// The next one after a delivery: the oldest other one with text, under the call as heard,
    /// else under its base call; ids below the delivered one count too (`id != ?`).
    #[test]
    fn the_lookahead_is_the_oldest_other_one_as_heard_then_by_base_call() {
        let inbox = vec![
            held(1, "K1ABC", "ONE", 0),
            held(2, "K1ABC", "", 0),
            held(3, "K1ABC", "THREE", 0),
            held(4, "K1ABC/MM", "FOUR", 0),
        ];
        assert_eq!(lookahead_for(&inbox, "K1ABC", 1), Some(3));
        assert_eq!(lookahead_for(&inbox, "K1ABC", 3), Some(1));
        assert_eq!(
            lookahead_for(&inbox, "K1ABC/MM", 4),
            Some(1),
            "then under the base call"
        );
        assert_eq!(lookahead_for(&inbox, "K1ABC/MM", 99), Some(4));
        assert_eq!(lookahead_for(&inbox, "N0XYZ", 1), None);
    }

    /// A lookup reads ten rows: an eleventh with text behind ten without is not found.
    #[test]
    fn a_lookup_reads_the_oldest_ten_rows() {
        let mut inbox: Vec<InboxEntry> = (1..=10).map(|i| held(i, "@FUN", "", 0)).collect();
        inbox.push(held(11, "@FUN", "LATE", 0));
        assert_eq!(group_next_for(&inbox, &[], "@FUN", "K1ABC", 0), None);
        inbox.remove(0);
        assert_eq!(group_next_for(&inbox, &[], "@FUN", "K1ABC", 0), Some(11));
    }

    /// A group's message goes to each station once, for two days: handed to K1ABC, it is not
    /// offered or counted for K1ABC again, and still is for N0XYZ; at two days it is offered to
    /// nobody.
    #[test]
    fn a_group_message_is_offered_to_each_station_once_for_two_days() {
        let inbox = vec![held(1, "@FUN", "ONE", 1_000), held(2, "@FUN", "TWO", 1_000)];
        let recipients = vec![(1, "K1ABC".to_string())];
        let now = 1_000 + GROUP_HOLD_MS - 1;
        assert_eq!(
            group_next_for(&inbox, &recipients, "@FUN", "K1ABC", now),
            Some(2)
        );
        assert_eq!(
            group_next_for(&inbox, &recipients, "@FUN", "N0XYZ", now),
            Some(1)
        );
        assert_eq!(
            group_count_for(&inbox, &recipients, "@FUN", "K1ABC", now),
            1
        );
        assert_eq!(
            group_count_for(&inbox, &recipients, "@FUN", "N0XYZ", now),
            2
        );
        assert_eq!(
            group_next_for(&inbox, &recipients, "@FUN", "N0XYZ", now + 1),
            None,
            "two days on, nobody"
        );
        assert_eq!(
            group_next_for(&inbox, &recipients, "@OTHER", "N0XYZ", now),
            None
        );
        assert_eq!(
            group_lookahead_for(&inbox, &recipients, "@FUN", "N0XYZ", 1, now),
            Some(2)
        );
        assert_eq!(
            group_lookahead_for(&inbox, &recipients, "@FUN", "K1ABC", 2, now),
            None,
            "K1ABC has had the other"
        );
    }

    /// 3.0's `pendingCount`: what waits, the group's too when the query was to the group, less
    /// what the reply itself names.
    #[test]
    fn pending_is_what_waits_less_what_the_reply_names() {
        let inbox = vec![
            held(1, "K1ABC", "ONE", 0),
            held(2, "K1ABC", "TWO", 0),
            held(3, "@FUN", "GROUP", 0),
        ];
        assert_eq!(pending(&inbox, &[], "K1ABC", None, 0, 1), 1);
        assert_eq!(pending(&inbox, &[], "K1ABC", Some("@FUN"), 0, 1), 2);
        assert_eq!(pending(&inbox, &[], "K1ABC", Some("@FUN"), 0, 2), 1);
        assert_eq!(pending(&inbox, &[], "K1ABC/P", None, 0, 2), -2);
    }
}
