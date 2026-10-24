//! A cold launch (the icon, no `--client` argument) connects again to the station this computer
//! last drove, unless Leave cleared the target or the station is no longer known here.
use super::*;

/// An address the operator typed for the station, kept with the target.
const AT: &str = "192.168.1.30:51800";

/// (a) After this computer enters client mode for a station, a cold start resolves to client mode
/// for that same station, at the address that was typed for it.
#[test]
fn a_cold_start_after_entering_resolves_to_the_station() {
    let path = scratch("reconnect-enter").join("client-target.json");
    let typed = Typed::read(AT).expect("a typed address");
    keep_target_at(&path, STATION, Some(&typed));
    // The cold start reads the kept target; the station is still known here.
    assert_eq!(
        resolve_target(read_target_at(&path), |id| id == STATION),
        Some((STATION.to_string(), Some(typed))),
        "a cold start did not resolve to the station last driven"
    );
}

/// (b) After Leave clears the target, a cold start resolves to local — this computer's own station.
#[test]
fn a_cold_start_after_leaving_resolves_to_local() {
    let path = scratch("reconnect-leave").join("client-target.json");
    keep_target_at(&path, STATION, None);
    clear_target_at(&path);
    assert_eq!(
        resolve_target(read_target_at(&path), |_| true),
        None,
        "a cold start after Leave entered client mode"
    );
}

/// (c) A kept target whose station this computer no longer knows (removed or forgotten) resolves
/// to local: a cold launch never drives a station the operator can no longer see.
#[test]
fn a_cold_start_for_a_forgotten_station_resolves_to_local() {
    let path = scratch("reconnect-forgotten").join("client-target.json");
    let typed = Typed::read(AT).expect("a typed address");
    keep_target_at(&path, STATION, Some(&typed));
    assert_eq!(
        resolve_target(read_target_at(&path), |_| false),
        None,
        "a cold start drove a station this computer no longer knows"
    );
}
