//! Packets the Packet Monitor driver reports as missed on the real-time stream.
//!
//! Every data callback carries two counts, one for packets the driver could not write into the
//! stream buffer and one for packets it could not read back out. The counts are added up here
//! and nothing about the packets themselves is kept.

use std::sync::atomic::{AtomicU64, Ordering};

/// Missed packets counted since the process started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MissedPackets {
    /// The sum of the write counts the driver reported.
    pub write: u64,
    /// The sum of the read counts the driver reported.
    pub read: u64,
}

pub(crate) struct MissedStats {
    write: AtomicU64,
    read: AtomicU64,
}

impl MissedStats {
    pub(crate) const fn new() -> Self {
        Self {
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
        }
    }

    pub(crate) fn record(&self, write: u32, read: u32) {
        self.write.fetch_add(u64::from(write), Ordering::Relaxed);
        self.read.fetch_add(u64::from(read), Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> MissedPackets {
        MissedPackets {
            write: self.write.load(Ordering::Relaxed),
            read: self.read.load(Ordering::Relaxed),
        }
    }
}

static STATS: MissedStats = MissedStats::new();

pub(crate) fn record_missed_packets(write: u32, read: u32) {
    STATS.record(write, read);
}

/// The missed packets the real-time stream reported, summed over every capture since the process
/// started. The counts only grow, so a caller that wants one capture's share keeps the value from
/// when that capture began and subtracts it. The legacy backend does not report them.
pub fn missed_packets() -> MissedPackets {
    STATS.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_missed_until_a_callback_reports_it() {
        assert_eq!(MissedStats::new().snapshot(), MissedPackets::default());
    }

    #[test]
    fn write_and_read_counts_add_up_separately() {
        let stats = MissedStats::new();
        stats.record(3, 0);
        stats.record(0, 5);
        stats.record(4, 1);
        assert_eq!(stats.snapshot(), MissedPackets { write: 7, read: 6 });
    }

    // Field data from a 1.14.0 app session: after 2,200,741 frames the write+read total grew by
    // exactly 6 per frame for the next 12 million frames. A driver that carries 6 on every callback
    // of a stream missed 6 packets once, not 6 more on each packet.
    #[test]
    fn a_total_the_driver_repeats_on_every_callback_is_counted_once() {
        let stats = MissedStats::new();
        for _ in 0..1000 {
            stats.record(6, 0);
        }
        assert_eq!(stats.snapshot(), MissedPackets { write: 6, read: 0 });
    }

    #[test]
    fn reading_the_counts_does_not_clear_them() {
        let stats = MissedStats::new();
        stats.record(2, 2);
        assert_eq!(stats.snapshot(), stats.snapshot());
    }

    #[test]
    fn the_largest_counts_do_not_wrap() {
        let stats = MissedStats::new();
        stats.record(u32::MAX, u32::MAX);
        stats.record(u32::MAX, u32::MAX);
        assert_eq!(stats.snapshot(), MissedPackets { write: 2 * u64::from(u32::MAX), read: 2 * u64::from(u32::MAX) });
    }
}
