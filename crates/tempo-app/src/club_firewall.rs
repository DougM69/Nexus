//! A press of **Allow Nexus through Windows Firewall**, or of its Remove (the club section of
//! Settings ▸ Contesting, on Windows): its refusals, the one elevated run of the command
//! `tempo_net::fdfirewall` builds, and the club section's reading of the firewall for the ports
//! as set. The rules, the command and what a reading means are `tempo_net::fdfirewall`'s; the
//! elevation is `tempo_core::process::run_elevated`, the one **Repair clock** asks through.
//!
//! ⚠️ **Not while transmitting.** Windows' administrator prompt takes the whole screen until it
//! is answered, and while it is up the operator cannot reach Stop in Nexus. So a press while
//! anything is on the air ([`Engine::on_air`]) is refused, and the prompt never comes up over an
//! over. Unlike Repair clock's, a press holds nothing afterwards: a firewall rule moves no clock
//! and keys nothing, so the transmit path is left exactly as it was.

use crate::engine::{engine_lock, Engine};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tempo_net::fdfirewall::{Reading, Status};

/// What a press asks Windows to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallAction {
    /// Add Nexus's three rules, replacing any under their names.
    Allow,
    /// Remove every rule under Nexus's three names.
    Remove,
}

/// Why a press ran nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallRefusal {
    /// Something is on the air ([`Engine::on_air`]).
    OnAir,
    /// A press is still waiting on Windows' answer.
    Running,
    /// The command could not be built: a path it cannot quote, or a port of 0.
    Command(tempo_net::fdfirewall::Refusal),
}

impl FirewallRefusal {
    /// The code the screens switch on.
    pub fn code(self) -> &'static str {
        match self {
            FirewallRefusal::OnAir => "onAir",
            FirewallRefusal::Running => "running",
            FirewallRefusal::Command(why) => why.code(),
        }
    }
}

/// One press at a time: a second press while Windows is still asking about the first is
/// refused, never queued as a second prompt.
pub struct FirewallPress(AtomicBool);

impl FirewallPress {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }
}

impl Default for FirewallPress {
    fn default() -> Self {
        Self::new()
    }
}

/// The shell's: one for the whole program, as Windows' prompt is.
pub static FIREWALL_PRESS: FirewallPress = FirewallPress::new();

/// A press under way; dropping it ends the press, however it ends.
struct Pressing<'a>(&'a AtomicBool);

impl Drop for Pressing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// What a press did, for the club section: whether `run` reported the command run and exited 0
/// (`false` too when the prompt was declined), and the firewall read back afterwards, which is
/// what the section shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PressOutcome {
    pub ran: bool,
    pub status: Option<Status>,
}

/// Press `action` for the executable `exe`: refused while a press is waiting on Windows, while
/// anything is on the air, and when the command cannot be built; otherwise `run` (the elevated
/// runner) gets the command once, with no lock held, since it waits on Windows' prompt. The Host
/// and scoreboard ports are the ones set, read under the engine lock with the on-air check.
pub fn press(
    engine: &Arc<Mutex<Engine>>,
    pressing: &FirewallPress,
    action: FirewallAction,
    exe: &str,
    run: impl FnOnce(&str) -> bool,
) -> Result<bool, FirewallRefusal> {
    if pressing.0.swap(true, Ordering::AcqRel) {
        return Err(FirewallRefusal::Running);
    }
    let _pressing = Pressing(&pressing.0);
    let args = {
        let eng = engine_lock(engine);
        if eng.on_air() {
            return Err(FirewallRefusal::OnAir);
        }
        let s = eng.settings();
        match action {
            FirewallAction::Allow => {
                tempo_net::fdfirewall::allow_args(exe, s.fd_host_port, s.fd_scoreboard_port)
            }
            FirewallAction::Remove => Ok(tempo_net::fdfirewall::remove_args()),
        }
    }
    .map_err(FirewallRefusal::Command)?;
    Ok(run(&args))
}

/// What the club section says about the firewall for `exe` and the ports as set, through `read`
/// (`tempo_net::fdfirewall::read`, which needs no administrator rights). `None` where the
/// firewall cannot be read.
pub fn status(
    engine: &Arc<Mutex<Engine>>,
    exe: &str,
    read: impl FnOnce(&str) -> Option<Reading>,
) -> Option<Status> {
    let (host_port, board_port) = {
        let eng = engine_lock(engine);
        let s = eng.settings();
        (s.fd_host_port, s.fd_scoreboard_port)
    };
    read(exe).map(|reading| tempo_net::fdfirewall::status(&reading, exe, host_port, board_port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempo_net::fdfirewall::{allow_args, remove_args, State};

    const EXE: &str = r"C:\Users\Ham Op\AppData\Local\Nexus\Nexus.exe";

    fn engine(host_port: u16, board_port: u16) -> Arc<Mutex<Engine>> {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let mut s = e.settings().clone();
        s.fd_host_port = host_port;
        s.fd_scoreboard_port = board_port;
        e.apply_settings(s);
        Arc::new(Mutex::new(e))
    }

    /// ⭐ **THE TEXT WINDOWS RUNS for a press of Allow**, end to end: PowerShell's
    /// `Start-Process -Verb RunAs` (the one administrator prompt), handing `cmd.exe` the
    /// command whole in a single-quoted string, for a path with a space and an apostrophe
    /// (doubled for PowerShell, and only for PowerShell: `cmd.exe` gets one).
    #[test]
    fn the_text_windows_runs_for_a_press() {
        let exe = r"C:\Users\Pat O'Hara\AppData\Local\Nexus\Nexus.exe";
        let ran = RefCell::new(Vec::new());
        let e = engine(42073, 7373);
        let pressing = FirewallPress::new();
        let ok = press(&e, &pressing, FirewallAction::Allow, exe, |args| {
            ran.borrow_mut()
                .push(tempo_core::process::elevated_script(args));
            true
        });
        assert_eq!(ok, Ok(true));
        assert_eq!(
            ran.into_inner(),
            vec![concat!(
                "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath cmd.exe -ArgumentList '",
                r#"/d /v:off /c "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - host (TCP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - finding events (UDP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - TV scoreboard (TCP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - host (TCP)" dir=in action=allow program="C:\Users\Pat O''Hara\AppData\Local\Nexus\Nexus.exe" protocol=TCP localport=42073 profile=private,public enable=yes && "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - finding events (UDP)" dir=in action=allow program="C:\Users\Pat O''Hara\AppData\Local\Nexus\Nexus.exe" protocol=UDP localport=42074 profile=private,public enable=yes && "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - TV scoreboard (TCP)" dir=in action=allow program="C:\Users\Pat O''Hara\AppData\Local\Nexus\Nexus.exe" protocol=TCP localport=7373 profile=private,public enable=yes"#,
                "' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode",
            )
            .to_string()]
        );
    }

    /// A press runs its command once, for the ports as SET (a Host port changed from the
    /// default included), and reports what the runner said.
    #[test]
    fn a_press_runs_its_command_once_for_the_ports_as_set() {
        let e = engine(50000, 7474);
        let pressing = FirewallPress::new();
        let ran = RefCell::new(Vec::new());
        let run = |args: &str| {
            ran.borrow_mut().push(args.to_string());
            false
        };
        assert_eq!(
            press(&e, &pressing, FirewallAction::Allow, EXE, run),
            Ok(false)
        );
        assert_eq!(
            press(&e, &pressing, FirewallAction::Remove, EXE, |args| {
                ran.borrow_mut().push(args.to_string());
                true
            }),
            Ok(true)
        );
        assert_eq!(
            ran.into_inner(),
            vec![allow_args(EXE, 50000, 7474).unwrap(), remove_args()]
        );
    }

    /// ⚠️ **Not while anything is on the air** (here a rig keyed at the radio, which Nexus did not
    /// start): the prompt would take the screen, Stop with it, over an over. Nothing runs. The
    /// control: the same press with the rig unkeyed runs.
    #[test]
    fn refused_while_anything_is_on_the_air() {
        let e = engine(42073, 7373);
        let pressing = FirewallPress::new();
        let runs = RefCell::new(0);
        engine_lock(&e).observe_rig_ptt(true);
        assert_eq!(
            press(&e, &pressing, FirewallAction::Allow, EXE, |_| {
                *runs.borrow_mut() += 1;
                true
            }),
            Err(FirewallRefusal::OnAir)
        );
        assert_eq!(
            press(&e, &pressing, FirewallAction::Remove, EXE, |_| {
                *runs.borrow_mut() += 1;
                true
            }),
            Err(FirewallRefusal::OnAir)
        );
        assert_eq!(*runs.borrow(), 0);
        engine_lock(&e).observe_rig_ptt(false);
        assert_eq!(
            press(&e, &pressing, FirewallAction::Allow, EXE, |_| {
                *runs.borrow_mut() += 1;
                true
            }),
            Ok(true)
        );
        assert_eq!(*runs.borrow(), 1);
    }

    /// A second press while Windows is still asking about the first is refused, not queued as a
    /// second prompt; once the first is answered, a press runs again.
    #[test]
    fn a_second_press_while_windows_asks_is_refused() {
        let e = engine(42073, 7373);
        let pressing = FirewallPress::new();
        let inner = RefCell::new(None);
        let first = press(&e, &pressing, FirewallAction::Allow, EXE, |_| {
            *inner.borrow_mut() = Some(press(&e, &pressing, FirewallAction::Remove, EXE, |_| true));
            true
        });
        assert_eq!(first, Ok(true));
        assert_eq!(inner.into_inner(), Some(Err(FirewallRefusal::Running)));
        assert_eq!(
            press(&e, &pressing, FirewallAction::Remove, EXE, |_| true),
            Ok(true)
        );
        // A refused press ends its turn too.
        engine_lock(&e).observe_rig_ptt(true);
        assert!(press(&e, &pressing, FirewallAction::Allow, EXE, |_| true).is_err());
        engine_lock(&e).observe_rig_ptt(false);
        assert_eq!(
            press(&e, &pressing, FirewallAction::Allow, EXE, |_| true),
            Ok(true)
        );
    }

    /// A path the command cannot quote runs nothing, and says why; so does a port of 0.
    #[test]
    fn a_command_that_cannot_be_built_runs_nothing() {
        let e = engine(42073, 7373);
        let pressing = FirewallPress::new();
        let runs = RefCell::new(0);
        let refused = press(
            &e,
            &pressing,
            FirewallAction::Allow,
            r"C:\Users\100%\Nexus.exe",
            |_| {
                *runs.borrow_mut() += 1;
                true
            },
        );
        assert_eq!(
            refused,
            Err(FirewallRefusal::Command(
                tempo_net::fdfirewall::Refusal::Path
            ))
        );
        assert_eq!(refused.unwrap_err().code(), "path");
        let zero = engine(42073, 0);
        assert_eq!(
            press(&zero, &pressing, FirewallAction::Allow, EXE, |_| {
                *runs.borrow_mut() += 1;
                true
            })
            .map_err(FirewallRefusal::code),
            Err("port")
        );
        assert_eq!(*runs.borrow(), 0);
        // Remove names no path and no port: it is never refused for one.
        assert_eq!(
            press(
                &e,
                &pressing,
                FirewallAction::Remove,
                r"C:\Users\100%\Nexus.exe",
                |_| true
            ),
            Ok(true)
        );
    }

    /// The section's reading is for the ports as set: a Host port changed since the press reads
    /// as out of date.
    #[test]
    fn the_reading_is_for_the_ports_as_set() {
        let made = |host: u16| {
            let mut reading = Reading {
                private: tempo_net::fdfirewall::ProfileReading {
                    on: true,
                    blocks_all: false,
                },
                ..Default::default()
            };
            reading.public = reading.private;
            for rule in tempo_net::fdfirewall::rules(host, 7373) {
                reading.rules.push(tempo_net::fdfirewall::RuleReading {
                    name: rule.name.into(),
                    program: EXE.into(),
                    allow: true,
                    enabled: true,
                    profiles: 6,
                    protocol: if rule.tcp { 6 } else { 17 },
                    local_ports: rule.port.to_string(),
                });
            }
            reading
        };
        let e = engine(42073, 7373);
        let read_exe = RefCell::new(String::new());
        let st = status(&e, EXE, |exe| {
            *read_exe.borrow_mut() = exe.to_string();
            Some(made(42073))
        })
        .unwrap();
        assert_eq!((st.state, st.host_port), (State::InPlace, 42073));
        assert_eq!(read_exe.into_inner(), EXE);
        let moved = engine(50000, 7373);
        assert_eq!(
            status(&moved, EXE, |_| Some(made(42073))).map(|st| st.state),
            Some(State::Stale)
        );
        assert_eq!(
            status(&e, EXE, |_| None),
            None,
            "nothing read, nothing said"
        );
    }
}
