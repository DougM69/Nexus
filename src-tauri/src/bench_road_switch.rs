//! The bench's road switch, in test builds only: one control on the computer that drives a station
//! (client mode) that blocks the command link or Stop's road, one at a time, at this computer's own
//! transport, so each road can be tried alone at the bench. The design's bench asks for exactly
//! this: "a diagnostics switch on this computer to block the command link, block the Stop road ...
//! so that each road can be tried alone. Shipped builds never carry it." The design's third
//! switch, forcing the internet road through its relay, comes with that road: a fourth
//! [`Position`], one more button on the banner, and that road's transport reading the switch it
//! was opened under ([`Switch::opening`]) as the two here do.
//!
//! ## Only in a test build
//!
//! All of it is behind the `bench-road-switch` feature, off by default. Without it there is no
//! module, no command, no row in the class table and nothing in the banner's status, so nothing is
//! drawn, and every transport is compiled as it is without it. A test build is made with
//! `scripts/build-windows-cross.sh --bench-road-switch`. `shipped_without_the_road_switch` fails if
//! a release workflow or a build script the release runs passes the feature, if another feature or
//! a Tauri config turns it on, or if a build without it has any of it.
//!
//! ## Where it lives
//!
//! The switch itself, what a blocked road is and which roads it reaches are the client core's
//! (`tempo_client::bench_road_switch`, which this feature turns on there and nothing else does).
//! This module is the command that moves it.
use std::sync::Arc;

use tauri::{Manager, Runtime};

use crate::client_mode::Client;
/// The switch, where it is, and the command's name (the client core's).
pub(crate) use tempo_client::bench_road_switch::{Position, Switch, COMMAND};

/// The switch moved to `to` from this computer's screen: the position it is in now. Refused with
/// `oneRoadAtATime` for a road asked to be blocked while the other is, and with `notClient` on a
/// computer running its own station, which drives no roads.
#[tauri::command]
pub(crate) fn bench_road_switch<R: Runtime>(
    app: tauri::AppHandle<R>,
    to: Position,
) -> Result<Position, String> {
    let client = app.try_state::<Arc<Client>>().ok_or("notClient")?;
    let moved = client.road_switch().set(to)?;
    tempo_core::applog::info(
        "client",
        &format!("test build: the road switch is at {moved:?}"),
    );
    Ok(moved)
}

/// The app's commands with this one in front of them: what `app_commands` hands out in a test
/// build.
pub(crate) fn with_its_command<R: Runtime>(
    every_build: Box<dyn Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync>,
) -> Box<dyn Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync> {
    let its_own: Box<dyn Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync> =
        Box::new(tauri::generate_handler![bench_road_switch]);
    Box::new(move |invoke| {
        if invoke.message.command() == COMMAND {
            its_own(invoke)
        } else {
            every_build(invoke)
        }
    })
}
