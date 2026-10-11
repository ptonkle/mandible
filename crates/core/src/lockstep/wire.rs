//! Wire types: peers, targets, seats and every message, as JSON.

use serde::{Deserialize, Serialize};

/// The protocol version. A joiner with a different one is refused.
/// 2: Ready Up (`Msg::Ready`, `SeatKind::Human::ready`) and Rematch rounds
/// (`round` in `Roster` and `Ping`).
pub const PROTOCOL_VERSION: u32 = 2;

/// No message this module produces is longer than this many bytes.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

/// An opaque peer name given by the transport.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PeerId(pub String);

/// Where an outgoing message goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// One peer.
    Peer(PeerId),
    /// Every connected peer.
    All,
}

/// A message for the transport to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub to: Target,
    pub text: String,
}

impl Outgoing {
    pub(crate) fn to_peer(peer: &PeerId, msg: &Msg) -> Outgoing {
        Outgoing {
            to: Target::Peer(peer.clone()),
            text: msg.encode(),
        }
    }

    pub(crate) fn to_all(msg: &Msg) -> Outgoing {
        Outgoing {
            to: Target::All,
            text: msg.encode(),
        }
    }
}

/// The protocol version and build id a client runs. Both must match the host's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub proto: u32,
    pub build: String,
}

impl Version {
    /// This protocol version with the given build id.
    pub fn current(build: &str) -> Version {
        Version {
            proto: PROTOCOL_VERSION,
            build: build.to_string(),
        }
    }
}

/// Who sits in a seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeatKind {
    Open,
    Closed,
    Human {
        client_id: String,
        name: String,
        /// Ready Up: this joiner is set to start. The host's own seat is
        /// never marked; the host's Start is its Ready Up.
        #[serde(default)]
        ready: bool,
    },
    Ai {
        difficulty: u8,
    },
}

/// One seat of the lobby and the match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub kind: SeatKind,
    pub team: u8,
}

impl Seat {
    /// The client id of a human seat.
    pub fn client_id(&self) -> Option<&str> {
        match &self.kind {
            SeatKind::Human { client_id, .. } => Some(client_id),
            _ => None,
        }
    }

    /// A human seat that has pressed Ready Up.
    pub fn ready(&self) -> bool {
        matches!(self.kind, SeatKind::Human { ready: true, .. })
    }
}

/// Why the host refused a joiner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum RefuseReason {
    /// No open seat.
    Full,
    /// The protocol version or build differs; the host's are named.
    WrongVersion { host_proto: u32, host_build: String },
    /// The match has already started.
    AlreadyStarted,
}

/// A turn's commands for one tick, as carried in drop messages.
pub type HeldTurn = (u32, Vec<String>);

/// Every message on the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    // ---- lobby ----
    Hello {
        proto: u32,
        build: String,
        client_id: String,
        name: String,
    },
    Refuse {
        reason: RefuseReason,
    },
    Roster {
        version: u32,
        seats: Vec<Seat>,
        settings: String,
        /// The lobby's round: 0 for a first lobby, one more for each
        /// Rematch.
        #[serde(default)]
        round: u32,
    },
    TeamRequest {
        team: u8,
    },
    /// A joiner's Ready Up (or its undoing), repeated until the roster
    /// shows it.
    Ready {
        ready: bool,
    },
    Leave,
    Dissolve,
    Ping {
        client_id: String,
        stamp: i64,
        /// The sender's round (see `Roster`): a ping of this round proves
        /// that everything the sender sent before it was the old match's.
        #[serde(default)]
        round: u32,
    },
    Pong {
        stamp: i64,
    },
    Start {
        seed: u64,
        ms_per_tick: u32,
        settings: String,
        seats: Vec<Seat>,
    },
    StartAck,
    // ---- match ----
    Turn {
        seat: u8,
        tick: u32,
        commands: Vec<String>,
        checksum: Option<(u32, u64)>,
    },
    DesyncReport {
        tick: u32,
    },
    ResyncBegin {
        epoch: u32,
        tick: u32,
    },
    SnapshotChunk {
        epoch: u32,
        tick: u32,
        index: u32,
        count: u32,
        data: String,
    },
    DropQuery {
        round: u32,
        seat: u8,
    },
    DropReport {
        round: u32,
        seat: u8,
        held: Option<u32>,
        turns: Vec<HeldTurn>,
    },
    DropCommit {
        seat: u8,
        from_tick: u32,
        turns: Vec<HeldTurn>,
    },
}

impl Msg {
    /// The JSON text of this message.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parses a message; `None` for text that is not one.
    pub fn decode(text: &str) -> Option<Msg> {
        serde_json::from_str(text).ok()
    }

    /// Lobby-phase messages, which are repeated until they take effect and so
    /// survive loss. Match messages need the reliable, ordered channel.
    pub fn is_lobby_kind(&self) -> bool {
        matches!(
            self,
            Msg::Hello { .. }
                | Msg::Refuse { .. }
                | Msg::Roster { .. }
                | Msg::TeamRequest { .. }
                | Msg::Ready { .. }
                | Msg::Ping { .. }
                | Msg::Pong { .. }
                | Msg::Start { .. }
                | Msg::StartAck
        )
    }

    /// Match messages that a lobby keeps until its match starts.
    pub(crate) fn is_match_kind(&self) -> bool {
        matches!(
            self,
            Msg::Turn { .. }
                | Msg::DesyncReport { .. }
                | Msg::ResyncBegin { .. }
                | Msg::SnapshotChunk { .. }
                | Msg::DropQuery { .. }
                | Msg::DropReport { .. }
                | Msg::DropCommit { .. }
        )
    }
}
