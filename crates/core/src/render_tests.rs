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
        let (fixed, points) =
            op::layout(code).unwrap_or_else(|| panic!("unknown op {code} at {i}"));
        let mut n = fixed;
        if points {
            n += 2 * list[i + fixed] as usize;
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

#[test]
fn a_blueprint_shows_its_progress_and_a_wall_its_colour() {
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
    let soft_area = |w: &World| -> i64 {
        decode(&frame(w, Some(0), &cam, &Overlay::default(), 0))
            .into_iter()
            .filter(|(code, a)| *code == op::POLY && a[0] == soft(0))
            .map(|(_, a)| area(&a[2..]))
            .sum()
    };
    assert_eq!(soft_area(&w), 0, "nothing delivered yet");
    let si = w.structure_index(id).unwrap();
    w.structures[si].delivered = w.structures[si].cost;
    let half = soft_area(&w);
    w.structures[si].work_done = w.structures[si].work;
    let full = soft_area(&w);
    assert!(half > 0 && full > half * 3 / 2, "{half} {full}");
    // The wall is a player-colour square, distinct from rock.
    let walls = decode(&frame(&w, Some(0), &cam, &Overlay::default(), 0))
        .into_iter()
        .filter(|(code, a)| *code == op::POLY && a[0] == player(0) && a[1] == 4)
        .count();
    assert_eq!(walls, 1);
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
fn rock_dirt_and_floor_look_different_in_both_biomes() {
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
    for biome in [Biome::Picnic, Biome::FlowerField] {
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
