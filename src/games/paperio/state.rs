use crate::game::traits::PlayerId;
use std::collections::HashMap;

/// A position on the game grid
///
/// (0,0) is the top-left corner,
/// x increases to the right, y increases downward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridPos {
    pub x: i32,
    pub y: i32,
}

impl GridPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    pub fn moved(&self, direction: Direction) -> Self {
        let (dx, dy) = direction.delta();
        self.offset(dx, dy)
    }

    pub fn distance(&self, other: &GridPos) -> u32 {
        ((self.x - other.x).abs() + (self.y - other.y).abs()) as u32
    }
}

impl std::ops::Add for GridPos {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl std::ops::Sub for GridPos {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    None,
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn delta(&self) -> (i32, i32) {
        match self {
            Direction::None => (0, 0),
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    pub fn is_opposite(&self, other: &Direction) -> bool {
        matches!(
            (self, other),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        )
    }

    pub fn get_opposite(&self) -> Direction {
        match self {
            Direction::None => Direction::None,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

pub struct TerritoryGrid {
    /// Width of the grid
    width: u32,
    /// Height of the grid
    height: u32,
    /// Ownership data: None = unclaimed, Some(id) = owned by player
    cells: Vec<Option<PlayerId>>,
}

impl TerritoryGrid {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cells: vec![None; (width * height) as usize],
        }
    }

    pub fn get_grid_dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn in_bounds(&self, pos: &GridPos) -> bool {
        pos.x >= 0 && pos.y >= 0 &&
            (pos.x as u32) < self.width &&
            (pos.y as u32) < self.height
    }

    fn pos_to_index(&self, pos: &GridPos) -> Option<usize> {
        if self.in_bounds(pos) {
            Some((pos.y as u32 * self.width + pos.x as u32) as usize)
        } else {
            None
        }
    }

    pub fn get_cell_owner(&self, pos: &GridPos) -> Option<PlayerId> {
        self.pos_to_index(pos)
            .and_then(|idx| self.cells[idx])
    }

    pub fn set_cell_owner(&mut self, pos: &GridPos, owner: Option<PlayerId>) {
        if let Some(idx) = self.pos_to_index(pos) {
            self.cells[idx] = owner;
        }
    }

    pub fn is_owned_by(&self, pos: &GridPos, player_id: PlayerId) -> bool {
        self.get_cell_owner(pos) == Some(player_id)
    }

    pub fn count_owned_by(&self, player_id: PlayerId) -> usize {
        self.cells.iter().filter(|&&c| c == Some(player_id)).count()
    }

    pub fn get_total_cells(&self) -> usize {
        self.cells.len()
    }

    pub fn get_ownership_percentage(&self, player_id: PlayerId) -> f32 {
        let owned = self.count_owned_by(player_id) as f32;
        let total = self.get_total_cells() as f32;
        (owned / total) * 100.0
    }

    pub fn get_owned_cells(&self, player_id: PlayerId) -> Vec<GridPos> {
        let mut cells = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let pos = GridPos::new(x as i32, y as i32);
                if self.is_owned_by(&pos, player_id) {
                    cells.push(pos);
                }
            }
        }
        cells
    }
}

impl std::fmt::Debug for TerritoryGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerritoryGrid")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("claimed_cells", &self.cells.iter().filter(|c| c.is_some()).count())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    /// Unique player identifier
    pub id: PlayerId,
    /// Display name
    pub name: String,
    /// Current position on the grid
    pub position: GridPos,
    /// Current movement direction
    pub direction: Direction,
    /// Trail left when moving outside own territory
    /// Empty when player is inside their own territory
    pub trail: Vec<GridPos>,
    /// Whether the player is currently alive
    pub alive: bool,
    /// Player's score (territory percentage * 100 for integer precision)
    pub score: u32,
    /// Player's color (RGBA packed as u32)
    pub color: u32,
    /// Ticks remaining until respawn (0 if alive)
    pub respawn_timer: u32,
    /// Ticks remaining of invulnerability (0 if vulnerable)
    pub invulnerability_timer: u32,
}

impl Player {
    pub fn new(id: PlayerId, name: String, position: GridPos, color: u32) -> Self {
        Self {
            id,
            name,
            position,
            direction: Direction::None,
            trail: Vec::new(),
            alive: true,
            score: 0,
            color,
            respawn_timer: 0,
            invulnerability_timer: 0,
        }
    }

    pub fn is_invulnerable(&self) -> bool {
        self.invulnerability_timer > 0
    }

    pub fn is_respawning(&self) -> bool {
        !self.alive && self.respawn_timer > 0
    }

    pub fn has_trail(&self) -> bool {
        !self.trail.is_empty()
    }
}

#[derive(Debug)]
pub struct GameState {
    /// All players in the game
    pub players: HashMap<PlayerId, Player>,
    /// Territory ownership grid
    pub territory: TerritoryGrid,
}

impl GameState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            players: HashMap::new(),
            territory: TerritoryGrid::new(width, height),
        }
    }

    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.get_mut(&id)
    }

    pub fn get_alive_players(&self) -> impl Iterator<Item = &Player> {
        self.players.values().filter(|p| p.alive)
    }

    pub fn get_player_ids(&self) -> Vec<PlayerId> {
        self.players.keys().copied().collect()
    }

    pub fn get_alive_count(&self) -> usize {
        self.players.values().filter(|p| p.alive).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_pos_operations() {
        let pos = GridPos::new(5, 10);
        assert_eq!(pos.offset(1, -1), GridPos::new(6, 9));
        assert_eq!(pos.moved(Direction::Up), GridPos::new(5, 9));
        assert_eq!(pos.moved(Direction::Right), GridPos::new(6, 10));
    }

    #[test]
    fn test_grid_pos_distance() {
        let a = GridPos::new(0, 0);
        let b = GridPos::new(3, 4);
        assert_eq!(a.distance(&b), 7); // Manhattan distance
    }

    #[test]
    fn test_direction_delta() {
        assert_eq!(Direction::Up.delta(), (0, -1));
        assert_eq!(Direction::Down.delta(), (0, 1));
        assert_eq!(Direction::Left.delta(), (-1, 0));
        assert_eq!(Direction::Right.delta(), (1, 0));
        assert_eq!(Direction::None.delta(), (0, 0));
    }

    #[test]
    fn test_direction_opposite() {
        assert!(Direction::Up.is_opposite(&Direction::Down));
        assert!(Direction::Left.is_opposite(&Direction::Right));
        assert!(!Direction::Up.is_opposite(&Direction::Left));
        assert!(!Direction::None.is_opposite(&Direction::Up));
    }

    #[test]
    fn test_territory_grid() {
        let mut grid = TerritoryGrid::new(10, 10);
        let pos = GridPos::new(5, 5);

        assert!(grid.in_bounds(&pos));
        assert!(!grid.in_bounds(&GridPos::new(-1, 0)));
        assert!(!grid.in_bounds(&GridPos::new(10, 5)));

        assert_eq!(grid.get_cell_owner(&pos), None);
        grid.set_cell_owner(&pos, Some(1));
        assert_eq!(grid.get_cell_owner(&pos), Some(1));
        assert!(grid.is_owned_by(&pos, 1));
    }

    #[test]
    fn test_ownership_percentage() {
        let mut grid = TerritoryGrid::new(10, 10); // 100 cells

        // Claim 10 cells for player 1
        for x in 0..10 {
            grid.set_cell_owner(&GridPos::new(x, 0), Some(1));
        }

        assert!((grid.get_ownership_percentage(1) - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_player_state() {
        let player = Player::new(1, "Test".to_string(), GridPos::new(0, 0), 0xFF0000FF);
        assert!(player.alive);
        assert!(!player.has_trail());
        assert!(!player.is_invulnerable());
    }

    #[test]
    fn test_game_state() {
        let state = GameState::new(100, 100);
        assert_eq!(state.players.len(), 0);
        assert_eq!(state.territory.get_grid_dimensions(), (100, 100));
    }
}
