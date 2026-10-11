//! Tests for `render.rs`: the draw list, shapes, images and camera.

use super::*;
use crate::settings::{Format, MatchSettings, SeatKind};
use crate::world::Command;

fn world(format: Format) -> World {
    let s = MatchSettings {
        format,
        events: false,
        ..MatchSettings::default()
    };
    let seats = vec![SeatKind::Human; format.seats()];
    World::new(7, s, &seats)
}

/// Splits a draw list into (op, arguments), panicking on an unknown op or a
/// list that ends inside an op.
fn decode(list: &[i32]) -> Vec<(i32, Vec<i32>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        let code = list[i];
        let (fixed, per_item) =
            op::layout(code).unwrap_or_else(|| panic!("unknown op {code} at {i}"));
        let mut n = fixed;
        if per_item > 0 {
            n += per_item * list[i + fixed] as usize;
        }
        assert!(
            i + 1 + n <= list.len(),
            "op {code} at {i} runs past the end"
        );
        out.push((code, list[i + 1..i + 1 + n].to_vec()));
        i += 1 + n;
    }
    out
}

fn area(points: &[i32]) -> i64 {
    let pts: Vec<Pt> = points.chunks(2).map(|c| (c[0], c[1])).collect();
    signed_area(&pts).abs() / 2
}

fn camera_on(p: Pos, zoom: usize) -> Camera {
    let mut cam = Camera::new(800, 600);
    cam.centre_x = p.x;
    cam.centre_y = p.y;
    cam.zoom = zoom;
    cam
}

fn queen_of(w: &World, colony: u8) -> usize {
    w.structures
        .iter()
        .position(|s| s.colony == colony && s.kind == StructureKind::Queen)
        .expect("a queen")
}

fn ant_shapes_drawn(list: &[i32], colour: i32) -> Vec<Vec<i32>> {
    decode(list)
        .into_iter()
        .filter(|(code, a)| {
            (*code == op::SHAPE || *code == op::FILL_SHAPE) && a[0] < ANT_SHAPES && a[4] == colour
        })
        .map(|(_, a)| a)
        .collect()
}

#[test]
fn draw_list_names_only_known_ops() {
    let mut w = world(Format::TwoVTwo);
    let c = w.map.chambers[w.map.centre].clone();
    w.apply(
        0,
        &Command::SetWaypoint {
            species: None,
            x: c.cx,
            y: c.cy,
            add: false,
        },
    );
    for _ in 0..30 {
        w.step(&[], &[]);
    }
    let q = w.structures[queen_of(&w, 0)].pos;
    let (qx, qy) = q.cell();
    w.add_structure(
        0,
        StructureKind::Wall,
        Pos::of_cell(qx + 6, qy),
        vec![(qx + 6, qy)],
        true,
    );
    w.add_structure(
        0,
        StructureKind::BroodCell,
        Pos::of_cell(qx - 6, qy),
        Vec::new(),
        false,
    );
    let assets: serde_json::Value = serde_json::from_str(&assets_json()).expect("assets are JSON");
    let shapes = assets["shapes"].as_array().expect("shapes").len() as i32;
    let palette = PALETTE.len() as i32;
    let overlay = Overlay {
        terrain_scale: 0,
        place: Some(Placing {
            colony: 0,
            kind: StructureKind::Wall,
            x: qx,
            y: qy + 5,
            x2: qx + 8,
            y2: qy + 5,
            valid: false,
        }),
        hover_waypoint: Some(Pos::of_cell(c.cx, c.cy)),
    };
    for zoom in [0, 7, 14] {
        for viewer in [None, Some(0), Some(3)] {
            let mut cam = Camera::new(1000, 700);
            cam.zoom = zoom;
            cam.centre_x = q.x;
            cam.centre_y = q.y;
            let list = frame(&w, viewer, &cam, &overlay, 400);
            let ops = decode(&list);
            assert_eq!(ops[0].0, op::CLEAR);
            for (code, a) in &ops {
                match *code {
                    op::CLEAR | op::POLY | op::OUTLINE | op::LINE => assert!(a[0] < palette),
                    op::IMAGE => assert!([IMAGE_TERRAIN, IMAGE_FOG].contains(&a[0])),
                    op::SHAPE => {
                        assert!(a[0] < shapes && a[4] < palette && a[5] < palette);
                        assert!(a[3] > 0 && a[3] <= SCALE_MAX);
                    }
                    op::FILL_SHAPE => {
                        assert!(a[0] < shapes && a[4] < palette);
                        assert!(a[3] > 0 && a[3] <= SCALE_MAX);
                    }
                    op::TEXT => {
                        assert!(a[2] > 0 && a[3] < palette && a[4] < palette && a[5] > 0);
                        assert!((a[6] as usize) < op::ALIGNS.len());
                        assert!(a[8..].iter().all(|c| char::from_u32(*c as u32).is_some()));
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn every_species_has_sixteen_headings() {
    let all = shapes();
    assert_eq!(all.len() as i32, ANT_SHAPES + 6);
    for s in Species::ALL {
        for detailed in [false, true] {
            let mut seen: Vec<&Shape> = Vec::new();
            for h in 0..HEADINGS as u8 {
                let shape = &all[ant_shape_id(s, h, detailed) as usize];
                assert!(!shape.is_empty());
                assert!(!seen.contains(&shape), "{s:?} heading {h} repeats another");
                seen.push(shape);
            }
        }
        // Headings follow `heading16`: 0 faces east and they turn clockwise
        // on screen, so the abdomen (the first part of a plain shape) trails
        // west, then north, then east.
        let abdomen = |h: u8| {
            let part = &all[ant_shape_id(s, h, false) as usize][0];
            let n = part.len() as i32;
            (
                part.iter().map(|p| p.0).sum::<i32>() / n,
                part.iter().map(|p| p.1).sum::<i32>() / n,
            )
        };
        assert!(abdomen(0).0 < 0, "{s:?} heading 0 does not face east");
        assert!(abdomen(4).1 < 0, "{s:?} heading 4 does not face south");
        assert!(abdomen(8).0 > 0, "{s:?} heading 8 does not face west");
        assert!(abdomen(12).1 > 0, "{s:?} heading 12 does not face north");
    }
    // Every species has its own silhouette.
    for a in Species::ALL {
        for b in Species::ALL {
            if a != b {
                assert_ne!(
                    ant_template(a, false),
                    ant_template(b, false),
                    "{a:?} {b:?}"
                );
            }
        }
    }
}

#[test]
fn health_pie_shrinks_with_health() {
    let mut w = world(Format::OneVOne);
    let qi = queen_of(&w, 0);
    let cam = camera_on(w.structures[qi].pos, 10);
    let max = w.structures[qi].max_health;
    let mut areas = Vec::new();
    for health in [max, max * 3 / 4, max / 2, max / 10] {
        w.structures[qi].health = health;
        let list = frame(&w, Some(0), &cam, &Overlay::default(), 0);
        let pies: Vec<i64> = decode(&list)
            .into_iter()
            .filter(|(code, a)| *code == op::POLY && a[0] == player(0))
            .map(|(_, a)| area(&a[2..]))
            .collect();
        assert_eq!(pies.len(), 1, "one health pie for the queen");
        areas.push(pies[0]);
    }
    assert!(areas.windows(2).all(|p| p[1] < p[0]), "{areas:?}");
    let ratio = areas[1] * 100 / areas[0];
    assert!(
        (70..=80).contains(&ratio),
        "three quarters health fills {ratio}%"
    );
}

/// Total length of the player-colour rim lines in a draw list, and whether a
/// closed player-colour rim (a whole outline) is there.
fn rim(list: &[i32], colony: u8) -> (i64, bool) {
    let mut len = 0;
    let mut closed = false;
    for (code, a) in decode(list) {
        if a[0] != player(colony) {
            continue;
        }
        if code == op::OUTLINE {
            closed = true;
        }
        if code == op::LINE {
            let pts: Vec<Pt> = a[3..].chunks(2).map(|c| (c[0], c[1])).collect();
            for p in pts.windows(2) {
                let (dx, dy) = ((p[1].0 - p[0].0) as i64, (p[1].1 - p[0].1) as i64);
                len += isqrt((dx * dx + dy * dy) as u64) as i64;
            }
        }
    }
    (len, closed)
}

#[test]
fn a_blueprint_pie_fills_smoothly_with_work_and_its_rim_with_delivery() {
    let mut w = world(Format::OneVOne);
    let q = w.structures[queen_of(&w, 0)].pos;
    let (qx, qy) = q.cell();
    let id = w.add_structure(
        0,
        StructureKind::BroodCell,
        Pos::of_cell(qx + 5, qy),
        Vec::new(),
        false,
    );
    w.add_structure(
        0,
        StructureKind::Wall,
        Pos::of_cell(qx - 5, qy),
        vec![(qx - 5, qy)],
        true,
    );
    let cam = camera_on(q, 10);
    let draw = |w: &World| frame(w, Some(0), &cam, &Overlay::default(), 0);
    let soft_area = |w: &World| -> i64 {
        decode(&draw(w))
            .into_iter()
            .filter(|(code, a)| *code == op::POLY && a[0] == soft(0))
            .map(|(_, a)| area(&a[2..]))
            .sum()
    };
    let si = w.structure_index(id).unwrap();
    assert_eq!(soft_area(&w), 0, "no work done yet");
    assert_eq!(rim(&draw(&w), 0), (0, false), "nothing delivered yet");
    // Delivery brightens the rim, a part at a time, and never the pie.
    let cost = w.structures[si].cost;
    w.structures[si].delivered = [cost[0] / 2, cost[1] / 2, cost[2] / 2];
    let (half_rim, closed) = rim(&draw(&w), 0);
    assert!(half_rim > 0 && !closed, "half delivered: an open rim");
    assert_eq!(soft_area(&w), 0, "delivery alone fills no pie");
    w.structures[si].delivered = cost;
    assert!(rim(&draw(&w), 0).1, "all delivered: the whole rim");
    // Build work fills the pie a little with every tick of work.
    let work = w.structures[si].work;
    let mut areas = Vec::new();
    for done in 0..=work {
        w.structures[si].work_done = done;
        areas.push(soft_area(&w));
    }
    let full = areas[work as usize];
    let half = areas[work as usize / 2];
    assert!(
        (full * 45 / 100..=full * 55 / 100).contains(&half),
        "half the work fills half the pie: {half} of {full}"
    );
    let jump = areas.windows(2).map(|p| p[1] - p[0]).max().unwrap();
    assert!(
        areas.windows(2).all(|p| p[1] >= p[0]) && jump <= full * 3 / work as i64,
        "the pie never shrinks or jumps: largest step {jump} of {full} over {work} ticks"
    );
    // The wall is a player-colour square, distinct from rock.
    let walls = decode(&draw(&w))
        .into_iter()
        .filter(|(code, a)| *code == op::POLY && a[0] == player(0) && a[1] == 4)
        .count();
    assert_eq!(walls, 1);
}

#[test]
fn a_wall_blueprint_fills_cell_by_cell() {
    let mut w = world(Format::OneVOne);
    let q = w.structures[queen_of(&w, 0)].pos;
    let (qx, qy) = q.cell();
    let cells: Vec<(i32, i32)> = (0..4).map(|k| (qx + 4 + k, qy + 4)).collect();
    let id = w.add_structure(
        0,
        StructureKind::Wall,
        Pos::of_cell(qx + 5, qy + 4),
        cells,
        false,
    );
    let si = w.structure_index(id).unwrap();
    w.structures[si].work_done = w.structures[si].work / 2;
    let cam = camera_on(q, 10);
    let pies: Vec<i64> = decode(&frame(&w, Some(0), &cam, &Overlay::default(), 0))
        .into_iter()
        .filter(|(code, a)| *code == op::POLY && a[0] == soft(0))
        .map(|(_, a)| area(&a[2..]))
        .collect();
    assert_eq!(
        pies.len(),
        2,
        "half the work: the first two cells, {pies:?}"
    );
    assert_eq!(pies[0], pies[1]);
}

#[test]
fn off_screen_ants_are_culled() {
    let mut w = world(Format::OneVOne);
    let q = w.structures[queen_of(&w, 0)].pos;
    let far = Pos::new(q.x + 60 * CELL, q.y);
    let i = w.ants.iter().position(|a| a.colony == 0).unwrap();
    w.ants[i].pos = far;
    w.ants[i].prev = far;
    let cam = camera_on(q, 12);
    let own = w.ants.iter().filter(|a| a.colony == 0).count();
    let drawn = ant_shapes_drawn(&frame(&w, None, &cam, &Overlay::default(), 0), player(0));
    assert_eq!(drawn.len(), own - 1, "every ant but the far one");
    for a in &drawn {
        assert!(a[1] > -800 && a[1] < 800 * 17 && a[2] > -800 && a[2] < 600 * 17);
    }
}

#[test]
fn enemy_ants_show_only_where_the_team_sees() {
    let mut w = world(Format::OneVOne);
    let q = w.structures[queen_of(&w, 0)].pos;
    let mut cam = Camera::new(800, 600);
    cam.fit(w.map.w, w.map.h);
    let enemies = |w: &World| {
        ant_shapes_drawn(&frame(w, Some(0), &cam, &Overlay::default(), 0), player(1)).len()
    };
    assert_eq!(enemies(&w), 0, "the enemy home is out of sight");
    let everyone =
        ant_shapes_drawn(&frame(&w, None, &cam, &Overlay::default(), 0), player(1)).len();
    assert!(everyone > 0);
    let i = w.ants.iter().position(|a| a.colony == 1).unwrap();
    w.ants[i].pos = q;
    w.ants[i].prev = q;
    assert_eq!(enemies(&w), 1, "an enemy at our queen is seen");
}

#[test]
fn ants_are_drawn_between_ticks() {
    let mut w = world(Format::OneVOne);
    w.ants.truncate(1);
    let p = w.ants[0].pos;
    w.ants[0].prev = p;
    w.ants[0].pos = Pos::new(p.x + 1000, p.y);
    let cam = camera_on(p, 13);
    let at = |interp: i32| {
        ant_shapes_drawn(
            &frame(&w, None, &cam, &Overlay::default(), interp),
            player(0),
        )[0][1]
    };
    let (a, b, c) = (at(0), at(500), at(1000));
    assert_eq!(b - a, cam.length(500));
    assert_eq!(c - a, cam.length(1000));
}

#[test]
fn terrain_is_deterministic_and_a_region_matches_the_whole() {
    let w = world(Format::OneVOne);
    let (iw, ih, img) = terrain_image(&w, 3);
    assert_eq!((iw, ih), (w.map.w * 3, w.map.h * 3));
    assert_eq!(img, terrain_image(&w, 3).2);
    let (rw, rh, part) = terrain_region(&w, 3, 10, 20, 40, 50);
    assert_eq!((rw, rh), (90, 90));
    for y in 0..rh {
        let a = (((20 * 3 + y) * iw + 10 * 3) * 4) as usize;
        let b = (y * rw * 4) as usize;
        let n = (rw * 4) as usize;
        assert_eq!(&img[a..a + n], &part[b..b + n], "row {y}");
    }
}

/// The mean colour of the centre pixels of cells whose whole 3×3
/// neighbourhood is `g`.
fn mean_colour(w: &World, img: &[u8], scale: i32, g: Ground) -> [i64; 3] {
    let mut sum = [0i64; 3];
    let mut n = 0;
    for y in 1..w.map.h - 1 {
        for x in 1..w.map.w - 1 {
            if (-1..=1).all(|dy| (-1..=1).all(|dx| w.map.at(x + dx, y + dy) == g)) {
                let row = y * scale + scale / 2;
                let i = ((row * w.map.w * scale + x * scale + scale / 2) * 4) as usize;
                for k in 0..3 {
                    sum[k] += img[i + k] as i64;
                }
                n += 1;
            }
        }
    }
    assert!(n > 0, "no {g:?} interior");
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

#[test]
fn rock_dirt_and_floor_look_different_in_every_biome() {
    // A map with a dirt plug at least three cells thick.
    let mut w = (1..40)
        .map(|seed| World::new(seed, MatchSettings::default(), &[SeatKind::Human; 2]))
        .find(|w| {
            (1..w.map.h - 1).any(|y| {
                (1..w.map.w - 1).any(|x| {
                    (-1..=1).all(|dy| (-1..=1).all(|dx| w.map.at(x + dx, y + dy) == Ground::Dirt))
                })
            })
        })
        .expect("some seed has dirt");
    for biome in Biome::ALL {
        w.settings.biome = biome;
        let (_, _, img) = terrain_image(&w, 4);
        let floor = mean_colour(&w, &img, 4, Ground::Floor);
        let rock = mean_colour(&w, &img, 4, Ground::Rock);
        let dirt = mean_colour(&w, &img, 4, Ground::Dirt);
        let far = |a: [i64; 3], b: [i64; 3]| (0..3).map(|k| (a[k] - b[k]).abs()).sum::<i64>() > 60;
        assert!(far(floor, rock), "{biome:?} floor {floor:?} rock {rock:?}");
        assert!(far(floor, dirt), "{biome:?} floor {floor:?} dirt {dirt:?}");
        assert!(far(rock, dirt), "{biome:?} rock {rock:?} dirt {dirt:?}");
    }
}

/// A palette colour as RGB.
fn rgb(colour: i32) -> [i64; 3] {
    let hex = PALETTE[colour as usize].trim_start_matches('#');
    let at = |k: usize| i64::from_str_radix(&hex[k..k + 2], 16).expect("hex colour");
    [at(0), at(2), at(4)]
}

#[test]
fn player_and_node_colours_stand_out_on_every_floor() {
    // Ants and nodes stand on floor: every player colour and every node
    // colour must be far from each biome's floor (TRIAGE row 44).
    let mut w = world(Format::OneVOne);
    let colours: Vec<(String, [i64; 3])> = PLAYER_RGB
        .iter()
        .enumerate()
        .map(|(i, c)| (format!("player {i}"), c.map(|v| v as i64)))
        .chain([
            ("Food".to_string(), rgb(C_CRUMB)),
            ("Nectar".to_string(), rgb(C_PETAL)),
            ("Leaves".to_string(), rgb(C_LEAF)),
        ])
        .collect();
    for biome in Biome::ALL {
        w.settings.biome = biome;
        let (_, _, img) = terrain_image(&w, 4);
        let floor = mean_colour(&w, &img, 4, Ground::Floor);
        for (name, c) in &colours {
            let d: i64 = (0..3).map(|k| (c[k] - floor[k]).abs()).sum();
            assert!(d >= 110, "{name} {c:?} on {biome:?} floor {floor:?}: {d}");
        }
    }
}

#[test]
fn props_lie_deep_in_rock_and_come_from_the_seed() {
    // TRIAGE rows 44 and 58: props only where nobody walks, the same for
    // the same seed, varied in kind, size and turn.
    let w = world(Format::OneVOne);
    for biome in Biome::ALL {
        let all = props::props(&w, biome);
        assert!(all.len() >= 20, "{biome:?} has {} props", all.len());
        assert_eq!(all, props::props(&w, biome), "{biome:?} is deterministic");
        let fine = CELL;
        for p in &all {
            // Every cell the prop can paint (shadow included) touches is
            // rock.
            let reach = p.r * 11 / 8 + fine / 8 + fine;
            for cy in (p.y - reach) / fine..=(p.y + reach) / fine {
                for cx in (p.x - reach) / fine..=(p.x + reach) / fine {
                    let (dx, dy) = (cx * fine + fine / 2 - p.x, cy * fine + fine / 2 - p.y);
                    if dx * dx + dy * dy <= reach * reach {
                        assert_eq!(w.map.at(cx, cy), Ground::Rock, "{biome:?} {p:?}");
                    }
                }
            }
        }
        let mut kinds: Vec<String> = all.iter().map(|p| format!("{:?}", p.kind)).collect();
        kinds.sort();
        kinds.dedup();
        assert!(kinds.len() >= 4, "{biome:?} kinds {kinds:?}");
        let sizes: std::collections::BTreeSet<i32> = all.iter().map(|p| p.r).collect();
        let turns: std::collections::BTreeSet<usize> = all.iter().map(|p| p.rot).collect();
        assert!(sizes.len() >= 4 && turns.len() >= 8, "{biome:?} varies");
    }
    let picnic = props::props(&w, Biome::Picnic);
    let mut food: Vec<String> = picnic.iter().map(|p| format!("{:?}", p.kind)).collect();
    food.sort();
    food.dedup();
    assert_eq!(food.len(), 8, "every picnic prop appears: {food:?}");
    let other = World::new(8, w.settings.clone(), &[SeatKind::Human; 2]);
    assert_ne!(picnic, props::props(&other, Biome::Picnic), "another seed");
}

#[test]
fn nodes_show_saturation_where_the_team_sees() {
    // TRIAGE row 42: "working/SATURATION" over each node the viewer's team
    // sees, counting every ant whose job is to gather there.
    let mut w = world(Format::OneVOne);
    w.step(&[], &[]);
    let q = w.structures[queen_of(&w, 0)].pos;
    let node = w
        .nodes
        .iter()
        .min_by_key(|n| n.pos.dist2(q))
        .expect("a node")
        .clone();
    for a in w.ants.iter_mut() {
        a.job = Job::Idle;
    }
    for a in w.ants.iter_mut().take(3) {
        a.job = Job::Gather(node.id);
    }
    let texts = |w: &World, viewer: Option<u8>, at: Pos, zoom: usize| -> Vec<(Pt, String)> {
        decode(&frame(
            w,
            viewer,
            &camera_on(at, zoom),
            &Overlay::default(),
            0,
        ))
        .into_iter()
        .filter(|(code, _)| *code == op::TEXT)
        .map(|(_, a)| {
            let s: String = a[8..]
                .iter()
                .map(|c| char::from_u32(*c as u32).unwrap())
                .collect();
            ((a[0], a[1]), s)
        })
        .collect()
    };
    let cam = camera_on(q, 10);
    let (nx, ny) = cam.screen(node.pos);
    let shown = texts(&w, Some(0), q, 10);
    let over = shown
        .iter()
        .find(|(p, _)| p.0 == nx && p.1 < ny)
        .unwrap_or_else(|| panic!("no Saturation over the home node: {shown:?}"));
    assert_eq!(over.1, format!("3/{SATURATION}"));
    for (_, s) in &shown {
        assert!(s.ends_with(&format!("/{SATURATION}")), "{s}");
    }
    // Hidden when zoomed far out.
    assert!(texts(&w, Some(0), q, 5).is_empty(), "zoomed out");
    // Not over the enemy's nodes, which colony 0 cannot see.
    let e = w.structures[queen_of(&w, 1)].pos;
    assert!(texts(&w, Some(0), e, 10).is_empty(), "enemy home unseen");
    assert!(!texts(&w, None, e, 10).is_empty(), "a spectator sees it");
}

#[test]
fn terrain_edges_are_smooth_not_tiled() {
    // Where floor meets rock, the edge often passes through the middle of a
    // cell (the two halves of the cell differ), so it does not follow the
    // cell grid.
    let w = world(Format::OneVOne);
    let scale = 6;
    let (_, _, img) = terrain_image(&w, scale);
    let pw = w.map.w * scale;
    let px = |x: i32, y: i32| {
        let i = ((y * pw + x) * 4) as usize;
        [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
    };
    let mut edges = 0;
    let mut split_cells = 0;
    for y in 1..w.map.h - 1 {
        for x in 1..w.map.w - 1 {
            if w.map.at(x, y) == Ground::Floor && w.map.at(x + 1, y) == Ground::Rock {
                edges += 1;
                let a = px(x * scale + 1, y * scale + scale / 2);
                let b = px(x * scale + scale - 1, y * scale + scale / 2);
                if (0..3).map(|k| (a[k] - b[k]).abs()).sum::<i32>() > 40 {
                    split_cells += 1;
                }
            }
        }
    }
    assert!(
        split_cells * 5 > edges,
        "only {split_cells} of {edges} edge cells hold part of the edge"
    );
}

#[test]
fn fog_has_three_states_and_allies_share_vision() {
    let mut w = world(Format::TwoVTwo);
    let alpha = |w: &World, viewer: u8, p: Pos| {
        let (x, y) = p.cell();
        fog_image(w, Some(viewer))[((y * w.map.w + x) * 4 + 3) as usize]
    };
    let ally_queen = w.structures[queen_of(&w, 1)].pos;
    let enemy_queen = w.structures[queen_of(&w, 2)].pos;
    assert_eq!(alpha(&w, 0, ally_queen), 0, "allies share vision");
    assert_eq!(alpha(&w, 0, enemy_queen), 255, "unexplored");
    let (ex, ey) = enemy_queen.cell();
    for dy in -3..=3 {
        for dx in -3..=3 {
            let i = w.map.idx(ex + dx, ey + dy);
            w.explored[0].set(i);
        }
    }
    assert_eq!(alpha(&w, 0, enemy_queen), 150, "explored but not visible");
    assert!(fog_image(&w, None).iter().all(|b| *b == 0));
}

#[test]
fn the_camera_maps_both_ways_and_zooms_about_the_pointer() {
    let mut cam = Camera::new(800, 600);
    cam.centre_x = 50 * CELL;
    cam.centre_y = 40 * CELL;
    cam.zoom = 9;
    let p = cam.world_at(123, 456);
    let (sx, sy) = cam.screen(p);
    assert!((sx - 123 * SUB_PX).abs() <= SUB_PX && (sy - 456 * SUB_PX).abs() <= SUB_PX);
    cam.zoom_at(2, 123, 456);
    let q = cam.world_at(123, 456);
    assert!(
        (q.x - p.x).abs() < CELL / 4 && (q.y - p.y).abs() < CELL / 4,
        "{p:?} {q:?}"
    );
    cam.pan(100, 0);
    assert!((cam.world_at(223, 456).x - q.x).abs() < CELL / 4);
    cam.fit(224, 150);
    assert!(224 * cam.units_per_cell() <= 800 * SUB_PX);
    assert!(224 * ZOOMS[cam.zoom + 1] > 800 * SUB_PX);
    cam.centre_x = -5000;
    cam.clamp(224, 150);
    assert_eq!(cam.centre_x, 0);
}

#[test]
fn minimap_shows_players_and_the_view() {
    let w = world(Format::OneVOne);
    let mut cam = Camera::new(800, 600);
    cam.fit(w.map.w, w.map.h);
    cam.zoom = 10;
    let img = minimap(&w, Some(0), &cam, 100);
    assert_eq!(img.len(), 100 * 100 * 4);
    let has = |c: [u8; 3]| {
        img.chunks(4)
            .any(|p| p[0] == c[0] && p[1] == c[1] && p[2] == c[2])
    };
    assert!(has([255, 255, 255]), "view rectangle");
    assert!(has(PLAYER_RGB[0]), "own colony");
    assert!(!has(PLAYER_RGB[1]), "the enemy is out of sight");
}

#[test]
fn assets_carry_exact_scales_and_every_op() {
    let a: serde_json::Value = serde_json::from_str(&assets_json()).expect("JSON");
    assert_eq!(a["base"].to_string(), "0.0625");
    assert_eq!(a["scales"].as_array().unwrap().len() as i32, SCALE_MAX + 1);
    assert_eq!(a["scales"][512].to_string(), "0.5");
    assert_eq!(a["scales"][3].to_string(), "0.0029296875");
    assert_eq!(a["scales"][2048].to_string(), "2");
    assert_eq!(a["palette"].as_array().unwrap().len(), PALETTE.len());
    for (name, code) in op::NAMES {
        assert_eq!(a["ops"][name], json!(code));
    }
}

/// Every point a SHAPE or FILL_SHAPE op covers, in draw-list units, widened
/// by the visible half of its outline.
fn shape_extent(all: &[Shape], code: i32, a: &[i32]) -> (i32, i32, i32, i32) {
    let (id, x, y, scale) = (a[0], a[1], a[2], a[3]);
    let half = if code == op::SHAPE {
        a[6] * scale / SCALE_ONE / 2
    } else {
        0
    };
    let b = bounds(&all[id as usize]);
    let at = |v: i32| v * scale / SCALE_ONE;
    (
        x + at(b.0) - half,
        y + at(b.1) - half,
        x + at(b.2) + half,
        y + at(b.3) + half,
    )
}

#[test]
fn every_species_has_its_own_role_mark_and_size() {
    for a in Species::ALL {
        assert!(!species_parts(a).mark.is_empty(), "{a:?} has a mark");
        for b in Species::ALL {
            if a != b {
                assert_ne!(species_parts(a).mark, species_parts(b).mark, "{a:?} {b:?}");
            }
        }
    }
    // Size tells workers from fighters: every Pop Cost 1 species is drawn
    // smaller than every stronger one, and the Carpenter Ant is clearly
    // bigger than the Harvester Ant it was mistaken for.
    for a in Species::ALL.iter().filter(|s| s.stats().pop == 1) {
        for b in Species::ALL.iter().filter(|s| s.stats().pop > 1) {
            assert!(drawn_radius(*a) < drawn_radius(*b), "{a:?} {b:?}");
        }
    }
    let (h, c) = (
        drawn_radius(Species::Harvester),
        drawn_radius(Species::Carpenter),
    );
    assert!(c * 100 >= h * 110, "Carpenter {c} vs Harvester {h}");
    // Marks are drawn in ink over the body from MARK_ZOOM, never below it.
    let mut w = world(Format::OneVOne);
    w.ants.truncate(1);
    let p = w.ants[0].pos;
    let marks = |zoom: usize| {
        decode(&frame(
            &w,
            None,
            &camera_on(p, zoom),
            &Overlay::default(),
            0,
        ))
        .into_iter()
        .filter(|(code, a)| {
            *code == op::FILL_SHAPE
                && a[0] >= mark_shape_id(Species::ALL[0], 0)
                && a[0] < ANT_SHAPES
        })
        .map(|(_, a)| a[4])
        .collect::<Vec<i32>>()
    };
    assert_eq!(marks(12), vec![C_INK], "12 pixels per cell shows the mark");
    assert_eq!(marks(10), vec![C_INK], "8 pixels per cell shows the mark");
    assert!(marks(5).is_empty(), "too small for a mark");
}

#[test]
fn icons_fit_inside_their_image() {
    let all = shapes();
    let mut keys: Vec<String> = Species::ALL
        .iter()
        .map(|s| format!("species.{}", s.key()))
        .collect();
    keys.extend(
        crate::rules::Upgrade::all()
            .iter()
            .map(|u| format!("upgrade.{}", u.key())),
    );
    keys.extend(
        StructureKind::all()
            .iter()
            .map(|k| format!("structure.{}", k.key())),
    );
    for k in [
        "action.all_ants",
        "action.evolve",
        "action.retreat",
        "focus.balanced",
        "focus.food",
        "focus.nectar",
        "focus.leaves",
    ] {
        keys.push(k.into());
    }
    let size = 48;
    let full = size * SUB_PX;
    for k in keys {
        let list = icon_list(&k, 0, size).unwrap();
        let mut seen = (full, full, 0, 0);
        for (code, a) in decode(&list) {
            let b = match code {
                op::SHAPE | op::FILL_SHAPE => shape_extent(&all, code, &a),
                op::POLY | op::OUTLINE => {
                    let skip = if code == op::POLY { 2 } else { 3 };
                    let pts: Vec<Pt> = a[skip..].chunks(2).map(|c| (c[0], c[1])).collect();
                    bounds(&vec![pts])
                }
                _ => continue,
            };
            assert!(
                b.0 >= 0 && b.1 >= 0 && b.2 <= full && b.3 <= full,
                "{k} pokes out of its {size} px image: {b:?} of {full}"
            );
            seen = (
                seen.0.min(b.0),
                seen.1.min(b.1),
                seen.2.max(b.2),
                seen.3.max(b.3),
            );
        }
        let span = (seen.2 - seen.0).max(seen.3 - seen.1);
        assert!(
            span >= full * 7 / 10,
            "{k} fills its image: {span} of {full}"
        );
    }
}

#[test]
fn upgrade_icons_differ_from_each_other_and_from_species() {
    // TRIAGE row 53: no two upgrades look alike, no upgrade looks like a
    // species, and every upgrade wears the upgrade disc.
    let upgrades: Vec<(String, Vec<i32>)> = crate::rules::Upgrade::all()
        .iter()
        .map(|u| {
            let k = format!("upgrade.{}", u.key());
            let l = icon_list(&k, 1, 48).unwrap();
            (k, l)
        })
        .collect();
    let species: Vec<Vec<i32>> = Species::ALL
        .iter()
        .map(|s| icon_list(&format!("species.{}", s.key()), 1, 48).unwrap())
        .collect();
    for (i, (k, l)) in upgrades.iter().enumerate() {
        for (k2, l2) in &upgrades[i + 1..] {
            assert_ne!(l, l2, "{k} looks like {k2}");
        }
        assert!(!species.contains(l), "{k} looks like a species");
        let first = &decode(l)[0];
        assert_eq!((first.0, first.1[0]), (op::POLY, C_BASE), "{k} disc");
    }
    // The stat upgrades carry a glyph, not an ant.
    for k in ["damage", "armor", "health", "build_rate"] {
        let l = icon_list(&format!("upgrade.{k}"), 1, 48).unwrap();
        assert!(
            ant_shapes_drawn(&l, player(1)).is_empty(),
            "upgrade.{k} draws an ant"
        );
    }
}

#[test]
fn an_upgrade_icon_can_print_its_rank() {
    let text = |k: &str| -> Vec<String> {
        decode(&icon_list(k, 0, 48).unwrap())
            .into_iter()
            .filter(|(code, _)| *code == op::TEXT)
            .map(|(_, a)| {
                a[8..]
                    .iter()
                    .map(|c| char::from_u32(*c as u32).unwrap())
                    .collect()
            })
            .collect()
    };
    assert!(text("upgrade.armor").is_empty());
    assert_eq!(text("upgrade.armor@1"), vec!["I"]);
    assert_eq!(text("upgrade.armor@3"), vec!["III"]);
    assert_eq!(text("upgrade.species.fire@2"), vec!["II"]);
    assert!(icon_list("upgrade.armor@4", 0, 48).is_none());
    assert!(icon_list("upgrade.armor@x", 0, 48).is_none());
}

#[test]
fn gather_focus_icons_wear_their_resources_colours() {
    let fills = |k: &str| -> Vec<i32> {
        decode(&icon_list(k, 1, 48).unwrap())
            .into_iter()
            .filter(|(code, _)| *code == op::SHAPE)
            .map(|(_, a)| a[4])
            .collect()
    };
    assert_eq!(fills("focus.food"), vec![C_CRUMB]);
    assert_eq!(fills("focus.nectar"), vec![C_PETAL, C_POLLEN]);
    assert_eq!(fills("focus.leaves"), vec![C_LEAF]);
    assert_eq!(
        fills("focus.balanced"),
        vec![C_CRUMB, C_PETAL, C_POLLEN, C_LEAF]
    );
    assert_ne!(
        icon_list("action.evolve", 0, 48),
        icon_list("action.retreat", 0, 48),
        "Evolve and Retreat have different pictures"
    );
}

#[test]
fn a_waypoint_prints_its_species_on_the_flag() {
    let mut w = world(Format::OneVOne);
    let q = w.structures[queen_of(&w, 0)].pos;
    let (qx, qy) = q.cell();
    let (x, y) = (qx + 3, qy + 3);
    for species in [Species::Soldier, Species::Acid, Species::Dinosaur] {
        w.apply(
            0,
            &Command::SetWaypoint {
                species: Some(species),
                x,
                y,
                add: false,
            },
        );
    }
    let wp = w.colonies[0].waypoints[Species::Soldier.index()][0];
    let cam = camera_on(q, 12);
    let overlay = Overlay {
        hover_waypoint: Some(wp),
        ..Overlay::default()
    };
    w.ants.clear();
    let ops = decode(&frame(&w, Some(0), &cam, &overlay, 0));
    let cloth = ops
        .iter()
        .find(|(code, a)| *code == op::POLY && a[0] == player(0) && a[1] == 5)
        .map(|(_, a)| {
            let pts: Vec<Pt> = a[2..].chunks(2).map(|c| (c[0], c[1])).collect();
            bounds(&vec![pts])
        })
        .expect("a pennant in the player colour");
    let all = shapes();
    let icons: Vec<_> = ops
        .iter()
        .filter(|(code, a)| (*code == op::SHAPE || *code == op::FILL_SHAPE) && a[0] < ANT_SHAPES)
        .collect();
    assert_eq!(icons.len(), 3, "one icon per species");
    for (code, a) in icons {
        assert_eq!(
            a[4], C_INK,
            "printed in ink, not an ant in the player colour"
        );
        let b = shape_extent(&all, *code, a);
        assert!(
            b.0 >= cloth.0 && b.1 >= cloth.1 && b.2 <= cloth.2 && b.3 <= cloth.3,
            "the icon {b:?} lies on the cloth {cloth:?}"
        );
    }
    assert!(
        ops.iter()
            .any(|(code, a)| *code == op::OUTLINE && a[0] == C_HOVER),
        "the hovered waypoint is highlighted"
    );
}

/// The draw list for one effect alone, `phase_ticks` after it began.
fn effect_ops(kind: EffectKind, age: i32, interp: i32) -> Vec<(i32, Vec<i32>)> {
    let mut w = world(Format::OneVOne);
    w.ants.clear();
    let q = w.structures[queen_of(&w, 0)].pos;
    let to = Pos::new(q.x + 4 * CELL, q.y);
    w.effects.push(crate::world::Effect {
        kind,
        from: Pos::new(to.x - 3 * CELL, to.y),
        to,
        until: w.tick + effect_life(kind) - age,
        colony: 0,
    });
    decode(&frame(
        &w,
        None,
        &camera_on(to, 12),
        &Overlay::default(),
        interp,
    ))
}

#[test]
fn each_hit_kind_has_its_own_look() {
    let has = |ops: &[(i32, Vec<i32>)], code: i32, colour: i32| {
        ops.iter().any(|(c, a)| *c == code && a[0] == colour)
    };
    // A Bite: jaws in the attacker's colour and a white flash on the target.
    let bite = effect_ops(EffectKind::Bite, 0, 200);
    assert!(has(&bite, op::LINE, player(0)) && has(&bite, op::POLY, C_BITE));
    // A Spray: a green droplet in flight, then a splat.
    let fly = effect_ops(EffectKind::Spray, 0, 500);
    let splat = effect_ops(EffectKind::Spray, 1, 800);
    assert!(has(&fly, op::POLY, C_ACID) && has(&fly, op::LINE, C_ACID));
    assert!(has(&splat, op::POLY, C_ACID) && !has(&splat, op::LINE, C_ACID));
    // A Blast: a ring that grows to the Splash radius.
    let ring = |age: i32, interp: i32| -> i32 {
        let ops = effect_ops(EffectKind::Blast, age, interp);
        let (_, a) = ops
            .iter()
            .find(|(c, a)| *c == op::OUTLINE && a[0] == C_BLAST_RING)
            .expect("a blast ring");
        let xs: Vec<i32> = a[3..].iter().step_by(2).copied().collect();
        (xs.iter().max().unwrap() - xs.iter().min().unwrap()) / 2
    };
    let cam = camera_on(Pos::new(0, 0), 12);
    let splash = cam.length(Species::Exploding.stats().splash);
    let (early, late) = (ring(0, 0), ring(3, 999));
    assert!(early < splash / 2, "starts small: {early} of {splash}");
    assert!(
        (splash * 9 / 10..=splash).contains(&late),
        "ends at the Splash radius: {late} of {splash}"
    );
    // A Heal: a green plus that rises.
    let plus_top = |age: i32| -> i32 {
        let ops = effect_ops(EffectKind::Heal, age, 0);
        let (_, a) = ops
            .iter()
            .find(|(c, a)| *c == op::POLY && a[0] == C_HEAL)
            .expect("a green plus");
        a[3..].iter().step_by(2).copied().min().unwrap()
    };
    assert!(plus_top(2) < plus_top(0), "the plus rises");
}
