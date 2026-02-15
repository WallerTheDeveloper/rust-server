use prost::Message;
use crate::game::traits::PlayerId;
use crate::protocol::paperio::{
    Direction as ProtoDirection, GridPosition, PaperioPlayer, PaperioState,
    TerritoryRow, TerritoryRun, PaperioJoinResponse, TerritoryCell, StateType,
};
use super::state::{Direction, GameState, GridPos, Player, TerritoryGrid};
use super::config::PaperioConfig;

fn direction_to_proto(dir: Direction) -> i32 {
    match dir {
        Direction::None => ProtoDirection::None as i32,
        Direction::Up => ProtoDirection::Up as i32,
        Direction::Down => ProtoDirection::Down as i32,
        Direction::Left => ProtoDirection::Left as i32,
        Direction::Right => ProtoDirection::Right as i32,
    }
}

fn grid_pos_to_proto(pos: &GridPos) -> GridPosition {
    GridPosition { x: pos.x, y: pos.y }
}

fn player_to_proto(player: &Player) -> PaperioPlayer {
    PaperioPlayer {
        player_id: player.id,
        name: player.name.clone(),
        position: Some(grid_pos_to_proto(&player.position)),
        direction: direction_to_proto(player.direction),
        trail: player.trail.iter().map(grid_pos_to_proto).collect(),
        alive: player.alive,
        score: player.score,
        color: player.color,
    }
}


/// A snapshot of territory ownership used for computing deltas.
/// Stores a flat copy of the territory grid's cells array.
#[derive(Clone)]
pub struct TerritorySnapshot {
    cells: Vec<Option<PlayerId>>,
    width: u32,
    height: u32,
}

impl TerritorySnapshot {
    pub fn capture(territory: &TerritoryGrid) -> Self {
        Self {
            cells: territory.cells.clone(),
            width: territory.width,
            height: territory.height,
        }
    }

    pub fn diff(&self, current: &TerritoryGrid) -> Vec<TerritoryCell> {
        let mut changes = Vec::new();

        for (idx, current_owner) in current.cells.iter().enumerate() {
            let previous_owner = self.cells[idx];

            if *current_owner != previous_owner {
                let x = (idx as u32 % self.width) as i32;
                let y = (idx as u32 / self.width) as i32;
                let owner_id = current_owner.unwrap_or(0);

                changes.push(TerritoryCell {
                    x,
                    y,
                    owner_id,
                });
            }
        }

        changes
    }

    pub fn has_changes(&self, current: &TerritoryGrid) -> bool {
        self.cells.iter().zip(current.cells.iter()).any(|(a, b)| a != b)
    }
}

fn encode_territory_rle(territory: &TerritoryGrid) -> Vec<TerritoryRow> {
    let mut rows = Vec::with_capacity(territory.height as usize);

    for y in 0..territory.height as i32 {
        let mut runs: Vec<TerritoryRun> = Vec::new();
        let mut current_owner: Option<PlayerId> = None;
        let mut current_count: u32 = 0;

        for x in 0..territory.width as i32 {
            let pos = GridPos::new(x, y);
            let owner = territory.get_cell_owner(&pos);

            if owner != current_owner {
                if current_count > 0 {
                    runs.push(TerritoryRun {
                        owner_id: current_owner.unwrap_or(0),
                        count: current_count,
                    });
                }
                current_owner = owner;
                current_count = 1;
            } else {
                current_count += 1;
            }
        }

        if current_count > 0 {
            runs.push(TerritoryRun {
                owner_id: current_owner.unwrap_or(0),
                count: current_count,
            });
        }

        let has_owned_cells = runs.iter().any(|r| r.owner_id != 0);
        if has_owned_cells {
            rows.push(TerritoryRow { y, runs });
        }
    }

    rows
}

pub fn encode_full_state(state: &GameState, tick: u32, config: &PaperioConfig) -> Vec<u8> {
    let players: Vec<PaperioPlayer> = state
        .players
        .values()
        .map(player_to_proto)
        .collect();

    let territory = encode_territory_rle(&state.territory);

    let proto_state = PaperioState {
        tick,
        players,
        territory,
        grid_width: config.grid_width,
        grid_height: config.grid_height,
        state_type: StateType::StateFull as i32,
        territory_changes: Vec::new(),
        keyframe_tick: tick,
    };

    proto_state.encode_to_vec()
}

pub fn encode_delta_state(
    state: &GameState,
    tick: u32,
    config: &PaperioConfig,
    territory_changes: Vec<TerritoryCell>,
    keyframe_tick: u32,
) -> Vec<u8> {
    let players: Vec<PaperioPlayer> = state
        .players
        .values()
        .map(player_to_proto)
        .collect();

    let proto_state = PaperioState {
        tick,
        players,
        territory: Vec::new(),
        grid_width: config.grid_width,
        grid_height: config.grid_height,
        state_type: StateType::StateDelta as i32,
        territory_changes,
        keyframe_tick,
    };

    proto_state.encode_to_vec()
}

pub fn encode_state(state: &GameState, tick: u32, config: &PaperioConfig) -> Vec<u8> {
    encode_full_state(state, tick, config)
}

pub fn encode_state_for_player(
    state: &GameState,
    tick: u32,
    config: &PaperioConfig,
    _player_id: PlayerId,
) -> Vec<u8> {
    encode_state(state, tick, config)
}

pub fn encode_join_response(
    player_id: PlayerId,
    state: &GameState,
    tick: u32,
    config: &PaperioConfig,
) -> Vec<u8> {
    let initial_state = PaperioState {
        tick,
        players: state.players.values().map(player_to_proto).collect(),
        territory: encode_territory_rle(&state.territory),
        grid_width: config.grid_width,
        grid_height: config.grid_height,
        state_type: StateType::StateFull as i32,
        territory_changes: Vec::new(),
        keyframe_tick: tick,
    };

    let response = PaperioJoinResponse {
        your_player_id: player_id,
        initial_state: Some(initial_state),
        tick_rate_ms: 1000 / config.tick_rate_hz,
        move_interval_ticks: 3
    };

    response.encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::paperio::systems::grant_starting_territory;

    #[test]
    fn test_direction_conversion() {
        assert_eq!(direction_to_proto(Direction::None), 0);
        assert_eq!(direction_to_proto(Direction::Up), 1);
        assert_eq!(direction_to_proto(Direction::Down), 2);
        assert_eq!(direction_to_proto(Direction::Left), 3);
        assert_eq!(direction_to_proto(Direction::Right), 4);
    }

    #[test]
    fn test_grid_pos_conversion() {
        let pos = GridPos::new(5, 10);
        let proto = grid_pos_to_proto(&pos);
        assert_eq!(proto.x, 5);
        assert_eq!(proto.y, 10);
    }

    #[test]
    fn test_player_conversion() {
        let player = Player::new(1, "Alice".to_string(), GridPos::new(10, 20), 0xFF0000FF);
        let proto = player_to_proto(&player);

        assert_eq!(proto.player_id, 1);
        assert_eq!(proto.name, "Alice");
        assert_eq!(proto.position.unwrap().x, 10);
        assert_eq!(proto.position.unwrap().y, 20);
        assert!(proto.alive);
        assert_eq!(proto.color, 0xFF0000FF);
    }

    #[test]
    fn test_territory_rle_empty() {
        let territory = TerritoryGrid::new(10, 10);
        let rows = encode_territory_rle(&territory);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_territory_rle_single_owner() {
        let mut territory = TerritoryGrid::new(10, 10);
        for x in 0..10 {
            territory.set_cell_owner(&GridPos::new(x, 5), Some(1));
        }
        let rows = encode_territory_rle(&territory);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].y, 5);
        assert_eq!(rows[0].runs.len(), 1);
        assert_eq!(rows[0].runs[0].owner_id, 1);
        assert_eq!(rows[0].runs[0].count, 10);
    }

    #[test]
    fn test_territory_rle_multiple_owners() {
        let mut territory = TerritoryGrid::new(10, 10);
        for x in 0..3 {
            territory.set_cell_owner(&GridPos::new(x, 3), Some(1));
        }
        for x in 5..9 {
            territory.set_cell_owner(&GridPos::new(x, 3), Some(2));
        }
        let rows = encode_territory_rle(&territory);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].y, 3);
        assert_eq!(rows[0].runs.len(), 4);
        assert_eq!(rows[0].runs[0], TerritoryRun { owner_id: 1, count: 3 });
        assert_eq!(rows[0].runs[1], TerritoryRun { owner_id: 0, count: 2 });
        assert_eq!(rows[0].runs[2], TerritoryRun { owner_id: 2, count: 4 });
        assert_eq!(rows[0].runs[3], TerritoryRun { owner_id: 0, count: 1 });
    }

    #[test]
    fn test_encode_full_state_produces_valid_protobuf() {
        let mut state = GameState::new(20, 20);
        let config = PaperioConfig::with_grid_size(20, 20);

        let spawn = GridPos::new(10, 10);
        let player = Player::new(1, "Test".to_string(), spawn, 0xAABBCCDD);
        state.players.insert(1, player);
        grant_starting_territory(&mut state.territory, 1, &spawn, 3);

        let bytes = encode_full_state(&state, 42, &config);
        assert!(!bytes.is_empty());

        let decoded = PaperioState::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.tick, 42);
        assert_eq!(decoded.grid_width, 20);
        assert_eq!(decoded.grid_height, 20);
        assert_eq!(decoded.players.len(), 1);
        assert_eq!(decoded.state_type, StateType::StateFull as i32);
        assert!(decoded.territory_changes.is_empty());
        assert!(!decoded.territory.is_empty());
    }

    #[test]
    fn test_encode_join_response() {
        let state = GameState::new(50, 50);
        let config = PaperioConfig::with_grid_size(50, 50);

        let bytes = encode_join_response(42, &state, 0, &config);
        let decoded = PaperioJoinResponse::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.your_player_id, 42);
        assert_eq!(decoded.tick_rate_ms, 50);
        assert!(decoded.initial_state.is_some());
    }

    #[test]
    fn test_snapshot_captures_territory() {
        let mut territory = TerritoryGrid::new(10, 10);
        territory.set_cell_owner(&GridPos::new(3, 3), Some(1));

        let snapshot = TerritorySnapshot::capture(&territory);

        assert_eq!(snapshot.cells[3 * 10 + 3], Some(1));
        assert_eq!(snapshot.cells[0], None);
    }

    #[test]
    fn test_snapshot_diff_detects_changes() {
        let mut territory = TerritoryGrid::new(10, 10);
        territory.set_cell_owner(&GridPos::new(3, 3), Some(1));

        let snapshot = TerritorySnapshot::capture(&territory);

        territory.set_cell_owner(&GridPos::new(4, 3), Some(1)); // New claim
        territory.set_cell_owner(&GridPos::new(3, 3), Some(2)); // Stolen by player 2

        let changes = snapshot.diff(&territory);

        assert_eq!(changes.len(), 2, "Should detect exactly 2 changed cells");

        let change_at_3_3 = changes.iter().find(|c| c.x == 3 && c.y == 3).unwrap();
        assert_eq!(change_at_3_3.owner_id, 2, "Cell (3,3) should now be owned by player 2");

        let change_at_4_3 = changes.iter().find(|c| c.x == 4 && c.y == 3).unwrap();
        assert_eq!(change_at_4_3.owner_id, 1, "Cell (4,3) should now be owned by player 1");
    }

    #[test]
    fn test_snapshot_diff_no_changes() {
        let mut territory = TerritoryGrid::new(10, 10);
        territory.set_cell_owner(&GridPos::new(5, 5), Some(1));

        let snapshot = TerritorySnapshot::capture(&territory);

        let changes = snapshot.diff(&territory);
        assert!(changes.is_empty(), "No changes should produce empty diff");
    }

    #[test]
    fn test_snapshot_diff_unclaim() {
        let mut territory = TerritoryGrid::new(10, 10);
        territory.set_cell_owner(&GridPos::new(5, 5), Some(1));

        let snapshot = TerritorySnapshot::capture(&territory);

        territory.set_cell_owner(&GridPos::new(5, 5), None);

        let changes = snapshot.diff(&territory);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].owner_id, 0, "Unclaimed cells should have owner_id 0");
    }

    #[test]
    fn test_snapshot_has_changes() {
        let mut territory = TerritoryGrid::new(10, 10);
        let snapshot = TerritorySnapshot::capture(&territory);

        assert!(!snapshot.has_changes(&territory), "Should detect no changes");

        territory.set_cell_owner(&GridPos::new(0, 0), Some(1));

        assert!(snapshot.has_changes(&territory), "Should detect change");
    }

    #[test]
    fn test_encode_delta_state() {
        let mut state = GameState::new(20, 20);
        let config = PaperioConfig::with_grid_size(20, 20);

        state.players.insert(1, Player::new(1, "Test".to_string(), GridPos::new(10, 10), 0xAABBCCDD));

        let changes = vec![
            TerritoryCell { x: 5, y: 5, owner_id: 1 },
            TerritoryCell { x: 6, y: 5, owner_id: 1 },
        ];

        let bytes = encode_delta_state(&state, 42, &config, changes, 20);
        assert!(!bytes.is_empty());

        let decoded = PaperioState::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.tick, 42);
        assert_eq!(decoded.state_type, StateType::StateDelta as i32);
        assert_eq!(decoded.keyframe_tick, 20);
        assert!(decoded.territory.is_empty(), "Delta should have no RLE territory");
        assert_eq!(decoded.territory_changes.len(), 2);
        assert_eq!(decoded.territory_changes[0].x, 5);
        assert_eq!(decoded.territory_changes[0].y, 5);
        assert_eq!(decoded.territory_changes[0].owner_id, 1);
    }

    #[test]
    fn test_delta_much_smaller_than_full() {
        let mut state = GameState::new(100, 100);
        let config = PaperioConfig::default();

        for y in 0..50 {
            for x in 0..50 {
                state.territory.set_cell_owner(&GridPos::new(x, y), Some(1));
            }
        }

        state.players.insert(1, Player::new(1, "Test".to_string(), GridPos::new(10, 10), 0xAABBCCDD));

        let full_bytes = encode_full_state(&state, 100, &config);

        let changes = vec![
            TerritoryCell { x: 50, y: 50, owner_id: 1 },
            TerritoryCell { x: 51, y: 50, owner_id: 1 },
            TerritoryCell { x: 52, y: 50, owner_id: 1 },
        ];
        let delta_bytes = encode_delta_state(&state, 101, &config, changes, 100);

        let ratio = delta_bytes.len() as f64 / full_bytes.len() as f64;
        tracing::info!(
            "Full state: {} bytes, Delta: {} bytes, Ratio: {:.1}%",
            full_bytes.len(),
            delta_bytes.len(),
            ratio * 100.0
        );

        assert!(
            delta_bytes.len() < full_bytes.len(),
            "Delta ({} bytes) should be smaller than full state ({} bytes)",
            delta_bytes.len(),
            full_bytes.len()
        );
    }
}