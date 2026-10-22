//! Line height index: a Fenwick (binary indexed) tree over per-line heights.
//!
//! Answers "y of line n" (prefix sum) and "line at y" (search) in O(log n), and updates one
//! line's height in O(log n). With uniform heights (QE1, no wrap) every line has the same height;
//! measured heights per line (soft wrap) are ED2/QE4 and use [`HeightIndex::set`].
//!
//! Nothing here depends on egui; it can move to `quire-core` if another consumer appears.

/// Fenwick tree of `f32` line heights.
///
/// While every line has the same height (no soft wrap yet) the index stores only that height and
/// the line count: building it is O(1), so a line-count change while typing in a large file does
/// not rebuild an O(n) tree each keystroke (E1). The tree is materialised on the first `set` of a
/// different height.
#[derive(Clone, Debug, Default)]
pub struct HeightIndex {
    /// 1-based Fenwick array; `tree[0]` is unused. Empty while `uniform`.
    tree: Vec<f32>,
    /// The plain heights, for `get` and rebuilds. Empty while `uniform`.
    heights: Vec<f32>,
    /// Every line has this height.
    uniform: Option<f32>,
    len: usize,
}

impl HeightIndex {
    /// `lines` lines, each `height` tall.
    pub fn uniform(lines: usize, height: f32) -> Self {
        Self {
            tree: Vec::new(),
            heights: Vec::new(),
            uniform: Some(height),
            len: lines,
        }
    }

    /// Switch from the uniform representation to the tree.
    fn materialize(&mut self) {
        if let Some(h) = self.uniform.take() {
            self.heights = vec![h; self.len];
            self.rebuild();
        }
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
        self.len
    }

    /// True when there are no lines.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Height of line `line`.
    pub fn get(&self, line: usize) -> f32 {
        match self.uniform {
            Some(h) => {
                assert!(line < self.len, "line {line} out of range {}", self.len);
                h
            }
            None => self.heights[line],
        }
    }

    /// Set the height of `line`.
    pub fn set(&mut self, line: usize, height: f32) {
        if self.uniform == Some(height) {
            assert!(line < self.len, "line {line} out of range {}", self.len);
            return;
        }
        self.materialize();
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
        if lines == self.len {
            return;
        }
        if self.uniform == Some(height) || (self.len == 0 && self.uniform.is_some()) {
            self.uniform = Some(height);
            self.len = lines;
            return;
        }
        self.materialize();
        self.heights.resize(lines, height);
        self.len = lines;
        self.rebuild();
    }

    /// Top y of `line` (sum of the heights of the lines before it). `line == len()` gives the total.
    pub fn y_of(&self, line: usize) -> f32 {
        let line = line.min(self.len);
        if let Some(h) = self.uniform {
            return line as f32 * h;
        }
        let mut i = line;
        let mut sum = 0.0;
        while i > 0 {
            sum += self.tree[i];
            i &= i - 1;
        }
        sum
    }

    /// Total height.
    pub fn total(&self) -> f32 {
        self.y_of(self.len)
    }

    /// The line containing `y` (clamped to `0..len()`; 0 when empty).
    pub fn line_at(&self, y: f32) -> usize {
        let n = self.len;
        if n == 0 || y <= 0.0 {
            return 0;
        }
        if let Some(h) = self.uniform {
            if h <= 0.0 {
                return n - 1;
            }
            // Division can round either way; settle on the largest line whose top is <= y.
            let mut l = ((y / h) as usize).min(n - 1);
            while l > 0 && self.y_of(l) > y {
                l -= 1;
            }
            while l + 1 < n && self.y_of(l + 1) <= y {
                l += 1;
            }
            return l;
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

    #[test]
    fn uniform_agrees_with_the_tree() {
        for h in [17.0f32, 17.3, 0.1] {
            let u = HeightIndex::uniform(1000, h);
            let mut t = HeightIndex::uniform(1000, h);
            t.set(0, h + 1.0);
            t.set(0, h); // materialised, same heights
            assert!(u.uniform.is_some() && t.uniform.is_none());
            for line in [0, 1, 10, 999, 1000, 5000] {
                assert!(
                    (u.y_of(line) - t.y_of(line)).abs() <= h * 1e-3,
                    "{h} {line}"
                );
            }
            for i in 0..3000 {
                let y = i as f32 * h * 0.37;
                let l = u.line_at(y);
                assert!(u.y_of(l) <= y || l == 0);
                assert!(l + 1 >= u.len() || u.y_of(l + 1) > y);
            }
        }
    }
}
