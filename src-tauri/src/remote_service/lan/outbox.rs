//! One connection's writer, at either end of the command link (`super::wire`): what is handed to it
//! goes on the socket in this order, written by a task of its own, so whoever hands it over never
//! waits for the network.
//!
//! 1. **Stop first** ([`Outbox::urgent`]): a Stop and its answer go ahead of everything waiting.
//! 2. **Then every other message, in turn** ([`Outbox::send`]), in the order handed over. A message
//!    of more than one frame sends one frame and goes to the back, so a big answer never holds the
//!    small ones behind it, and the small ones keep their order among themselves.
//! 3. **The feeds, latest only** ([`Outbox::feed`]): each feed holds one value waiting, and a newer
//!    one takes its place, so a feed never falls behind the shack's state. A value is numbered and
//!    deflated as it is taken to be written, never before: the numbers on the wire run 1, 2, 3 with
//!    no gap, and each feed's deflate stream holds exactly what was sent. The feeds and the messages
//!    in turn take turns with each other. The snapshot's numbers are where the state echo starts:
//!    each is recorded as it is given, with when its state was taken ([`Outbox::keep_time`]).
//!
//! A frame the socket does not take in time ends the connection, as on the relay's road
//! (`transport::Outbound`), and so does a queue the other end lets grow past [`WAITING`] messages:
//! it has stopped reading. Nothing handed over is dropped, but a feed's value a newer one replaced.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::stop_road::StateClock;
use super::wire::{self, Deflate, Whole};

/// Messages waiting to be written before the other end is judged to have stopped reading.
pub(crate) const WAITING: usize = 256;

/// A connection's writer. Cloned into whatever answers on the connection.
#[derive(Clone)]
pub(crate) struct Outbox {
    shared: Arc<Shared>,
}

struct Shared {
    queues: Mutex<Queues>,
    ready: Notify,
}

#[derive(Default)]
struct Queues {
    /// Each message as its frames: sent whole, one after another, before anything else.
    urgent: VecDeque<VecDeque<Message>>,
    /// Each message as its frames, a frame a turn.
    turns: VecDeque<VecDeque<Message>>,
    feeds: Vec<Slot>,
    /// Messages in `urgent` and `turns`.
    waiting: usize,
    /// The last stream number given to a message sent in pieces.
    stream: u32,
    /// Whether the feeds have the next turn.
    feeds_next: bool,
    /// A frame is on its way to the socket.
    writing: bool,
    /// The feed whose turn comes first.
    next_feed: usize,
    /// The feed whose numbers are recorded as they are given, and where.
    clock: Option<(&'static str, Arc<StateClock>)>,
    /// No more is written: the socket failed, the queue overflowed, or the connection ended.
    closed: bool,
}

/// One feed: the value waiting (with when it was taken), the count of values sent, its deflate
/// stream, and what is left of the value being written.
struct Slot {
    name: &'static str,
    latest: Option<(Vec<u8>, Instant)>,
    sent: u64,
    deflate: Deflate,
    frames: VecDeque<Message>,
}

enum Take {
    Frame(Message),
    Nothing,
    Closed,
}

fn lock(queues: &Mutex<Queues>) -> MutexGuard<'_, Queues> {
    queues
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `whole` as the frames it goes in.
fn frames(whole: Whole, stream: &mut u32) -> VecDeque<Message> {
    if wire::fits(&whole) {
        return VecDeque::from([match whole {
            Whole::Text(text) => Message::Text(text.into()),
            Whole::Binary(bytes) => Message::Binary(bytes.into()),
        }]);
    }
    *stream = stream.wrapping_add(1);
    wire::split(whole, *stream)
        .into_iter()
        .map(|frame| Message::Binary(frame.into()))
        .collect()
}

/// How long one frame may take to reach the wire: the relay road's rule (`transport`), 2 s and a
/// second for each 32 KiB.
fn deadline(bytes: usize) -> Duration {
    Duration::from_secs(2) + Duration::from_millis((bytes / 32) as u64)
}

impl Outbox {
    /// The writer for `sink`, and its task, which ends when the connection's writing does.
    pub(crate) fn spawn<S>(
        mut sink: SplitSink<WebSocketStream<S>, Message>,
    ) -> (Self, tokio::task::JoinHandle<()>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let shared = Arc::new(Shared {
            queues: Mutex::new(Queues::default()),
            ready: Notify::new(),
        });
        let outbox = Self {
            shared: shared.clone(),
        };
        let writer = tokio::spawn(async move {
            loop {
                let frame = match shared.take() {
                    Take::Frame(frame) => frame,
                    Take::Nothing => {
                        shared.ready.notified().await;
                        continue;
                    }
                    Take::Closed => return,
                };
                let written = tokio::time::timeout(deadline(frame.len()), sink.send(frame)).await;
                let mut queues = lock(&shared.queues);
                queues.writing = false;
                if !matches!(written, Ok(Ok(()))) {
                    queues.closed = true;
                    return;
                }
            }
        });
        (outbox, writer)
    }

    /// A Stop, or its answer: ahead of everything waiting. `Err` once the connection is over.
    pub(crate) fn urgent(&self, whole: Whole) -> Result<(), ()> {
        self.queue(whole, true)
    }

    /// Any other message, in its turn. `Err` once the connection is over.
    pub(crate) fn send(&self, whole: Whole) -> Result<(), ()> {
        self.queue(whole, false)
    }

    /// A message of JSON text, in its turn.
    pub(crate) fn text(&self, text: String) -> Result<(), ()> {
        self.send(Whole::Text(text))
    }

    /// A ping, in its turn: the other end then knows this one is there.
    pub(crate) fn ping(&self) -> Result<(), ()> {
        let mut queues = lock(&self.shared.queues);
        if queues.closed {
            return Err(());
        }
        queues.waiting += 1;
        queues
            .turns
            .push_back(VecDeque::from([Message::Ping(Vec::new().into())]));
        drop(queues);
        self.shared.ready.notify_one();
        Ok(())
    }

    /// The feed `name`'s newest value, as JSON, taken at `taken`, in place of any value of it still
    /// waiting.
    pub(crate) fn feed(&self, name: &'static str, json: Vec<u8>, taken: Instant) {
        let mut queues = lock(&self.shared.queues);
        match queues.feeds.iter_mut().find(|slot| slot.name == name) {
            Some(slot) => slot.latest = Some((json, taken)),
            None => queues.feeds.push(Slot {
                name,
                latest: Some((json, taken)),
                sent: 0,
                deflate: Deflate::default(),
                frames: VecDeque::new(),
            }),
        }
        drop(queues);
        self.shared.ready.notify_one();
    }

    /// Records each number the feed `name` is given in `clock`, with when its value was taken: the
    /// clock Stop's second road reads a heartbeat's state echo by (`super::stop_road`).
    pub(crate) fn keep_time(&self, name: &'static str, clock: Arc<StateClock>) {
        lock(&self.shared.queues).clock = Some((name, clock));
    }

    /// Waits, for at most `within`, until everything handed over is on the socket: a refusal said
    /// before the connection ends.
    pub(crate) async fn written(&self, within: Duration) {
        let settled = || {
            let queues = lock(&self.shared.queues);
            queues.closed
                || (queues.waiting == 0
                    && !queues.writing
                    && queues
                        .feeds
                        .iter()
                        .all(|slot| slot.latest.is_none() && slot.frames.is_empty()))
        };
        let _ = tokio::time::timeout(within, async {
            while !settled() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
    }

    fn queue(&self, whole: Whole, urgent: bool) -> Result<(), ()> {
        let mut queues = lock(&self.shared.queues);
        if queues.closed {
            return Err(());
        }
        if queues.waiting >= WAITING {
            queues.closed = true;
            drop(queues);
            self.shared.ready.notify_one();
            return Err(());
        }
        queues.waiting += 1;
        let message = frames(whole, &mut queues.stream);
        if urgent {
            queues.urgent.push_back(message);
        } else {
            queues.turns.push_back(message);
        }
        drop(queues);
        self.shared.ready.notify_one();
        Ok(())
    }
}

impl Shared {
    fn take(&self) -> Take {
        let mut queues = lock(&self.queues);
        let q = &mut *queues;
        if q.closed {
            return Take::Closed;
        }
        let frame = take_frame(q);
        if matches!(frame, Take::Frame(_)) {
            q.writing = true;
        }
        frame
    }
}

/// The next frame, by the order in the module header.
fn take_frame(q: &mut Queues) -> Take {
    if let Some(frame) = front(&mut q.urgent, &mut q.waiting, false) {
        return Take::Frame(frame);
    }
    let feeds_ready = q
        .feeds
        .iter()
        .any(|slot| !slot.frames.is_empty() || slot.latest.is_some());
    if feeds_ready && (q.feeds_next || q.turns.is_empty()) {
        q.feeds_next = false;
        return match feed_frame(q) {
            Some(frame) => Take::Frame(frame),
            None if q.closed => Take::Closed,
            None => Take::Nothing,
        };
    }
    q.feeds_next = true;
    match front(&mut q.turns, &mut q.waiting, true) {
        Some(frame) => Take::Frame(frame),
        None => Take::Nothing,
    }
}

/// The next frame of the first message in `queue`: the message leaves once its last frame has, and
/// with `rotate`, one with frames still to send goes to the back.
fn front(
    queue: &mut VecDeque<VecDeque<Message>>,
    waiting: &mut usize,
    rotate: bool,
) -> Option<Message> {
    let mut message = queue.pop_front()?;
    let frame = message.pop_front();
    if message.is_empty() {
        *waiting = waiting.saturating_sub(1);
    } else if rotate {
        queue.push_back(message);
    } else {
        queue.push_front(message);
    }
    frame
}

/// The next frame of a feed: the rest of the value being written, or the next feed's waiting value,
/// numbered and deflated now. The feeds take turns among themselves.
fn feed_frame(q: &mut Queues) -> Option<Message> {
    let Queues {
        feeds,
        stream,
        next_feed,
        closed,
        clock,
        ..
    } = q;
    let count = feeds.len();
    for step in 0..count {
        let index = (*next_feed + step) % count;
        let slot = &mut feeds[index];
        if slot.frames.is_empty() {
            let Some((json, taken)) = slot.latest.take() else {
                continue;
            };
            let Some(deflated) = slot.deflate.value(&json) else {
                *closed = true;
                return None;
            };
            slot.sent += 1;
            if let Some((_, clock)) = clock.as_ref().filter(|(name, _)| *name == slot.name) {
                clock.sent(slot.sent, taken);
            }
            slot.frames = frames(
                Whole::Binary(wire::feed_frame(slot.name, slot.sent, &deflated)),
                stream,
            );
        }
        if let Some(frame) = slot.frames.pop_front() {
            *next_feed = (index + 1) % count;
            return Some(frame);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::protocol::Role;

    type Theirs = WebSocketStream<tokio::io::DuplexStream>;

    /// An outbox writing into an in-memory pipe that holds `room` bytes, and the other end. Nothing
    /// more is written until the other end reads.
    async fn pipe(room: usize) -> (Outbox, Theirs) {
        let (ours, theirs) = tokio::io::duplex(room);
        let ours = WebSocketStream::from_raw_socket(ours, Role::Server, None).await;
        let theirs = WebSocketStream::from_raw_socket(theirs, Role::Client, None).await;
        let (sink, _stream) = ours.split();
        let (outbox, _writer) = Outbox::spawn(sink);
        (outbox, theirs)
    }

    /// The other end's next `count` messages, each whole: text, or a feed frame as bytes.
    async fn read(theirs: &mut Theirs, count: usize) -> Vec<Whole> {
        let mut pieces = wire::Assembly::new(wire::ANSWER_BYTES);
        let mut read = Vec::new();
        while read.len() < count {
            match theirs.next().await {
                Some(Ok(Message::Text(text))) => read.push(Whole::Text(text.to_string())),
                Some(Ok(Message::Binary(frame))) if wire::is_piece(&frame) => {
                    read.extend(pieces.piece(&frame).expect("a piece"))
                }
                Some(Ok(Message::Binary(frame))) => read.push(Whole::Binary(frame.to_vec())),
                other => panic!("{other:?}"),
            }
        }
        read
    }

    fn text(text: &str) -> Whole {
        Whole::Text(text.to_string())
    }

    /// Lets the writer take what is first and stall on the full pipe.
    async fn stalled() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    /// ★ Stop first: a message handed over as urgent goes on the socket before every message
    /// waiting in turn, though they were handed over first; it waits only for the frame already
    /// being written. CONTROL: the messages in turn keep their order.
    #[tokio::test]
    async fn an_urgent_message_goes_before_everything_waiting() {
        let (outbox, mut theirs) = pipe(64).await;
        let first = "x".repeat(1000);
        outbox.text(first.clone()).unwrap();
        stalled().await;
        for n in 0..5 {
            outbox.text(format!("turn {n}")).unwrap();
        }
        outbox.urgent(text("stop")).unwrap();
        let mut expected = vec![text(&first), text("stop")];
        expected.extend((0..5).map(|n| Whole::Text(format!("turn {n}"))));
        assert_eq!(read(&mut theirs, 7).await, expected);
    }

    /// ★ Turns: a message of more than one frame sends one and goes to the back, so the messages
    /// behind it are not held for all of it, and they keep their order. CONTROL: its pieces come
    /// back as the message.
    #[tokio::test]
    async fn a_big_message_gives_way_frame_by_frame() {
        let (outbox, mut theirs) = pipe(64).await;
        outbox.text("x".repeat(1000)).unwrap();
        stalled().await;
        let big = "b".repeat(3 * wire::FRAME);
        outbox.text(big.clone()).unwrap();
        outbox.text("one".to_string()).unwrap();
        outbox.text("two".to_string()).unwrap();
        let mut frames = Vec::new();
        while frames.len() < 7 {
            match theirs.next().await {
                Some(Ok(message)) => frames.push(message),
                other => panic!("{other:?}"),
            }
        }
        let texts: Vec<Option<String>> = frames
            .iter()
            .map(|frame| match frame {
                Message::Text(text) => Some(text.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts[1..4],
            [None, Some("one".to_string()), Some("two".to_string())],
            "one piece, then the small ones"
        );
        let mut pieces = wire::Assembly::new(wire::ANSWER_BYTES);
        let whole: Vec<Whole> = frames[1..]
            .iter()
            .filter_map(|frame| match frame {
                Message::Binary(bytes) => pieces.piece(bytes).expect("a piece"),
                _ => None,
            })
            .collect();
        assert_eq!(whole, vec![Whole::Text(big)]);
    }

    /// ★ The feeds' numbers: while the socket is full, each newer value takes the place of the one
    /// waiting, and the values written are numbered 1, 2, 3 with no gap, each inflating to the
    /// value it was. CONTROL: values handed over while the socket takes them are all written.
    #[tokio::test]
    async fn a_feed_sends_its_latest_value_numbered_without_a_gap() {
        let (outbox, mut theirs) = pipe(64).await;
        outbox.text("x".repeat(1000)).unwrap();
        stalled().await;
        for n in 1..=10 {
            outbox.feed(
                "snapshot/1",
                format!("{{\"n\":{n}}}").into_bytes(),
                Instant::now(),
            );
        }
        let mut inflate = wire::Inflate::default();
        let mut feed = |whole: &Whole| match whole {
            Whole::Binary(frame) => {
                let (name, number, deflated) = wire::read_feed(frame).expect("a feed frame");
                let value = inflate.value(deflated).expect("inflates");
                (name.to_string(), number, String::from_utf8(value).unwrap())
            }
            other => panic!("{other:?}"),
        };
        let first = read(&mut theirs, 2).await;
        assert_eq!(
            feed(&first[1]),
            ("snapshot/1".to_string(), 1, "{\"n\":10}".to_string()),
            "the latest only, numbered 1"
        );
        for (n, number) in [(11, 2), (12, 3)] {
            outbox.feed(
                "snapshot/1",
                format!("{{\"n\":{n}}}").into_bytes(),
                Instant::now(),
            );
            let next = read(&mut theirs, 1).await;
            assert_eq!(
                feed(&next[0]),
                ("snapshot/1".to_string(), number, format!("{{\"n\":{n}}}"))
            );
        }
    }

    /// ★ A peer that has stopped reading ends the connection: past [`WAITING`] messages waiting,
    /// the next is refused and the writing is over. CONTROL: [`WAITING`] messages are taken.
    #[tokio::test]
    async fn a_queue_the_peer_lets_grow_ends_the_writing() {
        let (outbox, _theirs) = pipe(64).await;
        outbox.text("x".repeat(1000)).unwrap();
        stalled().await;
        for n in 0..WAITING {
            assert_eq!(outbox.text(format!("{n}")), Ok(()), "{n}");
        }
        assert_eq!(outbox.text("one too many".to_string()), Err(()));
        assert_eq!(outbox.urgent(text("stop")), Err(()), "the writing is over");
        assert_eq!(outbox.ping(), Err(()));
    }
}
