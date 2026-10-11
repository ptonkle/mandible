//! A simulated full-mesh network for tests, driven by a fake clock.
//!
//! Each directed link has a latency and a jitter; the jitter is drawn from a
//! small seeded generator, so a run repeats exactly. Messages on a link stay
//! in order, as on a reliable, ordered data channel. Lobby-kind messages
//! (see [`Msg::is_lobby_kind`]) may be lost at a chosen rate; match messages
//! never are. A link can be cut silently and a node can disconnect.

use std::collections::{BTreeMap, BTreeSet};

use super::wire::{Msg, Outgoing, PeerId, Target};

/// Something the network delivers to a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Connected {
        at: usize,
        peer: PeerId,
    },
    Disconnected {
        at: usize,
        peer: PeerId,
    },
    Message {
        at: usize,
        from: PeerId,
        text: String,
    },
}

impl Event {
    /// The node the event is for.
    pub fn node(&self) -> usize {
        match self {
            Event::Connected { at, .. }
            | Event::Disconnected { at, .. }
            | Event::Message { at, .. } => *at,
        }
    }
}

#[derive(Clone, Debug)]
struct Pending {
    from: usize,
    event: Event,
}

/// The simulated network of `count` nodes, named `peer0`, `peer1`, ...
#[derive(Clone, Debug)]
pub struct Loopback {
    count: usize,
    now_ms: i64,
    rng: u64,
    latency: BTreeMap<(usize, usize), (i64, i64)>,
    default_link: (i64, i64),
    loss_permille: u64,
    gone: BTreeSet<usize>,
    cut: BTreeSet<(usize, usize)>,
    one_shot_drops: Vec<(usize, usize, String)>,
    queue: BTreeMap<(i64, u64), Pending>,
    seq: u64,
    link_last: BTreeMap<(usize, usize), i64>,
    delivered: u64,
    largest: usize,
}

impl Loopback {
    /// A network with every link at `latency_ms` plus up to `jitter_ms`.
    pub fn new(count: usize, seed: u64, latency_ms: i64, jitter_ms: i64) -> Loopback {
        Loopback {
            count,
            now_ms: 0,
            rng: seed ^ 0x9E37_79B9_7F4A_7C15,
            latency: BTreeMap::new(),
            default_link: (latency_ms, jitter_ms),
            loss_permille: 0,
            gone: BTreeSet::new(),
            cut: BTreeSet::new(),
            one_shot_drops: Vec::new(),
            queue: BTreeMap::new(),
            seq: 0,
            link_last: BTreeMap::new(),
            delivered: 0,
            largest: 0,
        }
    }

    /// The transport name of node `i`.
    pub fn peer_id(i: usize) -> PeerId {
        PeerId(format!("peer{i}"))
    }

    /// The node behind a transport name.
    pub fn index_of(peer: &PeerId) -> Option<usize> {
        peer.0.strip_prefix("peer").and_then(|n| n.parse().ok())
    }

    /// The fake clock.
    pub fn now_ms(&self) -> i64 {
        self.now_ms
    }

    /// Messages delivered so far.
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// The longest message text sent so far, in bytes.
    pub fn largest_message(&self) -> usize {
        self.largest
    }

    /// Sets the latency and jitter of the links between `a` and `b`.
    pub fn set_link(&mut self, a: usize, b: usize, latency_ms: i64, jitter_ms: i64) {
        self.latency.insert((a, b), (latency_ms, jitter_ms));
        self.latency.insert((b, a), (latency_ms, jitter_ms));
    }

    /// Loses this many lobby-kind messages per thousand.
    pub fn set_lobby_loss(&mut self, permille: u64) {
        self.loss_permille = permille;
    }

    /// Loses the next message from `from` to `to` whose text contains `needle`.
    pub fn drop_next(&mut self, from: usize, to: usize, needle: &str) {
        self.one_shot_drops.push((from, to, needle.to_string()));
    }

    /// Silently stops delivery from `from` to `to`, including what is in
    /// flight. `to` receives a prefix of what `from` sent, as on an ordered
    /// channel that dies.
    pub fn cut(&mut self, from: usize, to: usize) {
        self.cut.insert((from, to));
        self.queue
            .retain(|_, p| !(p.from == from && p.event.node() == to));
    }

    /// Node `i` leaves: what it already sent still arrives (unless cut), then
    /// every other node sees it disconnect. Nothing more goes to or from it.
    pub fn disconnect(&mut self, i: usize) {
        if !self.gone.insert(i) {
            return;
        }
        self.queue.retain(|_, p| p.event.node() != i);
        for j in 0..self.count {
            if j == i || self.gone.contains(&j) {
                continue;
            }
            let at = self.arrival(i, j);
            self.push(
                at,
                i,
                Event::Disconnected {
                    at: j,
                    peer: Loopback::peer_id(i),
                },
            );
        }
    }

    /// Every node sees every other connect, now.
    pub fn connect_all(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        for a in 0..self.count {
            for b in 0..self.count {
                if a != b {
                    out.push(Event::Connected {
                        at: a,
                        peer: Loopback::peer_id(b),
                    });
                }
            }
        }
        out
    }

    fn next_random(&mut self) -> u64 {
        // SplitMix64.
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn arrival(&mut self, from: usize, to: usize) -> i64 {
        let (lat, jit) = self
            .latency
            .get(&(from, to))
            .copied()
            .unwrap_or(self.default_link);
        let jitter = if jit > 0 {
            (self.next_random() % (jit as u64 + 1)) as i64
        } else {
            0
        };
        let earliest = self.now_ms + lat + jitter;
        let last = self.link_last.get(&(from, to)).copied().unwrap_or(0);
        let at = earliest.max(last);
        self.link_last.insert((from, to), at);
        at
    }

    fn push(&mut self, at: i64, from: usize, event: Event) {
        self.seq += 1;
        self.queue.insert((at, self.seq), Pending { from, event });
    }

    /// Sends node `from`'s outgoing messages.
    pub fn send(&mut self, from: usize, outgoing: Vec<Outgoing>) {
        if self.gone.contains(&from) {
            return;
        }
        for out in outgoing {
            self.largest = self.largest.max(out.text.len());
            let targets: Vec<usize> = match &out.to {
                Target::All => (0..self.count).filter(|j| *j != from).collect(),
                Target::Peer(p) => Loopback::index_of(p).into_iter().collect(),
            };
            let lossy = Msg::decode(&out.text).is_some_and(|m| m.is_lobby_kind());
            for to in targets {
                if to >= self.count || self.gone.contains(&to) || self.cut.contains(&(from, to)) {
                    continue;
                }
                if let Some(k) = self
                    .one_shot_drops
                    .iter()
                    .position(|(f, t, n)| *f == from && *t == to && out.text.contains(n.as_str()))
                {
                    self.one_shot_drops.remove(k);
                    continue;
                }
                if lossy && self.loss_permille > 0 && self.next_random() % 1000 < self.loss_permille
                {
                    continue;
                }
                let at = self.arrival(from, to);
                self.push(
                    at,
                    from,
                    Event::Message {
                        at: to,
                        from: Loopback::peer_id(from),
                        text: out.text.clone(),
                    },
                );
            }
        }
    }

    /// Moves the clock to `now_ms` and returns every event due by then, in
    /// order of arrival.
    pub fn advance(&mut self, now_ms: i64) -> Vec<Event> {
        self.now_ms = self.now_ms.max(now_ms);
        let later = self.queue.split_off(&(self.now_ms + 1, 0));
        let due = std::mem::replace(&mut self.queue, later);
        self.delivered += due.len() as u64;
        due.into_values().map(|p| p.event).collect()
    }
}
