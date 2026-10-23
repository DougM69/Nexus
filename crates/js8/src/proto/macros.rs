//! JS8Call's message macros: the 13 `<…>` words a saved message or a configured text (CQ, reply,
//! INFO, STATUS) may carry, replaced with the station's own values when a message is made
//! (`buildMacroValues`, mainwindow.cpp:4848-4888, and `replaceMacros`, mainwindow.h:789-799, read
//! as the spec; the list is the user guide's, JS8Call_User_Guide.md:637-651).
//!
//! CONTRACT. [`values`] answers the (token, value) pairs in JS8Call's map order — a `QMap`, so
//! sorted by token — with `<MYINFO>`, `<MYSTATUS>`, `<MYCQ>`, `<MYHB>` and `<MYREPLY>` each run
//! once through the pairs, in that order and without pruning, as JS8Call's are (so a value that
//! names a token sorted before its own is left with that token, as upstream leaves it).
//! `<CALL>`, `<SNR>` and `<TDELTA>` are there only for a selected call the station has heard, and
//! `<SNR>` only above −31 dB. [`replace`] walks the pairs in order and puts each value, upper-cased,
//! wherever its token stands; with `prune`, every `<…>` left after that is removed. A token is
//! matched whatever its ASCII case: JS8Call's editor has upper-cased the text by then, and matching
//! a lower-case token here gives the same result.
//!
//! WHO CALLS WHICH. The station replaces with `prune` where JS8Call does: the CQ (`sendCQ`,
//! mainwindow.cpp:4534-4541; the repeat calls the same) and the INFO? and STATUS? replies
//! (processCommandActivity.cpp:339-358). The desktop's compose box does the rest
//! (`ui/src/js8Macros.ts`): a button or a saved message fills with `prune`, a send replaces
//! without (`createMessage`, mainwindow.cpp:3436-3440). The two are held to one table of cases,
//! `tests/fixtures/js8-macros.json`, read by this module's tests and by the desktop's.
use crate::proto::frame::format_snr;

/// JS8Call's CQ message (`CQMessage`, Configuration.cpp:2154-2155).
pub const DEFAULT_CQ: &str = "CQ CQ CQ <MYGRID4>";
/// JS8Call's reply message (`Reply`, Configuration.cpp:2156).
pub const DEFAULT_REPLY: &str = "HW CPY?";
/// JS8Call's STATUS (`MyStatus`, Configuration.cpp:2150-2151): what an empty STATUS stands for.
pub const DEFAULT_STATUS: &str = "IDLE <MYIDLE> VERSION <MYVERSION>";
/// JS8Call's heartbeat text (`HBMessage`, Configuration.cpp:2153). Its editor is compiled out
/// upstream and Nexus has none, so `<MYHB>` is always this.
pub const HB_TEXT: &str = "HB <MYGRID4>";
/// What `<MYVERSION>` names: the application, as Nexus's STATUS reply has always said it.
pub const VERSION: &str = "Nexus";

/// The 13 tokens, in JS8Call's map order (sorted, as a `QMap` keeps them).
pub const TOKENS: [&str; 13] = [
    "<CALL>",
    "<MYCALL>",
    "<MYCQ>",
    "<MYGRID12>",
    "<MYGRID4>",
    "<MYHB>",
    "<MYIDLE>",
    "<MYINFO>",
    "<MYREPLY>",
    "<MYSTATUS>",
    "<MYVERSION>",
    "<SNR>",
    "<TDELTA>",
];

/// The selected call, when the station has heard it (`m_callActivity.contains`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selected<'a> {
    pub call: &'a str,
    pub snr_db: i32,
    /// `(int)(1000 * cd.tdrift)`: whole milliseconds, truncated toward zero.
    pub tdelta_ms: i32,
}

/// What the values are made from, as the settings hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MacroInput<'a> {
    pub mycall: &'a str,
    pub grid: &'a str,
    pub info: &'a str,
    /// STATUS as set; empty stands for [`DEFAULT_STATUS`].
    pub status: &'a str,
    pub cq: &'a str,
    pub reply: &'a str,
    /// Whole minutes since the operator last acted.
    pub idle_minutes: u16,
    pub selected: Option<Selected<'a>>,
}

/// The (token, value) pairs, in [`TOKENS`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values(Vec<(&'static str, String)>);

impl Values {
    /// A token's value, if it has one.
    pub fn get(&self, token: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, v)| v.as_str())
    }

    fn set(&mut self, token: &str, value: String) {
        if let Some(slot) = self.0.iter_mut().find(|(t, _)| *t == token) {
            slot.1 = value;
        }
    }
}

/// JS8Call's `since()` of the last activity for a whole-minute idle count, upper-cased, with
/// "NOW" read as "0M" (mainwindow.h:768-779, mainwindow.cpp:4849-4851). The count is whole
/// minutes, so `since()`'s seconds branch never applies.
pub fn idle_since(minutes: u16) -> String {
    let secs = u64::from(minutes) * 60;
    if secs >= 86_400 {
        format!("{}D", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}H", secs / 3_600)
    } else if secs >= 60 {
        format!("{}M", secs / 60)
    } else {
        "0M".to_string()
    }
}

/// `buildMacroValues` for `input`.
pub fn values(input: &MacroInput) -> Values {
    let grid = |n: usize| input.grid.chars().take(n).collect::<String>();
    let status = match input.status.trim() {
        "" => DEFAULT_STATUS,
        s => s,
    };
    let mut pairs: Vec<(&'static str, String)> = vec![
        ("<MYCALL>", input.mycall.to_string()),
        ("<MYCQ>", input.cq.trim().replace("CQCQCQ", "CQ CQ CQ")),
        ("<MYGRID12>", grid(12)),
        ("<MYGRID4>", grid(4)),
        ("<MYHB>", HB_TEXT.to_string()),
        ("<MYIDLE>", idle_since(input.idle_minutes)),
        ("<MYINFO>", input.info.trim().to_string()),
        ("<MYREPLY>", input.reply.trim().to_string()),
        ("<MYSTATUS>", status.to_string()),
        ("<MYVERSION>", VERSION.to_string()),
    ];
    if let Some(sel) = input.selected {
        pairs.push(("<CALL>", sel.call.to_string()));
        pairs.push(("<TDELTA>", format!("{} ms", sel.tdelta_ms)));
        if sel.snr_db > -31 {
            pairs.push(("<SNR>", format_snr(sel.snr_db)));
        }
    }
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut v = Values(pairs);
    // "these macros can have recursive macros" (mainwindow.cpp:4879-4885): each in turn, through
    // the map as it stands, unpruned.
    for token in ["<MYINFO>", "<MYSTATUS>", "<MYCQ>", "<MYHB>", "<MYREPLY>"] {
        let raw = v.get(token).unwrap_or_default().to_string();
        let done = replace(&raw, &v, false);
        v.set(token, done);
    }
    v
}

/// `replaceMacros`: every token in `values` order, each value upper-cased; then, with `prune`,
/// every `<…>` left removed (`[<](?:[^>]+)[>]`).
pub fn replace(text: &str, values: &Values, prune: bool) -> String {
    let mut out = text.to_string();
    for (token, value) in &values.0 {
        out = replace_ignoring_ascii_case(&out, token, &value.to_uppercase());
    }
    if prune {
        out = prune_tokens(&out);
    }
    out
}

/// Every non-overlapping `token` in `text`, found left to right whatever its ASCII case, replaced
/// by `value` (`QString::replace`, as `replaceMacros` calls it). ASCII case-folding moves no byte,
/// so an offset found in the folded copy is an offset in `text`.
fn replace_ignoring_ascii_case(text: &str, token: &str, value: &str) -> String {
    let folded = text.to_ascii_uppercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in folded.match_indices(token) {
        out.push_str(&text[last..at]);
        out.push_str(value);
        last = at + token.len();
    }
    out.push_str(&text[last..]);
    out
}

/// `[<](?:[^>]+)[>]` replaced by nothing: a `<`, one or more characters that are not `>`, and
/// the `>` that ends them.
fn prune_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        match after.find('>') {
            Some(gt) if gt > 0 => rest = &after[gt + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One case of the table the desktop shares (`ui/src/js8Macros.agreement.test.ts`).
    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        input: Input,
        text: String,
        prune: bool,
        expect: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Input {
        mycall: String,
        grid: String,
        info: String,
        status: String,
        cq: String,
        reply: String,
        idle_minutes: u16,
        selected: Option<Sel>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Sel {
        call: String,
        snr_db: i32,
        tdelta_ms: i32,
    }
    #[derive(serde::Deserialize)]
    struct Table {
        cases: Vec<Case>,
    }

    fn table() -> Table {
        serde_json::from_str(include_str!("../../tests/fixtures/js8-macros.json"))
            .expect("the shared macro table parses")
    }

    fn run(c: &Case) -> String {
        let i = &c.input;
        let input = MacroInput {
            mycall: &i.mycall,
            grid: &i.grid,
            info: &i.info,
            status: &i.status,
            cq: &i.cq,
            reply: &i.reply,
            idle_minutes: i.idle_minutes,
            selected: i.selected.as_ref().map(|s| Selected {
                call: &s.call,
                snr_db: s.snr_db,
                tdelta_ms: s.tdelta_ms,
            }),
        };
        replace(&c.text, &values(&input), c.prune)
    }

    #[test]
    fn the_macros_agree_with_the_desktop_on_every_case_in_the_shared_table() {
        let t = table();
        assert!(
            t.cases.len() >= 30,
            "the table is there ({})",
            t.cases.len()
        );
        // Every token is exercised, with and without pruning, so a table that skips one cannot
        // pass for a complete one.
        for token in TOKENS {
            assert!(
                t.cases
                    .iter()
                    .any(|c| c.text.to_ascii_uppercase().contains(token)),
                "no case names {token}"
            );
        }
        assert!(t.cases.iter().any(|c| c.prune) && t.cases.iter().any(|c| !c.prune));
        for c in &t.cases {
            assert_eq!(run(c), c.expect, "{}", c.name);
        }
    }

    #[test]
    fn the_values_are_in_js8calls_map_order() {
        let v = values(&MacroInput {
            mycall: "KD9TAW",
            grid: "EN52",
            selected: Some(Selected {
                call: "W1AW",
                snr_db: -7,
                tdelta_ms: 120,
            }),
            ..Default::default()
        });
        let order: Vec<&str> = v.0.iter().map(|(t, _)| *t).collect();
        assert_eq!(order, TOKENS.to_vec(), "a QMap walks its keys sorted");
        let mut sorted = TOKENS.to_vec();
        sorted.sort();
        assert_eq!(sorted, TOKENS.to_vec(), "TOKENS is itself sorted");
    }

    #[test]
    fn a_pruned_token_needs_a_character_between_its_brackets() {
        assert_eq!(prune_tokens("A <> B"), "A <> B", "`[^>]+` needs one");
        assert_eq!(
            prune_tokens("A <X> B <Y"),
            "A  B <Y",
            "an unclosed `<` stays"
        );
        assert_eq!(prune_tokens("<A<B>C"), "C", "the first `>` ends it");
        assert_eq!(prune_tokens("<A><B>"), "");
    }
}
