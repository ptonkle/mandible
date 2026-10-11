//! Balance tables: species, structures, upgrades and tiers. Every number the
//! game plays by lives here. On-screen names live in `data/strings.json`,
//! keyed by the `key()` of each item; the glossary guard checks them.
//!
//! Units: ticks (`TICKS_PER_SECOND` per second at Normal Game Speed),
//! sub-units (`CELL` per cell) and whole resources. Durations are written as
//! `ticks(tenths of a second)`, so they keep their real-time meaning if the
//! tick rate changes.

use serde::{Deserialize, Serialize};

use crate::fixed::CELL;

/// 20 ticks per second (50 ms at Normal Game Speed): ants step at most
/// about two thirds of a cell per tick even at top speed, so a one-cell wall
/// can never be stepped over.
pub const TICKS_PER_SECOND: i32 = 20;
pub const POP_CAP: i32 = 300;
pub const SATURATION: usize = 8;
pub const WAYPOINTS_PER_SPECIES: usize = 4;
/// The longest wall line one drag can place; cost (6 Leaves a cell) is the
/// real limit.
pub const WALL_MAX_CELLS: i32 = 64;
/// A wall line is placed as blueprints of at most this many cells, so a
/// long wall rises section by section. 8 cells cost 48 Leaves: one Carpenter
/// Ant trip.
pub const WALL_SECTION_CELLS: usize = 8;
pub const MIN_DAMAGE: i32 = 1;
/// A Human colony's stockpile in Sandbox, refilled every tick.
pub const SANDBOX_STOCK: i32 = 99_999;

/// Ticks for a duration given in tenths of a second.
pub const fn ticks(tenths: i32) -> i32 {
    tenths * TICKS_PER_SECOND / 10
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    #[default]
    Food,
    Nectar,
    Leaves,
}

impl Resource {
    pub const ALL: [Resource; 3] = [Resource::Food, Resource::Nectar, Resource::Leaves];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn key(self) -> &'static str {
        match self {
            Resource::Food => "food",
            Resource::Nectar => "nectar",
            Resource::Leaves => "leaves",
        }
    }
}

/// Food, Nectar, Leaves.
pub type Cost = [i32; 3];

pub fn affordable(stock: &Cost, cost: &Cost) -> bool {
    (0..3).all(|i| stock[i] >= cost[i])
}

/// How many times `cost` fits in `stock`.
pub fn times_affordable(stock: &Cost, cost: &Cost) -> i32 {
    let mut n = i32::MAX;
    for i in 0..3 {
        if cost[i] > 0 {
            n = n.min(stock[i] / cost[i]);
        }
    }
    if n == i32::MAX {
        0
    } else {
        n
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Species {
    #[default]
    Harvester,
    Soldier,
    Carpenter,
    Leafcutter,
    Fire,
    Turtle,
    Acid,
    Nurse,
    Flying,
    Exploding,
    TrapJaw,
    Dinosaur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Gatherer,
    Builder,
    Melee,
    Ranged,
    Tank,
    Support,
    Flyer,
    Brute,
    Bomber,
}

impl Role {
    pub fn key(self) -> &'static str {
        match self {
            Role::Gatherer => "gatherer",
            Role::Builder => "builder",
            Role::Melee => "melee",
            Role::Ranged => "ranged",
            Role::Tank => "tank",
            Role::Support => "support",
            Role::Flyer => "flyer",
            Role::Brute => "brute",
            Role::Bomber => "bomber",
        }
    }
}

/// Everything one species plays by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub role: Role,
    pub tier: u8,
    pub cost: Cost,
    pub pop: i32,
    pub train_ticks: i32,
    pub health: i32,
    pub armor: i32,
    pub damage: i32,
    pub attack_ticks: i32,
    /// Reach beyond touching, in sub-units.
    pub range: i32,
    /// Sub-units per tick. At 20 ticks per second a speed of 77 is about
    /// 6 cells a second.
    pub speed: i32,
    /// Body radius in sub-units.
    pub radius: i32,
    /// Cells.
    pub vision: i32,
    /// Splash radius in sub-units; 0 means single target.
    pub splash: i32,
    pub heal: i32,
    pub carry: i32,
    /// Ticks to gather one resource (after `Ranks::apply`: tenths of a tick).
    pub gather_ticks: i32,
    /// Move Speed while laden, as a percentage.
    pub laden_pct: i32,
    /// Build work per tick.
    pub build: i32,
    pub only_gathers: Option<Resource>,
}

const BASE: Stats = Stats {
    role: Role::Melee,
    tier: 1,
    cost: [50, 0, 0],
    pop: 1,
    train_ticks: ticks(80),
    health: 40,
    armor: 0,
    damage: 0,
    attack_ticks: ticks(10),
    range: 48,
    speed: 77,
    radius: 112,
    vision: 7,
    splash: 0,
    heal: 0,
    carry: 0,
    gather_ticks: 0,
    laden_pct: 60,
    build: 0,
    only_gathers: None,
};

impl Species {
    pub const ALL: [Species; 12] = [
        Species::Harvester,
        Species::Soldier,
        Species::Carpenter,
        Species::Leafcutter,
        Species::Fire,
        Species::Turtle,
        Species::Acid,
        Species::Nurse,
        Species::Flying,
        Species::Exploding,
        Species::TrapJaw,
        Species::Dinosaur,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Species> {
        Species::ALL.get(i).copied()
    }

    pub fn key(self) -> &'static str {
        match self {
            Species::Harvester => "harvester",
            Species::Soldier => "soldier",
            Species::Carpenter => "carpenter",
            Species::Leafcutter => "leafcutter",
            Species::Fire => "fire",
            Species::Turtle => "turtle",
            Species::Acid => "acid",
            Species::Nurse => "nurse",
            Species::Flying => "flying",
            Species::Exploding => "exploding",
            Species::TrapJaw => "trap_jaw",
            Species::Dinosaur => "dinosaur",
        }
    }

    pub fn from_key(k: &str) -> Option<Species> {
        Species::ALL.iter().copied().find(|s| s.key() == k)
    }

    /// Gatherers and Builders: never given waypoints, never retreat.
    pub fn is_worker(self) -> bool {
        matches!(self.stats().role, Role::Gatherer | Role::Builder)
    }

    pub fn is_combat(self) -> bool {
        !self.is_worker()
    }

    pub fn flies(self) -> bool {
        self == Species::Flying
    }

    /// Trained by a queen rather than a hatchery.
    pub fn from_queen(self) -> bool {
        self.stats().tier == 1
    }

    pub fn stats(self) -> Stats {
        match self {
            Species::Harvester => Stats {
                role: Role::Gatherer,
                cost: [50, 0, 0],
                train_ticks: ticks(70),
                health: 25,
                speed: 82,
                vision: 6,
                carry: 10,
                gather_ticks: ticks(3),
                laden_pct: 85,
                ..BASE
            },
            Species::Soldier => Stats {
                role: Role::Melee,
                cost: [55, 0, 0],
                train_ticks: ticks(80),
                health: 45,
                damage: 6,
                attack_ticks: ticks(10),
                ..BASE
            },
            Species::Carpenter => Stats {
                role: Role::Builder,
                cost: [50, 0, 10],
                train_ticks: ticks(90),
                health: 30,
                vision: 6,
                carry: 50,
                laden_pct: 75,
                build: 1,
                ..BASE
            },
            Species::Leafcutter => Stats {
                role: Role::Gatherer,
                tier: 2,
                cost: [50, 20, 0],
                train_ticks: ticks(80),
                health: 30,
                speed: 77,
                vision: 6,
                carry: 20,
                gather_ticks: ticks(2),
                laden_pct: 90,
                only_gathers: Some(Resource::Leaves),
                ..BASE
            },
            Species::Fire => Stats {
                role: Role::Melee,
                tier: 2,
                cost: [40, 10, 0],
                train_ticks: ticks(50),
                health: 25,
                damage: 4,
                attack_ticks: ticks(6),
                speed: 133,
                radius: 96,
                ..BASE
            },
            Species::Turtle => Stats {
                role: Role::Tank,
                tier: 2,
                cost: [70, 30, 0],
                pop: 2,
                train_ticks: ticks(120),
                health: 110,
                armor: 4,
                damage: 4,
                attack_ticks: ticks(12),
                speed: 51,
                radius: 144,
                vision: 6,
                ..BASE
            },
            Species::Acid => Stats {
                role: Role::Ranged,
                tier: 2,
                cost: [50, 40, 0],
                pop: 2,
                train_ticks: ticks(110),
                health: 30,
                damage: 9,
                attack_ticks: ticks(14),
                range: 5 * CELL,
                speed: 72,
                vision: 8,
                ..BASE
            },
            Species::Nurse => Stats {
                role: Role::Support,
                tier: 2,
                cost: [40, 50, 0],
                pop: 2,
                train_ticks: ticks(110),
                health: 35,
                attack_ticks: ticks(10),
                range: 3 * CELL,
                heal: 4,
                ..BASE
            },
            Species::Flying => Stats {
                role: Role::Flyer,
                tier: 3,
                cost: [60, 60, 0],
                pop: 2,
                train_ticks: ticks(130),
                health: 45,
                damage: 7,
                attack_ticks: ticks(10),
                speed: 108,
                vision: 10,
                ..BASE
            },
            Species::Exploding => Stats {
                role: Role::Bomber,
                tier: 3,
                cost: [50, 50, 0],
                pop: 2,
                train_ticks: ticks(110),
                health: 30,
                damage: 45,
                attack_ticks: ticks(10),
                speed: 97,
                vision: 6,
                splash: 2 * CELL + CELL / 2,
                ..BASE
            },
            Species::TrapJaw => Stats {
                role: Role::Melee,
                tier: 3,
                cost: [80, 60, 0],
                pop: 3,
                train_ticks: ticks(150),
                health: 85,
                armor: 2,
                damage: 18,
                attack_ticks: ticks(14),
                speed: 82,
                radius: 128,
                ..BASE
            },
            Species::Dinosaur => Stats {
                role: Role::Brute,
                tier: 3,
                cost: [150, 120, 0],
                pop: 5,
                train_ticks: ticks(220),
                health: 320,
                armor: 4,
                damage: 14,
                attack_ticks: ticks(16),
                range: 64,
                speed: 46,
                radius: 300,
                vision: 8,
                splash: CELL + CELL * 3 / 4,
                ..BASE
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructureKind {
    Queen,
    BroodCell,
    Wall,
    Hatchery(Species),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureStats {
    pub cost: Cost,
    pub health: i32,
    /// Build work needed, in Carpenter Ant ticks.
    pub work: i32,
    pub housing: i32,
    pub vision: i32,
    /// Footprint radius in sub-units.
    pub radius: i32,
    pub tier: u8,
}

impl StructureKind {
    /// Every kind in action-grid order (the Build tab).
    pub fn all() -> Vec<StructureKind> {
        let mut v = vec![
            StructureKind::Queen,
            StructureKind::BroodCell,
            StructureKind::Wall,
        ];
        for s in Species::ALL {
            if !s.from_queen() {
                v.push(StructureKind::Hatchery(s));
            }
        }
        v
    }

    pub fn key(self) -> String {
        match self {
            StructureKind::Queen => "queen".into(),
            StructureKind::BroodCell => "brood_cell".into(),
            StructureKind::Wall => "wall".into(),
            StructureKind::Hatchery(s) => format!("hatchery.{}", s.key()),
        }
    }

    pub fn from_key(k: &str) -> Option<StructureKind> {
        StructureKind::all().into_iter().find(|s| s.key() == k)
    }

    /// Stats for one structure; for a wall, one cell of it.
    pub fn stats(self) -> StructureStats {
        match self {
            StructureKind::Queen => StructureStats {
                cost: [250, 50, 200],
                health: 1500,
                work: ticks(450),
                housing: 20,
                vision: 10,
                radius: 2 * CELL + CELL / 2,
                tier: 1,
            },
            StructureKind::BroodCell => StructureStats {
                cost: [0, 0, 60],
                health: 300,
                work: ticks(100),
                housing: 15,
                vision: 6,
                radius: CELL + CELL / 4,
                tier: 1,
            },
            StructureKind::Wall => StructureStats {
                cost: [0, 0, 6],
                health: 150,
                work: ticks(15),
                housing: 0,
                vision: 3,
                radius: CELL / 2,
                tier: 1,
            },
            StructureKind::Hatchery(s) => {
                let t3 = s.stats().tier >= 3;
                StructureStats {
                    cost: if t3 { [150, 80, 150] } else { [120, 0, 100] },
                    health: 600,
                    work: ticks(if t3 { 300 } else { 200 }),
                    housing: 0,
                    vision: 7,
                    radius: 2 * CELL,
                    tier: s.stats().tier,
                }
            }
        }
    }

    /// Queens and hatcheries can make ants, so they keep a colony alive.
    pub fn makes_ants(self) -> bool {
        matches!(self, StructureKind::Queen | StructureKind::Hatchery(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Upgrade {
    Damage,
    Armor,
    Health,
    GatherRate,
    CarryCapacity,
    BuildRate,
    Species(Species),
}

impl Upgrade {
    pub fn key(self) -> String {
        match self {
            Upgrade::Damage => "damage".into(),
            Upgrade::Armor => "armor".into(),
            Upgrade::Health => "health".into(),
            Upgrade::GatherRate => "gather_rate".into(),
            Upgrade::CarryCapacity => "carry_capacity".into(),
            Upgrade::BuildRate => "build_rate".into(),
            Upgrade::Species(s) => format!("species.{}", s.key()),
        }
    }

    pub fn all() -> Vec<Upgrade> {
        let mut v = vec![
            Upgrade::Damage,
            Upgrade::Armor,
            Upgrade::Health,
            Upgrade::GatherRate,
            Upgrade::CarryCapacity,
            Upgrade::BuildRate,
        ];
        for s in Species::ALL {
            if s.is_combat() {
                v.push(Upgrade::Species(s));
            }
        }
        v
    }

    pub fn from_key(k: &str) -> Option<Upgrade> {
        Upgrade::all().into_iter().find(|u| u.key() == k)
    }

    pub fn max_rank(self) -> u8 {
        match self {
            Upgrade::Damage | Upgrade::Armor | Upgrade::Health | Upgrade::GatherRate => 3,
            Upgrade::CarryCapacity => 2,
            Upgrade::BuildRate => 1,
            Upgrade::Species(_) => 2,
        }
    }

    /// The tier a colony needs before it can buy `rank` (1-based).
    pub fn tier_for(self, rank: u8) -> u8 {
        match self {
            Upgrade::Species(s) => rank.max(s.stats().tier),
            _ => rank,
        }
    }

    pub fn cost(self, rank: u8) -> Cost {
        let r = rank as i32;
        match self {
            Upgrade::Damage | Upgrade::Armor => [100 * r, 60 * r, 0],
            Upgrade::Health => [120 * r, 50 * r, 0],
            Upgrade::GatherRate => [100 * r, 0, 60 * r],
            Upgrade::CarryCapacity => [80 * r, 0, 80 * r],
            Upgrade::BuildRate => [60, 0, 120],
            Upgrade::Species(_) => [80 * r, 80 * r, 0],
        }
    }

    pub fn research_ticks(self, rank: u8) -> i32 {
        ticks((150 + 100 * rank as i32).min(400))
    }

    /// Hatchery species upgrades are researched at that species' hatchery;
    /// everything else at a queen.
    pub fn at_hatchery(self) -> Option<Species> {
        match self {
            Upgrade::Species(s) if !s.from_queen() => Some(s),
            _ => None,
        }
    }
}

/// Cost and duration of evolving to `tier`.
pub fn evolve_cost(tier: u8) -> (Cost, i32) {
    match tier {
        2 => ([300, 150, 0], ticks(200)),
        _ => ([500, 300, 100], ticks(300)),
    }
}

/// A colony's upgrade ranks, indexed by `Upgrade::all()` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ranks {
    pub damage: u8,
    pub armor: u8,
    pub health: u8,
    pub gather_rate: u8,
    pub carry_capacity: u8,
    pub build_rate: u8,
    pub species: [u8; 12],
}

impl Ranks {
    pub fn get(&self, u: Upgrade) -> u8 {
        match u {
            Upgrade::Damage => self.damage,
            Upgrade::Armor => self.armor,
            Upgrade::Health => self.health,
            Upgrade::GatherRate => self.gather_rate,
            Upgrade::CarryCapacity => self.carry_capacity,
            Upgrade::BuildRate => self.build_rate,
            Upgrade::Species(s) => self.species[s.index()],
        }
    }

    pub fn raise(&mut self, u: Upgrade) {
        let slot = match u {
            Upgrade::Damage => &mut self.damage,
            Upgrade::Armor => &mut self.armor,
            Upgrade::Health => &mut self.health,
            Upgrade::GatherRate => &mut self.gather_rate,
            Upgrade::CarryCapacity => &mut self.carry_capacity,
            Upgrade::BuildRate => &mut self.build_rate,
            Upgrade::Species(s) => &mut self.species[s.index()],
        };
        *slot = (*slot + 1).min(u.max_rank());
    }

    /// A species' stats after this colony's upgrades.
    pub fn apply(&self, s: Species) -> Stats {
        let mut st = s.stats();
        let sr = self.species[s.index()] as i32;
        if s.is_combat() && st.damage > 0 {
            st.damage += self.damage as i32;
        }
        st.armor += self.armor as i32;
        st.health += st.health * 10 * self.health as i32 / 100;
        if st.gather_ticks > 0 {
            // +20% gather rate per rank: fewer ticks per resource, in tenths.
            st.gather_ticks *= 10;
            st.gather_ticks = st.gather_ticks * 100 / (100 + 20 * self.gather_rate as i32);
        } else {
            st.gather_ticks *= 10;
        }
        if st.carry > 0 {
            // +30% per rank: Harvester Ant 10 -> 13 -> 16, Carpenter Ant
            // 50 -> 65 -> 80.
            st.carry += st.carry * 3 * self.carry_capacity as i32 / 10;
        }
        if st.build > 0 {
            st.build *= 2 + self.build_rate as i32;
        }
        match s {
            Species::Soldier => st.health += 15 * sr,
            Species::Fire => st.speed += st.speed * 15 * sr / 100,
            Species::Turtle => st.armor += 2 * sr,
            Species::Acid => st.range += CELL * sr,
            Species::Nurse => st.heal += sr,
            Species::Flying => st.damage += 2 * sr,
            Species::Exploding => st.splash += CELL / 2 * sr,
            Species::TrapJaw => st.damage += 5 * sr,
            Species::Dinosaur => st.damage += 4 * sr,
            _ => {}
        }
        st
    }
}

/// Damage taken from one hit: flat armor, never below `MIN_DAMAGE`.
pub fn damage_taken(damage: i32, armor: i32) -> i32 {
    (damage - armor).max(MIN_DAMAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_is_flat_with_a_minimum_of_one() {
        // Worked by hand from the brief: 12 damage against 8 armor deals 4.
        assert_eq!(damage_taken(12, 8), 4);
        assert_eq!(damage_taken(3, 8), 1);
        assert_eq!(damage_taken(8, 8), 1);
        assert_eq!(damage_taken(10, 0), 10);
    }

    #[test]
    fn there_are_twelve_species_and_keys_round_trip() {
        assert_eq!(Species::ALL.len(), 12);
        for s in Species::ALL {
            assert_eq!(Species::from_key(s.key()), Some(s));
            assert_eq!(Species::from_index(s.index()), Some(s));
        }
    }

    #[test]
    fn every_species_has_a_tier_cost_and_health() {
        for s in Species::ALL {
            let st = s.stats();
            assert!((1..=3).contains(&st.tier), "{s:?}");
            assert!(st.cost.iter().sum::<i32>() > 0, "{s:?}");
            assert!(st.health > 0 && st.pop > 0 && st.speed > 0, "{s:?}");
            assert!(
                st.damage > 0 || st.heal > 0 || st.carry > 0,
                "{s:?} does nothing"
            );
        }
    }

    #[test]
    fn tier_one_species_come_from_queens_and_the_rest_from_hatcheries() {
        let from_queen: Vec<Species> = Species::ALL
            .into_iter()
            .filter(|s| s.from_queen())
            .collect();
        assert_eq!(
            from_queen,
            vec![Species::Harvester, Species::Soldier, Species::Carpenter]
        );
        assert_eq!(StructureKind::all().len(), 12);
    }

    #[test]
    fn ranks_need_their_tier() {
        assert_eq!(Upgrade::Damage.tier_for(1), 1);
        assert_eq!(Upgrade::Damage.tier_for(3), 3);
        assert_eq!(Upgrade::Species(Species::Dinosaur).tier_for(1), 3);
        assert_eq!(Upgrade::Species(Species::Soldier).tier_for(2), 2);
    }

    #[test]
    fn upgrades_raise_stats_and_stop_at_max_rank() {
        let mut r = Ranks::default();
        let before = r.apply(Species::Soldier);
        r.raise(Upgrade::Damage);
        r.raise(Upgrade::Armor);
        let after = r.apply(Species::Soldier);
        assert_eq!(after.damage, before.damage + 1);
        assert_eq!(after.armor, before.armor + 1);
        for _ in 0..5 {
            r.raise(Upgrade::BuildRate);
        }
        assert_eq!(r.build_rate, 1);
    }

    #[test]
    fn upgrade_and_structure_keys_round_trip() {
        for u in Upgrade::all() {
            assert_eq!(Upgrade::from_key(&u.key()), Some(u));
        }
        for s in StructureKind::all() {
            assert_eq!(StructureKind::from_key(&s.key()), Some(s));
        }
    }

    #[test]
    fn only_queens_and_hatcheries_make_ants() {
        assert!(StructureKind::Queen.makes_ants());
        assert!(StructureKind::Hatchery(Species::Acid).makes_ants());
        assert!(!StructureKind::Wall.makes_ants());
        assert!(!StructureKind::BroodCell.makes_ants());
    }
}
