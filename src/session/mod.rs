use crate::config::GRACE_PLAYER_TIME_SECONDS;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub type PlayerId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub player_id: PlayerId,
    pub player_name: String,
    pub addr: SocketAddr,
    pub room_code: Option<String>,
    pub last_seen: Instant,
    pub last_ping: Option<Instant>,
    pub latency_ms: Option<u32>,
    pub ping_count: u32,
    pub connection_state: ConnectionState,
    pub reconnect_token: String,
    pub disconnected_at: Option<Instant>,
}

/// Manages all connected player sessions
pub struct SessionManager {
    /// Map from socket address to session
    sessions_by_addr: HashMap<SocketAddr, Session>,
    /// Map from player ID to socket address (for reverse lookup)
    addr_by_player_id: HashMap<PlayerId, SocketAddr>,
    /// Map from player ID to token
    token_to_player_id: HashMap<String, PlayerId>,
    /// Next player ID to assign
    next_player_id: PlayerId,
    /// How long before a session is considered timed out
    timeout_duration: Duration,
    /// How long to wait before removing player
    grace_period: Duration,
}

impl SessionManager {
    pub fn new(timeout_seconds: u64) -> Self {
        Self {
            sessions_by_addr: HashMap::new(),
            addr_by_player_id: HashMap::new(),
            token_to_player_id: HashMap::new(),
            next_player_id: 1,
            timeout_duration: Duration::from_secs(timeout_seconds),
            grace_period: Duration::from_secs(GRACE_PLAYER_TIME_SECONDS as u64),
        }
    }

    pub fn register(&mut self, addr: SocketAddr, player_name: String) -> &Session {
        if let Some(session) = self.sessions_by_addr.get_mut(&addr) {
            session.last_seen = Instant::now();
            session.player_name = player_name;
            return self.sessions_by_addr.get(&addr).unwrap();
        }

        let player_id = self.next_player_id;

        // TODO: Generate unique player ID
        self.next_player_id += 1;

        let reconnect_token = Self::generate_reconnect_token();
        let session = Session {
            player_id,
            player_name,
            addr,
            room_code: None,
            last_seen: Instant::now(),
            last_ping: None,
            latency_ms: None,
            ping_count: 0,
            connection_state: ConnectionState::Connected,
            reconnect_token: reconnect_token.clone(),
            disconnected_at: None,
        };

        self.sessions_by_addr.insert(addr, session);
        self.addr_by_player_id.insert(player_id, addr);
        self.token_to_player_id.insert(reconnect_token, player_id);

        tracing::info!("New player registered: id={}, addr={}", player_id, addr);

        self.sessions_by_addr.get(&addr).unwrap()
    }
    pub fn ping(&mut self, addr: &SocketAddr) {
        if let Some(session) = self.sessions_by_addr.get_mut(addr) {
            session.last_ping = Some(Instant::now());
            session.last_seen = Instant::now();
            session.ping_count += 1;
        }
    }

    pub fn update_last_seen(&mut self, addr: &SocketAddr) {
        if let Some(session) = self.sessions_by_addr.get_mut(addr) {
            session.last_seen = Instant::now();
        }
    }

    pub fn get_by_addr(&self, addr: &SocketAddr) -> Option<&Session> {
        self.sessions_by_addr.get(addr)
    }

    pub fn get_by_addr_mut(&mut self, addr: &SocketAddr) -> Option<&mut Session> {
        self.sessions_by_addr.get_mut(addr)
    }

    pub fn get_by_player_id(&self, player_id: PlayerId) -> Option<&Session> {
        self.addr_by_player_id
            .get(&player_id)
            .and_then(|addr| self.sessions_by_addr.get(addr))
    }

    pub fn remove(&mut self, addr: &SocketAddr) -> Option<Session> {
        if let Some(session) = self.sessions_by_addr.remove(addr) {
            self.addr_by_player_id.remove(&session.player_id);
            tracing::info!(
                "Player disconnected: id={}, addr={}",
                session.player_id,
                addr
            );
            Some(session)
        } else {
            None
        }
    }

    pub fn cleanup_timed_out(&mut self) -> Vec<Session> {
        let now = Instant::now();
        let timeout = self.timeout_duration;

        let timed_out_addrs: Vec<SocketAddr> = self
            .sessions_by_addr
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_seen) > timeout)
            .map(|(addr, _)| *addr)
            .collect();

        let mut removed = Vec::new();
        for addr in timed_out_addrs {
            if let Some(session) = self.remove(&addr) {
                tracing::info!(
                    "Player timed out: id={}, name={}",
                    session.player_id,
                    session.player_name
                );
                removed.push(session);
            }
        }

        removed
    }

    pub fn mark_player_disconnected(&mut self, addr: &SocketAddr) -> Option<PlayerId> {
        if let Some(session) = self.sessions_by_addr.get_mut(addr) {
            session.connection_state = ConnectionState::Disconnected;
            session.disconnected_at = Some(Instant::now());
            tracing::info!(
                "Player {} marked as disconnected (grace period: {}s)",
                session.player_id,
                self.grace_period.as_secs()
            );
            return Some(session.player_id);
        }
        None
    }

    pub fn reconnected_by_token(
        &mut self,
        token: &str,
        new_addr: SocketAddr,
        player_name: String,
    ) -> Option<&Session> {
        let player_id = *self.token_to_player_id.get(token)?;
        let old_addr = *self.addr_by_player_id.get(&player_id)?;

        {
            let session = self.sessions_by_addr.get(&old_addr)?;

            if session.connection_state != ConnectionState::Disconnected {
                tracing::debug!(
                    "Reconnect rejected for player {}: not in disconnected state",
                    player_id
                );
                return None;
            }

            if let Some(disconnected_at) = session.disconnected_at {
                if disconnected_at.elapsed() > self.grace_period {
                    tracing::info!(
                        "Reconnect rejected for player {}: grace period expired",
                        player_id
                    );
                    return None;
                }
            }
        }

        let mut session = self.sessions_by_addr.remove(&old_addr).unwrap();

        session.addr = new_addr;
        session.player_name = player_name;
        session.connection_state = ConnectionState::Connected;
        session.disconnected_at = None;
        session.last_seen = Instant::now();

        self.addr_by_player_id.insert(player_id, new_addr);
        self.sessions_by_addr.insert(new_addr, session);

        tracing::info!("Player {player_id} reconnected from new address {new_addr}");

        self.sessions_by_addr.get(&new_addr)
    }

    pub fn grace_period_seconds(&self) -> u32 {
        self.grace_period.as_secs() as u32
    }

    pub fn cleanup_expired_disconnected(&mut self) -> Vec<Session> {
        let now = Instant::now();

        let expired_addrs: Vec<SocketAddr> = self
            .sessions_by_addr
            .iter()
            .filter(|(_, session)| {
                if session.connection_state == ConnectionState::Disconnected {
                    if let Some(disconnected_at) = session.disconnected_at {
                        return now.duration_since(disconnected_at) > self.grace_period;
                    }
                }
                false
            })
            .map(|(addr, _)| *addr)
            .collect();

        let mut removed = Vec::new();

        for addr in expired_addrs {
            if let Some(session) = self.remove(&addr) {
                tracing::info!("Player {} removed: grace period expired", session.player_id);
                removed.push(session);
            }
        }

        removed
    }

    pub fn remove(&mut self, addr: &SocketAddr) -> Option<Session> {
        if let Some(session) = self.sessions_by_addr.remove(addr) {
            self.addr_by_player_id.remove(&session.player_id);
            self.token_to_player_id.remove(&session.reconnect_token);
            tracing::info!("Player disconnected: id={}, addr={}", session.player_id, addr);

            Some(session)
        } else {
            None
        }
    }

    // TODO:
    // 1. Move to helper file
    // 2. This is not a safe implementation. Have to be cryptographically secure
    pub fn generate_reconnect_token() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        format!(
            "{:x}{:x}",
            timestamp,
            timestamp.wrapping_mul(6364136223846793005)
        )
    }
}
