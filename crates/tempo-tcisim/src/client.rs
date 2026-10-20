//! A minimal blocking WebSocket client: what the recorder speaks to a real server with, and what
//! the simulator's own tests drive it with.
//!
//! It knows nothing of TCI beyond text and binary messages; Nexus's TCI client is elsewhere
//! (`tempo-net::tci`). The recorder wraps a [`Peer`] privately so that its only way to write is
//! its observe-only command type ([`crate::record`]).

use std::io;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use tungstenite::{Message, WebSocket};

/// What arrived from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Text(String),
    Binary(Vec<u8>),
    /// The answer to [`Peer::ping`].
    Pong,
    /// The server closed the WebSocket, or the connection ended.
    Closed,
}

/// One WebSocket connection to a server.
pub struct Peer {
    ws: WebSocket<TcpStream>,
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl Peer {
    /// Connect to `addr` and finish the WebSocket handshake, each within `timeout`.
    pub fn connect(addr: SocketAddr, timeout: Duration) -> io::Result<Peer> {
        let tcp = TcpStream::connect_timeout(&addr, timeout)?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(timeout))?;
        let url = format!("ws://{addr}/");
        let (ws, _) = tungstenite::client(url.as_str(), tcp).map_err(other)?;
        Ok(Peer { ws })
    }

    /// This end's address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.ws.get_ref().local_addr()
    }

    /// Send one text message.
    pub fn send_text(&mut self, text: &str) -> io::Result<()> {
        self.ws.send(Message::text(text)).map_err(other)
    }

    /// Send one binary message.
    pub fn send_binary(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        self.ws.send(Message::binary(bytes)).map_err(other)
    }

    /// A WebSocket ping; the server's pong comes back as [`Incoming::Pong`].
    pub fn ping(&mut self) -> io::Result<()> {
        self.ws
            .send(Message::Ping(tungstenite::Bytes::new()))
            .map_err(other)
    }

    /// The next message, waiting at most `timeout`: `None` when nothing came.
    pub fn recv(&mut self, timeout: Duration) -> io::Result<Option<Incoming>> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.ws
                .get_mut()
                .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            match self.ws.read() {
                Ok(Message::Text(t)) => return Ok(Some(Incoming::Text(t.as_str().to_string()))),
                Ok(Message::Binary(b)) => return Ok(Some(Incoming::Binary(b.to_vec()))),
                Ok(Message::Close(_)) => return Ok(Some(Incoming::Closed)),
                Ok(Message::Pong(_)) => return Ok(Some(Incoming::Pong)),
                Ok(_) => {} // a ping, answered by tungstenite
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(Some(Incoming::Closed))
                }
                Err(tungstenite::Error::Protocol(_)) => return Ok(Some(Incoming::Closed)),
                Err(e) => return Err(other(e)),
            }
        }
    }

    /// Close cleanly: a close frame, then wait (at most `timeout`) for the server's.
    pub fn close(mut self, timeout: Duration) {
        if self.ws.close(None).is_err() {
            return;
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.recv(deadline.saturating_duration_since(Instant::now())) {
                Ok(Some(Incoming::Closed)) | Ok(None) | Err(_) => return,
                Ok(Some(_)) => {}
            }
        }
    }

    /// End the TCP connection with no close frame, as a lost network would.
    pub fn abort(self) {
        let _ = self.ws.get_ref().shutdown(Shutdown::Both);
    }
}
