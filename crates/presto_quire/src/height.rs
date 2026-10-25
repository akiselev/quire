//! Line height index: per-line heights in chunks, with Fenwick (binary indexed) trees over the
//! chunks and inside each chunk.
//!
//! Answers "y of line n" (prefix sum) and "line at y" (search) in O(log n), updates one line's
//! height in O(log n), and splices lines in or out in O(edit + chunk + n / chunk) (the chunk
//! holding the edit is rebuilt, and the small tree over the chunks), so a line-count change under
//! soft wrap does not rebuild an O(n) index (E1, W1). With uniform heights (QE1, no wrap) every
//! line has the same height; measured heights per line (soft wrap) are ED2/QE4 and use
//! [`HeightIndex::set`] and [`HeightIndex::splice`].
//!
//! Sums are kept in `f64`, so heights such as 13.6 add up without drift over 100k lines.
//!
//! A copy of egui_quire's `height.rs` (no egui in it); both can move to one crate (docs/PRESTO-QUIRE.md).

use std::ops::Range;

/// Lines per chunk when building; splices keep chunks between `CHUNK / 4` and `2 * CHUNK`.
const CHUNK: usize = 512;

/// Fenwick tree of `f64` (1-based; `t[0]` unused).
fn fen_build(vals: impl ExactSizeIterator<Item = f64>, t: &mut Vec<f64>) {
    let n = vals.len();
    t.clear();
    t.reserve(n + 1);
    t.push(0.0);
    t.extend(vals);
    for i in 1..=n {
        let j = i + (i & i.wrapping_neg());
        if j <= n {
            let v = t[i];
            t[j] += v;
        }
    }
}

fn fen_add(t: &mut [f64], i: usize, delta: f64) {
    let mut i = i + 1;
    while i < t.len() {
        t[i] += delta;
        i += i & i.wrapping_neg();
    }
}

/// Sum of the first `i` values.
fn fen_prefix(t: &[f64], i: usize) -> f64 {
    let mut i = i.min(t.len() - 1);
    let mut s = 0.0;
    while i > 0 {
        s += t[i];
        i &= i - 1;
    }
    s
}

/// Largest `pos` with prefix(pos) <= y, and y minus that prefix.
fn fen_search(t: &[f64], y: f64) -> (usize, f64) {
    let n = t.len() - 1;
    let mut pos = 0usize;
    let mut rem = y;
    let mut step = n.next_power_of_two();
    while step > 0 {
        let next = pos + step;
        if next <= n && t[next] <= rem {
            pos = next;
            rem -= t[next];
        }
        step >>= 1;
    }
    (pos, rem)
}

/// Fenwick tree of `usize` counts (lines per chunk).
fn cnt_build(vals: impl ExactSizeIterator<Item = usize>, t: &mut Vec<usize>) {
    let n = vals.len();
    t.clear();
    t.reserve(n + 1);
    t.push(0);
    t.extend(vals);
    for i in 1..=n {
        let j = i + (i & i.wrapping_neg());
        if j <= n {
            let v = t[i];
            t[j] += v;
        }
    }
}

/// One run of consecutive lines.
#[derive(Clone, Debug, Default)]
struct Chunk {
    heights: Vec<f32>,
    /// Fenwick tree over `heights`.
    tree: Vec<f64>,
}

impl Chunk {
    fn new(heights: Vec<f32>) -> Self {
        let mut c = Self {
            heights,
            tree: Vec::new(),
        };
        fen_build(c.heights.iter().map(|&h| h as f64), &mut c.tree);
        c
    }

    fn sum(&self) -> f64 {
        fen_prefix(&self.tree, self.heights.len())
    }
}

/// Per-line heights with prefix sums.
///
/// While every line has the same height (no soft wrap yet) the index stores only that height and
/// the line count: building it is O(1), so a line-count change while typing in a large file does
/// not rebuild anything (E1). The chunks are materialised on the first `set` of a different
/// height.
#[derive(Clone, Debug, Default)]
pub struct HeightIndex {
    /// Empty while `uniform`.
    chunks: Vec<Chunk>,
    /// Fenwick trees over the chunks' height sums and line counts.
    sums: Vec<f64>,
    counts: Vec<usize>,
    /// Every line has this height.
    uniform: Option<f32>,
    len: usize,
}

impl HeightIndex {
    /// `lines` lines, each `height` tall.
    pub fn uniform(lines: usize, height: f32) -> Self {
        Self {
            chunks: Vec::new(),
            sums: Vec::new(),
            counts: Vec::new(),
            uniform: Some(height),
            len: lines,
        }
    }

    /// `lines` lines, line `i` `height(i)` tall (O(n)).
    pub fn from_fn(lines: usize, height: impl FnMut(usize) -> f32) -> Self {
        let mut s = Self::uniform(0, 0.0);
        s.uniform = None;
        s.len = lines;
        s.set_chunks((0..lines).map(height).collect());
        s
    }

    /// Replace the chunks with `heights` cut into chunks of `CHUNK` lines.
    fn set_chunks(&mut self, heights: Vec<f32>) {
        self.chunks = heights
            .chunks(CHUNK)
            .map(|c| Chunk::new(c.to_vec()))
            .collect();
        self.rebuild_top();
    }

    /// Rebuild the trees over the chunks (O(chunks)).
    fn rebuild_top(&mut self) {
        let sums: Vec<f64> = self.chunks.iter().map(Chunk::sum).collect();
        fen_build(sums.into_iter(), &mut self.sums);
        cnt_build(
            self.chunks.iter().map(|c| c.heights.len()),
            &mut self.counts,
        );
    }

    /// Switch from the uniform representation to chunks.
    fn materialize(&mut self) {
        if let Some(h) = self.uniform.take() {
            self.set_chunks(vec![h; self.len]);
        }
    }

    /// (chunk, index in it) of `line < len`.
    fn locate(&self, line: usize) -> (usize, usize) {
        // Largest chunk count c with lines(c) <= line: line is in chunk c.
        let n = self.counts.len() - 1;
        let mut pos = 0usize;
        let mut rem = line;
        let mut step = n.next_power_of_two();
        while step > 0 {
            let next = pos + step;
            if next <= n && self.counts[next] <= rem {
                pos = next;
                rem -= self.counts[next];
            }
            step >>= 1;
        }
        (pos, rem)
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
        assert!(line < self.len, "line {line} out of range {}", self.len);
        match self.uniform {
            Some(h) => h,
            None => {
                let (c, i) = self.locate(line);
                self.chunks[c].heights[i]
            }
        }
    }

    /// Set the height of `line`.
    pub fn set(&mut self, line: usize, height: f32) {
        assert!(line < self.len, "line {line} out of range {}", self.len);
        if self.uniform == Some(height) {
            return;
        }
        self.materialize();
        let (c, i) = self.locate(line);
        let chunk = &mut self.chunks[c];
        let delta = height as f64 - chunk.heights[i] as f64;
        if delta == 0.0 {
            return;
        }
        chunk.heights[i] = height;
        fen_add(&mut chunk.tree, i, delta);
        fen_add(&mut self.sums, c, delta);
    }

    /// Replace lines `old` with `new_lines` lines of height `height` (the index then has
    /// `len() - old.len() + new_lines` lines). O(old + new + chunk + chunks).
    pub fn splice(&mut self, old: Range<usize>, new_lines: usize, height: f32) {
        assert!(
            old.start <= old.end && old.end <= self.len,
            "splice {old:?} out of range {}",
            self.len
        );
        if old.len() == new_lines && self.uniform == Some(height) {
            return;
        }
        let new_len = self.len - old.len() + new_lines;
        if let Some(h) = self.uniform
            && (h == height || new_len == 0)
        {
            self.len = new_len;
            return;
        }
        self.materialize();
        self.len = new_len;
        if self.chunks.is_empty() {
            self.set_chunks(vec![height; new_len]);
            return;
        }
        // The chunks touched: the one holding old.start through the one holding old.end (an
        // end at a chunk's start still takes that chunk, so an insertion at the very end has
        // one), plus the next one when the result would be small.
        let total_before = self.len - new_lines + old.len();
        let (c0, i0) = if old.start < total_before {
            self.locate(old.start)
        } else {
            let last = self.chunks.len() - 1;
            (last, self.chunks[last].heights.len())
        };
        let (mut c1, i1) = if old.end < total_before {
            self.locate(old.end)
        } else {
            let last = self.chunks.len() - 1;
            (last, self.chunks[last].heights.len())
        };
        let mut merged: Vec<f32> = Vec::new();
        merged.extend_from_slice(&self.chunks[c0].heights[..i0]);
        merged.extend(std::iter::repeat_n(height, new_lines));
        merged.extend_from_slice(&self.chunks[c1].heights[i1..]);
        if merged.len() < CHUNK / 4 && c1 + 1 < self.chunks.len() {
            c1 += 1;
            merged.extend_from_slice(&self.chunks[c1].heights);
        }
        let pieces: Vec<Chunk> = if merged.is_empty() {
            Vec::new()
        } else if merged.len() <= 2 * CHUNK {
            vec![Chunk::new(merged)]
        } else {
            let k = merged.len().div_ceil(CHUNK);
            let per = merged.len().div_ceil(k);
            merged.chunks(per).map(|c| Chunk::new(c.to_vec())).collect()
        };
        self.chunks.splice(c0..=c1, pieces);
        self.rebuild_top();
    }

    /// Top y of `line` (sum of the heights of the lines before it). `line == len()` gives the total.
    pub fn y_of(&self, line: usize) -> f32 {
        let line = line.min(self.len);
        if let Some(h) = self.uniform {
            return line as f32 * h;
        }
        if line == self.len {
            return self.total();
        }
        let (c, i) = self.locate(line);
        (fen_prefix(&self.sums, c) + fen_prefix(&self.chunks[c].tree, i)) as f32
    }

    /// Total height.
    pub fn total(&self) -> f32 {
        match self.uniform {
            Some(h) => self.len as f32 * h,
            None => fen_prefix(&self.sums, self.chunks.len()) as f32,
        }
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
        // Largest line count with prefix <= y: first the whole chunks, then inside the next.
        let (c, rem) = fen_search(&self.sums, y as f64);
        if c >= self.chunks.len() {
            return n - 1;
        }
        let before = self.lines_before(c);
        let (i, _) = fen_search(&self.chunks[c].tree, rem);
        (before + i.min(self.chunks[c].heights.len() - 1)).min(n - 1)
    }

    /// Lines in the chunks before chunk `c`.
    fn lines_before(&self, c: usize) -> usize {
        let mut i = c.min(self.counts.len() - 1);
        let mut s = 0;
        while i > 0 {
            s += self.counts[i];
            i &= i - 1;
        }
        s
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
    fn set_and_splice() {
        let mut h = HeightIndex::uniform(5, 10.0);
        h.set(2, 30.0);
        assert_eq!(h.y_of(3), 50.0);
        assert_eq!(h.line_at(25.0), 2);
        assert_eq!(h.line_at(49.0), 2);
        assert_eq!(h.line_at(50.0), 3);
        h.splice(5..5, 2, 10.0);
        assert_eq!(h.total(), 90.0);
        assert_eq!(h.get(2), 30.0);
        h.splice(1..3, 1, 10.0);
        assert_eq!(h.len(), 6);
        assert_eq!(h.total(), 60.0);
        assert!(HeightIndex::uniform(0, 1.0).is_empty());
    }

    #[test]
    fn uniform_agrees_with_the_chunks() {
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

    /// Splices, sets and queries against a plain vector, through chunk splits and merges.
    #[test]
    fn splices_agree_with_a_vector() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let mut v: Vec<f32> = (0..3000).map(|i| ((i % 4) as f32) * 10.0).collect();
        let mut h = HeightIndex::from_fn(v.len(), |i| v[i]);
        for step in 0..3000 {
            match rnd(4) {
                0 | 1 => {
                    // Small edit (Enter, join, paste of a few lines).
                    let a = rnd(v.len() + 1);
                    let b = (a + rnd(3)).min(v.len());
                    let n = rnd(4);
                    let ht = (rnd(3) * 10) as f32;
                    v.splice(a..b, std::iter::repeat_n(ht, n));
                    h.splice(a..b, n, ht);
                }
                2 => {
                    if !v.is_empty() {
                        let l = rnd(v.len());
                        let ht = (rnd(5) * 10) as f32;
                        v[l] = ht;
                        h.set(l, ht);
                    }
                }
                _ => {
                    // Occasionally a large paste or deletion.
                    let a = rnd(v.len() + 1);
                    let b = (a + rnd(1500)).min(v.len());
                    let n = if rnd(2) == 0 { rnd(1500) } else { 0 };
                    v.splice(a..b, std::iter::repeat_n(10.0, n));
                    h.splice(a..b, n, 10.0);
                }
            }
            assert_eq!(h.len(), v.len(), "step {step}");
            if step % 50 == 0 || v.len() < 10 {
                let mut y = 0.0f32;
                for (i, &x) in v.iter().enumerate() {
                    assert_eq!(h.get(i), x, "step {step} line {i}");
                    assert_eq!(h.y_of(i), y, "step {step} line {i}");
                    // y <= 0 is line 0 (never folded away in the editor) whatever its height.
                    if x > 0.0 && y > 0.0 {
                        assert_eq!(h.line_at(y), i, "step {step} y {y}");
                        assert_eq!(h.line_at(y + x * 0.5), i, "step {step}");
                    }
                    y += x;
                }
                assert_eq!(h.total(), y);
            }
        }
        // Everything removed, then lines again.
        let n = h.len();
        h.splice(0..n, 0, 10.0);
        assert!(h.is_empty() && h.total() == 0.0 && h.line_at(5.0) == 0);
        h.splice(0..0, 3, 10.0);
        assert_eq!((h.len(), h.total(), h.line_at(25.0)), (3, 30.0, 2));
    }
}
