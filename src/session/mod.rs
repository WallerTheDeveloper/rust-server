use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub type PlayerId = u32;

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
}

/// Manages all connected player sessions
pub struct SessionManager {
    /// Map from socket address to session
    sessions_by_addr: HashMap<SocketAddr, Session>,
    /// Map from player ID to socket address (for reverse lookup)
    addr_by_player_id: HashMap<PlayerId, SocketAddr>,
    /// Next player ID to assign
    next_player_id: PlayerId,
    /// How long before a session is considered timed out
    timeout_duration: Duration,
}

impl SessionManager {
    pub fn new(timeout_seconds: u64) -> Self {
        Self {
            sessions_by_addr: HashMap::new(),
            addr_by_player_id: HashMap::new(),
            next_player_id: 1,
            timeout_duration: Duration::from_secs(timeout_seconds),
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

        let session = Session {
            player_id,
            player_name,
            addr,
            room_code: None,
            last_seen: Instant::now(),
            last_ping: None,
            latency_ms: None,
            ping_count: 0,
        };

        self.sessions_by_addr.insert(addr, session);
        self.addr_by_player_id.insert(player_id, addr);

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
            tracing::info!("Player disconnected: id={}, addr={}", session.player_id, addr);
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
}