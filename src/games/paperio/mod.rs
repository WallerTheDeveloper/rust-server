pub mod config;
pub mod state;
pub mod systems;

use std::time::Duration;

use crate::game::traits::{Game, GameConfig, GameError, PlayerId, TickResult};

// Re-export commonly used types
pub use config::PaperioConfig;
pub use state::{Direction, GameState, GridPos, Player, TerritoryGrid};

pub struct PaperGame {
    /// Current game state
    state: GameState,
    /// Game configuration
    config: PaperioConfig,
    /// Current tick number
    tick: u32,
}

impl PaperGame {
    pub fn new() -> Self {
        Self::with_config(PaperioConfig::default())
    }

    pub fn with_config(config: PaperioConfig) -> Self {
        Self {
            state: GameState::new(config.grid_width, config.grid_height),
            config,
            tick: 0,
        }
    }

    pub fn current_tick(&self) -> u32 {
        self.tick
    }

    pub fn get_state(&self) -> &GameState {
        &self.state
    }
}

impl Default for PaperGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for PaperGame {
    fn tick(&mut self) -> TickResult {
        self.tick += 1;

        // TODO: Phase 3 - Implement movement system
        // TODO: Phase 4 - Implement territory claiming
        // TODO: Phase 5 - Implement collision detection

        TickResult::default()
    }

    fn handle_input(&mut self, player_id: PlayerId, input: &[u8]) -> Result<(), GameError> {
        // TODO: Phase 3 - Parse PaperioInput and update player direction
        let _ = (player_id, input);
        Ok(())
    }

    fn player_joined(&mut self, player_id: PlayerId, name: String) -> Result<Vec<u8>, GameError> {
        // TODO: Phase 2 - Create player with spawn position and starting territory
        let _ = (player_id, name);
        Ok(Vec::new())
    }

    fn player_left(&mut self, player_id: PlayerId) {
        // TODO: Phase 2 - Remove player from game state
        let _ = player_id;
    }

    fn encode_state(&self) -> Vec<u8> {
        // TODO: Phase 2 - Encode GameState to PaperioState protobuf
        Vec::new()
    }

    fn encode_state_for_player(&self, player_id: PlayerId) -> Vec<u8> {
        // TODO: Phase 8 - Implement player-specific state encoding
        let _ = player_id;
        self.encode_state()
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(1000 / self.config.tick_rate_hz as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_creation() {
        let game = PaperGame::new();
        assert_eq!(game.current_tick(), 0);
        assert_eq!(game.config.grid_width, 100);
        assert_eq!(game.config.grid_height, 100);
    }

    #[test]
    fn test_tick_increments() {
        let mut game = PaperGame::new();
        assert_eq!(game.current_tick(), 0);
        game.tick();
        assert_eq!(game.current_tick(), 1);
        game.tick();
        assert_eq!(game.current_tick(), 2);
    }

    #[test]
    fn test_tick_rate() {
        let game = PaperGame::new();
        // Default is 20 Hz = 50ms per tick
        assert_eq!(game.tick_rate(), Duration::from_millis(50));
    }
}
