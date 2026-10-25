//! The two-generation caches of shaped lines and wrapped rows (svelters
//! `plans/research/presto/notes/round2/text-stack.md` §5.2).
//!
//! An entry used in this frame or the previous one survives [`GenCache::end_frame`]; the rest are
//! dropped when the cache is over its byte budget, oldest generation first. Lines scrolled away
//! therefore stay for one frame (a scroll back costs nothing) and go once the budget is reached.
//! A font-set change clears everything ([`GenCache::clear`]).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Hash of one value with the std hasher (keys, content hashes).
pub fn hash_one(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// The key of a shaped line: what its glyphs depend on, nothing they do not (colours are paint ids
/// resolved at record time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShapeKey {
    /// The line's text.
    pub content: u64,
    /// Token style run boundaries and styles that change shaping (weight, italic), not colours.
    pub runs: u64,
    pub font_generation: u64,
    /// Font size in device pixels (`f32` bits).
    pub ppem_bits: u32,
    /// Variation coordinates and OpenType features of the editor font.
    pub features: u64,
}

/// The key of a line's wrapped rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowsKey {
    pub shape: ShapeKey,
    /// Wrap width in points (`f32` bits); `u32::MAX` = no wrapping.
    pub width_bits: u32,
    /// The continuation rows' indentation (points, `f32` bits).
    pub indent_bits: u32,
}

/// The size an entry takes, for the byte budget.
pub trait CacheBytes {
    fn cache_bytes(&self) -> usize;
}

struct Entry<V> {
    value: V,
    used: u64,
    bytes: usize,
}

/// A map whose entries age by frame generation.
pub struct GenCache<K, V> {
    map: HashMap<K, Entry<V>>,
    generation: u64,
    bytes: usize,
    budget: usize,
    hits: u64,
    misses: u64,
}

impl<K, V> std::fmt::Debug for GenCache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenCache")
            .field("len", &self.map.len())
            .field("generation", &self.generation)
            .field("bytes", &self.bytes)
            .field("budget", &self.budget)
            .finish()
    }
}

impl<K: Hash + Eq + Copy, V: Clone + CacheBytes> GenCache<K, V> {
    pub fn new(budget: usize) -> Self {
        GenCache {
            map: HashMap::new(),
            generation: 0,
            bytes: 0,
            budget,
            hits: 0,
            misses: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// (hits, misses) since creation.
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// The entry for `key`, marking it used this generation.
    pub fn get(&mut self, key: &K) -> Option<V> {
        let generation = self.generation;
        let e = self.map.get_mut(key)?;
        e.used = generation;
        self.hits += 1;
        Some(e.value.clone())
    }

    /// The entry without marking it used.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|e| &e.value)
    }

    pub fn insert(&mut self, key: K, value: V) {
        let bytes = value.cache_bytes();
        self.misses += 1;
        if let Some(old) = self.map.insert(
            key,
            Entry {
                value,
                used: self.generation,
                bytes,
            },
        ) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }

    pub fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> V {
        if let Some(v) = self.get(&key) {
            return v;
        }
        let v = make();
        self.insert(key, v.clone());
        v
    }

    /// Ends a frame: entries older than the previous generation are dropped while over budget.
    pub fn end_frame(&mut self) {
        self.generation += 1;
        if self.bytes <= self.budget {
            return;
        }
        let keep_from = self.generation.saturating_sub(1);
        let mut old: Vec<(u64, K)> = self
            .map
            .iter()
            .filter(|(_, e)| e.used < keep_from)
            .map(|(k, e)| (e.used, *k))
            .collect();
        old.sort_unstable_by_key(|(used, _)| *used);
        for (_, k) in old {
            if self.bytes <= self.budget {
                break;
            }
            if let Some(e) = self.map.remove(&k) {
                self.bytes -= e.bytes;
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct B(usize);
    impl CacheBytes for B {
        fn cache_bytes(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn entries_of_the_last_two_generations_survive_the_budget() {
        let mut c: GenCache<u32, B> = GenCache::new(10);
        c.insert(1, B(6));
        c.end_frame();
        c.insert(2, B(6));
        c.end_frame();
        // 1 was last used two generations ago: dropped; 2 is from the previous one: kept.
        assert!(c.peek(&1).is_none());
        assert!(c.peek(&2).is_some());
        assert_eq!(c.bytes(), 6);
    }

    #[test]
    fn under_budget_nothing_is_dropped() {
        let mut c: GenCache<u32, B> = GenCache::new(100);
        c.insert(1, B(6));
        for _ in 0..5 {
            c.end_frame();
        }
        assert!(c.peek(&1).is_some());
    }

    #[test]
    fn a_hit_renews_the_entry() {
        let mut c: GenCache<u32, B> = GenCache::new(5);
        c.insert(1, B(6));
        c.end_frame();
        assert!(c.get(&1).is_some());
        c.end_frame();
        assert!(c.peek(&1).is_some(), "used in the previous generation");
        c.end_frame();
        assert!(c.peek(&1).is_none());
    }
}
