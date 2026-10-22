//! Per-line galley cache.
//!
//! Key: (line content hash, token style hash, font id, pixels per point, wrap width). The theme
//! is not in the key: galleys are laid out with the theme's colours, so the theme's own hash is
//! folded into the style hash by the caller. LRU-bounded: when the cache grows past its capacity
//! the least recently used quarter is dropped.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{FontId, Galley};

/// Cache key for one laid-out line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GalleyKey {
    pub content: u64,
    pub style: u64,
    pub font: u64,
    pub pixels_per_point_bits: u32,
    pub wrap_width_bits: u32,
}

impl GalleyKey {
    pub fn new(content: u64, style: u64, font: &FontId, pixels_per_point: f32, wrap_width: f32) -> Self {
        Self {
            content,
            style,
            font: hash_one(font),
            pixels_per_point_bits: pixels_per_point.to_bits(),
            wrap_width_bits: wrap_width.to_bits(),
        }
    }
}

/// Hash any value with a fixed hasher (stable within a process).
pub fn hash_one(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// LRU-bounded map from [`GalleyKey`] to galley.
#[derive(Debug)]
pub struct GalleyCache {
    map: HashMap<GalleyKey, (Arc<Galley>, u64)>,
    tick: u64,
    capacity: usize,
    /// Number of galleys laid out (cache misses) since creation; tests read it.
    pub misses: u64,
}

impl Default for GalleyCache {
    fn default() -> Self {
        Self::with_capacity(4096)
    }
}

impl GalleyCache {
    pub fn with_capacity(capacity: usize) -> Self {
        Self { map: HashMap::new(), tick: 0, capacity: capacity.max(4), misses: 0 }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Get the galley for `key`, laying it out with `make` on a miss.
    pub fn get_or_insert(&mut self, key: GalleyKey, make: impl FnOnce() -> Arc<Galley>) -> Arc<Galley> {
        self.tick += 1;
        let tick = self.tick;
        if let Some((g, used)) = self.map.get_mut(&key) {
            *used = tick;
            return g.clone();
        }
        self.misses += 1;
        let g = make();
        self.map.insert(key, (g.clone(), tick));
        if self.map.len() > self.capacity {
            self.evict();
        }
        g
    }

    /// Drop the least recently used quarter.
    fn evict(&mut self) {
        let mut ticks: Vec<u64> = self.map.values().map(|(_, t)| *t).collect();
        let cut = ticks.len() / 4;
        let (_, &mut threshold, _) = ticks.select_nth_unstable(cut);
        self.map.retain(|_, (_, t)| *t > threshold);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}
