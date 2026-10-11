//! Integer geometry. The world has no floats (BUILDER-SPEC D2), so positions
//! are fixed-point: one cell is `CELL` sub-units, and angles are steps of a
//! 64-step circle with sine and cosine scaled by `TRIG_ONE`.

use serde::{Deserialize, Serialize};

/// Sub-units per cell. A cell is about one ant wide.
pub const CELL: i32 = 256;

/// Steps in a full circle for `sin64` and `cos64`.
pub const STEPS: usize = 64;

/// The value `sin64` returns for a quarter turn.
pub const TRIG_ONE: i32 = 1024;

const SIN: [i32; STEPS] = [
    0, 100, 200, 297, 392, 483, 569, 650, 724, 792, 851, 903, 946, 980, 1004, 1019, 1024, 1019,
    1004, 980, 946, 903, 851, 792, 724, 650, 569, 483, 392, 297, 200, 100, 0, -100, -200, -297,
    -392, -483, -569, -650, -724, -792, -851, -903, -946, -980, -1004, -1019, -1024, -1019, -1004,
    -980, -946, -903, -851, -792, -724, -650, -569, -483, -392, -297, -200, -100,
];

pub fn sin64(step: usize) -> i32 {
    SIN[step % STEPS]
}

pub fn cos64(step: usize) -> i32 {
    SIN[(step + STEPS / 4) % STEPS]
}

/// A point in sub-units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32) -> Pos {
        Pos { x, y }
    }

    /// The centre of a cell.
    pub fn of_cell(cx: i32, cy: i32) -> Pos {
        Pos::new(cx * CELL + CELL / 2, cy * CELL + CELL / 2)
    }

    pub fn cell(self) -> (i32, i32) {
        (self.x.div_euclid(CELL), self.y.div_euclid(CELL))
    }

    pub fn dist2(self, o: Pos) -> i64 {
        let dx = (self.x - o.x) as i64;
        let dy = (self.y - o.y) as i64;
        dx * dx + dy * dy
    }

    pub fn dist(self, o: Pos) -> i32 {
        isqrt(self.dist2(o) as u64) as i32
    }

    /// Moves towards `to` by at most `step` sub-units.
    pub fn toward(self, to: Pos, step: i32) -> Pos {
        let d = self.dist(to);
        if d <= step || d == 0 {
            return to;
        }
        Pos::new(
            self.x + ((to.x - self.x) as i64 * step as i64 / d as i64) as i32,
            self.y + ((to.y - self.y) as i64 * step as i64 / d as i64) as i32,
        )
    }
}

/// Integer square root, rounded down.
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// The nearest of 16 headings (0 = east, counting clockwise in screen space,
/// where y grows downwards) for a direction vector. Returns `None` for zero.
pub fn heading16(dx: i32, dy: i32) -> Option<u8> {
    if dx == 0 && dy == 0 {
        return None;
    }
    let mut best = 0usize;
    let mut best_dot = i64::MIN;
    for h in 0..16 {
        let step = h * 4;
        let dot = dx as i64 * cos64(step) as i64 + dy as i64 * sin64(step) as i64;
        if dot > best_dot {
            best_dot = dot;
            best = h;
        }
    }
    Some(best as u8)
}

/// Percentage of a value, rounded down.
pub fn pct(value: i32, percent: i32) -> i32 {
    (value as i64 * percent as i64 / 100) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_is_exact_on_squares_and_floors_between() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(1 << 40), 1 << 20);
        assert_eq!(isqrt(u64::MAX), 4294967295);
    }

    #[test]
    fn headings_point_the_right_way() {
        assert_eq!(heading16(10, 0), Some(0));
        assert_eq!(heading16(0, 10), Some(4));
        assert_eq!(heading16(-10, 0), Some(8));
        assert_eq!(heading16(0, -10), Some(12));
        assert_eq!(heading16(0, 0), None);
    }

    #[test]
    fn toward_never_overshoots() {
        let a = Pos::new(0, 0);
        let b = Pos::new(300, 400);
        assert_eq!(a.toward(b, 100), Pos::new(60, 80));
        assert_eq!(a.toward(b, 1000), b);
    }
}
