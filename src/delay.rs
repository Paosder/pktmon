//! How long a packet event took from the driver to this process.
//!
//! The ETW consumer compares each event's timestamp with the clock when its callback runs and
//! counts the difference in a fixed set of buckets. Nothing about the packet is kept.

use std::sync::atomic::{AtomicU64, Ordering};

/// Upper bounds of the delay buckets in milliseconds, inclusive. A delay above the last bound
/// falls into one more bucket after them.
pub const DELAY_BOUNDS_MS: [u32; 10] = [10, 25, 50, 75, 100, 125, 150, 200, 300, 500];

/// The number of buckets, one per bound plus the one above the last bound.
pub const DELAY_BUCKETS: usize = DELAY_BOUNDS_MS.len() + 1;

/// Delays counted since the previous [`take_delivery_delay`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DelaySnapshot {
    /// Events counted.
    pub count: u64,
    /// The largest delay in microseconds.
    pub max_us: u64,
    /// Events per bucket, in the order of [`DELAY_BOUNDS_MS`] and then the bucket above them.
    pub buckets: [u64; DELAY_BUCKETS],
}

pub(crate) struct DelayStats {
    count: AtomicU64,
    max_us: AtomicU64,
    buckets: [AtomicU64; DELAY_BUCKETS],
}

impl DelayStats {
    pub(crate) const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            buckets: [const { AtomicU64::new(0) }; DELAY_BUCKETS],
        }
    }

    pub(crate) fn record(&self, delay_us: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.max_us.fetch_max(delay_us, Ordering::Relaxed);
        self.buckets[bucket_index(delay_us)].fetch_add(1, Ordering::Relaxed);
    }

    /// Reads and clears the counters one by one. A concurrent `record` lands in this snapshot or
    /// the next one, never in neither.
    pub(crate) fn take(&self) -> DelaySnapshot {
        let mut snapshot = DelaySnapshot {
            count: self.count.swap(0, Ordering::Relaxed),
            max_us: self.max_us.swap(0, Ordering::Relaxed),
            buckets: [0; DELAY_BUCKETS],
        };
        for (slot, bucket) in snapshot.buckets.iter_mut().zip(&self.buckets) {
            *slot = bucket.swap(0, Ordering::Relaxed);
        }
        snapshot
    }
}

static STATS: DelayStats = DelayStats::new();

pub(crate) fn record_delivery_delay(delay_us: u64) {
    STATS.record(delay_us);
}

/// Delays of the packet events the ETW consumer delivered since the previous call, then clears
/// them. Only the legacy backend measures this. The real-time backend leaves it empty.
pub fn take_delivery_delay() -> DelaySnapshot {
    STATS.take()
}

pub(crate) fn bucket_index(delay_us: u64) -> usize {
    DELAY_BOUNDS_MS
        .iter()
        .position(|bound_ms| delay_us <= u64::from(*bound_ms) * 1000)
        .unwrap_or(DELAY_BOUNDS_MS.len())
}

/// Microseconds for a span of performance counter ticks. A span that is not positive (the
/// event is stamped after the callback runs, or the clocks disagree) counts as zero.
pub(crate) fn ticks_to_us(ticks: i64, frequency: u64) -> u64 {
    let Ok(ticks) = u64::try_from(ticks) else {
        return 0;
    };
    if frequency == 0 {
        return 0;
    }
    u64::try_from(u128::from(ticks) * 1_000_000 / u128::from(frequency)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delay_on_a_bound_stays_in_that_bucket_and_one_microsecond_more_moves_up() {
        assert_eq!(bucket_index(0), 0);
        assert_eq!(bucket_index(10_000), 0);
        assert_eq!(bucket_index(10_001), 1);
        assert_eq!(bucket_index(50_000), 2);
        assert_eq!(bucket_index(150_000), 6);
        assert_eq!(bucket_index(150_001), 7);
        assert_eq!(bucket_index(500_000), 9);
        assert_eq!(bucket_index(500_001), 10);
        assert_eq!(bucket_index(u64::MAX), 10);
    }

    #[test]
    fn recording_counts_the_event_and_keeps_the_largest_delay() {
        let stats = DelayStats::new();
        stats.record(5_000);
        stats.record(147_100);
        stats.record(89_200);
        let snapshot = stats.take();
        assert_eq!(snapshot.count, 3);
        assert_eq!(snapshot.max_us, 147_100);
        assert_eq!(snapshot.buckets, [1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn taking_clears_the_counters_for_the_next_window() {
        let stats = DelayStats::new();
        stats.record(1);
        assert_eq!(stats.take().count, 1);
        assert_eq!(stats.take(), DelaySnapshot::default());
        stats.record(600_000);
        let snapshot = stats.take();
        assert_eq!((snapshot.count, snapshot.max_us, snapshot.buckets[10]), (1, 600_000, 1));
    }

    #[test]
    fn ticks_convert_at_the_counter_frequency() {
        assert_eq!(ticks_to_us(10_000_000, 10_000_000), 1_000_000);
        assert_eq!(ticks_to_us(500_000, 10_000_000), 50_000);
        assert_eq!(ticks_to_us(1, 10_000_000), 0);
        assert_eq!(ticks_to_us(i64::MAX, 1), u64::MAX);
    }

    #[test]
    fn a_span_that_is_not_positive_counts_as_zero() {
        assert_eq!(ticks_to_us(0, 10_000_000), 0);
        assert_eq!(ticks_to_us(-5, 10_000_000), 0);
        assert_eq!(ticks_to_us(5, 0), 0);
    }
}
