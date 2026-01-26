use std::time::Duration;

pub type PlayerId = u32;

pub trait TickRate {
    fn tick_duration(&self) -> Duration;

    fn ticks_per_second(&self) -> u32 {
        (1.0 / self.tick_duration().as_secs_f64()) as u32
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GameError {
    /// Player not found in game state
    PlayerNotFound(PlayerId),
    /// Invalid input received
    InvalidInput(String),
    /// Game is not in a valid state for the operation
    InvalidState(String),
    /// Encoding/decoding error
    EncodingError(String),
    /// Generic game error
    Other(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameError::PlayerNotFound(id) => write!(f, "Player {} not found", id),
            GameError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            GameError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            GameError::EncodingError(msg) => write!(f, "Encoding error: {}", msg),
            GameError::Other(msg) => write!(f, "Game error: {}", msg),
        }
    }
}

impl std::error::Error for GameError {}

pub struct TickResult {
    pub broadcast: Option<Vec<u8>>,
    pub player_updates: Vec<(PlayerId, Vec<u8>)>,
    pub eliminated: Vec<PlayerId>,
    pub respawns: Vec<PlayerId>,
}

impl Default for TickResult {
    fn default() -> Self {
        Self {
            broadcast: None,
            player_updates: Vec::new(),
            eliminated: Vec::new(),
            respawns: Vec::new(),
        }
    }
}

pub trait Game: Send + Sync {
    fn tick(&mut self) -> TickResult;
    fn handle_input(&mut self, player_id: PlayerId, input: &[u8]) -> Result<(), GameError>;
    fn player_joined(&mut self, player_id: PlayerId, name: String) -> Result<Vec<u8>, GameError>;
    fn player_left(&mut self, player_id: PlayerId);
    fn encode_state(&self) -> Vec<u8>;
    fn encode_state_for_player(&self, player_id: PlayerId) -> Vec<u8> {
        let _ = player_id; // Silence unused warning in default impl
        self.encode_state()
    }
    fn tick_rate(&self) -> Duration;
    fn is_game_over(&self) -> bool {
        false
    }
    fn get_winners(&self) -> Vec<PlayerId> {
        Vec::new()
    }
}

pub struct GameConfig {
    pub grid_width: u32,
    pub grid_height: u32,
    pub tick_rate_hz: u32,
    pub max_players: usize,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            grid_width: 100,
            grid_height: 100,
            tick_rate_hz: 20,
            max_players: 16,
        }
    }
}
