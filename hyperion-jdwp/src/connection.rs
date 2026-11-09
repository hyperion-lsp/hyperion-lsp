// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use bytes::BytesMut;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::{Encoder, FramedRead};

use crate::commands::{Command, IdSizes};
use crate::error::{JdwpError, Result};
use crate::events::EventSet;
use crate::packet::{CommandPacket, JdwpCodec, Packet, ReplyPacket, HANDSHAKE};

const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Removes an in-flight request if its future is cancelled or exits before a reply arrives.
struct PendingReplyGuard<'a> {
    id: u32,
    pending: &'a DashMap<u32, oneshot::Sender<ReplyPacket>>,
}

impl Drop for PendingReplyGuard<'_> {
    fn drop(&mut self) {
        self.pending.remove(&self.id);
    }
}

/// A connection to a JVM debug agent via the JDWP protocol.
///
/// After connecting, call `id_sizes()` to query the JVM's ID field widths —
/// these are needed by most commands and reply parsers.
pub struct JdwpConnection {
    /// Sender half for writing command packets to the JVM.
    writer_tx: mpsc::Sender<CommandPacket>,
    /// Monotonically increasing packet ID.
    next_id: AtomicU32,
    /// Pending reply map: request ID → oneshot sender.
    pending: Arc<DashMap<u32, oneshot::Sender<ReplyPacket>>>,
    /// Channel for incoming events from the JVM.
    event_rx: mpsc::Receiver<EventSet>,
    /// ID sizes negotiated with the JVM.
    id_sizes: IdSizes,
    /// Handle to the background reader task.
    _reader_handle: tokio::task::JoinHandle<()>,
    /// Handle to the background writer task.
    _writer_handle: tokio::task::JoinHandle<()>,
}

impl JdwpConnection {
    /// Connect to a JVM debug agent at the given address, perform the JDWP handshake,
    /// and negotiate ID sizes.
    pub async fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| JdwpError::Timeout)??;
        let peer_addr = stream.peer_addr()?;

        // Perform handshake: send "JDWP-Handshake", expect the same back.
        let mut handshake_buf = [0u8; 14];
        tokio::time::timeout(CONNECT_TIMEOUT, async {
            stream.write_all(HANDSHAKE).await?;
            stream.read_exact(&mut handshake_buf).await
        })
        .await
        .map_err(|_| JdwpError::Timeout)??;

        if &handshake_buf != HANDSHAKE {
            return Err(JdwpError::HandshakeFailed);
        }

        tracing::info!("JDWP handshake complete with {peer_addr}");

        let (read_half, write_half) = stream.into_split();

        // Pending reply map shared between the reader task and send_command.
        let pending: Arc<DashMap<u32, oneshot::Sender<ReplyPacket>>> = Arc::new(DashMap::new());

        // Event channel: reader task → consumer.
        // Buffer 256 events; if the consumer is too slow, the reader blocks (backpressure).
        let (event_tx, event_rx) = mpsc::channel::<EventSet>(256);

        // Writer channel: send_command → writer task.
        let (writer_tx, mut writer_rx) = mpsc::channel::<CommandPacket>(256);

        // ID sizes — we'll set these after the first IDSizes command.
        // Start with defaults (8 bytes each, which is correct for most JVMs).
        let id_sizes_shared = Arc::new(tokio::sync::RwLock::new(IdSizes::default()));

        // Spawn background reader task.
        let pending_clone = pending.clone();
        let id_sizes_for_reader = id_sizes_shared.clone();
        let reader_handle = tokio::spawn(async move {
            let mut framed = FramedRead::new(read_half, JdwpCodec);
            use tokio_stream::StreamExt;

            loop {
                let packet = match framed.next().await {
                    Some(Ok(p)) => p,
                    Some(Err(e)) => {
                        tracing::error!("JDWP read error: {e}");
                        break;
                    }
                    None => {
                        tracing::info!("JDWP connection closed by debuggee");
                        break;
                    }
                };

                match packet {
                    Packet::Reply(reply) => {
                        // Match reply to pending request.
                        if let Some((_, tx)) = pending_clone.remove(&reply.id) {
                            let _ = tx.send(reply);
                        } else {
                            tracing::warn!("received reply for unknown request id {}", reply.id);
                        }
                    }
                    Packet::Command(cmd) => {
                        // Events come as command_set=64, command=100 (Composite).
                        if cmd.command_set == 64 && cmd.command == 100 {
                            let id_sizes = *id_sizes_for_reader.read().await;
                            let mut data = cmd.data;
                            match EventSet::parse(&mut data, &id_sizes) {
                                Ok(event_set) => {
                                    if event_tx.send(event_set).await.is_err() {
                                        tracing::debug!("event receiver dropped, stopping reader");
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("failed to parse event: {e}");
                                }
                            }
                        } else {
                            tracing::warn!(
                                "unexpected command packet: set={}, cmd={}",
                                cmd.command_set,
                                cmd.command
                            );
                        }
                    }
                }
            }

            // Wake up any pending requests so they don't hang forever.
            pending_clone.clear();
        });

        // Spawn background writer task.
        let writer_handle = tokio::spawn(async move {
            let mut write_half = write_half;
            let mut encode_buf = BytesMut::with_capacity(1024);
            let mut codec = JdwpCodec;

            while let Some(cmd) = writer_rx.recv().await {
                encode_buf.clear();
                if let Err(e) = codec.encode(cmd, &mut encode_buf) {
                    tracing::error!("JDWP encode error: {e}");
                    break;
                }
                if let Err(e) = write_half.write_all(&encode_buf).await {
                    tracing::error!("JDWP write error: {e}");
                    break;
                }
            }
        });

        let mut conn = Self {
            writer_tx,
            next_id: AtomicU32::new(1),
            pending,
            event_rx,
            id_sizes: IdSizes::default(),
            _reader_handle: reader_handle,
            _writer_handle: writer_handle,
        };

        // Query ID sizes — this must be the first command.
        let reply = conn.send_command(Command::vm_id_sizes()).await?;
        let mut data = reply.data;
        let id_sizes = IdSizes::parse(&mut data)?;
        conn.id_sizes = id_sizes;

        // Update the shared id_sizes for the reader task.
        *id_sizes_shared.write().await = id_sizes;

        tracing::info!(
            "JDWP ID sizes: field={}, method={}, object={}, ref_type={}, frame={}",
            id_sizes.field_id_size,
            id_sizes.method_id_size,
            id_sizes.object_id_size,
            id_sizes.reference_type_id_size,
            id_sizes.frame_id_size,
        );

        Ok(conn)
    }

    /// Send a JDWP command and wait for the reply.
    pub async fn send_command(&self, cmd: Command) -> Result<ReplyPacket> {
        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);

        // Packet ID zero is conventionally reserved, and a wrapped counter must not replace an
        // older in-flight request. Reserving through the map entry makes concurrent callers safe.
        let id = loop {
            let candidate = self.next_id.fetch_add(1, Ordering::Relaxed);
            if candidate == 0 {
                continue;
            }
            if let Entry::Vacant(entry) = self.pending.entry(candidate) {
                entry.insert(
                    tx.take()
                        .expect("sender is consumed only when the loop exits"),
                );
                break candidate;
            }
        };
        let _pending_guard = PendingReplyGuard {
            id,
            pending: &self.pending,
        };

        let packet = CommandPacket {
            id,
            command_set: cmd.command_set,
            command: cmd.command,
            data: cmd.data,
        };

        // Writer backpressure and the reply share a single deadline. A stalled debuggee may
        // fill the outbound queue, so waiting for capacity must not bypass the command timeout.
        let reply = tokio::time::timeout(COMMAND_TIMEOUT, async {
            self.writer_tx
                .send(packet)
                .await
                .map_err(|_| JdwpError::ConnectionClosed)?;
            rx.await.map_err(|_| JdwpError::ConnectionClosed)
        })
        .await
        .map_err(|_| JdwpError::Timeout)??;

        if reply.error_code != 0 {
            return Err(JdwpError::Protocol(crate::error::JdwpErrorCode(
                reply.error_code,
            )));
        }

        Ok(reply)
    }

    /// Receive the next event set from the JVM.
    /// Returns `None` if the connection is closed.
    pub async fn recv_event(&mut self) -> Option<EventSet> {
        self.event_rx.recv().await
    }

    /// Take ownership of the event receiver for use in `tokio::select!`.
    /// After calling this, `recv_event()` will always return `None`.
    pub fn take_event_receiver(&mut self) -> mpsc::Receiver<EventSet> {
        // Replace with a dummy channel that immediately closes.
        let (_, empty_rx) = mpsc::channel(1);
        std::mem::replace(&mut self.event_rx, empty_rx)
    }

    /// Get the negotiated ID sizes.
    pub const fn id_sizes(&self) -> &IdSizes {
        &self.id_sizes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn command_timeout_includes_waiting_for_writer_capacity() {
        let (writer_tx, _writer_rx) = mpsc::channel(1);
        writer_tx
            .send(CommandPacket {
                id: 1,
                command_set: 1,
                command: 7,
                data: BytesMut::new(),
            })
            .await
            .unwrap();
        let (_, event_rx) = mpsc::channel(1);
        let connection = JdwpConnection {
            writer_tx,
            next_id: AtomicU32::new(2),
            pending: Arc::new(DashMap::new()),
            event_rx,
            id_sizes: IdSizes::default(),
            _reader_handle: tokio::spawn(async {}),
            _writer_handle: tokio::spawn(async {}),
        };

        // Model a writer blocked on a debuggee that has stopped reading. The full queue must
        // consume the command's timeout budget instead of hanging before that timeout starts.
        let result = tokio::time::timeout(
            COMMAND_TIMEOUT * 2,
            connection.send_command(Command::vm_version()),
        )
        .await;
        assert!(matches!(result, Ok(Err(JdwpError::Timeout))));
        assert_eq!(connection.pending.len(), 0);
    }

    #[tokio::test]
    async fn connect_refuses_non_jdwp() {
        // Start a TCP listener that sends garbage instead of the handshake.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // Read the handshake from the client but reply with garbage.
            let mut buf = [0u8; 14];
            let _ = stream.read_exact(&mut buf).await;
            stream.write_all(b"NOT-JDWP-HELLO").await.unwrap();
        });

        let result = JdwpConnection::connect(("localhost", addr.port())).await;
        assert!(matches!(result, Err(JdwpError::HandshakeFailed)));
    }
}
