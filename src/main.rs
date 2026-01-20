use rust_server::network::udp::UdpServer;
use rust_server::protocol::client::{
    ClientMessage, Ping,
    client_message::Payload,
};
use rust_server::protocol::server::{
    ServerMessage, server_message,
    RoomJoined, RoomUpdate, PlayerInfo, PlayerLeft, GameStarting, GameMessage as ServerGameMessage, Error, Pong
};
use rust_server::session::SessionManager;
use rust_server::room::{RoomManager, RoomState};
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
    tracing::info!("Relay server started");

    let sessions = Arc::new(Mutex::new(SessionManager::new(30)));
    let rooms = Arc::new(Mutex::new(RoomManager::new(4)));

    // Cleanup task for timed-out sessions
    let sessions_cleanup = sessions.clone();
    let rooms_cleanup = rooms.clone();
    let server_cleanup = server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut sessions = sessions_cleanup.lock().await;
            let mut rooms = rooms_cleanup.lock().await;

            let timed_out = sessions.cleanup_timed_out();
            for session in timed_out {
                if let Some(room_code) = &session.room_code {
                    // Notify others in room
                    let player_ids = rooms.get_room_player_ids(room_code);
                    rooms.leave_room(session.player_id);

                    let msg = ServerMessage {
                        payload: Some(server_message::Payload::PlayerLeft(PlayerLeft {
                            player_id: session.player_id,
                        })),
                    };
                    let bytes = msg.encode_to_vec();

                    for pid in player_ids {
                        if pid != session.player_id {
                            if let Some(other) = sessions.get_by_player_id(pid) {
                                let _ = server_cleanup.send(&bytes, other.addr).await;
                            }
                        }
                    }
                }
            }
        }
    });

    // Main receive loop
    loop {
        let (data, addr) = match server.recv().await {
            Ok(result) => result,
            Err(e) => {
                tracing::debug!("recv error - sent to closed port. Ignoring. Error: {}", e);
                continue;
            }
        };

        let msg = match ClientMessage::decode(&data[..]) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("Failed to decode from {}: {}", addr, e);
                continue;
            }
        };

        let mut sessions = sessions.lock().await;
        let mut rooms = rooms.lock().await;

        match msg.payload {
            Some(Payload::JoinRoom(join)) => {
                handle_join_room(&server, &mut sessions, &mut rooms, addr, join).await;
            }

            Some(Payload::LeaveRoom(_)) => {
                handle_leave_room(&server, &mut sessions, &mut rooms, addr).await;
            }

            Some(Payload::Ready(_)) => {
                handle_ready(&server, &mut sessions, &mut rooms, addr).await;
            }

            Some(Payload::GameMessage(game_msg)) => {
                handle_game_message(&server, &mut sessions, &rooms, addr, game_msg.payload).await;
            }

            Some(Payload::Ping(ping)) => {
                handle_ping(&server, &mut sessions, addr, ping).await;
            }

            None => {
                tracing::warn!("Empty message from {}", addr);
            }
        }
    }
}

async fn handle_ping(server: &UdpServer,
                     sessions: &mut SessionManager,
                     addr: std::net::SocketAddr,
                     ping: Ping) {
    sessions.ping(&addr);

    if let Some(session) = sessions.get_by_addr(&addr) {
        tracing::trace!(
            "Ping from player {} (seq={}, count={}",
            session.player_id,
            ping.sequence,
            session.ping_count
        );
    } else {
        tracing::warn!("Ping from unknown address {}", addr);
    }

    let current_server_timestamp = current_timestamp_ms();

    let pong_message = ServerMessage {
        payload: Some(server_message::Payload::Pong(Pong {
            timestamp: ping.timestamp,
            sequence: ping.sequence,
            server_time: current_server_timestamp,
        }))
    };

    match server.send(&pong_message.encode_to_vec(), addr).await {
        Ok(msg) => msg,
        Err(e) => {
            tracing::warn!("Failed to send pong: {}", e);
        }
    };
}

async fn handle_join_room(
    server: &UdpServer,
    sessions: &mut SessionManager,
    rooms: &mut RoomManager,
    addr: std::net::SocketAddr,
    join: rust_server::protocol::client::JoinRoom,
) {
    let session = sessions.register(addr, join.player_name.clone());
    let player_id = session.player_id;

    match rooms.join_room(&join.room_code, player_id, join.player_name.clone()) {
        Ok(room) => {
            let room_code = room.code.clone();
            let player_count = room.player_count();
            let player_ids: Vec<u32> = room.get_player_ids();

            // Update session with room code
            if let Some(session) = sessions.get_by_addr_mut(&addr) {
                session.room_code = Some(room_code.clone());
            }

            // Build player list
            let players: Vec<PlayerInfo> = room.players.values().map(|p| {
                PlayerInfo {
                    player_id: p.player_id,
                    name: p.name.clone(),
                    ready: p.ready,
                }
            }).collect();

            // Send RoomJoined to the joining player
            let response = ServerMessage {
                payload: Some(server_message::Payload::RoomJoined(RoomJoined {
                    player_id,
                    room_code: room_code.clone(),
                    players: players.clone(),
                })),
            };
            let _ = server.send(&response.encode_to_vec(), addr).await;

            // Notify other players in room
            let update = ServerMessage {
                payload: Some(server_message::Payload::RoomUpdate(RoomUpdate {
                    players,
                })),
            };
            let update_bytes = update.encode_to_vec();

            for pid in &player_ids {
                if *pid != player_id {
                    if let Some(other) = sessions.get_by_player_id(*pid) {
                        let _ = server.send(&update_bytes, other.addr).await;
                    }
                }
            }

            tracing::info!(
                "Player {} ({}) joined room '{}' ({} players)",
                player_id, join.player_name, room_code, player_count
            );
        }
        Err(e) => {
            let response = ServerMessage {
                payload: Some(server_message::Payload::Error(Error {
                    message: format!("Failed to join room: {:?}", e),
                })),
            };
            let _ = server.send(&response.encode_to_vec(), addr).await;
        }
    }
}

async fn handle_leave_room(
    server: &UdpServer,
    sessions: &mut SessionManager,
    rooms: &mut RoomManager,
    addr: std::net::SocketAddr,
) {
    if let Some(session) = sessions.get_by_addr(&addr) {
        let player_id = session.player_id;

        if let Some(room_code) = rooms.leave_room(player_id) {
            // Update session
            if let Some(session) = sessions.get_by_addr_mut(&addr) {
                session.room_code = None;
            }

            // Notify others
            let remaining = rooms.get_room_player_ids(&room_code);
            let msg = ServerMessage {
                payload: Some(server_message::Payload::PlayerLeft(PlayerLeft {
                    player_id,
                })),
            };
            let bytes = msg.encode_to_vec();

            for pid in remaining {
                if let Some(other) = sessions.get_by_player_id(pid) {
                    let _ = server.send(&bytes, other.addr).await;
                }
            }

            tracing::info!("Player {} left room {}", player_id, room_code);
        }
    }
}

async fn handle_ready(
    server: &UdpServer,
    sessions: &mut SessionManager,
    rooms: &mut RoomManager,
    addr: std::net::SocketAddr,
) {
    sessions.update_last_seen(&addr);

    if let Some(session) = sessions.get_by_addr(&addr) {
        let player_id = session.player_id;

        if let Ok(room) = rooms.set_ready(player_id, true) {
            let room_code = room.code.clone();
            let all_ready = room.all_ready();
            let player_count = room.player_count();
            let player_ids = room.get_player_ids();

            // Build updated player list
            let players: Vec<PlayerInfo> = room.players.values().map(|p| {
                PlayerInfo {
                    player_id: p.player_id,
                    name: p.name.clone(),
                    ready: p.ready,
                }
            }).collect();

            tracing::info!(
                "Player {} ready in room {} ({}/{})",
                player_id, room_code,
                room.players.values().filter(|p| p.ready).count(),
                player_count
            );

            // Notify all players of updated ready status
            let update = ServerMessage {
                payload: Some(server_message::Payload::RoomUpdate(RoomUpdate {
                    players,
                })),
            };
            let update_bytes = update.encode_to_vec();

            for pid in &player_ids {
                if let Some(other) = sessions.get_by_player_id(*pid) {
                    let _ = server.send(&update_bytes, other.addr).await;
                }
            }

            // Check if game should start
            if all_ready && player_count >= 2 {
                // Update room state
                if let Some(room) = rooms.get_player_room_mut(player_id) {
                    room.state = RoomState::Playing;
                }

                // Notify all players game is starting
                let starting = ServerMessage {
                    payload: Some(server_message::Payload::GameStarting(GameStarting {
                        countdown_seconds: 3,
                    })),
                };
                let starting_bytes = starting.encode_to_vec();

                for pid in &player_ids {
                    if let Some(other) = sessions.get_by_player_id(*pid) {
                        let _ = server.send(&starting_bytes, other.addr).await;
                    }
                }

                tracing::info!("Room {} starting game!", room_code);
            }
        }
    }
}

async fn handle_game_message(
    server: &UdpServer,
    sessions: &mut SessionManager,
    rooms: &RoomManager,
    addr: std::net::SocketAddr,
    payload: Vec<u8>,
) {
    sessions.update_last_seen(&addr);

    let Some(session) = sessions.get_by_addr(&addr) else {
        tracing::warn!("GameMessage from unknown address: {}", addr);
        return;
    };

    let player_id = session.player_id;
    let Some(room_code) = &session.room_code else {
        tracing::warn!("GameMessage from player {} not in a room", player_id);
        return;
    };

    let Some(room) = rooms.get_room(room_code) else {
        tracing::warn!("Room {} not found", room_code);
        return;
    };

    // Only relay if game is in progress
    if room.state != RoomState::Playing {
        tracing::debug!("Ignoring GameMessage - room not playing");
        return;
    }

    // Wrap payload with sender info
    let relay_msg = ServerMessage {
        payload: Some(server_message::Payload::GameMessage(ServerGameMessage {
            from_player_id: player_id,
            payload,
        })),
    };
    let bytes = relay_msg.encode_to_vec();

    // Send to all OTHER players in room
    for pid in room.get_player_ids() {
        if pid != player_id {
            if let Some(other) = sessions.get_by_player_id(pid) {
                let _ = server.send(&bytes, other.addr).await;
            }
        }
    }

    tracing::trace!("Relayed message from player {} to room {}", player_id, room_code);
}

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}