//! Line height index: a Fenwick (binary indexed) tree over per-line heights.
//!
//! Answers "y of line n" (prefix sum) and "line at y" (search) in O(log n), and updates one
//! line's height in O(log n). With uniform heights (QE1, no wrap) every line has the same height;
//! measured heights per line (soft wrap) are ED2/QE4 and use [`HeightIndex::set`].
//!
//! Nothing here depends on egui; it can move to `quire-core` if another consumer appears.

/// Fenwick tree of `f32` line heights.
#[derive(Clone, Debug, Default)]
pub struct HeightIndex {
    /// 1-based Fenwick array; `tree[0]` is unused.
    tree: Vec<f32>,
    /// The plain heights, for `get` and rebuilds.
    heights: Vec<f32>,
}

impl HeightIndex {
    /// `lines` lines, each `height` tall.
    pub fn uniform(lines: usize, height: f32) -> Self {
        let mut s = Self {
            tree: Vec::new(),
            heights: vec![height; lines],
        };
        s.rebuild();
        s
    }

    /// Rebuild the tree from `heights` in O(n).
    fn rebuild(&mut self) {
        let n = self.heights.len();
        self.tree.clear();
        self.tree.resize(n + 1, 0.0);
        for i in 1..=n {
            self.tree[i] += self.heights[i - 1];
            let j = i + (i & i.wrapping_neg());
            if j <= n {
                let v = self.tree[i];
                self.tree[j] += v;
            }
        }
    }

    /// Number of lines.
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    /// True when there are no lines.
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// Height of line `line`.
    pub fn get(&self, line: usize) -> f32 {
        self.heights[line]
    }

    /// Set the height of `line`.
    pub fn set(&mut self, line: usize, height: f32) {
        let delta = height - self.heights[line];
        if delta == 0.0 {
            return;
        }
        self.heights[line] = height;
        let mut i = line + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i & i.wrapping_neg();
        }
    }

    /// Resize to `lines` lines; new lines get `height`.
    pub fn resize(&mut self, lines: usize, height: f32) {
        if lines != self.heights.len() {
            self.heights.resize(lines, height);
            self.rebuild();
        }
    }

    /// Top y of `line` (sum of the heights of the lines before it). `line == len()` gives the total.
    pub fn y_of(&self, line: usize) -> f32 {
        let mut i = line.min(self.heights.len());
        let mut sum = 0.0;
        while i > 0 {
            sum += self.tree[i];
            i &= i - 1;
        }
        sum
    }

    /// Total height.
    pub fn total(&self) -> f32 {
        self.y_of(self.heights.len())
    }

    /// The line containing `y` (clamped to `0..len()`; 0 when empty).
    pub fn line_at(&self, y: f32) -> usize {
        let n = self.heights.len();
        if n == 0 || y <= 0.0 {
            return 0;
        }
        // Largest `pos` with prefix(pos) <= y.
        let mut pos = 0usize;
        let mut rem = y;
        let mut step = n.next_power_of_two();
        while step > 0 {
            let next = pos + step;
            if next <= n && self.tree[next] <= rem {
                pos = next;
                rem -= self.tree[next];
            }
            step >>= 1;
        }
        pos.min(n - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_prefix_and_search() {
        let h = HeightIndex::uniform(1000, 20.0);
        assert_eq!(h.y_of(0), 0.0);
        assert_eq!(h.y_of(10), 200.0);
        assert_eq!(h.total(), 20_000.0);
        assert_eq!(h.line_at(0.0), 0);
        assert_eq!(h.line_at(19.9), 0);
        assert_eq!(h.line_at(20.0), 1);
        assert_eq!(h.line_at(1e9), 999);
    }

    #[test]
    fn set_and_resize() {
        let mut h = HeightIndex::uniform(5, 10.0);
        h.set(2, 30.0);
        assert_eq!(h.y_of(3), 50.0);
        assert_eq!(h.line_at(25.0), 2);
        assert_eq!(h.line_at(49.0), 2);
        assert_eq!(h.line_at(50.0), 3);
        h.resize(7, 10.0);
        assert_eq!(h.total(), 90.0);
        assert_eq!(h.get(2), 30.0);
        assert!(HeightIndex::uniform(0, 1.0).is_empty());
    }
}
