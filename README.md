# Paper.io 2 — Authoritative Multiplayer Game Server

> A high-performance, authoritative game server for a **Paper.io 2 clone** built in Rust with async networking, designed to pair with a Unity 3D client. Demonstrates production-grade multiplayer architecture: deterministic tick simulation, territory claiming via flood-fill, delta-compressed state sync, and seamless reconnection — all at 20 Hz.

Built as a portfolio piece for the **Senior Game Developer** position at [Voodoo](https://www.voodoo.io/), the original creators of Paper.io.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                       RUST SERVER (Authoritative)               │
│                                                                 │
│  ┌───────────┐   ┌────────────┐   ┌───────────────────────┐     │
│  │  Network  │   │  Session   │   │     Room Manager      │     │
│  │  Layer    │   │  Manager   │   │  (Waiting → Playing)  │     │
│  │           │   │            │   │                       │     │
│  │  UDP ◄────┤   │  Register  │   │  Join / Leave / Ready │     │
│  │  WS  ◄────┤   │  Reconnect │   │  Auto-start on ready  │     │
│  │           │   │  Timeout   │   │                       │     │
│  └─────┬─────┘   └──────┬─────┘   └───────────┬───────────┘     │
│        │                │                      │                │
│        ▼                ▼                      ▼                │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                  Unified Transport                       │   │
│  │         Routes to UDP or WebSocket automatically         │   │
│  └──────────────────────┬───────────────────────────────────┘   │
│                         │                                       │
│                         ▼                                       │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Game Room (per active match)                │   │
│  │                                                          │   │
│  │  ┌─────────────────────────────────────────────────┐     │   │
│  │  │            PaperioGame (Game trait)             │     │   │
│  │  │                                                 │     │   │
│  │  │  tick()  →  Movement → Collision → Claiming     │     │   │
│  │  │  handle_input()  →  Direction changes           │     │   │
│  │  │  encode_state()  →  Keyframe / Delta            │     │   │
│  │  └─────────────────────────────────────────────────┘     │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                    Protobuf over UDP / WS
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    UNITY CLIENT (3D Rendering)                  │
│                                                                 │
│  Input Capture → Network Manager → State Interpolation          │
│  Territory Renderer → Trail Renderer → Camera Controller        │
└─────────────────────────────────────────────────────────────────┘
```

The server is **fully authoritative** — the client never determines game outcomes. All simulation (movement, collisions, territory claiming) runs server-side at a fixed 20 Hz tick rate.

---

## Key Features

###  Dual-Transport Networking (UDP + WebSocket)

The server accepts connections over both raw UDP (for native clients) and WebSocket (for WebGL builds), using a unified `Transport` abstraction. WebSocket clients are assigned virtual `SocketAddr` values in the `127.255.x.x` range so the entire session/room/game pipeline works identically regardless of transport — like adding a second entrance to the same building while keeping all the hallways identical.

```
src/network/
├── udp.rs          # Async UDP socket (tokio)
├── ws.rs           # WebSocket listener with virtual addressing
└── transport.rs    # Unified send interface — auto-routes per client
```

###  Game Trait Abstraction

All game logic is decoupled from networking through a generic `Game` trait. The server infrastructure (transport, sessions, rooms) never imports game-specific code. This means a new game can be added by implementing the trait and creating a new binary — no changes to the core server.

```rust
pub trait Game: Send + Sync {
    fn tick(&mut self) -> TickResult;
    fn handle_input(&mut self, player_id: PlayerId, input: &[u8]) -> Result<(), GameError>;
    fn player_joined(&mut self, player_id: PlayerId, name: String) -> Result<Vec<u8>, GameError>;
    fn player_left(&mut self, player_id: PlayerId);
    fn encode_state(&self) -> Vec<u8>;
    fn tick_rate(&self) -> Duration;
}
```

###  Territory System with Flood-Fill Claiming

When a player's trail closes (returns to own territory), the server:

1. Converts all trail cells to the player's ownership
2. Runs an **edge-seeded BFS flood fill** to find all cells reachable from map borders without crossing the player's territory
3. Claims all unreachable (enclosed) cells — including stealing from other players
4. Returns a `ClaimResult` with cells claimed, cells stolen, and victim list

This is the same algorithm that powers the original Paper.io's "draw a loop to capture area" mechanic, implemented with correctness verified by unit tests covering simple rectangles, L-shapes, enemy territory theft, open shapes (no false enclosures), and empty trails.

###  Delta-Compressed State Synchronization

Instead of sending the full 100×100 territory grid every tick, the server uses a **keyframe + delta** scheme:

- **Keyframes** (every 20 ticks / 1 second): Full state with RLE-compressed territory rows
- **Deltas** (every other tick): Only changed cells since the last keyframe, sent as `TerritoryCell` diffs

Territory snapshots are captured at each keyframe and diffed against the current state. In practice, delta frames are dramatically smaller than full frames — verified by tests showing deltas at a fraction of keyframe size for typical game states.

###  Session Management & Reconnection

Players who drop connection enter a **grace period** (configurable, default 120 seconds). During this window:

- Their session is preserved with a unique reconnect token
- Other players receive a `PlayerDisconnected` notification with the grace duration
- On reconnect, the player's session migrates to the new address, sequence counters reset, and a `PlayerReconnected` event broadcasts to the room
- If reconnecting to an active game, the server re-adds them to the `GameRoom` and sends a fresh `PaperioJoinResponse` with full current state

Expired sessions are cleaned up automatically and their room membership is properly torn down.

###  Collision & Elimination System

Every tick after movement, the server checks:

- **Trail cuts**: If any player's position lands on another player's trail, the trail owner is eliminated
- **Self-collision**: Stepping on your own trail kills you
- **Head-on collision**: Two players on the same cell — higher score survives; tied scores eliminate both
- **Boundary collision**: Moving outside the grid eliminates the player
- **Invulnerability**: Recently respawned players have a configurable invulnerability window

Eliminated players respawn after a configurable delay at a position chosen by a distance-maximizing algorithm that avoids existing players and claimed territory.

###  Room System

Rooms handle the lobby lifecycle: players join (or auto-create) rooms, mark themselves ready, and the game starts when all players are ready with at least 2 present. The room state machine transitions through `Waiting → Playing → Ended`. Late joiners can connect to in-progress games and receive the current game state immediately.

---

## Protocol

All communication uses **Protocol Buffers** over UDP or WebSocket. Messages are wrapped in `ClientMessage` / `ServerMessage` envelopes with sequence numbers for ordering and duplicate detection.

### Client → Server
| Message | Purpose |
|---|---|
| `JoinRoom` | Join or create a room with a room code and player name |
| `Ready` | Signal readiness to start the game |
| `GameMessage` | Wrapped game-specific input (e.g., `PaperioInput` with direction) |
| `Ping` | Latency measurement |
| `Reconnect` | Resume a disconnected session using a token |
| `LeaveRoom` | Exit the current room |

### Server → Client
| Message | Purpose |
|---|---|
| `RoomJoined` | Confirmation with player ID, room code, player list, reconnect token |
| `RoomUpdate` | Updated player list (on join/leave/ready changes) |
| `GameStarting` | Countdown notification before match begins |
| `GameMessage` | Wrapped game state (full keyframe or delta) |
| `Pong` | Latency response with server timestamp |
| `PlayerDisconnected` / `PlayerReconnected` | Connection status changes |
| `PlayerLeft` | Permanent departure from room |

### Game-Specific Protocol (Paper.io)

`PaperioState` supports two encoding modes via the `StateType` discriminator:

- **`STATE_FULL`**: Complete player list + RLE-compressed territory grid (used for keyframes and join responses)
- **`STATE_DELTA`**: Complete player list + only changed `TerritoryCell` entries since the last keyframe

---

## Game Configuration

| Parameter | Default | Description |
|---|---|---|
| Grid Size | 100 × 100 | Playable area in cells |
| Tick Rate | 20 Hz (50ms) | Server simulation frequency |
| Max Players | 16 | Per room |
| Starting Territory | 3×3 | Square granted on spawn |
| Respawn Delay | 60 ticks (3s) | Time before respawning after death |
| Invulnerability | 40 ticks (2s) | Protection window after respawn |
| Min Spawn Distance | 15 cells | Minimum separation between spawn points |
| Keyframe Interval | 20 ticks (1s) | Full state broadcast frequency |
| Move Interval | 3 ticks | Grid cells per movement step |

---

## Testing

The game logic is backed by comprehensive unit tests covering the critical systems:

```bash
cargo test
```

Test coverage includes:

- **Territory claiming**: Simple rectangles, L-shapes, enemy territory theft, empty trails, open shapes (no false enclosures)
- **Flood fill**: Enclosed cell detection, open shapes correctly produce no enclosures, edge cases
- **Collision detection**: Trail cuts, head-on collisions (score-based resolution and ties), invulnerability bypass, mutual eliminations, separated players (no false positives)
- **Movement**: Direction changes, boundary detection, trail creation outside territory, stationary behavior, timer-based movement pacing
- **Respawn**: Timer countdown, spawn position distance constraints, invulnerability granting
- **Encoding**: RLE territory compression, delta snapshot diffs, full vs delta size comparison, join response encoding, direction/position conversion roundtrips
- **Configuration**: Default values, tick duration calculation, color assignment

---

## Tech Stack

| Component | Technology |
|---|---|
| Language | Rust (2024 edition) |
| Async Runtime | Tokio (full features) |
| Serialization | Protocol Buffers via `prost` |
| WebSocket | `tokio-tungstenite` + `futures-util` |
| Logging | `tracing` + `tracing-subscriber` |
| Client | Unity (URP, new Input System) |

---

## Two Server Binaries

The project ships two separate binaries, reflecting the module-based separation between generic infrastructure and game-specific logic:

1. **`server`** (`src/main.rs`) — A generic **relay server** that handles rooms, sessions, and message forwarding. No game simulation. Useful as a lobby server or for games where the client is authoritative.

2. **`paperio_server`** (`src/bin/paperio_server.rs`) — The **authoritative game server** that runs the full Paper.io simulation. Includes the tick loop, game rooms with `PaperioGame` instances, input queuing, and state broadcasting.

```bash
# Run the authoritative Paper.io server
cargo run --bin paperio_server

# Run the generic relay server (lobby only)
cargo run --bin server
```

---

## Design Principles

- **Generic networking never imports game code** — all game-to-network communication flows through the `Game` trait interface
- **Server is authoritative** — clients send input, server determines all outcomes
- **Correctness before optimization** — the claiming algorithm and collision system are unit-tested before any performance tuning
- **Game state encoding is the game's responsibility** — the generic server just forwards opaque `bytes` payloads
- **Module-based separation today, workspace extraction tomorrow** — the directory structure is designed so `network/`, `session/`, `room/`, and `game/` can become a `server-core` crate when multiple games exist

---

## License

This project is a portfolio demonstration. Not affiliated with or endorsed by Voodoo.
