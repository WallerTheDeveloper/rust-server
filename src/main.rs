use rust_server::network::udp::UdpServer;
use rust_server::protocol::client::{ClientMessage, client_message::Payload};
use rust_server::protocol::server::{ServerMessage, server_message, RoomJoined, PlayerInfo, PlayerLeft, Error};
use rust_server::session::SessionManager;
use rust_server::room::RoomManager;
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

    let server = Arc::new(UdpServer::bind("127.0.0.1:9000").await?);
    tracing::info!("Game server started");

    let sessions = Arc::new(Mutex::new(SessionManager::new(10)));
    let rooms = Arc::new(Mutex::new(RoomManager::new(4)));

    // Cleanup task
    let sessions_clone = sessions.clone();
    let rooms_clone = rooms.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut sessions_lock = sessions_clone.lock().await;
            let mut rooms_lock = rooms_clone.lock().await;

            let timed_out = sessions_lock.cleanup_timed_out();
            for session in timed_out {
                if let Some(room_code) = session.room_code {
                    rooms_lock.leave_room(session.player_id);
                    // TODO: Notify other players in room
                }
            }
        }
    });

    // Main loop
    loop {
        let (data, addr) = server.receive().await?;
        tracing::debug!("Received {} bytes from {}", data.len(), addr);

        let msg = match ClientMessage::decode(&data[..]) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("Failed to decode message from {}: {}", addr, e);
                continue;
            }
        };

        let mut sessions_lock = sessions.lock().await;
        let mut rooms_lock = rooms.lock().await;

        match msg.payload {
            Some(Payload::JoinRoom(join)) => {
                // Register session
                let session = sessions_lock.register(addr, join.player_name.clone());
                let player_id = session.player_id;

                // Join or create room
                match rooms_lock.join_room(&join.room_code, player_id, join.player_name.clone()) {
                    Ok(room) => {
                        // Update session with room code
                        if let Some(session) = sessions_lock.get_by_addr_mut(&addr) {
                            session.room_code = Some(room.code.clone());
                        }

                        // Build response
                        let players: Vec<PlayerInfo> = room.players.values().map(|p| {
                            PlayerInfo {
                                player_id: p.player_id,
                                name: p.name.clone(),
                                ready: p.ready,
                            }
                        }).collect();

                        let response = ServerMessage {
                            payload: Some(server_message::Payload::RoomJoined(RoomJoined {
                                player_id,
                                room_code: room.code.clone(),
                                players,
                            })),
                        };

                        let bytes = response.encode_to_vec();
                        if let Err(e) = server.send(&bytes, addr).await {
                            tracing::error!("Failed to send RoomJoined: {}", e);
                        }

                        tracing::info!(
                            "Player {} ({}) joined room '{}' ({} players)",
                            player_id,
                            join.player_name,
                            room.code,
                            room.player_count()
                        );

                        // TODO: Notify other players in room
                    }
                    Err(e) => {
                        let response = ServerMessage {
                            payload: Some(server_message::Payload::Error(Error {
                                message: format!("Failed to join room: {:?}", e),
                            })),
                        };
                        let bytes = response.encode_to_vec();
                        let _ = server.send(&bytes, addr).await;
                    }
                }
            }

            Some(Payload::PlayerInput(input)) => {
                sessions_lock.update_last_seen(&addr);

                if let Some(session) = sessions_lock.get_by_addr(&addr) {
                    tracing::debug!(
                        "Player {} input: tick={}, dir={:.2}, dash={}",
                        session.player_id,
                        input.tick,
                        input.direction,
                        input.dash
                    );
                    // TODO: Queue input for game tick
                }
            }

            Some(Payload::LeaveRoom(_)) => {
                if let Some(session) = sessions_lock.get_by_addr(&addr) {
                    let player_id = session.player_id;

                    if let Some(room_code) = rooms_lock.leave_room(player_id) {
                        // Update session
                        if let Some(session) = sessions_lock.get_by_addr_mut(&addr) {
                            session.room_code = None;
                        }

                        let response = ServerMessage {
                            payload: Some(server_message::Payload::PlayerLeft(PlayerLeft {
                                player_id,
                            })),
                        };
                        let bytes = response.encode_to_vec();

                        // Send to all remaining players in room
                        for other_player_id in rooms_lock.get_room_player_ids(&room_code) {
                            if let Some(other_session) = sessions_lock.get_by_player_id(other_player_id) {
                                let _ = server.send(&bytes, other_session.addr).await;
                            }
                        }

                        tracing::info!("Player {} left room {}", player_id, room_code);
                    }
                }
            }

            Some(Payload::Ready(_)) => {
                sessions_lock.update_last_seen(&addr);

                if let Some(session) = sessions_lock.get_by_addr(&addr) {
                    let player_id = session.player_id;

                    if let Ok(room) = rooms_lock.set_ready(player_id, true) {
                        tracing::info!(
                            "Player {} is ready in room {} ({}/{})",
                            player_id,
                            room.code,
                            room.players.values().filter(|p| p.ready).count(),
                            room.player_count()
                        );

                        if room.all_ready() && room.player_count() >= 2 {
                            tracing::info!("Room {} - all players ready! Starting game...", room.code);
                            // TODO: Start game
                        }
                    }
                }
            }

            None => {
                tracing::warn!("Empty message from {}", addr);
            }
        }
    }
}