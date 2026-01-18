use std::collections::HashMap;
use crate::session::PlayerId;

/// Possible states for a room
#[derive(Debug, Clone, PartialEq)]
pub enum RoomState {
    Waiting,
    Playing,
    Ended,
}

/// A player in a room
#[derive(Debug, Clone)]
pub struct RoomPlayer {
    pub player_id: PlayerId,
    pub name: String,
    pub ready: bool,
}

/// A game room
#[derive(Debug)]
pub struct Room {
    pub code: String,
    pub players: HashMap<PlayerId, RoomPlayer>,
    pub state: RoomState,
    pub max_players: usize,
}

impl Room {
    pub fn new(code: String, max_players: usize) -> Self {
        Self {
            code,
            players: HashMap::new(),
            state: RoomState::Waiting,
            max_players,
        }
    }

    pub fn add_player(&mut self, player_id: PlayerId, name: String) -> Result<(), RoomError> {
        if self.state != RoomState::Waiting {
            return Err(RoomError::GameInProgress);
        }

        if self.players.len() >= self.max_players {
            return Err(RoomError::RoomFull);
        }

        if self.players.contains_key(&player_id) {
            return Err(RoomError::AlreadyInRoom);
        }

        self.players.insert(player_id, RoomPlayer {
            player_id,
            name,
            ready: false,
        });

        Ok(())
    }

    pub fn remove_player(&mut self, player_id: PlayerId) -> Option<RoomPlayer> {
        self.players.remove(&player_id)
    }

    pub fn set_ready(&mut self, player_id: PlayerId, ready: bool) -> Result<(), RoomError> {
        let player = self.players.get_mut(&player_id).ok_or(RoomError::NotInRoom)?;
        player.ready = ready;
        Ok(())
    }

    pub fn all_ready(&self) -> bool {
        !self.players.is_empty() && self.players.values().all(|p| p.ready)
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn get_player_ids(&self) -> Vec<PlayerId> {
        self.players.keys().copied().collect()
    }
}

/// Room-related errors
#[derive(Debug, Clone, PartialEq)]
pub enum RoomError {
    RoomFull,
    GameInProgress,
    AlreadyInRoom,
    NotInRoom,
    RoomNotFound,
}

/// Manages all rooms
pub struct RoomManager {
    rooms: HashMap<String, Room>,
    player_room: HashMap<PlayerId, String>,
    max_players_per_room: usize,
}

impl RoomManager {
    pub fn new(max_players_per_room: usize) -> Self {
        Self {
            rooms: HashMap::new(),
            player_room: HashMap::new(),
            max_players_per_room,
        }
    }

    /// Create a new room with a random code
    pub fn create_room(&mut self) -> &Room {
        let code = self.generate_room_code();
        let room = Room::new(code.clone(), self.max_players_per_room);
        self.rooms.insert(code.clone(), room);
        tracing::info!("Room created: {}", code);
        self.rooms.get(&code).unwrap()
    }

    /// Join an existing room or create new one if code is empty
    pub fn join_room(
        &mut self,
        room_code: &str,
        player_id: PlayerId,
        player_name: String,
    ) -> Result<&Room, RoomError> {
        if let Some(old_code) = self.player_room.get(&player_id).cloned() {
            self.leave_room(player_id);
            tracing::debug!("Player {} left room {} to join {}", player_id, old_code, room_code);
        }

        let code = if room_code.is_empty() || !self.rooms.contains_key(room_code) {
            if room_code.is_empty() {
                let room = self.create_room();
                room.code.clone()
            } else {
                let room = Room::new(room_code.to_string(), self.max_players_per_room);
                self.rooms.insert(room_code.to_string(), room);
                tracing::info!("Room created: {}", room_code);
                room_code.to_string()
            }
        } else {
            room_code.to_string()
        };

        let room = self.rooms.get_mut(&code).unwrap();
        room.add_player(player_id, player_name)?;
        self.player_room.insert(player_id, code.clone());

        tracing::info!("Player {} joined room {}", player_id, code);
        Ok(self.rooms.get(&code).unwrap())
    }

    /// Remove player from their current room
    pub fn leave_room(&mut self, player_id: PlayerId) -> Option<String> {
        let room_code = self.player_room.remove(&player_id)?;

        if let Some(room) = self.rooms.get_mut(&room_code) {
            room.remove_player(player_id);
            tracing::info!("Player {} left room {}", player_id, room_code);

            // Clean up empty rooms
            if room.is_empty() {
                self.rooms.remove(&room_code);
                tracing::info!("Room {} removed (empty)", room_code);
            }
        }

        Some(room_code)
    }

    /// Set player ready status
    pub fn set_ready(&mut self, player_id: PlayerId, ready: bool) -> Result<&Room, RoomError> {
        let room_code = self.player_room.get(&player_id).ok_or(RoomError::NotInRoom)?;
        let room = self.rooms.get_mut(room_code).ok_or(RoomError::RoomNotFound)?;
        room.set_ready(player_id, ready)?;
        Ok(self.rooms.get(room_code).unwrap())
    }

    /// Get room by code
    pub fn get_room(&self, code: &str) -> Option<&Room> {
        self.rooms.get(code)
    }

    /// Get room by player ID
    pub fn get_player_room(&self, player_id: PlayerId) -> Option<&Room> {
        let code = self.player_room.get(&player_id)?;
        self.rooms.get(code)
    }

    /// Get mutable room by player ID
    pub fn get_player_room_mut(&mut self, player_id: PlayerId) -> Option<&mut Room> {
        let code = self.player_room.get(&player_id)?.clone();
        self.rooms.get_mut(&code)
    }

    /// Generate a random 4-character room code
    fn generate_room_code(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".chars().collect();
        let mut code = String::with_capacity(4);

        // Simple random based on time (not cryptographically secure, but fine for room codes)
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let mut n = seed;
        for _ in 0..4 {
            code.push(chars[(n % chars.len() as u128) as usize]);
            n /= chars.len() as u128;
        }

        // If code already exists, try again
        if self.rooms.contains_key(&code) {
            return self.generate_room_code();
        }

        code
    }

    /// Get all players in a room (for broadcasting)
    pub fn get_room_player_ids(&self, room_code: &str) -> Vec<PlayerId> {
        self.rooms
            .get(room_code)
            .map(|r| r.get_player_ids())
            .unwrap_or_default()
    }
}