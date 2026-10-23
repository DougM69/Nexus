//! What Nexus's engine shares with the crates below it, so that a build with no engine has it too:
//! the app's log ([`applog`]) and the UTC calendar its lines and the logbook's records are stamped
//! by ([`datetime_utc`]), and what the engine says of a remote over's microphone ([`mic`]) and of a
//! held press ([`ptt_hold`]), which Stop's road carries both ways. The client core (`tempo-client`)
//! builds on this crate and on none of the engine's; tempo-core and tempo-app name each item where
//! it always was. Nothing here needs another crate.
pub mod applog;
pub mod mic;
pub mod ptt_hold;

/// Unix seconds → (year, month, day, hour, min, sec) UTC, via Howard Hinnant's
/// civil-from-days algorithm (no external crates). `pub` so the ALL.TXT decode log
/// (tempo-app) can format WSJT-X-style UTC timestamps without a date dependency.
pub fn datetime_utc(unix: u64) -> (i32, u32, u32, u32, u32, u32) {
    let secs = unix as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d, h, mi, s)
}
