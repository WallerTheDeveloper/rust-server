use prost::Message;
use rust_server::config::SERVER_ADDR;
use rust_server::game::traits::{Game, PlayerId};
use rust_server::games::paperio::{PaperioConfig, PaperioGame};
use rust_server::network::udp::UdpServer;
use rust_server::protocol::client::{client_message::Payload, ClientMessage, Ping};
use rust_server::protocol::server::{
    server_message, Error, GameMessage as ServerGameMessage, GameStarting, PlayerDisconnected,
    PlayerInfo, PlayerLeft, PlayerReconnected, Pong, RoomJoined, RoomUpdate, ServerMessage,
};
use rust_server::room::{RoomManager, RoomState};
use rust_server::session::{SequenceCheck, SessionManager};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

struct QueuedInput {
    player_id: PlayerId,
    payload: Vec<u8>,
}

struct GameRoom {
    game: PaperioGame,
    player_addrs: HashMap<PlayerId, SocketAddr>,
    input_queue: Vec<QueuedInput>,
}

impl GameRoom {
    fn new(config: PaperioConfig) -> Self {
        Self {
            game: PaperioGame::with_config(config),
            player_addrs: HashMap::new(),
            input_queue: Vec::new(),
        }
    }

    fn add_player(&mut self, player_id: PlayerId, addr: SocketAddr, name: String) -> Result<Vec<u8>, String> {
        self.player_addrs.insert(player_id, addr);
        self.game.player_joined(player_id, name)
            .map_err(|e| format!("{:?}", e))
    }

    fn remove_player(&mut self, player_id: PlayerId) {
        self.player_addrs.remove(&player_id);
        self.game.player_left(player_id);
    }

    fn reconnect_player(
        &mut self,
        player_id: PlayerId,
        addr: SocketAddr,
        name: String,
    ) -> Result<Vec<u8>, String> {
        self.game.player_left(player_id);
        self.player_addrs.insert(player_id, addr);
        self.game
            .player_joined(player_id, name)
            .map_err(|e| format!("{:?}", e))
    }

    fn has_player(&self, player_id: PlayerId) -> bool {
        self.player_addrs.contains_key(&player_id)
    }

    fn queue_input(&mut self, player_id: PlayerId, payload: Vec<u8>) {
        self.input_queue.push(QueuedInput { player_id, payload });
    }

    fn process_tick(&mut self) -> Vec<u8> {
        for input in self.input_queue.drain(..) {
            if let Err(e) = self.game.handle_input(input.player_id, &input.payload) {
                tracing::warn!("Failed to process input from {}: {}", input.player_id, e);
            }
        }

        let result = self.game.tick();

        for player_id in &result.eliminated {
            tracing::info!("Player {} was eliminated", player_id);
        }

        for player_id in &result.respawns {
            tracing::info!("Player {} respawned", player_id);
        }

        result.broadcast.unwrap_or_default()
    }

    fn get_player_addrs(&self) -> Vec<(PlayerId, SocketAddr)> {
        self.player_addrs.iter().map(|(&id, &addr)| (id, addr)).collect()
    }

    fn player_count(&self) -> usize {
        self.player_addrs.len()
    }
}

struct ServerState {
    sessions: SessionManager,
    rooms: RoomManager,
    game_rooms: HashMap<String, GameRoom>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            sessions: SessionManager::new(30),
            rooms: RoomManager::new(4),
            game_rooms: HashMap::new(),
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rust_server=debug".parse().unwrap())
                .add_directive("paperio_server=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Paper.io Authoritative Server");

    let server = Arc::new(UdpServer::bind(SERVER_ADDR).await?);
    tracing::info!("Server bound to {}", SERVER_ADDR);

    let state = Arc::new(Mutex::new(ServerState::new()));

    let tick_server = server.clone();
    let tick_state = state.clone();
    tokio::spawn(async move {
        run_tick_loop(tick_server, tick_state).await;
    });

    let cleanup_state = state.clone();
    let cleanup_server = server.clone();
    tokio::spawn(async move {
        run_cleanup_loop(cleanup_server, cleanup_state).await;
    });

    loop {
        let (data, addr) = match server.recv().await {
            Ok(result) => result,
            Err(e) => {
                tracing::trace!("Connection lost with client: {}", e);
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

        let mut state = state.lock().await;

        match state.sessions.check_sequence(&addr, msg.sequence) {
            SequenceCheck::Valid => {}
            SequenceCheck::Gap(gap) => {
                tracing::debug!("Packet gap of {} from {}", gap, addr);
            }
            SequenceCheck::Duplicate => {
                tracing::trace!("Duplicate packet from {}", addr);
                continue;
            }
            SequenceCheck::Invalid => {
                tracing::warn!("Invalid sequence from {}", addr);
            }
        }

        match msg.payload {
            Some(Payload::JoinRoom(join)) => {
                tracing::debug!("-----Received JoinRoom: {:#?}-----", join);
                handle_join_room(&server, &mut state, addr, join).await;
            }
            Some(Payload::LeaveRoom(_)) => {
                tracing::debug!("-----Received LeaveRoom-----");
                handle_leave_room(&server, &mut state, addr).await;
            }
            Some(Payload::Ready(_)) => {
                tracing::debug!("-----Received Ready-----");
                handle_ready(&server, &mut state, addr).await;
            }
            Some(Payload::GameMessage(game_msg)) => {
                tracing::debug!("-----Received GameMessage: {:#?}-----", game_msg);
                handle_game_message(&mut state, addr, game_msg.payload);
            }
            Some(Payload::Ping(ping)) => {
                handle_ping(&server, &mut state, addr, ping).await;
            }
            Some(Payload::Reconnect(reconnect)) => {
                tracing::debug!("-----Received Reconnect: {:#?}-----", reconnect);
                handle_reconnect(&server, &mut state, addr, reconnect).await;
            }
            None => {
                tracing::warn!("Empty message from {}", addr);
            }
        }
    }
}

async fn run_tick_loop(server: Arc<UdpServer>, state: Arc<Mutex<ServerState>>) {
    let tick_duration = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        let elapsed = last_tick.elapsed();
        if elapsed < tick_duration {
            tokio::time::sleep(tick_duration - elapsed).await;
        }
        last_tick = Instant::now();

        let mut state = state.lock().await;

        let room_codes: Vec<String> = state.game_rooms.keys().cloned().collect();

        for room_code in room_codes {
            if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
                if game_room.player_count() == 0 {
                    continue;
                }

                let state_bytes = game_room.process_tick();

                if state_bytes.is_empty() {
                    continue;
                }

                let players = game_room.get_player_addrs();
                for (player_id, addr) in players {
                    let msg = ServerMessage {
                        sequence: state.sessions.next_send_sequence(&addr),
                        payload: Some(server_message::Payload::GameMessage(ServerGameMessage {
                            from_player_id: 0,
                            payload: state_bytes.clone(),
                        })),
                    };

                    if let Err(e) = server.send(&msg.encode_to_vec(), addr).await {
                        tracing::warn!("Failed to send state to player {}: {}", player_id, e);
                    }
                }
            }
        }
    }
}

async fn run_cleanup_loop(server: Arc<UdpServer>, state: Arc<Mutex<ServerState>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        interval.tick().await;
        let mut state = state.lock().await;

        let disconnected = state.sessions.mark_timed_out_as_disconnected();
        let grace_period = state.sessions.grace_period_seconds();

        for player_id in disconnected {
            let notify_task = (|| {
                let session = state.sessions.get_by_player_id(player_id)?;
                let room_code = session.room_code.as_ref()?;
                let player_ids = state.rooms.get_room_player_ids(room_code);
                Some((room_code.clone(), player_ids))
            })();

            if let Some((room_code, player_ids)) = notify_task {
                for pid in player_ids.into_iter().filter(|&id| id != player_id) {
                    if let Some(other) = state.sessions.get_by_player_id(pid) {
                        let addr = other.addr;
                        let msg = ServerMessage {
                            sequence: state.sessions.next_send_sequence(&addr),
                            payload: Some(server_message::Payload::PlayerDisconnected(
                                PlayerDisconnected {
                                    player_id,
                                    grace_period_seconds: grace_period,
                                },
                            )),
                        };
                        let _ = server.send(&msg.encode_to_vec(), addr).await;
                    }
                }

                if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
                    game_room.player_addrs.remove(&player_id);
                }

                tracing::info!(
                    "Player {} disconnected from room {} (grace period: {}s)",
                    player_id,
                    room_code,
                    grace_period
                );
            }
        }

        // Handle fully expired sessions (grace period done — permanent removal)
        let expired = state.sessions.cleanup_expired_disconnected();
        for session in expired {
            let room_code = match &session.room_code {
                Some(code) => code.clone(),
                None => continue,
            };

            // NOW we fully clean up: remove from game state and room
            if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
                game_room.remove_player(session.player_id);
            }
            state.rooms.leave_room(session.player_id);

            // Notify remaining players
            for other_id in state.rooms.get_room_player_ids(&room_code) {
                if let Some(other) = state.sessions.get_by_player_id(other_id) {
                    let addr = other.addr;
                    let msg = ServerMessage {
                        sequence: state.sessions.next_send_sequence(&addr),
                        payload: Some(server_message::Payload::PlayerLeft(PlayerLeft {
                            player_id: session.player_id,
                        })),
                    };
                    let _ = server.send(&msg.encode_to_vec(), addr).await;
                }
            }

            // Clean up empty game rooms only after permanent removal
            if let Some(game_room) = state.game_rooms.get(&room_code) {
                if game_room.player_count() == 0 {
                    // Check if any session still references this room
                    let any_session_in_room = state.sessions.has_players_in_room(&room_code);
                    if !any_session_in_room {
                        state.game_rooms.remove(&room_code);
                        tracing::info!("Game room {} removed (no players left)", room_code);
                    }
                }
            }

            tracing::info!(
                "Player {} permanently removed (grace period expired)",
                session.player_id
            );
        }
    }
}

async fn handle_join_room(
    server: &UdpServer,
    state: &mut ServerState,
    addr: SocketAddr,
    join: rust_server::protocol::client::JoinRoom,
) {
    let session = state.sessions.register(addr, join.player_name.clone());
    let (player_id, reconnect_token) = (session.player_id, session.reconnect_token.clone());

    let room_data = match state.rooms.join_room(&join.room_code, player_id, join.player_name.clone()) {
        Ok(room) => {
            let players: Vec<PlayerInfo> = room.players.values()
                .map(|p| PlayerInfo {
                    player_id: p.player_id,
                    name: p.name.clone(),
                    ready: p.ready,
                }).collect();

            let room_code = room.code.clone();
            let other_pids: Vec<u32> = room.get_player_ids().into_iter()
                .filter(|&id| id != player_id).collect();

            Ok((room_code, players, other_pids))
        }
        Err(e) => Err(e),
    };

    match room_data {
        Ok((room_code, players, other_pids)) => {
            if let Some(s) = state.sessions.get_by_addr_mut(&addr) {
                s.room_code = Some(room_code.clone());
            }

            let join_msg = ServerMessage {
                sequence: state.sessions.next_send_sequence(&addr),
                payload: Some(server_message::Payload::RoomJoined(RoomJoined {
                    player_id,
                    room_code: room_code.clone(),
                    players: players.clone(),
                    reconnect_token,
                })),
            };
            let _ = server.send(&join_msg.encode_to_vec(), addr).await;

            for pid in other_pids {
                if let Some(other_addr) = state.sessions.get_by_player_id(pid).map(|s| s.addr) {
                    let update_msg = ServerMessage {
                        sequence: state.sessions.next_send_sequence(&other_addr),
                        payload: Some(server_message::Payload::RoomUpdate(RoomUpdate {
                            players: players.clone(),
                        })),
                    };
                    let _ = server.send(&update_msg.encode_to_vec(), other_addr).await;
                }
            }

            tracing::info!("Player {} joined room '{}'", player_id, room_code);
            if let Some(room) = state.rooms.get_room(&room_code) {
                if room.state == RoomState::Playing {
                    if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
                        match game_room.add_player(player_id, addr, join.player_name.clone()) {
                            Ok(join_response) => {
                                // Send GameStarting with no countdown
                                let starting = ServerMessage {
                                    sequence: state.sessions.next_send_sequence(&addr),
                                    payload: Some(server_message::Payload::GameStarting(GameStarting {
                                        countdown_seconds: 0,
                                    })),
                                };
                                let _ = server.send(&starting.encode_to_vec(), addr).await;

                                // Send initial game state
                                let game_msg = ServerMessage {
                                    sequence: state.sessions.next_send_sequence(&addr),
                                    payload: Some(server_message::Payload::GameMessage(
                                        ServerGameMessage {
                                            from_player_id: 0,
                                            payload: join_response,
                                        },
                                    )),
                                };
                                let _ = server.send(&game_msg.encode_to_vec(), addr).await;

                                tracing::info!("Late joiner {} added to active game in room {}", player_id, room_code);
                            }
                            Err(e) => {
                                tracing::error!("Failed to add late joiner {} to game: {}", player_id, e);
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            let error_msg = ServerMessage {
                sequence: state.sessions.next_send_sequence(&addr),
                payload: Some(server_message::Payload::Error(Error {
                    message: format!("Failed to join room: {:?}", e),
                })),
            };
            let _ = server.send(&error_msg.encode_to_vec(), addr).await;
        }
    }
}
async fn handle_leave_room(server: &UdpServer, state: &mut ServerState, addr: SocketAddr) {
    let player_id = match state.sessions.get_by_addr(&addr) {
        Some(s) => s.player_id,
        None => return,
    };

    let room_code = match state.sessions.get_by_addr(&addr).and_then(|s| s.room_code.clone()) {
        Some(code) => code,
        None => return,
    };

    let is_game_playing = state
        .rooms
        .get_room(&room_code)
        .map(|r| r.state == RoomState::Playing)
        .unwrap_or(false);

    if is_game_playing {
        // Game is active — DON'T destroy the room.
        // Just remove from address map (stop sending ticks) and mark disconnected.
        // The player can reconnect during grace period.
        if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
            game_room.player_addrs.remove(&player_id);
        }
        state.sessions.mark_disconnected(&addr);

        // Notify others that player disconnected (not permanently left)
        let grace_period = state.sessions.grace_period_seconds();
        let player_ids = state.rooms.get_room_player_ids(&room_code);
        for pid in player_ids.into_iter().filter(|&id| id != player_id) {
            if let Some(other_addr) = state.sessions.get_by_player_id(pid).map(|s| s.addr) {
                let msg = ServerMessage {
                    sequence: state.sessions.next_send_sequence(&other_addr),
                    payload: Some(server_message::Payload::PlayerDisconnected(
                        PlayerDisconnected {
                            player_id,
                            grace_period_seconds: grace_period,
                        },
                    )),
                };
                let _ = server.send(&msg.encode_to_vec(), other_addr).await;
            }
        }

        tracing::info!(
            "Player {} left during active game in room {} — treating as disconnect (grace: {}s)",
            player_id,
            room_code,
            grace_period
        );
    } else {
        // Game is NOT active (Waiting/Ended) — original behavior: full removal
        state.rooms.leave_room(player_id);
        if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
            game_room.remove_player(player_id);
        }
        state.sessions.mark_disconnected(&addr);

        let remaining_ids = state.rooms.get_room_player_ids(&room_code);
        for pid in remaining_ids {
            if let Some(other_addr) = state.sessions.get_by_player_id(pid).map(|s| s.addr) {
                let msg = ServerMessage {
                    sequence: state.sessions.next_send_sequence(&other_addr),
                    payload: Some(server_message::Payload::PlayerLeft(PlayerLeft { player_id })),
                };
                let _ = server.send(&msg.encode_to_vec(), other_addr).await;
            }
        }

        tracing::info!("Player {} left room {} (lobby state)", player_id, room_code);
    }
}

async fn handle_ready(server: &UdpServer, state: &mut ServerState, addr: SocketAddr) {
    state.sessions.update_last_seen(&addr);

    let player_id = match state.sessions.get_by_addr(&addr) {
        Some(s) => s.player_id,
        None => return,
    };

    let room_update = match state.rooms.set_ready(player_id, true) {
        Ok(room) => {
            let players_info: Vec<PlayerInfo> = room.players.values()
                .map(|p| PlayerInfo {
                    player_id: p.player_id,
                    name: p.name.clone(),
                    ready: p.ready,
                }).collect();

            let player_data: Vec<(u32, String)> = room.players.values()
                .map(|p| (p.player_id, p.name.clone())).collect();

            Some((
                room.code.clone(),
                room.all_ready() && room.player_count() >= 1,
                players_info,
                player_data
            ))
        }
        Err(_) => None,
    };

    let (room_code, should_start, players_info, player_data) = match room_update {
        Some(data) => data,
        None => return,
    };

    // Notify all players of updated ready status
    for (pid, _) in &player_data {
        if let Some(other_addr) = state.sessions.get_by_player_id(*pid).map(|s| s.addr) {
            let seq = state.sessions.next_send_sequence(&other_addr);
            let update = ServerMessage {
                sequence: seq,
                payload: Some(server_message::Payload::RoomUpdate(RoomUpdate {
                    players: players_info.clone(),
                })),
            };
            let _ = server.send(&update.encode_to_vec(), other_addr).await;
        }
    }

    let is_already_playing = state.rooms.get_room(&room_code)
            .map(|r| r.state == RoomState::Playing)
            .unwrap_or(false);

    if should_start && !is_already_playing {
        if let Some(room) = state.rooms.get_player_room_mut(player_id) {
            room.state = RoomState::Playing;
        }

        let game_room = state.game_rooms
            .entry(room_code.clone())
            .or_insert_with(|| GameRoom::new(PaperioConfig::default()));

        for (pid, name) in player_data {
            if let Some(p_addr) = state.sessions.get_by_player_id(pid).map(|s| s.addr) {
                match game_room.add_player(pid, p_addr, name) {
                    Ok(join_response) => {
                        let seq = state.sessions.next_send_sequence(&p_addr);
                        let msg = ServerMessage {
                            sequence: seq,
                            payload: Some(server_message::Payload::GameMessage(
                                ServerGameMessage { from_player_id: 0, payload: join_response },
                            )),
                        };
                        let _ = server.send(&msg.encode_to_vec(), p_addr).await;
                    }
                    Err(e) => tracing::error!("Failed to add player {} to game: {}", pid, e),
                }

                let start_seq = state.sessions.next_send_sequence(&p_addr);
                let starting = ServerMessage {
                    sequence: start_seq,
                    payload: Some(server_message::Payload::GameStarting(GameStarting {
                        countdown_seconds: 3,
                    })),
                };
                let _ = server.send(&starting.encode_to_vec(), p_addr).await;
            }
        }
        tracing::info!("Room {} starting game!", room_code);
    }
}

fn handle_game_message(state: &mut ServerState, addr: SocketAddr, payload: Vec<u8>) {
    state.sessions.update_last_seen(&addr);

    let session_data = state.sessions.get_by_addr(&addr).map(|s| {
        (s.player_id, s.room_code.clone())
    });

    let Some((player_id, Some(room_code))) = session_data else {
        tracing::warn!("GameMessage from unknown address or player not in room: {}", addr);
        return;
    };

    // Queue input for next tick
    if let Some(game_room) = state.game_rooms.get_mut(&room_code) {
        game_room.queue_input(player_id, payload);
        tracing::trace!("Queued input from player {} in room {}", player_id, room_code);
    }
}

async fn handle_ping(server: &UdpServer, state: &mut ServerState, addr: SocketAddr, ping: Ping) {
    state.sessions.ping(&addr);

    let pong = ServerMessage {
        sequence: state.sessions.next_send_sequence(&addr),
        payload: Some(server_message::Payload::Pong(Pong {
            timestamp: ping.timestamp,
            sequence: ping.sequence,
            server_time: current_timestamp_ms(),
        })),
    };

    let _ = server.send(&pong.encode_to_vec(), addr).await;
}

async fn handle_reconnect(
    server: &UdpServer,
    state: &mut ServerState,
    addr: SocketAddr,
    reconnect: rust_server::protocol::client::Reconnect,
) {
    let session_data = state
        .sessions
        .reconnected_by_token(&reconnect.token, addr, reconnect.player_name.clone())
        .map(|s| (s.player_id, s.reconnect_token.clone(), s.room_code.clone()));

    let (player_id, reconnect_token, room_code) = match session_data {
        Some(data) => data,
        None => {
            let seq = state.sessions.next_send_sequence(&addr);
            let response = ServerMessage {
                sequence: seq,
                payload: Some(server_message::Payload::Error(Error {
                    message: "Reconnection failed: invalid token or grace period expired"
                        .to_string(),
                })),
            };
            let _ = server.send(&response.encode_to_vec(), addr).await;
            tracing::warn!("Failed reconnection attempt from {}", addr);
            return;
        }
    };

    let Some(code) = room_code else {
        let seq = state.sessions.next_send_sequence(&addr);
        let response = ServerMessage {
            sequence: seq,
            payload: Some(server_message::Payload::RoomJoined(RoomJoined {
                player_id,
                room_code: String::new(),
                players: vec![],
                reconnect_token,
            })),
        };
        let _ = server.send(&response.encode_to_vec(), addr).await;
        tracing::info!("Player {} reconnected (no room)", player_id);
        return;
    };

    let reconnect_game_data = {
        let is_playing = state
            .rooms
            .get_room(&code)
            .map(|r| r.state == RoomState::Playing)
            .unwrap_or(false);

        if is_playing {
            let player_name = state
                .rooms
                .get_room(&code)
                .and_then(|r| r.players.get(&player_id))
                .map(|p| p.name.clone())
                .unwrap_or_else(|| reconnect.player_name.clone());

            if let Some(game_room) = state.game_rooms.get_mut(&code) {
                match game_room.reconnect_player(player_id, addr, player_name) {
                    Ok(join_response) => Some(join_response),
                    Err(e) => {
                        tracing::error!(
                            "Failed to reconnect player {} to game: {}",
                            player_id,
                            e
                        );
                        game_room.player_addrs.insert(player_id, addr);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            if let Some(game_room) = state.game_rooms.get_mut(&code) {
                game_room.player_addrs.insert(player_id, addr);
            }
            None
        }
    };

    let room_info = state.rooms.get_room(&code).map(|room| {
        let players = room
            .players
            .values()
            .map(|p| PlayerInfo {
                player_id: p.player_id,
                name: p.name.clone(),
                ready: p.ready,
            })
            .collect::<Vec<_>>();
        let other_ids = room
            .get_player_ids()
            .into_iter()
            .filter(|&pid| pid != player_id)
            .collect::<Vec<_>>();
        (players, other_ids)
    });

    let (players, other_ids) = match room_info {
        Some(info) => info,
        None => {
            let seq = state.sessions.next_send_sequence(&addr);
            let _ = server
                .send(
                    &ServerMessage {
                        sequence: seq,
                        payload: Some(server_message::Payload::Error(Error {
                            message: "Room no longer exists".to_string(),
                        })),
                    }
                        .encode_to_vec(),
                    addr,
                )
                .await;
            return;
        }
    };

    let response = ServerMessage {
        sequence: state.sessions.next_send_sequence(&addr),
        payload: Some(server_message::Payload::RoomJoined(RoomJoined {
            player_id,
            room_code: code.clone(),
            players,
            reconnect_token,
        })),
    };
    let _ = server.send(&response.encode_to_vec(), addr).await;

    // Notify other players
    for pid in other_ids {
        if let Some(other_addr) = state.sessions.get_by_player_id(pid).map(|s| s.addr) {
            let msg = ServerMessage {
                sequence: state.sessions.next_send_sequence(&other_addr),
                payload: Some(server_message::Payload::PlayerReconnected(PlayerReconnected {
                    player_id,
                })),
            };
            let _ = server.send(&msg.encode_to_vec(), other_addr).await;
        }
    }

    // If game is running, send GameStarting + JoinResponse
    if let Some(join_response) = reconnect_game_data {
        let starting = ServerMessage {
            sequence: state.sessions.next_send_sequence(&addr),
            payload: Some(server_message::Payload::GameStarting(GameStarting {
                countdown_seconds: 0,
            })),
        };
        let _ = server.send(&starting.encode_to_vec(), addr).await;

        let game_msg = ServerMessage {
            sequence: state.sessions.next_send_sequence(&addr),
            payload: Some(server_message::Payload::GameMessage(ServerGameMessage {
                from_player_id: 0,
                payload: join_response,
            })),
        };
        let _ = server.send(&game_msg.encode_to_vec(), addr).await;

        tracing::info!(
            "Player {} reconnected and re-entered active game in room {}",
            player_id,
            code
        );
    } else {
        tracing::info!("Player {} reconnected to room {}", player_id, code);
    }
}

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}