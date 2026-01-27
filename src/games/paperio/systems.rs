use super::state::{Direction, GameState, GridPos, Player, TerritoryGrid};
use super::config::PaperioConfig;
use crate::game::traits::PlayerId;
use std::collections::{HashSet, VecDeque, HashMap};

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

#[derive(Debug, Default)]
pub struct ClaimResult {
    /// Number of new cells claimed
    pub cells_claimed: usize,
    /// Number of cells stolen from other players
    pub cells_stolen: usize,
    /// Players who lost territory
    pub victims: Vec<PlayerId>,
}


pub fn claim_territory(
    territory: &mut TerritoryGrid,
    player_id: PlayerId,
    trail: &[GridPos],
) -> ClaimResult {
    let mut result = ClaimResult::default();

    if trail.is_empty() {
        return result;
    }

    for pos in trail {
        if territory.in_bounds(pos) {
            let previous_owner = territory.get_cell_owner(pos);

            if let Some(owner) = previous_owner {
                if owner != player_id {
                    result.cells_stolen += 1;
                    if !result.victims.contains(&owner) {
                        result.victims.push(owner);
                    }
                }
            }

            if previous_owner != Some(player_id) {
                result.cells_claimed += 1;
            }

            territory.set_cell_owner(pos, Some(player_id));
        }
    }

    let enclosed_cells = flood_fill_from_edges(territory, player_id);

    for pos in enclosed_cells {
        let previous_owner = territory.get_cell_owner(&pos);

        if let Some(owner) = previous_owner {
            if owner != player_id {
                result.cells_stolen += 1;
                if !result.victims.contains(&owner) {
                    result.victims.push(owner);
                }
            }
        }

        result.cells_claimed += 1;
        territory.set_cell_owner(&pos, Some(player_id));
    }

    tracing::debug!(
        "Player {} claimed {} cells ({} stolen from {:?})",
        player_id, result.cells_claimed, result.cells_stolen, result.victims
    );

    result
}

fn flood_fill_from_edges(
    territory: &TerritoryGrid,
    player_id: PlayerId,
) -> Vec<GridPos> {
    let (width, height) = territory.get_grid_dimensions();

    let mut player_cells: HashSet<GridPos> = HashSet::new();
    for y in 0..height {
        for x in 0..width {
            let pos = GridPos::new(x as i32, y as i32);
            if territory.is_owned_by(&pos, player_id) {
                player_cells.insert(pos);
            }
        }
    }

    let mut reachable: HashSet<GridPos> = HashSet::new();
    let mut queue: VecDeque<GridPos> = VecDeque::new();

    for x in 0..width {
        let top = GridPos::new(x as i32, 0);
        let bottom = GridPos::new(x as i32, (height - 1) as i32);

        if !player_cells.contains(&top) && reachable.insert(top) {
            queue.push_back(top);
        }
        if !player_cells.contains(&bottom) && reachable.insert(bottom) {
            queue.push_back(bottom);
        }
    }

    for y in 0..height {
        let left = GridPos::new(0, y as i32);
        let right = GridPos::new((width - 1) as i32, y as i32);

        if !player_cells.contains(&left) && reachable.insert(left) {
            queue.push_back(left);
        }
        if !player_cells.contains(&right) && reachable.insert(right) {
            queue.push_back(right);
        }
    }

    // BFS flood fill - spread to all connected non-player cells
    while let Some(pos) = queue.pop_front() {
        for neighbor in get_neighbors(&pos) {
            if territory.in_bounds(&neighbor)
                && !player_cells.contains(&neighbor)
                && reachable.insert(neighbor)
            {
                queue.push_back(neighbor);
            }
        }
    }

    let mut enclosed = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let pos = GridPos::new(x as i32, y as i32);
            if !reachable.contains(&pos) && !player_cells.contains(&pos) {
                enclosed.push(pos);
            }
        }
    }

    enclosed
}

fn get_neighbors(pos: &GridPos) -> [GridPos; 4] {
    [
        pos.offset(-1, 0),  // Left
        pos.offset(1, 0),   // Right
        pos.offset(0, -1),  // Up
        pos.offset(0, 1),   // Down
    ]
}

#[derive(Debug, Clone)]
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

impl std::fmt::Display for EliminationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EliminationReason::TrailCut => write!(f, "trail was cut"),
            EliminationReason::SelfCollision => write!(f, "crossed own trail"),
            EliminationReason::HeadCollision => write!(f, "head-on collision"),
            EliminationReason::Boundary => write!(f, "hit boundary"),
        }
    }
}

pub fn check_collisions(state: &GameState) -> Vec<Elimination> {
    let mut eliminations = Vec::new();
    let mut already_eliminated: HashSet<PlayerId> = HashSet::new();

    let alive_players: Vec<(PlayerId, GridPos, bool, &Vec<GridPos>)> = state.players
        .values()
        .filter(|p| p.alive)
        .map(|p| (p.id, p.position, p.is_invulnerable(), &p.trail))
        .collect();

    let mut trail_map: HashMap<GridPos, PlayerId> = HashMap::new();
    for (player_id, _, _, trail) in &alive_players {
        for pos in *trail {
            trail_map.insert(*pos, *player_id);
        }
    }

    let mut position_map: HashMap<GridPos, Vec<PlayerId>> = HashMap::new();
    for (player_id, position, _, _) in &alive_players {
        position_map
            .entry(*position)
            .or_insert_with(Vec::new)
            .push(*player_id);
    }

    for (player_id, position, is_invulnerable, own_trail) in &alive_players {
        if *is_invulnerable {
            continue;
        }

        if let Some(&trail_owner) = trail_map.get(position) {
            if trail_owner == *player_id {
                tracing::info!(
                    "Player {} eliminated: crossed own trail at {:?}",
                    player_id, position
                );
                eliminations.push(Elimination {
                    victim: *player_id,
                    killer: 0, // Self-caused
                    reason: EliminationReason::SelfCollision,
                });
                already_eliminated.insert(*player_id);
            } else {
                if !already_eliminated.contains(&trail_owner) {
                    let trail_owner_invulnerable = state.players
                        .get(&trail_owner)
                        .map(|p| p.is_invulnerable())
                        .unwrap_or(false);

                    if !trail_owner_invulnerable {
                        tracing::info!(
                            "Player {} eliminated: trail cut by player {} at {:?}",
                            trail_owner, player_id, position
                        );
                        eliminations.push(Elimination {
                            victim: trail_owner,
                            killer: *player_id,
                            reason: EliminationReason::TrailCut,
                        });
                        already_eliminated.insert(trail_owner);
                    }
                }
            }
        }
    }

    for (position, players_at_pos) in &position_map {
        if players_at_pos.len() >= 2 {
            let mut colliding_players: Vec<(PlayerId, u32, bool)> = players_at_pos
                .iter()
                .filter(|id| !already_eliminated.contains(id))
                .filter_map(|id| {
                    state.players.get(id).map(|p| (*id, p.score, p.is_invulnerable()))
                })
                .collect();

            colliding_players.sort_by(|a, b| b.1.cmp(&a.1));

            let vulnerable_players: Vec<_> = colliding_players
                .iter()
                .filter(|(_, _, invuln)| !invuln)
                .collect();

            if vulnerable_players.len() >= 2 {
                let top_score = vulnerable_players[0].1;
                let tied_for_first: Vec<_> = vulnerable_players
                    .iter()
                    .filter(|(_, score, _)| *score == top_score)
                    .collect();

                if tied_for_first.len() >= 2 {
                    for (player_id, _, _) in tied_for_first {
                        tracing::info!(
                            "Player {} eliminated: head-on collision (tied score) at {:?}",
                            player_id, position
                        );
                        eliminations.push(Elimination {
                            victim: *player_id,
                            killer: 0, // Mutual elimination
                            reason: EliminationReason::HeadCollision,
                        });
                        already_eliminated.insert(*player_id);
                    }
                } else {
                    let winner_id = vulnerable_players[0].0;
                    for (player_id, _, _) in vulnerable_players.iter().skip(1) {
                        tracing::info!(
                            "Player {} eliminated: head-on collision with {} at {:?}",
                            player_id, winner_id, position
                        );
                        eliminations.push(Elimination {
                            victim: *player_id,
                            killer: winner_id,
                            reason: EliminationReason::HeadCollision,
                        });
                        already_eliminated.insert(*player_id);
                    }
                }
            }
        }
    }

    eliminations
}

pub fn eliminate_player(
    state: &mut GameState,
    player_id: PlayerId,
    reason: EliminationReason,
    respawn_delay: u32,
) {
    if let Some(player) = state.players.get_mut(&player_id) {
        player.alive = false;
        player.trail.clear();
        player.direction = Direction::None;
        player.respawn_timer = respawn_delay;

        tracing::info!(
            "Player {} eliminated ({}), respawning in {} ticks",
            player_id,
            reason,
            respawn_delay
        );
    }
}

pub fn find_spawn_position(state: &GameState, config: &PaperioConfig) -> Option<GridPos> {
    let (width, height) = state.territory.get_grid_dimensions();
    let margin = config.starting_territory_size as i32;
    let min_distance = config.min_spawn_distance;

    // Get positions of all alive players
    let occupied_positions: Vec<GridPos> = state.players
        .values()
        .filter(|p| p.alive)
        .map(|p| p.position)
        .collect();

    if occupied_positions.is_empty() {
        return Some(GridPos::new(width as i32 / 2, height as i32 / 2));
    }

    let center = GridPos::new(width as i32 / 2, height as i32 / 2);
    let mut best_pos: Option<GridPos> = None;
    let mut best_min_distance: u32 = 0;

    for radius in 0..=(width.max(height) / 2) as i32 {
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                if dx.abs() != radius && dy.abs() != radius {
                    continue;
                }

                let pos = center.offset(dx, dy);

                if pos.x < margin || pos.x >= (width as i32 - margin) ||
                    pos.y < margin || pos.y >= (height as i32 - margin) {
                    continue;
                }

                let min_dist = occupied_positions
                    .iter()
                    .map(|other| pos.distance(other))
                    .min()
                    .unwrap_or(u32::MAX);

                if min_dist >= min_distance {
                    let area_clear = is_spawn_area_clear(
                        &state.territory,
                        &pos,
                        config.starting_territory_size,
                    );

                    if area_clear {
                        return Some(pos);
                    }
                }

                if min_dist > best_min_distance {
                    best_min_distance = min_dist;
                    best_pos = Some(pos);
                }
            }
        }
    }

    best_pos.or(Some(center))
}

fn is_spawn_area_clear(territory: &TerritoryGrid, center: &GridPos, size: u32) -> bool {
    let half = (size / 2) as i32;
    let mut claimed_count = 0;
    let total_cells = (size * size) as i32;

    for dy in -half..=half {
        for dx in -half..=half {
            let pos = center.offset(dx, dy);
            if territory.get_cell_owner(&pos).is_some() {
                claimed_count += 1;
            }
        }
    }

    // Allow spawn if less than 25% of the area is claimed
    claimed_count < total_cells / 4
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

        tracing::info!(
            "Player {} respawned at {:?} with {} ticks invulnerability",
            player_id, spawn_pos, config.invulnerability_ticks
        );
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

        state.players.get_mut(&1).unwrap().position = GridPos::new(11, 10);

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
        assert_eq!(direction_from_proto(99), Direction::None);
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

    #[test]
    fn test_claim_simple_rectangle() {
        let mut state = GameState::new(10, 10);
        let center = GridPos::new(3, 3);

        grant_starting_territory(&mut state.territory, 1, &center, 3);

        let trail = vec![
            GridPos::new(5, 4),
            GridPos::new(5, 3),
            GridPos::new(5, 2),
            GridPos::new(4, 2),
        ];

        let result = claim_territory(&mut state.territory, 1, &trail);

        assert!(result.cells_claimed > 0);

        for pos in &trail {
            assert!(state.territory.is_owned_by(pos, 1),
                    "Trail cell {:?} should be owned by player 1", pos);
        }
    }

    #[test]
    fn test_claim_l_shaped_territory() {
        let mut state = GameState::new(10, 10);
        grant_starting_territory(&mut state.territory, 1, &GridPos::new(3, 3), 3);

        let trail = vec![
            GridPos::new(5, 3),
            GridPos::new(5, 4),
            GridPos::new(4, 4),
            GridPos::new(4, 5),
            GridPos::new(5, 5),
            GridPos::new(6, 5),
            GridPos::new(6, 4),
            GridPos::new(6, 3),
            GridPos::new(6, 2),
            GridPos::new(5, 2),
            GridPos::new(4, 2),
        ];

        let result = claim_territory(&mut state.territory, 1, &trail);

        assert!(result.cells_claimed > 0, "Should claim some cells");

        for pos in &trail {
            assert!(state.territory.is_owned_by(pos, 1));
        }
    }

    #[test]
    fn test_claim_steals_enemy_territory() {

        let mut state = GameState::new(10, 10);

        grant_starting_territory(&mut state.territory, 1, &GridPos::new(2, 5), 3);

        grant_starting_territory(&mut state.territory, 2, &GridPos::new(5, 5), 3);

        let initial_p2_cells = state.territory.count_owned_by(2);

        let trail = vec![
            GridPos::new(4, 5),
            GridPos::new(4, 4),
            GridPos::new(4, 3),
            GridPos::new(5, 3),
            GridPos::new(6, 3),
            GridPos::new(7, 3),
            GridPos::new(7, 4),
            GridPos::new(7, 5),
            GridPos::new(7, 6),
            GridPos::new(7, 7),
            GridPos::new(6, 7),
            GridPos::new(5, 7),
            GridPos::new(4, 7),
            GridPos::new(4, 6),
            GridPos::new(3, 6),
        ];

        let result = claim_territory(&mut state.territory, 1, &trail);

        assert!(result.cells_stolen > 0, "Should have stolen cells from player 2");
        assert!(result.victims.contains(&2), "Player 2 should be in victims list");

        let final_p2_cells = state.territory.count_owned_by(2);
        assert!(final_p2_cells < initial_p2_cells,
                "Player 2 should have lost territory: {} -> {}", initial_p2_cells, final_p2_cells);
    }

    #[test]
    fn test_claim_no_enclosed_area() {

        let mut state = GameState::new(10, 10);
        grant_starting_territory(&mut state.territory, 1, &GridPos::new(5, 5), 3);

        let trail = vec![
            GridPos::new(7, 5),
            GridPos::new(8, 5),
            GridPos::new(9, 5),
        ];

        let result = claim_territory(&mut state.territory, 1, &trail);

        assert_eq!(result.cells_claimed, 3);
        assert_eq!(result.cells_stolen, 0);
    }

    #[test]
    fn test_claim_empty_trail() {
        let mut state = GameState::new(10, 10);
        grant_starting_territory(&mut state.territory, 1, &GridPos::new(5, 5), 3);

        let trail: Vec<GridPos> = vec![];
        let result = claim_territory(&mut state.territory, 1, &trail);

        assert_eq!(result.cells_claimed, 0);
        assert_eq!(result.cells_stolen, 0);
        assert!(result.victims.is_empty());
    }

    #[test]
    fn test_flood_fill_finds_enclosed_cells() {
        let mut territory = TerritoryGrid::new(10, 10);

        for x in 2..=6 {
            territory.set_cell_owner(&GridPos::new(x, 2), Some(1));
            territory.set_cell_owner(&GridPos::new(x, 6), Some(1));
        }
        for y in 3..=5 {
            territory.set_cell_owner(&GridPos::new(2, y), Some(1));
            territory.set_cell_owner(&GridPos::new(6, y), Some(1));
        }

        let enclosed = flood_fill_from_edges(&territory, 1);

        assert_eq!(enclosed.len(), 9, "Should find 9 enclosed cells in the center");

        for pos in &enclosed {
            assert!(pos.x >= 3 && pos.x <= 5 && pos.y >= 3 && pos.y <= 5,
                    "Enclosed cell {:?} should be in center area", pos);
        }
    }

    #[test]
    fn test_flood_fill_no_enclosed_when_open() {
        let mut territory = TerritoryGrid::new(10, 10);

        // Create U-shape (open at top)
        for x in 2..=6 {
            territory.set_cell_owner(&GridPos::new(x, 6), Some(1)); // bottom
        }
        for y in 3..=5 {
            territory.set_cell_owner(&GridPos::new(2, y), Some(1)); // left
            territory.set_cell_owner(&GridPos::new(6, y), Some(1)); // right
        }
        // Top has gaps
        territory.set_cell_owner(&GridPos::new(2, 2), Some(1));
        territory.set_cell_owner(&GridPos::new(3, 2), Some(1));
        territory.set_cell_owner(&GridPos::new(5, 2), Some(1));
        territory.set_cell_owner(&GridPos::new(6, 2), Some(1));
        // Gap at (4, 2)

        let enclosed = flood_fill_from_edges(&territory, 1);

        assert!(enclosed.is_empty(),
                "Should have no enclosed cells when there's a gap, found: {:?}", enclosed);
    }

    #[test]
    fn test_get_neighbors() {
        let pos = GridPos::new(5, 5);
        let neighbors = get_neighbors(&pos);

        assert!(neighbors.contains(&GridPos::new(4, 5))); // Left
        assert!(neighbors.contains(&GridPos::new(6, 5))); // Right
        assert!(neighbors.contains(&GridPos::new(5, 4))); // Up
        assert!(neighbors.contains(&GridPos::new(5, 6))); // Down
    }

    #[test]
    fn test_self_collision() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        let player = state.players.get_mut(&1).unwrap();
        player.trail = vec![
            GridPos::new(12, 10),
            GridPos::new(13, 10),
            GridPos::new(14, 10),
        ];
        player.position = GridPos::new(13, 10);

        let eliminations = check_collisions(&state);

        assert_eq!(eliminations.len(), 1);
        assert_eq!(eliminations[0].victim, 1);
        assert_eq!(eliminations[0].reason, EliminationReason::SelfCollision);
    }

    #[test]
    fn test_trail_cut_by_other_player() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);
        setup_player_with_territory(&mut state, 2);

        let player1 = state.players.get_mut(&1).unwrap();
        player1.position = GridPos::new(15, 10);
        player1.trail = vec![
            GridPos::new(12, 10),
            GridPos::new(13, 10),
            GridPos::new(14, 10),
        ];

        let player2 = state.players.get_mut(&2).unwrap();
        player2.position = GridPos::new(13, 10); // On player 1's trail

        let eliminations = check_collisions(&state);

        assert_eq!(eliminations.len(), 1);
        assert_eq!(eliminations[0].victim, 1);
        assert_eq!(eliminations[0].killer, 2);
        assert_eq!(eliminations[0].reason, EliminationReason::TrailCut);
    }

    #[test]
    fn test_head_on_collision_different_scores() {
        let mut state = setup_game_state();

        let mut player1 = Player::new(1, "Player1".to_string(), GridPos::new(5, 5), 0xFF0000FF);
        player1.score = 1000;
        state.players.insert(1, player1);

        let mut player2 = Player::new(2, "Player2".to_string(), GridPos::new(5, 5), 0x00FF00FF);
        player2.score = 500;
        state.players.insert(2, player2);

        let eliminations = check_collisions(&state);

        assert_eq!(eliminations.len(), 1);
        assert_eq!(eliminations[0].victim, 2);
        assert_eq!(eliminations[0].killer, 1);
        assert_eq!(eliminations[0].reason, EliminationReason::HeadCollision);
    }

    #[test]
    fn test_head_on_collision_same_score() {
        let mut state = setup_game_state();

        let mut player1 = Player::new(1, "Player1".to_string(), GridPos::new(5, 5), 0xFF0000FF);
        player1.score = 500;
        state.players.insert(1, player1);

        let mut player2 = Player::new(2, "Player2".to_string(), GridPos::new(5, 5), 0x00FF00FF);
        player2.score = 500;
        state.players.insert(2, player2);

        let eliminations = check_collisions(&state);

        assert_eq!(eliminations.len(), 2);
        let victims: HashSet<_> = eliminations.iter().map(|e| e.victim).collect();
        assert!(victims.contains(&1));
        assert!(victims.contains(&2));
    }

    #[test]
    fn test_invulnerable_player_not_eliminated() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);
        setup_player_with_territory(&mut state, 2);

        let player1 = state.players.get_mut(&1).unwrap();
        player1.invulnerability_timer = 10;
        player1.trail = vec![
            GridPos::new(12, 10),
            GridPos::new(13, 10),
        ];

        let player2 = state.players.get_mut(&2).unwrap();
        player2.position = GridPos::new(13, 10);

        let eliminations = check_collisions(&state);

        assert!(eliminations.is_empty());
    }

    #[test]
    fn test_eliminate_player_sets_state() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);

        // Add a trail
        state.players.get_mut(&1).unwrap().trail = vec![
            GridPos::new(12, 10),
        ];

        eliminate_player(&mut state, 1, EliminationReason::TrailCut, 60);

        let player = state.get_player(1).unwrap();
        assert!(!player.alive);
        assert!(player.trail.is_empty());
        assert_eq!(player.direction, Direction::None);
        assert_eq!(player.respawn_timer, 60);
    }

    #[test]
    fn test_find_spawn_position_empty_map() {
        let state = GameState::new(100, 100);
        let config = PaperioConfig::default();

        let spawn_pos = find_spawn_position(&state, &config);

        assert!(spawn_pos.is_some());
        let pos = spawn_pos.unwrap();
        // Should be near center for empty map
        assert!(pos.x >= 40 && pos.x <= 60);
        assert!(pos.y >= 40 && pos.y <= 60);
    }

    #[test]
    fn test_find_spawn_position_with_players() {
        let mut state = GameState::new(50, 50);
        let config = PaperioConfig::default();

        let player = Player::new(1, "Center".to_string(), GridPos::new(25, 25), 0xFF0000FF);
        state.players.insert(1, player);
        grant_starting_territory(&mut state.territory, 1, &GridPos::new(25, 25), 5);

        let spawn_pos = find_spawn_position(&state, &config);

        assert!(spawn_pos.is_some());
        let pos = spawn_pos.unwrap();
        let distance = pos.distance(&GridPos::new(25, 25));
        assert!(distance >= config.min_spawn_distance,
                "Spawn position {:?} too close to existing player (distance: {})", pos, distance);
    }

    #[test]
    fn test_respawn_player_grants_invulnerability() {
        let mut state = setup_game_state();
        let config = PaperioConfig::default();

        let player = Player::new(1, "Test".to_string(), GridPos::new(10, 10), 0xFF0000FF);
        state.players.insert(1, player);
        eliminate_player(&mut state, 1, EliminationReason::Boundary, 60);

        // Respawn
        let spawn_pos = respawn_player(&mut state, 1, &config);

        assert!(spawn_pos.is_some());
        let player = state.get_player(1).unwrap();
        assert!(player.alive);
        assert!(player.is_invulnerable());
        assert_eq!(player.invulnerability_timer, config.invulnerability_ticks);
    }

    #[test]
    fn test_no_collision_when_players_separated() {
        let mut state = setup_game_state();
        setup_player_with_territory(&mut state, 1);
        setup_player_with_territory(&mut state, 2);

        state.players.get_mut(&1).unwrap().position = GridPos::new(5, 5);
        state.players.get_mut(&2).unwrap().position = GridPos::new(15, 15);

        let eliminations = check_collisions(&state);

        assert!(eliminations.is_empty());
    }

    #[test]
    fn test_multiple_trail_cuts_same_tick() {
        let mut state = setup_game_state();

        let mut player1 = Player::new(1, "P1".to_string(), GridPos::new(5, 5), 0xFF0000FF);
        player1.trail = vec![GridPos::new(10, 10), GridPos::new(10, 11)];
        state.players.insert(1, player1);

        let mut player2 = Player::new(2, "P2".to_string(), GridPos::new(10, 10), 0x00FF00FF);
        player2.trail = vec![GridPos::new(5, 5)];
        state.players.insert(2, player2);

        let eliminations = check_collisions(&state);

        assert_eq!(eliminations.len(), 2);
        let victims: HashSet<_> = eliminations.iter().map(|e| e.victim).collect();
        assert!(victims.contains(&1));
        assert!(victims.contains(&2));
    }
}
