//! Each contest's own FT8/FT4 messages, by value.
//!
//! Every contest the picker offers is run through the Contest screen's two roles — Running
//! (`fieldday-run`) and S&P (`fieldday-sp`) — against a scripted partner that answers the way
//! WSJT-X 3.0.2 does in the matching Special Operating Activity (`widgets/mainwindow.cpp`:
//! `genCQMsg` 9466-9531, `genStdMsgs` 9564-9722, `processMessage` 8918-9453, and the Tx1
//! switch-off at 8238-8254). What this station puts on the air is read off the planned over —
//! the same text the modulator encodes — and every over is pushed through the FT8 encoder to
//! name its 77-bit type and the text a receiving station decodes.
//!
//! The partner is a script, not a decoder: it reacts to the message it was addressed and
//! repeats its last over when it hears anything else, which is what WSJT-X's auto-sequencer
//! does with a message it has no arm for.
//!
//! The Operate cockpit's own gestures are driven through the verbs its commands call: a
//! double-click through `work_station` (the `call_station` command), the strip's Call CQ through
//! `start_running`.

use super::*;
use crate::dto::Tier;
use crate::test_util::StoredLog;

const ME: &str = "K1ABC";
const MY_GRID: &str = "FN42";
const DX: &str = "W9XYZ";
const DX_GRID: &str = "EN37";
/// The SNR every one of the partner's overs is decoded at, so the report this station sends
/// it is `-12`.
const DX_SNR: i32 = -12;
/// The report the partner sends this station.
const DX_RPT: &str = "-07";
/// Slots per scene: eight overs each way, enough for the longest exchange and a re-arm.
const SLOTS: u64 = 16;

/// Every contest the picker offers (`ui/src/fdEvent.ts` `CONTESTS`, in its order), after the
/// blank default a fresh install carries.
const CONTESTS: [&str; 19] = [
    "",
    "arrlfd",
    "wfd",
    "arrlss_cw",
    "arrlss_ssb",
    "arrlvhf_jan",
    "arrlvhf_jun",
    "arrlvhf_sep",
    "cqww_cw",
    "cqww_ssb",
    "cqww_rtty",
    "cqwpx_cw",
    "cqwpx_ssb",
    "cqp",
    "ilqp",
    "nyqp",
    "ohqp",
    "tnqp",
    "txqp",
];

/// The WSJT-X 3.0.2 Special Operating Activity a station in this contest runs
/// (`Configuration::SpecialOperatingActivity`). ARRL Field Day has FIELD_DAY and the three
/// ARRL VHF contests have NA_VHF; no other contest on the list has an activity of its own,
/// so the partner there sends the standard messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    FieldDay,
    NaVhf,
    None,
}

fn act_of(contest: &str) -> Act {
    match contest {
        "" | "arrlfd" | "wfd" => Act::FieldDay,
        "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => Act::NaVhf,
        _ => Act::None,
    }
}

/// The partner, as WSJT-X 3.0.2 with auto-sequencing on.
struct Partner {
    act: Act,
    /// What it sends on its next over.
    next: Option<String>,
    /// Its next over is its last: a 73, or a runner's RR73.
    last: bool,
}

impl Partner {
    /// A runner calls CQ from its first over; an S&P station waits for a CQ to answer.
    fn new(act: Act, running: bool) -> Self {
        let next = running.then(|| match act {
            Act::FieldDay => format!("CQ FD {DX} {DX_GRID}"),
            Act::NaVhf => format!("CQ TEST {DX} {DX_GRID}"),
            Act::None => format!("CQ {DX} {DX_GRID}"),
        });
        Self {
            act,
            next,
            last: false,
        }
    }

    /// What WSJT-X does on decoding `mine`.
    fn hear(&mut self, mine: &str) {
        let w: Vec<&str> = mine.split_whitespace().collect();
        let to_me = w.len() >= 3 && w[0] == DX && w[1] == ME;
        let a_cq = w.first() == Some(&"CQ") && w.contains(&ME);
        let is_grid = |s: &str| {
            let b = s.as_bytes();
            s.len() == 4
                && s != "RR73"
                && (b'A'..=b'R').contains(&b[0])
                && (b'A'..=b'R').contains(&b[1])
                && b[2].is_ascii_digit()
                && b[3].is_ascii_digit()
        };
        let is_report =
            |s: &str| (s.starts_with('-') || s.starts_with('+')) && s[1..].parse::<i32>().is_ok();
        let reply = |payload: &str| format!("{ME} {DX} {payload}");
        // Double-clicking a CQ: Tx2 in the contest activities (Tx1 is switched off,
        // mainwindow.cpp 8238-8248), Tx1 with no activity.
        if a_cq && self.next.is_none() && !self.last {
            self.next = Some(match self.act {
                Act::FieldDay => reply("1D WI"),
                Act::NaVhf | Act::None => reply(DX_GRID),
            });
            return;
        }
        if !to_me {
            return;
        }
        let closing = w[2] == "RR73" || w[2] == "RRR";
        let (next, last) = match self.act {
            Act::FieldDay if w.len() == 4 => (reply("R 1D WI"), false),
            Act::FieldDay if w.len() == 5 && w[2] == "R" => (reply("RR73"), true),
            Act::NaVhf if w.len() == 3 && is_grid(w[2]) => (reply(&format!("R {DX_GRID}")), false),
            Act::NaVhf if w.len() == 4 && w[2] == "R" && is_grid(w[3]) => (reply("RR73"), true),
            Act::FieldDay | Act::NaVhf if closing => (reply("73"), true),
            Act::None if w.len() == 3 && is_grid(w[2]) => (reply(DX_RPT), false),
            Act::None if w.len() == 3 && w[2].starts_with('R') && is_report(&w[2][1..]) => {
                (reply("RR73"), true)
            }
            Act::None if w.len() == 3 && is_report(w[2]) => (reply(&format!("R{DX_RPT}")), false),
            Act::None if closing => (reply("73"), true),
            // A 73, or anything this script has no arm for: nothing new.
            _ => return,
        };
        self.next = Some(next);
        self.last = last;
    }

    /// Its over, if it has one.
    fn transmit(&mut self) -> Option<String> {
        let out = self.next.clone();
        if self.last {
            self.next = None;
        }
        out
    }
}

/// A decode of the partner's over, as the engine's decoder would deliver it.
fn heard(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: DX_SNR,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// A station in `contest`, with the master switch on and every Settings field a contest on
/// the list sends filled for an out-of-state entrant in Massachusetts.
fn engine_for(contest: &str, grid: &str) -> Engine {
    let mut e = Engine::new(ME, grid, 0);
    let mut s = e.settings().clone();
    s.fd_event = contest.into();
    // Winter Field Day's categories are H, I, O and M; ARRL Field Day's are A to F.
    s.fd_class = if contest == "wfd" { "1H" } else { "2B" }.into();
    s.fd_section = "EMA".into();
    s.contest_qth_state = "MA".into();
    s.contest_cq_zone = 5;
    s.contest_itu_zone = 8;
    s.contest_check = "74".into();
    s.contest_category_operator = "SINGLE-OP".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.tx_even = true;
    s.fd_active = true;
    e.apply_settings(s);
    e
}

/// The text of the over `plan_tx` planned, if it keyed one — through `commit_tx`, as the
/// radio loop does.
fn keyed(e: &mut Engine, slot: u64) -> Option<String> {
    let plan = e.plan_tx(slot)?;
    let text = match &plan.waveform {
        TxWaveform::Native { text, .. }
        | TxWaveform::Fast { text, .. }
        | TxWaveform::Deep { text, .. } => text.clone(),
        TxWaveform::Js8 { .. } => return None,
    };
    let wave = plan.waveform.build();
    let overs = e.commit_tx(&plan, wave, slot);
    (!overs.is_empty()).then_some(text)
}

/// FT8 tones back to the 77 message bits (genft8.f90: Costas at 0-6, 36-42 and 72-78; three
/// Gray-coded bits a data symbol), `None` for the all-zero frame a message the packer refused
/// leaves behind.
fn bits77(tones: &[i32]) -> Option<Vec<i8>> {
    const COSTAS: [i32; 7] = [3, 1, 4, 0, 6, 5, 2];
    const INV_GRAY: [i8; 8] = [0, 1, 3, 2, 6, 4, 5, 7];
    if tones.len() != 79 || tones[..7] != COSTAS {
        return None;
    }
    let mut bits = Vec::with_capacity(174);
    for (k, &t) in tones.iter().enumerate() {
        if (7..36).contains(&k) || (43..72).contains(&k) {
            let v = INV_GRAY[t as usize];
            bits.extend([(v >> 2) & 1, (v >> 1) & 1, v & 1]);
        }
    }
    bits.truncate(77);
    Some(bits)
}

/// The 77-bit type `pack77` chose (`i3`, with `n3` when `i3` is 0) and the text a receiving
/// station decodes, for `text` as the FT8 encoder packs it.
pub(super) fn air_form(text: &str) -> (String, String) {
    let tones = modes::make_mode(modes::ModeKind::Ft8).encode(text);
    let Some(bits) = bits77(&tones) else {
        return ("refused".into(), String::new());
    };
    let n = |r: std::ops::Range<usize>| bits[r].iter().fold(0u8, |a, &b| a * 2 + b as u8);
    let (i3, n3) = (n(74..77), n(71..74));
    let kind = if i3 == 0 {
        format!("i3=0 n3={n3}")
    } else {
        format!("i3={i3}")
    };
    let decoded = tempo_fast::unpack77(&bits).unwrap_or_default();
    (kind, decoded)
}

/// The tones, as digits, for the byte-for-byte comparison.
fn tones_of(kind: modes::ModeKind, text: &str) -> String {
    modes::make_mode(kind)
        .encode(text)
        .iter()
        .map(|t| char::from_digit(*t as u32, 10).unwrap_or('?'))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Running,
    SearchAndPounce,
    /// S&P the way the Operate cockpit does it: a double-click on the partner's CQ.
    DoubleClick,
    /// A double-click on the partner's CQ (the cockpit's `call_station`) from the contest's
    /// S&P as the contest switch leaves it, TX off.
    ClickFromSp,
    /// The same double-click from the contest's Running.
    ClickFromRun,
    /// The same double-click from FT8 QSO mode, the contest switch on.
    ClickFromQso,
    /// The Operate strip's Call CQ, from the contest's S&P as the switch leaves it, TX off.
    CallCq,
    /// The Operate strip's Call CQ from FT8 QSO mode, the contest switch on.
    CallCqFromQso,
    /// The Operate strip's S&P pressed during the contest's Running, the partner calling CQ.
    StripSpFromRun,
    /// The Operate strip's S&P from FT8 QSO mode, the contest switch on.
    StripSpFromQso,
}

impl Role {
    /// The partner calls CQ, and this station answers it.
    fn partner_runs(self) -> bool {
        !matches!(self, Role::Running | Role::CallCq | Role::CallCqFromQso)
    }

    /// The operator double-clicks the partner's first CQ.
    fn clicks(self) -> bool {
        matches!(
            self,
            Role::DoubleClick | Role::ClickFromSp | Role::ClickFromRun | Role::ClickFromQso
        )
    }

    /// The dump's original scenes: past the TX switch at the end, they print nothing they did
    /// not print then.
    fn first_dump(self) -> bool {
        matches!(
            self,
            Role::Running | Role::SearchAndPounce | Role::DoubleClick
        )
    }
}

/// One scene's record: every over each side sent, by slot, and what the contest log kept.
struct Scene {
    entered: Result<(), String>,
    lines: Vec<String>,
    /// This station's overs, in order.
    sent: Vec<String>,
    /// The contest log's rows: call, received fields, sent fields, dupe mark.
    logged: Vec<String>,
    /// Where the station ended: the mode, its role and its sequencer's state.
    end: String,
    /// Whether TX was still on at the end.
    tx: bool,
    /// The slots at which the radio loop would key a CW ID ("CW ID after 73").
    cw_ids: Vec<u64>,
}

fn fields(v: &[tempo_core::contest::FieldValue]) -> String {
    v.iter()
        .map(|f| format!("{}={}", f.key, f.raw))
        .collect::<Vec<_>>()
        .join(",")
}

/// Run `role` in `contest` on `tier` against a WSJT-X partner in `act`.
fn scene(contest: &str, tier: Tier, role: Role, act: Act, grid: &str) -> Scene {
    play(engine_for(contest, grid), tier, role, act)
}

/// [`scene`] on the station `e`, set up for its contest.
fn play(mut e: Engine, tier: Tier, role: Role, act: Act) -> Scene {
    e.set_tier(tier);
    let entered = match role {
        Role::Running | Role::ClickFromRun | Role::StripSpFromRun => e.set_mode("fieldday-run"),
        Role::SearchAndPounce | Role::DoubleClick => e.set_mode("fieldday-sp"),
        // The contest switch has already put the station in S&P.
        Role::ClickFromSp | Role::CallCq => Ok(()),
        Role::ClickFromQso | Role::CallCqFromQso | Role::StripSpFromQso => {
            e.set_mode("qso-monitor")
        }
    };
    // S&P is passive and does not arm TX by itself; the operator's Enable TX does.
    if matches!(role, Role::SearchAndPounce | Role::DoubleClick) {
        e.set_tx_enabled(true);
    }
    let mut partner = Partner::new(act, role.partner_runs());
    let (mut lines, mut sent) = (Vec::new(), Vec::new());
    let mut cw_ids = Vec::new();
    let mut clicked = false;
    for slot in 0..SLOTS {
        // The radio loop's deferred "Disable Tx after sending 73", once the last over has played.
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        // …and its deferred CW ID, which it keys at the same moment.
        if e.take_pending_cw_id() {
            cw_ids.push(slot);
        }
        if slot % 2 == 0 {
            if slot == 0 && matches!(role, Role::CallCq | Role::CallCqFromQso) {
                let r = e.start_running();
                lines.push(format!("  s{slot:02} Call CQ: {r:?}"));
            }
            if matches!(
                (slot, role),
                (2, Role::StripSpFromRun) | (0, Role::StripSpFromQso)
            ) {
                let r = e.start_search_and_pounce();
                lines.push(format!("  s{slot:02} S&P: {r:?}"));
            }
            if let Some(text) = keyed(&mut e, slot) {
                let (kind, decoded) = air_form(&text);
                let tones = match tier {
                    Tier::Ft4 => tones_of(modes::ModeKind::Ft4, &text),
                    _ => tones_of(modes::ModeKind::Ft8, &text),
                };
                lines.push(format!(
                    "  s{slot:02} TX {text:?} {kind} decodes {decoded:?} tones {tones}"
                ));
                partner.hear(&text);
                sent.push(text);
            }
        } else {
            // The decoder runs on the partner's period whether or not it sent anything.
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            lines.push(format!("  s{slot:02} RX {text:?}"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
            if role.clicks() && !clicked && text.starts_with("CQ ") {
                clicked = true;
                let r = e.work_station(DX, Some(DX_GRID), Some(&text), Some(DX_SNR), None);
                lines.push(format!("  s{slot:02} double-click the CQ: {r:?}"));
            }
        }
    }
    let logged = match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| {
                format!(
                    "{} rx[{}] tx[{}] mode {} {} dupe {}",
                    q.call,
                    fields(&q.rx),
                    fields(&q.tx),
                    q.mode,
                    q.submode,
                    q.dupe
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    if e.take_pending_cw_id() {
        cw_ids.push(SLOTS);
    }
    let mode = mode_line(&e);
    lines.push(format!("  end: {mode} tx={}", on_off(e.tx_enabled())));
    for l in &logged {
        lines.push(format!("  LOG {l}"));
    }
    // Where a gesture leaves the contest, the contact is in the station's own log.
    if !role.first_dump() {
        for r in e.stored_log() {
            lines.push(format!(
                "  GENERAL {} {} grid {:?} sent {:?} rcvd {:?}",
                r.call, r.mode, r.grid, r.rst_sent, r.rst_rcvd
            ));
        }
    }
    Scene {
        entered,
        lines,
        sent,
        logged,
        end: mode,
        tx: e.tx_enabled(),
        cw_ids,
    }
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Ft8 => "FT8",
        Tier::Ft4 => "FT4",
        Tier::TempoFast => "FT1",
        Tier::TempoDeep => "DX1",
        Tier::Ft2 => "FT2",
        _ => "other",
    }
}

/// A stub country file for the contests that score by where the other station is (CQ WW,
/// CQ WPX), which refuse to start without one: it places this file's two calls and nothing
/// else.
fn place(call: &str) -> Option<tempo_core::contest::CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    (up.starts_with('K') || up.starts_with('W')).then_some(tempo_core::contest::CallLocation {
        entity: "United States",
        continent: "NA",
        cq_zone: None,
    })
}

/// One scene's block in the dump: its title and what `scene` recorded.
fn block(out: &mut String, title: String, s: Scene) {
    out.push_str(&format!("[{title}] entered={:?}\n", s.entered));
    for l in s.lines {
        out.push_str(&l);
        out.push('\n');
    }
}

/// The whole map, one block per contest, tier and role, in a fixed order — written to the
/// file `CONTEST_FT_DUMP` names (or printed), so a run on the base and a run on this branch can be
/// compared byte for byte.
///
/// ⚠️ Run it alone (`-- --ignored dump_contest_ft_messages`): it installs [`place`] as the
/// country file, which is process-wide and once-only, and every other test in this binary
/// runs without one.
#[test]
#[ignore = "writes the before/after dump; run by hand"]
fn dump_contest_ft_messages() {
    let _ = tempo_core::contest::install_call_resolver(place);
    let mut out = String::new();
    for contest in CONTESTS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::Running, Role::SearchAndPounce] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // Controls: Field Day with a six-character locator in Settings; the grid contests on
    // FT1, which keeps today's messages; a double-click S&P in Field Day and in a VHF contest.
    for role in [Role::Running, Role::SearchAndPounce] {
        let s = scene("arrlfd", Tier::Ft8, role, Act::FieldDay, "FN42ab");
        block(&mut out, format!("\"arrlfd\" FT8 {role:?} grid FN42ab"), s);
        let s = scene("arrlvhf_jan", Tier::TempoFast, role, Act::NaVhf, MY_GRID);
        block(&mut out, format!("\"arrlvhf_jan\" FT1 {role:?}"), s);
    }
    for contest in ["arrlfd", "arrlvhf_jan", "cqww_cw"] {
        let s = scene(
            contest,
            Tier::Ft8,
            Role::DoubleClick,
            act_of(contest),
            MY_GRID,
        );
        block(&mut out, format!("{contest:?} FT8 DoubleClick"), s);
    }
    // Outside any contest: Call CQ and a double-click, on FT8 and FT4.
    for tier in [Tier::Ft8, Tier::Ft4] {
        out.push_str(&no_contest(tier, true));
        out.push_str(&no_contest(tier, false));
    }
    // The cockpit's own gestures while a contest runs: a double-click on the partner's CQ, and
    // the Operate strip's Call CQ, from the contest's S&P as the switch leaves it, in every
    // contest on FT8 and FT4.
    for contest in CONTESTS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::ClickFromSp, Role::CallCq] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // …from the contest's Running, and from FT8 QSO mode with the switch on.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp", "wfd", "ilqp"] {
        for role in [Role::ClickFromRun, Role::ClickFromQso, Role::CallCqFromQso] {
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            block(&mut out, format!("{contest:?} FT8 {role:?}"), s);
        }
    }
    // FT1, DX1 and FT2 have no WSJT-X contest messages.
    for tier in [Tier::TempoFast, Tier::TempoDeep, Tier::Ft2] {
        for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
            for role in [Role::Running, Role::ClickFromSp, Role::CallCq] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // The strip's S&P during the contest's Running and from FT8 QSO mode.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp", "wfd"] {
        for role in [Role::StripSpFromRun, Role::StripSpFromQso] {
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            block(&mut out, format!("{contest:?} FT8 {role:?}"), s);
        }
    }
    // The contest's S&P with Tx 1st on the CQ station's period.
    for (contest, tier) in [
        ("arrlfd", Tier::Ft8),
        ("arrlvhf_jan", Tier::Ft8),
        ("txqp", Tier::Ft8),
        ("txqp", Tier::Ft4),
        ("arrlfd", Tier::TempoFast),
    ] {
        out.push_str(&scene_period(contest, tier));
    }
    // A plain gesture with a contest picked and not filled in.
    for on in [false, true] {
        out.push_str(&unfilled_contest(on));
    }
    match std::env::var("CONTEST_FT_DUMP") {
        Ok(path) => std::fs::write(&path, out).expect("the dump writes"),
        Err(_) => print!("{out}"),
    }
}

/// Call CQ (`qso-run`) or a double-click on a CQ (`qso-monitor`, then `call_station_ctx`)
/// with the master switch off, against a WSJT-X partner with no activity.
fn no_contest(tier: Tier, running: bool) -> String {
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(tier);
    let entered = if running {
        e.start_running()
    } else {
        e.set_mode("qso-monitor")
    };
    let mut partner = Partner::new(Act::None, !running);
    let mut out = format!(
        "[no contest {} {}] entered={entered:?}\n",
        tier_name(tier),
        if running { "CallCq" } else { "DoubleClick" }
    );
    let mut clicked = false;
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                let (kind, decoded) = air_form(&text);
                let kind_tones = match tier {
                    Tier::Ft4 => tones_of(modes::ModeKind::Ft4, &text),
                    _ => tones_of(modes::ModeKind::Ft8, &text),
                };
                out.push_str(&format!(
                    "  s{slot:02} TX {text:?} {kind} decodes {decoded:?} tones {kind_tones}\n"
                ));
                partner.hear(&text);
            }
        } else {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
            if !running && !clicked && text.starts_with("CQ ") {
                clicked = true;
                e.set_tx_enabled(true);
                let r = e.work_station(DX, Some(DX_GRID), Some(&text), Some(DX_SNR), None);
                out.push_str(&format!("  s{slot:02} double-click the CQ: {r:?}\n"));
            }
        }
    }
    out.push_str(&format!("  end: tx={}\n", on_off(e.tx_enabled())));
    out
}

/// Where the station is: the mode, its role and its sequencer's state.
fn mode_line(e: &Engine) -> String {
    match &e.mode {
        Mode::FieldDay { station, running } => {
            format!("FieldDay running={running} state={:?}", station.state)
        }
        Mode::Qso { station, running } => {
            format!("Qso running={running} state={:?}", station.state)
        }
        Mode::Chat => "Chat".into(),
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// The contest's S&P with this station's Tx 1st odd, the period the partner, a WSJT-X runner in
/// the contest's activity, calls CQ in: the partner hears only what goes out in the even periods,
/// and nothing is decoded in a period this station transmits in. Every slot is polled for this
/// station's over.
fn scene_period(contest: &str, tier: Tier) -> String {
    let mut e = engine_for(contest, MY_GRID);
    let mut s = e.settings().clone();
    s.tx_even = false;
    e.apply_settings(s);
    e.set_tier(tier);
    let entered = e.set_mode("fieldday-sp");
    e.set_tx_enabled(true);
    let mut partner = Partner::new(act_of(contest), true);
    let mut out = format!(
        "[{contest:?} {} S&P, Tx 1st odd] entered={entered:?}\n",
        tier_name(tier)
    );
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        let mine = keyed(&mut e, slot);
        if let Some(text) = &mine {
            out.push_str(&format!("  s{slot:02} TX {text:?}\n"));
            if slot % 2 == 0 {
                partner.hear(text);
            }
        }
        if slot % 2 == 1 {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            if mine.is_some() {
                out.push_str(&format!("  s{slot:02} RX missed, transmitting: {text:?}\n"));
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            }
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
        }
    }
    out.push_str(&format!(
        "  end: {} tx={}\n",
        mode_line(&e),
        on_off(e.tx_enabled())
    ));
    if let Mode::FieldDay { station, .. } = &e.mode {
        for q in station.log.qsos() {
            out.push_str(&format!(
                "  LOG {} rx[{}] tx[{}]\n",
                q.call,
                fields(&q.rx),
                fields(&q.tx)
            ));
        }
    }
    out
}

/// A plain gesture with a contest picked whose exchange is not filled in (the Texas QSO Party with
/// no county or state): with Contest mode off the strip's Call CQ, with it on the Tx6 row's plain
/// CQ; against a WSJT-X partner with no activity answering it.
fn unfilled_contest(on: bool) -> String {
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.fd_event = "txqp".into();
    s.fd_active = on;
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    let entered = if on {
        e.start_cq(None)
    } else {
        e.start_running()
    };
    e.set_tx_enabled(true);
    let mut partner = Partner::new(Act::None, false);
    let mut out = format!(
        "[txqp not filled in, Contest mode {}, {}] entered={entered:?}\n",
        on_off(on),
        if on { "Tx6 CQ" } else { "Call CQ" }
    );
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                out.push_str(&format!("  s{slot:02} TX {text:?}\n"));
                partner.hear(&text);
            }
        } else {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
        }
    }
    out.push_str(&format!("  end: tx={}\n", on_off(e.tx_enabled())));
    for r in e.stored_log() {
        out.push_str(&format!("  GENERAL {} {}\n", r.call, r.mode));
    }
    out
}

/// What this station sends in `contest` as `role`, against a WSJT-X partner, up to and
/// including its first CQ after the contact (Running) or its last over (S&P); and the row the
/// contest log keeps, `None` where the contact never completes.
fn expected(contest: &str, role: Role, tier: &str) -> (Vec<String>, Option<String>) {
    let row = |rx: &str, tx: &str| {
        Some(format!(
            "W9XYZ rx[{rx}] tx[{tx}] mode DIG {tier} dupe false"
        ))
    };
    let v = |s: &[&str]| s.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    match (contest, role) {
        // Field Day: what it has always sent, and the 73 WSJT-X's runner answers the caller's
        // RR73 with (Tx5).
        ("" | "arrlfd", Role::Running) => (
            v(&[
                "CQ FD K1ABC FN42",
                "W9XYZ K1ABC R 2B EMA",
                "W9XYZ K1ABC 73",
                "CQ FD K1ABC FN42",
            ]),
            row("CLASS=1D,SECTION=WI", "CLASS=2B,SECTION=EMA"),
        ),
        ("" | "arrlfd", _) => (
            v(&["W9XYZ K1ABC 2B EMA", "W9XYZ K1ABC RR73"]),
            row("CLASS=1D,SECTION=WI", "CLASS=2B,SECTION=EMA"),
        ),
        // Banned FT8/FT4: nothing new. Winter Field Day runs Field Day's exchange…
        ("wfd", Role::Running) => (
            v(&[
                "CQ FD K1ABC FN42",
                "W9XYZ K1ABC R 1H EMA",
                "CQ FD K1ABC FN42",
            ]),
            row("CLASS=1D,SECTION=WI", "CLASS=1H,SECTION=EMA"),
        ),
        ("wfd", _) => (
            v(&["W9XYZ K1ABC 1H EMA", "W9XYZ K1ABC RR73"]),
            row("CLASS=1D,SECTION=WI", "CLASS=1H,SECTION=EMA"),
        ),
        // …and the two parties keep sending what they sent, which completes nothing.
        ("ilqp" | "nyqp", Role::Running) => (v(&["CQ FD K1ABC FN42"; 8]), None),
        ("ilqp" | "nyqp", _) => (v(&["W9XYZ K1ABC  "; 7]), None),
        // The ARRL VHF contests: WSJT-X's NA VHF Contest messages.
        ("arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep", Role::Running) => (
            v(&[
                "CQ TEST K1ABC FN42",
                "W9XYZ K1ABC R FN42",
                "W9XYZ K1ABC 73",
                "CQ TEST K1ABC FN42",
            ]),
            row("GRID=EN37", "GRID=FN42"),
        ),
        ("arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep", _) => (
            v(&["W9XYZ K1ABC FN42", "W9XYZ K1ABC RR73"]),
            row("GRID=EN37", "GRID=FN42"),
        ),
        // Every other contest: WSJT-X has no activity for it, so the standard messages.
        (_, Role::Running) => (
            v(&[
                "CQ K1ABC FN42",
                "W9XYZ K1ABC -12",
                "W9XYZ K1ABC RR73",
                "CQ K1ABC FN42",
            ]),
            row("", ""),
        ),
        (_, _) => (
            v(&["W9XYZ K1ABC FN42", "W9XYZ K1ABC R-12", "W9XYZ K1ABC 73"]),
            row("", ""),
        ),
    }
}

/// ⭐ **Every contest on the list, both roles, FT8 and FT4, by value**: the text of each over,
/// its 77-bit type and the text a receiving station decodes, and the row the contest log
/// keeps. CQ WW and CQ WPX need a country file this binary does not install; they are in the
/// dump, and the table that picks their messages is pinned in tempo-core.
#[test]
fn every_contest_sends_its_own_messages_on_ft8_and_ft4() {
    let resolver_free = CONTESTS
        .iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"));
    for contest in resolver_free {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::Running, Role::SearchAndPounce] {
                let what = format!("{contest:?} {} {role:?}", tier_name(tier));
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                assert_eq!(s.entered, Ok(()), "{what}");
                let (want, row) = expected(contest, role, tier_name(tier));
                assert!(
                    s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                    "{what}: sent {:?}",
                    s.sent
                );
                if role == Role::SearchAndPounce {
                    assert_eq!(s.sent.len(), want.len(), "{what}: S&P sends nothing more");
                }
                assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
                // Each over packs as the standard or Field Day message it reads as, and
                // decodes back to exactly its own text.
                for text in &s.sent {
                    let (kind, decoded) = air_form(text);
                    let want_kind = if text.ends_with(" EMA") {
                        "i3=0 n3=3"
                    } else {
                        "i3=1"
                    };
                    assert_eq!(kind, want_kind, "{what}: {text:?}");
                    assert_eq!(decoded, text.trim(), "{what}: {text:?}");
                }
            }
        }
    }
}

/// FT1, DX1 and FT2 have no WSJT-X contest messages: in a grid contest they keep Field
/// Day's, unchanged. Moving to FT8 changes the messages and re-arms the run onto `CQ TEST`;
/// FT8↔FT4 keeps the contact in flight.
#[test]
fn the_tier_decides_whether_the_contest_messages_apply() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::TempoFast);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ FD K1ABC FN42"));
    e.set_tier(Tier::Ft8);
    e.set_tx_enabled(true); // a mode switch that moves the dial turns TX off, as WSJT-X does
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 4);
    assert_eq!(keyed(&mut e, 4).as_deref(), Some("W9XYZ K1ABC R FN42"));
    e.set_tier(Tier::Ft4);
    e.set_tx_enabled(true);
    assert_eq!(
        keyed(&mut e, 6).as_deref(),
        Some("W9XYZ K1ABC R FN42"),
        "FT8 to FT4 keeps the contact"
    );
}

/// ⛔ The keying gate: a contest whose FT8/FT4 messages carry the grid does not key without a
/// valid one in Settings. Field Day's exchange carries none and still keys (the control), and
/// the same contest keys once the grid is there.
#[test]
fn a_contest_whose_messages_carry_the_grid_does_not_key_without_one() {
    let mut e = engine_for("txqp", "");
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    for slot in [0, 2, 4] {
        assert_eq!(keyed(&mut e, slot), None, "no grid, no over (slot {slot})");
    }
    let mut fd = engine_for("arrlfd", "");
    fd.set_tier(Tier::Ft8);
    fd.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut fd, 0).as_deref(), Some("CQ FD K1ABC"));
    let mut ok = engine_for("txqp", MY_GRID);
    ok.set_tier(Tier::Ft8);
    ok.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut ok, 0).as_deref(), Some("CQ K1ABC FN42"));
}

/// The passive-first rule: turning a contest on enters S&P, which calls nobody — with TX
/// enabled and a quiet band, no contest keys anything.
#[test]
fn entering_any_contest_keys_nothing() {
    for contest in CONTESTS
        .iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"))
    {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(Tier::Ft8);
        assert!(
            matches!(e.mode, Mode::FieldDay { running: false, .. }),
            "{contest:?}: the switch enters S&P"
        );
        e.set_tx_enabled(true);
        for slot in (0..8).step_by(2) {
            assert_eq!(keyed(&mut e, slot), None, "{contest:?} slot {slot}");
            e.ingest_decodes_for_test(&[], slot + 1);
        }
    }
}

/// ⛔ The wall-clock watchdog still ends a contest run, and Halt still stops a contact in
/// flight — both checked on the grid contest's messages, with the same run keying first as
/// the control.
#[test]
fn the_watchdog_and_halt_stop_a_contest_run() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    let mut s = e.settings().clone();
    s.tx_watchdog_min = 6;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ TEST K1ABC FN42"));
    let limit = 6 * 60;
    e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(limit + 1));
    assert_eq!(keyed(&mut e, 2), None, "the watchdog trips");
    assert!(!e.tx_enabled, "and disarms TX");

    let mut e = engine_for("txqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 2);
    e.halt_tx();
    for slot in [2, 4, 6] {
        assert_eq!(
            keyed(&mut e, slot),
            None,
            "nothing after Halt (slot {slot})"
        );
    }
}

/// ⭐ A grid contest's FT8 contact is the row a typed one is: the same received and sent
/// GRID, so it dupes against a typed entry and a typed entry against it, and a rover in a
/// new grid is a new contact. S&P does not answer a station it has already worked; a
/// duplicate the run logs is journaled at once, like a typed one.
#[test]
fn an_ft8_grid_contact_dupes_and_journals_like_a_typed_one() {
    let path = std::env::temp_dir().join(format!("contest-ft-vhf-{}.adi", std::process::id()));
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_fd_log_path(path.clone());
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-sp").expect("S&P starts");
    e.set_tx_enabled(true);
    let contact = |e: &mut Engine, from: u64| {
        e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], from);
        assert_eq!(keyed(e, from).as_deref(), Some("W9XYZ K1ABC FN42"));
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ R EN37")], from + 2);
        assert_eq!(keyed(e, from + 2).as_deref(), Some("W9XYZ K1ABC RR73"));
        e.ingest_decodes_for_test(&[], from + 4);
    };
    contact(&mut e, 0);
    let rows = |e: &Engine| match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| (fields(&q.rx), fields(&q.tx), q.dupe))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let ft8 = rows(&e);
    assert_eq!(ft8, vec![("GRID=EN37".into(), "GRID=FN42".into(), false)]);
    // A typed entry for the same station and grid is the duplicate…
    let typed = vec![("GRID".to_string(), "EN37".to_string())];
    assert_eq!(
        e.contest_log_manual("W9XYZ", &typed, "DIG", Some("FT8")),
        Ok(true)
    );
    assert_eq!(rows(&e)[1], ("GRID=EN37".into(), "GRID=FN42".into(), true));
    // …a rover in a new grid is not…
    let moved = vec![("GRID".to_string(), "EN38".to_string())];
    assert_eq!(
        e.contest_log_manual("W9XYZ", &moved, "DIG", Some("FT8")),
        Ok(true)
    );
    assert!(!rows(&e)[2].2);
    // …and S&P does not answer that station's CQ again: the log already holds the contact.
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 6);
    assert_eq!(
        keyed(&mut e, 6),
        None,
        "the same contact is not answered twice"
    );
    // A runner works whoever calls, so the same station calling again is worked; that
    // duplicate is logged, marked, and journaled before anything else is.
    e.set_mode("fieldday-run").expect("Running starts");
    assert_eq!(keyed(&mut e, 8).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 10);
    assert_eq!(keyed(&mut e, 10).as_deref(), Some("W9XYZ K1ABC R FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 12);
    assert_eq!(keyed(&mut e, 12).as_deref(), Some("W9XYZ K1ABC 73"));
    assert_eq!(rows(&e).len(), 4);
    assert_eq!(rows(&e)[3], ("GRID=EN37".into(), "GRID=FN42".into(), true));
    e.station.journals.settle();
    let journal = std::fs::read_to_string(&path).expect("the journal is written");
    let records = journal.to_ascii_lowercase().matches("<eor>").count();
    let _ = std::fs::remove_file(&path);
    assert_eq!(records, 4, "every row is in the journal");
}

/// Field Day's runner answers the caller's RR73 or RRR with 73 and then calls CQ, in ARRL Field
/// Day on FT8 and FT4, as WSJT-X's Field Day activity does (Tx5, mainwindow.cpp 9210-9215). A
/// bare 73 from the caller, and a confirmation from anyone else, are answered with nothing, as
/// before; and FT1 and Winter Field Day close as they always have. One row each time.
#[test]
fn a_field_day_runner_answers_the_callers_rr73_and_rrr_with_73() {
    let rows = |e: &Engine| match &e.mode {
        Mode::FieldDay { station, .. } => station.log.qsos().len(),
        _ => 0,
    };
    let run = |contest: &str, tier: Tier, closing: &str| {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        let cq = keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
        let exch = keyed(&mut e, 2).expect("the rogered exchange");
        assert!(exch.starts_with("W9XYZ K1ABC R "), "{exch}");
        e.ingest_decodes_for_test(&[heard(closing)], 4);
        let after = keyed(&mut e, 4);
        e.ingest_decodes_for_test(&[], 6);
        let next = keyed(&mut e, 6);
        (cq, after, next, rows(&e))
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        for closing in ["K1ABC W9XYZ RR73", "K1ABC W9XYZ RRR"] {
            let (cq, after, next, logged) = run("arrlfd", tier, closing);
            let what = format!("{} {closing:?}", tier_name(tier));
            assert_eq!(after.as_deref(), Some("W9XYZ K1ABC 73"), "{what}");
            assert_eq!(next.as_deref(), Some(cq.as_str()), "{what}: then CQ");
            assert_eq!(logged, 1, "{what}");
        }
    }
    // Nothing goes back where it never did.
    for (contest, tier, closing) in [
        ("arrlfd", Tier::Ft8, "K1ABC W9XYZ 73"),
        ("arrlfd", Tier::Ft8, "K1ABC K2DEF RR73"),
        ("arrlfd", Tier::TempoFast, "K1ABC W9XYZ RR73"),
        ("wfd", Tier::Ft8, "K1ABC W9XYZ RR73"),
    ] {
        let (cq, after, _, logged) = run(contest, tier, closing);
        let what = format!("{contest:?} {} {closing:?}", tier_name(tier));
        assert_eq!(
            after.as_deref(),
            Some(cq.as_str()),
            "{what}: straight back to CQ"
        );
        assert_eq!(logged, 1, "{what}");
    }
}

/// Whether the station is in the contest's S&P, listening, with nothing to send.
fn listening(e: &Engine) -> bool {
    matches!(&e.mode, Mode::FieldDay { station, running: false }
        if station.state == tempo_core::fieldday::FdState::Listening && station.pending.is_none())
}

/// A station with the contest switch on and Field Day's class not filled in, in FT8 QSO mode:
/// the contest never started. The contest is a fresh install's (blank, ARRL Field Day), the
/// configuration most stations run.
fn contest_that_cannot_start() -> Engine {
    let mut e = engine_for("", MY_GRID);
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    let mut s = e.settings().clone();
    s.fd_class = String::new();
    s.fd_active = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    e
}

/// The double-click answers on the period opposite the CQ's, as WSJT-X's does (`m_txFirst`
/// from the clicked message's time, mainwindow.cpp 8963-8964), and moves RX — and TX, unless
/// Hold is on — onto the station; it enables TX and keys at once.
#[test]
fn the_contest_double_click_takes_the_period_and_the_frequency_of_the_cq() {
    for hold in [false, true] {
        let mut e = engine_for("arrlfd", MY_GRID);
        let mut s = e.settings().clone();
        s.tx_even = false;
        e.apply_settings(s);
        e.set_tier(Tier::Ft8);
        e.set_hold_tx_freq(hold);
        let before_tx = e.tx_offset_hz;
        // The CQ was heard in an odd period (decoded at the even boundary after it).
        e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
        e.work_station(
            DX,
            Some(DX_GRID),
            Some("CQ FD W9XYZ EN37"),
            Some(DX_SNR),
            Some(1234.0),
        )
        .expect("the double-click is taken");
        assert!(e.tx_enabled, "Double-click on call sets Tx enable");
        assert!(e.immediate_tx, "and the current period keys if it fits");
        assert_eq!(e.rx_offset_hz, 1234.0, "RX moves onto the station");
        if hold {
            assert_eq!(e.tx_offset_hz, before_tx, "Hold keeps TX where it was");
        } else {
            assert_eq!(e.tx_offset_hz, 1234.0, "TX follows RX without Hold");
        }
        assert_eq!(keyed(&mut e, 3), None, "never in the CQ's own period");
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ K1ABC 2B EMA"),
            "the period opposite the CQ's"
        );
    }
}

/// ⛔ A double-click on a CQ that carries no grid, in a contest that answers with the grid, is
/// refused and changes nothing: the contest's S&P does not answer such a CQ either.
#[test]
fn a_double_click_on_a_cq_with_no_grid_changes_nothing() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ")], 2);
    let r = e.work_station(DX, None, Some("CQ TEST W9XYZ"), Some(DX_SNR), None);
    assert!(r.is_err(), "a CQ with no grid: {r:?}");
    assert!(listening(&e), "the station is as it was");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⛔ With no decode to take the period from, the double-click is refused, as outside a contest.
#[test]
fn a_double_click_with_no_decode_changes_nothing() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    let r = e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None);
    assert!(r.is_err(), "no decode: {r:?}");
    assert!(listening(&e), "the station is as it was");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⛔ With the contest switch on and its exchange not filled in, the double-click is refused
/// with the reason the Contest screen gives, and changes nothing.
#[test]
fn a_double_click_in_a_contest_that_cannot_start_changes_nothing() {
    let mut e = contest_that_cannot_start();
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    let r = e.work_station(DX, None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None);
    assert!(r.is_err(), "no class: {r:?}");
    assert!(
        matches!(&e.mode, Mode::Qso { station, running: false } if station.dxcall.is_none()),
        "still in FT8 QSO mode, working nobody"
    );
    assert!(!e.tx_enabled, "TX stays off");
}

/// The cockpit's other clicks, and every click where the contest's own FT8/FT4 messages do not
/// apply, work the station as they always have: a double-click on a station calling you, a
/// roster click, a double-click with the contest switch off, on FT2, and on your own CQ.
#[test]
fn every_other_click_works_the_station_as_before() {
    let qso_with = |e: &Engine| match &e.mode {
        Mode::Qso { station, .. } => station.dxcall.clone(),
        _ => None,
    };
    // A double-click on a station calling you, in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("K1ABC W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "a call to you: FT8 QSO mode"
    );
    // A roster click (no message), in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, Some(DX_GRID), None, None, None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "a roster click: FT8 QSO mode"
    );
    // A double-click on a CQ on FT2, in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft2);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(qso_with(&e).as_deref(), Some(DX), "FT2: FT2 QSO mode");
    // A double-click on a CQ with the contest switch off.
    let mut e = Engine::new(ME, MY_GRID, 0);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "no contest: FT8 QSO mode"
    );
    // A double-click on your own CQ (its echo in the band activity) starts nothing.
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.work_station(ME, None, Some("CQ FD K1ABC FN42"), Some(DX_SNR), None)
        .expect("ignored");
    assert!(listening(&e), "your own CQ: nothing");
    assert!(!e.tx_enabled, "and TX stays off");
}

/// The contests whose FT8/FT4 messages these tests can run: every one on the list but CQ WW and
/// CQ WPX, which need a country file this binary does not install.
fn resolver_free() -> impl Iterator<Item = &'static str> {
    CONTESTS
        .into_iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"))
}

/// The contests whose rules ban FT8 and FT4.
fn bans_ft8(contest: &str) -> bool {
    matches!(contest, "wfd" | "ilqp" | "nyqp")
}

/// ⭐ **The Operate cockpit's own gestures while a contest runs, every contest, FT8 and FT4, by
/// value.** A double-click on the partner's CQ hands that station to the contest's S&P, which
/// sends exactly what S&P sends and logs the same row; the strip's Call CQ is the contest's
/// Running and sends what Running sends. In the three contests that ban FT8 and FT4 both leave
/// the contest as they always have, and nothing reaches the contest log.
#[test]
fn the_cockpits_gestures_work_the_running_contest() {
    for contest in resolver_free() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::ClickFromSp, Role::CallCq] {
                let what = format!("{contest:?} {} {role:?}", tier_name(tier));
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                assert_eq!(s.entered, Ok(()), "{what}");
                if bans_ft8(contest) {
                    assert!(
                        s.end.starts_with("Qso"),
                        "{what}: left as before, {}",
                        s.end
                    );
                    assert!(s.logged.is_empty(), "{what}: {:?}", s.logged);
                    continue;
                }
                let like = if role == Role::CallCq {
                    Role::Running
                } else {
                    Role::SearchAndPounce
                };
                let (want, row) = expected(contest, like, tier_name(tier));
                assert!(
                    s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                    "{what}: sent {:?}",
                    s.sent
                );
                if like == Role::SearchAndPounce {
                    assert_eq!(s.sent.len(), want.len(), "{what}: nothing more");
                }
                assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
                let running = role == Role::CallCq;
                assert!(
                    s.end.starts_with(&format!("FieldDay running={running}")),
                    "{what}: {}",
                    s.end
                );
            }
        }
    }
    // The same double-click from the contest's Running and from FT8 QSO mode, and Call CQ from
    // FT8 QSO mode.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        let cq = expected(contest, Role::Running, "FT8").0[0].clone();
        for role in [Role::ClickFromRun, Role::ClickFromQso, Role::CallCqFromQso] {
            let what = format!("{contest:?} FT8 {role:?}");
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            let like = if role == Role::CallCqFromQso {
                Role::Running
            } else {
                Role::SearchAndPounce
            };
            let (mut want, row) = expected(contest, like, "FT8");
            if role == Role::ClickFromRun {
                want.insert(0, cq.clone());
            }
            assert!(
                s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                "{what}: sent {:?}",
                s.sent
            );
            assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
        }
    }
}

/// ⛔ Call CQ while the contest cannot start (its exchange not filled in) is refused as the
/// Contest screen's Running is, and changes nothing.
#[test]
fn call_cq_in_a_contest_that_cannot_start_changes_nothing() {
    let mut e = contest_that_cannot_start();
    let r = e.start_running();
    assert!(r.is_err(), "no class: {r:?}");
    assert!(
        matches!(&e.mode, Mode::Qso { running: false, .. }),
        "still in FT8 QSO mode"
    );
    assert!(!e.tx_enabled, "TX stays off");
    assert_eq!(keyed(&mut e, 0), None, "nothing keys");
}

/// The Tx6 row still calls the CQ it shows, plain or directed, as an FT8 QSO-mode run, in a
/// contest too: the strip's Call CQ is the only one that follows the contest.
#[test]
fn the_tx6_row_still_sends_the_cq_it_shows() {
    for (dir, want) in [(None, "CQ K1ABC FN42"), (Some("DX"), "CQ DX K1ABC FN42")] {
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(Tier::Ft8);
        e.start_cq(dir).expect("the run starts");
        assert!(
            matches!(&e.mode, Mode::Qso { running: true, .. }),
            "Tx6 {dir:?}: FT8 QSO mode"
        );
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(want), "Tx6 {dir:?}");
    }
}

/// ⛔ The transmit guards hold for the cockpit's gestures exactly as for the Contest screen's
/// buttons: a contest whose messages carry the grid keys nothing on Call CQ without one; the
/// wall-clock watchdog ends a run Call CQ started; Halt stops a contact a double-click started;
/// and with "Double-click on call sets Tx enable" off, a double-click leaves TX off.
#[test]
fn the_transmit_guards_hold_for_the_cockpits_gestures() {
    // No grid in Settings: Call CQ keys nothing.
    let mut e = engine_for("txqp", "");
    e.set_tier(Tier::Ft8);
    e.start_running().expect("the run starts");
    for slot in [0, 2, 4] {
        assert_eq!(keyed(&mut e, slot), None, "no grid, no over (slot {slot})");
    }
    // The watchdog ends a run Call CQ started.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    let mut s = e.settings().clone();
    s.tx_watchdog_min = 6;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.start_running().expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(6 * 60 + 1));
    assert_eq!(keyed(&mut e, 2), None, "the watchdog trips");
    assert!(!e.tx_enabled, "and disarms TX");
    // Halt stops a contact a double-click started.
    let mut e = engine_for("txqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("W9XYZ K1ABC FN42"));
    e.halt_tx();
    for slot in [4, 6, 8] {
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ -07")], slot);
        assert_eq!(
            keyed(&mut e, slot),
            None,
            "nothing after Halt (slot {slot})"
        );
    }
    // The double-click arms TX only where the operator lets it.
    let mut e = engine_for("txqp", MY_GRID);
    let mut s = e.settings().clone();
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(!e.tx_enabled, "TX stays off");
    assert_eq!(keyed(&mut e, 2), None, "and nothing keys");
}

/// Where a running contest's own FT8/FT4 messages do not apply (the contest switch off, FT2, a
/// contest whose rules ban FT8), Call CQ is the plain CQ run it always was. And like any Call CQ
/// it clears a directed-CQ token the Tx6 row left, so no later run inherits "CQ DX".
#[test]
fn call_cq_outside_the_contests_messages_is_a_plain_run() {
    let plain = |e: &mut Engine, what: &str| {
        e.start_running().expect(what);
        assert!(
            matches!(&e.mode, Mode::Qso { running: true, .. }),
            "{what}: QSO mode"
        );
        assert_eq!(keyed(e, 0).as_deref(), Some("CQ K1ABC FN42"), "{what}");
    };
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    plain(&mut e, "the contest switch off");
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft2);
    plain(&mut e, "FT2");
    let mut e = engine_for("ilqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    plain(&mut e, "a contest that bans FT8");
    // The token: Tx6's "CQ DX", then Call CQ in the contest, then the contest switched off.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.start_cq(Some("DX")).expect("Tx6");
    e.start_running().expect("Call CQ");
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    e.set_mode("qso-run").expect("a run");
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("CQ K1ABC FN42"),
        "no CQ DX left behind"
    );
}

/// The double-click follows the contest that is running: a Save that moves the pick off the
/// contest running switches to the new one, and a double-click on a CQ then works the station in
/// that contest's S&P, in its messages.
#[test]
fn a_double_click_follows_the_contest_a_save_switched_to() {
    // From a contest whose rules ban FT8, where the click would work the station outside it.
    let mut e = engine_for("ilqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    let mut s = e.settings().clone();
    s.fd_event = "arrlvhf_jan".into();
    e.apply_settings(s);
    assert!(
        matches!(&e.mode, Mode::FieldDay { station, .. }
            if station.log.session.event_id == "arrlvhf_jan"),
        "the Save switched the running contest"
    );
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(
        matches!(e.mode, Mode::FieldDay { running: false, .. }),
        "in the running contest's S&P"
    );
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("W9XYZ K1ABC FN42"));
}

/// A double-click carries the decode's own sender; a click naming one station on another's CQ
/// is not that station's CQ, and works the named station outside the contest, as before. And a
/// deferred "disable TX after 73" left from an earlier contact never disarms the station the
/// double-click is working.
#[test]
fn the_double_click_works_only_the_cqs_own_station_and_keeps_tx() {
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    e.work_station("K2DEF", None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert!(
        matches!(&e.mode, Mode::Qso { station, .. } if station.dxcall.as_deref() == Some("K2DEF")),
        "the named station, outside the contest"
    );
    // With "Double-click on call sets Tx enable" off, the click arms nothing itself (arming TX
    // would cancel the disable on its own): the operator armed TX, and the earlier contact's
    // disable is still to be carried out.
    let mut e = engine_for("arrlfd", MY_GRID);
    let mut s = e.settings().clone();
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_tx_enabled(true);
    e.pending_tx_disable = true;
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(
        !e.pending_tx_disable,
        "the earlier contact's disable is cancelled"
    );
    assert!(e.tx_enabled, "TX stays armed for the station being worked");
}

/// Whether the runner sends the 73 follows the tier it is on now: a run started on FT1 and
/// moved to FT8 answers the caller's RR73 with 73; a run moved from FT8 to FT1 does not.
#[test]
fn the_runners_73_follows_the_tier_it_is_on() {
    for (from, to, want_73) in [
        (Tier::TempoFast, Tier::Ft8, true),
        (Tier::Ft8, Tier::TempoFast, false),
    ] {
        let what = format!("{} to {}", tier_name(from), tier_name(to));
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(from);
        e.set_mode("fieldday-run").expect("the run starts");
        e.set_tier(to);
        e.set_tx_enabled(true); // a mode switch that moves the dial turns TX off, as WSJT-X does
        let cq = keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
        keyed(&mut e, 2).expect("the rogered exchange");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 4);
        let after = keyed(&mut e, 4);
        if want_73 {
            assert_eq!(after.as_deref(), Some("W9XYZ K1ABC 73"), "{what}");
        } else {
            assert_eq!(after, Some(cq), "{what}: straight back to CQ");
        }
    }
}

/// ⛔ Call CQ into a contest's Running needs a valid callsign, as the Contest screen's Running
/// does (the `set_mode` command's identity check): refused with the reason, and nothing changes.
#[test]
fn call_cq_without_a_callsign_is_refused_as_the_screens_running_is() {
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    let mut s = e.settings().clone();
    s.mycall = String::new();
    e.apply_settings(s);
    let r = e.start_running();
    assert!(r.is_err(), "no callsign: {r:?}");
    assert!(listening(&e), "still the contest's S&P, listening");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⭐ The contest's S&P answers each CQ it picks on the period opposite the CQ's, as WSJT-X takes
/// Tx 1st from the CQ it answers: with Tx 1st on the runner's own period, the answer goes out
/// while the runner listens, never over its next CQ. On each tier a contest runs on, for a CQ
/// heard in either period.
#[test]
fn the_contests_sp_answers_each_cq_on_the_period_opposite_it() {
    for (contest, tier, answer) in [
        ("arrlfd", Tier::Ft8, "W9XYZ K1ABC 2B EMA"),
        ("arrlvhf_jan", Tier::Ft8, "W9XYZ K1ABC FN42"),
        ("txqp", Tier::Ft4, "W9XYZ K1ABC FN42"),
        ("arrlfd", Tier::TempoFast, "W9XYZ K1ABC 2B EMA"),
    ] {
        for cq_slot in [1, 2] {
            let mut e = engine_for(contest, MY_GRID);
            let mut s = e.settings().clone();
            s.tx_even = cq_slot % 2 == 0;
            e.apply_settings(s);
            e.set_tier(tier);
            e.set_mode("fieldday-sp").expect("the contest's S&P");
            e.set_tx_enabled(true);
            let cq = Partner::new(act_of(contest), true)
                .transmit()
                .expect("the runner's CQ");
            // Heard in period `cq_slot`, decoded at the boundary after it.
            e.ingest_decodes_for_test(&[heard(&cq)], cq_slot + 1);
            let first = keyed(&mut e, cq_slot + 1);
            let next = keyed(&mut e, cq_slot + 2);
            assert_eq!(
                (first.as_deref(), next.as_deref()),
                (Some(answer), None),
                "{contest} {}: the answer to a CQ heard in period {cq_slot}",
                tier_name(tier)
            );
        }
    }
}

/// ⭐ "Disable Tx after sending 73" turns TX off after a contest contact the operator picked with a
/// double-click on its CQ, once its last over has gone out, as WSJT-X turns it off after a
/// double-clicked contact's RR73 or 73; the station stays in the contest's S&P, listening. With
/// the setting off TX stays on, and S&P's own pounces keep TX on to work the next CQ they hear.
#[test]
fn tx_goes_off_after_a_double_clicked_contest_contact() {
    let listening = "FieldDay running=false state=Listening";
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        let s = scene(
            contest,
            Tier::Ft8,
            Role::DoubleClick,
            act_of(contest),
            MY_GRID,
        );
        assert_eq!(
            (s.logged.len(), s.tx, s.end.as_str()),
            (1, false, listening),
            "{contest}: TX off after the double-clicked contact, still in S&P"
        );
        let mut e = engine_for(contest, MY_GRID);
        let mut st = e.settings().clone();
        st.disable_tx_after_73 = false;
        e.apply_settings(st);
        let s = play(e, Tier::Ft8, Role::DoubleClick, act_of(contest));
        assert_eq!(
            (s.logged.len(), s.tx, s.end.as_str()),
            (1, true, listening),
            "{contest}: with the setting off TX stays on"
        );
        let s = scene(
            contest,
            Tier::Ft8,
            Role::SearchAndPounce,
            act_of(contest),
            MY_GRID,
        );
        assert_eq!(
            (s.logged.len(), s.tx, s.end.as_str()),
            (1, true, listening),
            "{contest}: S&P's own pounce keeps TX on"
        );
    }
}

/// ⭐ NO FT MODE KEYS A CW ID, ON ANY PATH. With "CW ID after 73" on, no scene keys one: every
/// contest the picker offers on FT8 and FT4, and two grid contests and a state QSO party on FT1,
/// DX1 and FT2, through the Contest screen's Running and S&P and every gesture of the Operate
/// cockpit's. WSJT-X 3.0.2 clears the ID on every GUI tick in FT8 and FT4
/// (`widgets/mainwindow.cpp:7673`), whatever the activity; FT2, FT1 and DX1 do as FT8 does. The
/// gestures that leave the contest close an S&P contact with this station's 73 in QSO mode, the
/// over the ID followed. The control: the same gesture on Q65, which keeps the ID, keys one.
#[test]
fn no_ft_mode_keys_a_cw_id_on_any_path() {
    let roles = [
        Role::Running,
        Role::SearchAndPounce,
        Role::DoubleClick,
        Role::ClickFromSp,
        Role::ClickFromRun,
        Role::ClickFromQso,
        Role::CallCq,
        Role::CallCqFromQso,
        Role::StripSpFromRun,
        Role::StripSpFromQso,
    ];
    let cw_ids = |contest: &str, tier: Tier, role: Role| {
        let mut e = engine_for(contest, MY_GRID);
        let mut st = e.settings().clone();
        st.cw_id_after_73 = true;
        e.apply_settings(st);
        play(e, tier, role, act_of(contest)).cw_ids
    };
    let mut with_id = Vec::new();
    let tiers = [
        (&CONTESTS[..], Tier::Ft8),
        (&CONTESTS[..], Tier::Ft4),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::TempoFast),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::TempoDeep),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::Ft2),
    ];
    for (contests, tier) in tiers {
        for &contest in contests {
            for role in roles {
                let ids = cw_ids(contest, tier, role);
                if !ids.is_empty() {
                    with_id.push(format!("{contest:?} {} {role:?}: {ids:?}", tier_name(tier)));
                }
            }
        }
    }
    assert_eq!(with_id, Vec::<String>::new(), "CW IDs keyed in an FT mode");
    assert_eq!(
        cw_ids("txqp", Tier::Q65, Role::ClickFromSp),
        [7],
        "control: Q65 keys its CW ID after this station's 73"
    );
}

/// ⭐ The Operate strip's S&P hands the station to the running contest's S&P, its messages and
/// its log, from the contest's Running and from FT8 QSO mode, as the Contest screen's S&P does,
/// opening the contest's session when it enters the contest from outside it. Outside the
/// contest's own messages (Contest mode off, or FT1) it is FT8 QSO mode's S&P as before, and a
/// contest that cannot start refuses it and nothing changes.
#[test]
fn the_strips_sp_is_the_running_contests_sp() {
    let contest_sp = "FieldDay running=false state=Listening";
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        for (from, opens) in [("fieldday-run", false), ("qso-monitor", true)] {
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(Tier::Ft8);
            e.set_mode(from).expect(from);
            let opened = e.start_search_and_pounce_opens_session();
            let r = e.start_search_and_pounce();
            assert_eq!(
                (r, opened, mode_line(&e)),
                (Ok(()), opens, contest_sp.to_string()),
                "{contest} from {from}"
            );
        }
    }
    let monitor = "Qso running=false state=Listening";
    for (on, tier) in [(false, Tier::Ft8), (true, Tier::TempoFast)] {
        let mut e = engine_for("arrlfd", MY_GRID);
        let mut s = e.settings().clone();
        s.fd_active = on;
        e.apply_settings(s);
        e.set_tier(tier);
        e.set_mode("qso-run").expect("a plain run");
        let opened = e.start_search_and_pounce_opens_session();
        let r = e.start_search_and_pounce();
        assert_eq!(
            (r, opened, mode_line(&e)),
            (Ok(()), false, monitor.to_string()),
            "Contest mode {on} on {}",
            tier_name(tier)
        );
    }
    let mut e = contest_that_cannot_start();
    let r = e.start_search_and_pounce();
    assert!(r.is_err(), "no class: {r:?}");
    assert_eq!(mode_line(&e), monitor, "still FT8 QSO mode");
}
