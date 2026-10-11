//! Fog of War with Line of Sight. Each source (an ant or structure) casts
//! rays to the edge of its vision; a ray stops at the first rock, dirt or
//! wall cell, which is itself seen. Corners therefore hide what is behind
//! them, as the brief asks.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A packed set of cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Bits {
    words: Vec<u64>,
}

impl Bits {
    pub fn new(n: usize) -> Bits {
        Bits {
            words: vec![0; n.div_ceil(64)],
        }
    }
    pub fn get(&self, i: usize) -> bool {
        self.words
            .get(i / 64)
            .map(|w| w >> (i % 64) & 1 == 1)
            .unwrap_or(false)
    }
    pub fn set(&mut self, i: usize) {
        if let Some(w) = self.words.get_mut(i / 64) {
            *w |= 1 << (i % 64);
        }
    }
    /// Sets the first `n` cells (the whole map).
    pub fn set_all(&mut self, n: usize) {
        for (k, w) in self.words.iter_mut().enumerate() {
            let lo = k * 64;
            *w = if lo + 64 <= n {
                u64::MAX
            } else if lo < n {
                (1u64 << (n - lo)) - 1
            } else {
                0
            };
        }
    }
    pub fn clear_all(&mut self) {
        for w in self.words.iter_mut() {
            *w = 0;
        }
    }
    pub fn union_with(&mut self, o: &Bits) {
        for (a, b) in self.words.iter_mut().zip(o.words.iter()) {
            *a |= *b;
        }
    }
    pub fn count(&self) -> u32 {
        self.words.iter().map(|w| w.count_ones()).sum()
    }
}

/// Ray cells for every vision radius, built once.
pub struct Rays {
    by_radius: Vec<Vec<Vec<(i32, i32)>>>,
}

pub const MAX_VISION: i32 = 16;

impl Default for Rays {
    fn default() -> Self {
        Rays::new()
    }
}

impl Rays {
    pub fn new() -> Rays {
        let mut by_radius = Vec::new();
        for r in 0..=MAX_VISION {
            let mut rays = Vec::new();
            let mut ends = BTreeSet::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    let d2 = dx * dx + dy * dy;
                    let inside = d2 <= r * r + r;
                    let rim = d2 > (r - 1) * (r - 1) + (r - 1);
                    if inside && (rim || r == 0) {
                        ends.insert((dx, dy));
                    }
                }
            }
            for (ex, ey) in ends {
                rays.push(line(0, 0, ex, ey));
            }
            by_radius.push(rays);
        }
        Rays { by_radius }
    }

    /// Marks every cell a source at (x, y) with radius `r` can see.
    pub fn cast(
        &self,
        w: i32,
        h: i32,
        x: i32,
        y: i32,
        r: i32,
        opaque: &dyn Fn(usize) -> bool,
        out: &mut Bits,
    ) {
        let r = r.clamp(0, MAX_VISION) as usize;
        for ray in &self.by_radius[r] {
            for &(dx, dy) in ray {
                let (cx, cy) = (x + dx, y + dy);
                if cx < 0 || cy < 0 || cx >= w || cy >= h {
                    break;
                }
                let i = (cy * w + cx) as usize;
                out.set(i);
                if (dx != 0 || dy != 0) && opaque(i) {
                    break;
                }
            }
        }
    }
}

/// Bresenham cells from (x0, y0) to (x1, y1), both included.
pub fn line(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        out.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> (i32, i32, Vec<bool>) {
        let h = rows.len() as i32;
        let w = rows[0].len() as i32;
        let opaque = rows
            .iter()
            .flat_map(|r| r.chars().map(|c| c == '#'))
            .collect();
        (w, h, opaque)
    }

    #[test]
    fn vision_does_not_turn_corners() {
        // The source at S looks along the corridor; the cell X round the
        // corner is hidden by rock.
        let (w, h, op) = grid(&[
            "S.....", //
            "#####.", "X.....",
        ]);
        let rays = Rays::new();
        let mut seen = Bits::new((w * h) as usize);
        rays.cast(w, h, 0, 0, 8, &|i| op[i], &mut seen);
        assert!(seen.get(5)); // end of the corridor
        assert!(seen.get(6)); // the rock itself is seen
        assert!(!seen.get(12)); // X, round the corner
    }

    #[test]
    fn an_open_disk_is_fully_visible() {
        let (w, h, op) = grid(&[".........."; 10]);
        let rays = Rays::new();
        let mut seen = Bits::new((w * h) as usize);
        rays.cast(w, h, 5, 5, 3, &|i| op[i], &mut seen);
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                if dx * dx + dy * dy <= 9 {
                    let i = ((5 + dy) * w + 5 + dx) as usize;
                    assert!(seen.get(i), "{dx},{dy}");
                }
            }
        }
    }

    #[test]
    fn bits_set_get_and_union() {
        let mut a = Bits::new(130);
        let mut b = Bits::new(130);
        a.set(3);
        b.set(129);
        a.union_with(&b);
        assert!(a.get(3) && a.get(129) && !a.get(4));
        assert_eq!(a.count(), 2);
    }
}
