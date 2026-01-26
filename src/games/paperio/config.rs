use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PaperioConfig {
    /// Grid width in cells
    pub grid_width: u32,
    /// Grid height in cells
    pub grid_height: u32,
    /// Server tick rate in Hz (ticks per second)
    pub tick_rate_hz: u32,
    /// Maximum number of players per game
    pub max_players: usize,
    /// Size of starting territory (square around spawn point)
    pub starting_territory_size: u32,
    /// Respawn delay in ticks
    pub respawn_delay_ticks: u32,
    /// Invulnerability period after respawn in ticks
    pub invulnerability_ticks: u32,
    /// Minimum distance between spawn points
    pub min_spawn_distance: u32,
}

impl PaperioConfig {
    pub fn with_grid_size(width: u32, height: u32) -> Self {
        Self {
            grid_width: width,
            grid_height: height,
            ..Default::default()
        }
    }

    pub fn tick_duration(&self) -> Duration {
        Duration::from_millis(1000 / self.tick_rate_hz as u64)
    }

    pub fn respawn_delay_seconds(&self) -> f32 {
        self.respawn_delay_ticks as f32 / self.tick_rate_hz as f32
    }
}

impl Default for PaperioConfig {
    fn default() -> Self {
        Self {
            grid_width: 100,
            grid_height: 100,
            tick_rate_hz: 20,
            max_players: 16,
            starting_territory_size: 3,
            respawn_delay_ticks: 60,
            invulnerability_ticks: 40,
            min_spawn_distance: 15,
        }
    }
}

pub const PLAYER_COLORS: [u32; 16] = [
    0xFF5733FF, // Orange-red
    0x33FF57FF, // Green
    0x3357FFFF, // Blue
    0xFFFF33FF, // Yellow
    0xFF33FFFF, // Magenta
    0x33FFFFFF, // Cyan
    0xFF8C00FF, // Dark orange
    0x8A2BE2FF, // Blue violet
    0x00CED1FF, // Dark turquoise
    0xDC143CFF, // Crimson
    0x32CD32FF, // Lime green
    0x4169E1FF, // Royal blue
    0xFFD700FF, // Gold
    0x9400D3FF, // Dark violet
    0x00FA9AFF, // Medium spring green
    0xFF6347FF, // Tomato
];

pub fn get_player_color(player_id: u32) -> u32 {
    PLAYER_COLORS[(player_id as usize) % PLAYER_COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PaperioConfig::default();
        assert_eq!(config.grid_width, 100);
        assert_eq!(config.grid_height, 100);
        assert_eq!(config.tick_rate_hz, 20);
        assert_eq!(config.tick_duration(), Duration::from_millis(50));
    }

    #[test]
    fn test_respawn_delay() {
        let config = PaperioConfig::default();
        assert!((config.respawn_delay_seconds() - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_player_colors() {
        // Colors should cycle
        assert_eq!(get_player_color(0), get_player_color(16));
        assert_eq!(get_player_color(1), get_player_color(17));
        // But adjacent should differ
        assert_ne!(get_player_color(0), get_player_color(1));
    }
}