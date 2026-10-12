//! Lockstep tests over the loopback network with a fake clock and a tiny
//! fake simulation: the state is a u64 that each tick folds in the tick's
//! commands and drops (FNV), and the checksum is the state.

use std::collections::BTreeMap;

use tool_core::lockstep::loopback::{Event, Loopback};
use tool_core::lockstep::{
    Lobby, LobbyError, LobbyStatus, Match, MatchStatus, Msg, Outgoing, PeerId, RefuseReason,
    SeatKind, TickInput, Version, CHECKSUM_EVERY, MAX_MESSAGE_BYTES, PROTOCOL_VERSION,
};

const MS_PER_TICK: u32 = 50;
const STEP_MS: i64 = 10;
const BUILD: &str = "build-1";

fn fold(state: u64, input: &TickInput) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h = state ^ 0xCBF2_9CE4_8422_2325;
    let mut mix = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    };
    for b in input.tick.to_le_bytes() {
        mix(b);
    }
    for (seat, cmd) in &input.commands {
        mix(*seat);
        for b in cmd.bytes() {
            mix(b);
        }
    }
    for seat in &input.dropped {
        mix(0xFF);
        mix(*seat);
    }
    h
}

#[derive(Default)]
struct Client {
    lobby: Option<Lobby>,
    game: Option<Match>,
    started_at: i64,
    state: u64,
    sums: BTreeMap<u32, u64>,
    drops: Vec<(u32, u8)>,
    corrupt_at: Option<u32>,
    cmd_every: u32,
    delay_plan: BTreeMap<u32, u32>,
    gone: bool,
    snapshots_loaded: u32,
    delays_seen: Vec<u32>,
    last_snapshot: Option<String>,
    manual_snapshot: bool,
    /// Times a due tick had to wait for a turn.
    stalls: u32,
    /// A command queued as soon as the next match begins: in its turn for
    /// tick 0.
    first_command: Option<String>,
}

impl Client {
    fn ended(&self) -> bool {
        self.game
            .as_ref()
            .is_some_and(|g| matches!(g.status(), MatchStatus::Ended(_)))
    }
}

struct World {
    net: Loopback,
    clients: Vec<Client>,
    now: i64,
    goal: u32,
    start_msgs: usize,
    /// A fixed input delay each client sets before its first turn.
    fixed_delay: Option<u32>,
}

impl World {
    /// A lobby: client 0 hosts four seats; clients 1.. join.
    fn lobby(humans: usize, seed: u64, latency: i64, jitter: i64, loss: u64) -> World {
        World::lobby_as(humans, seed, latency, jitter, loss, Version::current(BUILD))
    }

    fn lobby_as(
        humans: usize,
        seed: u64,
        latency: i64,
        jitter: i64,
        loss: u64,
        joiner_version: Version,
    ) -> World {
        let mut net = Loopback::new(humans, seed, latency, jitter);
        net.set_lobby_loss(loss);
        let mut clients: Vec<Client> = (0..humans).map(|_| Client::default()).collect();
        clients[0].lobby = Some(Lobby::host(
            Version::current(BUILD),
            "c0",
            "Ann",
            4,
            "{\"map\":1}",
            0,
        ));
        for (i, c) in clients.iter_mut().enumerate().skip(1) {
            let (lobby, out) = Lobby::join(
                joiner_version.clone(),
                Loopback::peer_id(0),
                &format!("c{i}"),
                &format!("Player {i}"),
                0,
            );
            c.lobby = Some(lobby);
            net.send(i, out);
        }
        let mut world = World {
            net,
            clients,
            now: 0,
            goal: 0,
            start_msgs: 0,
            fixed_delay: None,
        };
        let events = world.net.connect_all();
        for ev in events {
            world.route(ev);
        }
        world
    }

    /// A lobby with `humans` seated, `ai` AI seats after them, the rest
    /// closed, then started by the host.
    fn started(humans: usize, ai: usize, seed: u64, latency: i64, jitter: i64) -> World {
        World::started_with(humans, ai, seed, latency, jitter, None)
    }

    fn started_with(
        humans: usize,
        ai: usize,
        seed: u64,
        latency: i64,
        jitter: i64,
        fixed_delay: Option<u32>,
    ) -> World {
        let mut w = World::lobby(humans, seed, latency, jitter, 0);
        w.fixed_delay = fixed_delay;
        w.run_until(3_000, |w| {
            w.clients.iter().skip(1).all(|c| {
                matches!(
                    c.lobby.as_ref().map(|l| l.status()),
                    Some(LobbyStatus::Seated(_))
                )
            })
        });
        let now = w.now;
        for i in humans..4 {
            let kind = if i < humans + ai {
                SeatKind::Ai { difficulty: 1 }
            } else {
                SeatKind::Closed
            };
            let out = w.clients[0]
                .lobby
                .as_mut()
                .unwrap()
                .set_seat(i, kind, now)
                .unwrap();
            w.net.send(0, out);
        }
        w.run_for(1_500);
        w.host_starts();
        let all = w.run_until(5_000, |w| w.clients.iter().all(|c| c.game.is_some()));
        assert!(all, "not every client started");
        w
    }

    /// Every seated joiner presses Ready Up; runs until the host sees it.
    fn ready_all(&mut self) {
        let now = self.now;
        for i in 1..self.clients.len() {
            if let Some(l) = self.clients[i].lobby.as_mut() {
                if let Ok(out) = l.set_ready(true, now) {
                    self.net.send(i, out);
                }
            }
        }
        let ready = self.run_until(10_000, |w| {
            w.clients[0].lobby.as_ref().is_some_and(|l| l.all_ready())
        });
        assert!(ready, "the host never saw every joiner ready");
    }

    fn host_starts(&mut self) {
        self.ready_all();
        let now = self.now;
        let out = self.clients[0]
            .lobby
            .as_mut()
            .unwrap()
            .start(42, MS_PER_TICK, now)
            .unwrap();
        self.net.send(0, out);
        self.begin_match_if_started(0);
    }

    fn begin_match_if_started(&mut self, i: usize) {
        let now = self.now;
        let c = &mut self.clients[i];
        if c.game.is_some() {
            return;
        }
        let Some(start) = c.lobby.as_mut().and_then(|l| l.take_start()) else {
            return;
        };
        let (mut game, out) = Match::new(start, now);
        game.set_delay_override(self.fixed_delay);
        if let Some(cmd) = c.first_command.take() {
            game.queue_command(cmd);
        }
        c.game = Some(game);
        c.lobby = None;
        c.started_at = now;
        // A new match (a rematch too) starts from a fresh state.
        c.state = 0;
        c.sums.clear();
        c.drops.clear();
        self.net.send(i, out);
    }

    fn route(&mut self, ev: Event) {
        let now = self.now;
        let i = ev.node();
        if self.clients[i].gone {
            return;
        }
        let out = {
            let c = &mut self.clients[i];
            match (&ev, c.game.as_mut(), c.lobby.as_mut()) {
                (Event::Message { from, text, .. }, Some(g), _) if g.is_rematch(from, text) => {
                    // The host's Rematch: back to its lobby.
                    let (mut l, mut out) = Lobby::rejoin(
                        Version::current(BUILD),
                        from.clone(),
                        &format!("c{i}"),
                        &format!("Player {i}"),
                        g,
                        now,
                    );
                    out.extend(l.receive(from, text, now));
                    c.lobby = Some(l);
                    c.game = None;
                    out
                }
                (Event::Message { from, text, .. }, Some(g), _) => g.receive(from, text, now),
                (Event::Message { from, text, .. }, None, Some(l)) => {
                    if Msg::decode(text).is_some_and(|m| matches!(m, Msg::Start { .. })) {
                        self.start_msgs += 1;
                    }
                    l.receive(from, text, now)
                }
                (Event::Disconnected { peer, .. }, Some(g), _) => {
                    g.peer_disconnected(peer);
                    Vec::new()
                }
                (Event::Disconnected { peer, .. }, None, Some(l)) => l.peer_disconnected(peer, now),
                (Event::Connected { peer, .. }, None, Some(l)) => {
                    l.peer_connected(peer.clone(), now)
                }
                _ => Vec::new(),
            }
        };
        self.net.send(i, out);
        self.begin_match_if_started(i);
    }

    fn step(&mut self) {
        self.now += STEP_MS;
        let events = self.net.advance(self.now);
        for ev in events {
            self.route(ev);
        }
        for i in 0..self.clients.len() {
            if self.clients[i].gone {
                continue;
            }
            let out = self.tick_client(i);
            self.net.send(i, out);
        }
    }

    fn tick_client(&mut self, i: usize) -> Vec<Outgoing> {
        let now = self.now;
        let goal = self.goal;
        let c = &mut self.clients[i];
        let Some(g) = c.game.as_mut() else {
            return c.lobby.as_mut().map(|l| l.update(now)).unwrap_or_default();
        };
        if let Some(d) = c.delay_plan.get(&g.next_tick()) {
            g.set_delay_override(Some(*d));
        }
        let mut out = g.update(now);
        loop {
            if let (Some(t), false) = (g.wants_snapshot(), c.manual_snapshot) {
                out.extend(g.provide_snapshot(t, &c.state.to_string()));
            }
            if let Some((_, s)) = g.take_snapshot() {
                c.state = s.parse().unwrap_or(0);
                c.last_snapshot = Some(s);
                c.snapshots_loaded += 1;
            }
            let t = g.next_tick();
            if t >= goal || t as i64 > (now - c.started_at) / MS_PER_TICK as i64 {
                break;
            }
            if let Some(d) = c.delay_plan.get(&t) {
                g.set_delay_override(Some(*d));
            }
            let Some(input) = g.take_tick(t) else {
                c.stalls += 1;
                break;
            };
            c.state = fold(c.state, &input);
            if c.corrupt_at == Some(t) {
                c.state ^= 0xDEAD;
            }
            for d in &input.dropped {
                c.drops.push((t, *d));
            }
            if t.is_multiple_of(CHECKSUM_EVERY) {
                c.sums.insert(t, c.state);
                g.record_checksum(t, c.state);
            }
            // Commands depend on the seat, not on which node joined first.
            let seat = g.local_seat() as u32;
            if c.cmd_every > 0 && (t + seat).is_multiple_of(c.cmd_every) {
                g.queue_command(format!("cmd-{seat}-{t}"));
                if t % 7 == 0 {
                    g.queue_command(format!("extra-{seat}-{t}"));
                }
            }
            out.extend(g.update(now));
            if c.delays_seen.last() != Some(&g.delay()) {
                c.delays_seen.push(g.delay());
            }
        }
        out
    }

    fn run_for(&mut self, ms: i64) {
        let end = self.now + ms;
        while self.now < end {
            self.step();
        }
    }

    fn run_until(&mut self, max_ms: i64, done: impl Fn(&World) -> bool) -> bool {
        let end = self.now + max_ms;
        while self.now < end {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    fn live(&self) -> impl Iterator<Item = &Client> {
        self.clients.iter().filter(|c| !c.gone && !c.ended())
    }

    /// Runs every live client to `goal` ticks; false if that takes too long.
    fn play_to(&mut self, goal: u32, max_ms: i64) -> bool {
        self.goal = goal;
        self.run_until(max_ms, |w| {
            w.live()
                .all(|c| c.game.as_ref().is_some_and(|g| g.next_tick() >= goal))
        })
    }

    fn leave(&mut self, i: usize) {
        self.net.disconnect(i);
        self.clients[i].gone = true;
    }

    fn final_states(&self) -> Vec<u64> {
        self.live().map(|c| c.state).collect()
    }

    fn assert_in_step(&self) {
        let states = self.final_states();
        assert!(states.len() >= 2, "fewer than two live clients");
        assert!(
            states.windows(2).all(|w| w[0] == w[1]),
            "final states differ: {states:?}"
        );
        let first = &self.live().next().unwrap().sums;
        for c in self.live().skip(1) {
            assert_eq!(&c.sums, first, "checksum history differs");
        }
    }
}

fn set_commands(w: &mut World, every: u32) {
    for c in &mut w.clients {
        c.cmd_every = every;
    }
}

#[test]
fn two_peers_stay_in_step_for_10000_ticks() {
    let mut w = World::started(2, 0, 1, 40, 300);
    set_commands(&mut w, 3);
    assert!(w.play_to(10_000, 2_000_000), "did not reach 10000 ticks");
    w.assert_in_step();
    assert!(w.clients[0].sums.len() >= 500);
    assert!(
        w.clients.iter().all(|c| c.stalls > 0),
        "no turn was ever late"
    );
}

#[test]
fn four_peers_stay_in_step_for_5000_ticks() {
    let mut w = World::started(4, 0, 2, 30, 200);
    w.net.set_link(0, 3, 90, 300);
    set_commands(&mut w, 2);
    assert!(w.play_to(5_000, 2_000_000), "did not reach 5000 ticks");
    w.assert_in_step();
    assert!(
        w.clients.iter().all(|c| c.stalls > 0),
        "no turn was ever late"
    );
}

fn fixed_delay_run(jitter: i64) -> (u64, i64) {
    let mut w = World::started_with(3, 0, 7, 40, jitter, Some(4));
    set_commands(&mut w, 2);
    let begun = w.now;
    assert!(w.play_to(1_500, 1_000_000));
    w.assert_in_step();
    (w.final_states()[0], w.now - begun)
}

#[test]
fn jitter_changes_nothing_but_the_wait() {
    let (calm, calm_ms) = fixed_delay_run(0);
    let (jittery, jittery_ms) = fixed_delay_run(400);
    assert_eq!(calm, jittery, "jitter changed the outcome");
    assert!(
        jittery_ms > calm_ms,
        "jitter should only cost waiting: {calm_ms} vs {jittery_ms}"
    );
}

#[test]
fn delay_changes_mid_match_keep_peers_in_step() {
    let mut w = World::started(3, 0, 3, 30, 30);
    set_commands(&mut w, 2);
    w.clients[0].delay_plan = [(100, 6), (300, 2), (500, 4), (700, 2)].into();
    w.clients[1].delay_plan = [(200, 5), (400, 2), (650, 6)].into();
    w.clients[2].delay_plan = [(150, 2), (250, 6), (600, 3)].into();
    assert!(w.play_to(1_000, 1_000_000), "did not reach 1000 ticks");
    w.assert_in_step();
    for c in &w.clients {
        assert!(
            c.delays_seen.len() >= 3,
            "delay never changed: {:?}",
            c.delays_seen
        );
    }
}

#[test]
fn ai_seats_need_no_turns() {
    let mut w = World::started(2, 2, 4, 30, 10);
    let seats = w.clients[1].game.as_ref().unwrap().seats().to_vec();
    assert!(matches!(seats[2].kind, SeatKind::Ai { .. }));
    assert!(matches!(seats[3].kind, SeatKind::Ai { .. }));
    set_commands(&mut w, 2);
    assert!(w.play_to(500, 200_000), "AI seats blocked the match");
    w.assert_in_step();
}

#[test]
fn a_desync_is_resynced() {
    let mut w = World::started(3, 0, 5, 30, 20);
    set_commands(&mut w, 2);
    w.clients[2].corrupt_at = Some(105);
    assert!(w.play_to(800, 400_000), "did not reach 800 ticks");
    let detected: u32 = w
        .clients
        .iter()
        .map(|c| c.game.as_ref().unwrap().desyncs_detected())
        .sum();
    assert!(detected > 0, "the desync was never detected");
    assert!(w.clients[2].snapshots_loaded >= 1, "no snapshot loaded");
    let states = w.final_states();
    assert!(states.windows(2).all(|p| p[0] == p[1]), "{states:?}");
    // After the resync every checksum agrees again.
    let after =
        |c: &Client| -> Vec<(u32, u64)> { c.sums.range(300..).map(|(t, s)| (*t, *s)).collect() };
    assert_eq!(after(&w.clients[0]), after(&w.clients[1]));
    assert_eq!(after(&w.clients[0]), after(&w.clients[2]));
    assert_eq!(after(&w.clients[0]).len(), 25);
    for c in &w.clients {
        assert_eq!(c.game.as_ref().unwrap().status(), MatchStatus::Running);
    }
}

fn assert_same_drop(w: &World, seat: u8) -> u32 {
    let drops: Vec<&Vec<(u32, u8)>> = w.live().map(|c| &c.drops).collect();
    assert_eq!(drops[0].len(), 1, "dropped {:?}", drops[0]);
    assert_eq!(drops[0][0].1, seat);
    for d in &drops {
        assert_eq!(*d, drops[0], "drops differ: {drops:?}");
    }
    drops[0][0].0
}

#[test]
fn a_dropped_peer_is_dropped_on_the_same_tick_everywhere() {
    let mut w = World::started(4, 0, 6, 30, 0);
    set_commands(&mut w, 2);
    w.goal = 10_000;
    w.run_until(100_000, |w| {
        w.clients[3]
            .game
            .as_ref()
            .is_some_and(|g| g.next_tick() >= 300)
    });
    // Seat 3's last turns reach clients 0 and 2 but not client 1.
    w.net.cut(3, 1);
    w.run_for(200);
    let held = |w: &World, i: usize| w.clients[i].game.as_ref().unwrap().held(3);
    assert!(
        held(&w, 1) < held(&w, 2),
        "client 1 should hold fewer turns from seat 3: {:?} vs {:?}",
        held(&w, 1),
        held(&w, 2)
    );
    w.leave(3);
    assert!(w.play_to(800, 400_000), "the match did not continue");
    let d = assert_same_drop(&w, 3);
    assert!(d > 300);
    w.assert_in_step();
}

#[test]
fn a_silent_peer_is_dropped_after_15_seconds() {
    let mut w = World::started(3, 0, 8, 30, 0);
    set_commands(&mut w, 2);
    w.goal = 10_000;
    w.run_until(100_000, |w| {
        w.clients[2]
            .game
            .as_ref()
            .is_some_and(|g| g.next_tick() >= 100)
    });
    w.net.cut(2, 0);
    w.net.cut(2, 1);
    w.run_for(3_000);
    assert_eq!(
        w.clients[0].game.as_ref().unwrap().status(),
        MatchStatus::WaitingOn(vec![2])
    );
    assert!(w.play_to(400, 100_000), "the silent peer was never dropped");
    assert!(
        w.clients[2].ended(),
        "the silent peer should learn it was dropped"
    );
    assert_same_drop(&w, 2);
    w.assert_in_step();
}

#[test]
fn the_host_leaving_does_not_end_the_match() {
    let mut w = World::started(3, 1, 9, 30, 20);
    set_commands(&mut w, 2);
    w.goal = 10_000;
    w.run_until(100_000, |w| {
        w.clients[0]
            .game
            .as_ref()
            .is_some_and(|g| g.next_tick() >= 200)
    });
    w.leave(0);
    assert!(w.play_to(700, 400_000), "the match ended with the host");
    assert_same_drop(&w, 0);
    w.assert_in_step();
    assert_eq!(w.clients[1].game.as_ref().unwrap().coordinator(), Some(1));
    assert_eq!(w.clients[2].game.as_ref().unwrap().coordinator(), Some(1));
}

#[test]
fn a_lost_start_is_sent_again() {
    let mut w = World::lobby(2, 10, 30, 0, 0);
    w.run_for(1_500);
    w.net.drop_next(0, 1, "\"t\":\"start\"");
    w.host_starts();
    assert!(w.play_to(200, 60_000), "the joiner never started");
    assert!(w.start_msgs >= 1);
    w.assert_in_step();
}

#[test]
fn wrong_version_is_refused_by_name() {
    for version in [
        Version {
            proto: 99,
            build: BUILD.to_string(),
        },
        Version::current("other-build"),
    ] {
        let mut w = World::lobby_as(2, 11, 20, 0, 0, version);
        w.run_for(1_000);
        assert_eq!(
            w.clients[1].lobby.as_ref().unwrap().status(),
            &LobbyStatus::Refused(RefuseReason::WrongVersion {
                host_proto: PROTOCOL_VERSION,
                host_build: BUILD.to_string()
            })
        );
        assert_eq!(
            w.clients[0].lobby.as_ref().unwrap().seats()[1].kind,
            SeatKind::Open
        );
    }
}

#[test]
fn a_full_lobby_refuses() {
    let mut w = World::lobby(3, 12, 20, 0, 0);
    let host = Lobby::host(Version::current(BUILD), "c0", "Ann", 2, "{}", 0);
    w.clients[0].lobby = Some(host);
    w.run_for(2_000);
    let statuses: Vec<LobbyStatus> = w.clients[1..]
        .iter()
        .map(|c| c.lobby.as_ref().unwrap().status().clone())
        .collect();
    assert!(statuses.contains(&LobbyStatus::Seated(1)), "{statuses:?}");
    assert!(
        statuses.contains(&LobbyStatus::Refused(RefuseReason::Full)),
        "{statuses:?}"
    );
}

#[test]
fn a_started_match_refuses_a_late_joiner() {
    let mut w = World::started(2, 0, 13, 20, 0);
    w.goal = 50;
    w.run_for(500);
    let host = w.clients[0].game.as_mut().unwrap();
    let hello = Msg::Hello {
        proto: 1,
        build: BUILD.to_string(),
        client_id: "late".into(),
        name: "Late".into(),
    };
    let out = host.receive(&Loopback::peer_id(7), &hello.encode(), w.now);
    assert_eq!(
        Msg::decode(&out[0].text),
        Some(Msg::Refuse {
            reason: RefuseReason::AlreadyStarted
        })
    );
}

#[test]
fn the_lobby_never_starts_with_every_played_seat_on_one_team() {
    // TRIAGE row 81, in the lobby itself: whatever the page shows.
    let mut w = World::lobby(1, 3, 25, 0, 0);
    let now = w.now;
    let host = w.clients[0].lobby.as_mut().unwrap();
    host.set_seat(1, SeatKind::Ai { difficulty: 1 }, now)
        .unwrap();
    host.set_team(1, 0, now).unwrap();
    // Open seats are played (a Pupa AI fills them); Closed seats are not.
    assert_eq!(host.team_count(), 3);
    host.set_seat(2, SeatKind::Closed, now).unwrap();
    host.set_seat(3, SeatKind::Closed, now).unwrap();
    assert_eq!(host.team_count(), 1);
    assert_eq!(
        host.start(1, 50, now),
        Err(tool_core::lockstep::LobbyError::OneTeam)
    );
    host.set_teams(&[2, 1], now).unwrap();
    assert_eq!(host.team_count(), 2);
    assert!(host.start(1, 50, now).is_ok());
}

#[test]
fn lobby_host_sets_seats_teams_and_settings_and_every_joiner_sees_the_same_roster() {
    let mut w = World::lobby(3, 14, 25, 15, 200);
    w.run_until(10_000, |w| {
        w.clients
            .iter()
            .skip(1)
            .all(|c| matches!(c.lobby.as_ref().unwrap().status(), LobbyStatus::Seated(_)))
    });
    let now = w.now;
    let host = w.clients[0].lobby.as_mut().unwrap();
    let mut out = host
        .set_seat(3, SeatKind::Ai { difficulty: 2 }, now)
        .unwrap();
    out.extend(host.set_team(3, 3, now).unwrap());
    out.extend(host.set_team(0, 2, now).unwrap());
    // Teams are Team 1–4 (0–3): no fifth.
    assert_eq!(
        host.set_team(3, 7, now),
        Err(tool_core::lockstep::LobbyError::NoSuchTeam)
    );
    out.extend(host.set_settings("{\"speed\":\"fast\"}", now).unwrap());
    assert_eq!(
        host.set_seat(0, SeatKind::Closed, now),
        Err(tool_core::lockstep::LobbyError::OwnSeat)
    );
    assert_eq!(
        host.set_seat(1, SeatKind::Closed, now),
        Err(tool_core::lockstep::LobbyError::SeatTaken)
    );
    w.net.send(0, out);
    let out = w.clients[2]
        .lobby
        .as_mut()
        .unwrap()
        .request_team(2, now)
        .unwrap();
    w.net.send(2, out);
    w.run_for(8_000);
    let host_seats = w.clients[0].lobby.as_ref().unwrap().seats().to_vec();
    assert_eq!(host_seats[3].kind, SeatKind::Ai { difficulty: 2 });
    assert_eq!(host_seats[3].team, 3);
    assert_eq!(host_seats[0].team, 2);
    let joiner_seat = host_seats
        .iter()
        .position(|s| s.client_id() == Some("c2"))
        .unwrap();
    assert_eq!(host_seats[joiner_seat].team, 2, "the joiner's team request");
    for c in &w.clients[1..] {
        let l = c.lobby.as_ref().unwrap();
        assert_eq!(l.seats(), &host_seats[..]);
        assert_eq!(l.settings(), "{\"speed\":\"fast\"}");
        assert!(l.rtt_p90(&Loopback::peer_id(0)).is_some());
    }
    // A joiner cannot set seats.
    let now = w.now;
    let j = w.clients[1].lobby.as_mut().unwrap();
    assert_eq!(
        j.set_settings("x", now),
        Err(tool_core::lockstep::LobbyError::NotHost)
    );
}

#[test]
fn the_host_leaving_the_lobby_dissolves_it() {
    let mut w = World::lobby(2, 15, 20, 0, 0);
    w.run_for(1_000);
    let out = w.clients[0].lobby.as_mut().unwrap().leave();
    w.net.send(0, out);
    w.run_for(500);
    assert_eq!(
        w.clients[1].lobby.as_ref().unwrap().status(),
        &LobbyStatus::Dissolved
    );
}

#[test]
fn a_joiner_leaving_frees_its_seat() {
    let mut w = World::lobby(2, 16, 20, 0, 0);
    w.run_for(1_000);
    let out = w.clients[1].lobby.as_mut().unwrap().leave();
    w.net.send(1, out);
    w.run_for(500);
    assert_eq!(
        w.clients[0].lobby.as_ref().unwrap().seats()[1].kind,
        SeatKind::Open
    );
}

#[test]
fn a_large_snapshot_is_sent_in_chunks_under_16_kb() {
    let mut w = World::started(2, 0, 17, 20, 0);
    w.clients[1].corrupt_at = Some(30);
    w.clients[0].manual_snapshot = true;
    w.goal = 10_000;
    w.run_until(100_000, |w| {
        w.clients[0]
            .game
            .as_ref()
            .unwrap()
            .wants_snapshot()
            .is_some()
    });
    let g = w.clients[0].game.as_mut().unwrap();
    let t = g
        .wants_snapshot()
        .expect("the coordinator never asked for a snapshot");
    let big: String = (0..40_000)
        .map(|i| ["\"", "\\", "a", "\n", "\u{1}", "é"][i % 6])
        .collect();
    let out = g.provide_snapshot(t, &big);
    assert!(out.len() > 3, "expected several chunks, got {}", out.len());
    for o in &out {
        assert!(o.text.len() <= MAX_MESSAGE_BYTES, "{} bytes", o.text.len());
    }
    w.net.send(0, out);
    w.run_until(10_000, |w| w.clients[1].last_snapshot.is_some());
    assert_eq!(w.clients[1].last_snapshot.as_deref(), Some(big.as_str()));
}

// ---- Ready Up, the seat count and Rematch (TRIAGE rows 40, 41, 49) ----

fn seated(w: &mut World) {
    let ok = w.run_until(10_000, |w| {
        w.clients.iter().skip(1).all(|c| {
            matches!(
                c.lobby.as_ref().map(|l| l.status()),
                Some(LobbyStatus::Seated(_))
            )
        })
    });
    assert!(ok, "not every joiner was seated");
}

fn ready_up(w: &mut World, i: usize, ready: bool) {
    let now = w.now;
    let out = w.clients[i]
        .lobby
        .as_mut()
        .unwrap()
        .set_ready(ready, now)
        .unwrap();
    w.net.send(i, out);
}

#[test]
fn the_host_starts_only_when_every_joiner_is_ready() {
    let mut w = World::lobby(3, 20, 25, 10, 150);
    seated(&mut w);
    let now = w.now;
    assert_eq!(
        w.clients[0]
            .lobby
            .as_mut()
            .unwrap()
            .start(1, MS_PER_TICK, now),
        Err(LobbyError::NotReady)
    );
    ready_up(&mut w, 1, true);
    w.run_for(4_000);
    for c in &w.clients {
        let l = c.lobby.as_ref().unwrap();
        let seat = l
            .seats()
            .iter()
            .position(|s| s.client_id() == Some("c1"))
            .unwrap();
        assert!(l.seats()[seat].ready(), "every client sees c1 ready");
    }
    assert!(!w.clients[0].lobby.as_ref().unwrap().all_ready());
    // c1 takes it back; c2 readies.
    ready_up(&mut w, 1, false);
    ready_up(&mut w, 2, true);
    w.run_for(4_000);
    assert!(!w.clients[0].lobby.as_ref().unwrap().all_ready());
    assert!(!w.clients[1].lobby.as_ref().unwrap().is_ready());
    assert!(w.clients[2].lobby.as_ref().unwrap().is_ready());
    // New Match Settings clear every mark.
    let now = w.now;
    let out = w.clients[0]
        .lobby
        .as_mut()
        .unwrap()
        .set_settings("{\"x\":2}", now)
        .unwrap();
    w.net.send(0, out);
    w.run_for(4_000);
    assert!(
        !w.clients[2].lobby.as_ref().unwrap().is_ready(),
        "new settings clear Ready Up"
    );
    w.host_starts();
    assert!(w.run_until(5_000, |w| w.clients.iter().all(|c| c.game.is_some())));
}

#[test]
fn a_lobby_hosted_with_two_seats_grows_to_four_humans() {
    let mut w = World::lobby(4, 21, 20, 5, 0);
    let mut host = Lobby::host(Version::current(BUILD), "c0", "Ann", 2, "{}", 0);
    let out = host.set_seat_count(4, 0).unwrap();
    w.clients[0].lobby = Some(host);
    w.net.send(0, out);
    seated(&mut w);
    let now = w.now;
    let host = w.clients[0].lobby.as_mut().unwrap();
    assert_eq!(host.seats().len(), 4);
    assert_eq!(
        host.set_seat_count(2, now),
        Err(LobbyError::SeatTaken),
        "four humans do not fit in two seats"
    );
    w.host_starts();
    assert!(w.run_until(5_000, |w| w.clients.iter().all(|c| c.game.is_some())));
    set_commands(&mut w, 3);
    assert!(w.play_to(300, 60_000), "four humans play");
    w.assert_in_step();
}

#[test]
fn shrinking_the_lobby_moves_a_human_into_a_kept_seat() {
    let mut w = World::lobby(2, 22, 20, 0, 0);
    let mut host = Lobby::host(Version::current(BUILD), "c0", "Ann", 4, "{}", 0);
    let _ = host.set_seat(1, SeatKind::Closed, 0).unwrap();
    let _ = host.set_seat(2, SeatKind::Ai { difficulty: 0 }, 0).unwrap();
    w.clients[0].lobby = Some(host);
    seated(&mut w);
    assert_eq!(
        w.clients[1].lobby.as_ref().unwrap().status(),
        &LobbyStatus::Seated(3)
    );
    let now = w.now;
    let out = w.clients[0]
        .lobby
        .as_mut()
        .unwrap()
        .set_seat_count(2, now)
        .unwrap();
    w.net.send(0, out);
    w.run_for(1_000);
    assert_eq!(
        w.clients[1].lobby.as_ref().unwrap().status(),
        &LobbyStatus::Seated(1)
    );
    assert_eq!(w.clients[1].lobby.as_ref().unwrap().seats().len(), 2);
}

#[test]
fn a_joiner_that_loses_the_host_says_so() {
    let mut w = World::lobby(2, 23, 20, 0, 0);
    w.run_for(1_000);
    w.leave(0);
    w.run_for(500);
    assert_eq!(
        w.clients[1].lobby.as_ref().unwrap().status(),
        &LobbyStatus::Lost
    );
}

#[test]
fn names_are_trimmed_and_kept_short() {
    let mut w = World::lobby(2, 24, 20, 0, 0);
    w.run_for(200);
    let now = w.now;
    let hello = Msg::Hello {
        proto: PROTOCOL_VERSION,
        build: BUILD.to_string(),
        client_id: "long".into(),
        name: "  A very long name indeed\n".into(),
    };
    let mut host = Lobby::host(Version::current(BUILD), "c0", "Ann", 4, "{}", now);
    host.receive(&PeerId("x".into()), &hello.encode(), now);
    let seat = host
        .seats()
        .iter()
        .find(|s| s.client_id() == Some("long"))
        .unwrap();
    match &seat.kind {
        SeatKind::Human { name, .. } => assert_eq!(name, "A very long name"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rematch_brings_every_joiner_back_to_the_same_lobby() {
    // Started at once: old turns reach the new match.
    rematch_run(0);
}

#[test]
fn a_rematch_started_later_keeps_old_turns_out_of_the_lobby() {
    // Started once every new-round ping is in: old turns reach the lobby.
    rematch_run(2_000);
}

/// Joiner-to-joiner messages are slow, and the host calls the rematch at
/// once, so the joiners' old turns from tick 0 are still on the way to each
/// other when both are back in the lobby. Joiner 1's first new turn differs
/// from its first old one, so an old turn taken for a new one shows as a
/// desync. The host starts the rematch `wait_ms` after everyone is back.
fn rematch_run(wait_ms: i64) {
    let mut w = World::lobby(3, 25, 30, 20, 0);
    w.net.set_link(1, 2, 400, 0);
    seated(&mut w);
    let now = w.now;
    let out = w.clients[0]
        .lobby
        .as_mut()
        .unwrap()
        .set_seat(3, SeatKind::Ai { difficulty: 1 }, now)
        .unwrap();
    w.net.send(0, out);
    w.host_starts();
    assert!(w.run_until(5_000, |w| w.clients.iter().all(|c| c.game.is_some())));
    w.goal = 10_000;
    w.run_for(100);
    let seats_before = w.clients[0].game.as_ref().unwrap().seats().to_vec();
    let now = w.now;
    let (lobby, out) = Lobby::rematch(
        Version::current(BUILD),
        "c0",
        "Ann",
        w.clients[0].game.as_ref().unwrap(),
        now,
    );
    w.clients[0].game = None;
    w.clients[0].lobby = Some(lobby);
    w.net.send(0, out);
    let back = w.run_until(10_000, |w| {
        w.clients.iter().skip(1).all(|c| {
            c.game.is_none()
                && matches!(
                    c.lobby.as_ref().map(|l| l.status()),
                    Some(LobbyStatus::Seated(_))
                )
        })
    });
    assert!(back, "every joiner came back to the lobby");
    for c in &w.clients {
        let l = c.lobby.as_ref().unwrap();
        assert_eq!(l.seats().len(), seats_before.len());
        for (now, before) in l.seats().iter().zip(&seats_before) {
            assert_eq!(now.client_id(), before.client_id(), "the same seats");
            assert!(!now.ready(), "Ready Up is cleared");
        }
        assert_eq!(l.seats()[3].kind, SeatKind::Ai { difficulty: 1 });
    }
    w.run_for(wait_ms);
    w.clients[1].first_command = Some("new-match".into());
    w.host_starts();
    set_commands(&mut w, 1);
    assert!(w.run_until(5_000, |w| w.clients.iter().all(|c| c.game.is_some())));
    assert!(w.play_to(300, 120_000), "the rematch runs");
    w.assert_in_step();
    let desyncs: Vec<u32> = w
        .clients
        .iter()
        .map(|c| c.game.as_ref().unwrap().desyncs_detected())
        .collect();
    assert_eq!(
        desyncs,
        vec![0, 0, 0],
        "no old turn was taken for a new one"
    );
}

#[test]
fn a_rematch_opens_the_seat_of_a_player_who_left() {
    let mut w = World::started(3, 0, 26, 30, 0);
    assert!(w.play_to(60, 60_000));
    w.leave(2);
    w.run_for(200);
    let now = w.now;
    let (lobby, _) = Lobby::rematch(
        Version::current(BUILD),
        "c0",
        "Ann",
        w.clients[0].game.as_ref().unwrap(),
        now,
    );
    let gone = w.clients[0]
        .game
        .as_ref()
        .unwrap()
        .seats()
        .iter()
        .position(|s| s.client_id() == Some("c2"))
        .unwrap();
    assert_eq!(lobby.seats()[gone].kind, SeatKind::Open);
    assert!(lobby.seats().iter().any(|s| s.client_id() == Some("c1")));
}

// ---- the same through the App, with the page's transport faked ----

mod through_the_app {
    use serde_json::Value;
    use tool_core::app::{App, Screen};

    fn app(id: &str) -> App {
        let strings = include_str!("../../../data/strings.json");
        let glossary = include_str!("../../../data/glossary.json");
        App::new(strings, glossary, "", "", id).expect("data files parse")
    }

    fn peer(i: usize) -> String {
        format!("p{i}")
    }

    /// Apps joined in a mesh: what one sends, the others receive at once.
    struct Mesh {
        apps: Vec<App>,
        linked: Vec<bool>,
        now: i64,
        /// Transport requests each app made, by op.
        ops: Vec<Vec<String>>,
    }

    impl Mesh {
        fn new(n: usize) -> Mesh {
            let mut apps: Vec<App> = (0..n).map(|i| app(&format!("c{i}"))).collect();
            for a in &mut apps {
                a.act("online.room", "TEST", false, false);
            }
            Mesh {
                apps,
                linked: vec![false; n],
                now: 1_000,
                ops: vec![Vec::new(); n],
            }
        }

        /// App `i` reaches every linked app (the host first announces itself).
        fn link(&mut self, i: usize) {
            for j in 0..self.apps.len() {
                if j != i && self.linked[j] {
                    self.apps[i].net_event("connected", &peer(j), "", j == 0);
                    self.apps[j].net_event("connected", &peer(i), "", i == 0);
                }
            }
            self.linked[i] = true;
        }

        fn unlink(&mut self, i: usize) {
            self.linked[i] = false;
            for j in 0..self.apps.len() {
                if j != i && self.linked[j] {
                    self.apps[j].net_event("disconnected", &peer(i), "", false);
                }
            }
        }

        fn ui(&mut self, i: usize) -> Value {
            let ui = self.apps[i].ui();
            if let Some(reqs) = ui["net_requests"].as_array() {
                // As the page does: messages go out before the transport acts.
                self.deliver(i);
                for r in reqs {
                    let op = r["op"].as_str().unwrap_or("").to_string();
                    if op == "leave" && self.linked[i] {
                        self.unlink(i);
                    }
                    self.ops[i].push(op);
                }
            }
            ui
        }

        fn step(&mut self) {
            self.now += 20;
            for i in 0..self.apps.len() {
                self.apps[i].advance(self.now);
                self.ui(i);
                self.deliver(i);
            }
        }

        fn deliver(&mut self, i: usize) {
            let out = self.apps[i].take_outbox();
            for m in out.as_array().unwrap() {
                let text = m["text"].as_str().unwrap();
                for j in 0..self.apps.len() {
                    let to_j = m["to"].is_null() || m["to"].as_str() == Some(&peer(j));
                    if j != i && to_j && self.linked[i] && self.linked[j] {
                        self.apps[j].net_event("message", &peer(i), text, false);
                    }
                }
            }
        }

        fn run(&mut self, ms: i64) {
            for _ in 0..ms / 20 {
                self.step();
            }
        }

        fn act(&mut self, i: usize, id: &str, value: &str) {
            self.apps[i].act(id, value, false, false);
            self.ui(i);
        }

        fn panels(&mut self, i: usize) -> String {
            self.ui(i)["panels"].to_string()
        }

        fn seats(&self, i: usize) -> usize {
            self.apps[i]
                .online
                .lobby
                .as_ref()
                .map(|l| l.seats().len())
                .unwrap_or(0)
        }
    }

    /// Host with the Default Match Settings (1v1), switch to Free-for-all,
    /// three more humans join, each by its own Player Name.
    fn four_in_a_lobby() -> Mesh {
        let mut m = Mesh::new(4);
        for (i, name) in ["Ann", "Bo", "Cy", "Di"].iter().enumerate() {
            m.act(i, "go.online", "");
            m.act(i, "online.name", name);
        }
        m.act(0, "online.host", "");
        assert_eq!(m.seats(0), 2, "a 1v1 lobby has two seats");
        m.act(0, "lobby.format", "free_for_all");
        assert_eq!(m.seats(0), 4, "Free-for-all gives the lobby four seats");
        m.link(0);
        for i in 1..4 {
            m.act(i, "online.join", "");
            m.link(i);
        }
        m.run(2_000);
        for i in 1..4 {
            assert_eq!(m.apps[i].screen, Screen::Lobby, "app {i} is in the lobby");
        }
        let host = m.panels(0);
        for name in ["Ann", "Bo", "Cy", "Di"] {
            assert!(host.contains(name), "the host's lobby shows {name}");
        }
        m
    }

    /// How many seats show Ready in their Status cell (the seat table's
    /// fourth column), matched whole: "Ready Up" holds the same word.
    fn ready_seats(m: &mut Mesh, i: usize) -> usize {
        let ui = m.ui(i);
        let mut n = 0;
        for p in ui["panels"].as_array().unwrap() {
            for item in p["items"].as_array().into_iter().flatten() {
                if item["type"] == "table" {
                    n += item["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|r| r[3] == "Ready")
                        .count();
                }
            }
        }
        n
    }

    fn start_disabled(m: &mut Mesh) -> bool {
        let ui = m.ui(0);
        let row = ui["panels"].as_array().unwrap().last().unwrap().clone();
        let start = row["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == "lobby.start")
            .cloned()
            .unwrap();
        start["disabled"] == Value::Bool(true)
    }

    #[test]
    fn four_humans_ready_up_and_start_from_a_1v1_default() {
        let mut m = four_in_a_lobby();
        assert!(start_disabled(&mut m), "Start waits for Ready Up");
        m.act(0, "lobby.start", "");
        assert_eq!(m.apps[0].screen, Screen::Lobby, "not every joiner is ready");
        for i in 1..4 {
            m.act(i, "lobby.ready", "");
        }
        m.run(1_000);
        assert!(ready_seats(&mut m, 0) == 3, "the host sees who is ready");
        // One takes it back, then readies again.
        m.act(2, "lobby.ready", "");
        m.run(1_000);
        assert!(start_disabled(&mut m));
        m.act(2, "lobby.ready", "");
        m.run(1_000);
        assert!(!start_disabled(&mut m));
        m.act(0, "lobby.start", "");
        m.run(2_000);
        for i in 0..4 {
            assert_eq!(m.apps[i].screen, Screen::Match, "app {i} plays");
        }
        m.run(3_000);
        let ticks: Vec<i32> = m
            .apps
            .iter()
            .map(|a| a.game.as_ref().unwrap().world.tick)
            .collect();
        assert!(ticks.iter().all(|t| *t > 20), "{ticks:?}");
    }

    #[test]
    fn the_host_sets_ai_difficulty_and_closes_seats_in_any_format() {
        let mut m = Mesh::new(2);
        m.act(0, "go.online", "");
        m.act(0, "online.host", "");
        m.act(0, "lobby.format", "two_v_two");
        m.act(0, "seat.1", "ai.2");
        m.act(0, "seat.2", "ai.0");
        m.act(0, "seat.3", "closed");
        let p = m.panels(0);
        assert!(
            p.contains("\"value\":\"ai.2\"") && p.contains("\"value\":\"ai.0\""),
            "{p}"
        );
        assert!(p.contains("\"value\":\"closed\""));
        m.act(0, "lobby.start", "");
        assert_eq!(m.apps[0].screen, Screen::Match);
        let w = &m.apps[0].game.as_ref().unwrap().world;
        use tool_core::settings::Difficulty;
        use tool_core::world::Controller;
        assert_eq!(w.colonies[1].controller, Controller::Ai(Difficulty::Imago));
        assert_eq!(w.colonies[2].controller, Controller::Ai(Difficulty::Larva));
        assert!(w.colonies[3].defeated, "a Closed seat is not played");
        assert!(!w.colonies[0].defeated);
    }

    #[test]
    fn status_is_true_after_leaving_and_joining_again() {
        let mut m = Mesh::new(2);
        m.act(0, "go.online", "");
        m.act(1, "go.online", "");
        assert!(m.panels(1).contains("Not Connected"));
        m.act(0, "online.host", "");
        m.link(0);
        m.act(1, "online.join", "");
        assert!(m.panels(1).contains("Connecting"));
        m.link(1);
        m.run(1_000);
        assert!(m.panels(1).contains("In Lobby"));
        // Leave: the transport is closed and Status says so.
        m.act(1, "lobby.leave", "");
        assert_eq!(m.apps[1].screen, Screen::Online);
        assert!(m.panels(1).contains("Not Connected"), "{}", m.panels(1));
        assert_eq!(m.ops[1].last().map(String::as_str), Some("leave"));
        m.run(500);
        let host_seat = m.apps[0].online.lobby.as_ref().unwrap().seats()[1]
            .kind
            .clone();
        assert_eq!(
            host_seat,
            tool_core::lockstep::SeatKind::Open,
            "the seat is free again"
        );
        // Join again: Connecting, then In Lobby.
        m.act(1, "online.join", "");
        assert!(m.panels(1).contains("Connecting"));
        m.link(1);
        m.run(1_000);
        assert!(m.panels(1).contains("In Lobby"), "{}", m.panels(1));
        // The host leaves: the joiner is told the lobby closed.
        m.act(0, "lobby.leave", "");
        m.run(500);
        assert_eq!(m.apps[1].screen, Screen::Online);
        assert!(m.panels(1).contains("Lobby Closed"), "{}", m.panels(1));
        // Host again; the joiner's link to the host just drops.
        m.act(0, "online.host", "");
        m.link(0);
        m.act(1, "online.join", "");
        m.link(1);
        m.run(1_000);
        assert!(m.panels(1).contains("In Lobby"));
        m.unlink(0);
        m.run(100);
        assert!(m.panels(1).contains("Connection Lost"), "{}", m.panels(1));
        assert_eq!(m.apps[1].screen, Screen::Online);
    }

    #[test]
    fn rematch_online_returns_everyone_to_the_lobby_with_ready_cleared() {
        let mut m = Mesh::new(2);
        m.act(0, "go.online", "");
        m.act(0, "online.host", "");
        m.link(0);
        m.act(1, "online.join", "");
        m.link(1);
        m.run(1_000);
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        m.act(0, "lobby.start", "");
        m.run(2_000);
        assert_eq!(m.apps[1].screen, Screen::Match);
        // The joiner concedes; the host wins and sees Rematch.
        m.apps[1]
            .game
            .as_mut()
            .unwrap()
            .issue(tool_core::world::Command::Concede);
        m.run(2_000);
        let end = m.ui(0)["match"]["end"].to_string();
        assert!(end.contains("end.rematch"), "{end}");
        let joiner_end = m.ui(1)["match"]["end"].to_string();
        assert!(
            !joiner_end.contains("end.rematch"),
            "only the host has Rematch"
        );
        m.act(0, "end.rematch", "");
        m.run(1_000);
        for i in 0..2 {
            assert_eq!(
                m.apps[i].screen,
                Screen::Lobby,
                "app {i} is back in the lobby"
            );
        }
        assert_eq!(ready_seats(&mut m, 0), 0, "Ready Up is cleared");
        assert!(start_disabled(&mut m));
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        m.act(0, "lobby.start", "");
        m.run(2_000);
        for i in 0..2 {
            assert_eq!(m.apps[i].screen, Screen::Match, "app {i} plays the rematch");
            assert!(m.apps[i].game.as_ref().unwrap().world.winner.is_none());
        }
    }

    #[test]
    fn rematch_against_the_ai_keeps_settings_and_seats_on_a_new_seed() {
        let mut a = app("me");
        a.act("go.play_vs_ai", "", false, false);
        a.act("pva.format", "free_for_all", false, false);
        let panels = a.ui()["panels"].to_string();
        for i in 1..4 {
            assert!(
                panels.contains(&format!("\"ai.{i}\"")),
                "seat {i} has a Difficulty"
            );
        }
        a.act("ai.2", "imago", false, false);
        a.act("ai.3", "larva", false, false);
        a.act("pva.seed", "77", false, false);
        a.act("pva.start", "", false, false);
        let first = a.game.as_ref().unwrap().world.seed;
        a.act("end.rematch", "", false, false);
        let w = &a.game.as_ref().unwrap().world;
        use tool_core::settings::{Difficulty, Format};
        use tool_core::world::Controller;
        assert_ne!(w.seed, first, "a new seed");
        assert_eq!(w.settings.format, Format::FreeForAll);
        assert_eq!(w.colonies[2].controller, Controller::Ai(Difficulty::Imago));
        assert_eq!(w.colonies[3].controller, Controller::Ai(Difficulty::Larva));
        assert_eq!(w.tick, 0);
        assert!(w.winner.is_none());
    }

    #[test]
    fn the_end_screen_offers_rematch_after_a_local_match() {
        let mut a = app("me");
        a.act("go.play_vs_ai", "", false, false);
        a.act("pva.start", "", false, false);
        a.game
            .as_mut()
            .unwrap()
            .issue(tool_core::world::Command::Concede);
        for t in 1..10 {
            a.advance(t * 200);
        }
        let end = a.ui()["match"]["end"].to_string();
        assert!(end.contains("end.rematch"), "{end}");
        a.act("end.rematch", "", false, false);
        assert_eq!(a.screen, Screen::Match);
        assert!(a.game.as_ref().unwrap().world.winner.is_none());
    }

    #[test]
    fn waiting_on_names_the_player() {
        let mut m = Mesh::new(2);
        m.act(1, "online.name", "Bo");
        m.act(0, "online.host", "");
        m.link(0);
        m.act(1, "online.join", "");
        m.link(1);
        m.run(1_000);
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        m.act(0, "lobby.start", "");
        m.run(1_000);
        // Bo goes quiet without leaving.
        m.linked[1] = false;
        for _ in 0..150 {
            m.now += 20;
            m.apps[0].advance(m.now);
        }
        let flags = m.ui(0)["match"]["hud"]["flags"].to_string();
        assert!(flags.contains("Waiting On: Bo"), "{flags}");
    }

    #[test]
    fn the_player_name_is_kept_in_player_settings() {
        let mut a = app("me");
        a.act("go.online", "", false, false);
        a.act("online.name", "Ann\u{7}e of the Long Name", false, false);
        assert_eq!(a.player.name, "Anne of the Long");
        let save = a.ui()["save"]["player"].as_str().unwrap().to_string();
        assert!(save.contains("Anne of the Long"), "{save}");
        let b = App::new(
            include_str!("../../../data/strings.json"),
            include_str!("../../../data/glossary.json"),
            "",
            &save,
            "x",
        )
        .unwrap();
        assert_eq!(b.player.name, "Anne of the Long");
    }

    #[test]
    fn copy_gives_the_page_the_room_code() {
        let mut a = app("me");
        a.act("go.online", "", false, false);
        a.act("online.room", "abc123", false, false);
        let p = a.ui()["panels"].to_string();
        assert!(p.contains("\"copy\":\"ABC123\""), "{p}");
        a.act("online.host", "", false, false);
        let p = a.ui()["panels"].to_string();
        assert!(p.contains("\"copy\":\"ABC123\""), "the Lobby has Copy too");
    }

    /// The Copy item (the Room Code and its button) in the panels.
    fn copy_item(a: &mut App) -> Value {
        let ui = a.ui();
        for p in ui["panels"].as_array().unwrap() {
            for item in p["items"].as_array().into_iter().flatten() {
                if item["type"] == "copy" {
                    return item.clone();
                }
            }
        }
        Value::Null
    }

    #[test]
    fn copy_sits_with_the_room_code_and_reads_copied_for_a_moment() {
        // TRIAGE row 87.
        let mut a = app("me");
        a.act("go.online", "", false, false);
        let c = copy_item(&mut a);
        assert_eq!(c["label"], "Room Code");
        assert_eq!(c["input"], "online.room", "the code field is in the group");
        assert_eq!(c["disabled"], true, "nothing to copy yet");
        a.act("online.room", "abc123", false, false);
        a.advance(10_000);
        a.act("online.copy", "", false, false);
        let c = copy_item(&mut a);
        assert_eq!(c["text"], "Copied", "{c}");
        assert_eq!(c["value"], "ABC123");
        a.advance(10_000 + tool_core::app::COPIED_MS);
        assert_eq!(copy_item(&mut a)["text"], "Copy");
        // In the Lobby, the code is shown as text beside Copy.
        a.act("online.host", "", false, false);
        let c = copy_item(&mut a);
        assert_eq!(c["value"], "ABC123");
        assert!(c["input"].is_null(), "{c}");
    }

    /// The seat table's Team cells: the select's value (0-based) or the
    /// text "Team N" as 0-based.
    fn lobby_teams(m: &mut Mesh, i: usize) -> Vec<u8> {
        let ui = m.ui(i);
        for p in ui["panels"].as_array().unwrap() {
            for item in p["items"].as_array().into_iter().flatten() {
                if item["type"] == "table" {
                    return item["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| match r[2].as_str() {
                            Some(t) => t.trim_start_matches("Team ").parse::<u8>().unwrap() - 1,
                            None => r[2]["value"].as_str().unwrap().parse().unwrap(),
                        })
                        .collect();
                }
            }
        }
        Vec::new()
    }

    #[test]
    fn lobby_teams_default_by_format_and_each_player_picks_freely() {
        // TRIAGE row 80.
        let mut m = Mesh::new(2);
        for i in 0..2 {
            m.act(i, "go.online", "");
        }
        m.act(0, "online.host", "");
        assert_eq!(lobby_teams(&mut m, 0), [0, 1], "1v1: Team 1 and Team 2");
        m.act(0, "lobby.format", "two_v_two");
        assert_eq!(lobby_teams(&mut m, 0), [0, 0, 1, 1], "2v2: 1, 1, 2, 2");
        m.act(0, "lobby.format", "free_for_all");
        assert_eq!(lobby_teams(&mut m, 0), [0, 1, 2, 3], "Free-for-all: 1-4");
        m.link(0);
        m.act(1, "online.join", "");
        m.link(1);
        m.run(1_000);
        // The host sets an AI seat's team; the joiner picks its own.
        m.act(0, "seat.2", "ai.2");
        m.act(0, "lobby.team.2", "3");
        m.act(1, "lobby.team.1", "3");
        m.run(1_000);
        assert_eq!(lobby_teams(&mut m, 0), [0, 3, 3, 3]);
        assert_eq!(lobby_teams(&mut m, 1), [0, 3, 3, 3], "the joiner sees it");
        // A joiner cannot set another seat's team.
        m.act(1, "lobby.team.0", "3");
        m.run(1_000);
        assert_eq!(lobby_teams(&mut m, 0), [0, 3, 3, 3]);
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        m.act(0, "lobby.start", "");
        m.run(2_000);
        for i in 0..2 {
            assert_eq!(m.apps[i].screen, Screen::Match, "app {i}");
            let teams: Vec<u8> = m.apps[i]
                .game
                .as_ref()
                .unwrap()
                .world
                .colonies
                .iter()
                .map(|c| c.team)
                .collect();
            assert_eq!(teams, [0, 3, 3, 3], "app {i} plays the Lobby's teams");
        }
    }

    #[test]
    fn the_lobby_refuses_start_while_every_colony_is_on_one_team() {
        // TRIAGE row 81: Start is refused, with "Teams: 1", until the seats
        // that are played make two teams.
        let mut m = Mesh::new(2);
        for i in 0..2 {
            m.act(i, "go.online", "");
        }
        m.act(0, "online.host", "");
        m.link(0);
        m.act(1, "online.join", "");
        m.link(1);
        m.run(1_000);
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        assert!(!start_disabled(&mut m));
        // The joiner asks for Team 1: both colonies on one team.
        m.act(1, "lobby.team.1", "0");
        m.run(1_000);
        assert_eq!(lobby_teams(&mut m, 0), [0, 0]);
        // The team change cleared Ready Up; ready again.
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        assert!(start_disabled(&mut m), "one team: Start is refused");
        let p = m.panels(0);
        assert!(p.contains("[\"Teams\",\"1\"]"), "{p}");
        m.act(0, "lobby.start", "");
        m.run(1_000);
        assert_eq!(m.apps[0].screen, Screen::Lobby, "no match starts");
        // Free-for-all with the other seats Closed: still one team.
        m.act(0, "lobby.format", "free_for_all");
        m.act(0, "lobby.team.1", "0");
        m.act(0, "seat.2", "closed");
        m.act(0, "seat.3", "closed");
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        assert!(start_disabled(&mut m), "Closed seats are not played");
        // An Open seat on another team is filled by a Pupa AI: two teams.
        m.act(0, "seat.3", "open");
        m.act(1, "lobby.ready", "");
        m.run(1_000);
        assert!(!start_disabled(&mut m));
        assert!(!m.panels(0).contains("[\"Teams\",\"1\"]"));
    }
}
