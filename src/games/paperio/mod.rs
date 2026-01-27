pub mod config;
pub mod state;
pub mod systems;

use std::time::Duration;

use prost::Message;
use crate::game::traits::{Game, GameError, PlayerId, TickResult};
use crate::protocol::paperio::PaperioInput;

pub use config::{PaperioConfig, get_player_color};
pub use state::{Direction, GameState, GridPos, Player, TerritoryGrid};

pub struct PaperioGame {
    /// Current game state
    state: GameState,
    /// Game configuration
    config: PaperioConfig,
    /// Current tick number
    tick: u32,
}

impl PaperioGame {
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

    pub fn state(&self) -> &GameState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    pub fn config(&self) -> &PaperioConfig {
        &self.config
    }
}

impl Default for PaperioGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for PaperioGame {
    fn tick(&mut self) -> TickResult {
        self.tick += 1;
        let mut result = TickResult::default();

        let ready_to_respawn = systems::update_timers(&mut self.state);

        for player_id in ready_to_respawn {
            if let Some(_pos) = systems::respawn_player(&mut self.state, player_id, &self.config) {
                result.respawns.push(player_id);
            }
        }

        let move_results = systems::update_movement(&mut self.state, &self.config);

        // Handle boundary collisions and territory claims
        for (player_id, move_result) in move_results {
            if move_result.hit_boundary {
                systems::eliminate_player(
                    &mut self.state,
                    player_id,
                    systems::EliminationReason::Boundary,
                    self.config.respawn_delay_ticks,
                );
                result.eliminated.push(player_id);
            } else if move_result.should_claim {
                let claim_result = systems::claim_territory(
                    &mut self.state.territory,
                    player_id,
                    &move_result.trail_to_claim,
                );

                tracing::debug!(
                    "Player {} claimed {} cells ({} stolen)",
                    player_id,
                    claim_result.cells_claimed,
                    claim_result.cells_stolen
                );
            }
        }

        let eliminations = systems::check_collisions(&self.state);

        for elimination in eliminations {
            if !result.eliminated.contains(&elimination.victim) {
                systems::eliminate_player(
                    &mut self.state,
                    elimination.victim,
                    elimination.reason,
                    self.config.respawn_delay_ticks,
                );
                result.eliminated.push(elimination.victim);

                tracing::info!(
                    "Player {} eliminated by player {} ({})",
                    elimination.victim,
                    elimination.killer,
                    elimination.reason
                );
            }
        }

        systems::update_scores(&mut self.state);

        result
    }

    fn handle_input(&mut self, player_id: PlayerId, input: &[u8]) -> Result<(), GameError> {
        let paperio_input = PaperioInput::decode(input)
            .map_err(|e| GameError::InvalidInput(format!("Failed to decode input: {}", e)))?;

        let direction = systems::direction_from_proto(paperio_input.direction);

        systems::set_player_direction(&mut self.state, player_id, direction)
            .map_err(|e| GameError::InvalidInput(e.to_string()))?;

        Ok(())
    }

    fn player_joined(&mut self, player_id: PlayerId, name: String) -> Result<Vec<u8>, GameError> {
        let spawn_pos = systems::find_spawn_position(&self.state, &self.config)
            .ok_or_else(|| GameError::InvalidState("No valid spawn position".to_string()))?;

        let color = get_player_color(player_id);

        let mut player = Player::new(player_id, name.clone(), spawn_pos, color);
        player.invulnerability_timer = self.config.invulnerability_ticks;

        self.state.players.insert(player_id, player);

        systems::grant_starting_territory(
            &mut self.state.territory,
            player_id,
            &spawn_pos,
            self.config.starting_territory_size,
        );

        systems::update_scores(&mut self.state);

        tracing::info!(
            "Player {} ({}) joined at {:?} with {} ticks invulnerability",
            player_id, name, spawn_pos, self.config.invulnerability_ticks
        );

        Ok(Vec::new())
    }

    fn player_left(&mut self, player_id: PlayerId) {
        if let Some(player) = self.state.players.remove(&player_id) {
            let owned_cells = self.state.territory.get_owned_cells(player_id);
            for pos in owned_cells {
                self.state.territory.set_cell_owner(&pos, None);
            }

            tracing::info!("Player {} ({}) left the game", player_id, player.name);
        }
    }

    fn encode_state(&self) -> Vec<u8> {
        // TODO: Phase 8 - Encode GameState to PaperioState protobuf
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
    use crate::game::traits::Game;

    #[test]
    fn test_game_creation() {
        let game = PaperioGame::new();
        assert_eq!(game.current_tick(), 0);
        assert_eq!(game.config.grid_width, 100);
        assert_eq!(game.config.grid_height, 100);
    }

    #[test]
    fn test_tick_increments() {
        let mut game = PaperioGame::new();
        assert_eq!(game.current_tick(), 0);
        game.tick();
        assert_eq!(game.current_tick(), 1);
        game.tick();
        assert_eq!(game.current_tick(), 2);
    }

    #[test]
    fn test_tick_rate() {
        let game = PaperioGame::new();
        assert_eq!(game.tick_rate(), Duration::from_millis(50));
    }

    #[test]
    fn test_player_join() {
        let mut game = PaperioGame::new();

        let result = game.player_joined(1, "Alice".to_string());
        assert!(result.is_ok());

        let player = game.state().get_player(1);
        assert!(player.is_some());

        let player = player.unwrap();
        assert_eq!(player.name, "Alice");
        assert!(player.alive);
        // New players should have invulnerability
        assert!(player.is_invulnerable());

        let owned = game.state().territory.count_owned_by(1);
        assert!(owned > 0);
    }

    #[test]
    fn test_player_leave() {
        let mut game = PaperioGame::new();

        game.player_joined(1, "Alice".to_string()).unwrap();
        assert!(game.state().get_player(1).is_some());

        game.player_left(1);
        assert!(game.state().get_player(1).is_none());

        let owned = game.state().territory.count_owned_by(1);
        assert_eq!(owned, 0);
    }

    #[test]
    fn test_handle_input_direction() {
        let mut game = PaperioGame::new();
        game.player_joined(1, "Alice".to_string()).unwrap();

        let input = PaperioInput { direction: 1 }; // UP
        let bytes = input.encode_to_vec();

        let result = game.handle_input(1, &bytes);
        assert!(result.is_ok());

        let player = game.state().get_player(1).unwrap();
        assert_eq!(player.direction, Direction::Up);
    }

    #[test]
    fn test_full_tick_with_movement() {
        let mut game = PaperioGame::new();
        game.player_joined(1, "Alice".to_string()).unwrap();

        let initial_pos = game.state().get_player(1).unwrap().position;

        let input = PaperioInput { direction: 4 }; // RIGHT
        game.handle_input(1, &input.encode_to_vec()).unwrap();

        game.tick();

        let new_pos = game.state().get_player(1).unwrap().position;
        assert_eq!(new_pos.x, initial_pos.x + 1);
        assert_eq!(new_pos.y, initial_pos.y);
    }

    #[test]
    fn test_multiple_players() {
        let mut game = PaperioGame::new();

        game.player_joined(1, "Alice".to_string()).unwrap();
        game.player_joined(2, "Bob".to_string()).unwrap();

        assert_eq!(game.state().players.len(), 2);
        assert!(game.state().get_player(1).is_some());
        assert!(game.state().get_player(2).is_some());
    }

    #[test]
    fn test_territory_claim_through_tick() {
        let config = PaperioConfig::with_grid_size(20, 20);
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();

        let initial_territory = game.state().territory.count_owned_by(1);

        game.handle_input(1, &PaperioInput { direction: 4 }.encode_to_vec()).unwrap();
        for _ in 0..3 {
            game.tick();
        }

        game.handle_input(1, &PaperioInput { direction: 2 }.encode_to_vec()).unwrap();
        for _ in 0..3 {
            game.tick();
        }

        game.handle_input(1, &PaperioInput { direction: 3 }.encode_to_vec()).unwrap();
        for _ in 0..5 {
            game.tick();
        }

        game.handle_input(1, &PaperioInput { direction: 1 }.encode_to_vec()).unwrap();
        for _ in 0..5 {
            game.tick();
        }

        let final_territory = game.state().territory.count_owned_by(1);

        assert!(final_territory >= initial_territory,
                "Territory should not decrease: {} -> {}", initial_territory, final_territory);
    }

    #[test]
    fn test_boundary_elimination_through_tick() {
        let config = PaperioConfig::with_grid_size(20, 20);
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();

        game.state_mut().players.get_mut(&1).unwrap().invulnerability_timer = 0;

        game.state_mut().players.get_mut(&1).unwrap().position = GridPos::new(0, 10);

        game.handle_input(1, &PaperioInput { direction: 3 }.encode_to_vec()).unwrap(); // LEFT

        let result = game.tick();

        assert!(result.eliminated.contains(&1));
        assert!(!game.state().get_player(1).unwrap().alive);
    }

    #[test]
    fn test_self_collision_through_tick() {
        let config = PaperioConfig::with_grid_size(30, 30);
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();

        game.state_mut().players.get_mut(&1).unwrap().invulnerability_timer = 0;

        let player = game.state_mut().players.get_mut(&1).unwrap();
        player.position = GridPos::new(20, 15);
        player.direction = Direction::Left;
        player.trail = vec![
            GridPos::new(21, 15),
            GridPos::new(20, 15),
            GridPos::new(19, 15),
        ];

        let result = game.tick();

        assert!(result.eliminated.contains(&1),
                "Player should be eliminated from self-collision");
        assert!(!game.state().get_player(1).unwrap().alive,
                "Player should not be alive after self-collision");
    }

    #[test]
    fn test_trail_cut_between_players() {
        let config = PaperioConfig::with_grid_size(50, 50);
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();
        game.player_joined(2, "Bob".to_string()).unwrap();

        game.state_mut().players.get_mut(&1).unwrap().invulnerability_timer = 0;
        game.state_mut().players.get_mut(&2).unwrap().invulnerability_timer = 0;

        game.state_mut().players.get_mut(&1).unwrap().position = GridPos::new(20, 25);
        game.state_mut().players.get_mut(&2).unwrap().position = GridPos::new(25, 20);

        game.state_mut().players.get_mut(&1).unwrap().trail = vec![
            GridPos::new(21, 25),
            GridPos::new(22, 25),
            GridPos::new(23, 25),
            GridPos::new(24, 25),
            GridPos::new(25, 25),
        ];

        game.state_mut().players.get_mut(&2).unwrap().position = GridPos::new(23, 25);

        let result = game.tick();

        assert!(result.eliminated.contains(&1),
                "Player 1 should be eliminated when their trail is cut");
    }

    #[test]
    fn test_respawn_after_elimination() {
        let mut config = PaperioConfig::with_grid_size(20, 20);
        config.respawn_delay_ticks = 3; // Short delay for test
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();

        game.state_mut().players.get_mut(&1).unwrap().invulnerability_timer = 0;

        game.state_mut().players.get_mut(&1).unwrap().position = GridPos::new(0, 10);
        game.handle_input(1, &PaperioInput { direction: 3 }.encode_to_vec()).unwrap();

        let result = game.tick();
        assert!(result.eliminated.contains(&1));
        assert!(!game.state().get_player(1).unwrap().alive);
        assert_eq!(game.state().get_player(1).unwrap().respawn_timer, 3);

        game.tick(); // timer = 2
        assert_eq!(game.state().get_player(1).unwrap().respawn_timer, 2);

        game.tick(); // timer = 1
        assert_eq!(game.state().get_player(1).unwrap().respawn_timer, 1);

        let result = game.tick(); // timer = 0, should respawn
        assert!(result.respawns.contains(&1));
        assert!(game.state().get_player(1).unwrap().alive);
        assert!(game.state().get_player(1).unwrap().is_invulnerable());
    }

    #[test]
    fn test_invulnerability_protects_from_trail_cut() {
        let config = PaperioConfig::with_grid_size(50, 50);
        let mut game = PaperioGame::with_config(config);

        game.player_joined(1, "Alice".to_string()).unwrap();
        game.player_joined(2, "Bob".to_string()).unwrap();

        assert!(game.state().get_player(1).unwrap().is_invulnerable());

        game.state_mut().players.get_mut(&2).unwrap().invulnerability_timer = 0;

        game.state_mut().players.get_mut(&1).unwrap().trail = vec![
            GridPos::new(25, 25),
            GridPos::new(26, 25),
        ];

        game.state_mut().players.get_mut(&2).unwrap().position = GridPos::new(25, 25);

        let result = game.tick();

        assert!(!result.eliminated.contains(&1));
        assert!(game.state().get_player(1).unwrap().alive);
    }
}
