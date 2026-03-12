// WebSocket transport layer for WebGL clients.
// Each WS connection is assigned a virtual SocketAddr so the existing
// session / room / game pipeline works unchanged — like adding a second
// entrance to the same building while keeping all the hallways identical.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

/// A virtual address we assign to each WebSocket client so the rest of the
/// server (sessions, rooms, game) can identify them the same way it
/// identifies UDP clients — by SocketAddr.
///
/// We use the 127.255.x.x range with incrementing ports. These never
/// collide with real UDP addresses because no real client connects from
/// the loopback range on the server machine.
fn next_virtual_addr(counter: &mut u32) -> SocketAddr {
    let id = *counter;
    *counter += 1;
    let a = ((id >> 8) & 0xFF) as u8;
    let b = (id & 0xFF) as u8;
    let port = 40000 + (id % 25000) as u16;
    SocketAddr::from(([127, 255, a, b], port))
}

/// Messages flowing between the per-connection task and the main loop.
#[derive(Debug)]
pub enum WsEvent {
    /// New connection established. Contains the virtual address and
    /// a sender half the main loop uses to push data back.
    Connected {
        virtual_addr: SocketAddr,
        sender: mpsc::UnboundedSender<Vec<u8>>,
    },
    /// Binary data received from the client (protobuf bytes).
    Data {
        virtual_addr: SocketAddr,
        payload: Vec<u8>,
    },
    /// Client disconnected.
    Disconnected {
        virtual_addr: SocketAddr,
    },
}

/// Outbound channel: main loop can push bytes to any virtual address.
/// The WsManager holds the mapping and forwards to the right WS sender.
pub struct WsManager {
    senders: HashMap<SocketAddr, mpsc::UnboundedSender<Vec<u8>>>,
}

impl WsManager {
    pub fn new() -> Self {
        Self {
            senders: HashMap::new(),
        }
    }

    pub fn register(&mut self, addr: SocketAddr, sender: mpsc::UnboundedSender<Vec<u8>>) {
        self.senders.insert(addr, sender);
    }

    pub fn unregister(&mut self, addr: &SocketAddr) {
        self.senders.remove(addr);
    }

    /// Send binary data to a WebSocket client. Returns false if the
    /// client is gone (channel closed).
    pub fn send(&self, addr: &SocketAddr, data: &[u8]) -> bool {
        if let Some(sender) = self.senders.get(addr) {
            sender.send(data.to_vec()).is_ok()
        } else {
            false
        }
    }

    /// Check if this address belongs to a WebSocket client.
    pub fn is_ws_client(&self, addr: &SocketAddr) -> bool {
        self.senders.contains_key(addr)
    }
}

/// Start the WebSocket listener. Returns a channel that delivers events
/// to the main server loop.
///
/// Call this once at startup alongside the UDP socket bind.
pub async fn start_ws_listener(
    bind_addr: &str,
) -> std::io::Result<(mpsc::UnboundedReceiver<WsEvent>, Arc<Mutex<WsManager>>)> {
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!("WebSocket server listening on {}", bind_addr);

    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let manager = Arc::new(Mutex::new(WsManager::new()));
    let counter = Arc::new(Mutex::new(0u32));

    let mgr = manager.clone();

    tokio::spawn(async move {
        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!("WS accept error: {}", e);
                    continue;
                }
            };

            tracing::info!("New WebSocket connection from {}", peer_addr);

            let event_tx = event_tx.clone();
            let mgr = mgr.clone();
            let counter = counter.clone();

            tokio::spawn(async move {
                let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                    Ok(ws) => ws,
                    Err(e) => {
                        tracing::warn!("WS handshake failed from {}: {}", peer_addr, e);
                        return;
                    }
                };

                let virtual_addr = {
                    let mut c = counter.lock().await;
                    next_virtual_addr(&mut c)
                };

                tracing::info!(
                    "WS client {} assigned virtual addr {}",
                    peer_addr,
                    virtual_addr
                );

                let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

                // Channel for sending data back to this client
                let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();

                // Register with manager
                {
                    let mut mgr = mgr.lock().await;
                    mgr.register(virtual_addr, outbound_tx.clone());
                }

                // Notify main loop of new connection
                let _ = event_tx.send(WsEvent::Connected {
                    virtual_addr,
                    sender: outbound_tx,
                });

                // Outbound task: reads from channel, writes to WS sink
                let outbound_handle = tokio::spawn(async move {
                    while let Some(data) = outbound_rx.recv().await {
                        if ws_sink.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                });

                // Inbound: read from WS, forward to main loop
                while let Some(msg_result) = ws_stream_rx.next().await {
                    match msg_result {
                        Ok(Message::Binary(data)) => {
                            let _ = event_tx.send(WsEvent::Data {
                                virtual_addr,
                                payload: data.to_vec(),
                            });
                        }
                        Ok(Message::Close(_)) => {
                            tracing::info!("WS client {} sent close", virtual_addr);
                            break;
                        }
                        Ok(Message::Ping(data)) => {
                            // Pong is handled automatically by tungstenite in most cases,
                            // but we can safely ignore ping frames here.
                            let _ = data;
                        }
                        Ok(_) => {
                            // Text frames, pong frames — ignore
                        }
                        Err(e) => {
                            tracing::debug!("WS read error from {}: {}", virtual_addr, e);
                            break;
                        }
                    }
                }

                // Cleanup
                outbound_handle.abort();
                {
                    let mut mgr = mgr.lock().await;
                    mgr.unregister(&virtual_addr);
                }
                let _ = event_tx.send(WsEvent::Disconnected { virtual_addr });

                tracing::info!("WS client {} disconnected", virtual_addr);
            });
        }
    });

    Ok((event_rx, manager))
}