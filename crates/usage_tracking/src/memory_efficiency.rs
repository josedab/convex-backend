//! Memory efficiency utilities for reduced lock contention and allocations.
//!
//! This module provides optimized data structures for high-frequency operations:
//! - `ThreadLocalCounter`: Per-thread counters that aggregate on demand
//! - `AtomicCounter`: Simple atomic counter for aggregate metrics
//! - `ConcurrentUsageMap`: Lock-free concurrent map for usage tracking

use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use thread_local::ThreadLocal;

/// A counter that maintains per-thread values to avoid lock contention.
///
/// Each thread has its own atomic counter, and the total is computed by
/// iterating over all thread-local values. This is ideal for high-frequency
/// increment operations where reads are less common.
///
/// # Example
/// ```ignore
/// let counter = ThreadLocalCounter::new();
/// counter.increment(10);
/// counter.increment(5);
/// assert_eq!(counter.sum(), 15);
/// ```
#[derive(Debug, Default)]
pub struct ThreadLocalCounter {
    locals: ThreadLocal<AtomicU64>,
}

impl ThreadLocalCounter {
    /// Creates a new thread-local counter.
    pub fn new() -> Self {
        Self {
            locals: ThreadLocal::new(),
        }
    }

    /// Increments the counter by the specified amount.
    ///
    /// This operation is lock-free and thread-local.
    #[inline]
    pub fn increment(&self, value: u64) {
        let atomic = self.locals.get_or(|| AtomicU64::new(0));
        atomic.fetch_add(value, Ordering::Relaxed);
    }

    /// Returns the sum of all thread-local counters.
    ///
    /// This requires iterating over all thread-local values.
    pub fn sum(&self) -> u64 {
        self.locals.iter().map(|c| c.load(Ordering::Relaxed)).sum()
    }

    /// Resets all counters to zero and returns the previous sum.
    pub fn reset(&self) -> u64 {
        let mut total = 0;
        for atomic in self.locals.iter() {
            total += atomic.swap(0, Ordering::Relaxed);
        }
        total
    }
}

impl Clone for ThreadLocalCounter {
    fn clone(&self) -> Self {
        // Clone creates a new counter with the same sum
        let new = Self::new();
        new.increment(self.sum());
        new
    }
}

/// A simple atomic counter for aggregate metrics.
///
/// This is useful for simple counters that need thread-safe access
/// without the overhead of maintaining per-thread values.
#[derive(Debug, Default)]
pub struct AtomicCounter {
    value: AtomicU64,
}

impl AtomicCounter {
    /// Creates a new atomic counter initialized to zero.
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Creates a new atomic counter with the specified initial value.
    pub fn with_value(value: u64) -> Self {
        Self {
            value: AtomicU64::new(value),
        }
    }

    /// Adds to the counter and returns the previous value.
    #[inline]
    pub fn fetch_add(&self, value: u64) -> u64 {
        self.value.fetch_add(value, Ordering::Relaxed)
    }

    /// Increments the counter by the specified amount.
    #[inline]
    pub fn increment(&self, value: u64) {
        self.value.fetch_add(value, Ordering::Relaxed);
    }

    /// Returns the current value.
    #[inline]
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Sets the counter to a new value and returns the previous value.
    #[inline]
    pub fn swap(&self, value: u64) -> u64 {
        self.value.swap(value, Ordering::Relaxed)
    }

    /// Resets the counter to zero and returns the previous value.
    #[inline]
    pub fn reset(&self) -> u64 {
        self.value.swap(0, Ordering::Relaxed)
    }
}

impl Clone for AtomicCounter {
    fn clone(&self) -> Self {
        Self::with_value(self.get())
    }
}

/// A concurrent hash map optimized for usage tracking.
///
/// This provides lock-free access for reads and fine-grained locking for writes,
/// making it ideal for scenarios with many concurrent readers and writers.
#[derive(Debug)]
pub struct ConcurrentUsageMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    map: DashMap<K, V>,
}

impl<K, V> Default for ConcurrentUsageMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> ConcurrentUsageMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Creates a new empty concurrent usage map.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Creates a new concurrent usage map with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: DashMap::with_capacity(capacity),
        }
    }

    /// Returns the number of entries in the map.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Inserts a key-value pair into the map.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.map.insert(key, value)
    }

    /// Removes a key from the map and returns its value.
    pub fn remove(&self, key: &K) -> Option<(K, V)> {
        self.map.remove(key)
    }

    /// Returns true if the map contains the specified key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Clears all entries from the map.
    pub fn clear(&self) {
        self.map.clear();
    }
}

impl<K> ConcurrentUsageMap<K, u64>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Increments the value for the given key, inserting 0 if it doesn't exist.
    ///
    /// This is the primary operation for usage tracking - it atomically
    /// increments the counter for a given key.
    #[inline]
    pub fn increment(&self, key: K, delta: u64) {
        self.map
            .entry(key)
            .and_modify(|v| *v += delta)
            .or_insert(delta);
    }

    /// Gets the current value for the key, or 0 if it doesn't exist.
    pub fn get_or_default(&self, key: &K) -> u64 {
        self.map.get(key).map(|v| *v).unwrap_or(0)
    }

    /// Collects all entries into a BTreeMap for serialization.
    pub fn to_btree_map(&self) -> std::collections::BTreeMap<K, u64>
    where
        K: Ord,
    {
        self.map.iter().map(|r| (r.key().clone(), *r.value())).collect()
    }

    /// Merges another map's values into this one.
    pub fn merge(&self, other: &Self) {
        for entry in other.map.iter() {
            self.increment(entry.key().clone(), *entry.value());
        }
    }

    /// Returns the sum of all values in the map.
    pub fn sum(&self) -> u64 {
        self.map.iter().map(|r| *r.value()).sum()
    }
}

impl<K, V> Clone for ConcurrentUsageMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        let new_map = DashMap::with_capacity(self.map.len());
        for entry in self.map.iter() {
            new_map.insert(entry.key().clone(), entry.value().clone());
        }
        Self { map: new_map }
    }
}

/// Aggregate counters using atomic operations for high-frequency updates.
///
/// This struct provides fast, lock-free counters for common aggregate metrics.
/// Use this when you need simple counters without per-key breakdown.
#[derive(Debug, Default, Clone)]
pub struct AggregateCounters {
    /// Total database read bytes
    pub database_read_bytes: AtomicCounter,
    /// Total database write bytes
    pub database_write_bytes: AtomicCounter,
    /// Total storage read bytes
    pub storage_read_bytes: AtomicCounter,
    /// Total storage write bytes
    pub storage_write_bytes: AtomicCounter,
    /// Total vector index read bytes
    pub vector_read_bytes: AtomicCounter,
    /// Total vector index write bytes
    pub vector_write_bytes: AtomicCounter,
}

impl AggregateCounters {
    /// Creates new aggregate counters initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets all counters to zero.
    pub fn reset(&self) {
        self.database_read_bytes.reset();
        self.database_write_bytes.reset();
        self.storage_read_bytes.reset();
        self.storage_write_bytes.reset();
        self.vector_read_bytes.reset();
        self.vector_write_bytes.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new();
        assert_eq!(counter.get(), 0);

        counter.increment(10);
        assert_eq!(counter.get(), 10);

        counter.increment(5);
        assert_eq!(counter.get(), 15);

        let prev = counter.reset();
        assert_eq!(prev, 15);
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_thread_local_counter() {
        let counter = ThreadLocalCounter::new();
        counter.increment(10);
        counter.increment(5);
        assert_eq!(counter.sum(), 15);

        let prev = counter.reset();
        assert_eq!(prev, 15);
        assert_eq!(counter.sum(), 0);
    }

    #[test]
    fn test_concurrent_usage_map() {
        let map: ConcurrentUsageMap<String, u64> = ConcurrentUsageMap::new();

        map.increment("table1".to_string(), 100);
        map.increment("table1".to_string(), 50);
        map.increment("table2".to_string(), 200);

        assert_eq!(map.get_or_default(&"table1".to_string()), 150);
        assert_eq!(map.get_or_default(&"table2".to_string()), 200);
        assert_eq!(map.sum(), 350);
    }

    #[test]
    fn test_concurrent_usage_map_merge() {
        let map1: ConcurrentUsageMap<String, u64> = ConcurrentUsageMap::new();
        let map2: ConcurrentUsageMap<String, u64> = ConcurrentUsageMap::new();

        map1.increment("table1".to_string(), 100);
        map2.increment("table1".to_string(), 50);
        map2.increment("table2".to_string(), 200);

        map1.merge(&map2);

        assert_eq!(map1.get_or_default(&"table1".to_string()), 150);
        assert_eq!(map1.get_or_default(&"table2".to_string()), 200);
    }

    #[test]
    fn test_aggregate_counters() {
        let counters = AggregateCounters::new();

        counters.database_read_bytes.increment(1000);
        counters.database_write_bytes.increment(500);

        assert_eq!(counters.database_read_bytes.get(), 1000);
        assert_eq!(counters.database_write_bytes.get(), 500);

        counters.reset();
        assert_eq!(counters.database_read_bytes.get(), 0);
        assert_eq!(counters.database_write_bytes.get(), 0);
    }
}
