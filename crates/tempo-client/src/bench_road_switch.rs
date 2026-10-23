//! The bench's road switch, the half that blocks: in test builds only (the `bench-road-switch`
//! feature, which src-tauri's feature of the same name turns on and nothing else may), the switch
//! client mode opens its link's roads under, and how each of this computer's transports reads it.
//! The command that moves it, and the banner that shows it, are src-tauri's (its
//! `bench_road_switch`).
//!
//! ## What a blocked road is
//!
//! What a firewall on this computer would do to it, both ways, so the station sees only what it
//! would see then:
//! - **The command link** (TCP to the station's listener, 42075 on the LAN): nothing is written to
//!   the socket and nothing is read from it ([`Gated`]). What this computer writes meanwhile is
//!   held here, as a socket's own buffer holds it (up to [`HELD`]), and goes out in order once the
//!   link is open again, as TCP sends it again once a firewall lets it through. No new connection
//!   is made: one is given up after the connect's own wait, as an unanswered one is
//!   ([`crate::road::open`]).
//! - **Stop's road** (UDP, 42077): every datagram this computer seals is dropped instead of sent,
//!   and every one that arrives is dropped unread ([`crate::stop_road`]).
//! - **The audio road** (UDP, 42076) is never blocked.
//!
//! Nothing at the station changes, and nothing here decides anything about a transmission. The
//! design's rules then decide, as they are: a start needs both roads, so with Stop's road blocked
//! presence lapses at the station within five seconds and here soon after, and with the link
//! blocked the state that road's heartbeat names stops moving, so presence lapses within about
//! seven; and a Stop goes by whichever road is open.
//!
//! Where this differs from a firewall: the station's own TCP still has what it sends acknowledged
//! by this computer's, where a firewall would leave it unacknowledged, so the station's writes
//! wait for this computer's receive buffer rather than its own send buffer (either way it ends the
//! connection by its own rules: 15 s with nothing heard, or a frame its socket did not take in
//! time). This computer's link never ends by itself while blocked, since its TCP never has to give
//! up sending again. And a link this computer closes while it is blocked (Leave, or quitting)
//! still closes at the station: the operating system says so as the socket goes.
//!
//! ## One road at a time, and never kept
//!
//! The switch has three positions ([`Position`]): Normal, Block the command link and Block the Stop
//! road. Blocking one road while the other is blocked is refused (`oneRoadAtATime`): Normal comes
//! first, so the two are never blocked at once. The position lives in memory with client mode's
//! state, so every launch starts at Normal.
//!
//! ## Which roads it reaches
//!
//! Only the roads client mode opens for its link: client mode opens each link under its switch
//! ([`Switch::over`]), and each transport keeps the switch it was opened under
//! ([`Switch::opening`]). Nothing else this computer opens to a station (the stations window's
//! road, a pairing) is ever under it, nor anything a test opens without one.
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{ready, Context, Poll, Waker};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The command a test build's screen moves the switch with.
pub const COMMAND: &str = "bench_road_switch";
/// The most this computer holds of what it writes to a blocked link: about what a socket's own
/// send buffer takes before a write waits.
pub const HELD: usize = 256 * 1024;

/// Where the switch is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Position {
    /// Both roads as they are.
    #[default]
    Normal,
    /// The command link blocked.
    BlockLink,
    /// Stop's road blocked.
    BlockStopRoad,
}

impl Position {
    fn of(byte: u8) -> Self {
        match byte {
            1 => Position::BlockLink,
            2 => Position::BlockStopRoad,
            _ => Position::Normal,
        }
    }

    fn byte(self) -> u8 {
        match self {
            Position::Normal => 0,
            Position::BlockLink => 1,
            Position::BlockStopRoad => 2,
        }
    }
}

/// A road the switch can block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Road {
    /// The command link.
    Link,
    /// Stop's road.
    StopRoad,
}

/// The switch: its position, and the link's reads and writes waiting for the link to open again.
#[derive(Default)]
pub struct Switch {
    position: AtomicU8,
    waiting: Mutex<Vec<Waker>>,
}

tokio::task_local! {
    /// The switch over the link client mode is opening now ([`Switch::over`]).
    static OPENING: Arc<Switch>;
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Switch {
    /// Where the switch is now.
    pub fn position(&self) -> Position {
        Position::of(self.position.load(Ordering::SeqCst))
    }

    /// Whether `road` is blocked now.
    pub fn blocks(&self, road: Road) -> bool {
        matches!(
            (self.position(), road),
            (Position::BlockLink, Road::Link) | (Position::BlockStopRoad, Road::StopRoad)
        )
    }

    /// Moves the switch to `to`: the position it is in now, or `oneRoadAtATime` when `to` blocks a
    /// road while the other is blocked, which leaves it where it was. Once the link is open again,
    /// every read and write that waited for it goes on.
    pub fn set(&self, to: Position) -> Result<Position, &'static str> {
        self.position
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                let now = Position::of(now);
                (now == Position::Normal || to == Position::Normal || now == to)
                    .then_some(to.byte())
            })
            .map_err(|_| "oneRoadAtATime")?;
        if !self.blocks(Road::Link) {
            let woken = std::mem::take(&mut *lock(&self.waiting));
            for waker in woken {
                waker.wake();
            }
        }
        Ok(to)
    }

    /// Whether the link is blocked now; while it is, `waker` is woken once it is open again.
    fn waits_for_the_link(&self, waker: &Waker) -> bool {
        let mut waiting = lock(&self.waiting);
        if !self.blocks(Road::Link) {
            return false;
        }
        if !waiting.iter().any(|kept| kept.will_wake(waker)) {
            waiting.push(waker.clone());
        }
        true
    }

    /// `opening`, with every transport it opens under this switch: client mode's link, each time it
    /// connects.
    pub fn over<F: Future>(self: &Arc<Self>, opening: F) -> impl Future<Output = F::Output> {
        OPENING.scope(Arc::clone(self), opening)
    }

    /// The switch the transport being opened now is under, if it is under one.
    pub fn opening() -> Option<Arc<Self>> {
        OPENING.try_with(Arc::clone).ok()
    }
}

/// The command link's socket under the switch. While the link is blocked nothing is read, and what
/// is written is held, in order, until it is open again.
pub struct Gated<S> {
    inner: S,
    switch: Option<Arc<Switch>>,
    /// Written while the link was blocked, and not on the wire yet.
    held: Vec<u8>,
}

impl<S> Gated<S> {
    /// `inner` under `switch`, the one it was opened under; with none it is `inner` as it is.
    pub fn new(inner: S, switch: Option<Arc<Switch>>) -> Self {
        Self {
            inner,
            switch,
            held: Vec::new(),
        }
    }

    /// Whether the link is blocked now; while it is, `cx`'s task is woken once it is open again.
    fn blocked(&self, cx: &Context<'_>) -> bool {
        self.switch
            .as_ref()
            .is_some_and(|switch| switch.waits_for_the_link(cx.waker()))
    }
}

/// The socket itself, to read what it is (whether Nagle is off, say). Every byte goes through the
/// switch: nothing here writes or reads past it.
impl<S> std::ops::Deref for Gated<S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.inner
    }
}

impl<S: AsyncWrite + Unpin> Gated<S> {
    /// What was held goes on the wire before anything written after it: ready once all of it has.
    fn poll_held(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.held.is_empty() {
            let wrote = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.held))?;
            if wrote == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.held.drain(..wrote);
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Gated<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.blocked(cx) {
            return Poll::Pending;
        }
        // What was held goes out first, whichever half of the link is polled first after it opens.
        if let Poll::Ready(Err(error)) = this.poll_held(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Gated<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        if this.blocked(cx) {
            // Taken, as a socket's own buffer takes it, until that is full.
            let room = HELD.saturating_sub(this.held.len());
            if room == 0 {
                return Poll::Pending;
            }
            let took = room.min(buf.len());
            this.held.extend_from_slice(&buf[..took]);
            return Poll::Ready(Ok(took));
        }
        ready!(this.poll_held(cx))?;
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        // Held, as a socket's own buffer holds what a flush handed it.
        if this.blocked(cx) {
            return Poll::Ready(Ok(()));
        }
        ready!(this.poll_held(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.blocked(cx) {
            return Poll::Pending;
        }
        ready!(this.poll_held(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// ★ One road at a time: from Normal either road is blocked, and back; blocking the other road
    /// while one is blocked is refused `oneRoadAtATime` and leaves the switch where it was, so the
    /// two are never blocked at once. Asking for where it is already is taken. CONTROL: each block
    /// blocks its own road and only that one.
    #[test]
    fn one_road_at_a_time() {
        let switch = Switch::default();
        assert_eq!(switch.position(), Position::Normal);
        assert!(!switch.blocks(Road::Link) && !switch.blocks(Road::StopRoad));
        for (first, second) in [
            (Position::BlockLink, Position::BlockStopRoad),
            (Position::BlockStopRoad, Position::BlockLink),
        ] {
            assert_eq!(switch.set(first), Ok(first));
            assert_eq!(switch.set(first), Ok(first), "where it is already");
            assert_eq!(switch.set(second), Err("oneRoadAtATime"));
            assert_eq!(switch.position(), first, "left where it was");
            let link = first == Position::BlockLink;
            assert_eq!(
                (switch.blocks(Road::Link), switch.blocks(Road::StopRoad)),
                (link, !link),
                "{first:?} blocks its own road and only that one"
            );
            assert_eq!(switch.set(Position::Normal), Ok(Position::Normal));
            assert!(!switch.blocks(Road::Link) && !switch.blocks(Road::StopRoad));
        }
    }

    /// The two ends of a TCP connection on loopback: this computer's under `switch`, and the
    /// station's.
    async fn pair(switch: &Arc<Switch>) -> (Gated<tokio::net::TcpStream>, tokio::net::TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        let (here, there) = tokio::join!(tokio::net::TcpStream::connect(at), listener.accept());
        (
            Gated::new(here.unwrap(), Some(Arc::clone(switch))),
            there.unwrap().0,
        )
    }

    /// What `socket` reads within `within`: nothing, if nothing came.
    async fn read_within(socket: &mut (impl AsyncRead + Unpin), within: Duration) -> Vec<u8> {
        let mut got = vec![0u8; 64];
        match tokio::time::timeout(within, socket.read(&mut got)).await {
            Ok(Ok(n)) => got[..n].to_vec(),
            _ => Vec::new(),
        }
    }

    /// ★ A blocked link carries nothing either way, and then goes on where it stopped. Blocked,
    /// what this computer writes is taken and does not reach the station, and what the station
    /// writes is not read here; opened again, both arrive whole and in order, what was written
    /// while blocked ahead of what came after. CONTROL: before the block, a write reaches the
    /// other end at once both ways; a link under no switch is never held.
    #[tokio::test]
    async fn a_blocked_link_carries_nothing_and_then_goes_on_where_it_stopped() {
        const NOTHING: Duration = Duration::from_millis(300);
        let switch = Arc::new(Switch::default());
        let (mut here, mut there) = pair(&switch).await;
        here.write_all(b"before").await.unwrap();
        assert_eq!(read_within(&mut there, NOTHING).await, b"before", "CONTROL");
        there.write_all(b"answer").await.unwrap();
        assert_eq!(read_within(&mut here, NOTHING).await, b"answer", "CONTROL");

        assert_eq!(switch.set(Position::BlockLink), Ok(Position::BlockLink));
        here.write_all(b"held").await.unwrap();
        here.flush().await.unwrap();
        assert_eq!(
            read_within(&mut there, NOTHING).await,
            b"",
            "written while blocked, it reached the station"
        );
        there.write_all(b"unread").await.unwrap();
        assert_eq!(
            read_within(&mut here, NOTHING).await,
            b"",
            "the station's write was read while blocked"
        );

        assert_eq!(switch.set(Position::Normal), Ok(Position::Normal));
        here.write_all(b"+after").await.unwrap();
        let mut arrived = Vec::new();
        while arrived.len() < b"held+after".len() {
            let more = read_within(&mut there, Duration::from_secs(2)).await;
            assert!(!more.is_empty(), "only {arrived:?} arrived");
            arrived.extend(more);
        }
        assert_eq!(arrived, b"held+after", "in order, the held first");
        assert_eq!(
            read_within(&mut here, Duration::from_secs(2)).await,
            b"unread"
        );

        // A link under no switch is the socket as it is.
        let (mut free, mut its_station) = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = listener.local_addr().unwrap();
            let (here, there) = tokio::join!(tokio::net::TcpStream::connect(at), listener.accept());
            (Gated::new(here.unwrap(), None), there.unwrap().0)
        };
        assert_eq!(switch.set(Position::BlockLink), Ok(Position::BlockLink));
        free.write_all(b"free").await.unwrap();
        assert_eq!(read_within(&mut its_station, NOTHING).await, b"free");
    }
}
