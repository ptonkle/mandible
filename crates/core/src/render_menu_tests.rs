use super::*;
use crate::render::shapes;

/// Every menu screen key the view model has, plus Match History (being
/// added alongside).
const SCREENS: [&str; 7] = [
    "main_menu",
    "play_vs_ai",
    "match_settings",
    "online",
    "lobby",
    "player_settings",
    "match_history",
];

/// Walks a draw list op by op: (code, arguments, list items).
fn ops(list: &[i32]) -> Vec<(i32, Vec<i32>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < list.len() {
        let code = list[at];
        let (fixed, per) = op::layout(code).expect("known op code");
        let mut len = fixed;
        if per > 0 {
            len += per * list[at + fixed] as usize;
        }
        out.push((code, list[at + 1..at + 1 + len].to_vec()));
        at += 1 + len;
    }
    assert_eq!(at, list.len(), "the list ends on an op boundary");
    out
}

#[test]
fn each_screen_gets_a_distinct_background() {
    let mut seen: Vec<(Vec<u8>, Vec<i32>)> = Vec::new();
    for screen in SCREENS {
        let bg = MenuBackground::new(screen);
        let list = bg.draw(1280, 720);
        let ants = ops(&list)
            .iter()
            .filter(|(c, _)| *c == op::FILL_SHAPE)
            .count();
        assert!(bg.width() > 0 && bg.height() > 0, "{screen}: an image");
        assert_eq!(bg.image().len(), (bg.width() * bg.height() * 4) as usize);
        assert!(ants >= 10, "{screen}: ant silhouettes, found {ants}");
        let pair = (bg.image(), list);
        assert!(
            seen.iter().all(|p| p.0 != pair.0 && p.1 != pair.1),
            "{screen}: the same background as another screen"
        );
        seen.push(pair);
    }
}

#[test]
fn a_background_is_static() {
    for screen in ["main_menu", "lobby", "a_screen_added_later"] {
        let (a, b) = (MenuBackground::new(screen), MenuBackground::new(screen));
        assert_eq!(a.image(), b.image(), "{screen}: same image twice");
        assert_eq!(
            a.draw(1280, 720),
            b.draw(1280, 720),
            "{screen}: same list twice"
        );
        assert_eq!(
            a.draw(1280, 720),
            a.draw(1280, 720),
            "{screen}: drawing changes nothing"
        );
    }
}

#[test]
fn the_list_covers_any_view_with_valid_ops() {
    let bg = MenuBackground::new("main_menu");
    let shape_count = shapes().len() as i32;
    let colours = palette().len() as i32;
    for (w, h) in [(1280, 720), (1920, 1080), (800, 1200), (360, 640), (1, 1)] {
        let list = bg.draw(w, h);
        let all = ops(&list);
        let (code, args) = &all[1];
        assert_eq!(*code, op::IMAGE, "the ground comes first after clearing");
        let [sx, sy, sw, sh] = [args[1], args[2], args[3], args[4]];
        assert!(sx >= 0 && sy >= 0 && sx + sw <= bg.width() && sy + sh <= bg.height());
        assert_eq!(
            &args[5..],
            &[0, 0, w * SUB_PX, h * SUB_PX],
            "{w}x{h}: covers the view"
        );
        // The image keeps its proportions (within a pixel of rounding).
        let skew = (sw as i64 * h as i64 - sh as i64 * w as i64).abs();
        assert!(skew <= (w.max(h)) as i64, "{w}x{h}: stretched");
        for (code, args) in &all {
            match *code {
                op::CLEAR | op::POLY => assert!(args[0] < colours),
                op::FILL_SHAPE => {
                    assert!(args[0] < shape_count);
                    assert!(args[3] >= 1 && args[3] <= SCALE_MAX);
                    assert!(args[4] < colours);
                }
                op::IMAGE => {}
                c => panic!("unexpected op {c}"),
            }
        }
    }
}

/// Readability: through the dark layer, no ground pixel is brighter than
/// MAX_LUMA and the ground averages no more than MEAN_LUMA, so text on and
/// between the panels keeps its contrast.
#[test]
fn the_background_is_dim() {
    for screen in SCREENS {
        let bg = MenuBackground::new(screen);
        let img = bg.image();
        let dim = bg.dim_percent();
        let (mut brightest, mut total) = (0, 0i64);
        for px in img.chunks(4) {
            let mix = |c: u8, d: u8| (c as i32 * (100 - dim) + d as i32 * dim) / 100;
            let (r, g, b) = (
                mix(px[0], DIM_RGB[0]),
                mix(px[1], DIM_RGB[1]),
                mix(px[2], DIM_RGB[2]),
            );
            let luma = (r * 299 + g * 587 + b * 114) / 1000;
            brightest = brightest.max(luma);
            total += luma as i64;
        }
        let mean = (total / (img.len() as i64 / 4)) as i32;
        assert!(
            mean <= MEAN_LUMA,
            "{screen}: mean luma {mean} > {MEAN_LUMA}"
        );
        assert!(
            brightest <= MAX_LUMA,
            "{screen}: brightest pixel luma {brightest} > {MAX_LUMA}"
        );
    }
}

#[test]
fn the_palette_is_css_colours() {
    let p = palette();
    assert_eq!(p[C_DIM as usize], "rgba(14,10,7,0.72)");
    assert!(p.iter().all(|c| c.starts_with("rgb")));
    assert_eq!(palette_json(), serde_json::to_string(&p).unwrap());
}
