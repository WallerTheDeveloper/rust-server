use rust_server::network::udp::UdpServer;
use rust_server::protocol::client::ClientMessage;
use prost::Message;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    let server = UdpServer::bind("127.0.0.1:9000").await?;
    tracing::info!("Game server started");

    loop {
        let (data, addr) = server.receive().await?;
        tracing::info!("Received {} bytes from {}", data.len(), addr);

        match ClientMessage::decode(&data[..]) {
            Ok(msg) => {
                tracing::info!("Received message from {}: {:?}", addr, msg);
            }
            Err(e) => {
                tracing::warn!("Failed to decode message: {}", e);
            }
        }
    }
}