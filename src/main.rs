use rust_server::network::udp::UdpServer;
use rust_server::protocol::client::{ClientMessage, client_message::Payload};
use rust_server::session::SessionManager;
use prost::Message;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rust_server=debug".parse().unwrap()),
        )
        .init();

    let server = UdpServer::bind("127.0.0.1:9000").await?;
    tracing::info!("Game server started");

    let sessions = Arc::new(Mutex::new(SessionManager::new(10)));

    let sessions_cleanup = sessions.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut sessions = sessions_cleanup.lock().await;
            let timed_out = sessions.cleanup_timed_out();
            for session in timed_out {
                // TODO: notify room that player left
                tracing::debug!("Cleaned up session for player {}", session.player_id);
            }
        }
    });

    loop {
        let (data, addr) = server.receive().await?;
        tracing::debug!("Received {} bytes from {}", data.len(), addr);

        // Decode message
        let msg = match ClientMessage::decode(&data[..]) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("Failed to decode message from {}: {}", addr, e);
                continue;
            }
        };

        let mut sessions = sessions.lock().await;

        match msg.payload {
            Some(Payload::JoinRoom(join)) => {
                let session = sessions.register(addr, join.player_name.clone());
                tracing::info!(
                    "Player {} ({}) wants to join room '{}'",
                    session.player_id,
                    join.player_name,
                    join.room_code
                );

                // TODO: Add player to room
                // TODO: Send RoomJoined response
            }

            Some(Payload::PlayerInput(input)) => {
                sessions.update_last_seen(&addr);

                if let Some(session) = sessions.get_by_addr(&addr) {
                    tracing::debug!(
                        "Player {} input: tick={}, dir={:.2}, dash={}",
                        session.player_id,
                        input.tick,
                        input.direction,
                        input.dash
                    );
                    // TODO: Queue input for processing
                } else {
                    tracing::warn!("Input from unknown player: {}", addr);
                }
            }

            Some(Payload::LeaveRoom(_)) => {
                if let Some(session) = sessions.remove(&addr) {
                    tracing::info!("Player {} left", session.player_id);
                    // TODO: Remove from room
                    // TODO: Notify other players
                }
            }

            Some(Payload::Ready(_)) => {
                sessions.update_last_seen(&addr);
                if let Some(session) = sessions.get_by_addr(&addr) {
                    tracing::info!("Player {} is ready", session.player_id);
                    // TODO: Mark player as ready in room
                }
            }

            None => {
                tracing::warn!("Empty message from {}", addr);
            }
        }
    }
}