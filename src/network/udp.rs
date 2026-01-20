use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::io::Result;

pub struct UdpServer {
    socket: UdpSocket,
}

impl UdpServer {
    pub async fn bind(addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        tracing::info!("UDP server listening on {}", addr);
        Ok(Self { socket })
    }

    pub async fn recv(&self) -> Result<(Vec<u8>, SocketAddr)> {
        let mut buf = vec![0u8; 1024];
        let (len, addr) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(len);
        Ok((buf, addr))
    }

    pub async fn send(&self, data: &[u8], addr: SocketAddr) -> Result<()> {
        self.socket.send_to(data, addr).await?;
        Ok(())
    }

    pub async fn send_to_many(&self, data: &[u8], addrs: &[SocketAddr]) {
        for addr in addrs {
            if let Err(e) = self.socket.send_to(data, addr).await {
                tracing::warn!("Failed to send to {}: {}", addr, e);
            }
        }
    }
}