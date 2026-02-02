use prost::Message;
use crate::game::traits::PlayerId;
use crate::protocol::paperio::{
    Direction as ProtoDirection, GridPosition, PaperioPlayer, PaperioState,
    TerritoryRow, TerritoryRun, PaperioJoinResponse,
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

/// Encode territory grid using run-length encoding
///
/// For each row, we create a TerritoryRow with runs of consecutive cells
/// owned by the same player. This significantly reduces bandwidth for
/// large territories compared to sending each cell individually.
///
/// Example: A row like [1,1,1,None,None,2,2] becomes:
/// [Run{owner:1, count:3}, Run{owner:0, count:2}, Run{owner:2, count:2}]
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

pub fn encode_state(state: &GameState, tick: u32, config: &PaperioConfig) -> Vec<u8> {
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
    };

    proto_state.encode_to_vec()
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
    };

    let response = PaperioJoinResponse {
        your_player_id: player_id,
        initial_state: Some(initial_state),
        tick_rate_ms: 1000 / config.tick_rate_hz,
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

        // Fill row 5 with player 1
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

        // Row 3: [1,1,1,0,0,2,2,2,2,0]
        for x in 0..3 {
            territory.set_cell_owner(&GridPos::new(x, 3), Some(1));
        }
        for x in 5..9 {
            territory.set_cell_owner(&GridPos::new(x, 3), Some(2));
        }

        let rows = encode_territory_rle(&territory);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].y, 3);

        // Should have 4 runs: [1x3, 0x2, 2x4, 0x1]
        assert_eq!(rows[0].runs.len(), 4);
        assert_eq!(rows[0].runs[0], TerritoryRun { owner_id: 1, count: 3 });
        assert_eq!(rows[0].runs[1], TerritoryRun { owner_id: 0, count: 2 });
        assert_eq!(rows[0].runs[2], TerritoryRun { owner_id: 2, count: 4 });
        assert_eq!(rows[0].runs[3], TerritoryRun { owner_id: 0, count: 1 });
    }

    #[test]
    fn test_encode_state_produces_valid_protobuf() {
        let mut state = GameState::new(20, 20);
        let config = PaperioConfig::with_grid_size(20, 20);

        let spawn = GridPos::new(10, 10);
        let player = Player::new(1, "Test".to_string(), spawn, 0xAABBCCDD);
        state.players.insert(1, player);
        grant_starting_territory(&mut state.territory, 1, &spawn, 3);

        let bytes = encode_state(&state, 42, &config);

        assert!(!bytes.is_empty());

        let decoded = PaperioState::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.tick, 42);
        assert_eq!(decoded.grid_width, 20);
        assert_eq!(decoded.grid_height, 20);
        assert_eq!(decoded.players.len(), 1);
        assert_eq!(decoded.players[0].player_id, 1);
        assert_eq!(decoded.players[0].name, "Test");
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
}
