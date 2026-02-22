pub mod config;
pub mod encoding;
pub mod state;
pub mod systems;

use std::time::Duration;

use prost::Message;
use crate::game::traits::{Game, GameError, PlayerId, TickResult};
use crate::protocol::paperio::PaperioInput;

pub use config::{PaperioConfig, get_player_color};
pub use state::{Direction, GameState, GridPos, Player, TerritoryGrid};
use encoding::TerritorySnapshot;

pub struct PaperioGame {
    /// Current game state
    state: GameState,
    /// Game configuration
    config: PaperioConfig,
    /// Current tick number
    tick: u32,
    /// Snapshot of territory at the last keyframe.
    /// Used to compute diffs for delta updates.
    last_keyframe_snapshot: Option<TerritorySnapshot>,
    /// Tick number of the last keyframe sent.
    last_keyframe_tick: u32,
    force_keyframe: bool,
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
            last_keyframe_snapshot: None,
            last_keyframe_tick: 0,
            force_keyframe: false,
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

    fn is_keyframe_tick(&self) -> bool {
        self.force_keyframe || self.tick == 1 || self.tick % self.config.keyframe_interval == 0
    }

    pub fn force_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    pub fn force_next_keyframe(&mut self) {
        self.last_keyframe_snapshot = None;
    }
    
    fn encode_tick_state(&mut self) -> Vec<u8> {
        self.force_keyframe = false;
        if self.is_keyframe_tick() || self.last_keyframe_snapshot.is_none() {
            self.last_keyframe_snapshot = Some(TerritorySnapshot::capture(&self.state.territory));
            self.last_keyframe_tick = self.tick;

            tracing::trace!(
                "Tick {}: sending KEYFRAME (full state)",
                self.tick
            );

            encoding::encode_full_state(&self.state, self.tick, &self.config)
        } else {
            let snapshot = self.last_keyframe_snapshot.as_ref().unwrap();
            let territory_changes = snapshot.diff(&self.state.territory);

            tracing::trace!(
                "Tick {}: sending DELTA ({} territory changes, keyframe={})",
                self.tick,
                territory_changes.len(),
                self.last_keyframe_tick
            );

            self.last_keyframe_snapshot = Some(TerritorySnapshot::capture(&self.state.territory));

            encoding::encode_delta_state(
                &self.state,
                self.tick,
                &self.config,
                territory_changes,
                self.last_keyframe_tick,
            )
        }
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
                    "Player {} claimed {} cells (stole {} from {:?})",
                    player_id,
                    claim_result.cells_claimed,
                    claim_result.cells_stolen,
                    claim_result.victims
                );

                if let Some(player) = self.state.players.get_mut(&player_id) {
                    player.trail.clear();
                }
            }
        }

        let eliminations = systems::check_collisions(&self.state);
        for elimination in eliminations {
            systems::eliminate_player(
                &mut self.state,
                elimination.victim,
                elimination.reason,
                self.config.respawn_delay_ticks,
            );
            result.eliminated.push(elimination.victim);
        }

        systems::update_scores(&mut self.state);

        result.broadcast = Some(self.encode_tick_state());

        result
    }

    fn handle_input(&mut self, player_id: PlayerId, input: &[u8]) -> Result<(), GameError> {
        let paperio_input = PaperioInput::decode(input)
            .map_err(|e| GameError::InvalidInput(format!("Failed to decode input: {}", e)))?;

        let direction = systems::direction_from_proto(paperio_input.direction);

        systems::set_player_direction(&mut self.state, player_id, direction)
            .map_err(|e| GameError::InvalidInput(e.parse().unwrap()))
    }

    fn player_joined(&mut self, player_id: PlayerId, name: String) -> Result<Vec<u8>, GameError> {
        if self.state.players.contains_key(&player_id) {
            return Err(GameError::InvalidState(format!(
                "Player {} already exists",
                player_id
            )));
        }

        let spawn_pos = systems::find_spawn_position(&self.state, &self.config)
            .ok_or_else(|| GameError::InvalidState("No valid spawn position found".to_string()))?;

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

        tracing::info!(
            "Player {} ({}) joined at {:?}",
            player_id,
            name,
            spawn_pos
        );

        Ok(encoding::encode_join_response(
            player_id,
            &self.state,
            self.tick,
            &self.config,
        ))
    }

    fn player_left(&mut self, player_id: PlayerId) {
        if let Some(player) = self.state.players.remove(&player_id) {
            let owned = self.state.territory.get_owned_cells(player_id);
            for pos in owned {
                self.state.territory.set_cell_owner(&pos, None);
            }
            tracing::info!("Player {} ({}) left the game", player_id, player.name);
        }
    }

    fn encode_state(&self) -> Vec<u8> {
        encoding::encode_state(&self.state, self.tick, &self.config)
    }

    fn encode_state_for_player(&self, player_id: PlayerId) -> Vec<u8> {
        encoding::encode_state_for_player(&self.state, self.tick, &self.config, player_id)
    }

    fn tick_rate(&self) -> Duration {
        self.config.tick_duration()
    }
}