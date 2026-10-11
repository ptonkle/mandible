//! The match: per-player turns over a full mesh.
//!
//! Every live human seat sends exactly one turn per tick, in order, from
//! tick 0, to every peer. A tick runs when this client holds that tick's turn
//! from every live human seat. AI seats send nothing; every client runs them.
//!
//! **Input delay.** The send frontier is the first tick not yet sent. On each
//! [`Match::update`], while `frontier <= next_tick + delay`, the client sends
//! the frontier's turn, carrying every queued local command in the first
//! unsent turn. Each client picks its own delay from measured round trips and
//! may change it at any moment: turns are per player, so nothing needs
//! agreeing.
//!
//! **Checksums.** The caller records a checksum every [`CHECKSUM_EVERY`]
//! ticks; it rides on the next outgoing turn and receivers compare it with
//! their own for that tick.
//!
//! **Resync.** On a mismatch the coordinator (the lowest live human seat)
//! stops sending turns at its frontier `R`, broadcasts `ResyncBegin`, runs up
//! to `R`, asks the caller for a snapshot, and sends it in chunks before its
//! turn `R`. Nobody can run tick `R` before that turn, so every other client
//! is still at or before `R`; each loads the snapshot at `R` and continues.
//!
//! **Drops.** A human whose turn for the next tick has been missing for
//! [`WAITING_MS`] is shown as waited on; after [`DROP_AFTER_MS`], or at once
//! on a transport disconnect, it is suspected. The coordinator freezes the
//! suspect's turns, asks every other live human for the turns it holds from
//! the suspect (each freezes too), picks `D = highest held + 1`, and
//! broadcasts `DropCommit` with the turns some clients lacked. Every client
//! then runs ticks below `D` with the same turns and lists the seat in
//! [`TickInput::dropped`] at tick `D` exactly once. If the coordinator is the
//! one who left, the next lowest live human becomes coordinator.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::lobby::MatchStart;
use super::rtt::RttWindow;
use super::wire::{HeldTurn, Msg, Outgoing, PeerId, RefuseReason, Seat};

/// The caller records a checksum every this many ticks.
pub const CHECKSUM_EVERY: u32 = 20;
/// The most escaped snapshot bytes in one chunk; with the envelope a chunk
/// stays under [`super::MAX_MESSAGE_BYTES`].
pub const SNAPSHOT_CHUNK_BYTES: usize = 15_000;
/// A missing turn this old makes its seat "waited on".
pub const WAITING_MS: i64 = 2_000;
/// A missing turn this old makes its seat suspected and dropped.
pub const DROP_AFTER_MS: i64 = 15_000;
const START_RETRY_MS: i64 = 300;
const PING_EVERY_MS: i64 = 1_000;
const MIN_DELAY: u32 = 2;
/// 12 ticks is 600 ms at 20 ticks per second (Normal), as 6 was at 10.
const MAX_DELAY: u32 = 12;
/// Turns at or after `next_tick - REPORT_WINDOW` go in a drop report.
const REPORT_WINDOW: u32 = 32;
/// Turns older than `next_tick - KEEP_TURNS` are forgotten.
const KEEP_TURNS: u32 = 64;
/// Checksums older than this many ticks are forgotten.
const KEEP_SUMS: u32 = 400;

/// The inputs for one tick, for the caller's simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickInput {
    pub tick: u32,
    /// Sorted by seat, then in the order the seat queued them.
    pub commands: Vec<(u8, String)>,
    /// Seats dropped from this tick on; the caller hands them to the AI.
    pub dropped: Vec<u8>,
}

/// What the match is doing, for the caller to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchStatus {
    Running,
    /// These seats' turns have been missing for [`WAITING_MS`] or more.
    WaitingOn(Vec<u8>),
    Resyncing,
    Ended(String),
}

#[derive(Clone, Debug)]
struct Player {
    peer: Option<PeerId>,
    turns: BTreeMap<u32, Vec<String>>,
    /// The highest tick held; turns arrive in order, so all below are held.
    held: Option<u32>,
    last_turn_ms: i64,
    suspected: bool,
    frozen: bool,
    dropped_from: Option<u32>,
}

impl Player {
    fn has(&self, tick: u32) -> bool {
        self.held.is_some_and(|h| h >= tick)
    }

    fn live(&self) -> bool {
        self.dropped_from.is_none()
    }
}

#[derive(Clone, Debug)]
enum Resync {
    Leading {
        tick: u32,
    },
    Following {
        epoch: u32,
        tick: u32,
        leader: u8,
        chunks: Vec<Option<String>>,
        complete: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct DropRound {
    round: u32,
    seat: u8,
    waiting: BTreeSet<u8>,
    held: Vec<Option<u32>>,
    turns: BTreeMap<u32, Vec<String>>,
}

/// One client's lockstep match.
#[derive(Clone, Debug)]
pub struct Match {
    local_seat: u8,
    client_id: String,
    seed: u64,
    ms_per_tick: u32,
    settings: String,
    seats: Vec<Seat>,
    players: BTreeMap<u8, Player>,
    peer_seats: BTreeMap<PeerId, u8>,
    rtt: BTreeMap<PeerId, RttWindow>,
    next_tick: u32,
    send_frontier: u32,
    delay: u32,
    delay_override: Option<u32>,
    queued: Vec<String>,
    local_sums: BTreeMap<u32, u64>,
    remote_sums: BTreeMap<u32, Vec<(u8, u64)>>,
    unsent_sums: VecDeque<(u32, u64)>,
    checksum_floor: u32,
    reported_upto: Option<u32>,
    resync: Option<Resync>,
    epoch: u32,
    drop_round: Option<DropRound>,
    round_counter: u32,
    start_resend: Option<(String, BTreeMap<u8, PeerId>)>,
    last_start_ms: i64,
    last_ping_ms: i64,
    now_ms: i64,
    ended: Option<String>,
    desyncs: u32,
    resyncs: u32,
    outbox: Vec<Outgoing>,
    /// The lobby round this match began in (see the lobby's Rematch notes).
    round: u32,
    /// Peers that have sent a ping of this round; after a rematch, match
    /// messages from any other peer are the old match's.
    heard: BTreeSet<PeerId>,
}

impl Match {
    /// Begins the match at tick 0, replaying match messages that arrived
    /// before the Start, and returns the replies. The first turns go out on
    /// the first [`Match::update`], so the caller can set a delay first.
    pub fn new(start: MatchStart, now_ms: i64) -> (Match, Vec<Outgoing>) {
        let mut players = BTreeMap::new();
        for (i, seat) in start.seats.iter().enumerate() {
            if seat.client_id().is_some() {
                let peer = start
                    .peer_seats
                    .iter()
                    .find(|(_, s)| **s as usize == i)
                    .map(|(p, _)| p.clone());
                players.insert(
                    i as u8,
                    Player {
                        peer,
                        turns: BTreeMap::new(),
                        held: None,
                        last_turn_ms: now_ms,
                        suspected: false,
                        frozen: false,
                        dropped_from: None,
                    },
                );
            }
        }
        let mut game = Match {
            local_seat: start.local_seat,
            client_id: start.client_id,
            seed: start.seed,
            ms_per_tick: start.ms_per_tick.max(1),
            settings: start.settings,
            seats: start.seats,
            players,
            peer_seats: start.peer_seats,
            rtt: start.rtt,
            next_tick: 0,
            send_frontier: 0,
            delay: MIN_DELAY,
            delay_override: None,
            queued: Vec::new(),
            local_sums: BTreeMap::new(),
            remote_sums: BTreeMap::new(),
            unsent_sums: VecDeque::new(),
            checksum_floor: 0,
            reported_upto: None,
            resync: None,
            epoch: 0,
            drop_round: None,
            round_counter: 0,
            start_resend: start.start_resend,
            last_start_ms: now_ms,
            last_ping_ms: now_ms,
            now_ms,
            ended: None,
            desyncs: 0,
            resyncs: 0,
            outbox: Vec::new(),
            round: start.round,
            heard: start.heard,
        };
        for (peer, text) in start.early {
            let out = game.receive(&peer, &text, now_ms);
            game.outbox.extend(out);
        }
        let out = std::mem::take(&mut game.outbox);
        (game, out)
    }

    // ---------------------------------------------------------------- queries

    /// This client's seat.
    pub fn local_seat(&self) -> u8 {
        self.local_seat
    }

    /// The match seed from the Start.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Milliseconds per tick from the Start.
    pub fn ms_per_tick(&self) -> u32 {
        self.ms_per_tick
    }

    /// The opaque match settings from the Start.
    pub fn settings(&self) -> &str {
        &self.settings
    }

    /// The seats from the Start.
    pub fn seats(&self) -> &[Seat] {
        &self.seats
    }

    /// The next tick [`Match::take_tick`] will give.
    pub fn next_tick(&self) -> u32 {
        self.next_tick
    }

    /// The input delay in ticks this client is using now.
    pub fn delay(&self) -> u32 {
        self.delay
    }

    /// The lobby round this match began in: 0 for a first lobby, one more
    /// for each Rematch.
    pub fn round(&self) -> u32 {
        self.round
    }

    /// Which peer plays which seat, as far as this client knows.
    pub fn peer_seats(&self) -> &BTreeMap<PeerId, u8> {
        &self.peer_seats
    }

    /// A human seat whose player is still here: not dropped, not suspected.
    /// The local seat is always here.
    pub fn present(&self, seat: u8) -> bool {
        seat == self.local_seat
            || self
                .players
                .get(&seat)
                .is_some_and(|p| p.live() && !p.suspected)
    }

    /// True when `text` from `peer` is the host's rematch roster: the host
    /// (seat 0) has gone back to the lobby, and this joiner should follow
    /// with [`super::Lobby::rejoin`].
    pub fn is_rematch(&self, peer: &PeerId, text: &str) -> bool {
        self.local_seat != 0
            && self.peer_seats.get(peer) == Some(&0)
            && matches!(Msg::decode(text), Some(Msg::Roster { .. }))
    }

    /// The lowest live human seat, which runs resyncs and drops.
    pub fn coordinator(&self) -> Option<u8> {
        self.players
            .iter()
            .find(|(s, p)| p.live() && (!p.suspected || **s == self.local_seat))
            .map(|(s, _)| *s)
    }

    /// Seats dropped so far, with the tick each was dropped from.
    pub fn dropped(&self) -> Vec<(u8, u32)> {
        self.players
            .iter()
            .filter_map(|(s, p)| p.dropped_from.map(|d| (*s, d)))
            .collect()
    }

    /// The highest tick held from a human seat's turns.
    pub fn held(&self, seat: u8) -> Option<u32> {
        self.players.get(&seat).and_then(|p| p.held)
    }

    /// How many checksum mismatches this client has seen.
    pub fn desyncs_detected(&self) -> u32 {
        self.desyncs
    }

    /// How many resyncs this client has completed.
    pub fn resyncs_done(&self) -> u32 {
        self.resyncs
    }

    /// The 90th-percentile round trip to a peer, in milliseconds.
    pub fn rtt_p90(&self, peer: &PeerId) -> Option<i64> {
        self.rtt.get(peer).and_then(|w| w.p90())
    }

    /// What the match is doing, as of the last call that passed `now_ms`.
    pub fn status(&self) -> MatchStatus {
        if let Some(reason) = &self.ended {
            return MatchStatus::Ended(reason.clone());
        }
        if self.resync.is_some() {
            return MatchStatus::Resyncing;
        }
        let waiting: Vec<u8> = self
            .players
            .iter()
            .filter(|(s, p)| {
                **s != self.local_seat
                    && p.live()
                    && !p.has(self.next_tick)
                    && self.now_ms - p.last_turn_ms >= WAITING_MS
            })
            .map(|(s, _)| *s)
            .collect();
        if waiting.is_empty() {
            MatchStatus::Running
        } else {
            MatchStatus::WaitingOn(waiting)
        }
    }

    // ---------------------------------------------------------------- inputs

    /// Queues a local command; it goes into the first unsent turn.
    pub fn queue_command(&mut self, command: String) {
        if self.ended.is_none() {
            self.queued.push(command);
        }
    }

    /// Fixes the input delay (clamped to 2..=6), or `None` to choose it from
    /// round trips again.
    pub fn set_delay_override(&mut self, delay: Option<u32>) {
        self.delay_override = delay.map(|d| d.clamp(MIN_DELAY, MAX_DELAY));
    }

    /// Records this client's checksum for a tick it has run. It rides on the
    /// next outgoing turn and is compared with the others' for that tick.
    pub fn record_checksum(&mut self, tick: u32, checksum: u64) {
        if tick < self.checksum_floor {
            return;
        }
        self.local_sums.insert(tick, checksum);
        self.unsent_sums.push_back((tick, checksum));
        let differs = self
            .remote_sums
            .get(&tick)
            .is_some_and(|v| v.iter().any(|(_, c)| *c != checksum));
        if differs {
            self.mismatch(tick);
        }
    }

    /// The transport lost a peer: its seat is suspected at once.
    pub fn peer_disconnected(&mut self, peer: &PeerId) {
        self.rtt.remove(peer);
        if let Some(seat) = self.peer_seats.get(peer).copied() {
            if seat != self.local_seat {
                if let Some(p) = self.players.get_mut(&seat) {
                    if p.live() {
                        p.suspected = true;
                    }
                }
            }
        }
    }

    /// Sends due turns, repeats the Start, pings, and runs timeouts, drops
    /// and the delay choice.
    pub fn update(&mut self, now_ms: i64) -> Vec<Outgoing> {
        self.now_ms = now_ms;
        if self.ended.is_some() {
            return std::mem::take(&mut self.outbox);
        }
        self.repeat_start(now_ms);
        self.run_timeouts(now_ms);
        self.run_drop_round();
        self.delay = self.choose_delay();
        self.send_due_turns();
        if now_ms - self.last_ping_ms >= PING_EVERY_MS {
            self.last_ping_ms = now_ms;
            self.outbox.push(Outgoing::to_all(&Msg::Ping {
                client_id: self.client_id.clone(),
                stamp: now_ms,
                round: self.round,
            }));
        }
        std::mem::take(&mut self.outbox)
    }

    /// The inputs for `tick` once every live human's turn for it is here.
    /// Ticks are given in order from 0; any other tick gives `None`.
    pub fn take_tick(&mut self, tick: u32) -> Option<TickInput> {
        if self.ended.is_some() || tick != self.next_tick {
            return None;
        }
        if let Some(Resync::Following { tick: r, .. }) = &self.resync {
            if tick >= *r {
                return None;
            }
        }
        let mut commands = Vec::new();
        let mut dropped = Vec::new();
        for (seat, p) in &self.players {
            if let Some(d) = p.dropped_from {
                if tick >= d {
                    if tick == d {
                        dropped.push(*seat);
                    }
                    continue;
                }
            }
            let cmds = p.turns.get(&tick)?;
            commands.extend(cmds.iter().map(|c| (*seat, c.clone())));
        }
        self.next_tick += 1;
        let keep_from = tick.saturating_sub(KEEP_TURNS);
        for p in self.players.values_mut() {
            p.turns = p.turns.split_off(&keep_from);
        }
        let sums_from = tick.saturating_sub(KEEP_SUMS);
        self.local_sums = self.local_sums.split_off(&sums_from);
        self.remote_sums = self.remote_sums.split_off(&sums_from);
        Some(TickInput {
            tick,
            commands,
            dropped,
        })
    }

    /// Coordinator during a resync: the tick whose start state the caller
    /// must give to [`Match::provide_snapshot`] (the state after running every
    /// tick below it).
    pub fn wants_snapshot(&self) -> Option<u32> {
        match self.resync {
            Some(Resync::Leading { tick }) if self.next_tick == tick => Some(tick),
            _ => None,
        }
    }

    /// Coordinator: sends the snapshot for the tick [`Match::wants_snapshot`]
    /// named, then resumes sending turns.
    pub fn provide_snapshot(&mut self, tick: u32, snapshot: &str) -> Vec<Outgoing> {
        if self.wants_snapshot() != Some(tick) {
            return Vec::new();
        }
        let chunks = split_snapshot(snapshot);
        let count = chunks.len() as u32;
        for (index, data) in chunks.into_iter().enumerate() {
            self.outbox.push(Outgoing::to_all(&Msg::SnapshotChunk {
                epoch: self.epoch,
                tick,
                index: index as u32,
                count,
                data,
            }));
        }
        self.resync = None;
        self.resyncs += 1;
        self.send_due_turns();
        std::mem::take(&mut self.outbox)
    }

    /// Other clients during a resync: the coordinator's snapshot, once it has
    /// arrived and this client has reached its tick. Load it, then continue
    /// with [`Match::take_tick`] from that tick.
    pub fn take_snapshot(&mut self) -> Option<(u32, String)> {
        let Some(Resync::Following { tick, complete, .. }) = &mut self.resync else {
            return None;
        };
        if self.next_tick != *tick || complete.is_none() {
            return None;
        }
        let tick = *tick;
        let data = complete.take().unwrap_or_default();
        self.resync = None;
        self.resyncs += 1;
        Some((tick, data))
    }

    /// Handles one message from a peer.
    pub fn receive(&mut self, peer: &PeerId, text: &str, now_ms: i64) -> Vec<Outgoing> {
        self.now_ms = now_ms;
        let Some(msg) = Msg::decode(text) else {
            return Vec::new();
        };
        if let Msg::Ping { round, .. } = &msg {
            if *round == self.round {
                self.heard.insert(peer.clone());
            }
        }
        if self.round > 0 && msg.is_match_kind() && !self.heard.contains(peer) {
            return Vec::new();
        }
        match msg {
            Msg::Ping { stamp, .. } => {
                self.outbox
                    .push(Outgoing::to_peer(peer, &Msg::Pong { stamp }));
            }
            Msg::Pong { stamp } => {
                self.rtt
                    .entry(peer.clone())
                    .or_default()
                    .push(now_ms - stamp);
            }
            Msg::Hello { .. } => {
                let reason = RefuseReason::AlreadyStarted;
                self.outbox
                    .push(Outgoing::to_peer(peer, &Msg::Refuse { reason }));
            }
            Msg::Start { .. } => {
                self.outbox.push(Outgoing::to_peer(peer, &Msg::StartAck));
            }
            Msg::StartAck => {
                if let Some(seat) = self.peer_seats.get(peer).copied() {
                    self.start_acknowledged(seat);
                }
            }
            Msg::Turn {
                seat,
                tick,
                commands,
                checksum,
            } => self.receive_turn(peer, seat, tick, commands, checksum, now_ms),
            Msg::DesyncReport { tick } => {
                if self.coordinator() == Some(self.local_seat) {
                    self.begin_resync(tick);
                }
            }
            Msg::ResyncBegin { epoch, tick } => self.follow_resync(peer, epoch, tick),
            Msg::SnapshotChunk {
                epoch,
                tick,
                index,
                count,
                data,
            } => self.receive_chunk(epoch, tick, index, count, data),
            Msg::DropQuery { round, seat } => self.answer_drop_query(peer, round, seat),
            Msg::DropReport {
                round,
                seat,
                held,
                turns,
            } => self.receive_drop_report(peer, round, seat, held, turns),
            Msg::DropCommit {
                seat,
                from_tick,
                turns,
            } => self.apply_drop(seat, from_tick, turns),
            _ => {}
        }
        std::mem::take(&mut self.outbox)
    }

    // ---------------------------------------------------------------- turns

    fn choose_delay(&self) -> u32 {
        if let Some(d) = self.delay_override {
            return d;
        }
        let worst = self
            .rtt
            .iter()
            .filter(|(peer, _)| {
                self.peer_seats
                    .get(*peer)
                    .and_then(|s| self.players.get(s))
                    .is_some_and(|p| p.live() && !p.suspected)
            })
            .filter_map(|(_, w)| w.p90())
            .max();
        match worst {
            None => self.delay,
            Some(rtt) => {
                let ticks = rtt.max(0) as u64;
                let per = self.ms_per_tick as u64;
                let d = ticks.div_ceil(per) + 1;
                (d.min(MAX_DELAY as u64) as u32).clamp(MIN_DELAY, MAX_DELAY)
            }
        }
    }

    fn send_due_turns(&mut self) {
        while self.send_frontier <= self.next_tick + self.delay {
            if let Some(Resync::Leading { tick }) = self.resync {
                if self.send_frontier >= tick {
                    break;
                }
            }
            let tick = self.send_frontier;
            let commands = std::mem::take(&mut self.queued);
            let mut checksum = None;
            while let Some((t, c)) = self.unsent_sums.pop_front() {
                if t >= self.checksum_floor {
                    checksum = Some((t, c));
                    break;
                }
            }
            if let Some(me) = self.players.get_mut(&self.local_seat) {
                me.turns.insert(tick, commands.clone());
                me.held = Some(tick);
            }
            self.outbox.push(Outgoing::to_all(&Msg::Turn {
                seat: self.local_seat,
                tick,
                commands,
                checksum,
            }));
            self.send_frontier += 1;
        }
    }

    fn receive_turn(
        &mut self,
        peer: &PeerId,
        seat: u8,
        tick: u32,
        commands: Vec<String>,
        checksum: Option<(u32, u64)>,
        now_ms: i64,
    ) {
        if seat == self.local_seat {
            return;
        }
        let Some(p) = self.players.get_mut(&seat) else {
            return;
        };
        self.peer_seats.insert(peer.clone(), seat);
        p.peer = Some(peer.clone());
        if p.frozen || !p.live() {
            return;
        }
        let expected = p.held.map_or(0, |h| h + 1);
        if tick != expected {
            return;
        }
        p.turns.insert(tick, commands);
        p.held = Some(tick);
        p.last_turn_ms = now_ms;
        self.start_acknowledged(seat);
        if let Some((t, c)) = checksum {
            self.remote_checksum(seat, t, c);
        }
    }

    fn repeat_start(&mut self, now_ms: i64) {
        if let Some((text, pending)) = &self.start_resend {
            if !pending.is_empty() && now_ms - self.last_start_ms >= START_RETRY_MS {
                self.last_start_ms = now_ms;
                for peer in pending.values() {
                    self.outbox.push(Outgoing {
                        to: super::Target::Peer(peer.clone()),
                        text: text.clone(),
                    });
                }
            }
        }
    }

    fn start_acknowledged(&mut self, seat: u8) {
        if let Some((_, pending)) = &mut self.start_resend {
            pending.remove(&seat);
        }
    }

    // ---------------------------------------------------------------- checksums and resync

    fn remote_checksum(&mut self, seat: u8, tick: u32, checksum: u64) {
        if tick < self.checksum_floor {
            return;
        }
        self.remote_sums
            .entry(tick)
            .or_default()
            .push((seat, checksum));
        if self.local_sums.get(&tick).is_some_and(|c| *c != checksum) {
            self.mismatch(tick);
        }
    }

    fn mismatch(&mut self, tick: u32) {
        self.desyncs += 1;
        if self.coordinator() == Some(self.local_seat) {
            self.begin_resync(tick);
            return;
        }
        if self.reported_upto.is_some_and(|r| r >= tick) {
            return;
        }
        self.reported_upto = Some(tick);
        let peer = self
            .coordinator()
            .and_then(|c| self.players.get(&c))
            .and_then(|p| p.peer.clone());
        if let Some(peer) = peer {
            self.outbox
                .push(Outgoing::to_peer(&peer, &Msg::DesyncReport { tick }));
        }
    }

    fn raise_floor(&mut self, tick: u32) {
        self.checksum_floor = self.checksum_floor.max(tick);
        let floor = self.checksum_floor;
        self.local_sums = self.local_sums.split_off(&floor);
        self.remote_sums = self.remote_sums.split_off(&floor);
        self.unsent_sums.retain(|(t, _)| *t >= floor);
    }

    fn begin_resync(&mut self, bad_tick: u32) {
        if bad_tick < self.checksum_floor || self.resync.is_some() || self.ended.is_some() {
            return;
        }
        self.epoch += 1;
        let tick = self.send_frontier;
        self.resync = Some(Resync::Leading { tick });
        self.raise_floor(tick);
        self.outbox.push(Outgoing::to_all(&Msg::ResyncBegin {
            epoch: self.epoch,
            tick,
        }));
    }

    fn follow_resync(&mut self, peer: &PeerId, epoch: u32, tick: u32) {
        let Some(leader) = self.peer_seats.get(peer).copied() else {
            return;
        };
        if tick < self.next_tick {
            return;
        }
        self.epoch = epoch;
        self.resync = Some(Resync::Following {
            epoch,
            tick,
            leader,
            chunks: Vec::new(),
            complete: None,
        });
        self.raise_floor(tick);
    }

    fn receive_chunk(&mut self, epoch: u32, tick: u32, index: u32, count: u32, data: String) {
        let Some(Resync::Following {
            epoch: e,
            tick: t,
            chunks,
            complete,
            ..
        }) = &mut self.resync
        else {
            return;
        };
        if *e != epoch || *t != tick || count == 0 || index >= count {
            return;
        }
        if chunks.len() != count as usize {
            *chunks = vec![None; count as usize];
        }
        chunks[index as usize] = Some(data);
        if chunks.iter().all(|c| c.is_some()) {
            let whole: String = chunks.iter().flatten().map(|s| s.as_str()).collect();
            *complete = Some(whole);
        }
    }

    // ---------------------------------------------------------------- drops

    fn run_timeouts(&mut self, now_ms: i64) {
        let leader = match &self.resync {
            Some(Resync::Following { leader, .. }) => Some(*leader),
            _ => None,
        };
        let next = self.next_tick;
        for (seat, p) in self.players.iter_mut() {
            if *seat == self.local_seat || !p.live() || p.suspected || Some(*seat) == leader {
                continue;
            }
            if !p.has(next) && now_ms - p.last_turn_ms >= DROP_AFTER_MS {
                p.suspected = true;
            }
        }
    }

    fn report_turns(&self, seat: u8) -> (Option<u32>, Vec<HeldTurn>) {
        let from = self.next_tick.saturating_sub(REPORT_WINDOW);
        match self.players.get(&seat) {
            None => (None, Vec::new()),
            Some(p) => (
                p.held,
                p.turns
                    .range(from..)
                    .map(|(t, c)| (*t, c.clone()))
                    .collect(),
            ),
        }
    }

    fn run_drop_round(&mut self) {
        if self.coordinator() != Some(self.local_seat) {
            return;
        }
        if let Some(r) = &self.drop_round {
            let target_gone = self.players.get(&r.seat).is_none_or(|p| !p.live());
            let helper_gone = r
                .waiting
                .iter()
                .any(|s| self.players.get(s).is_none_or(|p| !p.live() || p.suspected));
            if !target_gone && !helper_gone {
                return;
            }
            self.drop_round = None;
        }
        let target = self
            .players
            .iter()
            .find(|(s, p)| **s != self.local_seat && p.live() && p.suspected)
            .map(|(s, _)| *s);
        let Some(seat) = target else {
            return;
        };
        if let Some(p) = self.players.get_mut(&seat) {
            p.frozen = true;
        }
        self.round_counter += 1;
        let round = self.round_counter * 8 + self.local_seat as u32;
        let waiting: BTreeSet<u8> = self
            .players
            .iter()
            .filter(|(s, p)| **s != self.local_seat && **s != seat && p.live() && !p.suspected)
            .map(|(s, _)| *s)
            .collect();
        let (held, turns) = self.report_turns(seat);
        self.drop_round = Some(DropRound {
            round,
            seat,
            waiting,
            held: vec![held],
            turns: turns.into_iter().collect(),
        });
        self.outbox
            .push(Outgoing::to_all(&Msg::DropQuery { round, seat }));
        self.maybe_commit_drop();
    }

    fn answer_drop_query(&mut self, peer: &PeerId, round: u32, seat: u8) {
        if seat == self.local_seat {
            return;
        }
        let Some(p) = self.players.get_mut(&seat) else {
            return;
        };
        p.suspected = true;
        p.frozen = true;
        let (held, turns) = self.report_turns(seat);
        self.outbox.push(Outgoing::to_peer(
            peer,
            &Msg::DropReport {
                round,
                seat,
                held,
                turns,
            },
        ));
    }

    fn receive_drop_report(
        &mut self,
        peer: &PeerId,
        round: u32,
        seat: u8,
        held: Option<u32>,
        turns: Vec<HeldTurn>,
    ) {
        let Some(from) = self.peer_seats.get(peer).copied() else {
            return;
        };
        let Some(r) = &mut self.drop_round else {
            return;
        };
        if r.round != round || r.seat != seat || !r.waiting.remove(&from) {
            return;
        }
        r.held.push(held);
        for (t, c) in turns {
            r.turns.entry(t).or_insert(c);
        }
        self.maybe_commit_drop();
    }

    fn maybe_commit_drop(&mut self) {
        let ready = self
            .drop_round
            .as_ref()
            .is_some_and(|r| r.waiting.is_empty());
        if !ready {
            return;
        }
        let Some(r) = self.drop_round.take() else {
            return;
        };
        let from_tick = r.held.iter().flatten().max().map_or(0, |h| h + 1);
        let turns: Vec<HeldTurn> = r
            .turns
            .into_iter()
            .filter(|(t, _)| *t < from_tick)
            .collect();
        self.outbox.push(Outgoing::to_all(&Msg::DropCommit {
            seat: r.seat,
            from_tick,
            turns: turns.clone(),
        }));
        self.apply_drop(r.seat, from_tick, turns);
    }

    fn apply_drop(&mut self, seat: u8, from_tick: u32, turns: Vec<HeldTurn>) {
        if seat == self.local_seat {
            self.ended = Some("dropped by the other players".to_string());
            return;
        }
        let Some(p) = self.players.get_mut(&seat) else {
            return;
        };
        if !p.live() {
            return;
        }
        for (t, c) in turns {
            if t < from_tick && p.held.is_none_or(|h| t > h) {
                p.turns.insert(t, c);
            }
        }
        if from_tick > 0 {
            p.held = Some(p.held.map_or(from_tick - 1, |h| h.max(from_tick - 1)));
        }
        p.dropped_from = Some(from_tick);
        p.suspected = true;
        p.frozen = true;
        self.start_acknowledged(seat);
        if self.drop_round.as_ref().is_some_and(|r| r.seat == seat) {
            self.drop_round = None;
        }
        if let Some(Resync::Following { leader, .. }) = &self.resync {
            if *leader == seat {
                self.resync = None;
            }
        }
    }
}

/// Splits a snapshot so each chunk's JSON-escaped form is at most
/// [`SNAPSHOT_CHUNK_BYTES`].
fn split_snapshot(data: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut cost = 0;
    for c in data.chars() {
        let width = match c {
            '"' | '\\' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        };
        if cost + width > SNAPSHOT_CHUNK_BYTES && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            cost = 0;
        }
        current.push(c);
        cost += width;
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}
