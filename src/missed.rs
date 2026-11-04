//! Packets the Packet Monitor driver reports as missed on the real-time stream.
//!
//! Every data callback carries two counts, one for packets the driver could not write into the
//! stream buffer and one for packets it could not read back out. A callback does not carry how
//! many packets were missed since the last one. It carries a running total that the driver keeps
//! repeating: a field capture of one app session showed the summed counts grow by exactly 6 on
//! every frame for 12 million frames after the driver had missed 6 packets once. So a stream's
//! share is how far its running total grew, and a total that only repeats adds nothing.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Missed packets counted since the process started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MissedPackets {
    /// The packets the driver could not write, summed over every stream.
    pub write: u64,
    /// The packets the driver could not read back, summed over every stream.
    pub read: u64,
}

/// The highest running total one stream has reported for each count.
///
/// The driver may call back from more than one thread, so a callback with a lower total than the
/// highest seen adds nothing, whichever order the callbacks run in.
#[derive(Debug)]
pub(crate) struct StreamMissed {
    write: AtomicU32,
    read: AtomicU32,
}

impl StreamMissed {
    pub(crate) const fn new() -> Self {
        Self {
            write: AtomicU32::new(0),
            read: AtomicU32::new(0),
        }
    }

    /// How far the write and read totals grew with this callback.
    fn growth(&self, write: u32, read: u32) -> (u32, u32) {
        (
            write.saturating_sub(self.write.fetch_max(write, Ordering::Relaxed)),
            read.saturating_sub(self.read.fetch_max(read, Ordering::Relaxed)),
        )
    }
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

    /// Adds what this callback's totals grew by over the stream's earlier ones, and returns that
    /// growth as `(write, read)`.
    pub(crate) fn record(&self, stream: &StreamMissed, write: u32, read: u32) -> (u32, u32) {
        let (write_growth, read_growth) = stream.growth(write, read);
        self.write
            .fetch_add(u64::from(write_growth), Ordering::Relaxed);
        self.read
            .fetch_add(u64::from(read_growth), Ordering::Relaxed);
        (write_growth, read_growth)
    }

    pub(crate) fn snapshot(&self) -> MissedPackets {
        MissedPackets {
            write: self.write.load(Ordering::Relaxed),
            read: self.read.load(Ordering::Relaxed),
        }
    }
}

static STATS: MissedStats = MissedStats::new();

pub(crate) fn record_missed_packets(stream: &StreamMissed, write: u32, read: u32) -> (u32, u32) {
    STATS.record(stream, write, read)
}

/// The missed packets the real-time streams reported, summed over every capture since the process
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

    // Field data from a 1.14.0 app session: after 2,200,741 frames the write+read total grew by
    // exactly 6 per frame for the next 12 million frames. A driver that carries 6 on every callback
    // of a stream missed 6 packets once, not 6 more on each packet.
    #[test]
    fn a_total_the_driver_repeats_on_every_callback_is_counted_once() {
        let stats = MissedStats::new();
        let stream = StreamMissed::new();
        for _ in 0..1000 {
            stats.record(&stream, 6, 0);
        }
        assert_eq!(stats.snapshot(), MissedPackets { write: 6, read: 0 });
    }

    #[test]
    fn a_total_that_grows_adds_only_its_growth() {
        let stats = MissedStats::new();
        let stream = StreamMissed::new();
        let growth: Vec<_> = [0, 2, 2, 5, 5]
            .iter()
            .map(|total| stats.record(&stream, *total, 0))
            .collect();
        assert_eq!(growth, [(0, 0), (2, 0), (0, 0), (3, 0), (0, 0)]);
        assert_eq!(stats.snapshot(), MissedPackets { write: 5, read: 0 });
    }

    #[test]
    fn write_and_read_totals_count_separately() {
        let stats = MissedStats::new();
        let stream = StreamMissed::new();
        stats.record(&stream, 3, 0);
        stats.record(&stream, 3, 5);
        stats.record(&stream, 4, 5);
        assert_eq!(stats.snapshot(), MissedPackets { write: 4, read: 5 });
    }

    #[test]
    fn a_lower_total_than_one_already_seen_adds_nothing() {
        let stats = MissedStats::new();
        let stream = StreamMissed::new();
        stats.record(&stream, 9, 4);
        stats.record(&stream, 3, 1);
        assert_eq!(stats.snapshot(), MissedPackets { write: 9, read: 4 });
    }

    #[test]
    fn every_stream_starts_its_own_total_from_zero() {
        let stats = MissedStats::new();
        let (first, second) = (StreamMissed::new(), StreamMissed::new());
        stats.record(&first, 6, 1);
        stats.record(&second, 4, 0);
        stats.record(&first, 6, 1);
        assert_eq!(stats.snapshot(), MissedPackets { write: 10, read: 1 });
    }

    #[test]
    fn callbacks_on_several_threads_count_the_highest_total_once() {
        let stats = MissedStats::new();
        let stream = StreamMissed::new();
        std::thread::scope(|scope| {
            for thread in 0..4u32 {
                let (stats, stream) = (&stats, &stream);
                scope.spawn(move || {
                    for total in 1..=1000u32 {
                        stats.record(stream, total + thread, 0);
                    }
                });
            }
        });
        assert_eq!(
            stats.snapshot(),
            MissedPackets {
                write: 1003,
                read: 0
            }
        );
    }

    #[test]
    fn reading_the_counts_does_not_clear_them() {
        let stats = MissedStats::new();
        stats.record(&StreamMissed::new(), 2, 2);
        assert_eq!(stats.snapshot(), stats.snapshot());
    }

    #[test]
    fn the_largest_totals_do_not_wrap() {
        let stats = MissedStats::new();
        stats.record(&StreamMissed::new(), u32::MAX, u32::MAX);
        stats.record(&StreamMissed::new(), u32::MAX, u32::MAX);
        assert_eq!(
            stats.snapshot(),
            MissedPackets {
                write: 2 * u64::from(u32::MAX),
                read: 2 * u64::from(u32::MAX)
            }
        );
    }
}
