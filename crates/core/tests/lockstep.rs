//! Lockstep tests over the loopback network with a fake clock and a tiny
//! fake simulation: the state is a u64 that each tick folds in the tick's
//! commands and drops (FNV), and the checksum is the state.

use std::collections::BTreeMap;

use tool_core::lockstep::loopback::{Event, Loopback};
use tool_core::lockstep::{
    Lobby, LobbyStatus, Match, MatchStatus, Msg, Outgoing, RefuseReason, SeatKind, TickInput,
    Version, CHECKSUM_EVERY, MAX_MESSAGE_BYTES,
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

    fn host_starts(&mut self) {
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
        c.game = Some(game);
        c.started_at = now;
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
                host_proto: 1,
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
    out.extend(host.set_team(3, 7, now).unwrap());
    out.extend(host.set_team(0, 5, now).unwrap());
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
        .request_team(5, now)
        .unwrap();
    w.net.send(2, out);
    w.run_for(8_000);
    let host_seats = w.clients[0].lobby.as_ref().unwrap().seats().to_vec();
    assert_eq!(host_seats[3].kind, SeatKind::Ai { difficulty: 2 });
    assert_eq!(host_seats[3].team, 7);
    assert_eq!(host_seats[0].team, 5);
    let joiner_seat = host_seats
        .iter()
        .position(|s| s.client_id() == Some("c2"))
        .unwrap();
    assert_eq!(host_seats[joiner_seat].team, 5, "the joiner's team request");
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
