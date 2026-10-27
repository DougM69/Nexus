//! The radio's PA (finals) temperature and its SSB transmit-bandwidth step, per radio, from each
//! maker's own CAT reference — and the radio loop's small driver that reads and writes them.
//!
//! Neither has a Hamlib verb that covers the fleet, so each radio that has one is named here
//! with its command, and every other radio is asked NOTHING: no poll is ever issued to a radio
//! this table does not name.
//!
//! ## What the vendor references say (PDF page numbers)
//!
//! **Temperature.** Only the FTDX101D/MP among Yaesus reports one: `RM9;` answers
//! `RM9 P2P2P2 P3P3P3;`, P1 `9: TEMP`, P2 `0 - 255` (FTDX101MP/D CAT Operation Reference,
//! 2308-L, p. 21). The manual gives NO degree scale for it, so Nexus shows the radio's own meter
//! position and never invents degrees (Hamlib 4.7.1 maps it `i / 255 * 100` °C, its own guess,
//! `newcat.c:5968`). The FTDX10 (2308-F p. 21), FT-710 (2306-C p. 20), FT-991A (p. 16), FTX-1
//! (2508-C p. 24) and FT-891 (1909-C p. 17) list no TEMP meter type at all. No Icom CI-V
//! reference in the fleet has a temperature in its `15` meter family. A FlexRadio's comes from
//! its own meter stream (`flexspectrum`), in real °C.
//!
//! **SSB transmit bandwidth.** Icom: `16 58`, `00` WIDE, `01` MID, `02` NAR, the same on the
//! IC-7300, IC-7300MK2, IC-7610, IC-705, IC-9700, IC-905 and IC-7760 (each radio's CI-V
//! reference; served by the native daemon, `civ::broker`). Yaesu: the five-step "TX BPF SEL"
//! menu item, read `EX<item>;` → `EX<item><n>;`, set `EX<item><n>;`: FTDX101 `EX010110`
//! (p. 10-11), FTDX10 `EX010112` (p. 10-11), FT-710 `EX010113` (p. 11), FTX-1 `EX010112`
//! (p. 11-12), FT-991A `EX110` ("SSB TX BPF", p. 8-9), FT-891 `EX1109` ("SSB TX BPF", p. 9).
//! The steps are named by their passband edges as each manual prints them.
//!
//! ⛔ **THE YAESU MENU IS PARKED** ([`YAESU_TX_BPF_LIVE`] is `false`): nothing above is read or
//! written on a Yaesu, and no Yaesu is offered the chip. Hamlib's rigctld passes a raw set
//! through `w` and then waits for a reply a Yaesu never sends, holding the radio's serial port
//! for the model's whole read timeout (2 s on the FTDX101/FTDX10/FT-991/FT-891, 1 s on the
//! FT-710; measured 2.0 s on rigctld with a fake FTDX101). Every command behind it — the dial
//! probe, a key, an UNKEY — waits that long. The table stays, behind the gate, for the day a
//! write form that does not stall is found and measured on a radio.
//!
//! **Which Yaesu it is comes from the radio, not the setting.** Hamlib model 1035 is the FT-991
//! AND the FT-991A, and the two menus differ: the FT-991A's item 110 is SSB TX BPF, the plain
//! FT-991's 110 is SSB PTT SELECT (`0` DAKY, `1` RTS, `2` DTR; its TX BPF is 112). A write
//! meant for one would re-route the other's PTT. So every Yaesu row here is chosen by the
//! radio's own answer to `ID;` (each reference's ID table: FTDX101D `0681`, FTDX101MP `0682`,
//! FTDX10 `0761`, FT-710 `0800`, FTX-1 `0840`, FT-991A `0670`, FT-891 `0650`; the FT-991 is
//! `0570` and is offered nothing). No ID match: no read and no control.
//!
//! None of these references says whether a write is safe while transmitting, so Nexus never
//! writes the bandwidth while the radio is keyed (the engine refuses the pick, and the driver
//! checks again). Everything here is NEEDS-BENCH: a vendor table, not a radio.

#![cfg_attr(not(feature = "device"), allow(dead_code))] // the driver's caller lives in service.rs

use std::sync::{Arc, Mutex};

use tempo_app::engine::{engine_lock, Engine, EngineGuard};

use crate::rig::Rig;

/// ⛔ The Yaesu TX BPF menu is parked: no Yaesu is offered the chip, and nothing reads or writes
/// its menu item, until a write form that does not hold rigctld for the radio's read timeout is
/// found AND measured on a radio (see the module note). Every Yaesu bandwidth path below sits
/// behind this one switch.
pub(crate) const YAESU_TX_BPF_LIVE: bool = false;

/// Hamlib model numbers (`include/hamlib/riglist.h`, 4.7.1). Only which Yaesus are ASKED their
/// `ID;`: what each is offered is chosen by its answer ([`yaesu_by_id`]).
const FT991: u32 = 1035;
const FT891: u32 = 1036;
const FTDX101D: u32 = 1040;
const FTDX10: u32 = 1042;
const FTDX101MP: u32 = 1044;
const FT710: u32 = 1049;
const FTX1: u32 = 1051;

/// The Icom SSB transmit-bandwidth steps, in `16 58` byte order.
pub(crate) const ICOM_TBW_STEPS: &[&str] = &["WIDE", "MID", "NAR"];
/// The Yaesu five-step TX BPF, in menu-value order, as the FTDX10/FTDX101/FT-710/FTX-1 print it.
const YAESU_BPF_STEPS: &[&str] = &["50-3050", "100-2900", "200-2800", "300-2700", "400-2600"];
/// The FT-991A's: its widest step is 50-3000 (CAT reference, menu 110).
const FT991_BPF_STEPS: &[&str] = &["50-3000", "100-2900", "200-2800", "300-2700", "400-2600"];
/// The FT-891's: its widest step is 100-3000.
const FT891_BPF_STEPS: &[&str] = &["100-3000", "100-2900", "200-2800", "300-2700", "400-2600"];

/// The rigctld level token the native Icom daemon answers `16 58` on. Nexus's own: Hamlib has no
/// verb for it, and only the native daemon is ever asked.
pub(crate) const ICOM_TBW_TOKEN: &str = "SSB_TX_BW";

/// Which CAT link the radio is on, as far as these two readings care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Link {
    /// Nexus's own CI-V daemon (USB or the Icom network).
    IcomNative,
    /// Hamlib's rigctld, which passes a raw Yaesu string through with `w`.
    Hamlib,
    /// Anything else (OmniRig, a Flex or TCI session, no CAT): nothing here is asked.
    Other,
}

/// How a radio's transmit bandwidth is read and written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BwPath {
    /// `l SSB_TX_BW` / `L SSB_TX_BW <n>` on the native daemon.
    Icom,
    /// The Yaesu menu item `EX<item>`, raw through Hamlib.
    Yaesu(&'static str),
}

/// One radio's transmit-bandwidth control: how to reach it and its steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BwSpec {
    pub path: BwPath,
    pub steps: &'static [&'static str],
}

/// What one Yaesu, known by its `ID;` answer, is asked: its TX BPF menu item and steps, and
/// whether it has the `RM9` TEMP meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct YaesuRow {
    /// The TX BPF menu item and its steps, as the radio's own reference prints them.
    pub bpf: (&'static str, &'static [&'static str]),
    /// The FTDX101's `RM9` TEMP meter (the only Yaesu reference that lists one).
    pub rm9_temp: bool,
}

/// The Yaesu that answered `ID;` with `id` (four digits), from each radio's CAT reference ID
/// table, or `None`: then that radio is asked nothing more.
pub(crate) fn yaesu_by_id(id: &str) -> Option<YaesuRow> {
    let row = |item, steps, rm9_temp| {
        Some(YaesuRow {
            bpf: (item, steps),
            rm9_temp,
        })
    };
    match id {
        // FTDX101D, FTDX101MP (2308-L p. 14).
        "0681" | "0682" => row("EX010110", YAESU_BPF_STEPS, true),
        // FTDX10 (2308-F).
        "0761" => row("EX010112", YAESU_BPF_STEPS, false),
        // FT-710 (2306-C, "0800 (Fixed)").
        "0800" => row("EX010113", YAESU_BPF_STEPS, false),
        // FTX-1 (2508-C, "0840 (Fixed)").
        "0840" => row("EX010112", YAESU_BPF_STEPS, false),
        // FT-991A (its CAT reference: "0670: FT-991A"; menu 110 SSB TX BPF).
        "0670" => row("EX110", FT991_BPF_STEPS, false),
        // FT-891 (1909-C, "0650: FT-891").
        "0650" => row("EX1109", FT891_BPF_STEPS, false),
        // ⛔ The plain FT-991 ("0570: FT-991") is NOT the FT-991A: its menu 110 is SSB PTT
        // SELECT (DAKY/RTS/DTR). It is offered nothing, and anything else unknown likewise.
        _ => None,
    }
}

/// The four ID digits in a Yaesu's answer to `ID;` (`ID0681;`), or `None` for anything else.
pub(crate) fn yaesu_id_from_reply(reply: &str) -> Option<&str> {
    let id = reply.strip_prefix("ID")?.strip_suffix(';')?;
    (id.len() == 4 && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The transmit-bandwidth control a Yaesu known by its ID would have — `None` while the Yaesu
/// menu is parked ([`YAESU_TX_BPF_LIVE`]), whatever the ID.
pub(crate) fn yaesu_bw_spec(row: YaesuRow) -> Option<BwSpec> {
    YAESU_TX_BPF_LIVE.then_some(BwSpec {
        path: BwPath::Yaesu(row.bpf.0),
        steps: row.bpf.1,
    })
}

/// Is this Hamlib model one Nexus asks `ID;` of? Only where an answer could bring a row to life:
/// the FTDX101s for the TEMP meter, and every table Yaesu once the menu is live. The answer, not
/// the model, then decides what is asked ([`yaesu_by_id`]).
fn asks_yaesu_id(model: u32) -> bool {
    matches!(model, FTDX101D | FTDX101MP)
        || (YAESU_TX_BPF_LIVE && matches!(model, FTDX10 | FT710 | FTX1 | FT991 | FT891))
}

/// This radio's transmit-bandwidth control on this link, before any answer is in: the native
/// Icom link's `16 58`, or `None` (a Yaesu's comes from its `ID;` answer; anything else is
/// asked nothing).
pub(crate) fn tx_bw_spec(link: Link) -> Option<BwSpec> {
    (link == Link::IcomNative).then_some(BwSpec {
        path: BwPath::Icom,
        steps: ICOM_TBW_STEPS,
    })
}

/// The FTDX101's TEMP meter position, 0.0 to 1.0, from its `RM9` answer: `RM9`, P2 (three
/// digits, 0-255) and P3 (three digits, `000`). Anything else is a missed read, never a guess.
pub(crate) fn temp_meter_from_rm9(reply: &str) -> Option<f32> {
    let digits = reply.strip_prefix("RM9")?.strip_suffix(';')?;
    if !matches!(digits.len(), 3 | 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let p2: u16 = digits[..3].parse().ok()?;
    (p2 <= 255).then(|| f32::from(p2) / 255.0)
}

/// The Yaesu read of menu `item`: `EX010112;`.
pub(crate) fn yaesu_bw_read(item: &str) -> String {
    format!("{item};")
}

/// The Yaesu write of step `idx` to menu `item`: `EX0101123;`.
pub(crate) fn yaesu_bw_write(item: &str, idx: usize) -> String {
    format!("{item}{idx};")
}

/// The step index in a Yaesu answer to [`yaesu_bw_read`]: the item, ONE digit inside the step
/// list, `;`. `?;` or anything else is a missed read.
pub(crate) fn yaesu_bw_parse(item: &str, steps: usize, reply: &str) -> Option<usize> {
    let v = reply.strip_prefix(item)?.strip_suffix(';')?;
    if v.len() != 1 {
        return None;
    }
    let i = usize::from(v.as_bytes()[0].checked_sub(b'0')?);
    (i < steps).then_some(i)
}

/// How many heavy polls between reads: a temperature moves over minutes, and a menu setting
/// only when someone changes it. One read on each turn, alternating between the two.
pub(crate) const READ_EVERY_POLLS: u32 = 8;
/// Consecutive misses before a reading is given up for this connection — the loop's ladder.
const GIVE_UP_AFTER: u8 = 3;

/// One reading's capability ladder: unknown, proven, or given up after [`GIVE_UP_AFTER`] misses.
#[derive(Debug, Default, Clone, Copy)]
struct Ladder {
    given_up: bool,
    misses: u8,
}

impl Ladder {
    fn note(&mut self, ok: bool) {
        if ok {
            self.misses = 0;
        } else {
            self.misses = self.misses.saturating_add(1);
            self.given_up = self.misses >= GIVE_UP_AFTER;
        }
    }
}

/// The radio loop's driver: configured per radio, reads on the heavy poll, writes between overs.
#[derive(Debug, Default)]
pub(crate) struct TxExtrasPoll {
    /// The radio and link it is configured for; a change resets everything.
    for_radio: Option<(u32, Link)>,
    /// A Yaesu whose `ID;` answer is still owed: nothing else is asked until it is in.
    id_due: bool,
    temp: Option<&'static str>,
    bw: Option<BwSpec>,
    id_ladder: Ladder,
    temp_ladder: Ladder,
    bw_ladder: Ladder,
    /// Which reading this turn takes (the two alternate).
    turn: u8,
    /// A write went out: read the bandwidth back on the next turn.
    readback_due: bool,
}

impl TxExtrasPoll {
    /// Point the driver at this radio. A different radio or link starts afresh, and the
    /// engine forgets what the last one reported.
    pub(crate) fn configure(&mut self, model: u32, link: Link, engine: &Arc<Mutex<Engine>>) {
        let fresh = self.for_radio != Some((model, link));
        let mut eng = lock(engine);
        if fresh {
            eng.clear_rig_cat_extras();
        }
        // The meter's PROC switch only where its write is the vendor's own command: the
        // native Icom link's `16 44` (00 OFF, 01 ON). Hamlib's Yaesu `PR` mapping disagrees
        // with the Yaesu manuals, so a Yaesu is not offered it; nor is any other Hamlib radio.
        // Said on EVERY call, not only on a change: the engine forgets it on its own too (a
        // Remote radio selection clears every extra), and this radio and link are unchanged then.
        eng.observe_rig_proc_switch(link == Link::IcomNative);
        drop(eng);
        if fresh {
            *self = Self {
                for_radio: Some((model, link)),
                id_due: link == Link::Hamlib && asks_yaesu_id(model),
                bw: tx_bw_spec(link),
                ..Self::default()
            };
        }
    }

    /// Forget the radio (a handoff or a CAT drop): the next `configure` starts afresh, so a
    /// given-up reading is asked again of a radio that may now answer.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Has this radio anything to read? `false` = the loop owes it no turn at all.
    pub(crate) fn has_reads(&self) -> bool {
        (self.id_due && !self.id_ladder.given_up)
            || (self.temp.is_some() && !self.temp_ladder.given_up)
            || (self.bw.is_some() && !self.bw_ladder.given_up)
    }

    /// Is a read-back owed now (after a write), ahead of the ordinary cadence?
    pub(crate) fn readback_due(&self) -> bool {
        self.readback_due
    }

    /// One read, on the heavy poll's turn: the bandwidth if a write is waiting for its
    /// read-back, else the temperature and the bandwidth alternately. A read never writes.
    pub(crate) fn read_one(&mut self, rig: &mut Rig, engine: &Arc<Mutex<Engine>>) {
        if self.id_due {
            // Which Yaesu this is, first: nothing else is asked until it has said.
            let reply = rig.send_raw("ID;");
            let id = reply.as_deref().and_then(yaesu_id_from_reply);
            self.id_ladder.note(id.is_some());
            if let Some(id) = id {
                self.id_due = false;
                if let Some(row) = yaesu_by_id(id) {
                    self.temp = row.rm9_temp.then_some("RM9;");
                    self.bw = yaesu_bw_spec(row);
                }
            }
            return;
        }
        let temp_live = self.temp.is_some() && !self.temp_ladder.given_up;
        let bw_live = self.bw.is_some() && !self.bw_ladder.given_up;
        let read_bw = bw_live && (self.readback_due || !temp_live || self.turn % 2 == 1);
        self.turn = self.turn.wrapping_add(1);
        if read_bw {
            self.readback_due = false;
            let spec = self.bw.expect("bw_live");
            let got = read_bw_index(rig, spec);
            self.bw_ladder.note(got.is_some());
            if got.is_some() || !self.bw_ladder.given_up {
                lock(engine).observe_rig_tx_bw(spec.steps, got);
            } else {
                lock(engine).observe_rig_tx_bw(&[], None);
            }
        } else if temp_live {
            let query = self.temp.expect("temp_live");
            let got = rig.send_raw(query).as_deref().and_then(temp_meter_from_rm9);
            self.temp_ladder.note(got.is_some());
            if let Some(frac) = got {
                lock(engine).observe_rig_pa_temp_meter(frac);
            } else if self.temp_ladder.given_up {
                // Given up: the last reading goes too, rather than stand on screen for good.
                lock(engine).forget_rig_pa_temp_meter();
            }
        }
    }

    /// Write the operator's pending transmit-bandwidth pick, if any — never while `keyed`.
    /// A pick taken while keyed is dropped (the engine refused it already; this is the second
    /// check) and the radio is read back so the selector shows what the radio holds.
    pub(crate) fn write_pending(
        &mut self,
        rig: &mut Rig,
        engine: &Arc<Mutex<Engine>>,
        keyed: bool,
    ) {
        let Some(idx) = lock(engine).take_tx_bw_request() else {
            return;
        };
        let Some(spec) = self.bw.filter(|_| !self.bw_ladder.given_up) else {
            return;
        };
        if !keyed && idx < spec.steps.len() {
            match spec.path {
                BwPath::Icom => {
                    // A refusal needs no handling of its own: the read-back shows the radio's step.
                    let _ = rig.set_ssb_tx_bw(idx as u8);
                }
                // ⛔ PARKED: unreachable while `YAESU_TX_BPF_LIVE` is false (no Yaesu spec is ever
                // built then), and gated here again. Through rigctld this raw set holds the
                // radio's port for its whole read timeout (module note): do not open it without
                // a write form that does not stall, measured on a radio.
                BwPath::Yaesu(item) => {
                    if YAESU_TX_BPF_LIVE {
                        rig.send_raw_set(&yaesu_bw_write(item, idx));
                    }
                }
            }
        }
        self.readback_due = true;
    }
}

/// The transmit-bandwidth step the radio reports, by index into `spec.steps`.
fn read_bw_index(rig: &mut Rig, spec: BwSpec) -> Option<usize> {
    match spec.path {
        BwPath::Icom => rig
            .read_meter_f32(ICOM_TBW_TOKEN)
            .filter(|v| v.fract() == 0.0)
            .map(|v| v as usize)
            .filter(|&i| i < spec.steps.len()),
        BwPath::Yaesu(item) => rig
            .send_raw(&yaesu_bw_read(item))
            .and_then(|r| yaesu_bw_parse(item, spec.steps.len(), &r)),
    }
}

fn lock(engine: &Arc<Mutex<Engine>>) -> EngineGuard<'_> {
    engine_lock(engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ID each Yaesu CAT reference prints for its own radio.
    const IDS: [(&str, &str); 8] = [
        ("FTDX101D", "0681"),
        ("FTDX101MP", "0682"),
        ("FTDX10", "0761"),
        ("FT-710", "0800"),
        ("FTX-1", "0840"),
        ("FT-991A", "0670"),
        ("FT-891", "0650"),
        ("FT-991", "0570"),
    ];

    #[test]
    fn only_an_ftdx101_by_its_own_id_is_read_for_a_temperature() {
        for (name, id) in IDS {
            let want = matches!(id, "0681" | "0682");
            assert_eq!(
                yaesu_by_id(id).is_some_and(|r| r.rm9_temp),
                want,
                "{name} ({id})"
            );
        }
        assert_eq!(yaesu_by_id("0000"), None);
    }

    /// ⛔ C1: Hamlib model 1035 is the FT-991 AND the FT-991A, and menu 110 means SSB TX BPF on
    /// the A but SSB PTT SELECT on the plain FT-991. The radio's ID decides: the A gets its
    /// `EX110` row, the plain FT-991 nothing at all.
    #[test]
    fn an_ft991_id_gets_no_row_and_an_ft991a_id_gets_ex110() {
        assert_eq!(yaesu_by_id("0570"), None, "the plain FT-991");
        let a = yaesu_by_id("0670").expect("the FT-991A");
        assert_eq!(a.bpf.0, "EX110");
        assert_eq!(a.bpf.1[0], "50-3000");
        // No row anywhere names an item that is a PTT setting on the plain FT-991.
        for (name, id) in IDS {
            if let Some(r) = yaesu_by_id(id) {
                assert!(
                    id != "0570" && (r.bpf.0 != "EX110" || id == "0670"),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn the_id_is_four_digits_between_id_and_the_terminator() {
        assert_eq!(yaesu_id_from_reply("ID0681;"), Some("0681"));
        for bad in ["?;", "ID068;", "ID06811;", "ID0681", "IDx681;", "FA0681;"] {
            assert_eq!(yaesu_id_from_reply(bad), None, "{bad}");
        }
    }

    /// ⛔ H1: the Yaesu menu is parked. Whatever a Yaesu answers, no bandwidth control is built
    /// for it, so nothing reads or writes its menu item.
    #[test]
    fn the_yaesu_menu_is_parked_for_every_id() {
        const { assert!(!YAESU_TX_BPF_LIVE) };
        for (name, id) in IDS {
            assert_eq!(yaesu_by_id(id).and_then(yaesu_bw_spec), None, "{name}");
        }
        assert_eq!(tx_bw_spec(Link::Hamlib), None);
        assert_eq!(tx_bw_spec(Link::Other), None);
        // Only the FTDX101s are asked their ID while parked (for the TEMP meter).
        for m in [FTDX101D, FTDX101MP] {
            assert!(asks_yaesu_id(m), "model {m}");
        }
        for m in [FTDX10, FT710, FTX1, FT991, FT891, 2014, 3073] {
            assert!(!asks_yaesu_id(m), "model {m}");
        }
    }

    #[test]
    fn the_rm9_answer_is_the_meter_position_and_nothing_else_is_believed() {
        assert_eq!(temp_meter_from_rm9("RM9000000;"), Some(0.0));
        assert_eq!(temp_meter_from_rm9("RM9255000;"), Some(1.0));
        assert_eq!(temp_meter_from_rm9("RM9102;"), Some(0.4));
        for bad in [
            "?;",
            "RM9256000;",
            "RM4102000;",
            "RM9xyz000;",
            "RM9102000",
            "RM91020;",
        ] {
            assert_eq!(temp_meter_from_rm9(bad), None, "{bad}");
        }
    }

    #[test]
    fn each_yaesu_id_names_its_own_tx_bpf_menu_item_and_steps() {
        let item = |id| yaesu_by_id(id).map(|r| r.bpf.0);
        assert_eq!(item("0681"), Some("EX010110"));
        assert_eq!(item("0682"), Some("EX010110"));
        assert_eq!(item("0761"), Some("EX010112"));
        assert_eq!(item("0800"), Some("EX010113"));
        assert_eq!(item("0840"), Some("EX010112"));
        assert_eq!(item("0670"), Some("EX110"));
        assert_eq!(item("0650"), Some("EX1109"));
        let steps = |id| yaesu_by_id(id).unwrap().bpf.1;
        assert_eq!(steps("0761")[0], "50-3050");
        assert_eq!(steps("0670")[0], "50-3000");
        assert_eq!(steps("0650")[0], "100-3000");
        for (name, id) in IDS.into_iter().filter(|(_, id)| *id != "0570") {
            assert_eq!(steps(id).len(), 5, "{name}");
            assert_eq!(steps(id)[4], "400-2600", "{name}");
        }
    }

    #[test]
    fn the_icom_native_link_offers_wide_mid_nar() {
        let s = tx_bw_spec(Link::IcomNative).unwrap();
        assert_eq!(s.path, BwPath::Icom);
        assert_eq!(s.steps, &["WIDE", "MID", "NAR"]);
    }

    #[test]
    fn yaesu_menu_strings_and_answers() {
        assert_eq!(yaesu_bw_read("EX010112"), "EX010112;");
        assert_eq!(yaesu_bw_write("EX010112", 3), "EX0101123;");
        assert_eq!(yaesu_bw_write("EX110", 1), "EX1101;");
        assert_eq!(yaesu_bw_write("EX1109", 4), "EX11094;");
        assert_eq!(yaesu_bw_parse("EX010112", 5, "EX0101123;"), Some(3));
        assert_eq!(yaesu_bw_parse("EX110", 5, "EX1104;"), Some(4));
        for bad in [
            "?;",
            "EX0101125;",
            "EX01011;",
            "EX01011234;",
            "EX010113 3;",
            "EX010112a;",
        ] {
            assert_eq!(yaesu_bw_parse("EX010112", 5, bad), None, "{bad}");
        }
    }

    #[test]
    fn a_reading_is_given_up_after_three_misses_and_a_new_radio_starts_afresh() {
        let e = Arc::new(Mutex::new(Engine::new("K1ABC", "EN50", 0)));
        let mut p = TxExtrasPoll::default();
        p.configure(FTDX101D, Link::Hamlib, &e);
        assert!(p.has_reads(), "its ID is owed");
        for _ in 0..3 {
            p.id_ladder.note(false);
        }
        assert!(!p.has_reads(), "given up: the loop owes it no turn");
        p.configure(FTDX101D, Link::Hamlib, &e);
        assert!(!p.has_reads(), "the same radio stays given up");
        p.configure(FTDX101MP, Link::Hamlib, &e);
        assert!(p.has_reads(), "another radio is asked again");
        p.configure(3073, Link::IcomNative, &e);
        assert!(p.has_reads(), "the native Icom link reads 16 58");
        for _ in 0..3 {
            p.bw_ladder.note(false);
        }
        assert!(!p.has_reads());
        p.reset();
        p.configure(3073, Link::Other, &e);
        assert!(
            !p.has_reads(),
            "nothing to read on a link this table does not name"
        );
    }
}
