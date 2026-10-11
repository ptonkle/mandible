//! Ant behaviour, one tick at a time. Gatherers and Builders run themselves;
//! combat ants follow their species' waypoints and fight what they meet on
//! the way (attack-move); Retreat pulls them home. No ant is ever selected.

use std::collections::BTreeMap;

use crate::fixed::{heading16, isqrt, Pos, CELL};
use crate::map::Ground;
use crate::nav::{downhill, FieldKey, Mode, Terrain, NO_WALL, UNREACHED};
use crate::rules::{damage_taken, ticks, Resource, Role, Species, StructureKind, SATURATION};
use crate::world::{
    drop_reach, pickup_reach, Ant, Effect, EffectKind, Id, Job, Target, World, INCOME_TICKS,
};

/// Spatial buckets rebuilt every tick: indices into `ants` and `structures`.
#[derive(Default)]
pub struct Buckets {
    cols: i32,
    rows: i32,
    ants: Vec<Vec<u32>>,
    structures: Vec<Vec<u32>>,
}

const BUCKET: i32 = 4 * CELL;

impl Buckets {
    fn rebuild(&mut self, w: &World) {
        self.cols = w.map.w * CELL / BUCKET + 1;
        self.rows = w.map.h * CELL / BUCKET + 1;
        let n = (self.cols * self.rows) as usize;
        self.ants = vec![Vec::new(); n];
        self.structures = vec![Vec::new(); n];
        for (i, a) in w.ants.iter().enumerate() {
            let b = self.slot(a.pos);
            self.ants[b].push(i as u32);
        }
        for (i, s) in w.structures.iter().enumerate() {
            let b = self.slot(s.pos);
            self.structures[b].push(i as u32);
        }
    }

    fn slot(&self, p: Pos) -> usize {
        let bx = (p.x / BUCKET).clamp(0, self.cols - 1);
        let by = (p.y / BUCKET).clamp(0, self.rows - 1);
        (by * self.cols + bx) as usize
    }

    /// Bucket indices covering a square around `p`.
    fn around(&self, p: Pos, r: i32) -> Vec<usize> {
        let x0 = ((p.x - r) / BUCKET).clamp(0, self.cols - 1);
        let x1 = ((p.x + r) / BUCKET).clamp(0, self.cols - 1);
        let y0 = ((p.y - r) / BUCKET).clamp(0, self.rows - 1);
        let y1 = ((p.y + r) / BUCKET).clamp(0, self.rows - 1);
        let mut v = Vec::new();
        for by in y0..=y1 {
            for bx in x0..=x1 {
                v.push((by * self.cols + bx) as usize);
            }
        }
        v
    }
}

enum Step {
    Arrived,
    Moved,
    Unreachable,
    /// The way on is dirt or an enemy wall at this cell.
    Blocked(i32, i32),
}

/// A small per-ant offset so a crowd does not walk in single file.
fn jitter(id: Id) -> (i32, i32) {
    let a = (id.wrapping_mul(2_654_435_761) >> 8) as i32;
    ((a % 129) - 64, ((a / 129) % 129) - 64)
}

impl World {
    pub(crate) fn run_ants(&mut self) {
        for a in self.ants.iter_mut() {
            a.prev = a.pos;
        }
        let mut grid = std::mem::take(&mut self.grid);
        grid.rebuild(self);
        self.grid = grid;

        // Who is working where, for saturation and builder spread.
        let mut node_load: BTreeMap<Id, usize> = BTreeMap::new();
        let mut bp_load: BTreeMap<Id, usize> = BTreeMap::new();
        let mut res_load: Vec<[i32; 3]> = vec![[0; 3]; self.colonies.len()];
        for a in &self.ants {
            match a.job {
                Job::Gather(n) => {
                    *node_load.entry(n).or_insert(0) += 1;
                    if let Some(ni) = self.node_index(n) {
                        res_load[a.colony as usize][self.nodes[ni].resource.index()] += 1;
                    }
                }
                Job::Fetch(b) | Job::Haul(b) | Job::Build(b) => *bp_load.entry(b).or_insert(0) += 1,
                _ => {}
            }
        }
        // Waypoint assignment: each species' ants split evenly, by id order.
        let mut ordinal: BTreeMap<(u8, usize), usize> = BTreeMap::new();
        // Idle Carpenter Ants so far this tick, per colony, for spreading
        // them round the queens.
        let mut idle_builders: BTreeMap<u8, usize> = BTreeMap::new();
        let scouts = self.pick_scouts();

        for i in 0..self.ants.len() {
            let mut a = self.ants[i];
            if a.health <= 0 || a.spent {
                continue;
            }
            a.cooldown = (a.cooldown - 1).max(0);
            match a.species.stats().role {
                Role::Gatherer => self.gatherer(&mut a, &mut node_load, &mut res_load),
                Role::Builder => self.builder(&mut a, &mut bp_load, &mut idle_builders),
                _ if scouts[a.colony as usize] == Some(a.id) => self.scout(&mut a),
                _ => {
                    let k = (a.colony, a.species.index());
                    let n = ordinal.entry(k).or_insert(0);
                    let my = *n;
                    *n += 1;
                    self.fighter(&mut a, my);
                }
            }
            if let Some(h) = heading16(a.pos.x - a.prev.x, a.pos.y - a.prev.y) {
                a.heading = h;
            }
            self.ants[i] = a;
        }
        self.separate();
        self.bury();
    }

    /// Each colony's team, indexed by colony, for `Terrain::teams`.
    pub(crate) fn team_table(&self) -> [u8; 8] {
        let mut t = [0u8; 8];
        for (i, slot) in t.iter_mut().enumerate() {
            *slot = self
                .colonies
                .get(i)
                .map(|c| c.team)
                .unwrap_or(100 + i as u8);
        }
        t
    }

    fn mode_of(s: Species) -> Mode {
        if s.flies() {
            Mode::Fly
        } else if s.is_worker() {
            Mode::Walk
        } else {
            Mode::Dig
        }
    }

    fn standable(&self, a: &Ant, p: Pos) -> bool {
        let (x, y) = p.cell();
        let teams = self.team_table();
        let t = Terrain {
            w: self.map.w,
            h: self.map.h,
            ground: &self.map.ground,
            wall: &self.wall,
            teams: &teams,
        };
        t.standable(x, y, a.colony, World::mode_of(a.species))
    }

    /// Moves by at most `speed` toward `to`, sliding along obstacles.
    fn step_toward(&self, a: &mut Ant, to: Pos, speed: i32) -> bool {
        let next = a.pos.toward(to, speed);
        for cand in [next, Pos::new(next.x, a.pos.y), Pos::new(a.pos.x, next.y)] {
            if cand != a.pos && self.standable(a, cand) {
                a.pos = cand;
                return true;
            }
        }
        // Stuck inside something (a wall finished on top of it): step out.
        if !self.standable(a, a.pos) {
            a.pos = next;
        }
        false
    }

    fn speed_of(&self, a: &Ant) -> i32 {
        let st = self.colonies[a.colony as usize].ranks.apply(a.species);
        if a.carried() > 0 {
            st.speed * st.laden_pct / 100
        } else {
            st.speed
        }
    }

    /// One step along the flow field for `key`.
    fn follow(&mut self, a: &mut Ant, key: FieldKey) -> Step {
        let (w, h) = (self.map.w, self.map.h);
        let (cx, cy) = a.pos.cell();
        let teams = self.team_table();
        let t = Terrain {
            w,
            h,
            ground: &self.map.ground,
            wall: &self.wall,
            teams: &teams,
        };
        let f = self.nav.field(&t, key);
        let here = f.at(w, cx, cy);
        if here == UNREACHED {
            // Standing somewhere the field never reached (pushed into a
            // corner): take any neighbour that is reached.
            match downhill(f, w, h, cx, cy) {
                Some((nx, ny)) if (nx, ny) != (cx, cy) => {
                    let to = Pos::of_cell(nx, ny);
                    let sp = self.speed_of(a);
                    self.step_toward(a, to, sp);
                    return Step::Moved;
                }
                _ => return Step::Unreachable,
            }
        }
        if here == 0 {
            return Step::Arrived;
        }
        let Some(mut s1) = downhill(f, w, h, cx, cy) else {
            return Step::Unreachable;
        };
        let mode = World::mode_of(a.species);
        // Never step diagonally past a closed corner: the straight move
        // would clip it and the ant would freeze. Take the open side step.
        let (dx, dy) = (s1.0 - cx, s1.1 - cy);
        if dx != 0 && dy != 0 {
            let side_x = t.standable(cx + dx, cy, a.colony, mode);
            let side_y = t.standable(cx, cy + dy, a.colony, mode);
            if !(side_x && side_y) {
                let mut sides = Vec::new();
                if side_x {
                    sides.push((f.at(w, cx + dx, cy), (cx + dx, cy)));
                }
                if side_y {
                    sides.push((f.at(w, cx, cy + dy), (cx, cy + dy)));
                }
                if let Some((_, c)) = sides.into_iter().min() {
                    s1 = c;
                }
            }
        }
        let s2 = downhill(f, w, h, s1.0, s1.1).unwrap_or(s1);
        if !t.standable(s1.0, s1.1, a.colony, mode) {
            return Step::Blocked(s1.0, s1.1);
        }
        let aim_cell = if t.standable(s2.0, s2.1, a.colony, mode) {
            s2
        } else {
            s1
        };
        let (jx, jy) = jitter(a.id);
        let aim = Pos::new(
            aim_cell.0 * CELL + CELL / 2 + jx,
            aim_cell.1 * CELL + CELL / 2 + jy,
        );
        let sp = self.speed_of(a);
        if !self.step_toward(a, aim, sp) {
            let c = Pos::of_cell(s1.0, s1.1);
            if !self.step_toward(a, c, sp) {
                // Wedged against a corner: back to the middle of this cell,
                // from where the next step is clear.
                self.step_toward(a, Pos::of_cell(cx, cy), sp);
            }
        }
        Step::Moved
    }

    fn key_for(a: &Ant, x: i32, y: i32, r: i32) -> FieldKey {
        FieldKey {
            colony: a.colony,
            mode: World::mode_of(a.species),
            x,
            y,
            r,
        }
    }

    fn queen_key(&self, a: &Ant) -> Option<(FieldKey, Pos, i32)> {
        let q = self.nearest_queen(a.colony, a.pos)?;
        let (x, y) = q.pos.cell();
        let r = q.radius() / CELL + 1;
        Some((World::key_for(a, x, y, r), q.pos, q.radius()))
    }

    // ---- gatherers ----

    fn gatherer(
        &mut self,
        a: &mut Ant,
        node_load: &mut BTreeMap<Id, usize>,
        res_load: &mut [[i32; 3]],
    ) {
        let st = self.colonies[a.colony as usize].ranks.apply(a.species);
        if a.carried() >= st.carry {
            a.job = Job::Deliver;
        }
        if a.job == Job::Deliver {
            let Some((key, qpos, qr)) = self.queen_key(a) else {
                return;
            };
            let step = self.follow(a, key);
            if matches!(step, Step::Arrived) || a.pos.dist(qpos) <= qr + st.radius + CELL {
                let ci = a.colony as usize;
                let slot = self.tick as usize % INCOME_TICKS;
                for r in 0..3 {
                    self.colonies[ci].stock[r] += a.inventory[r];
                    self.colonies[ci].income_ring[slot][r] += a.inventory[r];
                    self.colonies[ci].tally.gathered[r] += a.inventory[r];
                }
                a.inventory = [0; 3];
                a.job = Job::Idle;
            }
            return;
        }
        if let Job::Gather(nid) = a.job {
            let Some(ni) = self.node_index(nid) else {
                a.job = if a.carried() > 0 {
                    Job::Deliver
                } else {
                    Job::Idle
                };
                return;
            };
            if self.nodes[ni].amount <= 0 {
                a.job = if a.carried() > 0 {
                    Job::Deliver
                } else {
                    Job::Idle
                };
                return;
            }
            let npos = self.nodes[ni].pos;
            let mut in_range = a.pos.dist(npos) <= CELL * 3 / 2 + st.radius;
            if !in_range {
                let (x, y) = npos.cell();
                let key = World::key_for(a, x, y, 1);
                match self.follow(a, key) {
                    // The field's goal area is the node's neighbourhood.
                    Step::Arrived => in_range = true,
                    Step::Unreachable => {
                        a.job = Job::Idle;
                        return;
                    }
                    _ => {}
                }
            }
            if in_range {
                a.gather_acc += 10;
                let res = self.nodes[ni].resource.index();
                while a.gather_acc >= st.gather_ticks.max(1)
                    && a.carried() < st.carry
                    && self.nodes[ni].amount > 0
                {
                    a.gather_acc -= st.gather_ticks.max(1);
                    self.nodes[ni].amount -= 1;
                    a.inventory[res] += 1;
                }
                if a.carried() >= st.carry || self.nodes[ni].amount <= 0 {
                    a.job = Job::Deliver;
                    a.gather_acc = 0;
                }
            }
            return;
        }
        // Idle: choose a node by Gather Focus, nearness and saturation.
        if let Some(nid) = self.choose_node(a, node_load, res_load) {
            *node_load.entry(nid).or_insert(0) += 1;
            if let Some(ni) = self.node_index(nid) {
                res_load[a.colony as usize][self.nodes[ni].resource.index()] += 1;
            }
            a.job = Job::Gather(nid);
        } else if a.carried() > 0 {
            a.job = Job::Deliver;
        } else if let Some((key, _, _)) = self.queen_key(a) {
            self.follow(a, key);
        }
    }

    fn choose_node(
        &mut self,
        a: &Ant,
        node_load: &BTreeMap<Id, usize>,
        res_load: &[[i32; 3]],
    ) -> Option<Id> {
        let st = a.species.stats();
        let ci = a.colony as usize;
        let weights = self.colonies[ci].focus.weights();
        let load = res_load[ci];
        let mut order: Vec<Resource> = match st.only_gathers {
            Some(r) => vec![r],
            None => Resource::ALL.to_vec(),
        };
        // Most wanted first: highest weight per gatherer already on it.
        order.sort_by(|x, y| {
            let lx = (weights[x.index()] as i64) * (load[y.index()] as i64 + 1);
            let ly = (weights[y.index()] as i64) * (load[x.index()] as i64 + 1);
            ly.cmp(&lx).then(x.cmp(y))
        });
        let explored = self.team_explored(a.colony);
        for res in order {
            let mut cands: Vec<(i64, Id, Pos)> = self
                .nodes
                .iter()
                .filter(|n| {
                    n.resource == res
                        && n.amount > 0
                        && node_load.get(&n.id).copied().unwrap_or(0) < SATURATION
                        && {
                            let (x, y) = n.pos.cell();
                            explored.get(self.map.idx(x, y))
                        }
                })
                .map(|n| (n.pos.dist2(a.pos), n.id, n.pos))
                .collect();
            cands.sort();
            for (_, id, pos) in cands.into_iter().take(3) {
                let (x, y) = pos.cell();
                let key = World::key_for(a, x, y, 1);
                let teams = self.team_table();
                let t = Terrain {
                    w: self.map.w,
                    h: self.map.h,
                    ground: &self.map.ground,
                    wall: &self.wall,
                    teams: &teams,
                };
                let (cx, cy) = a.pos.cell();
                if self.nav.field(&t, key).at(self.map.w, cx, cy) != UNREACHED {
                    return Some(id);
                }
            }
        }
        None
    }

    // ---- builders ----

    fn builder(
        &mut self,
        a: &mut Ant,
        bp_load: &mut BTreeMap<Id, usize>,
        idle: &mut BTreeMap<u8, usize>,
    ) {
        let st = self.colonies[a.colony as usize].ranks.apply(a.species);
        let bid = match a.job {
            Job::Fetch(b) | Job::Haul(b) | Job::Build(b) => Some(b),
            _ => None,
        };
        if let Some(b) = bid {
            let Some(si) = self.structure_index(b) else {
                a.job = Job::Idle;
                a.inventory = [0; 3];
                return;
            };
            if self.structures[si].complete {
                a.job = Job::Idle;
                return;
            }
        }
        match a.job {
            Job::Fetch(b) => {
                let si = self.structure_index(b).expect("checked");
                let need = self.structures[si].still_needed();
                if need.iter().sum::<i32>() == 0 {
                    a.job = Job::Build(b);
                    return;
                }
                let Some((key, qpos, qr)) = self.queen_key(a) else {
                    return;
                };
                // The goal area lies inside the pickup reach, which
                // placement keeps apart from every blueprint's site.
                let key = FieldKey {
                    r: qr / CELL,
                    ..key
                };
                let step = self.follow(a, key);
                if matches!(step, Step::Arrived) || a.pos.dist(qpos) <= pickup_reach(qr) {
                    let mut room = st.carry;
                    let mut take = [0; 3];
                    for r in 0..3 {
                        let t = need[r].min(room);
                        take[r] = t;
                        room -= t;
                    }
                    for r in 0..3 {
                        self.structures[si].promised[r] += take[r];
                    }
                    a.inventory = take;
                    a.job = Job::Haul(b);
                }
            }
            Job::Haul(b) => {
                let si = self.structure_index(b).expect("checked");
                let (x, y, r) = self.structures[si].site_goal(a.pos);
                let key = World::key_for(a, x, y, r);
                let step = self.follow(a, key);
                let si = self.structure_index(b).expect("checked");
                if matches!(step, Step::Arrived)
                    || self.structures[si].site_dist(a.pos) <= drop_reach()
                {
                    let s = &mut self.structures[si];
                    for r in 0..3 {
                        s.delivered[r] += a.inventory[r];
                        s.promised[r] = (s.promised[r] - a.inventory[r]).max(0);
                    }
                    a.inventory = [0; 3];
                    // Now apply what was just brought.
                    a.job = Job::Build(b);
                }
            }
            Job::Build(b) => {
                // Apply delivered Leaves as build work, a little each tick,
                // filling the build bar; when nothing delivered is left to
                // apply, fetch more instead of waiting.
                let si = self.structure_index(b).expect("checked");
                let s = &self.structures[si];
                let (x, y, r) = s.site_goal(a.pos);
                let key = World::key_for(a, x, y, r);
                let step = self.follow(a, key);
                let si = self.structure_index(b).expect("checked");
                let close = matches!(step, Step::Arrived)
                    || self.structures[si].site_dist(a.pos) <= drop_reach();
                if !close {
                    if matches!(step, Step::Unreachable) {
                        a.job = Job::Idle;
                    }
                    return;
                }
                let s = &mut self.structures[si];
                let covered = s.covered_work();
                if s.work_done < covered {
                    s.work_done = (s.work_done + st.build).min(covered);
                    // Health grows with the work, as the structure rises.
                    s.health = (s.max_health * s.work_done / s.work.max(1)).max(s.health);
                }
                if s.work_done >= s.work && s.fully_delivered() {
                    self.complete(si);
                    a.job = Job::Idle;
                } else if s.work_done >= covered {
                    a.job = if s.still_needed().iter().sum::<i32>() > 0 {
                        Job::Fetch(b)
                    } else {
                        // Others carry the rest: find other work.
                        Job::Idle
                    };
                }
            }
            _ => {
                // Idle: the blueprint with the fewest builders, oldest first.
                let pick = self
                    .structures
                    .iter()
                    .filter(|s| s.colony == a.colony && !s.complete && s.has_builder_work())
                    .map(|s| (bp_load.get(&s.id).copied().unwrap_or(0), s.id))
                    .min();
                if let Some((_, b)) = pick {
                    *bp_load.entry(b).or_insert(0) += 1;
                    a.job = Job::Fetch(b);
                } else {
                    let n = idle.entry(a.colony).or_insert(0);
                    let k = *n;
                    *n += 1;
                    if let Some(key) = self.waiting_spot(a, k) {
                        self.follow(a, key);
                    }
                }
            }
        }
    }

    /// Where the `k`-th idle Carpenter Ant of a colony waits (TRIAGE row
    /// 54): idle Carpenter Ants take the queens in turn, so each queen gets
    /// an even share, and each one at a queen has its own spot on a ring
    /// round it, so they do not stand in one clump.
    fn waiting_spot(&self, a: &Ant, k: usize) -> Option<FieldKey> {
        let queens: Vec<(Pos, i32)> = self.queens(a.colony).map(|q| (q.pos, q.radius())).collect();
        if queens.is_empty() {
            return None;
        }
        let (qpos, qr) = queens[k % queens.len()];
        let j = k / queens.len();
        // Eight spots a ring, spread round the queen; further rings beyond.
        let ring = (j / 8) as i32;
        let d = qr / CELL + 3 + ring * 2;
        let base = [0usize, 32, 16, 48, 8, 40, 24, 56][j % 8] + 4 * (ring as usize % 2);
        let (qx, qy) = qpos.cell();
        let teams = self.team_table();
        let t = Terrain {
            w: self.map.w,
            h: self.map.h,
            ground: &self.map.ground,
            wall: &self.wall,
            teams: &teams,
        };
        // The first open spot from that angle on round the ring.
        for step in 0..16 {
            let ang = (base + step * 4) % 64;
            let x = qx + d * crate::fixed::cos64(ang) / crate::fixed::TRIG_ONE;
            let y = qy + d * crate::fixed::sin64(ang) / crate::fixed::TRIG_ONE;
            if t.standable(x, y, a.colony, Mode::Walk) && self.wall[self.map.idx(x, y)] == NO_WALL {
                return Some(World::key_for(a, x, y, 1));
            }
        }
        let (x, y) = qpos.cell();
        Some(World::key_for(a, x, y, qr / CELL + 3))
    }

    // ---- combat ants ----

    fn target_pos(&self, t: Target) -> Option<(Pos, i32, u8)> {
        match t {
            Target::Ant(id) => self
                .ant(id)
                .filter(|o| o.health > 0 && !o.spent)
                .map(|o| (o.pos, o.species.stats().radius, o.colony)),
            Target::Structure(id) => self
                .structure(id)
                .filter(|s| s.health > 0)
                .map(|s| (s.pos, s.radius(), s.colony)),
            Target::Dirt(x, y) => {
                (self.map.at(x, y) == Ground::Dirt).then(|| (Pos::of_cell(x, y), CELL / 2, u8::MAX))
            }
        }
    }

    fn find_target(&self, a: &Ant, aggro: i32) -> Option<Target> {
        let me = a.colony;
        let mut best: Option<(i64, Target)> = None;
        for b in self.grid.around(a.pos, aggro) {
            for &j in &self.grid.ants[b] {
                let o = &self.ants[j as usize];
                if o.health <= 0 || o.spent || self.allied(o.colony, me) {
                    continue;
                }
                let d = o.pos.dist2(a.pos);
                if d <= (aggro as i64).pow(2) && best.map(|x| d < x.0).unwrap_or(true) {
                    best = Some((d, Target::Ant(o.id)));
                }
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        for b in self.grid.around(a.pos, aggro + 3 * CELL) {
            for &j in &self.grid.structures[b] {
                let Some(s) = self.structures.get(j as usize) else {
                    continue;
                };
                if s.health <= 0 || self.allied(s.colony, me) {
                    continue;
                }
                let d = s.pos.dist2(a.pos);
                let reach = (aggro + s.radius()) as i64;
                if d <= reach * reach && best.map(|x| d < x.0).unwrap_or(true) {
                    best = Some((d, Target::Structure(s.id)));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Whether a straight line between two points crosses rock, dirt or a
    /// wall (other than at the target's own cell).
    pub fn clear_line(&self, from: Pos, to: Pos) -> bool {
        let (x0, y0) = from.cell();
        let (x1, y1) = to.cell();
        let cells = crate::fog::line(x0, y0, x1, y1);
        let n = cells.len();
        cells
            .into_iter()
            .take(n.saturating_sub(1))
            .skip(1)
            .all(|(x, y)| self.map.is_floor(x, y) && self.wall[self.map.idx(x, y)] == NO_WALL)
    }

    fn fighter(&mut self, a: &mut Ant, ordinal: usize) {
        let ci = a.colony as usize;
        let st = self.colonies[ci].ranks.apply(a.species);
        let aggro = st.vision * CELL;

        let retreat = self.colonies[ci].retreat;
        if retreat {
            // Home by walking where possible: a retreat never stops to dig
            // or break a wall unless that is the only way back, and then it
            // digs (below) instead of standing still.
            let blocking = match a.target {
                Some(Target::Dirt(x, y)) => a.pos.dist(Pos::of_cell(x, y)) <= 2 * CELL,
                Some(Target::Structure(id)) => self.structure(id).is_some_and(|s| {
                    s.kind == StructureKind::Wall && s.pos.dist(a.pos) <= 2 * CELL
                }),
                _ => false,
            };
            if !blocking {
                a.target = None;
            }
            if a.target.is_none() {
                let Some((key, _, _)) = self.queen_key(a) else {
                    return;
                };
                let home = FieldKey {
                    r: key.r + 3,
                    ..key
                };
                let walk = FieldKey {
                    mode: if a.species.flies() {
                        Mode::Fly
                    } else {
                        Mode::Walk
                    },
                    ..home
                };
                let step = if self.reaches(a, walk) {
                    self.follow(a, walk)
                } else {
                    self.follow(a, home)
                };
                if let Step::Blocked(x, y) = step {
                    a.target = self.blocker(a, x, y);
                }
                if a.target.is_none() {
                    return;
                }
            }
        }

        if st.heal > 0 && !retreat {
            self.nurse(a, &st);
            return;
        }

        // Keep a target only while it is real and not too far.
        if let Some(t) = a.target {
            match self.target_pos(t) {
                Some((p, _, _)) if p.dist(a.pos) <= aggro * 3 / 2 => {}
                _ => a.target = None,
            }
        }
        if !retreat && (a.target.is_none() || (self.tick + a.id as i32) % ticks(5) == 0) {
            if let Some(t) = self.find_target(a, aggro) {
                // Keep digging unless an enemy shows up.
                let digging = matches!(a.target, Some(Target::Dirt(..)));
                if !digging || matches!(t, Target::Ant(_)) {
                    a.target = Some(t);
                }
            }
        }

        if let Some(t) = a.target {
            if let Some((tp, tr, _)) = self.target_pos(t) {
                let reach = st.radius + tr + st.range;
                let d = a.pos.dist(tp);
                let ranged = st.range > CELL;
                if d <= reach && (!ranged || self.clear_line(a.pos, tp)) {
                    if a.cooldown == 0 {
                        self.strike(a, t, &st);
                        a.cooldown = st.attack_ticks;
                    }
                    return;
                }
                let sp = self.speed_of(a);
                let before = a.pos;
                self.step_toward(a, tp, sp);
                if a.pos == before {
                    // Cannot get closer in a straight line: forget it.
                    if !matches!(t, Target::Dirt(..)) {
                        a.target = None;
                    }
                }
                return;
            }
            a.target = None;
        }

        // No target: go to this ant's waypoint, or wait at the nearest nest.
        let wps = &self.colonies[ci].waypoints[a.species.index()];
        let step = if !wps.is_empty() {
            let wp = wps[ordinal % wps.len()];
            let share = (self
                .ants
                .iter()
                .filter(|o| o.colony == a.colony && o.species == a.species)
                .count()
                / wps.len())
            .max(1);
            let r = 2 + (isqrt(share as u64 / 2) as i32 / 2) * 2;
            let (x, y) = wp.cell();
            let key = World::key_for(a, x, y, r);
            self.follow(a, key)
        } else if let Some((key, _, _)) = self.queen_key(a) {
            let k = FieldKey {
                r: key.r + 4,
                ..key
            };
            self.follow(a, k)
        } else {
            Step::Arrived
        };
        if let Step::Blocked(x, y) = step {
            a.target = self.blocker(a, x, y);
        }
    }

    /// What an ant must dig or break to get past (x, y): dirt, or a wall
    /// that is not its team's.
    fn blocker(&self, a: &Ant, x: i32, y: i32) -> Option<Target> {
        if self.map.at(x, y) == Ground::Dirt {
            return Some(Target::Dirt(x, y));
        }
        self.structures
            .iter()
            .find(|s| {
                s.kind == StructureKind::Wall
                    && s.complete
                    && !self.allied(s.colony, a.colony)
                    && s.cells.first() == Some(&(x, y))
            })
            .map(|s| Target::Structure(s.id))
    }

    /// Whether the field for `key` reaches the ant's cell.
    fn reaches(&mut self, a: &Ant, key: FieldKey) -> bool {
        let teams = self.team_table();
        let t = Terrain {
            w: self.map.w,
            h: self.map.h,
            ground: &self.map.ground,
            wall: &self.wall,
            teams: &teams,
        };
        let (cx, cy) = a.pos.cell();
        self.nav.field(&t, key).at(self.map.w, cx, cy) != UNREACHED
    }

    // ---- Auto-Scout ----

    /// Keeps exactly one scout for every colony with Auto-Scout on: the
    /// current one while it lives, else the best candidate (Fire Ant, then
    /// Soldier Ant, then the fastest other combat ant; lowest id first).
    fn pick_scouts(&mut self) -> Vec<Option<Id>> {
        let mut out = vec![None; self.colonies.len()];
        for (ci, slot) in out.iter_mut().enumerate() {
            let c = &self.colonies[ci];
            if !c.auto_scout || c.defeated {
                continue;
            }
            let alive = |id: Id| {
                self.ant(id)
                    .is_some_and(|a| a.colony as usize == ci && a.health > 0 && !a.spent)
            };
            let cur = c.scout.filter(|&id| alive(id));
            let pick = cur.or_else(|| {
                self.ants
                    .iter()
                    .filter(|a| a.colony as usize == ci && a.species.is_combat() && a.health > 0)
                    .min_by_key(|a| (scout_rank(a.species), a.id))
                    .map(|a| a.id)
            });
            if pick != self.colonies[ci].scout {
                self.colonies[ci].scout = pick;
                self.colonies[ci].scout_goal = None;
            }
            *slot = pick;
        }
        out
    }

    /// The scout: ignores waypoints, Retreat and enemies, and visits the
    /// chambers the team has not seen, nearest first, then sweeps the
    /// chambers out of sight.
    fn scout(&mut self, a: &mut Ant) {
        a.target = None;
        let ci = a.colony as usize;
        let mode = if a.species.flies() {
            Mode::Fly
        } else {
            // Walking fields go round dirt and enemy walls: a scout never
            // stops to dig.
            Mode::Walk
        };
        let goal = self.colonies[ci].scout_goal.filter(|&g| {
            let ch = &self.map.chambers[g as usize];
            !self.team_sees(a.colony, ch.cx, ch.cy)
        });
        let goal = match goal {
            Some(g) => Some(g),
            None => {
                let last = self.colonies[ci].scout_goal;
                let cands = crate::ai::scout_candidates(self, a.colony, a.pos, last);
                let (cx, cy) = a.pos.cell();
                let mut found = None;
                for i in cands.into_iter().take(6) {
                    let ch = &self.map.chambers[i];
                    let key = FieldKey {
                        colony: a.colony,
                        mode,
                        x: ch.cx,
                        y: ch.cy,
                        r: 2,
                    };
                    let teams = self.team_table();
                    let t = Terrain {
                        w: self.map.w,
                        h: self.map.h,
                        ground: &self.map.ground,
                        wall: &self.wall,
                        teams: &teams,
                    };
                    if self.nav.field(&t, key).at(self.map.w, cx, cy) != UNREACHED {
                        found = Some(i as u32);
                        break;
                    }
                }
                found
            }
        };
        if goal != self.colonies[ci].scout_goal && goal.is_some() {
            self.colonies[ci].scout_goal = goal;
        }
        let Some(g) = goal else {
            return;
        };
        let ch = &self.map.chambers[g as usize];
        let key = FieldKey {
            colony: a.colony,
            mode,
            x: ch.cx,
            y: ch.cy,
            r: 2,
        };
        self.follow(a, key);
    }

    fn nurse(&mut self, a: &mut Ant, st: &crate::rules::Stats) {
        let team = self.colonies[a.colony as usize].team;
        let mut best: Option<(i32, i64, usize)> = None;
        for b in self.grid.around(a.pos, st.vision * CELL) {
            for &j in &self.grid.ants[b] {
                let o = &self.ants[j as usize];
                if o.id == a.id || o.health <= 0 || self.colonies[o.colony as usize].team != team {
                    continue;
                }
                let max = self.colonies[o.colony as usize]
                    .ranks
                    .apply(o.species)
                    .health;
                let missing = max - o.health;
                if missing <= 0 {
                    continue;
                }
                let d = o.pos.dist2(a.pos);
                if d <= ((st.vision * CELL) as i64).pow(2) {
                    let key = (missing, -d, j as usize);
                    if best.map(|b| (key.0, key.1) > (b.0, b.1)).unwrap_or(true) {
                        best = Some(key);
                    }
                }
            }
        }
        if let Some((_, _, j)) = best {
            let tp = self.ants[j].pos;
            if a.pos.dist(tp) <= st.range + st.radius * 2 {
                if a.cooldown == 0 {
                    let o = self.ants[j];
                    let max = self.colonies[o.colony as usize]
                        .ranks
                        .apply(o.species)
                        .health;
                    self.ants[j].health = (o.health + st.heal).min(max);
                    a.cooldown = st.attack_ticks;
                    self.effects.push(Effect {
                        kind: EffectKind::Heal,
                        from: a.pos,
                        to: tp,
                        until: self.tick + ticks(3),
                        colony: a.colony,
                    });
                }
            } else {
                let sp = self.speed_of(a);
                self.step_toward(a, tp, sp);
            }
            return;
        }
        // Nobody hurt: walk with the waypoint like any combat ant.
        a.target = None;
        let ci = a.colony as usize;
        let wps = self.colonies[ci].waypoints[a.species.index()].clone();
        let key = if let Some(wp) = wps.first() {
            let (x, y) = wp.cell();
            Some(World::key_for(a, x, y, 3))
        } else {
            self.queen_key(a)
                .map(|(k, _, _)| FieldKey { r: k.r + 4, ..k })
        };
        if let Some(k) = key {
            self.follow(a, k);
        }
    }

    fn hit_ant(&mut self, j: usize, damage: i32, by: u8) {
        let o = self.ants[j];
        if o.health <= 0 {
            return;
        }
        let armor = self.colonies[o.colony as usize]
            .ranks
            .apply(o.species)
            .armor;
        self.ants[j].health -= damage_taken(damage, armor);
        if self.ants[j].health <= 0 {
            self.colonies[by as usize].tally.killed += 1;
        }
    }

    fn hit_structure(&mut self, si: usize, damage: i32) {
        self.structures[si].health -= damage;
    }

    fn strike(&mut self, a: &mut Ant, t: Target, st: &crate::rules::Stats) {
        let Some((tp, _, _)) = self.target_pos(t) else {
            return;
        };
        let kind = if st.splash > 0 {
            EffectKind::Blast
        } else if st.range > CELL {
            EffectKind::Spray
        } else {
            EffectKind::Bite
        };
        self.effects.push(Effect {
            kind,
            from: a.pos,
            to: tp,
            until: self.tick + ticks(if kind == EffectKind::Blast { 4 } else { 2 }),
            colony: a.colony,
        });
        if a.species == Species::Exploding {
            a.spent = true;
            a.health = 0;
        }
        if st.splash > 0 {
            let r = st.splash;
            for b in self.grid.around(tp, r + CELL) {
                for jj in self.grid.ants[b].clone() {
                    let o = self.ants[jj as usize];
                    if o.id != a.id
                        && !self.allied(o.colony, a.colony)
                        && o.pos.dist(tp) <= r + o.species.stats().radius
                    {
                        self.hit_ant(jj as usize, st.damage, a.colony);
                    }
                }
                for sj in self.grid.structures[b].clone() {
                    let Some(s) = self.structures.get(sj as usize) else {
                        continue;
                    };
                    if !self.allied(s.colony, a.colony) && s.pos.dist(tp) <= r + s.radius() {
                        self.hit_structure(sj as usize, st.damage);
                    }
                }
            }
            if let Target::Dirt(x, y) = t {
                self.dig(x, y, st.damage);
            }
            return;
        }
        match t {
            Target::Ant(id) => {
                if let Some(j) = self.ant_index(id) {
                    self.hit_ant(j, st.damage, a.colony);
                }
            }
            Target::Structure(id) => {
                if let Some(si) = self.structure_index(id) {
                    self.hit_structure(si, st.damage);
                }
            }
            Target::Dirt(x, y) => self.dig(x, y, st.damage),
        }
    }

    fn dig(&mut self, x: i32, y: i32, damage: i32) {
        if self.map.at(x, y) != Ground::Dirt {
            return;
        }
        let i = self.map.idx(x, y);
        self.dirt[i] -= damage.min(i16::MAX as i32) as i16;
        if self.dirt[i] <= 0 {
            self.dirt[i] = 0;
            self.map.ground[i] = Ground::Floor;
            self.nav.invalidate();
        }
    }

    /// Ants push apart when they overlap more than 30% of a body width.
    fn separate(&mut self) {
        let mut grid = std::mem::take(&mut self.grid);
        grid.rebuild(self);
        let n = self.ants.len();
        for i in 0..n {
            let a = self.ants[i];
            if a.health <= 0 {
                continue;
            }
            let ra = a.species.stats().radius;
            for b in grid.around(a.pos, ra * 3) {
                for &j in &grid.ants[b] {
                    let j = j as usize;
                    if j <= i {
                        continue;
                    }
                    let o = self.ants[j];
                    if o.health <= 0 || a.species.flies() != o.species.flies() {
                        continue;
                    }
                    let a = self.ants[i];
                    let min = (ra + o.species.stats().radius) * 7 / 10;
                    let d = a.pos.dist(o.pos);
                    if d >= min {
                        continue;
                    }
                    let push = (min - d) / 2 + 1;
                    let (dx, dy) = if d == 0 {
                        let (jx, jy) = jitter(a.id ^ o.id);
                        (jx.max(1), jy)
                    } else {
                        (o.pos.x - a.pos.x, o.pos.y - a.pos.y)
                    };
                    let len =
                        isqrt((dx as i64 * dx as i64 + dy as i64 * dy as i64) as u64).max(1) as i32;
                    let px = dx * push / len;
                    let py = dy * push / len;
                    let na = Pos::new(a.pos.x - px, a.pos.y - py);
                    let no = Pos::new(o.pos.x + px, o.pos.y + py);
                    if self.standable(&a, na) {
                        self.ants[i].pos = na;
                    }
                    if self.standable(&o, no) {
                        self.ants[j].pos = no;
                    }
                }
            }
        }
        self.grid = grid;
    }

    /// Removes dead ants and destroyed structures.
    fn bury(&mut self) {
        let mut lost: Vec<u8> = Vec::new();
        self.ants.retain(|a| {
            let dead = a.health <= 0 || a.spent;
            if dead {
                lost.push(a.colony);
            }
            !dead
        });
        for c in lost {
            self.colonies[c as usize].tally.lost += 1;
        }
        let mut gone: Vec<Id> = Vec::new();
        let mut walls_changed = false;
        let mut queens_lost: Vec<u8> = Vec::new();
        for s in &self.structures {
            if s.health <= 0 {
                gone.push(s.id);
                if s.kind == StructureKind::Wall && s.complete {
                    for &(x, y) in &s.cells {
                        let i = self.map.idx(x, y);
                        self.wall[i] = NO_WALL;
                    }
                    walls_changed = true;
                }
                if s.kind == StructureKind::Queen && s.complete {
                    queens_lost.push(s.colony);
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        self.structures.retain(|s| s.health > 0);
        for a in self.ants.iter_mut() {
            if let Job::Fetch(b) | Job::Haul(b) | Job::Build(b) = a.job {
                if gone.contains(&b) {
                    a.job = Job::Idle;
                    a.inventory = [0; 3];
                }
            }
            if let Some(Target::Structure(id)) = a.target {
                if gone.contains(&id) {
                    a.target = None;
                }
            }
        }
        if walls_changed {
            self.nav.invalidate();
        }
        for c in queens_lost {
            self.announce("notice.queen_lost", Some(c), None);
        }
    }
}

/// Auto-Scout's preference: the fast, cheap Fire Ant, then the Soldier Ant,
/// then any other combat ant, fastest first.
fn scout_rank(s: Species) -> (u8, i32) {
    match s {
        Species::Fire => (0, 0),
        Species::Soldier => (1, 0),
        _ => (2, -s.stats().speed),
    }
}
