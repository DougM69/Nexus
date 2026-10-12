//! The readback per kind against the simulator's radio, on the test's own clock.
//!
//! Every command the session writes is answered from `tempo-flexsim`'s bundled session: its reply
//! at once, its statuses at the times their waits say, each rule's statuses after the earlier
//! rules' (the API's ordering rule, as the simulator keeps it). A fault that changes or swallows
//! a rule's statuses does it here exactly as in the simulator ([`statuses`]). Time moves a
//! millisecond at a time and the session is polled at each, so a deadline shows at the
//! millisecond it falls.
//!
//! The kinds no bench has confirmed are admitted here through the configuration's test-only door
//! ([`Config::unbenched`]); in production admission refuses their starts. Nexus's own tests.

use super::*;
use crate::flex::encode::CwxText;
use crate::flex::ptt_evidence::{Failure, KeyHandle, OperatorKey, ATU_CEILING_MS};
use tempo_flexsim::fault::{statuses, PGXL};
use tempo_flexsim::session::{Item, Rule};
use tempo_flexsim::{Fault, Foreign, Session as SimSession};

/// The bundled session's first handle: ours.
const OURS: u32 = 0x2B6E_1F40;
/// A previous session of ours on this radio.
const PREVIOUS: u32 = 0x2B6E_1F00;

/// An interlock sample naming `handle`, keyed under `source`.
fn keyed_by(handle: u32, source: &str) -> String {
    format!(
        "S0|interlock tx_client_handle=0x{handle:08X} state=TRANSMITTING reason= source={source} \
         tx_allowed=1 amplifier="
    )
}

/// The radio, played from the simulator's rules on the test's clock.
struct Radio {
    rules: SimSession,
    faults: Vec<Fault>,
    /// Lines to deliver: when, in what order they were queued, the line.
    queue: Vec<(u64, u64, String)>,
    queued: u64,
    /// When the last status of the latest rule is due.
    last_status_due: u64,
    /// Pings so far, for the simulator's dropped-ping fault.
    pings: usize,
}

impl Radio {
    fn push(&mut self, due: u64, line: String) {
        self.queued += 1;
        self.queue.push((due, self.queued, line));
    }

    /// The reply to one command at `now`, then its rule's statuses unless a fault withholds them.
    fn answer(&mut self, now: u64, seq: u32, text: &str) {
        if text == "ping" {
            self.pings += 1;
            let lost = self.faults.iter().any(|f| {
                matches!(f, Fault::DropPings { first, count }
                    if (*first..first + count).contains(&self.pings))
            });
            if lost {
                return;
            }
        }
        let expand = |t: &str| t.replace("{h}", &format!("{OURS:08X}"));
        let (code, message, items) = match self.rules.lookup(text) {
            Some(group) => {
                let rule = self.rules.rule(group, 0);
                (rule.code.clone(), rule.message.clone(), rule.items.clone())
            }
            None => (self.rules.default_code.clone(), String::new(), Vec::new()),
        };
        self.push(now, format!("R{seq}|{code}|{}", expand(&message)));
        let mut due = self.last_status_due.max(now);
        for item in statuses(&self.faults, text, items) {
            match item {
                Item::Wait(ms) => due += ms,
                Item::Send(line) => self.push(due, expand(&line)),
                Item::Vita(_) => {}
            }
        }
        self.last_status_due = self.last_status_due.max(due);
    }

    /// The lines due by `now`, in order.
    fn due(&mut self, now: u64) -> Vec<String> {
        self.queue.sort_by_key(|(due, order, _)| (*due, *order));
        let n = self
            .queue
            .iter()
            .take_while(|(due, _, _)| *due <= now)
            .count();
        self.queue.drain(..n).map(|(_, _, line)| line).collect()
    }
}

/// A session and the radio it talks to.
struct Bench {
    s: Session,
    out: Vec<u8>,
    now: u64,
    radio: Radio,
    /// How much of `out` the radio has read.
    read: usize,
    /// Every command written, with the time it was written.
    wrote: Vec<(u64, String)>,
    events: Vec<Event>,
}

impl Bench {
    /// Connected, registered, with our transmit slice made and the radio idle.
    fn ready(faults: Vec<Fault>, previous_handles: Vec<u32>) -> Bench {
        Bench::with_key(faults, previous_handles, ptt_evidence::OPERATOR_KEY)
    }

    /// [`Bench::ready`], the session taking `key` as the operator's own.
    fn with_key(faults: Vec<Fault>, previous_handles: Vec<u32>, key: Option<OperatorKey>) -> Bench {
        Bench::with_inputs(faults, previous_handles, key, ptt_evidence::OPERATOR_MIC)
    }

    /// [`Bench::ready`], the session taking `mic` as the operator's own mic.
    fn with_mic(faults: Vec<Fault>, previous_handles: Vec<u32>, mic: Option<OperatorKey>) -> Bench {
        Bench::with_inputs(faults, previous_handles, ptt_evidence::OPERATOR_KEY, mic)
    }

    /// [`Bench::ready`], the session taking `key` and `mic` as the operator's own.
    fn with_inputs(
        faults: Vec<Fault>,
        previous_handles: Vec<u32>,
        key: Option<OperatorKey>,
        mic: Option<OperatorKey>,
    ) -> Bench {
        let mut config = Config::new(Station::new("Nexus").unwrap());
        config.previous_handles = previous_handles;
        config.unbenched = true;
        config.operator_key = key;
        config.operator_mic = mic;
        let mut radio = Radio {
            rules: SimSession::v4_gui_client(),
            faults,
            queue: Vec::new(),
            queued: 0,
            last_status_due: 0,
            pings: 0,
        };
        radio.push(0, radio.rules.prologue.clone());
        radio.push(0, format!("H{OURS:08X}"));
        let mut b = Bench {
            s: Session::new(config, 0),
            out: Vec::new(),
            now: 0,
            radio,
            read: 0,
            wrote: Vec::new(),
            events: Vec::new(),
        };
        b.settle();
        assert_eq!(b.s.phase(), Phase::Ready, "{:?}", b.events);
        let create = Command::SliceCreate {
            pan: None,
            freq_hz: 14_074_000.0,
            mode: None,
        };
        b.s.send(&mut b.out, &create, b.now).expect("slice create");
        b.settle();
        assert!(b.s.transmit_ready(), "our TX slice, the radio idle");
        b
    }

    /// Answer what the session wrote and deliver what is due, until nothing changes.
    fn settle(&mut self) {
        loop {
            let mut moved = false;
            let end = self
                .out
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |i| i + 1);
            if end > self.read {
                let text = String::from_utf8_lossy(&self.out[self.read..end]).into_owned();
                self.read = end;
                for line in text.split('\n') {
                    let line = line.trim_start_matches('\u{4}');
                    let Some((seq, command)) =
                        line.strip_prefix('C').and_then(|l| l.split_once('|'))
                    else {
                        continue;
                    };
                    let seq: u32 = seq.parse().expect("a sequence number");
                    self.wrote.push((self.now, command.to_string()));
                    self.radio.answer(self.now, seq, command);
                }
                moved = true;
            }
            for line in self.radio.due(self.now) {
                self.s
                    .on_bytes(&mut self.out, format!("{line}\n").as_bytes(), self.now);
                moved = true;
            }
            self.events.extend(self.s.take_events());
            if !moved {
                return;
            }
        }
    }

    /// Let `ms` pass, a millisecond at a time.
    fn advance(&mut self, ms: u64) {
        for _ in 0..ms {
            self.now += 1;
            self.s.poll(&mut self.out, self.now);
            self.settle();
        }
    }

    /// A line from the radio, now, besides what the rules send.
    fn line(&mut self, line: &str) {
        self.s
            .on_bytes(&mut self.out, format!("{line}\n").as_bytes(), self.now);
        self.settle();
    }

    /// One stop, as the shim sends one for `\stop_morse`.
    fn stop(&mut self, stop: TxStop) -> StopOutcome {
        let stopped = self.s.stop(&mut self.out, stop, self.now);
        self.settle();
        stopped
    }

    /// Our transmit slice in CW, as the radio reports a mode change.
    fn in_cw(mut self) -> Bench {
        self.line(&format!("S{OURS:08X}|slice 0 mode=CW"));
        self
    }

    fn start(&mut self, start: TxStart, lasting_ms: Option<u64>) -> Result<u32, Refusal> {
        let started = self
            .s
            .start_lasting(&mut self.out, start, lasting_ms, self.now);
        self.settle();
        started
    }

    /// rigctld's `T 0`: what it wrote, in order.
    fn end_ours(&mut self) -> Vec<String> {
        let before = self.wrote.len();
        let ended = self.s.end_ours(&mut self.out, self.now);
        self.settle();
        let wrote: Vec<String> = self.wrote[before..]
            .iter()
            .map(|(_, c)| c.clone())
            .collect();
        match ended {
            EndOutcome::Sent(sent) => assert_eq!(sent.len(), wrote.len(), "{wrote:?}"),
            EndOutcome::NotConnected => panic!("closed"),
        }
        wrote
    }

    /// The transmit commands written, starts and stops, in order.
    fn transmit_wire(&self) -> Vec<String> {
        self.wrote
            .iter()
            .map(|(_, c)| c.clone())
            .filter(|c| {
                c.starts_with("xmit ")
                    || c.starts_with("transmit tune ")
                    || c.starts_with("cwx ")
                    || c.starts_with("atu ")
            })
            .collect()
    }

    /// When `command` was first written.
    fn written_at(&self, command: &str) -> Option<u64> {
        self.wrote
            .iter()
            .find(|(_, c)| c == command)
            .map(|(t, _)| *t)
    }

    fn count(&self, event: &Event) -> usize {
        self.events.iter().filter(|e| *e == event).count()
    }

    fn closed_unconfirmed(&self) -> bool {
        self.events.iter().any(|e| {
            matches!(
                e,
                Event::Closed {
                    end: End::UnkeyUnconfirmed,
                    was_keyed: true,
                    ..
                }
            )
        })
    }

    /// The radio answers `command` by `rules` from now on, in place of its own.
    fn answer_with(&mut self, command: &str, rules: Vec<Rule>) {
        let group = self.radio.rules.lookup(command).expect("a rule");
        self.radio.rules.rules[group].1 = rules;
    }

    /// The starts the radio refused, as the session reported them.
    fn refused(&self) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| matches!(e, Event::StartRefused { .. }))
            .collect()
    }
}

/// The bundled radio's own rules for `command`.
fn bundled(command: &str) -> Vec<Rule> {
    let s = SimSession::v4_gui_client();
    let group = s.lookup(command).expect("a rule");
    s.rules[group].1.clone()
}

/// A radio that answers with `code` and sends nothing else.
fn refuses(code: &str) -> Vec<Rule> {
    vec![Rule {
        code: code.to_string(),
        message: String::new(),
        items: Vec::new(),
    }]
}

fn cwx(text: &str) -> TxStart {
    TxStart::CwxSend(CwxText::new(text).unwrap())
}

/// The operator's own key as these tests name it: no bench has shown what the radio reports for a
/// paddle, so `PADDLE` is invented.
const PADDLE: OperatorKey = OperatorKey {
    source: "PADDLE",
    handle: KeyHandle::Ours,
};

/// The simulator's operator: their paddle keys `after` ms into `word`'s keying, under `source` and
/// `handle` (`None`: ours), for `hold` ms, then lets go.
fn touch(word: &str, after: u64, source: &str, handle: Option<u32>, hold: u64) -> Fault {
    Fault::OperatorKeys {
        word: word.into(),
        after: Duration::from_millis(after),
        source: source.into(),
        handle,
        hold: Duration::from_millis(hold),
    }
}

/// The radio idle: no client, no source.
const IDLE: &str =
    "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";

/// The take-over, as the session reports it.
const TOOK_OVER: Event = Event::OperatorTookOver {
    kind: StartKind::Cwx,
};

impl Bench {
    /// The readback failures the session reported, in order.
    fn failures(&self) -> Vec<&ReadbackFailure> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::ReadbackFailed(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    /// Where the first event `pred` holds for sits, in order.
    fn position(&self, pred: impl Fn(&Event) -> bool) -> Option<usize> {
        self.events.iter().position(pred)
    }
}

/// ⭐ rigctld's `T 0` sends the open operation's own stop, and for what only a previous session of
/// ours may hold, every stop its status shows; with nothing of ours, nothing at all.
#[test]
fn t_0_sends_only_the_stops_that_are_ours() {
    let none: Vec<String> = Vec::new();
    // Nothing of ours.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    assert_eq!(b.end_ours(), none);
    // Our over.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::Key, None).expect("keyed");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0"]);
    // Our tune.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["transmit tune 0"]);
    // A previous session's tune.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line("S0|transmit tune=1");
    b.line(&keyed_by(PREVIOUS, "TUNE"));
    assert_eq!(b.end_ours(), ["transmit tune 0", "cwx clear", "xmit 0"]);
    // Our CWX word and our ATU cycle: theirs, the ATU's being both (no stop for a cycle is
    // documented).
    let mut b = Bench::ready(vec![], vec![PREVIOUS]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(20);
    assert_eq!(b.end_ours(), ["cwx clear"]);
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0", "transmit tune 0"]);
}

/// ⭐ A tune whose carrier stays up after its stop (the simulator's StuckTune) is stopped again,
/// unkeyed as well, the operator told, the session closed: its stop armed the escalation clock as
/// an unkey does.
#[test]
fn a_tune_whose_carrier_stays_up_escalates() {
    let mut b = Bench::ready(vec![Fault::StuckTune], vec![]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["transmit tune 0"]);
    let stopped = b.now;
    b.advance(TRANSITION_TIMEOUT_MS - 1);
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0);
    assert!(b.s.keyed());
    b.advance(1);
    assert_eq!(
        b.transmit_wire(),
        [
            "transmit tune 1",
            "transmit tune 0",
            "transmit tune 0",
            "xmit 0"
        ]
    );
    assert_eq!(
        b.written_at("xmit 0"),
        Some(stopped + TRANSITION_TIMEOUT_MS)
    );
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert_eq!(b.s.phase(), Phase::Closed);
    assert!(b.closed_unconfirmed());
}

/// ⭐ With nobody asking, the session ends a latched tune at its hold plus the margin, whether or
/// not the radio loop runs; then the radio's release proves it.
#[test]
fn the_session_ends_a_latched_tune_at_its_deadline_without_the_loop() {
    let mut b = Bench::ready(vec![], vec![]);
    let started = b.now;
    b.start(TxStart::TuneOn, Some(3_000)).expect("tuning");
    b.advance(3_000 + ptt_evidence::TUNE_MARGIN_MS - 1);
    assert_eq!(b.written_at("transmit tune 0"), None);
    b.advance(1);
    assert_eq!(
        b.written_at("transmit tune 0"),
        Some(started + 3_000 + ptt_evidence::TUNE_MARGIN_MS)
    );
    b.advance(1_000);
    assert!(!b.s.keyed(), "the release is the proof");
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.transmit_wire(), ["transmit tune 1", "transmit tune 0"]);
    // No hold given: the ceiling.
    let mut b = Bench::ready(vec![], vec![]);
    let started = b.now;
    b.start(TxStart::TuneOn, Some(10 * ptt_evidence::TUNE_HOLD_MAX_MS))
        .expect("tuning");
    b.advance(ptt_evidence::TUNE_HOLD_MAX_MS + ptt_evidence::TUNE_MARGIN_MS);
    assert_eq!(
        b.written_at("transmit tune 0"),
        Some(started + ptt_evidence::TUNE_HOLD_MAX_MS + ptt_evidence::TUNE_MARGIN_MS)
    );
}

/// ⭐ A second CWX word while our own CWX is open joins it, and a key inside it is refused; the
/// radio's own break-in then ends the operation, with no stop from Nexus.
#[test]
fn a_second_word_appends_and_a_key_inside_our_cwx_is_refused() {
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(50);
    assert!(b.s.keyed());
    b.start(cwx("TEST"), Some(300))
        .expect("a second word joins");
    assert_eq!(b.start(TxStart::Key, None), Err(Refusal::AlreadyKeyed));
    assert_eq!(
        b.start(TxStart::TuneOn, Some(1_000)),
        Err(Refusal::AlreadyKeyed)
    );
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1", "cwx send \"TEST\" 2"]
    );
    b.advance(5_000);
    assert!(!b.s.keyed());
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1", "cwx send \"TEST\" 2"],
        "no stop: the radio ended it"
    );
    // A word with no keying time cannot be proven ended.
    assert_eq!(
        b.start(cwx("CQ"), None),
        Err(Refusal::NoReadback(StartKind::Cwx))
    );
    // Once a stop has gone out for our CWX, it takes no more words.
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(20);
    assert_eq!(b.end_ours(), ["cwx clear"]);
    assert_eq!(b.start(cwx("TEST"), Some(300)), Err(Refusal::AlreadyKeyed));
}

/// ⭐ A physical source inside a CWX window fails the attempt: the CWX stops go out at once and
/// the operator is told.
#[test]
fn a_foreign_source_inside_a_cwx_window_fails_the_attempt() {
    for handle in [OURS, 0] {
        let mut b = Bench::ready(vec![], vec![]).in_cw();
        b.start(cwx("CQ"), Some(300)).expect("sending");
        b.advance(20);
        b.line(&keyed_by(handle, "MIC"));
        assert_eq!(
            b.transmit_wire(),
            ["cwx send \"CQ\" 1", "cwx clear", "xmit 0"],
            "0x{handle:08X}"
        );
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
        assert!(b.closed_unconfirmed());
    }
    // The control: the same word without the sample ends by the radio's hand.
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(3_000);
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.transmit_wire(), ["cwx send \"CQ\" 1"]);
}

/// ⭐ After a reconnect, a previous session's CWX is ours to clear; another client's never is.
#[test]
fn a_previous_sessions_cwx_is_cleared_and_another_clients_is_not() {
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line(&keyed_by(PREVIOUS, ptt_evidence::CWX.source));
    assert_eq!(b.end_ours(), ["cwx clear", "xmit 0"]);
    assert!(matches!(
        b.s.stop(&mut b.out, TxStop::CwxClear, b.now),
        StopOutcome::Sent { .. }
    ));
    let foreign = Foreign::default().handle;
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line(&keyed_by(foreign, ptt_evidence::CWX.source));
    assert_eq!(b.end_ours(), Vec::<String>::new());
    assert_eq!(
        b.s.stop(&mut b.out, TxStop::CwxClear, b.now),
        StopOutcome::NothingOfOurs
    );
}

/// The session's own stops follow the kind too: closing, or a missed ping, while our tune is up
/// sends `transmit tune 0`, not an unkey the tune may not obey.
#[test]
fn closing_or_a_missed_ping_ends_a_tune_with_its_own_stop() {
    let mut b = Bench::ready(vec![], vec![]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    b.s.close(&mut b.out, b.now);
    b.settle();
    assert_eq!(b.transmit_wire(), ["transmit tune 1", "transmit tune 0"]);

    let mut b = Bench::ready(
        vec![Fault::DropPings {
            first: 1,
            count: 100,
        }],
        vec![],
    );
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(2_100);
    assert!(b
        .events
        .iter()
        .any(|e| matches!(e, Event::UnkeyedOnMissedPing { .. })));
    assert_eq!(
        b.transmit_wire().first().map(String::as_str),
        Some("transmit tune 1")
    );
    assert_eq!(b.transmit_wire()[1], "transmit tune 0");
}

/// ⭐ An amplifier in line (the simulator's PGXL profile: every PTT_REQUESTED carries
/// `reason=AMP:PG-XL`, every TRANSMITTING the amplifiers): an over, a tune and a CWX word each end
/// by their own proof, with no alarm. Any other reason on the same reports still fails the over:
/// its unkey goes out again, the operator is told, the session closes.
#[test]
fn an_amplifiers_reason_on_a_keying_report_is_ours_and_any_other_fails() {
    let pgxl = || {
        vec![Fault::KeyingReason {
            reason: PGXL.into(),
        }]
    };
    let ended_clean = |b: &Bench, what: &str| {
        assert!(!b.s.keyed(), "{what}: {:?}", b.events);
        assert_eq!(b.count(&Event::UnkeyConfirmed), 1, "{what}");
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0, "{what}");
        assert_eq!(b.s.phase(), Phase::Ready, "{what}");
    };
    let mut b = Bench::ready(pgxl(), vec![]);
    b.start(TxStart::Key, None).expect("keyed");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0"]);
    b.advance(1_000);
    ended_clean(&b, "an over");
    assert_eq!(b.transmit_wire(), ["xmit 1", "xmit 0"]);

    let mut b = Bench::ready(pgxl(), vec![]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["transmit tune 0"]);
    b.advance(1_000);
    ended_clean(&b, "a tune");

    let mut b = Bench::ready(pgxl(), vec![]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(3_000);
    ended_clean(&b, "a CWX word");
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1"],
        "the radio ended it"
    );

    for reason in ["PA_RANGE", "AMP:PG-XL,ANT:ANT2"] {
        let mut b = Bench::ready(
            vec![Fault::KeyingReason {
                reason: reason.into(),
            }],
            vec![],
        );
        b.start(TxStart::Key, None).expect("keyed");
        b.advance(100);
        assert_eq!(b.end_ours(), ["xmit 0"]);
        b.advance(TRANSITION_TIMEOUT_MS);
        assert_eq!(
            b.transmit_wire(),
            ["xmit 1", "xmit 0", "xmit 0"],
            "{reason}"
        );
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1, "{reason}");
        assert!(b.closed_unconfirmed(), "{reason}");
    }
}

/// ⭐ Stop TX in the middle of a CW message: `cwx clear` goes out at once, no word joins after it,
/// and the radio's own release ends the operation, with no unkey. A radio still showing our CWX
/// transmitting break-in delay + 300 ms after the clear's reply (the simulator's held CWX) is sent
/// the unkey then, once, and its release proves the end: no alarm either way.
#[test]
fn stop_tx_mid_macro_clears_and_sends_nothing_after() {
    let ended_clean = |b: &Bench| {
        assert!(!b.s.keyed(), "{:?}", b.events);
        assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0);
        assert_eq!(b.s.phase(), Phase::Ready);
    };
    // The radio's break-in ends it.
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(1_620)).expect("sending");
    b.advance(400);
    assert!(matches!(b.stop(TxStop::CwxClear), StopOutcome::Sent { .. }));
    assert_eq!(
        b.start(cwx("TEST"), Some(1_000)),
        Err(Refusal::AlreadyKeyed)
    );
    b.advance(TRANSITION_TIMEOUT_MS + 1_000);
    ended_clean(&b);
    assert_eq!(b.transmit_wire(), ["cwx send \"CQ\" 1", "cwx clear"]);

    // The radio holds transmit after the clear.
    let mut b = Bench::ready(vec![Fault::HoldsCwx], vec![]).in_cw();
    b.start(cwx("CQ"), Some(1_620)).expect("sending");
    b.advance(400);
    let replied = b.now;
    assert!(matches!(b.stop(TxStop::CwxClear), StopOutcome::Sent { .. }));
    b.advance(300 + 300 - 1);
    assert_eq!(b.written_at("xmit 0"), None, "inside the break-in window");
    b.advance(1);
    assert_eq!(b.written_at("xmit 0"), Some(replied + 300 + 300));
    b.advance(TRANSITION_TIMEOUT_MS);
    ended_clean(&b);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1", "cwx clear", "xmit 0"]
    );

    // Stopped after the word's expected end, on a radio that holds it: from the clear's reply the
    // end has a transition's time, so the unkey and its release still come inside it.
    let mut b = Bench::ready(vec![Fault::HoldsCwx], vec![]).in_cw();
    b.start(cwx("E"), Some(300)).expect("sending");
    b.advance(1_000);
    let replied = b.now;
    assert!(matches!(b.stop(TxStop::CwxClear), StopOutcome::Sent { .. }));
    b.advance(TRANSITION_TIMEOUT_MS);
    assert_eq!(b.written_at("xmit 0"), Some(replied + 300 + 300));
    ended_clean(&b);
}

/// ⭐ A long word stopped part way ends on the radio's release, not at its expected end: the clear
/// emptied the radio's buffer. Waiting out a 12-second word past the five seconds a stop has to be
/// proven would raise "may still be transmitting" and drop the connection over an idle radio.
#[test]
fn a_long_word_stopped_part_way_ends_on_the_radios_release() {
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(12_000)).expect("sending");
    b.advance(400);
    assert!(matches!(b.stop(TxStop::CwxClear), StopOutcome::Sent { .. }));
    b.advance(TRANSITION_TIMEOUT_MS + 1_000);
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0, "{:?}", b.events);
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.transmit_wire(), ["cwx send \"CQ\" 1", "cwx clear"]);
}

/// ⭐ An ATU start the radio refuses in its reply, with nothing keyed before it, ends there: no
/// stop, no alarm, the session up, nothing of ours left on the air or to stop, and the refusal
/// reported in the radio's words. The next press starts a cycle. A refusal that comes after a
/// keyed sample is not that: the radio keyed for the start, its end is not proven, and both stops
/// go out, the operator told, as before. (Which code a radio refuses with is not established: the
/// simulator's error code stands in.)
#[test]
fn a_refused_atu_start_ends_quietly_and_says_why() {
    let none: Vec<String> = Vec::new();
    let code = u32::from_str_radix(tempo_flexsim::fault::REFUSED, 16).unwrap();
    let mut b = Bench::ready(vec![], vec![]);
    b.answer_with("atu start", refuses(tempo_flexsim::fault::REFUSED));
    b.start(TxStart::AtuStart, None).expect("written");
    b.advance(ATU_CEILING_MS + TRANSITION_TIMEOUT_MS);
    assert_eq!(
        (
            b.transmit_wire(),
            b.s.keyed(),
            b.s.snapshot().ours_on_air,
            b.count(&Event::UnkeyUnconfirmed),
            b.s.phase()
        ),
        (vec!["atu start".to_string()], false, false, 0, Phase::Ready)
    );
    assert_eq!(
        b.refused(),
        [&Event::StartRefused {
            kind: StartKind::Atu,
            code,
            message: String::new()
        }]
    );
    assert!(b.s.transmit_ready(), "the readback idle again");
    assert_eq!(b.end_ours(), none, "nothing of ours to stop");
    // The next press, which the radio takes: one cycle, proven.
    b.answer_with("atu start", bundled("atu start"));
    b.start(TxStart::AtuStart, None).expect("tuning");
    b.advance(2_000);
    assert_eq!(b.transmit_wire(), ["atu start", "atu start"]);
    assert_eq!((b.count(&Event::UnkeyConfirmed), b.s.keyed()), (1, false));

    // The radio keyed before it refused.
    let mut b = Bench::ready(vec![], vec![]);
    b.answer_with("atu start", refuses(tempo_flexsim::fault::REFUSED));
    b.s.start_lasting(&mut b.out, TxStart::AtuStart, None, b.now)
        .expect("written");
    b.line(&keyed_by(OURS, "TUNE"));
    b.advance(100);
    assert_eq!(
        b.transmit_wire(),
        ["atu start", "xmit 0", "transmit tune 0"]
    );
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert!(b.closed_unconfirmed());
    assert!(b.refused().is_empty(), "{:?}", b.events);
}

/// ⭐ One ATU press, one cycle: `atu start`, then keyed through the cycle until the radio reports a
/// result AND the interlock is idle again. The result alone is not the end: the bundled radio
/// reports it 530 ms after the reply and falls idle at 960 ms. A cycle that never ends (the
/// simulator's AtuNeverEnds) is ended at its ceiling, 20 s from the start's reply and not a
/// millisecond before: both stops again, the operator told, the session closed.
#[test]
fn an_atu_press_starts_one_cycle_its_status_ends_it_and_a_stuck_one_alarms() {
    let mut b = Bench::ready(vec![], vec![]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    let started = b.now;
    let (mut result, mut ended) = (None, None);
    for _ in 0..2_000 {
        b.advance(1);
        if result.is_none() && b.s.model().atu.status.as_deref() == Some("TUNE_SUCCESSFUL") {
            result = Some(b.now - started);
        }
        if ended.is_none() && !b.s.keyed() {
            ended = Some(b.now - started);
        }
    }
    assert_eq!((result, ended), (Some(530), Some(960)));
    assert_eq!(b.transmit_wire(), ["atu start"]);
    assert_eq!(
        (
            b.count(&Event::UnkeyConfirmed),
            b.count(&Event::UnkeyUnconfirmed),
            b.s.phase()
        ),
        (1, 0, Phase::Ready)
    );

    // The cycle that never ends.
    let mut b = Bench::ready(vec![Fault::AtuNeverEnds], vec![]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    let replied = b.now;
    b.advance(ATU_CEILING_MS - 1);
    assert_eq!(
        (
            b.transmit_wire(),
            b.count(&Event::UnkeyUnconfirmed),
            b.s.keyed()
        ),
        (vec!["atu start".to_string()], 0, true)
    );
    b.advance(1);
    assert_eq!(
        b.transmit_wire(),
        ["atu start", "xmit 0", "transmit tune 0"]
    );
    assert_eq!(b.written_at("xmit 0"), Some(replied + ATU_CEILING_MS));
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert!(b.closed_unconfirmed());
}

/// ⭐ Stop TX during an ATU cycle (rigctld's `T 0`): no command that ends a cycle part way is
/// documented, so both stops go out, `xmit 0` and `transmit tune 0`. A radio that then reports the
/// cycle's result and the idle interlock ends it with no alarm. One that does neither
/// (AtuNeverEnds) gets both stops again 5 s after the first, the operator is told and the session
/// closes: every stop's deadline, long before the cycle's ceiling.
#[test]
fn stop_tx_mid_cycle_sends_both_stops_and_a_cycle_that_goes_on_alarms() {
    let mut b = Bench::ready(vec![], vec![]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0", "transmit tune 0"]);
    b.advance(TRANSITION_TIMEOUT_MS + 1_000);
    assert_eq!(
        b.transmit_wire(),
        ["atu start", "xmit 0", "transmit tune 0"]
    );
    assert_eq!(
        (
            b.count(&Event::UnkeyConfirmed),
            b.count(&Event::UnkeyUnconfirmed),
            b.s.keyed(),
            b.s.phase()
        ),
        (1, 0, false, Phase::Ready)
    );

    let mut b = Bench::ready(vec![Fault::AtuNeverEnds], vec![]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0", "transmit tune 0"]);
    let stopped = b.now;
    b.advance(TRANSITION_TIMEOUT_MS - 1);
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0);
    b.advance(1);
    assert_eq!(
        b.transmit_wire(),
        [
            "atu start",
            "xmit 0",
            "transmit tune 0",
            "xmit 0",
            "transmit tune 0"
        ]
    );
    let again: Vec<u64> = b
        .wrote
        .iter()
        .filter(|(_, c)| c == "xmit 0")
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(again, [stopped, stopped + TRANSITION_TIMEOUT_MS]);
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert!(b.closed_unconfirmed());
}

// ── A paddle touch during our CW (operator ruling, 2026-10-12: the paddle wins) ──────────────
//
// The simulator's operator (`Fault::OperatorKeys`) keys the radio part way into a word, under a
// source and handle the test supposes, holds it, then lets go. No bench has named what a real
// radio reports for a paddle, so the rule is inert as it ships; these tests name a key of their
// own (`PADDLE`) through the configuration's test-only door, as a bench's value would name one.

/// ⭐ THE DIAGNOSTIC LINE. A readback check that fails is reported once, before the alarm it leads
/// to, with the check, the kind of start and the radio's own line as it arrived: what a tester's
/// bench sends back to show what the radio reports. It keys nothing: the wire is today's. A missed
/// deadline reports the last interlock line before it. An operation that ends well reports nothing.
#[test]
fn a_failed_readback_check_is_reported_with_the_radios_line() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 1_000)],
        vec![],
        None,
    )
    .in_cw();
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    let line = format!(
        "S0|interlock tx_client_handle=0x{OURS:08X} state=TRANSMITTING reason= source=PADDLE \
         tx_allowed=1 amplifier="
    );
    let failure = ReadbackFailure {
        kind: Some(StartKind::Cwx),
        handle: Some(OURS),
        report: FailureReport {
            failure: Failure::Ownership,
            line: Some(line.clone()),
            last_interlock: None,
        },
    };
    assert_eq!(b.failures(), [&failure]);
    assert_eq!(
        failure.to_string(),
        format!(
            "readback check failed: Ownership on a Cwx start, Nexus's handle 0x2B6E1F40; the \
             radio's line: {line}"
        )
    );
    assert!(
        b.position(|e| matches!(e, Event::ReadbackFailed(_)))
            < b.position(|e| *e == Event::UnkeyUnconfirmed),
        "before the alarm: {:?}",
        b.events
    );
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "xmit 0"],
        "today's wire"
    );

    // A missed deadline: an over the radio does not unkey (the simulator's StuckTransmit).
    let mut b = Bench::ready(vec![Fault::StuckTransmit], vec![]);
    b.start(TxStart::Key, None).expect("keyed");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0"]);
    b.advance(TRANSITION_TIMEOUT_MS);
    let keyed = keyed_by(OURS, "SW");
    let failure = ReadbackFailure {
        kind: Some(StartKind::Key),
        handle: Some(OURS),
        report: FailureReport {
            failure: Failure::Timeout,
            line: None,
            last_interlock: Some(keyed.clone()),
        },
    };
    assert_eq!(b.failures(), [&failure]);
    assert_eq!(
        failure.to_string(),
        format!(
            "readback check failed: Timeout on a Key start, Nexus's handle 0x2B6E1F40; no line \
             failed it; the last interlock line: {keyed}"
        )
    );
    // Before the handshake there is no handle to name, and with nothing of ours open, no kind.
    assert_eq!(
        ReadbackFailure {
            kind: None,
            handle: None,
            report: FailureReport {
                failure: Failure::Write,
                line: None,
                last_interlock: None,
            },
        }
        .to_string(),
        "readback check failed: Write with no start of ours open; no line from the radio"
    );
    assert!(b.closed_unconfirmed());

    // The control: a word the radio's break-in ends.
    let mut b = Bench::ready(vec![], vec![]).in_cw();
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(3_000);
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.failures(), Vec::<&ReadbackFailure>::new());
}

/// ⭐ AS IT SHIPS, NO KEY IS NAMED, and a paddle touch during the CW is handled as today: an unknown
/// keying source fails the readback, the CW stops go out at once (`cwx clear`, then `xmit 0`), the
/// operator is told and the session closes. The scene of the take-over below, with no key.
#[test]
fn with_no_key_named_the_paddle_is_handled_as_today() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 1_000)],
        vec![],
        None,
    )
    .in_cw();
    let sent = b.now;
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "xmit 0"]
    );
    assert_eq!(b.written_at("xmit 0"), Some(sent + 10 + 200));
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert!(b.closed_unconfirmed());
    assert_eq!(b.count(&TOOK_OVER), 0);
}

/// ⭐ THE PADDLE WINS, with a key named as a bench would name it. The operator's paddle keys 200 ms
/// into the word: `cwx clear` goes out on that sample, and no other stop ever does, no unkey above
/// all. The session reports the take-over and no alarm, stays up, and refuses every start while the
/// operator has the radio, the message's next word included. Our part ends once the clear's reply
/// and the hold (300 + 300) have passed, while the operator still keys; nothing of ours is on the
/// air after it, and `T 0` sends nothing. Once the radio is idle the next word sends as usual.
#[test]
fn the_operators_key_during_a_cwx_word_drops_the_rest_with_no_alarm() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 1_000)],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    let sent = b.now;
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    assert_eq!(b.transmit_wire(), ["cwx send \"TEST\" 1", "cwx clear"]);
    assert_eq!(
        b.written_at("cwx clear"),
        Some(sent + 10 + 200),
        "on the operator's first sample"
    );
    assert_eq!(b.count(&TOOK_OVER), 1);
    assert_eq!(b.start(cwx("DE"), Some(300)), Err(Refusal::OperatorKeying));
    assert_eq!(b.start(TxStart::Key, None), Err(Refusal::OperatorKeying));
    assert!(
        b.s.snapshot().ours_on_air,
        "ours until our part is proven over"
    );
    b.advance(600 - 1);
    assert!(b.s.keyed(), "inside the hold");
    b.advance(1);
    assert!(!b.s.keyed(), "{:?}", b.events);
    assert!(
        !b.s.snapshot().ours_on_air,
        "the operator's keying is not ours"
    );
    assert!(!b.s.transmit_ready(), "the radio not yet seen idle");
    assert_eq!(b.end_ours(), Vec::<String>::new());
    assert_eq!(
        b.start(cwx("DE"), Some(300)),
        Err(Refusal::OperatorKeying),
        "while the paddle keys"
    );
    // The operator lets go, and the radio is idle.
    b.advance(1_000);
    assert!(b.s.transmit_ready());
    b.start(cwx("DE"), Some(300))
        .expect("the next word sends as usual");
    b.advance(3_000);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "cwx send \"DE\" 2"]
    );
    assert_eq!(
        (
            b.count(&TOOK_OVER),
            b.count(&Event::UnkeyUnconfirmed),
            b.count(&Event::UnkeyConfirmed),
            b.s.phase()
        ),
        (1, 0, 1, Phase::Ready),
        "the one end confirmed is DE's own"
    );
    assert!(
        !b.events
            .iter()
            .any(|e| matches!(e, Event::Closed { .. } | Event::ReadbackFailed(_))),
        "{:?}",
        b.events
    );

    // A touch shorter than the hold: the paddle has let go, and our part has not yet ended. A
    // word then is still refused for the operator, not as "already keyed".
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 100)],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200 + 100 + 150);
    assert!(b.s.keyed(), "inside the hold");
    assert_eq!(
        b.s.model()
            .interlock
            .sample
            .as_ref()
            .map(|s| s.state.as_str()),
        Some("READY"),
        "premise: the paddle has let go"
    );
    assert_eq!(b.start(cwx("DE"), Some(300)), Err(Refusal::OperatorKeying));
    b.advance(600);
    assert!(!b.s.keyed());
    assert!(b.s.transmit_ready(), "seen idle as our part ended");
    b.start(cwx("DE"), Some(300))
        .expect("the next word sends as usual");

    // Stop TX during the hand-over (rigctld's `T 0`): never gated, and still only our CWX's own
    // stop, the clear; never an unkey at the paddle.
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 1_000)],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200 + 100);
    assert!(b.s.keyed(), "inside the hand-over");
    assert_eq!(b.end_ours(), ["cwx clear"]);
    b.advance(2_000);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "cwx clear"]
    );
    assert_eq!((b.count(&Event::UnkeyUnconfirmed), b.s.keyed()), (0, false));
}

/// ⭐ With a key named, any other keying during our CWX still fails the readback, as today: the
/// mic's PTT, the accessory and foot-switch inputs, the radio's voice keyer, a tune, a source no one
/// named, and the key's own source under another client's handle or under no client when the bench
/// showed ours. The CW stops go out and the operator is told; no take-over is reported.
#[test]
fn an_unrecognised_source_inside_a_cwx_window_still_alarms() {
    for (source, handle) in [
        ("MIC", None),
        ("ACC", None),
        ("RCA", None),
        ("DVK", None),
        ("TUNE", None),
        ("FOOT", None),
        ("PADDLE", Some(0x7A3C_0001)),
        ("PADDLE", Some(0)),
    ] {
        let what = format!("{source} {handle:?}");
        let mut b = Bench::with_key(
            vec![touch("TEST", 200, source, handle, 1_000)],
            vec![],
            Some(PADDLE),
        )
        .in_cw();
        b.start(cwx("TEST"), Some(1_500)).expect("sending");
        b.advance(10 + 200);
        assert_eq!(
            b.transmit_wire(),
            ["cwx send \"TEST\" 1", "cwx clear", "xmit 0"],
            "{what}"
        );
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1, "{what}");
        assert!(b.closed_unconfirmed(), "{what}");
        assert_eq!(b.count(&TOOK_OVER), 0, "{what}");
        assert_eq!(b.failures().len(), 1, "{what}");
    }
}

/// ⭐ Nexus's own CW still keying after its clear is still Nexus's to stop, with the paddle on the
/// air: after a 300 ms touch the radio keys our CWX again and holds it (the simulator's held CWX).
/// Still keyed when the clear's reply and the hold (300 + 300) have passed, it fails the readback at
/// that moment: the CW stops go out again, `cwx clear` and `xmit 0`, the operator is told, and the
/// diagnostic line carries the radio's last report, our CWX keying.
#[test]
fn our_cwx_still_keying_after_the_clear_still_escalates_with_the_paddle_on_the_air() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 300), Fault::HoldsCwx],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    let sent = b.now;
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    assert_eq!(b.transmit_wire(), ["cwx send \"TEST\" 1", "cwx clear"]);
    b.advance(600 - 1);
    assert_eq!(b.written_at("xmit 0"), None, "inside the hold");
    b.advance(1);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "cwx clear", "xmit 0"]
    );
    assert_eq!(b.written_at("xmit 0"), Some(sent + 10 + 200 + 600));
    assert_eq!(
        (
            b.count(&TOOK_OVER),
            b.count(&Event::UnkeyUnconfirmed),
            b.closed_unconfirmed()
        ),
        (1, 1, true)
    );
    assert_eq!(
        b.failures(),
        [&ReadbackFailure {
            kind: Some(StartKind::Cwx),
            handle: Some(OURS),
            report: FailureReport {
                failure: Failure::State,
                line: None,
                last_interlock: Some(keyed_by(OURS, "SW"))
            }
        }]
    );
}

/// ⭐ While the operator's paddle keys the radio and Nexus is idle, a key of ours is refused with
/// the operator's own reason and nothing reaches the wire; once the radio is idle the same key
/// sends. Under no client when the bench says so. With no key named, the same sample refuses as any
/// keying source does today.
#[test]
fn a_key_while_the_paddle_keys_is_refused_and_one_after_it_sends() {
    let no_client = OperatorKey {
        handle: KeyHandle::NoClient,
        ..PADDLE
    };
    for (key, handle) in [(PADDLE, OURS), (no_client, 0)] {
        let mut b = Bench::with_key(vec![], vec![], Some(key)).in_cw();
        b.line(&keyed_by(handle, "PADDLE"));
        assert_eq!(
            b.start(cwx("CQ"), Some(300)),
            Err(Refusal::OperatorKeying),
            "{key:?}"
        );
        assert_eq!(
            b.start(TxStart::Key, None),
            Err(Refusal::OperatorKeying),
            "{key:?}"
        );
        assert_eq!(b.transmit_wire(), Vec::<String>::new(), "{key:?}");
        b.line(&format!(
            "S0|interlock tx_client_handle=0x{handle:08X} state=UNKEY_REQUESTED reason= source= \
             tx_allowed=1 amplifier="
        ));
        b.line(IDLE);
        b.start(cwx("CQ"), Some(300)).expect("after the paddle");
        assert_eq!(b.transmit_wire(), ["cwx send \"CQ\" 1"], "{key:?}");
    }
    let mut b = Bench::with_key(vec![], vec![], None).in_cw();
    b.line(&keyed_by(OURS, "PADDLE"));
    assert_eq!(
        b.start(cwx("CQ"), Some(300)),
        Err(Refusal::SourceActive {
            source: "PADDLE".into()
        })
    );
}

/// ⭐ STOP TX, THEN THE PADDLE: the unkey that follows a clear goes out only while the radio shows
/// Nexus's own CW transmitting, never at the operator's key, whatever the radio reports for it.
/// Stop TX 100 ms into the word, the operator's paddle 100 ms later, under our handle, for 3 s: no
/// unkey, no alarm, and our part ends with the clear's reply and the hold. On a radio that refused
/// the clear, our part cannot be shown over: the follow-up's moment passes with the paddle keying
/// and sends nothing, and the stops come 5 s after Stop TX, as for any clear the radio never takes.
#[test]
fn stop_tx_then_the_paddle_never_draws_the_follow_up_unkey() {
    let scene = |refused: bool| {
        let mut b = Bench::with_key(
            vec![touch("TEST", 200, "PADDLE", None, 3_000)],
            vec![],
            Some(PADDLE),
        )
        .in_cw();
        if refused {
            b.answer_with("cwx clear", refuses(tempo_flexsim::fault::REFUSED));
        }
        b.start(cwx("TEST"), Some(1_500)).expect("sending");
        b.advance(10 + 100);
        let stopped = b.now;
        assert!(matches!(b.stop(TxStop::CwxClear), StopOutcome::Sent { .. }));
        (b, stopped)
    };
    let (mut b, _) = scene(false);
    b.advance(3_500);
    assert_eq!(b.transmit_wire(), ["cwx send \"TEST\" 1", "cwx clear"]);
    assert_eq!(
        (
            b.s.keyed(),
            b.count(&TOOK_OVER),
            b.count(&Event::UnkeyUnconfirmed),
            b.s.phase()
        ),
        (false, 1, 0, Phase::Ready)
    );

    let (mut b, stopped) = scene(true);
    b.advance(300 + 300);
    assert_eq!(b.count(&TOOK_OVER), 1);
    assert_eq!(b.written_at("xmit 0"), None, "no unkey at the paddle");
    b.advance(TRANSITION_TIMEOUT_MS);
    assert_eq!(
        b.written_at("xmit 0"),
        Some(stopped + TRANSITION_TIMEOUT_MS),
        "the escalation's"
    );
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "cwx clear", "xmit 0"]
    );
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
}

/// The radio's refusal of a `cwx clear` while another client transmits (TCPIP-cwx: "Transmit not
/// allowed due to other client transmitting"), as it may answer one while the operator's paddle
/// keys.
const OTHER_CLIENT_TRANSMITTING: &str = "500000C2";

/// ⭐ WAIT WHILE THE PADDLE KEYS (operator ruling, 2026-10-12): the radio refuses the hand-over's
/// clear while the operator's paddle keys, for 8 s, so our part cannot be shown over and the
/// escalation comes due 5 s after the clear. Its unkey waits while the radio shows the paddle
/// keying, Stop TX meanwhile sends only our own clear (never held), and the unkey goes once the
/// hold (300 + 300) has passed after the radio stopped showing the paddle: our CW still unproven
/// after the paddle let go, the stops go out again and the operator is told. Our part is ours
/// throughout. Then a refused clear with the radio reporting its own transmit timeout during the
/// wait: anything the readback does not take ends the wait at once.
#[test]
fn a_refused_clear_holds_the_unkey_while_the_paddle_keys_and_sends_it_after() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 8_000)],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    b.answer_with("cwx clear", refuses(OTHER_CLIENT_TRANSMITTING));
    let sent = b.now;
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    let touched = b.now;
    assert_eq!(b.transmit_wire(), ["cwx send \"TEST\" 1", "cwx clear"]);
    assert_eq!(b.written_at("cwx clear"), Some(touched));
    b.advance(TRANSITION_TIMEOUT_MS + 2_000);
    assert_eq!(
        (b.written_at("xmit 0"), b.count(&Event::UnkeyUnconfirmed)),
        (None, 0),
        "no unkey while the paddle keys, {} ms after the clear",
        b.now - touched
    );
    assert!(
        b.s.keyed() && b.s.snapshot().ours_on_air,
        "our part still ours to end"
    );
    assert_eq!(b.end_ours(), ["cwx clear"], "Stop TX: our own stop");
    // The paddle lets go 8 s after its touch (UNKEY_REQUESTED and READY, then idle 100 ms on).
    let let_go = sent + 10 + 200 + 8_000;
    b.advance(let_go + 600 - 1 - b.now);
    assert_eq!(
        b.written_at("xmit 0"),
        None,
        "inside the hold after it let go"
    );
    b.advance(1);
    assert_eq!(b.written_at("xmit 0"), Some(let_go + 600));
    assert_eq!(
        b.transmit_wire(),
        [
            "cwx send \"TEST\" 1",
            "cwx clear",
            "cwx clear",
            "cwx clear",
            "xmit 0"
        ]
    );
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert!(b.closed_unconfirmed());

    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 8_000)],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    b.answer_with("cwx clear", refuses(OTHER_CLIENT_TRANSMITTING));
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200 + TRANSITION_TIMEOUT_MS + 1_000);
    assert_eq!(b.written_at("xmit 0"), None, "the wait");
    let timeout = b.now;
    b.line(&format!(
        "S0|interlock tx_client_handle=0x{OURS:08X} state=TIMEOUT reason= source= tx_allowed=1 \
         amplifier="
    ));
    b.advance(1);
    assert_eq!(b.written_at("xmit 0"), Some(timeout));
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
}

/// ⭐ …and as soon as the radio shows Nexus's own CW, the wait is over: after the paddle's 8 s the
/// radio keys our CWX again and holds it (the simulator's held CWX), and the stops go out at the
/// next millisecond, the paddle no longer keying. Until then, the paddle keying, nothing did.
#[test]
fn a_refused_clear_sends_the_unkey_as_soon_as_the_radio_shows_our_cw() {
    let mut b = Bench::with_key(
        vec![touch("TEST", 200, "PADDLE", None, 8_000), Fault::HoldsCwx],
        vec![],
        Some(PADDLE),
    )
    .in_cw();
    b.answer_with("cwx clear", refuses(OTHER_CLIENT_TRANSMITTING));
    let sent = b.now;
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    let ours_again = sent + 10 + 200 + 8_000;
    b.advance(ours_again - b.now);
    assert_eq!(
        (b.written_at("xmit 0"), b.count(&Event::UnkeyUnconfirmed)),
        (None, 0),
        "nothing while the paddle keyed"
    );
    assert_eq!(
        b.s.model()
            .interlock
            .sample
            .as_ref()
            .map(|s| s.source.as_str()),
        Some(ptt_evidence::CWX.source),
        "premise: the radio shows our CWX keying"
    );
    b.advance(1);
    assert_eq!(b.written_at("xmit 0"), Some(ours_again + 1));
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "cwx clear", "xmit 0"]
    );
    assert_eq!(
        (b.count(&TOOK_OVER), b.count(&Event::UnkeyUnconfirmed)),
        (1, 1)
    );
}

/// ⭐ THE OPERATOR'S KEY ALONE IS NOT OURS, with a key named: with nothing of ours open and that key
/// keying under this session's handle, rigctld's `T 0`, a stop and the teardown send nothing, and
/// the snapshot does not say something of ours is on the air. A previous session's handle keeps
/// today's rule (every stop), and so does our own CW's source; with no key named, so does the
/// paddle.
#[test]
fn t_0_with_only_the_operators_key_on_the_air_sends_nothing() {
    let none = Vec::<String>::new();
    let mut b = Bench::with_key(vec![], vec![PREVIOUS], Some(PADDLE));
    b.line(&keyed_by(OURS, "PADDLE"));
    assert!(!b.s.snapshot().ours_on_air);
    assert_eq!(b.end_ours(), none);
    assert_eq!(b.stop(TxStop::Unkey), StopOutcome::NothingOfOurs);
    assert_eq!(b.stop(TxStop::CwxClear), StopOutcome::NothingOfOurs);
    b.s.close(&mut b.out, b.now);
    b.settle();
    assert_eq!(b.transmit_wire(), none, "the teardown sends nothing at it");
    for (key, line) in [
        (Some(PADDLE), keyed_by(PREVIOUS, "PADDLE")),
        (Some(PADDLE), keyed_by(OURS, ptt_evidence::CWX.source)),
        (None, keyed_by(OURS, "PADDLE")),
    ] {
        let mut b = Bench::with_key(vec![], vec![PREVIOUS], key);
        b.line(&line);
        assert!(b.s.snapshot().ours_on_air, "{line}");
        assert_eq!(b.end_ours(), ["cwx clear", "xmit 0"], "{line}");
    }
}

/// The operator's own mic as these tests name it: no bench has shown what the radio reports for
/// the mic's PTT during an over of ours, so `MIC` under our handle is supposed.
const MIC: OperatorKey = OperatorKey {
    source: "MIC",
    handle: KeyHandle::Ours,
};

/// The take-over of an over, as the session reports it.
const MIC_TOOK_OVER: Event = Event::OperatorTookOver {
    kind: StartKind::Key,
};

/// The simulator's operator: their mic's PTT keys `after` ms into each over's keying, under
/// `source` and `handle` (`None`: ours), for `hold` ms, then lets go; our unkey changes nothing the
/// interlock shows meanwhile.
fn press(after: u64, source: &str, handle: Option<u32>, hold: u64) -> Fault {
    Fault::OperatorMic {
        after: Duration::from_millis(after),
        source: source.into(),
        handle,
        hold: Duration::from_millis(hold),
    }
}

impl Bench {
    /// Let time pass, a millisecond at a time, until the radio's last whole sample is keyed under
    /// `source`: when it arrived.
    fn advance_until_keyed_by(&mut self, source: &str) -> u64 {
        for _ in 0..10_000 {
            let keyed = self.s.model().interlock.sample.as_ref().is_some_and(|s| {
                s.source == source && matches!(s.state.as_str(), "PTT_REQUESTED" | "TRANSMITTING")
            });
            if keyed {
                return self.now;
            }
            self.advance(1);
        }
        panic!("the radio never showed {source} keying: {:?}", self.events);
    }
}

/// ⭐ AS IT SHIPS, NO MIC IS NAMED, and the mic's PTT during an over is handled as today: an
/// unknown keying source fails the readback, and nothing more happens until the over's own unkey
/// (the radio loop's `T 0` at its end), which the radio cannot then prove: 5 s later the unkey goes
/// out again, the operator is told and the session closes. The scene of the take-over below, with
/// no mic.
#[test]
fn with_no_mic_named_the_mics_ptt_during_an_over_is_handled_as_today() {
    let mut b = Bench::with_mic(vec![press(300, "MIC", None, 2_000)], vec![], None);
    b.start(TxStart::Key, None).expect("keying");
    b.advance_until_keyed_by("MIC");
    b.advance(500);
    assert_eq!(b.transmit_wire(), ["xmit 1"], "the over runs on");
    assert_eq!(b.failures().len(), 1);
    assert_eq!(b.end_ours(), ["xmit 0"], "the over's own end");
    let ended = b.now;
    b.advance(TRANSITION_TIMEOUT_MS);
    assert_eq!(b.transmit_wire(), ["xmit 1", "xmit 0", "xmit 0"]);
    assert_eq!(
        b.wrote
            .iter()
            .filter(|(_, c)| c == "xmit 0")
            .map(|(t, _)| *t)
            .next_back(),
        Some(ended + TRANSITION_TIMEOUT_MS)
    );
    assert_eq!(
        (
            b.count(&MIC_TOOK_OVER),
            b.count(&Event::UnkeyUnconfirmed),
            b.closed_unconfirmed()
        ),
        (0, 1, true)
    );
}

/// ⭐ YOUR MIC WINS, with a mic named as a bench would name it: the operator's mic keys 300 ms into
/// our over. Our own unkey goes out on that sample, the session reports the take-over and no alarm,
/// and stays up. Our part is ours until the unkey's reply and the hold have passed (a key then is
/// refused as already keyed); after it the mic's keying is not ours (`T 0` sends nothing at it, a
/// key is refused for the mic), and once the radio is idle the next over keys as usual.
#[test]
fn the_operators_mic_during_an_over_ends_our_key_with_no_alarm() {
    let mut b = Bench::with_mic(vec![press(300, "MIC", None, 2_000)], vec![], Some(MIC));
    b.start(TxStart::Key, None).expect("keying");
    let pressed = b.advance_until_keyed_by("MIC");
    assert_eq!(b.transmit_wire(), ["xmit 1", "xmit 0"]);
    assert_eq!(
        b.written_at("xmit 0"),
        Some(pressed),
        "on the mic's first sample"
    );
    assert_eq!(b.count(&MIC_TOOK_OVER), 1);
    assert!(
        b.s.keyed() && b.s.snapshot().ours_on_air,
        "ours until our part is proven over"
    );
    assert_eq!(b.start(TxStart::Key, None), Err(Refusal::AlreadyKeyed));
    b.advance(ptt_evidence::MIC_HOLD_MS - 1);
    assert!(b.s.keyed(), "inside the hold");
    b.advance(1);
    assert!(!b.s.keyed(), "{:?}", b.events);
    assert!(!b.s.snapshot().ours_on_air, "the mic's keying is not ours");
    assert_eq!(
        b.end_ours(),
        Vec::<String>::new(),
        "T 0 sends nothing at it"
    );
    assert_eq!(
        b.start(TxStart::Key, None),
        Err(Refusal::HardwarePtt {
            source: "MIC".into()
        }),
        "while the mic keys"
    );
    // The mic lets go 2 s after its press, and the radio is idle 100 ms on.
    b.advance(pressed + 2_000 + 100 - b.now);
    assert!(b.s.transmit_ready());
    b.start(TxStart::Key, None)
        .expect("the next over keys as usual");
    assert_eq!(b.transmit_wire(), ["xmit 1", "xmit 0", "xmit 1"]);
    assert_eq!(
        (
            b.count(&MIC_TOOK_OVER),
            b.count(&Event::UnkeyUnconfirmed),
            b.count(&Event::UnkeyConfirmed),
            b.s.phase()
        ),
        (1, 0, 0, Phase::Ready)
    );
    assert!(
        !b.events
            .iter()
            .any(|e| matches!(e, Event::Closed { .. } | Event::ReadbackFailed(_))),
        "{:?}",
        b.events
    );
}

/// ⭐ With a mic named, any other keying during our over still fails the readback, as today: the
/// accessory and foot-switch inputs, the radio's voice keyer, a tune, the paddle, a source no one
/// named, and the mic's own source under another client's handle or under no client when the bench
/// showed ours. No take-over is reported, the over's unkey cannot be proven, and 5 s after it the
/// operator is told. A mic named takes over no CWX either.
#[test]
fn an_unrecognised_source_during_an_over_still_alarms() {
    for (source, handle) in [
        ("ACC", None),
        ("RCA", None),
        ("DVK", None),
        ("TUNE", None),
        ("PADDLE", None),
        ("FOOT", None),
        ("MIC", Some(0x7A3C_0001)),
        ("MIC", Some(0)),
    ] {
        let what = format!("{source} {handle:?}");
        let mut b = Bench::with_mic(vec![press(300, source, handle, 2_000)], vec![], Some(MIC));
        b.start(TxStart::Key, None).expect("keying");
        b.advance_until_keyed_by(source);
        assert_eq!(b.transmit_wire(), ["xmit 1"], "{what}");
        assert_eq!(b.failures().len(), 1, "{what}");
        assert_eq!(b.end_ours(), ["xmit 0"], "{what}");
        b.advance(TRANSITION_TIMEOUT_MS);
        assert_eq!(
            (
                b.count(&MIC_TOOK_OVER),
                b.count(&Event::UnkeyUnconfirmed),
                b.closed_unconfirmed()
            ),
            (0, 1, true),
            "{what}"
        );
    }
    let mut b = Bench::with_inputs(
        vec![touch("TEST", 200, "MIC", None, 1_000)],
        vec![],
        None,
        Some(MIC),
    )
    .in_cw();
    b.start(cwx("TEST"), Some(1_500)).expect("sending");
    b.advance(10 + 200);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"TEST\" 1", "cwx clear", "xmit 0"]
    );
    assert_eq!((b.count(&TOOK_OVER), b.count(&MIC_TOOK_OVER)), (0, 0));
}

/// ⭐ THE OPERATOR'S MIC ALONE IS NOT OURS, with a mic named: with nothing of ours open and the mic
/// keying under this session's handle, rigctld's `T 0` and a stop send nothing, and the snapshot
/// does not say something of ours is on the air. A previous session's handle keeps today's rule
/// (every stop), and with no mic named, so does the mic.
#[test]
fn t_0_with_only_the_operators_mic_on_the_air_sends_nothing() {
    let mut b = Bench::with_mic(vec![], vec![PREVIOUS], Some(MIC));
    b.line(&keyed_by(OURS, "MIC"));
    assert!(!b.s.snapshot().ours_on_air);
    assert_eq!(b.end_ours(), Vec::<String>::new());
    assert_eq!(b.stop(TxStop::Unkey), StopOutcome::NothingOfOurs);
    for (mic, line) in [
        (Some(MIC), keyed_by(PREVIOUS, "MIC")),
        (None, keyed_by(OURS, "MIC")),
    ] {
        let mut b = Bench::with_mic(vec![], vec![PREVIOUS], mic);
        b.line(&line);
        assert!(b.s.snapshot().ours_on_air, "{line}");
        assert_eq!(b.end_ours(), ["cwx clear", "xmit 0"], "{line}");
    }
}
