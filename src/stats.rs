use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Shared, lock-free mining statistics.
pub struct Stats {
    pub hash_count: Vec<AtomicU64>,  // per-thread hash counters
    pub accepted:   AtomicU64,
    pub rejected:   AtomicU64,
    pub start:      Instant,
}

impl Stats {
    pub fn new(threads: usize) -> Self {
        let hash_count = (0..threads).map(|_| AtomicU64::new(0)).collect();
        Self {
            hash_count,
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            start:    Instant::now(),
        }
    }

    /// Called by each miner thread every N hashes.
    #[inline]
    pub fn add_hashes(&self, thread_id: usize, n: u64) {
        self.hash_count[thread_id].fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_accepted(&self) { self.accepted.fetch_add(1, Ordering::Relaxed); }
    pub fn add_rejected(&self) { self.rejected.fetch_add(1, Ordering::Relaxed); }

    /// Total hashes across all threads since start.
    pub fn total_hashes(&self) -> u64 {
        self.hash_count.iter().map(|c| c.load(Ordering::Relaxed)).sum()
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64().max(0.001)
    }

    /// Global hashrate in H/s.
    pub fn hashrate(&self) -> f64 {
        self.total_hashes() as f64 / self.elapsed_secs()
    }

    /// Per-thread hashrate in H/s.
    pub fn thread_hashrate(&self, id: usize) -> f64 {
        self.hash_count[id].load(Ordering::Relaxed) as f64 / self.elapsed_secs()
    }

    pub fn accepted_count(&self) -> u64 { self.accepted.load(Ordering::Relaxed) }
    pub fn rejected_count(&self) -> u64 { self.rejected.load(Ordering::Relaxed) }
}

/// Format H/s into a human-readable string (KH/s, MH/s, GH/s).
pub fn format_hashrate(hs: f64) -> String {
    if hs >= 1_000_000_000.0 {
        format!("{:.2} GH/s", hs / 1_000_000_000.0)
    } else if hs >= 1_000_000.0 {
        format!("{:.2} MH/s", hs / 1_000_000.0)
    } else if hs >= 1_000.0 {
        format!("{:.2} KH/s", hs / 1_000.0)
    } else {
        format!("{:.0} H/s", hs)
    }
}
