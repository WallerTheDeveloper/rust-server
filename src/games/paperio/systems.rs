use super::state::{Direction, GameState, GridPos, Player, TerritoryGrid};
use super::config::PaperioConfig;
use crate::game::traits::PlayerId;

#[derive(Debug, Default)]
pub struct MoveResult {
    /// Player completed a trail and should claim territory
    pub should_claim: bool,
    /// Player hit boundary
    pub hit_boundary: bool,
    /// The trail to claim (if should_claim is true)
    pub trail_to_claim: Vec<GridPos>,
}

pub fn update_movement(state: &mut GameState, config: &PaperioConfig) -> Vec<(PlayerId, MoveResult)> {
    // Get all alive player IDs first to avoid borrow issues
    let player_ids: Vec<PlayerId> = state.get_alive_players()
        .filter(|p| p.direction != Direction::None)
        .map(|p| p.id)
        .collect();

    let mut results = Vec::new();

    for player_id in player_ids {
        let result = move_player(state, player_id, config);
        if result.should_claim || result.hit_boundary {
            results.push((player_id, result));
        }
    }

    results
}

fn move_player(state: &mut GameState, player_id: PlayerId, config: &PaperioConfig) -> MoveResult {
    let mut result = MoveResult::default();

    let (current_pos, direction, was_in_territory) = {
        let player = match state.players.get(&player_id) {
            Some(p) if p.alive => p,
            _ => return result,
        };
        let in_territory = is_in_own_territory(&state.territory, player_id, &player.position);
        (player.position, player.direction, in_territory)
    };

    let new_pos = current_pos.moved(direction);

    if !state.territory.in_bounds(&new_pos) {
        result.hit_boundary = true;
        return result;
    }

    let now_in_territory = is_in_own_territory(&state.territory, player_id, &new_pos);

    let player = state.players.get_mut(&player_id).unwrap();

    if !now_in_territory {
        add_to_trail(player, current_pos);
    }

    if now_in_territory && player.has_trail() {
        add_to_trail(player, new_pos);

        result.trail_to_claim = player.trail.clone();
        result.should_claim = true;
        clear_trail(player);
    }

    player.position = new_pos;

    result
}

pub fn is_in_own_territory(territory: &TerritoryGrid, player_id: PlayerId, pos: &GridPos) -> bool {
    territory.is_owned_by(pos, player_id)
}

pub fn add_to_trail(player: &mut Player, pos: GridPos) {
    if player.trail.last() != Some(&pos) {
        player.trail.push(pos);
    }
}

pub fn clear_trail(player: &mut Player) {
    player.trail.clear();
}

pub fn set_player_direction(
    state: &mut GameState,
    player_id: PlayerId,
    new_direction: Direction,
) -> Result<(), &'static str> {
    let player = state.players.get_mut(&player_id)
        .ok_or("Player not found")?;

    if !player.alive {
        return Err("Player is dead");
    }

    if player.direction != Direction::None && player.direction.is_opposite(&new_direction) {
        return Err("Cannot reverse direction");
    }

    player.direction = new_direction;
    Ok(())
}

pub fn direction_from_proto(value: i32) -> Direction {
    match value {
        1 => Direction::Up,
        2 => Direction::Down,
        3 => Direction::Left,
        4 => Direction::Right,
        _ => Direction::None,
    }
}

pub fn direction_to_proto(direction: Direction) -> i32 {
    match direction {
        Direction::None => 0,
        Direction::Up => 1,
        Direction::Down => 2,
        Direction::Left => 3,
        Direction::Right => 4,
    }
}

pub fn claim_territory(
    _territory: &mut TerritoryGrid,
    _player_id: PlayerId,
    _trail: &[GridPos],
) -> ClaimResult {
    // TODO: Phase 4 implementation
    ClaimResult::default()
}

#[derive(Debug, Default)]
pub struct ClaimResult {
    /// Number of new cells claimed
    pub cells_claimed: usize,
    /// Number of cells stolen from other players
    pub cells_stolen: usize,
    /// Players who lost territory
    pub victims: Vec<PlayerId>,
}

fn flood_fill_from_edges(
    _territory: &TerritoryGrid,
    _player_id: PlayerId,
    _trail: &[GridPos],
) -> Vec<GridPos> {
    // TODO: Phase 4 implementation
    // Returns all cells that should become player's territory
    Vec::new()
}

/// Check for all collisions and return eliminated players
pub fn check_collisions(_state: &GameState) -> Vec<Elimination> {
    // TODO: Phase 5 implementation
    Vec::new()
}

#[derive(Debug)]
pub struct Elimination {
    /// Player who was eliminated
    pub victim: PlayerId,
    /// Player who caused the elimination (0 for self/boundary)
    pub killer: PlayerId,
    /// Reason for elimination
    pub reason: EliminationReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EliminationReason {
    /// Trail was crossed by another player
    TrailCut,
    /// Player crossed their own trail
    SelfCollision,
    /// Head-on collision with another player
    HeadCollision,
    /// Hit map boundary
    Boundary,
}

pub fn eliminate_player(
    state: &mut GameState,
    player_id: PlayerId,
    _reason: EliminationReason,
    respawn_delay: u32,
) {
    if let Some(player) = state.players.get_mut(&player_id) {
        player.alive = false;
        player.trail.clear();
        player.direction = Direction::None;
        player.respawn_timer = respawn_delay;

        tracing::info!(
            "Player {} eliminated, respawning in {} ticks",
            player_id,
            respawn_delay
        );
    }
}

pub fn find_spawn_position(state: &GameState, config: &PaperioConfig) -> Option<GridPos> {
    // TODO: Phase 5 implementation
    let (width, height) = state.territory.get_grid_dimensions();
    let center = GridPos::new(width as i32 / 2, height as i32 / 2);

    let _ = config;
    Some(center)
}

pub fn grant_starting_territory(
    territory: &mut TerritoryGrid,
    player_id: PlayerId,
    center: &GridPos,
    size: u32,
) {
    let half = (size / 2) as i32;
    for dy in -half..=half {
        for dx in -half..=half {
            let pos = center.offset(dx, dy);
            if territory.in_bounds(&pos) {
                territory.set_cell_owner(&pos, Some(player_id));
            }
        }
    }
}

pub fn respawn_player(
    state: &mut GameState,
    player_id: PlayerId,
    config: &PaperioConfig,
) -> Option<GridPos> {
    let spawn_pos = find_spawn_position(state, config)?;

    if let Some(player) = state.players.get_mut(&player_id) {
        player.position = spawn_pos;
        player.alive = true;
        player.direction = Direction::None;
        player.trail.clear();
        player.invulnerability_timer = config.invulnerability_ticks;
    }

    grant_starting_territory(
        &mut state.territory,
        player_id,
        &spawn_pos,
        config.starting_territory_size,
    );

    Some(spawn_pos)
}

pub fn update_timers(state: &mut GameState) -> Vec<PlayerId> {
    let mut ready_to_respawn = Vec::new();

    for player in state.players.values_mut() {
        if player.invulnerability_timer > 0 {
            player.invulnerability_timer -= 1;
        }

        if !player.alive && player.respawn_timer > 0 {
            player.respawn_timer -= 1;
            if player.respawn_timer == 0 {
                ready_to_respawn.push(player.id);
            }
        }
    }

    ready_to_respawn
}

pub fn update_scores(state: &mut GameState) {
    let player_ids: Vec<PlayerId> = state.players.keys().copied().collect();

    for player_id in player_ids {
        let percentage = state.territory.get_ownership_percentage(player_id);
        // Store as integer with 2 decimal precision (e.g., 12.34% -> 1234)
        let score = (percentage * 100.0) as u32;

        if let Some(player) = state.players.get_mut(&player_id) {
            player.score = score;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_game_state() -> GameState {
        GameState::new(20, 20)
    }

    fn setup_player_with_territory(state: &mut GameState, player_id: PlayerId) {
        let spawn = GridPos::new(10, 10);
        let player = Player::new(player_id, "Test".to_string(), spawn, 0xFFFFFFFF);
        state.players.insert(player_id, player);
        grant_starting_territory(&mut state.territory, player_id, &spawn, 3);
    }

    #[test]
    fn test_is_in_own_territory() {
        let mut territory = TerritoryGrid::new(10, 10);
        let pos = GridPos::new(5, 5);

        assert!(!is_in_own_territory(&territory, 1, &pos));
        territory.set_cell_owner(&pos, Some(1));
        assert!(is_in_own_territory(&territory, 1, &pos));
        assert!(!is_in_own_territory(&territory, 2, &pos));
    }

    #[test]
    fn test_grant_starting_territory() {
        let mut territory = TerritoryGrid::new(10, 10);
        let center = GridPos::new(5, 5);

        grant_starting_territory(&mut territory, 1, &center, 3);

        assert!(territory.is_owned_by(&GridPos::new(4, 4), 1));
        assert!(territory.is_owned_by(&GridPos::new(5, 5), 1));
        assert!(territory.is_owned_by(&GridPos::new(6, 6), 1));
        assert!(!territory.is_owned_by(&GridPos::new(3, 3), 1));
    }

    #[test]
    fn test_trail_operations() {
        let mut player = Player::new(1, "Test".to_string(), GridPos::new(0, 0), 0xFFFFFFFF);

        assert!(!player.has_trail());

        add_to_trail(&mut player, GridPos::new(1, 0));
        add_to_trail(&mut player, GridPos::new(2, 0));

        assert!(player.has_trail());
        assert_eq!(player.trail.len(), 2);

        clear_trail(&mut player);
        assert!(!player.has_trail());
    }

    #[test]
    fn test_update_scores() {
        let mut state = GameState::new(10, 10); // 100 cells

        // Add a player
        state.players.insert(1, Player::new(
            1,
            "Test".to_string(),
            GridPos::new(5, 5),
            0xFFFFFFFF
        ));

        for x in 0..10 {
            state.territory.set_cell_owner(&GridPos::new(x, 0), Some(1));
        }

        update_scores(&mut state);

        let player = state.get_player(1).unwrap();
        assert_eq!(player.score, 1000); // 10.00% * 100 = 1000
    }


    #[test]
    fn test_player_moves_in_direction() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        set_player_direction(&mut state, 1, Direction::Right).unwrap();

        let initial_pos = state.get_player(1).unwrap().position;

        let config = PaperioConfig::default();
        update_movement(&mut state, &config);

        let new_pos = state.get_player(1).unwrap().position;
        assert_eq!(new_pos, initial_pos.moved(Direction::Right));
    }

    #[test]
    fn test_player_stationary_when_direction_none() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        let initial_pos = state.get_player(1).unwrap().position;

        let config = PaperioConfig::default();
        update_movement(&mut state, &config);

        let new_pos = state.get_player(1).unwrap().position;
        assert_eq!(new_pos, initial_pos);
    }

    #[test]
    fn test_trail_created_outside_territory() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        state.players.get_mut(&1).unwrap().position = GridPos::new(11, 10); // Right edge of 3x3

        set_player_direction(&mut state, 1, Direction::Right).unwrap();

        let config = PaperioConfig::default();

        update_movement(&mut state, &config);

        let player = state.get_player(1).unwrap();
        assert_eq!(player.position, GridPos::new(12, 10));
        assert!(player.has_trail());
    }

    #[test]
    fn test_trail_completes_on_return() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);
        let config = PaperioConfig::default();

        state.players.get_mut(&1).unwrap().position = GridPos::new(11, 10);
        set_player_direction(&mut state, 1, Direction::Right).unwrap();

        update_movement(&mut state, &config);

        let player = state.get_player(1).unwrap();
        assert!(player.has_trail());

        update_movement(&mut state, &config);

        set_player_direction(&mut state, 1, Direction::Down).unwrap();
        update_movement(&mut state, &config);

        set_player_direction(&mut state, 1, Direction::Left).unwrap();
        update_movement(&mut state, &config);
        update_movement(&mut state, &config);

        let results = update_movement(&mut state, &config);

        assert!(!state.get_player(1).unwrap().has_trail());
    }

    #[test]
    fn test_cannot_reverse_direction() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        set_player_direction(&mut state, 1, Direction::Right).unwrap();

        let result = set_player_direction(&mut state, 1, Direction::Left);
        assert!(result.is_err());

        assert_eq!(state.get_player(1).unwrap().direction, Direction::Right);
    }

    #[test]
    fn test_can_turn_90_degrees() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        set_player_direction(&mut state, 1, Direction::Right).unwrap();

        set_player_direction(&mut state, 1, Direction::Up).unwrap();
        assert_eq!(state.get_player(1).unwrap().direction, Direction::Up);

        set_player_direction(&mut state, 1, Direction::Left).unwrap();
        assert_eq!(state.get_player(1).unwrap().direction, Direction::Left);
    }

    #[test]
    fn test_boundary_collision() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);
        let config = PaperioConfig::default();

        state.players.get_mut(&1).unwrap().position = GridPos::new(10, 0);

        set_player_direction(&mut state, 1, Direction::Up).unwrap();

        let results = update_movement(&mut state, &config);

        assert_eq!(results.len(), 1);
        assert!(results[0].1.hit_boundary);
    }

    #[test]
    fn test_direction_from_proto() {
        assert_eq!(direction_from_proto(0), Direction::None);
        assert_eq!(direction_from_proto(1), Direction::Up);
        assert_eq!(direction_from_proto(2), Direction::Down);
        assert_eq!(direction_from_proto(3), Direction::Left);
        assert_eq!(direction_from_proto(4), Direction::Right);
        assert_eq!(direction_from_proto(99), Direction::None); // Invalid defaults to None
    }

    #[test]
    fn test_direction_to_proto() {
        assert_eq!(direction_to_proto(Direction::None), 0);
        assert_eq!(direction_to_proto(Direction::Up), 1);
        assert_eq!(direction_to_proto(Direction::Down), 2);
        assert_eq!(direction_to_proto(Direction::Left), 3);
        assert_eq!(direction_to_proto(Direction::Right), 4);
    }

    #[test]
    fn test_timer_decrement() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        state.players.get_mut(&1).unwrap().invulnerability_timer = 5;

        update_timers(&mut state);
        assert_eq!(state.get_player(1).unwrap().invulnerability_timer, 4);

        update_timers(&mut state);
        assert_eq!(state.get_player(1).unwrap().invulnerability_timer, 3);
    }

    #[test]
    fn test_respawn_timer() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        eliminate_player(&mut state, 1, EliminationReason::Boundary, 3);

        assert!(!state.get_player(1).unwrap().alive);
        assert_eq!(state.get_player(1).unwrap().respawn_timer, 3);

        let ready = update_timers(&mut state);
        assert!(ready.is_empty());
        assert_eq!(state.get_player(1).unwrap().respawn_timer, 2);

        update_timers(&mut state);
        assert_eq!(state.get_player(1).unwrap().respawn_timer, 1);

        let ready = update_timers(&mut state);
        assert_eq!(ready, vec![1]);
    }
}