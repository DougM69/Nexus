//! The pipe through a station's room, both ends' byte stream: on the internet the command link runs
//! its TLS inside a pipe the Remote service opens between this computer and the station
//! (`remote/src/pipe.ts`), which passes each binary message between the two ends unread. Each end
//! turns its WebSocket into one byte stream ([`bridge`]) and runs TLS over it: the station's end in
//! src-tauri's `remote_service::tunnel`, this computer's in [`crate::link::Link::connect_tunnel`].
//! The room only ever holds ciphertext.
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

/// The largest message either end of the pipe may send: the room's bound (`PIPE_MESSAGE_BYTES`).
pub const PIPE_MESSAGE_BYTES: usize = 64 * 1024;
/// What is read from TLS for one message: a TLS record and a little more, well inside the bound.
const CHUNK: usize = 16 * 1024 + 256;

/// The pipe as one byte stream, for TLS: what the room passes in binary messages, in order, and what
/// TLS writes, a message at a time. Two pumps, one each way, so neither waits on the other. Text, a
/// close, an error or the end of either side ends both, and the stream reads its end.
pub fn bridge<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<()>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (ours, theirs) = tokio::io::duplex(2 * PIPE_MESSAGE_BYTES);
    let task = tokio::spawn(async move {
        let (mut sink, mut source) = socket.split();
        let (mut from_tls, mut to_tls) = tokio::io::split(theirs);
        let inward = async {
            while let Some(Ok(message)) = source.next().await {
                match message {
                    Message::Binary(bytes) => {
                        if to_tls.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    // Answered by the socket itself.
                    Message::Ping(_) | Message::Pong(_) => {}
                    _ => break,
                }
            }
            let _ = to_tls.shutdown().await;
        };
        let outward = async {
            let mut chunk = vec![0; CHUNK];
            loop {
                match from_tls.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sink
                            .send(Message::Binary(chunk[..n].to_vec().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            let _ = sink.close().await;
        };
        tokio::select! {
            () = inward => {}
            () = outward => {}
        }
    });
    (ours, task)
}
