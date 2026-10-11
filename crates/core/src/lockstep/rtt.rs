//! Round-trip samples to one peer.

use std::collections::VecDeque;

/// The last [`RttWindow::SIZE`] round-trip samples to one peer, in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RttWindow {
    samples: VecDeque<i64>,
}

impl RttWindow {
    /// How many samples are kept.
    pub const SIZE: usize = 10;

    /// Adds a sample, forgetting the oldest beyond [`RttWindow::SIZE`].
    pub fn push(&mut self, rtt_ms: i64) {
        self.samples.push_back(rtt_ms.max(0));
        while self.samples.len() > Self::SIZE {
            self.samples.pop_front();
        }
    }

    /// The 90th percentile (nearest rank), or `None` with no samples.
    pub fn p90(&self) -> Option<i64> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<i64> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let rank = (sorted.len() * 9).div_ceil(10);
        Some(sorted[rank.max(1) - 1])
    }

    /// How many samples are held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// True when no sample is held.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}
