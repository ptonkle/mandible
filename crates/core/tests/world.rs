//! Behaviour tests for the simulation. Expected values are worked out from
//! the rules by hand, never copied from the simulation's own output.

use tool_core::fixed::{Pos, CELL};
use tool_core::rules::{ticks, Species, StructureKind, Upgrade};
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

/// An empty arena: both colonies keep only their queens, and Auto-Scout
/// (on by default) is off, so no ant leaves on its own.
fn arena(seed: u64) -> World {
    let mut w = world(seed);
    w.ants.clear();
    w.settings.events = false;
    for c in w.colonies.iter_mut() {
        c.auto_scout = false;
    }
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
    run(&mut w, ticks(400));
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
    run(&mut w, ticks(300));
    let g0 = w.colonies[0].tally.gathered;
    run(&mut w, ticks(100));
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
    for _ in 0..40 {
        w.spawn_ant(0, Species::Harvester, h);
    }
    w.colonies[0].focus = Focus::Food;
    // Everyone chooses on the first tick, before any inventory fills.
    run(&mut w, 1);
    let mut per = std::collections::BTreeMap::new();
    for a in &w.ants {
        if let Job::Gather(n) = a.job {
            *per.entry(n).or_insert(0) += 1;
        }
    }
    // Only the four home nodes are explored: 4 x 8 gatherers, 8 left idle.
    assert_eq!(
        per.values().copied().collect::<Vec<i32>>(),
        vec![8, 8, 8, 8],
        "{per:?}"
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
        run(&mut w, ticks(30));
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
    run(&mut w, ticks(50));
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
    run(&mut w, ticks(20));
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
    run(&mut w, ticks(200));
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
    run(&mut w, ticks(200) - 1);
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
    run(&mut w, ticks(260));
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
        .rfind(|s| !s.complete)
        .expect("placed")
        .id
}

#[test]
fn carpenters_apply_each_load_as_it_arrives() {
    // TRIAGE row 36: a Carpenter Ant brings its Leaves, then spends time
    // applying them, which fills the build bar. Work never runs ahead of the
    // Leaves delivered, but starts before the whole cost has arrived.
    let mut w = world(10);
    // One Carpenter Ant, so its loads arrive one after another.
    let mut first = true;
    w.ants.retain(|a| {
        if a.colony != 0 || a.species != Species::Carpenter {
            return true;
        }
        let keep = first;
        first = false;
        keep
    });
    let id = place_near_home(&mut w, StructureKind::BroodCell);
    // The cost left the stockpile at once: 200 - 60 Leaves.
    assert_eq!(w.colonies[0].stock[2], 140);
    let s = w.structures.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.delivered, [0, 0, 0]);
    let mut built_before_all_arrived = false;
    let mut last_work = 0;
    for _ in 0..ticks(600) {
        w.step(&[], &[]);
        let Some(s) = w.structures.iter().find(|s| s.id == id) else {
            break;
        };
        if s.complete {
            break;
        }
        assert!(
            s.work_done <= s.covered_work(),
            "work {} ran ahead of the Leaves delivered ({} covered)",
            s.work_done,
            s.covered_work()
        );
        assert!(s.work_done >= last_work, "the build bar never goes back");
        last_work = s.work_done;
        if s.work_done > 0 && !s.fully_delivered() {
            built_before_all_arrived = true;
        }
    }
    let s = w.structures.iter().find(|s| s.id == id).unwrap();
    assert!(s.complete, "the brood cell was built");
    // 60 Leaves at 50 per trip: the first load is applied while the second
    // is still on its way.
    assert!(built_before_all_arrived, "no waiting for the whole cost");
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
    run(&mut w, ticks(900));
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
    run(&mut w, ticks(25));
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
    run(&mut w, ticks(30));
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
                add: true, // Shift + click: three waypoints, not one
            },
        );
    }
    let wps = w.colonies[0].waypoints[Species::Soldier.index()].clone();
    assert_eq!(wps.len(), 3);
    run(&mut w, ticks(200));
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
            add: false,
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
            add: false,
        },
    );
    let centre = Pos::of_cell(cx, cy);
    run(&mut w, ticks(400));
    let far = w.ants.iter().map(|a| a.pos.dist(h)).min().unwrap();
    assert!(far > 20 * CELL, "they reached the centre");
    w.apply(0, &Command::Retreat { on: true });
    run(&mut w, ticks(400));
    assert!(
        w.ants.iter().all(|a| a.pos.dist(h) < 10 * CELL),
        "they came home"
    );
    w.apply(0, &Command::Retreat { on: false });
    run(&mut w, ticks(400));
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
        run(&mut w, ticks(120));
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

#[test]
fn all_ants_never_moves_gatherers_or_builders() {
    let mut w = world(25);
    let (hx, hy) = home(&w, 0).cell();
    w.apply(
        0,
        &Command::SetWaypoint {
            species: None,
            x: hx - 8,
            y: hy,
            add: false,
        },
    );
    for s in Species::ALL {
        let has = !w.colonies[0].waypoints[s.index()].is_empty();
        assert_eq!(has, s.is_combat(), "{s:?}");
    }
    assert!(w.colonies[0].waypoints[Species::Harvester.index()].is_empty());
}

#[test]
fn a_plain_waypoint_replaces_and_shift_adds() {
    let mut w = arena(26);
    let (hx, hy) = home(&w, 0).cell();
    let set = |w: &mut World, dy: i32, add: bool| {
        w.apply(
            0,
            &Command::SetWaypoint {
                species: Some(Species::Soldier),
                x: hx - 8,
                y: hy + dy,
                add,
            },
        )
    };
    set(&mut w, -6, false);
    set(&mut w, 0, false);
    assert_eq!(w.colonies[0].waypoints[Species::Soldier.index()].len(), 1);
    set(&mut w, 6, true);
    assert_eq!(w.colonies[0].waypoints[Species::Soldier.index()].len(), 2);
}

// ---- TRIAGE rows 12, 26, 10: carrying, expansions and long walls ----

/// Runs until every blueprint in `ids` is complete (or `limit` ticks) and
/// checks every Carpenter Ant trip: from taking Leaves at the queen to
/// handing them in, the ant walked at least half a cell and took at least
/// two ticks. Returns the number of trips.
fn trips_walk(w: &mut World, ids: &[u32], limit: i32) -> usize {
    use std::collections::BTreeMap;
    let mut start: BTreeMap<u32, (Pos, i32)> = BTreeMap::new();
    let mut trips = 0;
    for _ in 0..limit {
        let before: BTreeMap<u32, Job> = w.ants.iter().map(|a| (a.id, a.job)).collect();
        w.step(&[], &[]);
        for a in w.ants.iter().filter(|a| a.species == Species::Carpenter) {
            match (before.get(&a.id).copied(), a.job) {
                (Some(Job::Fetch(b)), Job::Haul(b2)) if b == b2 && ids.contains(&b) => {
                    start.insert(a.id, (a.pos, w.tick));
                }
                (Some(Job::Haul(b)), j) if j != Job::Haul(b) && ids.contains(&b) => {
                    if let Some((p, t)) = start.remove(&a.id) {
                        let walked = a.pos.dist(p);
                        assert!(
                            walked >= CELL / 2 && w.tick - t >= 2,
                            "Leaves handed in after {walked} sub-units and {} ticks",
                            w.tick - t
                        );
                        trips += 1;
                    }
                }
                _ => {}
            }
        }
        if ids
            .iter()
            .all(|id| w.structures.iter().all(|s| s.id != *id || s.complete))
        {
            break;
        }
    }
    trips
}

#[test]
fn leaves_are_walked_to_a_wall_beside_the_queen() {
    // TRIAGE row 12, worked by hand: seed 10, a 7-cell wall 6 cells from the
    // queen. A queen hands Leaves over within 2.5 + 0.44 + 1 = 3.94 cells
    // of its centre; a wall takes them within 0.5 + 0.44 + 0.5 = 1.44 cells
    // of its nearest cell's centre, so at least 0.6 cells lie between.
    let mut w = world(10);
    let (hx, hy) = home(&w, 0).cell();
    w.colonies[0].stock = [0, 0, 500];
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
    let ids: Vec<u32> = w
        .structures
        .iter()
        .filter(|s| !s.complete)
        .map(|s| s.id)
        .collect();
    assert_eq!(ids.len(), 1, "7 cells: one section");
    let trips = trips_walk(&mut w, &ids, ticks(900));
    assert!(trips >= 1, "the Leaves were carried ({trips} trips)");
    let walls = w
        .structures
        .iter()
        .filter(|s| s.kind == StructureKind::Wall && s.complete)
        .count();
    assert_eq!(walls, 7);
}

#[test]
fn leaves_are_walked_to_every_kind_placed_close_to_the_queen() {
    for kind in [
        StructureKind::BroodCell,
        StructureKind::Hatchery(Species::Fire),
        StructureKind::Hatchery(Species::Dinosaur),
    ] {
        let mut w = world(10);
        w.colonies[0].tier = 3;
        w.colonies[0].stock = [1000, 1000, 1000];
        // The closest spot placement allows.
        let h = home(&w, 0);
        let (x, y) = tool_core::ai::find_spot(&w, 0, kind, h, 1).expect("a spot");
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
        let id = w.structures.iter().rfind(|s| !s.complete).unwrap().id;
        let trips = trips_walk(&mut w, &[id], ticks(1200));
        assert!(trips >= 1, "{kind:?}: {trips} trips");
        assert!(
            w.structures.iter().any(|s| s.id == id && s.complete),
            "{kind:?} was built"
        );
    }
}

#[test]
fn only_leaves_are_carried_and_a_carpenter_carries_fifty() {
    // TRIAGE row 26: a Fire Ant Hatchery costs 120 Food and 100 Leaves. The
    // Food leaves the stockpile and counts as delivered at placement; the
    // 100 Leaves take two trips of 50.
    let mut w = world(10);
    w.colonies[0].tier = 2;
    w.colonies[0].stock = [1000, 1000, 1000];
    let id = place_near_home(&mut w, StructureKind::Hatchery(Species::Fire));
    assert_eq!(w.colonies[0].stock, [880, 1000, 900]);
    let s = w.structures.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.delivered, [120, 0, 0]);
    assert_eq!(s.still_needed(), [0, 0, 100]);
    let mut loads = Vec::new();
    for _ in 0..ticks(300) {
        let before: Vec<(u32, Job)> = w.ants.iter().map(|a| (a.id, a.job)).collect();
        w.step(&[], &[]);
        for a in &w.ants {
            if a.job == Job::Haul(id) && before.contains(&(a.id, Job::Fetch(id))) {
                loads.push(a.inventory);
            }
        }
    }
    assert_eq!(loads, vec![[0, 0, 50], [0, 0, 50]]);
    // Carry Capacity I: +30%, 65.
    w.colonies[0].ranks.carry_capacity = 1;
    assert_eq!(w.colonies[0].ranks.apply(Species::Carpenter).carry, 65);
}

/// A row of `n` cells where a wall line fits whole, for seat 0.
fn open_row(w: &World, n: i32) -> (i32, i32) {
    for y in 0..w.map.h {
        for x in 0..w.map.w - n {
            if w.wall_plan(0, x, y, x + n - 1, y).len() == n as usize {
                return (x, y);
            }
        }
    }
    panic!("no open row of {n}");
}

#[test]
fn a_long_wall_skips_rock_and_rises_in_sections() {
    // TRIAGE row 10: a 30-cell line (the old limit was 12) with 3 cells of
    // rock in it: 27 cells at 6 Leaves = 162, split into runs of 13 and 14,
    // and those into sections of at most 8: 8 + 5 and 8 + 6.
    let mut w = arena(10);
    let n = w.map.w * w.map.h;
    w.explored[0].set_all(n as usize);
    let (x, y) = open_row(&w, 30);
    for dx in 13..16 {
        let i = w.map.idx(x + dx, y);
        w.map.ground[i] = tool_core::map::Ground::Rock;
    }
    let h = home(&w, 0);
    for _ in 0..6 {
        w.spawn_ant(0, Species::Carpenter, h);
    }
    w.colonies[0].stock = [0, 0, 1000];
    w.apply(
        0,
        &Command::Place {
            kind: StructureKind::Wall,
            x,
            y,
            x2: x + 29,
            y2: y,
        },
    );
    assert_eq!(w.colonies[0].stock[2], 1000 - 162);
    let mut sizes: Vec<usize> = w
        .structures
        .iter()
        .filter(|s| !s.complete)
        .map(|s| s.cells.len())
        .collect();
    sizes.sort();
    assert_eq!(sizes, vec![5, 6, 8, 8]);
    let mut part_built = false;
    for _ in 0..ticks(3000) {
        w.step(&[], &[]);
        let built = w
            .structures
            .iter()
            .filter(|s| s.kind == StructureKind::Wall && s.complete)
            .count();
        let waiting = w.structures.iter().any(|s| !s.complete);
        part_built |= built > 0 && waiting;
        if !waiting {
            break;
        }
    }
    assert!(part_built, "some sections stood before the others");
    let built = w
        .structures
        .iter()
        .filter(|s| s.kind == StructureKind::Wall && s.complete)
        .count();
    assert_eq!(built, 27);
}

// ---- TRIAGE row 9: Auto-Scout ----

#[test]
fn auto_scout_sends_one_fire_ant_to_unseen_chambers() {
    let mut w = arena(30);
    let h = home(&w, 0);
    let soldier = w.spawn_ant(0, Species::Soldier, h);
    let fire1 = w.spawn_ant(0, Species::Fire, h);
    let fire2 = w.spawn_ant(0, Species::Fire, h);
    w.apply(0, &Command::Retreat { on: true });
    let seen = |w: &World| w.team_sighted(0).count();
    let seen0 = seen(&w);
    w.apply(0, &Command::AutoScout { on: true });
    w.step(&[], &[]);
    assert_eq!(w.colonies[0].scout, Some(fire1), "the lowest-id Fire Ant");
    run(&mut w, ticks(300));
    let scout = w.ants.iter().find(|a| a.id == fire1).expect("alive");
    assert!(scout.pos.dist(h) > 15 * CELL, "the scout left home");
    for id in [soldier, fire2] {
        let a = w.ants.iter().find(|a| a.id == id).unwrap();
        assert!(a.pos.dist(h) < 10 * CELL, "Retreat keeps the rest home");
    }
    assert!(seen(&w) > seen0 * 2, "{} vs {seen0} cells seen", seen(&w));
    // The scout dies: the other Fire Ant takes over.
    w.ants.retain(|a| a.id != fire1);
    w.step(&[], &[]);
    assert_eq!(w.colonies[0].scout, Some(fire2));
    w.apply(0, &Command::AutoScout { on: false });
    w.step(&[], &[]);
    assert_eq!(w.colonies[0].scout, None);
}

#[test]
fn the_scout_does_not_fight() {
    // An enemy Turtle Ant stands beside the scout: it walks on without
    // biting (a Fire Ant would hit it every 0.6 s).
    let mut w = arena(30);
    let h = home(&w, 0);
    let fire = put(&mut w, 0, Species::Fire, h);
    let enemy = put(&mut w, 1, Species::Turtle, near(h, 1, 0));
    let full = w.ants.iter().find(|a| a.id == enemy).unwrap().health;
    w.apply(0, &Command::AutoScout { on: true });
    run(&mut w, ticks(20));
    let e = w.ants.iter().find(|a| a.id == enemy).unwrap();
    assert_eq!(e.health, full, "the scout never attacked");
    let s = w.ants.iter().find(|a| a.id == fire);
    assert!(s.is_none_or(|s| s.pos.dist(h) > 3 * CELL), "it moved on");
}

#[test]
fn auto_scout_takes_a_soldier_without_fire_ants_and_stays_deterministic() {
    let mut a = world(31);
    a.apply(0, &Command::AutoScout { on: true });
    a.step(&[], &[]);
    let soldiers: Vec<u32> = a
        .ants
        .iter()
        .filter(|x| x.colony == 0 && x.species == Species::Soldier)
        .map(|x| x.id)
        .collect();
    assert_eq!(a.colonies[0].scout, soldiers.first().copied());
    let mut b = World::restore(&a.snapshot()).unwrap();
    for t in 0..ticks(300) {
        a.step(&[], &[]);
        b.step(&[], &[]);
        assert_eq!(a.checksum(), b.checksum(), "diverged at {t}");
    }
    assert_eq!(a.colonies[0].scout_goal, b.colonies[0].scout_goal);
}

// ---- TRIAGE row 15: Map Revealed ----

#[test]
fn map_revealed_explores_everything_but_shows_only_line_of_sight() {
    let s = MatchSettings {
        map_revealed: true,
        ..MatchSettings::default()
    };
    let w = World::new(32, s, &humans(2));
    let n = (w.map.w * w.map.h) as u32;
    for c in 0..2u8 {
        assert_eq!(w.explored[c as usize].count(), n, "seat {c} explored");
        assert!(w.visible[c as usize].count() < n / 4, "seat {c} visible");
        let enemy = home(&w, 1 - c as usize).cell();
        assert!(!w.team_visible(c).get(w.map.idx(enemy.0, enemy.1)));
        assert!(!w.team_sighted(c).get(w.map.idx(enemy.0, enemy.1)));
    }
    let plain = world(32);
    assert!(plain.explored[0].count() < n / 4);
}

#[test]
fn with_map_revealed_the_ai_does_not_scout() {
    // Pupa sends Soldier Ants to the nearest unexplored chamber from 25 s;
    // with the map revealed there is none.
    let scouted = |revealed: bool| {
        let s = MatchSettings {
            map_revealed: revealed,
            ..MatchSettings::default()
        };
        let mut w = World::new(33, s, &[SeatKind::Ai(Difficulty::Pupa), SeatKind::Human]);
        run(&mut w, ticks(400));
        !w.colonies[0].waypoints[Species::Soldier.index()].is_empty()
    };
    assert!(scouted(false));
    assert!(!scouted(true));
}

// ---- TRIAGE row 2: Sandbox ----

#[test]
fn sandbox_gives_humans_tier_three_and_unlimited_resources() {
    use tool_core::rules::SANDBOX_STOCK;
    let s = MatchSettings {
        sandbox: true,
        ..MatchSettings::default()
    };
    let mut w = World::new(34, s, &[SeatKind::Human, SeatKind::Ai(Difficulty::Pupa)]);
    assert_eq!(w.colonies[0].tier, 3);
    assert_eq!(w.colonies[0].stock, [SANDBOX_STOCK; 3]);
    assert_eq!(w.colonies[1].tier, 1);
    assert_eq!(w.colonies[1].stock, [300, 50, 200]);
    w.step(
        &[(
            0,
            Command::Train {
                species: Species::Soldier,
                amount: Amount::Ten,
            },
        )],
        &[],
    );
    let queued: usize = w
        .structures
        .iter()
        .filter(|s| s.colony == 0)
        .map(|s| s.lane.len())
        .sum();
    assert_eq!(queued, 10, "queueing works the same");
    assert_eq!(w.colonies[0].stock, [SANDBOX_STOCK; 3], "refilled");
}

// ---- TRIAGE row 24: twice the speed ----

#[test]
fn ants_walk_twice_as_fast_and_durations_keep_their_real_time() {
    use tool_core::rules::TICKS_PER_SECOND;
    // A Soldier Ant moved 77 sub-units a tick at 10 ticks a second, 3 cells
    // a second; now 6 (77 x 20 / 256).
    let st = Species::Soldier.stats();
    assert_eq!(st.speed * TICKS_PER_SECOND / CELL, 6);
    // A Soldier Ant still trains in 8 s.
    assert_eq!(st.train_ticks, 8 * TICKS_PER_SECOND);
    // Walked for real: 4 s along open floor covers about 24 cells.
    let mut w = arena(35);
    let n = w.map.w * w.map.h;
    w.explored[0].set_all(n as usize);
    let (x, y) = open_row(&w, 30);
    let id = put(&mut w, 0, Species::Soldier, Pos::of_cell(x, y));
    w.apply(
        0,
        &Command::SetWaypoint {
            species: Some(Species::Soldier),
            x: x + 29,
            y,
            add: false,
        },
    );
    run(&mut w, 4 * TICKS_PER_SECOND);
    let a = w.ants.iter().find(|a| a.id == id).unwrap();
    let cells = a.pos.dist(Pos::of_cell(x, y)) / CELL;
    assert!((20..=25).contains(&cells), "walked {cells} cells in 4 s");
}

#[test]
fn colonies_record_a_sample_every_ten_seconds() {
    let mut w = world(30);
    run(&mut w, tool_core::world::SAMPLE_TICKS * 3);
    let h = &w.colonies[0].history;
    assert_eq!(h.len(), 3);
    assert_eq!(h[2].tick, tool_core::world::SAMPLE_TICKS * 3);
    // Six Harvester Ants, two Carpenter Ants: 8 workers; 2 Soldier Ants: 2 army.
    assert_eq!(h[0].workers, 8);
    assert_eq!(h[0].army, 2);
    assert!(h[2].gathered.iter().sum::<i32>() >= h[0].gathered.iter().sum::<i32>());
}

// ---- TRIAGE row 38: Retreat and walls ----

/// Builds a finished wall on every cell between `r0` and `r0 + 2` cells
/// from `seat`'s home centre where a wall may be placed: a full ring round
/// the nest, as close as placement allows. With `closed`, every floor
/// cell of the ring gets a wall, so nothing walks round it.
fn wall_ring(w: &mut World, owner: u8, seat: usize, r0: i32, closed: bool) -> usize {
    let h = w.map.home(seat).clone();
    let n = (w.map.w * w.map.h) as usize;
    w.explored[owner as usize].set_all(n);
    let mut cells = Vec::new();
    for y in h.cy - r0 - 3..=h.cy + r0 + 3 {
        for x in h.cx - r0 - 3..=h.cx + r0 + 3 {
            let d2 = (x - h.cx).pow(2) + (y - h.cy).pow(2);
            if d2 >= r0 * r0
                && d2 < (r0 + 2).pow(2)
                && (if closed {
                    w.map.is_floor(x, y)
                } else {
                    w.wall_plan(owner, x, y, x, y).len() == 1
                })
            {
                cells.push((x, y));
            }
        }
    }
    for &(x, y) in &cells {
        w.add_structure(
            owner,
            StructureKind::Wall,
            Pos::of_cell(x, y),
            vec![(x, y)],
            true,
        );
    }
    cells.len()
}

/// Spawns `n` combat ants of `colony` in the centre chamber.
fn army_at_centre(w: &mut World, colony: u8, n: usize) -> Vec<u32> {
    let c = w.map.chambers[w.map.centre].clone();
    (0..n)
        .map(|k| {
            let sp = [Species::Soldier, Species::Soldier, Species::Turtle][k % 3];
            let p = Pos::of_cell(c.cx + k as i32 % 5 - 2, c.cy + k as i32 / 5 - 3);
            put(w, colony, sp, p)
        })
        .collect()
}

fn on_a_wall(w: &World, ids: &[u32]) -> usize {
    w.ants
        .iter()
        .filter(|a| ids.contains(&a.id))
        .filter(|a| {
            let (x, y) = a.pos.cell();
            w.wall[w.map.idx(x, y)] != tool_core::nav::NO_WALL
        })
        .count()
}

fn farthest(w: &World, ids: &[u32], from: Pos) -> i32 {
    w.ants
        .iter()
        .filter(|a| ids.contains(&a.id))
        .map(|a| a.pos.dist(from) / CELL)
        .max()
        .unwrap_or(0)
}

#[test]
fn retreating_ants_pass_their_walls_and_never_stop_on_one() {
    // TRIAGE row 38. The closest a wall may stand to a queen is about 6
    // cells, and Retreat used to stop ants anywhere within 6.5 cells of the
    // queen: an army came home and stood on the ring of walls, looking stuck
    // on it. Own walls are gates; ants pass them and wait inside.
    let mut w = arena(17);
    let ring = wall_ring(&mut w, 0, 0, 6, false);
    assert!(ring > 40, "a ring of {ring} walls");
    let ids = army_at_centre(&mut w, 0, 30);
    let h = home(&w, 0);
    w.apply(0, &Command::Retreat { on: true });
    run(&mut w, ticks(600));
    let far = farthest(&w, &ids, h);
    assert!(far <= 8, "every ant came home ({far} cells out)");
    assert_eq!(on_a_wall(&w, &ids), 0, "ants standing on a wall");
    // Idle at the nest (no waypoint) they do not stand on walls either.
    w.apply(0, &Command::Retreat { on: false });
    run(&mut w, ticks(200));
    assert_eq!(on_a_wall(&w, &ids), 0, "idle ants standing on a wall");
}

#[test]
fn an_ally_wall_lets_a_retreat_through_and_is_never_attacked() {
    // In 2v2 a wall is a gate for its whole team: a retreating ally walks
    // through it rather than standing at it, and no ally breaks it.
    let s = MatchSettings {
        format: Format::TwoVTwo,
        events: false,
        ..MatchSettings::default()
    };
    let mut w = World::new(18, s, &humans(4));
    w.ants.clear();
    for c in w.colonies.iter_mut() {
        c.auto_scout = false;
    }
    // Seat 1 walls in its nest. Seat 0 loses its own queen and gets one
    // inside seat 1's ring, so its retreat must cross the ally walls.
    let ring = wall_ring(&mut w, 1, 1, 8, true);
    assert!(ring > 40, "a ring of {ring} walls");
    w.structures
        .retain(|s| !(s.colony == 0 && s.kind == StructureKind::Queen));
    let h1 = w.map.home(1).clone();
    w.add_structure(
        0,
        StructureKind::Queen,
        Pos::of_cell(h1.cx + 1, h1.cy),
        vec![],
        true,
    );
    let ids = army_at_centre(&mut w, 0, 12);
    let walls = |w: &World| -> i32 {
        w.structures
            .iter()
            .filter(|s| s.kind == StructureKind::Wall)
            .map(|s| s.health)
            .sum()
    };
    let before = walls(&w);
    w.apply(0, &Command::Retreat { on: true });
    run(&mut w, ticks(900));
    let far = farthest(&w, &ids, Pos::of_cell(h1.cx, h1.cy));
    assert!(
        far <= 9,
        "every ant reached the nest inside the ring ({far} cells out)"
    );
    assert_eq!(walls(&w), before, "an ally wall was hit");
}

#[test]
fn a_retreat_walled_out_by_an_enemy_breaks_through() {
    // With no way home but through enemy walls, retreating ants break them
    // instead of standing at them for ever.
    let mut w = arena(17);
    wall_ring(&mut w, 1, 0, 8, true);
    let ids = army_at_centre(&mut w, 0, 12);
    w.apply(0, &Command::Retreat { on: true });
    run(&mut w, ticks(1200));
    let far = farthest(&w, &ids, home(&w, 0));
    assert!(far <= 8, "every ant came home ({far} cells out)");
}

// ---- TRIAGE row 54: idle Carpenter Ants ----

#[test]
fn idle_carpenters_wait_spread_evenly_round_the_queens() {
    // Six idle Carpenter Ants and two queens: three wait at each queen, each
    // on its own spot round it, never in one clump.
    let mut w = arena(10);
    let c = w.map.chambers[w.map.centre].clone();
    let q2 = Pos::of_cell(c.cx, c.cy);
    w.add_structure(0, StructureKind::Queen, q2, vec![], true);
    let q1 = home(&w, 0);
    let ids: Vec<u32> = (0..6)
        .map(|_| w.spawn_ant(0, Species::Carpenter, q1))
        .collect();
    run(&mut w, ticks(900));
    let carps: Vec<Pos> = w
        .ants
        .iter()
        .filter(|a| ids.contains(&a.id))
        .map(|a| a.pos)
        .collect();
    let at = |q: Pos| carps.iter().filter(|p| p.dist(q) <= 8 * CELL).count();
    assert_eq!((at(q1), at(q2)), (3, 3), "Carpenter Ants per queen");
    let mut closest = i32::MAX;
    for (i, a) in carps.iter().enumerate() {
        for b in &carps[i + 1..] {
            closest = closest.min(a.dist(*b));
        }
    }
    assert!(
        closest >= 2 * CELL,
        "two Carpenter Ants {} tenths of a cell apart",
        closest * 10 / CELL
    );
}

// ---- TRIAGE row 45: Auto-Scout from the start ----

#[test]
fn auto_scout_is_on_from_the_start_for_every_colony() {
    let seats = [SeatKind::Human, SeatKind::Ai(Difficulty::Imago)];
    let mut w = World::new(5, MatchSettings::default(), &seats);
    assert!(w.colonies.iter().all(|c| c.auto_scout));
    w.step(&[], &[]);
    for c in 0..2 {
        let scout = w.colonies[c].scout.expect("a scout from the first tick");
        let a = w.ants.iter().find(|a| a.id == scout).unwrap();
        assert_eq!(a.species, Species::Soldier, "the only combat ants");
    }
}

// ---- TRIAGE row 46: queen placement ----

/// Whether a queen at (x, y) has what the rule asks for, worked out here
/// from the map alone: floor under a disk of radius 3 (footprint 2 plus
/// room 1), no node within 4.5 cells (2.5 footprint + 1.5 + a half for
/// the node), and at least `gap` cells from every queen.
fn plainly_free(w: &World, x: i32, y: i32, gap: i32) -> bool {
    let floor = (-3..=3).all(|dy: i32| {
        (-3..=3).all(|dx: i32| dx * dx + dy * dy > 12 || w.map.is_floor(x + dx, y + dy))
    });
    let p = Pos::of_cell(x, y);
    floor
        && w.nodes.iter().all(|n| n.pos.dist(p) >= 5 * CELL)
        && w.structures.iter().all(|s| s.pos.dist(p) >= gap * CELL)
}

fn queen_ok(w: &World, x: i32, y: i32) -> bool {
    w.check_place(0, StructureKind::Queen, x, y, x, y).is_ok()
}

#[test]
fn a_queen_fits_on_chamber_floor_outside_the_planned_circle() {
    // The old rule asked for a spot inside a chamber's planned circle, but
    // chamber outlines bulge past it: floor that is plainly chamber was
    // refused. Every free spot of drawn floor near a chamber now takes one.
    let mut w = world(4);
    let n = (w.map.w * w.map.h) as usize;
    w.explored[0].set_all(n);
    let mut tried = 0;
    for c in w.map.chambers.clone() {
        for y in c.cy - c.r - 5..=c.cy + c.r + 5 {
            for x in c.cx - c.r - 5..=c.cx + c.r + 5 {
                let d2 = (x - c.cx).pow(2) + (y - c.cy).pow(2);
                let outside = d2 > c.r * c.r && w.map.chamber_at(x, y).is_none();
                if outside && d2 <= (c.r + 4).pow(2) && plainly_free(&w, x, y, 12) {
                    tried += 1;
                    assert!(queen_ok(&w, x, y), "refused at {x},{y} by chamber {c:?}");
                }
            }
        }
    }
    assert!(tried > 20, "only {tried} spots tried");
}

#[test]
fn a_queen_fits_on_open_ground_away_from_chambers() {
    let s = MatchSettings {
        map_type: tool_core::settings::MapType::Open,
        ..MatchSettings::default()
    };
    let mut w = World::new(6, s, &humans(2));
    let n = (w.map.w * w.map.h) as usize;
    w.explored[0].set_all(n);
    let mut tried = 0;
    for y in (4..w.map.h - 4).step_by(3) {
        for x in (4..w.map.w - 4).step_by(3) {
            if w.map.chamber_at(x, y).is_none() && plainly_free(&w, x, y, 12) {
                tried += 1;
                assert!(queen_ok(&w, x, y), "refused at {x},{y}");
            }
        }
    }
    assert!(tried > 100, "only {tried} spots tried");
}

#[test]
fn queens_need_nine_cells_between_centres_and_ignore_enemy_pickup_reach() {
    // Two of the old refusals that looked wrong: a queen 10 cells from
    // another (the old spacing was 14), and a queen beside an enemy brood
    // cell (the pickup clearance only matters for a colony's own Carpenter
    // Ants). Still refused: 8 cells from a queen, a node under it, rock in
    // its room, unexplored ground.
    let mut w = arena(4);
    let n = (w.map.w * w.map.h) as usize;
    w.explored[0].set_all(n);
    let h = w.map.home(0).clone();
    // A spot 10 cells from the home queen.
    let spot = (0..64)
        .map(|a: usize| {
            (
                h.cx + 11 * tool_core::fixed::cos64(a) / 1024,
                h.cy + 11 * tool_core::fixed::sin64(a) / 1024,
            )
        })
        .find(|&(x, y)| {
            let d = Pos::of_cell(x, y).dist(home(&w, 0));
            plainly_free(&w, x, y, 9) && (10 * CELL..11 * CELL).contains(&d)
        })
        .expect("a free spot 10 cells out");
    assert!(queen_ok(&w, spot.0, spot.1), "10 cells from the queen");
    // An enemy brood cell 5 cells away does not matter.
    let (x, y) = spot;
    let dir = (x - h.cx, y - h.cy);
    let len = Pos::of_cell(0, 0).dist(Pos::of_cell(dir.0, dir.1));
    let at = Pos::of_cell(x, y);
    let side = Pos::new(
        at.x - (5 * CELL + CELL / 4) * dir.1 * CELL / len,
        at.y + (5 * CELL + CELL / 4) * dir.0 * CELL / len,
    );
    w.add_structure(1, StructureKind::BroodCell, side, vec![], true);
    let gap = side.dist(at) / CELL;
    assert_eq!(gap, 5, "the enemy brood cell is 5 cells away");
    assert!(queen_ok(&w, x, y), "beside an enemy brood cell");
    // Still refused.
    let near = (h.cx + 8 * dir.0 / 10, h.cy + 8 * dir.1 / 10);
    assert!(!queen_ok(&w, near.0, near.1), "8 cells from a queen");
    let node = w.nodes.iter().find(|n| !n.cache).unwrap().pos.cell();
    assert!(!queen_ok(&w, node.0, node.1), "on a node");
    w.explored[0] = tool_core::fog::Bits::new(n);
    assert!(!queen_ok(&w, x, y), "unexplored");
}

#[test]
fn a_queen_needs_a_ring_of_open_ground_round_its_footprint() {
    let mut w = arena(4);
    let n = (w.map.w * w.map.h) as usize;
    w.explored[0].set_all(n);
    // A free spot whose room ring meets rock: right beside a rock face.
    let mut found = None;
    'scan: for y in 4..w.map.h - 4 {
        for x in 4..w.map.w - 4 {
            let p = Pos::of_cell(x, y);
            let foot = (-2..=2).all(|dy: i32| {
                (-2..=2).all(|dx: i32| dx * dx + dy * dy > 6 || w.map.is_floor(x + dx, y + dy))
            });
            let rock_in_ring = !w.map.is_floor(x + 3, y);
            let clear = w.nodes.iter().all(|n| n.pos.dist(p) >= 5 * CELL)
                && w.structures.iter().all(|s| s.pos.dist(p) >= 12 * CELL);
            if foot && rock_in_ring && clear {
                found = Some((x, y));
                break 'scan;
            }
        }
    }
    let (x, y) = found.expect("a spot against rock");
    assert!(!queen_ok(&w, x, y), "a queen pressed against rock");
}

#[test]
fn an_enemy_behind_rock_does_not_hold_an_ant_against_it() {
    // An ant used to take any enemy in reach as its target and walk at it
    // in a straight line, even through rock: it pressed against the rock
    // for good (seen as ants stuck on walls, and in SECOND-ORDER row 42 as
    // an army never found). An enemy it cannot walk straight to is left
    // alone; the ant goes on to its waypoint.
    let mut w = arena(17);
    let h = w.map.home(0).clone();
    // A rock wall across the home chamber, 3 cells east of the middle.
    for y in h.cy - 12..=h.cy + 12 {
        let i = w.map.idx(h.cx + 3, y);
        w.map.ground[i] = tool_core::map::Ground::Rock;
    }
    w.nav.invalidate();
    w.add_structure(
        1,
        StructureKind::BroodCell,
        Pos::of_cell(h.cx + 6, h.cy),
        vec![],
        true,
    );
    let id = put(&mut w, 0, Species::Soldier, Pos::of_cell(h.cx + 1, h.cy));
    let (wx, wy) = (h.cx - 8, h.cy + 6);
    w.apply(
        0,
        &Command::SetWaypoint {
            species: Some(Species::Soldier),
            x: wx,
            y: wy,
            add: false,
        },
    );
    run(&mut w, ticks(100));
    let a = w.ants.iter().find(|a| a.id == id).unwrap();
    assert!(
        a.pos.dist(Pos::of_cell(wx, wy)) <= 4 * CELL,
        "the Soldier Ant stayed at {:?}, target {:?}",
        a.pos.cell(),
        a.target
    );
}

// ---- TRIAGE row 52: less Nectar early ----

#[test]
fn balanced_sends_a_sixth_of_harvesters_to_nectar_and_starts_with_little() {
    // Balanced is Food 3 : Nectar 1 : Leaves 2. Worked by hand for 12 idle
    // Harvester Ants choosing in turn, each taking the resource with the
    // most weight per ant already on it (ties to Food, then Nectar): F L F
    // F N L F L F F N L, so 6 Food, 2 Nectar, 4 Leaves.
    let mut w = arena(4);
    let h = home(&w, 0);
    for _ in 0..12 {
        w.spawn_ant(0, Species::Harvester, h);
    }
    assert_eq!(w.colonies[0].focus, Focus::Balanced);
    run(&mut w, 1);
    let mut by = [0; 3];
    for a in &w.ants {
        if let Job::Gather(n) = a.job {
            let r = w.nodes.iter().find(|x| x.id == n).unwrap().resource;
            by[r.index()] += 1;
        }
    }
    assert_eq!(by, [6, 2, 4]);
    // Normal Starting Resources: 50 Nectar; the home Flower holds 1,000.
    let w = world(4);
    assert_eq!(w.colonies[0].stock, [300, 50, 200]);
    let h = w.map.home(0).clone();
    let flower: i32 = w
        .nodes
        .iter()
        .filter(|n| n.resource == tool_core::rules::Resource::Nectar)
        .filter(|n| n.pos.dist(Pos::of_cell(h.cx, h.cy)) <= h.r * CELL)
        .map(|n| n.amount)
        .sum();
    assert_eq!(flower, 1000);
}

#[test]
fn an_ai_with_nothing_left_to_defend_stops_scouting() {
    // The AI keeps Auto-Scout on (it scouts chambers for nests and finds
    // enemies to attack) until its last queen and hatchery are gone; then
    // every ant left fights, rather than one wandering the map and making
    // the match drag on.
    let seats = [SeatKind::Human, SeatKind::Ai(Difficulty::Pupa)];
    let mut w = World::new(5, MatchSettings::default(), &seats);
    run(&mut w, ticks(50));
    assert!(w.colonies[1].auto_scout, "on while it has a queen");
    w.structures.retain(|s| s.colony != 1);
    run(&mut w, ticks(50));
    assert!(!w.colonies[1].auto_scout, "off with nothing to defend");
    assert!(
        w.colonies[0].auto_scout,
        "a player's Auto-Scout is untouched"
    );
}
