//! Club sync through Windows Firewall: the inbound rules **Allow Nexus through Windows Firewall**
//! adds (the club section of Settings ▸ Contesting, on Windows), the exact command that adds or
//! removes them under Windows' administrator prompt, and what the club section says about the
//! firewall, read with no administrator rights.
//!
//! ## What the rules let in, and where
//!
//! Three inbound allow rules for Nexus's own executable (its real path), on **Private AND
//! Public** networks: a club site's network is as often one Windows calls Public as one it calls
//! Private, and an allow on Private only kept such a site out (the operator's ruling,
//! 2026-10-10). Many sites run with the firewall off, and nothing in club sync depends on it
//! being on: with it off for both, there is nothing to allow, and the section says so.
//! - [`HOST_RULE`]: TCP on the Host port as set (42073 by default), what a joining laptop
//!   connects to.
//! - [`SEARCH_RULE`]: UDP [`BEACON_PORT`], where Find club events hears a host's beacon.
//! - [`BOARD_RULE`]: TCP on the club TV scoreboard's port (7373 by default), which serves on
//!   every network this laptop is on (`0.0.0.0`) while it is switched on.
//!
//! ## The command
//!
//! One press is one administrator prompt (`tempo_core::process::run_elevated`, the elevation
//! **Repair clock** asks through): Windows' own `netsh`, by its absolute path, first deleting
//! every inbound rule under Nexus's three names, so that a second press, or a changed port,
//! replaces Nexus's rules and touches nothing else, then adding the three.
//!
//! ⛔ **Nothing but fixed words, two port numbers and the executable's path reaches the command
//! line.** The ports are `u16`s and never 0. The path is the one input with characters of its
//! own, so it is checked before anything is built ([`allow_args`]): `cmd.exe` expands `%` even
//! inside quotes, and a `"` or a control character would end the quoted path, so a path holding
//! one is refused, never escaped. Everything else a Windows path may hold (a space, `&`, `(`,
//! `)`, `^`, `'`, `$`, `!`) is literal inside the quotes: `/v:off` keeps `!` literal whatever the
//! registry says, and `/d` keeps a `cmd.exe` AutoRun entry out of an elevated run. How the text
//! survives PowerShell on its way to `cmd.exe` is `run_elevated`'s, tested there.
//!
//! ## Reading needs no administrator rights
//!
//! [`read`] asks Windows' firewall policy (`INetFwPolicy2`) whether the firewall is on for
//! Private and Public networks, and for the inbound rules that are Nexus's (by name) or name this
//! program; [`status`] puts that in the club section's terms. Windows' own rules for Nexus count
//! too: "Cancel" at Windows' question, or any answer from an account without administrator
//! rights, leaves rules that BLOCK Nexus, and a block wins over every allow, so the section says
//! so rather than calling Nexus's rules enough. Those rules are Windows', and the button leaves
//! them alone.
//!
//! ⚠️ **NEEDS-BENCH.** Nothing here has run on Windows. What is tested is the command text
//! ([`allow_args`], [`remove_args`]) and the reading's meaning ([`status`]); the reader itself is
//! type-checked for Windows and runs nowhere else ([`read`] is `None` off Windows).

use crate::fdsync::BEACON_PORT;
use serde::Serialize;

/// The rule for the Host port: what a joining laptop connects to.
pub const HOST_RULE: &str = "Nexus club sync - host (TCP)";
/// The rule for Find club events: the host's beacon, heard on [`BEACON_PORT`].
pub const SEARCH_RULE: &str = "Nexus club sync - finding events (UDP)";
/// The rule for the club TV scoreboard's port.
pub const BOARD_RULE: &str = "Nexus club sync - TV scoreboard (TCP)";

/// Windows' own `netsh`, by its absolute path: the elevated `cmd.exe` searches its current
/// directory before `PATH`.
const NETSH: &str = r"C:\Windows\System32\netsh.exe";

/// IP protocol numbers as a rule holds them.
const TCP: i32 = 6;
const UDP: i32 = 17;
const ANY: i32 = 256;
/// Profile bits as a rule holds them (`NET_FW_PROFILE_TYPE2`).
const PRIVATE: i32 = 2;
const PUBLIC: i32 = 4;

/// Is this one of Nexus's three rule names?
fn is_ours(name: &str) -> bool {
    [HOST_RULE, SEARCH_RULE, BOARD_RULE].contains(&name)
}

/// One of Nexus's rules: its name, TCP or UDP, and its port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSpec {
    pub name: &'static str,
    pub tcp: bool,
    pub port: u16,
}

/// The three rules for a Host port and a scoreboard port.
pub fn rules(host_port: u16, board_port: u16) -> [RuleSpec; 3] {
    [
        RuleSpec {
            name: HOST_RULE,
            tcp: true,
            port: host_port,
        },
        RuleSpec {
            name: SEARCH_RULE,
            tcp: false,
            port: BEACON_PORT,
        },
        RuleSpec {
            name: BOARD_RULE,
            tcp: true,
            port: board_port,
        },
    ]
}

/// Why a press cannot build its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The executable's path is not one the command can quote: not an absolute `….exe` path,
    /// or holding a `%`, a `"` or a control character.
    Path,
    /// A port of 0.
    Port,
}

impl Refusal {
    /// The code the screens switch on.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Path => "path",
            Refusal::Port => "port",
        }
    }
}

/// `cmd.exe`'s arguments for **Allow Nexus through Windows Firewall**: delete every inbound rule
/// under Nexus's three names, then add the three for `exe` on Private and Public networks.
pub fn allow_args(exe: &str, host_port: u16, board_port: u16) -> Result<String, Refusal> {
    if !quotable(exe) {
        return Err(Refusal::Path);
    }
    if host_port == 0 || board_port == 0 {
        return Err(Refusal::Port);
    }
    // `&&`: a rule that fails to go in stops the rest, and the press's status is its failure.
    let adds = rules(host_port, board_port)
        .iter()
        .map(|rule| {
            format!(
                "{NETSH} advfirewall firewall add rule name=\"{name}\" dir=in action=allow \
                 program=\"{exe}\" protocol={protocol} localport={port} profile=private,public \
                 enable=yes",
                name = rule.name,
                protocol = if rule.tcp { "TCP" } else { "UDP" },
                port = rule.port,
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(format!("/d /v:off /c {} & {adds}", deletes()))
}

/// `cmd.exe`'s arguments for removing Nexus's rules: every inbound rule under its three names.
pub fn remove_args() -> String {
    format!("/d /v:off /c {}", deletes())
}

/// Delete every inbound rule under each of Nexus's three names. `&`: a name with no rule yet
/// (`netsh` fails it) does not stop the next.
fn deletes() -> String {
    [HOST_RULE, SEARCH_RULE, BOARD_RULE]
        .iter()
        .map(|name| format!("{NETSH} advfirewall firewall delete rule name=\"{name}\" dir=in"))
        .collect::<Vec<_>>()
        .join(" & ")
}

/// Can `exe` go into the command between double quotes as it is? An absolute path to an `.exe`
/// on a drive (`C:\…`) or a share (`\\server\…`, not the `\\?\` or `\\.\` device forms Windows'
/// firewall does not name programs by), with no `%` (`cmd.exe` expands it even inside quotes), no
/// `"` and no control character (either would end the quoted path).
fn quotable(exe: &str) -> bool {
    let b = exe.as_bytes();
    let drive = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    let share = exe.starts_with(r"\\") && !exe.starts_with(r"\\?\") && !exe.starts_with(r"\\.\");
    let tail = exe.len().saturating_sub(4);
    let dot_exe = exe.is_char_boundary(tail) && exe[tail..].eq_ignore_ascii_case(".exe");
    (drive || share) && dot_exe && !exe.chars().any(|c| c == '%' || c == '"' || c.is_control())
}

/// The firewall on one kind of network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileReading {
    /// The firewall is on there.
    pub on: bool,
    /// It blocks every incoming connection there, whatever the rules allow.
    pub blocks_all: bool,
}

/// One inbound rule as [`read`] found it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleReading {
    pub name: String,
    /// The program it names, as Windows hands it back ("" for none).
    pub program: String,
    /// It allows; otherwise it blocks.
    pub allow: bool,
    pub enabled: bool,
    /// The profiles it is in force on, as bits: 1 Domain, 2 Private, 4 Public.
    pub profiles: i32,
    /// 6 TCP, 17 UDP, 256 any.
    pub protocol: i32,
    /// Its local ports as Windows writes them ("42073", "*").
    pub local_ports: String,
}

/// Windows' firewall as the club section reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub private: ProfileReading,
    pub public: ProfileReading,
    /// Every inbound rule that is Nexus's (by name) or names this program.
    pub rules: Vec<RuleReading>,
}

/// Where Nexus's rules stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// The firewall is off for Private and Public networks: there is nothing to allow.
    Off,
    /// None of Nexus's rules is there.
    Missing,
    /// Some are there, but not all three as they would be made now: another port, another copy
    /// of Nexus, a profile or a rule switched off. A press replaces them.
    Stale,
    /// All three are there as they would be made now.
    InPlace,
}

/// What the club section says about the firewall ([`status`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: State,
    /// A rule of Nexus's is there (whatever its state), so there is something to remove.
    pub ours: bool,
    /// Windows has a rule of its own blocking this program on Private networks, with the
    /// firewall on there: it wins over every allow.
    pub blocked_private: bool,
    /// The same, on Public networks.
    pub blocked_public: bool,
    /// The firewall blocks every incoming connection on Private networks.
    pub blocks_all_private: bool,
    /// The same, on Public networks.
    pub blocks_all_public: bool,
    /// The ports Nexus's rules are for: the Host port, Find club events', the scoreboard's.
    pub host_port: u16,
    pub search_port: u16,
    pub board_port: u16,
}

/// What `reading` means for club sync on this laptop, for the executable `exe` and the ports as
/// set.
pub fn status(reading: &Reading, exe: &str, host_port: u16, board_port: u16) -> Status {
    // Windows compares a rule's program to the running one in any case.
    let this_program = |rule: &RuleReading| rule.program.eq_ignore_ascii_case(exe);
    // Each of the three, as a press would make it now: allowing, switched on, for this program,
    // on its protocol and port, on Private and Public.
    let in_place = rules(host_port, board_port).iter().all(|spec| {
        reading.rules.iter().any(|rule| {
            rule.name == spec.name
                && rule.allow
                && rule.enabled
                && this_program(rule)
                && rule.protocol == if spec.tcp { TCP } else { UDP }
                && rule.local_ports == spec.port.to_string()
                && rule.profiles & (PRIVATE | PUBLIC) == PRIVATE | PUBLIC
        })
    });
    let ours = reading.rules.iter().any(|rule| is_ours(&rule.name));
    // A block for this program wins over every allow, whatever port it names.
    let blocked = |profile: &ProfileReading, bit: i32| {
        profile.on
            && reading.rules.iter().any(|rule| {
                !rule.allow
                    && rule.enabled
                    && this_program(rule)
                    && rule.profiles & bit != 0
                    && matches!(rule.protocol, TCP | UDP | ANY)
            })
    };
    let state = if !reading.private.on && !reading.public.on {
        State::Off
    } else if in_place {
        State::InPlace
    } else if ours {
        State::Stale
    } else {
        State::Missing
    };
    Status {
        state,
        ours,
        blocked_private: blocked(&reading.private, PRIVATE),
        blocked_public: blocked(&reading.public, PUBLIC),
        blocks_all_private: reading.private.on && reading.private.blocks_all,
        blocks_all_public: reading.public.on && reading.public.blocks_all,
        host_port,
        search_port: BEACON_PORT,
        board_port,
    }
}

/// The firewall as Windows has it now, for the executable `exe`, with no administrator rights.
/// `None` where it cannot be read: off Windows, or when Windows' firewall policy does not answer.
/// Blocks for the time the rules take to read (about a tenth of a second for some 770 rules, as
/// measured for Remote's reader on Windows 11): call it off any thread that must not wait.
pub fn read(exe: &str) -> Option<Reading> {
    #[cfg(windows)]
    {
        system::read(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        None
    }
}

#[cfg(windows)]
mod system {
    use windows::core::Interface;
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NetFwPolicy2, NET_FW_ACTION_ALLOW, NET_FW_PROFILE_TYPE2,
        NET_FW_RULE_DIR_IN,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Ole::IEnumVARIANT;
    use windows::Win32::System::Variant::{VARIANT, VT_DISPATCH};

    use super::{is_ours, ProfileReading, Reading, RuleReading, PRIVATE, PUBLIC};

    pub fn read(program: &str) -> Option<Reading> {
        // SAFETY: COM on this thread for the reads below, undone when it was done here (a thread
        // already in another apartment keeps it, and COM still works there).
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let read = policy(program);
        if init.is_ok() {
            // SAFETY: balances the initialisation above.
            unsafe { CoUninitialize() };
        }
        read
    }

    /// The firewall's state on Private and Public networks, and the rules that matter here.
    fn policy(program: &str) -> Option<Reading> {
        let private = NET_FW_PROFILE_TYPE2(PRIVATE);
        let public = NET_FW_PROFILE_TYPE2(PUBLIC);
        // SAFETY: COM calls on interfaces COM handed back, each released as it goes.
        unsafe {
            let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL).ok()?;
            Some(Reading {
                private: ProfileReading {
                    on: policy.get_FirewallEnabled(private).ok()?.as_bool(),
                    blocks_all: policy.get_BlockAllInboundTraffic(private).ok()?.as_bool(),
                },
                public: ProfileReading {
                    on: policy.get_FirewallEnabled(public).ok()?.as_bool(),
                    blocks_all: policy.get_BlockAllInboundTraffic(public).ok()?.as_bool(),
                },
                rules: rules(&policy, program)?,
            })
        }
    }

    /// The inbound rules that are Nexus's (by name) or whose program is `program` (the path, in
    /// any case).
    ///
    /// # Safety
    /// `policy` is a live firewall policy object on this thread's apartment.
    unsafe fn rules(policy: &INetFwPolicy2, program: &str) -> Option<Vec<RuleReading>> {
        // SAFETY: as the caller promises.
        let each: IEnumVARIANT = unsafe { policy.Rules().ok()?._NewEnum().ok()?.cast().ok()? };
        let mut found = Vec::new();
        loop {
            let mut one = [VARIANT::default()];
            let mut fetched = 0;
            // SAFETY: room for one, and a count to say whether it came.
            let _ = unsafe { each.Next(&mut one, &mut fetched) };
            if fetched == 0 {
                return Some(found);
            }
            if one[0].vt() != VT_DISPATCH {
                continue;
            }
            // SAFETY: a VT_DISPATCH variant holds an IDispatch, released when `one` is.
            let rule = unsafe { one[0].Anonymous.Anonymous.Anonymous.pdispVal.as_ref() }
                .and_then(|rule| rule.cast::<INetFwRule>().ok());
            let Some(rule) = rule else {
                continue;
            };
            // SAFETY: property reads on a live rule.
            unsafe {
                if rule.Direction().ok() != Some(NET_FW_RULE_DIR_IN) {
                    continue;
                }
                let name = rule.Name().map(|n| n.to_string()).unwrap_or_default();
                let app = rule
                    .ApplicationName()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                if !is_ours(&name) && !app.eq_ignore_ascii_case(program) {
                    continue;
                }
                found.push(RuleReading {
                    name,
                    program: app,
                    allow: rule.Action().ok() == Some(NET_FW_ACTION_ALLOW),
                    enabled: rule.Enabled().is_ok_and(|enabled| enabled.as_bool()),
                    profiles: rule.Profiles().unwrap_or(0),
                    protocol: rule.Protocol().unwrap_or(0),
                    local_ports: rule.LocalPorts().map(|p| p.to_string()).unwrap_or_default(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A per-user install's path, with the space a Windows account name often has.
    const EXE: &str = r"C:\Users\Ham Op\AppData\Local\Nexus\Nexus.exe";

    /// ⭐ **THE EXACT COMMAND, for a path with a space.** Fixed words, the two ports and the
    /// quoted path, and nothing else: first every rule under Nexus's three names goes (`&`, so a
    /// name with no rule yet does not stop the rest), then the three are added (`&&`, so a
    /// failed one stops the press and its status says so).
    #[test]
    fn the_allow_command_is_fixed_words_the_ports_and_the_quoted_path() {
        assert_eq!(
            allow_args(EXE, 42073, 7373).unwrap(),
            concat!(
                r#"/d /v:off /c "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - host (TCP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - finding events (UDP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - TV scoreboard (TCP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - host (TCP)" dir=in action=allow program="C:\Users\Ham Op\AppData\Local\Nexus\Nexus.exe" protocol=TCP localport=42073 profile=private,public enable=yes && "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - finding events (UDP)" dir=in action=allow program="C:\Users\Ham Op\AppData\Local\Nexus\Nexus.exe" protocol=UDP localport=42074 profile=private,public enable=yes && "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall add rule name="Nexus club sync - TV scoreboard (TCP)" dir=in action=allow program="C:\Users\Ham Op\AppData\Local\Nexus\Nexus.exe" protocol=TCP localport=7373 profile=private,public enable=yes"#,
            )
        );
    }

    /// A changed Host port is a press like any other: the same three names go first, so the
    /// old port's rule is replaced, and the new port is the one added.
    #[test]
    fn a_changed_host_port_replaces_the_rule_under_the_same_name() {
        let args = allow_args(EXE, 50000, 7474).unwrap();
        let (deletes, adds) = args
            .split_once(" & C:\\Windows\\System32\\netsh.exe advfirewall firewall add ")
            .unwrap();
        assert_eq!(deletes.matches("delete rule name=").count(), 3, "{deletes}");
        assert!(
            !deletes.contains("localport"),
            "a delete is by name alone: {deletes}"
        );
        assert!(
            adds.contains(r#"name="Nexus club sync - host (TCP)" dir=in action=allow"#),
            "{adds}"
        );
        assert!(adds.contains("protocol=TCP localport=50000 "), "{adds}");
        assert!(adds.contains("protocol=TCP localport=7474 "), "{adds}");
        assert!(adds.contains("protocol=UDP localport=42074 "), "{adds}");
        assert!(!args.contains("42073"), "{args}");
    }

    /// Everything else a Windows folder name may hold is literal inside the quotes, so it goes
    /// into the command as it is.
    #[test]
    fn a_path_cmd_would_read_outside_quotes_goes_in_as_it_is() {
        let exe = r"C:\Users\O'Brien & Co (Field) ^$!\AppData\Local\Nexus\Nexus.exe";
        let args = allow_args(exe, 42073, 7373).unwrap();
        assert_eq!(
            args.matches(&format!(r#" program="{exe}" "#)).count(),
            3,
            "{args}"
        );
        // A path on another drive, or a share, is a path like any other.
        assert!(allow_args(r"D:\Nexus\Nexus.EXE", 42073, 7373).is_ok());
        assert!(allow_args(r"\\club-nas\apps\Nexus\Nexus.exe", 42073, 7373).is_ok());
    }

    /// ⛔ A path the command cannot quote is refused, never escaped; so is a port of 0.
    #[test]
    fn a_path_the_command_cannot_quote_is_refused() {
        for exe in [
            r"C:\Users\100%\AppData\Local\Nexus\Nexus.exe",
            r"C:\Users\%USERNAME%\Nexus.exe",
            "C:\\Users\\a\"b\\Nexus.exe",
            "C:\\Users\\a\nb\\Nexus.exe",
            "C:\\Users\\a\u{7}b\\Nexus.exe",
            r"Nexus.exe",
            r"Users\Nexus.exe",
            r"C:\Users\Ham\Nexus.bat",
            r"\\?\C:\Users\Ham\Nexus.exe",
            r"\\.\C:\Users\Ham\Nexus.exe",
            "",
        ] {
            assert_eq!(allow_args(exe, 42073, 7373), Err(Refusal::Path), "{exe:?}");
        }
        assert_eq!(allow_args(EXE, 0, 7373), Err(Refusal::Port));
        assert_eq!(allow_args(EXE, 42073, 0), Err(Refusal::Port));
        // CONTROL: the same path without the character is taken.
        assert!(allow_args(r"C:\Users\100\AppData\Local\Nexus\Nexus.exe", 42073, 7373).is_ok());
    }

    /// The remove command: every inbound rule under Nexus's three names, and nothing else.
    #[test]
    fn the_remove_command_deletes_nexuss_three_names_and_nothing_else() {
        assert_eq!(
            remove_args(),
            concat!(
                r#"/d /v:off /c "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - host (TCP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - finding events (UDP)" dir=in & "#,
                r#"C:\Windows\System32\netsh.exe advfirewall firewall delete rule name="Nexus club sync - TV scoreboard (TCP)" dir=in"#,
            )
        );
    }

    fn ours(name: &str, tcp: bool, port: u16) -> RuleReading {
        RuleReading {
            name: name.into(),
            program: EXE.into(),
            allow: true,
            enabled: true,
            profiles: PRIVATE | PUBLIC,
            protocol: if tcp { TCP } else { UDP },
            local_ports: port.to_string(),
        }
    }

    fn all_three() -> Vec<RuleReading> {
        vec![
            ours(HOST_RULE, true, 42073),
            ours(SEARCH_RULE, false, 42074),
            ours(BOARD_RULE, true, 7373),
        ]
    }

    fn on(rules: Vec<RuleReading>) -> Reading {
        let on = ProfileReading {
            on: true,
            blocks_all: false,
        };
        Reading {
            private: on,
            public: on,
            rules,
        }
    }

    /// ⭐ **Where Nexus's rules stand, each state by value.** CONTROL in each row: the reading
    /// with the one difference removed reads as in place.
    #[test]
    fn the_status_says_where_nexuss_rules_stand() {
        let in_place = status(&on(all_three()), EXE, 42073, 7373);
        assert_eq!(
            in_place,
            Status {
                state: State::InPlace,
                ours: true,
                blocked_private: false,
                blocked_public: false,
                blocks_all_private: false,
                blocks_all_public: false,
                host_port: 42073,
                search_port: 42074,
                board_port: 7373,
            }
        );
        // The program's path is compared as Windows compares it, in any case.
        let mut upper = all_three();
        for rule in &mut upper {
            rule.program = EXE.to_uppercase();
        }
        assert_eq!(status(&on(upper), EXE, 42073, 7373).state, State::InPlace);

        assert_eq!(status(&on(vec![]), EXE, 42073, 7373).state, State::Missing);
        assert!(!status(&on(vec![]), EXE, 42073, 7373).ours);

        // Another Host port set since the press.
        assert_eq!(
            status(&on(all_three()), EXE, 50000, 7373).state,
            State::Stale
        );
        // Another scoreboard port.
        assert_eq!(
            status(&on(all_three()), EXE, 42073, 7474).state,
            State::Stale
        );
        // Another copy of Nexus (moved, or reinstalled elsewhere).
        assert_eq!(
            status(&on(all_three()), r"D:\Nexus\Nexus.exe", 42073, 7373).state,
            State::Stale
        );
        // One of the three missing.
        let mut two = all_three();
        two.pop();
        let stale = status(&on(two), EXE, 42073, 7373);
        assert_eq!((stale.state, stale.ours), (State::Stale, true));
        // Public dropped from one, a rule switched off, a block under Nexus's name, UDP for TCP.
        for change in [
            (|r: &mut RuleReading| r.profiles = PRIVATE) as fn(&mut RuleReading),
            |r| r.enabled = false,
            |r| r.allow = false,
            |r| r.protocol = UDP,
        ] {
            let mut rules = all_three();
            change(&mut rules[0]);
            assert_eq!(status(&on(rules), EXE, 42073, 7373).state, State::Stale);
        }
        // Every profile, Domain included, covers Private and Public.
        let mut everywhere = all_three();
        for rule in &mut everywhere {
            rule.profiles = 0x7fff_ffff;
        }
        assert_eq!(
            status(&on(everywhere), EXE, 42073, 7373).state,
            State::InPlace
        );
    }

    /// ⭐ **Nothing depends on the firewall being on**: off for Private and Public, there is
    /// nothing to allow, whatever rules are there (and Nexus's can still be removed). On for
    /// either one, the rules matter.
    #[test]
    fn with_the_firewall_off_there_is_nothing_to_allow() {
        let off = ProfileReading::default();
        let reading = Reading {
            private: off,
            public: off,
            rules: all_three(),
        };
        let st = status(&reading, EXE, 42073, 7373);
        assert_eq!((st.state, st.ours), (State::Off, true));
        let none = Reading {
            rules: vec![],
            ..reading.clone()
        };
        assert_eq!(status(&none, EXE, 42073, 7373).state, State::Off);
        // A firewall that is off blocks nothing, whatever it is set to block when on.
        let set_to_block = ProfileReading {
            on: false,
            blocks_all: true,
        };
        let idle = status(
            &Reading {
                private: set_to_block,
                public: set_to_block,
                rules: vec![],
            },
            EXE,
            42073,
            7373,
        );
        assert_eq!(
            (idle.state, idle.blocks_all_private, idle.blocks_all_public),
            (State::Off, false, false)
        );
        // CONTROL: on for Public alone, the missing rules are missing.
        let public_on = Reading {
            public: ProfileReading {
                on: true,
                blocks_all: false,
            },
            ..none
        };
        assert_eq!(status(&public_on, EXE, 42073, 7373).state, State::Missing);
    }

    /// ⭐ **Windows' own block rules, and blocking everything, are said, not hidden.** "Cancel" at
    /// Windows' question leaves a block rule for TCP and one for UDP, and a block wins over every
    /// allow: Nexus's rules in place do not let the club in on that kind of network. Said only
    /// where the firewall is on.
    #[test]
    fn a_block_from_windows_own_question_is_said() {
        let block = |profiles: i32, protocol: i32| RuleReading {
            name: "Nexus".into(),
            program: EXE.into(),
            allow: false,
            enabled: true,
            profiles,
            protocol,
            local_ports: "*".into(),
        };
        let mut rules = all_three();
        rules.push(block(PUBLIC, TCP));
        rules.push(block(PUBLIC, UDP));
        let st = status(&on(rules), EXE, 42073, 7373);
        assert_eq!(
            (st.state, st.blocked_private, st.blocked_public),
            (State::InPlace, false, true)
        );
        let any = status(&on(vec![block(PRIVATE | PUBLIC, ANY)]), EXE, 42073, 7373);
        assert_eq!(
            (any.state, any.blocked_private, any.blocked_public),
            (State::Missing, true, true)
        );
        // CONTROL: switched off, for another program, or on a kind of network whose firewall is
        // off, a block rule blocks nothing here.
        let mut off_rule = block(PUBLIC, TCP);
        off_rule.enabled = false;
        let mut other = block(PUBLIC, TCP);
        other.program = r"C:\Other\Other.exe".into();
        assert!(!status(&on(vec![off_rule, other]), EXE, 42073, 7373).blocked_public);
        let public_off = Reading {
            public: ProfileReading::default(),
            ..on(vec![block(PUBLIC, TCP)])
        };
        assert!(!status(&public_off, EXE, 42073, 7373).blocked_public);

        let blocks_all = Reading {
            public: ProfileReading {
                on: true,
                blocks_all: true,
            },
            ..on(all_three())
        };
        let st = status(&blocks_all, EXE, 42073, 7373);
        assert_eq!(
            (st.state, st.blocks_all_private, st.blocks_all_public),
            (State::InPlace, false, true)
        );
    }

    /// The rules for a Host port and a scoreboard port: names, protocols and ports by value.
    #[test]
    fn the_three_rules_are_the_host_port_the_search_and_the_scoreboard() {
        assert_eq!(
            rules(42073, 7373),
            [
                RuleSpec {
                    name: "Nexus club sync - host (TCP)",
                    tcp: true,
                    port: 42073
                },
                RuleSpec {
                    name: "Nexus club sync - finding events (UDP)",
                    tcp: false,
                    port: 42074
                },
                RuleSpec {
                    name: "Nexus club sync - TV scoreboard (TCP)",
                    tcp: true,
                    port: 7373
                },
            ]
        );
    }

    /// Off Windows there is no firewall to read.
    #[cfg(not(windows))]
    #[test]
    fn off_windows_nothing_is_read() {
        assert_eq!(read(EXE), None);
    }
}
