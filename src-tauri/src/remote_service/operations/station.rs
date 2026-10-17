//! The station actions a browser may still send: the FT gestures, each under a transmit permit,
//! and the held FT contact's log actions, under the logging grant. They use the same Engine verbs
//! as the local cockpit.

use serde::{Deserialize, Serialize};
use tempo_app::{
    engine::Engine,
    remote_control::{Completion, Evidence, Outcome, Reason},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    radio_id: u32,
    radio_connection: Option<u64>,
    amp_connection: Option<u64>,
    amp_read_sequence: Option<u64>,
}

impl Context {
    pub fn capture(engine: &Engine) -> Self {
        let observation = engine.remote_monitor_observation();
        let amp = observation.amplifier.as_ref().and_then(|a| a.reading);
        Self {
            radio_id: observation.radio.id,
            radio_connection: observation
                .radio
                .readings
                .cat
                .map(|r| r.connection_generation),
            amp_connection: amp.map(|r| r.connection_generation),
            amp_read_sequence: amp.map(|r| r.read_sequence),
        }
    }

    fn matches_radio(&self, engine: &Engine) -> bool {
        self.radio_id == engine.settings().active_radio
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Action {
    #[serde(rename = "ft.runtime")]
    FtRuntime {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        expected: tempo_app::engine::remote_transmit::runtime::FtRuntimeContext,
        change: tempo_app::engine::remote_transmit::runtime::FtRuntimeChange,
    },
    #[serde(rename = "ft.setting")]
    FtSetting {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        expected: tempo_app::engine::remote_transmit::settings::FtSettingsContext,
        change: tempo_app::engine::remote_transmit::settings::FtSettingChange,
    },
    #[serde(rename = "qso.logCurrent")]
    QsoLogCurrent {
        #[serde(rename = "expectedKey")]
        expected_key: String,
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
    },
    #[serde(rename = "qso.confirm")]
    QsoConfirm {
        #[serde(rename = "expectedKey")]
        expected_key: String,
        edits: tempo_app::engine::remote_logging::PendingLogEdits,
    },
    #[serde(rename = "qso.discard")]
    QsoDiscard {
        #[serde(rename = "expectedKey")]
        expected_key: String,
    },
    #[serde(rename = "ft.message")]
    FtMessage {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
        call: String,
        grid: Option<String>,
        text: String,
    },
    #[serde(rename = "ft.exchange")]
    FtExchange {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        #[serde(rename = "expectedQso")]
        expected_qso: tempo_app::engine::remote_transmit::FtExchangeContext,
        change: tempo_app::engine::remote_transmit::FtExchangeChange,
    },
    #[serde(rename = "ft.cq")]
    FtCq {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        direction: Option<String>,
    },
    #[serde(rename = "ft.call")]
    FtCall {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        selection: tempo_app::engine::remote_transmit::FtCallSelection,
    },
    #[serde(rename = "ft.txEnabled")]
    FtTxEnabled {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
        on: bool,
    },
}

impl Action {
    pub fn is_logging(&self) -> bool {
        matches!(
            self,
            Self::QsoLogCurrent { .. } | Self::QsoConfirm { .. } | Self::QsoDiscard { .. }
        )
    }

    /// Every action left is an operation v4 one.
    pub fn minimum_version(&self) -> u8 {
        4
    }
}

impl Action {
    pub fn transmit_epoch(&self) -> Option<&str> {
        match self {
            Self::FtCq { transmit_epoch, .. }
            | Self::FtTxEnabled { transmit_epoch, .. }
            | Self::FtCall { transmit_epoch, .. }
            | Self::FtExchange { transmit_epoch, .. }
            | Self::FtSetting { transmit_epoch, .. }
            | Self::FtRuntime { transmit_epoch, .. }
            | Self::FtMessage { transmit_epoch, .. } => Some(transmit_epoch),
            _ => None,
        }
    }
}

pub fn execute_transmit(
    engine: &mut Engine,
    context: &Context,
    action: &Action,
    permit: tempo_app::remote_control::transmit::TransmitPermit,
) -> Result<Completion, Reason> {
    if !context.matches_radio(engine) {
        return Err(Reason::ContextChanged);
    }
    match action {
        Action::FtRuntime {
            expected_tier,
            expected,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_runtime(&permit, expected, change)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: if matches!(
                    change,
                    tempo_app::engine::remote_transmit::runtime::FtRuntimeChange::RxOffset { .. }
                ) {
                    Evidence::SettingsSaved
                } else {
                    Evidence::StationState
                },
            });
            return Ok(result);
        }
        Action::FtSetting {
            expected_tier,
            expected,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_setting(&permit, expected, change)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: if matches!(
                    change,
                    tempo_app::engine::remote_transmit::settings::FtSettingChange::Auto { .. }
                ) {
                    Evidence::StationState
                } else {
                    Evidence::SettingsSaved
                },
            });
            return Ok(result);
        }
        Action::FtMessage {
            expected_tier,
            expected_qso,
            call,
            grid,
            text,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.set_remote_ft_message(permit, expected_qso, call, grid.as_deref(), text)?;
        }
        Action::FtExchange {
            expected_tier,
            expected_qso,
            change,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.change_remote_ft_exchange(permit, expected_qso, change)?;
        }
        Action::FtCq {
            expected_tier,
            direction,
            ..
        } => {
            if let Some(direction) = direction {
                let letters = (1..=4).contains(&direction.len())
                    && direction.bytes().all(|b| b.is_ascii_uppercase());
                let digits = direction.len() == 3 && direction.bytes().all(|b| b.is_ascii_digit());
                if !letters && !digits {
                    return Err(Reason::InvalidAction);
                }
            }
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.start_remote_ft_cq(permit, direction.as_deref())?;
        }
        Action::FtCall {
            expected_tier,
            selection,
            ..
        } => {
            engine.validate_remote_ft_radio(
                *expected_tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            engine.call_remote_ft_selection(permit, selection)?;
        }
        Action::FtTxEnabled {
            expected_tier, on, ..
        } => {
            if engine.tier() != *expected_tier {
                return Err(Reason::ContextChanged);
            }
            if *on {
                engine.validate_remote_ft_radio(
                    *expected_tier,
                    context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                    &permit,
                )?;
            }
            engine.set_remote_ft_tx_enabled(permit, *on)?;
        }
        _ => return Err(Reason::UnsupportedAction),
    }
    let result = Completion::default();
    result.finish(Outcome::Applied {
        evidence: Evidence::StationState,
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE SAFETY PIN UNDER OPTIMISTIC TUNING. The browser is allowed to draw a dial the radio has
    /// not reached yet, and the first argument for why that is safe is this: **the whole remote
    /// transmit surface is FT-sequencer arming.** There is no remote PTT, no remote CW key and no
    /// remote Tune, so no browser-side number can ever become a keying decision — the engine keys
    /// through its own gates against its own CAT readback.
    ///
    /// That argument is only as good as the code, so it is pinned here rather than written down:
    /// every `Action` carrying a `transmit_epoch` is an `Ft*` action, and every `Ft*` action
    /// carries one. The day someone adds a remote PTT, this fails at that commit.
    ///
    /// It cannot be skipped by accident. `ordinal` matches EXHAUSTIVELY — no wildcard arm — so a
    /// new variant does not compile until it is listed, and the test then demands a dense set of
    /// ordinals, so the author must also add an instance to `every_action` (a gap fails, a reused
    /// number fails, a number past the end fails). Whatever they do, the assertion runs on it.
    fn ordinal(action: &Action) -> (usize, &'static str) {
        match action {
            Action::FtRuntime { .. } => (0, "FtRuntime"),
            Action::FtSetting { .. } => (1, "FtSetting"),
            Action::QsoLogCurrent { .. } => (2, "QsoLogCurrent"),
            Action::QsoConfirm { .. } => (3, "QsoConfirm"),
            Action::QsoDiscard { .. } => (4, "QsoDiscard"),
            Action::FtMessage { .. } => (5, "FtMessage"),
            Action::FtExchange { .. } => (6, "FtExchange"),
            Action::FtCq { .. } => (7, "FtCq"),
            Action::FtCall { .. } => (8, "FtCall"),
            Action::FtTxEnabled { .. } => (9, "FtTxEnabled"),
        }
    }

    /// One of every variant. The field VALUES are placeholders and mean nothing — `transmit_epoch`
    /// is read for its presence, never its content.
    fn every_action() -> Vec<Action> {
        use tempo_app::dto::Tier;
        use tempo_app::engine::remote_logging::PendingLogEdits;
        use tempo_app::engine::remote_transmit::runtime::{FtRuntimeChange, FtRuntimeContext};
        use tempo_app::engine::remote_transmit::settings::{FtSettingChange, FtSettingsContext};
        use tempo_app::engine::remote_transmit::{
            FtCallSelection, FtExchangeChange, FtExchangeContext,
        };

        let ft_settings = || FtSettingsContext {
            key: "x".into(),
            tx_offset_hz: 0.0,
            rx_offset_hz: 0.0,
            hold_tx_freq: false,
            tx_even: false,
            tx_cycle_auto: false,
        };
        let ft_qso = || FtExchangeContext {
            dxcall: None,
            state: "x".into(),
            tx_now: None,
            cq_running: false,
        };
        vec![
            Action::FtRuntime {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected: FtRuntimeContext {
                    settings: ft_settings(),
                    skip_tx1: false,
                },
                change: FtRuntimeChange::RxOffset { hz: 0.0 },
            },
            Action::FtSetting {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected: ft_settings(),
                change: FtSettingChange::TxOffset { hz: 0.0 },
            },
            Action::QsoLogCurrent {
                expected_key: "x".into(),
                expected_tier: Tier::Ft8,
                expected_qso: ft_qso(),
            },
            Action::QsoConfirm {
                expected_key: "x".into(),
                edits: PendingLogEdits {
                    call: "x".into(),
                    grid: None,
                    rst_sent: None,
                    rst_rcvd: None,
                },
            },
            Action::QsoDiscard {
                expected_key: "x".into(),
            },
            Action::FtMessage {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected_qso: ft_qso(),
                call: "x".into(),
                grid: None,
                text: "x".into(),
            },
            Action::FtExchange {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                expected_qso: ft_qso(),
                change: FtExchangeChange::Resend,
            },
            Action::FtCq {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                direction: None,
            },
            Action::FtCall {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                selection: FtCallSelection {
                    call: "x".into(),
                    grid: None,
                    message: None,
                    snr: None,
                    freq: None,
                },
            },
            Action::FtTxEnabled {
                expected_tier: Tier::Ft8,
                transmit_epoch: "2a".into(),
                on: false,
            },
        ]
    }

    /// Every variant name on the `Action` declaration, read out of this file's own source. A
    /// variant is one line at the enum's own indentation beginning with an upper-case name; its
    /// fields are indented deeper and named in lower case, so nothing else can match.
    fn declared_variants() -> Vec<&'static str> {
        let source = include_str!("station.rs");
        let body = source
            .split_once("pub enum Action {")
            .expect("the Action declaration")
            .1;
        let mut names = Vec::new();
        for line in body.lines() {
            if line == "}" {
                break;
            }
            let Some(rest) = line.strip_prefix("    ") else {
                continue;
            };
            if !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                names.push(name);
            }
        }
        names
    }

    #[test]
    fn every_action_carrying_a_transmit_epoch_is_ft_arming() {
        let actions = every_action();
        let mut seen: Vec<Option<&'static str>> = vec![None; actions.len()];
        for action in &actions {
            let (at, name) = ordinal(action);
            assert!(
                at < seen.len(),
                "{name} has ordinal {at} but only {} actions are built: add one to every_action",
                seen.len()
            );
            assert!(
                seen[at].is_none(),
                "{name} reuses ordinal {at}, already taken by {}: every variant needs its own",
                seen[at].unwrap()
            );
            seen[at] = Some(name);
        }
        for (at, name) in seen.iter().enumerate() {
            assert!(
                name.is_some(),
                "no action was built for ordinal {at}: every_action is missing a variant"
            );
        }
        // AND THE VARIANTS THEMSELVES, READ OFF THE DECLARATION. The exhaustive match alone does
        // not close this: an author who adds `Ptt`, gives it the next free ordinal and builds no
        // instance compiles and passes, because nothing ever calls `ordinal` on a variant that has
        // no instance. (Found by running exactly that mutation against the first version of this
        // test, which went green.) So the enum's own declaration is the roll call, and every name
        // on it has to have answered above.
        let declared = declared_variants();
        assert!(
            declared.len() >= 10 && declared.contains(&"FtCq") && declared.contains(&"QsoDiscard"),
            "the Action declaration was not parsed: {} name(s) found",
            declared.len()
        );
        let built: Vec<&str> = seen.iter().map(|n| n.unwrap()).collect();
        for name in &declared {
            assert!(
                built.contains(name),
                "{name} is declared on Action but every_action builds no instance of it, so the \
                 transmit-epoch check below never ran against it"
            );
        }
        assert_eq!(
            declared.len(),
            built.len(),
            "declared vs built action variants"
        );
        for action in &actions {
            let (_, name) = ordinal(action);
            assert_eq!(
                action.transmit_epoch().is_some(),
                name.starts_with("Ft"),
                "{name}: the remote transmit surface is FT arming only. An action that carries a \
                 transmit epoch can key the radio, and every one of them must be an Ft* action — \
                 adding a remote PTT, CW key or Tune breaks the safety case that lets the browser \
                 show an optimistic dial at all. Re-read that case before changing this test."
            );
        }
    }
}
