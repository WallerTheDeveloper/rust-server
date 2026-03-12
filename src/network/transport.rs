// Unified send interface. Think of it like a mail service that can deliver
// via truck (UDP) or drone (WebSocket) — the sender just drops a package
// and the transport picks the right carrier based on the address.

use crate::network::udp::UdpServer;
use crate::network::ws::WsManager;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Transport {
    udp: Arc<UdpServer>,
    ws_manager: Arc<Mutex<WsManager>>,
}

impl Transport {
    pub fn new(udp: Arc<UdpServer>, ws_manager: Arc<Mutex<WsManager>>) -> Self {
        Self { udp, ws_manager }
    }

    /// Send data to a client, automatically routing through UDP or WebSocket
    /// based on whether the address is a registered WS virtual address.
    pub async fn send(&self, data: &[u8], addr: SocketAddr) -> std::io::Result<()> {
        let ws_mgr = self.ws_manager.lock().await;
        if ws_mgr.is_ws_client(&addr) {
            if ws_mgr.send(&addr, data) {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "WebSocket client channel closed",
                ))
            }
        } else {
            drop(ws_mgr); // Release lock before UDP send
            self.udp.send(data, addr).await
        }
    }

    /// Send to multiple addresses (used by tick broadcast).
    pub async fn send_to_many(&self, data: &[u8], addrs: &[SocketAddr]) {
        let ws_mgr = self.ws_manager.lock().await;

        let mut udp_addrs = Vec::new();

        for &addr in addrs {
            if ws_mgr.is_ws_client(&addr) {
                let _ = ws_mgr.send(&addr, data);
            } else {
                udp_addrs.push(addr);
            }
        }

        drop(ws_mgr);

        if !udp_addrs.is_empty() {
            self.udp.send_to_many(data, &udp_addrs).await;
        }
    }
}