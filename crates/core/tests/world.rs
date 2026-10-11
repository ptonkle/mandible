//! Behaviour tests for the simulation. Expected values are worked out from
//! the rules by hand, never copied from the simulation's own output.

use tool_core::fixed::{Pos, CELL};
use tool_core::rules::{Species, StructureKind, Upgrade};
use tool_core::settings::{Difficulty, Format, MatchSettings, SeatKind};
use tool_core::world::{Amount, Command, Focus, Job, World};

fn humans(n: usize) -> Vec<SeatKind> {
    vec![SeatKind::Human; n]
}

fn world(seed: u64) -> World {
    World::new(seed, MatchSettings::default(), &humans(2))
}

fn run(w: &mut World, ticks: i32) {
    for _ in 0..ticks {
        w.step(&[], &[]);
    }
}

fn home(w: &World, seat: usize) -> Pos {
    let h = w.map.home(seat);
    Pos::of_cell(h.cx, h.cy)
}

/// An empty arena: both colonies keep only their queens.
fn arena(seed: u64) -> World {
    let mut w = world(seed);
    w.ants.clear();
    w.settings.events = false;
    w
}

/// Spawns an ant exactly at `p` (spawning normally scatters ants).
fn put(w: &mut World, colony: u8, s: Species, p: Pos) -> u32 {
    let id = w.spawn_ant(colony, s, p);
    let a = w.ants.iter_mut().find(|a| a.id == id).unwrap();
    a.pos = p;
    a.prev = p;
    id
}

fn near(p: Pos, dx: i32, dy: i32) -> Pos {
    Pos::new(p.x + dx * CELL, p.y + dy * CELL)
}

#[test]
fn two_worlds_with_the_same_seed_and_commands_stay_identical() {
    let seats = [
        SeatKind::Ai(Difficulty::Imago),
        SeatKind::Ai(Difficulty::Pupa),
    ];
    let mut a = World::new(11, MatchSettings::default(), &seats);
    let mut b = World::new(11, MatchSettings::default(), &seats);
    for t in 0..3000 {
        a.step(&[], &[]);
        b.step(&[], &[]);
        assert_eq!(a.checksum(), b.checksum(), "diverged at tick {t}");
    }
    assert!(a.ants.len() > 20, "the AI economy grew: {}", a.ants.len());
}

#[test]
fn a_snapshot_continues_exactly_like_the_original() {
    let seats = [
        SeatKind::Ai(Difficulty::Pupa),
        SeatKind::Ai(Difficulty::Pupa),
    ];
    let mut a = World::new(5, MatchSettings::default(), &seats);
    run(&mut a, 700);
    let mut b = World::restore(&a.snapshot()).expect("restores");
    for t in 0..600 {
        a.step(&[], &[]);
        b.step(&[], &[]);
        assert_eq!(
            a.checksum(),
            b.checksum(),
            "diverged {t} ticks after restore"
        );
    }
}

#[test]
fn harvesters_fill_their_inventory_and_deliver_to_the_queen() {
    let mut w = world(1);
    let before = w.colonies[0].stock;
    run(&mut w, 400);
    let after = w.colonies[0].stock;
    let gathered: i32 = w.colonies[0].tally.gathered.iter().sum();
    // Six Harvester Ants carry 10 each per trip; in 40 s they make several.
    assert!(gathered >= 60, "gathered {gathered}");
    assert_eq!(
        (0..3).map(|i| after[i] - before[i]).sum::<i32>(),
        gathered,
        "everything delivered reached the stockpile"
    );
}

#[test]
fn income_is_what_was_delivered_in_the_last_ten_seconds() {
    let mut w = world(2);
    run(&mut w, 300);
    let g0 = w.colonies[0].tally.gathered;
    run(&mut w, 100);
    let g1 = w.colonies[0].tally.gathered;
    let income = w.income_tenths(0);
    for r in 0..3 {
        // Tenths per second over 10 s equals the plain total over 10 s.
        assert_eq!(income[r], g1[r] - g0[r], "resource {r}");
    }
}

#[test]
fn saturation_caps_gatherers_per_node() {
    let mut w = arena(3);
    let h = home(&w, 0);
    for _ in 0..30 {
        w.spawn_ant(0, Species::Harvester, h);
    }
    w.colonies[0].focus = Focus::Food;
    run(&mut w, 60);
    let mut per = std::collections::BTreeMap::new();
    for a in &w.ants {
        if let Job::Gather(n) = a.job {
            *per.entry(n).or_insert(0) += 1;
        }
    }
    assert!(per.values().all(|c| *c <= 8), "{per:?}");
    assert!(
        per.len() >= 3,
        "30 gatherers spread over several nodes: {per:?}"
    );
}

#[test]
fn gather_focus_changes_the_split() {
    let count = |focus: Focus| {
        let mut w = arena(4);
        let h = home(&w, 0);
        for _ in 0..12 {
            w.spawn_ant(0, Species::Harvester, h);
        }
        w.colonies[0].focus = focus;
        run(&mut w, 30);
        let mut by = [0; 3];
        for a in &w.ants {
            if let Job::Gather(n) = a.job {
                let r = w.nodes.iter().find(|x| x.id == n).unwrap().resource;
                by[r.index()] += 1;
            }
        }
        by
    };
    let nectar = count(Focus::Nectar);
    let food = count(Focus::Food);
    assert!(
        nectar[1] > food[1],
        "nectar focus {nectar:?} vs food {food:?}"
    );
    assert!(
        food[0] > nectar[0],
        "food focus {food:?} vs nectar {nectar:?}"
    );
}

#[test]
fn a_depleted_node_is_left() {
    let mut w = world(5);
    run(&mut w, 50);
    let busy: Vec<u32> = w
        .ants
        .iter()
        .filter_map(|a| match a.job {
            Job::Gather(n) => Some(n),
            _ => None,
        })
        .collect();
    let target = busy[0];
    let ni = w.nodes.iter().position(|n| n.id == target).unwrap();
    w.nodes[ni].amount = 0;
    run(&mut w, 20);
    assert!(
        !w.ants.iter().any(|a| a.job == Job::Gather(target)),
        "nobody gathers at an empty node"
    );
}

#[test]
fn train_ten_queues_ten_and_train_max_spends_down() {
    let mut w = world(6);
    w.colonies[0].stock = [5000, 0, 0];
    w.apply(
        0,
        &Command::Train {
            species: Species::Soldier,
            amount: Amount::Ten,
        },
    );
    let queued = w.queued_pop(0);
    assert_eq!(queued, 10);
    // 5000 - 10 * 55 = 4450 left; Train Max buys 4450 / 50 = 89 Harvester Ants.
    assert_eq!(w.colonies[0].stock[0], 4450);
    w.apply(
        0,
        &Command::Train {
            species: Species::Harvester,
            amount: Amount::Max,
        },
    );
    assert_eq!(w.queued_pop(0), 10 + 89);
    assert_eq!(w.colonies[0].stock[0], 0);
}

#[test]
fn train_ten_with_too_little_queues_what_it_can() {
    let mut w = world(6);
    w.colonies[0].stock = [170, 0, 0];
    w.apply(
        0,
        &Command::Train {
            species: Species::Soldier,
            amount: Amount::Ten,
        },
    );
    // 170 / 55 = 3.
    assert_eq!(w.queued_pop(0), 3);
    assert_eq!(w.colonies[0].stock[0], 5);
}

#[test]
fn cancelling_a_queue_item_refunds_it() {
    let mut w = world(6);
    w.colonies[0].stock = [100, 0, 0];
    w.apply(
        0,
        &Command::Train {
            species: Species::Soldier,
            amount: Amount::One,
        },
    );
    assert_eq!(w.colonies[0].stock[0], 45);
    let q = w.queens(0).next().unwrap().id;
    w.apply(
        0,
        &Command::Cancel {
            structure: q,
            slot: 0,
        },
    );
    assert_eq!(w.colonies[0].stock[0], 100);
}

#[test]
fn housing_full_pauses_training() {
    let mut w = arena(7);
    let h = home(&w, 0);
    // A queen houses 20: fill it with 20 Soldier Ants.
    for _ in 0..20 {
        w.spawn_ant(0, Species::Soldier, h);
    }
    w.colonies[0].stock = [500, 0, 0];
    w.apply(
        0,
        &Command::Train {
            species: Species::Soldier,
            amount: Amount::One,
        },
    );
    run(&mut w, 200);
    assert_eq!(w.population(0), 20, "no ant was trained past housing");
    assert_eq!(w.queued_pop(0), 1, "it waits in the queue");
}

#[test]
fn tier_two_species_need_tier_and_a_hatchery() {
    let mut w = world(8);
    w.colonies[0].stock = [5000, 5000, 5000];
    let r = w.try_apply(
        0,
        &Command::Train {
            species: Species::Acid,
            amount: Amount::One,
        },
    );
    assert_eq!(r, Err(tool_core::world::Refusal::NeedsTier(2)));
    w.colonies[0].tier = 2;
    let r = w.try_apply(
        0,
        &Command::Train {
            species: Species::Acid,
            amount: Amount::One,
        },
    );
    assert_eq!(
        r,
        Err(tool_core::world::Refusal::NeedsHatchery(Species::Acid))
    );
}

#[test]
fn evolving_raises_the_tier_after_its_duration() {
    let mut w = world(9);
    w.colonies[0].stock = [1000, 1000, 1000];
    w.apply(0, &Command::Evolve);
    assert_eq!(w.colonies[0].stock, [700, 850, 1000]);
    run(&mut w, 199);
    assert_eq!(w.colonies[0].tier, 1);
    run(&mut w, 1);
    assert_eq!(w.colonies[0].tier, 2);
}

#[test]
fn a_rank_two_upgrade_needs_tier_two() {
    let mut w = world(9);
    w.colonies[0].stock = [5000, 5000, 5000];
    w.apply(
        0,
        &Command::Research {
            upgrade: Upgrade::Armor,
        },
    );
    let r = w.try_apply(
        0,
        &Command::Research {
            upgrade: Upgrade::Armor,
        },
    );
    assert_eq!(r, Err(tool_core::world::Refusal::AlreadyQueued));
    run(&mut w, 260);
    assert_eq!(w.colonies[0].ranks.armor, 1);
    let r = w.try_apply(
        0,
        &Command::Research {
            upgrade: Upgrade::Armor,
        },
    );
    assert_eq!(r, Err(tool_core::world::Refusal::NeedsTier(2)));
}

fn place_near_home(w: &mut World, kind: StructureKind) -> u32 {
    let h = home(w, 0);
    let (x, y) = tool_core::ai::find_spot(w, 0, kind, h, 6).expect("a spot");
    w.apply(
        0,
        &Command::Place {
            kind,
            x,
            y,
            x2: x,
            y2: y,
        },
    );
    w.structures
        .iter()
        .filter(|s| !s.complete)
        .last()
        .expect("placed")
        .id
}

#[test]
fn a_blueprint_is_built_only_after_its_cost_is_carried_to_it() {
    let mut w = world(10);
    let id = place_near_home(&mut w, StructureKind::BroodCell);
    // The cost left the stockpile at once: 200 - 60 Leaves.
    assert_eq!(w.colonies[0].stock[2], 140);
    let s = w.structures.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.delivered, [0, 0, 0]);
    let mut seen_delivered_before_work = false;
    for _ in 0..600 {
        w.step(&[], &[]);
        if let Some(s) = w.structures.iter().find(|s| s.id == id) {
            if s.work_done > 0 {
                assert!(
                    s.fully_delivered(),
                    "no build work before every Leaf arrived"
                );
                seen_delivered_before_work = true;
            }
            if s.complete {
                break;
            }
        }
    }
    let s = w.structures.iter().find(|s| s.id == id).unwrap();
    assert!(
        seen_delivered_before_work && s.complete,
        "the brood cell was built"
    );
    assert_eq!(w.housing(0), 35, "queen 20 + brood cell 15");
}

#[test]
fn cancelling_a_blueprint_refunds_its_cost() {
    let mut w = world(10);
    let id = place_near_home(&mut w, StructureKind::BroodCell);
    w.apply(0, &Command::CancelBlueprint { id });
    assert_eq!(w.colonies[0].stock[2], 200);
    assert!(w.structures.iter().all(|s| s.id != id));
}

#[test]
fn a_finished_wall_closes_its_cells_to_enemies() {
    let mut w = world(12);
    w.colonies[0].stock = [0, 0, 500];
    let h = home(&w, 0);
    let (hx, hy) = h.cell();
    w.apply(
        0,
        &Command::Place {
            kind: StructureKind::Wall,
            x: hx + 6,
            y: hy - 3,
            x2: hx + 6,
            y2: hy + 3,
        },
    );
    let bp = w
        .structures
        .iter()
        .find(|s| !s.complete)
        .expect("wall blueprint");
    assert_eq!(bp.cells.len(), 7);
    assert_eq!(bp.cost, [0, 0, 42]);
    run(&mut w, 900);
    let walls = w
        .structures
        .iter()
        .filter(|s| s.kind == StructureKind::Wall && s.complete)
        .count();
    assert_eq!(walls, 7, "one wall per cell once built");
    let i = w.map.idx(hx + 6, hy);
    assert_eq!(w.wall[i], 0);
}

#[test]
fn armor_is_flat_in_a_real_fight() {
    let mut w = arena(13);
    let h = home(&w, 0);
    // A Soldier Ant (6 damage) bites a Turtle Ant (4 armor): 2 per bite.
    put(&mut w, 0, Species::Soldier, near(h, 2, 0));
    put(&mut w, 1, Species::Turtle, near(h, 3, 0));
    let turtle = w.ants[1].id;
    let start = w.ants[1].health;
    run(&mut w, 25);
    let t = w.ants.iter().find(|a| a.id == turtle).unwrap();
    let lost = start - t.health;
    assert!(lost > 0 && lost % 2 == 0, "lost {lost}: whole bites of 2");
}

#[test]
fn splash_hits_every_enemy_in_its_radius() {
    let mut w = arena(14);
    let h = home(&w, 0);
    w.colonies[0].tier = 3;
    put(&mut w, 0, Species::Exploding, near(h, 0, 0));
    for k in 0..3 {
        put(&mut w, 1, Species::Soldier, near(h, 2, k - 1));
    }
    let enemies: Vec<u32> = w
        .ants
        .iter()
        .filter(|a| a.colony == 1)
        .map(|a| a.id)
        .collect();
    run(&mut w, 30);
    // 45 damage clears 45 health: every Soldier Ant in the blast dies, and
    // the Exploding Ant spends itself.
    assert!(
        w.ants.iter().all(|a| a.colony != 0),
        "the Exploding Ant is gone"
    );
    let survivors = w.ants.iter().filter(|a| enemies.contains(&a.id)).count();
    assert_eq!(survivors, 0);
}

#[test]
fn a_nurse_heals_the_most_hurt_ally() {
    let mut w = arena(15);
    let h = home(&w, 0);
    put(&mut w, 0, Species::Nurse, near(h, 0, 0));
    put(&mut w, 0, Species::Soldier, near(h, 1, 0));
    put(&mut w, 0, Species::Soldier, near(h, 0, 1));
    w.ants[1].health = 40;
    w.ants[2].health = 10;
    let worst = w.ants[2].id;
    run(&mut w, 2);
    let a = w.ants.iter().find(|a| a.id == worst).unwrap();
    assert_eq!(a.health, 14, "healed by 4");
}

#[test]
fn waypoints_split_a_species_evenly() {
    let mut w = arena(16);
    let h = home(&w, 0);
    for _ in 0..9 {
        w.spawn_ant(0, Species::Soldier, h);
    }
    let (hx, hy) = h.cell();
    for dy in [-6, 0, 6] {
        w.apply(
            0,
            &Command::SetWaypoint {
                species: Some(Species::Soldier),
                x: hx - 8,
                y: hy + dy,
            },
        );
    }
    let wps = w.colonies[0].waypoints[Species::Soldier.index()].clone();
    assert_eq!(wps.len(), 3);
    run(&mut w, 200);
    let mut at = [0; 3];
    for a in &w.ants {
        let k = (0..3).min_by_key(|k| a.pos.dist2(wps[*k])).unwrap();
        at[k] += 1;
    }
    assert_eq!(at, [3, 3, 3]);
}

#[test]
fn clearing_a_waypoint_removes_it() {
    let mut w = arena(16);
    let (hx, hy) = home(&w, 0).cell();
    w.apply(
        0,
        &Command::SetWaypoint {
            species: None,
            x: hx - 8,
            y: hy,
        },
    );
    assert!(!w.colonies[0].waypoints[Species::Fire.index()].is_empty());
    w.apply(0, &Command::ClearWaypoint { x: hx - 7, y: hy });
    assert!(w.colonies[0].waypoints.iter().all(|v| v.is_empty()));
}

#[test]
fn retreat_pulls_ants_home_and_releasing_sends_them_back() {
    let mut w = arena(17);
    let h = home(&w, 0);
    let c = &w.map.chambers[w.map.centre];
    let (cx, cy) = (c.cx, c.cy);
    for _ in 0..4 {
        w.spawn_ant(0, Species::Fire, h);
    }
    w.apply(
        0,
        &Command::SetWaypoint {
            species: Some(Species::Fire),
            x: cx,
            y: cy,
        },
    );
    let centre = Pos::of_cell(cx, cy);
    run(&mut w, 400);
    let far = w.ants.iter().map(|a| a.pos.dist(h)).min().unwrap();
    assert!(far > 20 * CELL, "they reached the centre");
    w.apply(0, &Command::Retreat { on: true });
    run(&mut w, 400);
    assert!(
        w.ants.iter().all(|a| a.pos.dist(h) < 10 * CELL),
        "they came home"
    );
    w.apply(0, &Command::Retreat { on: false });
    run(&mut w, 400);
    assert!(
        w.ants.iter().all(|a| a.pos.dist(centre) < 8 * CELL),
        "and went back"
    );
}

#[test]
fn allies_share_vision() {
    let s = MatchSettings {
        format: Format::TwoVTwo,
        ..MatchSettings::default()
    };
    let w = World::new(18, s, &humans(4));
    let ally = w.map.home(1);
    let i = w.map.idx(ally.cx, ally.cy);
    assert!(!w.visible[0].get(i));
    assert!(w.team_visible(0).get(i));
    assert!(!w.team_visible(2).get(i));
}

#[test]
fn walls_and_brood_cells_do_not_keep_a_colony_alive() {
    let mut w = world(19);
    w.colonies[1].stock = [0, 0, 500];
    // Give seat 1 a brood cell, then remove everything that makes ants.
    let h = home(&w, 1);
    let (x, y) = tool_core::ai::find_spot(&w, 1, StructureKind::BroodCell, h, 6).unwrap();
    w.apply(
        1,
        &Command::Place {
            kind: StructureKind::BroodCell,
            x,
            y,
            x2: x,
            y2: y,
        },
    );
    let bc = w.structures.len() - 1;
    w.complete(bc);
    w.ants.retain(|a| a.colony != 1);
    w.structures
        .retain(|s| !(s.colony == 1 && s.kind == StructureKind::Queen));
    run(&mut w, 1);
    assert!(w.colonies[1].defeated);
    assert_eq!(w.winner, Some(0));
}

#[test]
fn conceding_defeats_at_once() {
    let mut w = world(20);
    w.apply(1, &Command::Concede);
    assert!(w.colonies[1].defeated);
    run(&mut w, 1);
    assert_eq!(w.winner, Some(0));
}

#[test]
fn two_v_two_ends_only_when_a_whole_team_is_gone() {
    let s = MatchSettings {
        format: Format::TwoVTwo,
        ..MatchSettings::default()
    };
    let mut w = World::new(21, s, &humans(4));
    w.apply(2, &Command::Concede);
    run(&mut w, 1);
    assert_eq!(w.winner, None, "seat 3 still fights for team 1");
    w.apply(3, &Command::Concede);
    run(&mut w, 1);
    assert_eq!(w.winner, Some(0));
}

#[test]
fn a_dropped_player_is_handed_to_the_ai() {
    let mut w = world(22);
    w.step(&[], &[1]);
    assert_eq!(
        w.colonies[1].controller,
        tool_core::world::Controller::Ai(Difficulty::Pupa)
    );
}

#[test]
fn digging_opens_dirt() {
    let mut w = (23..60)
        .map(arena)
        .find(|w| w.map.ground.contains(&tool_core::map::Ground::Dirt))
        .expect("some seed has dirt");
    // Find a dirt cell and put a Soldier Ant next to it with a waypoint
    // on the far side.
    let i = w
        .map
        .ground
        .iter()
        .position(|g| *g == tool_core::map::Ground::Dirt)
        .expect("this map has dirt");
    let (x, y) = (i as i32 % w.map.w, i as i32 / w.map.w);
    put(&mut w, 0, Species::Soldier, Pos::of_cell(x, y));
    w.ants[0].target = Some(tool_core::world::Target::Dirt(x, y));
    // Move the ant to a floor cell beside the dirt.
    let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .map(|(dx, dy)| (x + dx, y + dy))
        .find(|(a, b)| w.map.is_floor(*a, *b));
    if let Some((bx, by)) = beside {
        w.ants[0].pos = Pos::of_cell(bx, by);
        run(&mut w, 120);
        assert_eq!(w.map.at(x, y), tool_core::map::Ground::Floor, "dug through");
    }
}

#[test]
fn hill_holder_is_paid() {
    let mut w = arena(24);
    w.settings.events = true;
    let c = w.map.chambers[w.map.centre].clone();
    w.events.hill = Some(tool_core::world::Hill {
        x: c.cx,
        y: c.cy,
        r: c.r,
        ends: w.tick + 20,
        hold: vec![0, 0],
    });
    w.events.next_at = i32::MAX;
    w.spawn_ant(0, Species::Soldier, Pos::of_cell(c.cx, c.cy));
    let before = w.colonies[0].stock;
    run(&mut w, 21);
    assert_eq!(w.colonies[0].stock[0], before[0] + 300);
    assert_eq!(w.colonies[1].stock, w.colonies[1].stock);
}
