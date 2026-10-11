//! Deterministic lockstep networking (PLAN decision 17).
//!
//! Pure state machines for the lobby and the match. They are fed transport
//! events (peer connected, peer disconnected, message text) and the current
//! milliseconds from the caller, and they return the messages to send. They
//! read no clock and touch no transport, so `cargo test` drives them with the
//! [`loopback`] network and a fake clock.
//!
//! The transport is a full mesh: every peer can message every other directly,
//! over one reliable, ordered channel per pair. Messages are JSON text.
//!
//! - [`Lobby`]: host-authoritative seats, teams and settings, round-trip
//!   measurement, and the robust Start.
//! - [`Match`]: per-player turns with per-player input delay, checksums,
//!   resync from the coordinator's snapshot, agreed drops and coordinator
//!   migration.
//!
//! The game simulation is not part of this module: commands, settings and
//! snapshots are opaque strings.

mod game;
mod lobby;
pub mod loopback;
mod rtt;
mod wire;

pub use game::{Match, MatchStatus, TickInput, CHECKSUM_EVERY, SNAPSHOT_CHUNK_BYTES};
pub use lobby::{clean_name, Lobby, LobbyError, LobbyStatus, MatchStart, MAX_NAME_CHARS};
pub use rtt::RttWindow;
pub use wire::{
    Msg, Outgoing, PeerId, RefuseReason, Seat, SeatKind, Target, Version, MAX_MESSAGE_BYTES,
    PROTOCOL_VERSION,
};
