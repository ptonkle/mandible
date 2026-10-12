//! The host-authoritative lobby.
//!
//! The host owns the seats and the settings and broadcasts the roster when
//! they change and once a second (lobby messages may be lost). A joiner
//! repeats its Hello until it is seated or refused, and repeats a team
//! request until the roster shows it. Every peer pings every connected peer.
//! When the host starts, each seated human gets a Start, repeated by the
//! host's [`super::Match`] until acknowledged.
//!
//! **Ready Up.** A joiner marks its seat ready (and may undo it), repeated
//! until the roster shows it. The host may start only when every joined human
//! is ready; a change to the match settings clears every mark.
//!
//! **Rematch.** After a match the host builds a new lobby from it
//! ([`Lobby::rematch`]): the same seats and settings, every mark cleared, and
//! a seat whose player has gone made Open. Its roster reaches each joiner while
//! it is still in the old match ([`super::Match::is_rematch`]), and the joiner
//! comes back with [`Lobby::rejoin`]. Each Rematch starts a new round. Old
//! messages (old-lobby pings, old-match turns) may still be in flight, and a
//! peer may not even be known yet, so after a rematch a peer's match messages
//! are kept only once a ping of the new round has come from it: on an
//! ordered channel that ping follows everything the peer sent before.

use std::collections::{BTreeMap, BTreeSet};

use super::game::Match;
use super::rtt::RttWindow;
use super::wire::{Msg, Outgoing, PeerId, RefuseReason, Seat, SeatKind, Version};

/// How often a joiner repeats its Hello or team request, in milliseconds.
const RETRY_MS: i64 = 500;
/// How often the host rebroadcasts the roster.
const ROSTER_EVERY_MS: i64 = 1000;
/// How often every peer pings every connected peer.
const PING_EVERY_MS: i64 = 500;
/// The most seats a lobby has.
const MAX_SEATS: usize = 4;
/// The longest player name, in characters.
pub const MAX_NAME_CHARS: usize = 16;

/// A player name as the lobby shows it: printable, trimmed, at most
/// [`MAX_NAME_CHARS`] characters. Empty means "no name given".
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// What the lobby is doing, for the caller to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyStatus {
    /// A joiner waiting for its seat.
    Joining,
    /// Seated in this seat.
    Seated(u8),
    /// The host refused this joiner.
    Refused(RefuseReason),
    /// The host left before the start; the lobby is gone.
    Dissolved,
    /// A joiner's link to the host closed without the host leaving.
    Lost,
    /// This client left.
    Left,
    /// The match has started; [`Lobby::take_start`] gives what the match needs.
    Started,
}

/// Why a lobby action was not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyError {
    /// Only the host may do this.
    NotHost,
    /// No seat has this index.
    NoSuchSeat,
    /// The host's own seat cannot be changed.
    OwnSeat,
    /// A human sits there; humans join by Hello and leave by themselves.
    SeatTaken,
    /// Humans cannot be placed by the host.
    NotAllowed,
    /// The lobby is not open (not seated, or already started).
    NotOpen,
    /// A joined human has not pressed Ready Up.
    NotReady,
    /// No team 0–3 (Team 1–4).
    NoSuchTeam,
    /// The seats that are played make fewer than two teams (TRIAGE row 81).
    OneTeam,
}

/// Everything a [`super::Match`] needs, produced when the match starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchStart {
    pub local_seat: u8,
    pub client_id: String,
    pub seed: u64,
    pub ms_per_tick: u32,
    pub settings: String,
    pub seats: Vec<Seat>,
    /// Which peer plays which seat, as far as this client knows.
    pub peer_seats: BTreeMap<PeerId, u8>,
    /// Round-trip samples measured in the lobby.
    pub rtt: BTreeMap<PeerId, RttWindow>,
    /// Match messages that arrived before this client's Start.
    pub early: Vec<(PeerId, String)>,
    /// Host only: the Start text and the seats still to acknowledge it.
    pub start_resend: Option<(String, BTreeMap<u8, PeerId>)>,
    /// The round (0 for a first lobby, one more for each Rematch).
    pub round: u32,
    /// Peers that have sent a ping of this round. After a rematch, match
    /// messages from any other peer are old and are thrown away.
    pub heard: BTreeSet<PeerId>,
}

#[derive(Clone, Debug, Default)]
struct LobbyPeer {
    client_id: Option<String>,
    rtt: RttWindow,
}

/// One client's view of the lobby; the host's view is the authority.
#[derive(Clone, Debug)]
pub struct Lobby {
    version: Version,
    client_id: String,
    name: String,
    /// `None` on the host.
    host_peer: Option<PeerId>,
    seats: Vec<Seat>,
    settings: String,
    roster_version: u32,
    peers: BTreeMap<PeerId, LobbyPeer>,
    status: LobbyStatus,
    last_hello_ms: i64,
    last_roster_ms: i64,
    last_ping_ms: i64,
    last_team_ms: i64,
    wanted_team: Option<u8>,
    last_ready_ms: i64,
    wanted_ready: Option<bool>,
    start: Option<MatchStart>,
    early: Vec<(PeerId, String)>,
    round: u32,
    heard: BTreeSet<PeerId>,
}

impl Lobby {
    fn blank(version: Version, client_id: &str, name: &str, now_ms: i64) -> Lobby {
        Lobby {
            version,
            client_id: client_id.to_string(),
            name: name.to_string(),
            host_peer: None,
            seats: Vec::new(),
            settings: String::new(),
            roster_version: 0,
            peers: BTreeMap::new(),
            status: LobbyStatus::Joining,
            last_hello_ms: now_ms,
            last_roster_ms: now_ms,
            last_ping_ms: now_ms - PING_EVERY_MS,
            last_team_ms: now_ms,
            wanted_team: None,
            last_ready_ms: now_ms,
            wanted_ready: None,
            start: None,
            early: Vec::new(),
            round: 0,
            heard: BTreeSet::new(),
        }
    }

    /// Creates a lobby as its host, seated in seat 0. `seat_count` is clamped
    /// to 1..=4; seats start Open with team = seat index.
    pub fn host(
        version: Version,
        client_id: &str,
        name: &str,
        seat_count: usize,
        settings: &str,
        now_ms: i64,
    ) -> Lobby {
        let mut lobby = Lobby::blank(version, client_id, name, now_ms);
        let count = seat_count.clamp(1, MAX_SEATS);
        lobby.seats = (0..count)
            .map(|i| Seat {
                kind: SeatKind::Open,
                team: i as u8,
            })
            .collect();
        lobby.seats[0].kind = SeatKind::Human {
            client_id: client_id.to_string(),
            name: name.to_string(),
            ready: false,
        };
        lobby.settings = settings.to_string();
        lobby.roster_version = 1;
        lobby.status = LobbyStatus::Seated(0);
        lobby
    }

    /// Joins the lobby hosted by `host`, which the transport has connected.
    pub fn join(
        version: Version,
        host: PeerId,
        client_id: &str,
        name: &str,
        now_ms: i64,
    ) -> (Lobby, Vec<Outgoing>) {
        let mut lobby = Lobby::blank(version, client_id, name, now_ms);
        lobby.peers.insert(host.clone(), LobbyPeer::default());
        lobby.host_peer = Some(host.clone());
        let hello = lobby.hello();
        (lobby, vec![Outgoing::to_peer(&host, &hello)])
    }

    /// Host, after a match: the same seats and settings for a rematch. Every
    /// Ready Up mark is cleared, a human seat whose player has gone becomes
    /// Open, and the roster goes to everyone still connected.
    pub fn rematch(
        version: Version,
        client_id: &str,
        name: &str,
        game: &Match,
        now_ms: i64,
    ) -> (Lobby, Vec<Outgoing>) {
        let mut lobby = Lobby::blank(version, client_id, name, now_ms);
        lobby.settings = game.settings().to_string();
        lobby.seats = game.seats().to_vec();
        for (i, seat) in lobby.seats.iter_mut().enumerate() {
            if let SeatKind::Human { ready, .. } = &mut seat.kind {
                *ready = false;
                if i != 0 && !game.present(i as u8) {
                    seat.kind = SeatKind::Open;
                }
            }
        }
        lobby.seats[0].kind = SeatKind::Human {
            client_id: client_id.to_string(),
            name: name.to_string(),
            ready: false,
        };
        lobby.adopt_peers(game);
        lobby.status = LobbyStatus::Seated(0);
        let out = lobby.roster_changed(now_ms);
        (lobby, out)
    }

    /// Joiner, still in the old match, on the host's rematch roster: joins
    /// the new lobby. The caller passes that roster to [`Lobby::receive`].
    pub fn rejoin(
        version: Version,
        host: PeerId,
        client_id: &str,
        name: &str,
        game: &Match,
        now_ms: i64,
    ) -> (Lobby, Vec<Outgoing>) {
        let (mut lobby, out) = Lobby::join(version, host, client_id, name, now_ms);
        lobby.adopt_peers(game);
        (lobby, out)
    }

    /// Knows the old match's peers, and starts the next round.
    fn adopt_peers(&mut self, game: &Match) {
        self.round = game.round() + 1;
        for (peer, seat) in game.peer_seats() {
            let client_id = game
                .seats()
                .get(*seat as usize)
                .and_then(|s| s.client_id())
                .map(str::to_string);
            self.peers.entry(peer.clone()).or_default().client_id = client_id;
        }
    }

    fn hello(&self) -> Msg {
        Msg::Hello {
            proto: self.version.proto,
            build: self.version.build.clone(),
            client_id: self.client_id.clone(),
            name: self.name.clone(),
        }
    }

    fn roster(&self) -> Msg {
        Msg::Roster {
            version: self.roster_version,
            seats: self.seats.clone(),
            settings: self.settings.clone(),
            round: self.round,
        }
    }

    fn roster_changed(&mut self, now_ms: i64) -> Vec<Outgoing> {
        self.roster_version += 1;
        self.last_roster_ms = now_ms;
        vec![Outgoing::to_all(&self.roster())]
    }

    /// True on the host.
    pub fn is_host(&self) -> bool {
        self.host_peer.is_none()
    }

    /// The seats as this client last saw them.
    pub fn seats(&self) -> &[Seat] {
        &self.seats
    }

    /// The match settings as this client last saw them.
    pub fn settings(&self) -> &str {
        &self.settings
    }

    /// What the lobby is doing.
    pub fn status(&self) -> &LobbyStatus {
        &self.status
    }

    /// True when every joined human has pressed Ready Up (the host's own
    /// seat needs no mark).
    pub fn all_ready(&self) -> bool {
        self.seats
            .iter()
            .enumerate()
            .all(|(i, s)| i == 0 || s.client_id().is_none() || s.ready())
    }

    /// The 90th-percentile round trip to a peer, in milliseconds.
    pub fn rtt_p90(&self, peer: &PeerId) -> Option<i64> {
        self.peers.get(peer).and_then(|p| p.rtt.p90())
    }

    /// Once started: what the match needs. Given once.
    pub fn take_start(&mut self) -> Option<MatchStart> {
        self.start.take()
    }

    fn open(&self) -> bool {
        matches!(self.status, LobbyStatus::Seated(_) | LobbyStatus::Joining)
    }

    fn seat_of_client(&self, client_id: &str) -> Option<usize> {
        self.seats
            .iter()
            .position(|s| s.client_id() == Some(client_id))
    }

    fn seat_of_peer(&self, peer: &PeerId) -> Option<usize> {
        let client_id = self.peers.get(peer)?.client_id.clone()?;
        self.seat_of_client(&client_id)
    }

    /// The transport connected a peer.
    pub fn peer_connected(&mut self, peer: PeerId, _now_ms: i64) -> Vec<Outgoing> {
        self.peers.entry(peer).or_default();
        Vec::new()
    }

    /// The transport lost a peer. A joiner that loses the host sees the
    /// lobby dissolve; the host frees a departed joiner's seat.
    pub fn peer_disconnected(&mut self, peer: &PeerId, now_ms: i64) -> Vec<Outgoing> {
        if self.host_peer.as_ref() == Some(peer) {
            if self.open() {
                self.status = LobbyStatus::Lost;
            }
            return Vec::new();
        }
        let out = self.free_seat_of(peer, now_ms);
        self.peers.remove(peer);
        out
    }

    fn free_seat_of(&mut self, peer: &PeerId, now_ms: i64) -> Vec<Outgoing> {
        if !self.is_host() || self.status == LobbyStatus::Started {
            return Vec::new();
        }
        match self.seat_of_peer(peer) {
            Some(i) if i != 0 => {
                self.seats[i].kind = SeatKind::Open;
                self.roster_changed(now_ms)
            }
            _ => Vec::new(),
        }
    }

    /// Host: puts Open, Closed or an AI in a seat.
    pub fn set_seat(
        &mut self,
        index: usize,
        kind: SeatKind,
        now_ms: i64,
    ) -> Result<Vec<Outgoing>, LobbyError> {
        self.host_check(index)?;
        if matches!(kind, SeatKind::Human { .. }) {
            return Err(LobbyError::NotAllowed);
        }
        if matches!(self.seats[index].kind, SeatKind::Human { .. }) {
            return Err(LobbyError::SeatTaken);
        }
        self.seats[index].kind = kind;
        Ok(self.roster_changed(now_ms))
    }

    /// Host: sets how many seats the lobby has (1..=4), as the format asks.
    /// New seats are Open; a human in a seat that goes moves to the first
    /// seat without a human. Refused when there are more humans than seats.
    pub fn set_seat_count(
        &mut self,
        count: usize,
        now_ms: i64,
    ) -> Result<Vec<Outgoing>, LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if !self.open() {
            return Err(LobbyError::NotOpen);
        }
        let count = count.clamp(1, MAX_SEATS);
        if count == self.seats.len() {
            return Ok(Vec::new());
        }
        let humans = self
            .seats
            .iter()
            .filter(|s| s.client_id().is_some())
            .count();
        if humans > count {
            return Err(LobbyError::SeatTaken);
        }
        while self.seats.len() < count {
            let team = self.seats.len() as u8;
            self.seats.push(Seat {
                kind: SeatKind::Open,
                team,
            });
        }
        let moving: Vec<SeatKind> = self.seats[count..]
            .iter()
            .filter(|s| s.client_id().is_some())
            .map(|s| s.kind.clone())
            .collect();
        self.seats.truncate(count);
        for kind in moving {
            if let Some(free) = self.seats.iter_mut().find(|s| s.client_id().is_none()) {
                free.kind = kind;
            }
        }
        Ok(self.roster_changed(now_ms))
    }

    /// Host: sets any seat's team (0–3), its own included. Every Ready Up
    /// mark is cleared: the match has changed.
    pub fn set_team(
        &mut self,
        index: usize,
        team: u8,
        now_ms: i64,
    ) -> Result<Vec<Outgoing>, LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if !self.open() {
            return Err(LobbyError::NotOpen);
        }
        if team as usize >= MAX_SEATS {
            return Err(LobbyError::NoSuchTeam);
        }
        let seat = self.seats.get_mut(index).ok_or(LobbyError::NoSuchSeat)?;
        seat.team = team;
        self.clear_ready();
        Ok(self.roster_changed(now_ms))
    }

    /// Host: sets every seat's team at once (seat order; seats past the
    /// list keep theirs), for example the format's defaults after a format
    /// change. Every Ready Up mark is cleared.
    pub fn set_teams(&mut self, teams: &[u8], now_ms: i64) -> Result<Vec<Outgoing>, LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if !self.open() {
            return Err(LobbyError::NotOpen);
        }
        if teams.iter().any(|t| *t as usize >= MAX_SEATS) {
            return Err(LobbyError::NoSuchTeam);
        }
        for (seat, team) in self.seats.iter_mut().zip(teams) {
            seat.team = *team;
        }
        self.clear_ready();
        Ok(self.roster_changed(now_ms))
    }

    /// How many teams the seats that are played (every seat but Closed)
    /// make. Start needs at least two.
    pub fn team_count(&self) -> usize {
        crate::settings::team_count(
            self.seats
                .iter()
                .filter(|s| s.kind != SeatKind::Closed)
                .map(|s| s.team),
        )
    }

    fn clear_ready(&mut self) {
        for seat in &mut self.seats {
            if let SeatKind::Human { ready, .. } = &mut seat.kind {
                *ready = false;
            }
        }
    }

    /// Host: replaces the opaque match settings.
    pub fn set_settings(
        &mut self,
        settings: &str,
        now_ms: i64,
    ) -> Result<Vec<Outgoing>, LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if !self.open() {
            return Err(LobbyError::NotOpen);
        }
        self.settings = settings.to_string();
        // New settings: every joiner looks again before Ready Up.
        self.clear_ready();
        Ok(self.roster_changed(now_ms))
    }

    fn host_check(&self, index: usize) -> Result<(), LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if !self.open() {
            return Err(LobbyError::NotOpen);
        }
        if index >= self.seats.len() {
            return Err(LobbyError::NoSuchSeat);
        }
        if index == 0 {
            return Err(LobbyError::OwnSeat);
        }
        Ok(())
    }

    /// Joiner: asks the host for a team for its own seat. Repeated until the
    /// roster shows it.
    pub fn request_team(&mut self, team: u8, now_ms: i64) -> Result<Vec<Outgoing>, LobbyError> {
        let host = self.host_peer.clone().ok_or(LobbyError::NotAllowed)?;
        if !matches!(self.status, LobbyStatus::Seated(_)) {
            return Err(LobbyError::NotOpen);
        }
        self.wanted_team = Some(team);
        self.last_team_ms = now_ms;
        Ok(vec![Outgoing::to_peer(&host, &Msg::TeamRequest { team })])
    }

    /// Joiner: presses Ready Up (`true`) or undoes it. Repeated until the
    /// roster shows it.
    pub fn set_ready(&mut self, ready: bool, now_ms: i64) -> Result<Vec<Outgoing>, LobbyError> {
        let host = self.host_peer.clone().ok_or(LobbyError::NotAllowed)?;
        if !matches!(self.status, LobbyStatus::Seated(_)) {
            return Err(LobbyError::NotOpen);
        }
        self.wanted_ready = Some(ready);
        self.last_ready_ms = now_ms;
        Ok(vec![Outgoing::to_peer(&host, &Msg::Ready { ready })])
    }

    /// This client's own seat has pressed Ready Up, as the roster shows.
    pub fn is_ready(&self) -> bool {
        match self.status {
            LobbyStatus::Seated(i) => self.seats.get(i as usize).is_some_and(|s| s.ready()),
            _ => false,
        }
    }

    /// Ready Up as this joiner last asked for it, before the roster shows it.
    pub fn wants_ready(&self) -> bool {
        self.wanted_ready.unwrap_or_else(|| self.is_ready())
    }

    /// Leaves the lobby. On the host this dissolves it for everyone.
    pub fn leave(&mut self) -> Vec<Outgoing> {
        if !self.open() {
            return Vec::new();
        }
        match self.host_peer.clone() {
            None => {
                self.status = LobbyStatus::Dissolved;
                vec![Outgoing::to_all(&Msg::Dissolve)]
            }
            Some(host) => {
                self.status = LobbyStatus::Left;
                vec![Outgoing::to_peer(&host, &Msg::Leave)]
            }
        }
    }

    /// Host: starts the match. Every seated human gets a Start; the host's
    /// own match begins now. `ms_per_tick` travels in the Start.
    pub fn start(
        &mut self,
        seed: u64,
        ms_per_tick: u32,
        _now_ms: i64,
    ) -> Result<Vec<Outgoing>, LobbyError> {
        if !self.is_host() {
            return Err(LobbyError::NotHost);
        }
        if self.status != LobbyStatus::Seated(0) {
            return Err(LobbyError::NotOpen);
        }
        if !self.all_ready() {
            return Err(LobbyError::NotReady);
        }
        if self.team_count() < 2 {
            return Err(LobbyError::OneTeam);
        }
        let msg = Msg::Start {
            seed,
            ms_per_tick,
            settings: self.settings.clone(),
            seats: self.seats.clone(),
        };
        let text = msg.encode();
        let mut pending = BTreeMap::new();
        let mut peer_seats = BTreeMap::new();
        for (peer, info) in &self.peers {
            if let Some(i) = info
                .client_id
                .as_deref()
                .and_then(|c| self.seat_of_client(c))
            {
                if i != 0 {
                    pending.insert(i as u8, peer.clone());
                    peer_seats.insert(peer.clone(), i as u8);
                }
            }
        }
        let out = pending
            .values()
            .map(|peer| Outgoing::to_peer(peer, &msg))
            .collect();
        self.start = Some(MatchStart {
            local_seat: 0,
            client_id: self.client_id.clone(),
            seed,
            ms_per_tick,
            settings: self.settings.clone(),
            seats: self.seats.clone(),
            peer_seats,
            rtt: self.rtts(),
            early: std::mem::take(&mut self.early),
            start_resend: Some((text, pending)),
            round: self.round,
            heard: self.heard.clone(),
        });
        self.status = LobbyStatus::Started;
        Ok(out)
    }

    fn rtts(&self) -> BTreeMap<PeerId, RttWindow> {
        self.peers
            .iter()
            .map(|(p, info)| (p.clone(), info.rtt.clone()))
            .collect()
    }

    /// Repeats what may have been lost, and pings.
    pub fn update(&mut self, now_ms: i64) -> Vec<Outgoing> {
        let mut out = Vec::new();
        if !self.open() {
            return out;
        }
        match self.host_peer.clone() {
            None => {
                if now_ms - self.last_roster_ms >= ROSTER_EVERY_MS {
                    self.last_roster_ms = now_ms;
                    out.push(Outgoing::to_all(&self.roster()));
                }
            }
            Some(host) => {
                if self.status == LobbyStatus::Joining && now_ms - self.last_hello_ms >= RETRY_MS {
                    self.last_hello_ms = now_ms;
                    out.push(Outgoing::to_peer(&host, &self.hello()));
                }
                if let (Some(team), LobbyStatus::Seated(i)) = (self.wanted_team, &self.status) {
                    if self.seats[*i as usize].team == team {
                        self.wanted_team = None;
                    } else if now_ms - self.last_team_ms >= RETRY_MS {
                        self.last_team_ms = now_ms;
                        out.push(Outgoing::to_peer(&host, &Msg::TeamRequest { team }));
                    }
                }
                if let Some(ready) = self.wanted_ready {
                    if self.is_ready() == ready {
                        self.wanted_ready = None;
                    } else if now_ms - self.last_ready_ms >= RETRY_MS {
                        self.last_ready_ms = now_ms;
                        out.push(Outgoing::to_peer(&host, &Msg::Ready { ready }));
                    }
                }
            }
        }
        if now_ms - self.last_ping_ms >= PING_EVERY_MS {
            self.last_ping_ms = now_ms;
            out.push(Outgoing::to_all(&Msg::Ping {
                client_id: self.client_id.clone(),
                stamp: now_ms,
                round: self.round,
            }));
        }
        out
    }

    /// Handles one message from a peer.
    pub fn receive(&mut self, peer: &PeerId, text: &str, now_ms: i64) -> Vec<Outgoing> {
        let Some(msg) = Msg::decode(text) else {
            return Vec::new();
        };
        if msg.is_match_kind() {
            // After a rematch, a peer not heard in this round may still be
            // sending the old match.
            let fresh = self.round == 0 || self.heard.contains(peer);
            if self.open() && fresh {
                self.early.push((peer.clone(), text.to_string()));
            }
            return Vec::new();
        }
        match msg {
            Msg::Ping {
                client_id,
                stamp,
                round,
            } => {
                if round == self.round {
                    self.heard.insert(peer.clone());
                }
                self.peers.entry(peer.clone()).or_default().client_id = Some(client_id);
                vec![Outgoing::to_peer(peer, &Msg::Pong { stamp })]
            }
            Msg::Pong { stamp } => {
                self.peers
                    .entry(peer.clone())
                    .or_default()
                    .rtt
                    .push(now_ms - stamp);
                Vec::new()
            }
            other => {
                if self.is_host() {
                    self.receive_as_host(peer, other, now_ms)
                } else if self.host_peer.as_ref() == Some(peer) {
                    self.receive_from_host(peer, other)
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn receive_as_host(&mut self, peer: &PeerId, msg: Msg, now_ms: i64) -> Vec<Outgoing> {
        match msg {
            Msg::Hello {
                proto,
                build,
                client_id,
                name,
            } => {
                if self.status == LobbyStatus::Started {
                    let reason = RefuseReason::AlreadyStarted;
                    return vec![Outgoing::to_peer(peer, &Msg::Refuse { reason })];
                }
                if !self.open() {
                    return Vec::new();
                }
                if proto != self.version.proto || build != self.version.build {
                    let reason = RefuseReason::WrongVersion {
                        host_proto: self.version.proto,
                        host_build: self.version.build.clone(),
                    };
                    return vec![Outgoing::to_peer(peer, &Msg::Refuse { reason })];
                }
                self.peers.entry(peer.clone()).or_default().client_id = Some(client_id.clone());
                if self.seat_of_client(&client_id).is_some() {
                    return vec![Outgoing::to_peer(peer, &self.roster())];
                }
                match self.seats.iter().position(|s| s.kind == SeatKind::Open) {
                    Some(i) => {
                        self.seats[i].kind = SeatKind::Human {
                            client_id,
                            name: clean_name(&name),
                            ready: false,
                        };
                        self.roster_changed(now_ms)
                    }
                    None => {
                        let reason = RefuseReason::Full;
                        vec![Outgoing::to_peer(peer, &Msg::Refuse { reason })]
                    }
                }
            }
            Msg::TeamRequest { team } => {
                if !self.open() {
                    return Vec::new();
                }
                match self.seat_of_peer(peer) {
                    Some(i) if self.seats[i].team != team && (team as usize) < MAX_SEATS => {
                        self.seats[i].team = team;
                        self.clear_ready();
                        self.roster_changed(now_ms)
                    }
                    _ => Vec::new(),
                }
            }
            Msg::Ready { ready } => {
                if !self.open() {
                    return Vec::new();
                }
                match self.seat_of_peer(peer) {
                    Some(i) if i != 0 && self.seats[i].ready() != ready => {
                        if let SeatKind::Human { ready: r, .. } = &mut self.seats[i].kind {
                            *r = ready;
                        }
                        self.roster_changed(now_ms)
                    }
                    _ => Vec::new(),
                }
            }
            Msg::Leave => self.free_seat_of(peer, now_ms),
            _ => Vec::new(),
        }
    }

    fn receive_from_host(&mut self, host: &PeerId, msg: Msg) -> Vec<Outgoing> {
        match msg {
            Msg::Roster {
                version,
                seats,
                settings,
                round,
            } => {
                if !self.open() || version <= self.roster_version {
                    return Vec::new();
                }
                // A newcomer to a rematch lobby learns its round here.
                if round != self.round {
                    self.round = round;
                    self.heard.clear();
                    self.heard.insert(host.clone());
                }
                self.roster_version = version;
                self.seats = seats;
                self.settings = settings;
                self.status = match self.seat_of_client(&self.client_id) {
                    Some(i) => LobbyStatus::Seated(i as u8),
                    None => LobbyStatus::Joining,
                };
                Vec::new()
            }
            Msg::Refuse { reason } => {
                if self.status == LobbyStatus::Joining {
                    self.status = LobbyStatus::Refused(reason);
                }
                Vec::new()
            }
            Msg::Dissolve => {
                if self.open() {
                    self.status = LobbyStatus::Dissolved;
                }
                Vec::new()
            }
            Msg::Start {
                seed,
                ms_per_tick,
                settings,
                seats,
            } => {
                let ack = vec![Outgoing::to_peer(host, &Msg::StartAck)];
                if self.status == LobbyStatus::Started {
                    return ack;
                }
                if !self.open() {
                    return Vec::new();
                }
                let Some(local) = seats
                    .iter()
                    .position(|s| s.client_id() == Some(&self.client_id))
                else {
                    return Vec::new();
                };
                let mut peer_seats = BTreeMap::new();
                for (peer, info) in &self.peers {
                    let seat = info
                        .client_id
                        .as_deref()
                        .and_then(|c| seats.iter().position(|s| s.client_id() == Some(c)));
                    if let Some(i) = seat {
                        peer_seats.insert(peer.clone(), i as u8);
                    }
                }
                peer_seats.insert(host.clone(), 0);
                self.seats = seats.clone();
                self.settings = settings.clone();
                self.start = Some(MatchStart {
                    local_seat: local as u8,
                    client_id: self.client_id.clone(),
                    seed,
                    ms_per_tick,
                    settings,
                    seats,
                    peer_seats,
                    rtt: self.rtts(),
                    early: std::mem::take(&mut self.early),
                    start_resend: None,
                    round: self.round,
                    heard: self.heard.clone(),
                });
                self.status = LobbyStatus::Started;
                ack
            }
            _ => Vec::new(),
        }
    }
}
