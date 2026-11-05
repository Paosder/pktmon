//! What the Packet Monitor driver tells a real-time stream besides packets.
//!
//! The stream callbacks used to log these events and forget them. Here they are counted in
//! process-wide atomics instead, so an app can read numbers and codes later. The callbacks run on
//! the driver's threads, so nothing here locks or allocates. Only counts and driver codes are
//! kept, never packet bytes or addresses.

use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};

/// The `reason` of a process-info event that says a packet did not fit the stream buffer
/// (`ERROR_INSUFFICIENT_BUFFER`).
pub const REASON_INSUFFICIENT_BUFFER: u32 = 122;

/// The largest `PacketLength` of each insufficient-buffer length class but the last.
const LEN_CLASS_LIMITS: [u64; 3] = [16_384, 32_768, 65_535];

/// Stream events counted since the process started, summed over every stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamEvents {
    /// Stream-started events.
    pub started: u32,
    /// Non-fatal stop events.
    pub stopped: u32,
    /// Fatal-error events plus stop events with `is_fatal_error`.
    pub fatal: u32,
    /// The reason of the last stop event, 0 when there was none.
    pub last_stop_reason: u32,
    /// Process-info events that are warnings.
    pub warnings: u64,
    /// Process-info events that are not warnings.
    pub errors: u64,
    /// Process-info events with reason [`REASON_INSUFFICIENT_BUFFER`], warning or error.
    pub insufficient_buffer: u64,
    /// The `PacketLength` of those events by class: up to 16384, up to 32768, up to 65535, larger.
    pub insufficient_buffer_len_classes: [u64; 4],
    /// The largest `PacketLength` seen with reason [`REASON_INSUFFICIENT_BUFFER`].
    pub insufficient_buffer_max_len: u64,
    /// Process-info events with any other reason.
    pub other_reason_events: u64,
    /// The last such reason, 0 when there was none.
    pub other_reason_last: u32,
    /// The truncation size the driver reported in the last stream-started event, 0 when none.
    pub reported_truncation_size: u16,
    /// The buffer size in bytes the driver reported in the last stream-started event, 0 when none.
    pub reported_buffer_bytes: u64,
}

/// How many distinct drop reasons are counted one by one.
pub const DROP_REASON_SLOTS: usize = 8;

/// Packets the driver reported as dropped, from the metadata that comes with every packet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DropEvents {
    /// Events whose `drop_reason` is not 0.
    pub events: u64,
    /// `(reason, count)`. The first distinct reasons claim a slot in the order they arrive, and an
    /// empty slot is `(0, 0)`.
    pub reasons: [(u32, u64); DROP_REASON_SLOTS],
    /// Events whose reason found no free slot.
    pub other: u64,
    /// The `drop_location` of the last dropped event, 0 when there was none.
    pub last_location: u32,
}

/// The counters behind [`StreamEvents`] and [`DropEvents`].
///
/// A snapshot reads each counter on its own, so it is not one instant across all of them.
#[derive(Debug)]
pub(crate) struct StreamEventLog {
    started: AtomicU32,
    stopped: AtomicU32,
    fatal: AtomicU32,
    last_stop_reason: AtomicU32,
    warnings: AtomicU64,
    errors: AtomicU64,
    insufficient_buffer: AtomicU64,
    insufficient_buffer_len_classes: [AtomicU64; 4],
    insufficient_buffer_max_len: AtomicU64,
    other_reason_events: AtomicU64,
    other_reason_last: AtomicU32,
    reported_truncation_size: AtomicU16,
    reported_buffer_bytes: AtomicU64,
    drop_events: AtomicU64,
    drop_reasons: [AtomicU32; DROP_REASON_SLOTS],
    drop_reason_counts: [AtomicU64; DROP_REASON_SLOTS],
    drop_other: AtomicU64,
    drop_last_location: AtomicU32,
}

impl StreamEventLog {
    pub(crate) const fn new() -> Self {
        Self {
            started: AtomicU32::new(0),
            stopped: AtomicU32::new(0),
            fatal: AtomicU32::new(0),
            last_stop_reason: AtomicU32::new(0),
            warnings: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            insufficient_buffer: AtomicU64::new(0),
            insufficient_buffer_len_classes: [const { AtomicU64::new(0) }; 4],
            insufficient_buffer_max_len: AtomicU64::new(0),
            other_reason_events: AtomicU64::new(0),
            other_reason_last: AtomicU32::new(0),
            reported_truncation_size: AtomicU16::new(0),
            reported_buffer_bytes: AtomicU64::new(0),
            drop_events: AtomicU64::new(0),
            drop_reasons: [const { AtomicU32::new(0) }; DROP_REASON_SLOTS],
            drop_reason_counts: [const { AtomicU64::new(0) }; DROP_REASON_SLOTS],
            drop_other: AtomicU64::new(0),
            drop_last_location: AtomicU32::new(0),
        }
    }

    pub(crate) fn record_started(&self, truncation_size: u16, buffer_bytes: u64) {
        self.started.fetch_add(1, Ordering::Relaxed);
        self.reported_truncation_size
            .store(truncation_size, Ordering::Relaxed);
        self.reported_buffer_bytes
            .store(buffer_bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_stopped(&self, is_fatal: bool, reason: u32) {
        let counter = if is_fatal { &self.fatal } else { &self.stopped };
        counter.fetch_add(1, Ordering::Relaxed);
        self.last_stop_reason.store(reason, Ordering::Relaxed);
    }

    pub(crate) fn record_fatal(&self) {
        self.fatal.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_process_info(&self, is_warning: bool, reason: u32, packet_length: u64) {
        let counter = if is_warning {
            &self.warnings
        } else {
            &self.errors
        };
        counter.fetch_add(1, Ordering::Relaxed);

        if reason == REASON_INSUFFICIENT_BUFFER {
            self.insufficient_buffer.fetch_add(1, Ordering::Relaxed);
            self.insufficient_buffer_len_classes[len_class(packet_length)]
                .fetch_add(1, Ordering::Relaxed);
            self.insufficient_buffer_max_len
                .fetch_max(packet_length, Ordering::Relaxed);
        } else {
            self.other_reason_events.fetch_add(1, Ordering::Relaxed);
            self.other_reason_last.store(reason, Ordering::Relaxed);
        }
    }

    /// Counts a packet's drop reason. A reason of 0 means the packet was not dropped.
    pub(crate) fn record_drop(&self, reason: u32, location: u32) {
        if reason == 0 {
            return;
        }
        self.drop_events.fetch_add(1, Ordering::Relaxed);
        self.drop_last_location.store(location, Ordering::Relaxed);
        match self.slot_for(reason) {
            Some(slot) => self.drop_reason_counts[slot].fetch_add(1, Ordering::Relaxed),
            None => self.drop_other.fetch_add(1, Ordering::Relaxed),
        };
    }

    /// The slot that counts `reason`, claimed from the first empty one when no slot has it yet.
    /// `None` when every slot belongs to another reason.
    fn slot_for(&self, reason: u32) -> Option<usize> {
        for (slot, claimed) in self.drop_reasons.iter().enumerate() {
            let mut owner = claimed.load(Ordering::Relaxed);
            if owner == 0 {
                owner =
                    match claimed.compare_exchange(0, reason, Ordering::Relaxed, Ordering::Relaxed)
                    {
                        Ok(_) => reason,
                        Err(other) => other,
                    };
            }
            if owner == reason {
                return Some(slot);
            }
        }
        None
    }

    pub(crate) fn snapshot_drops(&self) -> DropEvents {
        DropEvents {
            events: self.drop_events.load(Ordering::Relaxed),
            reasons: std::array::from_fn(|slot| {
                (
                    self.drop_reasons[slot].load(Ordering::Relaxed),
                    self.drop_reason_counts[slot].load(Ordering::Relaxed),
                )
            }),
            other: self.drop_other.load(Ordering::Relaxed),
            last_location: self.drop_last_location.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn snapshot(&self) -> StreamEvents {
        StreamEvents {
            started: self.started.load(Ordering::Relaxed),
            stopped: self.stopped.load(Ordering::Relaxed),
            fatal: self.fatal.load(Ordering::Relaxed),
            last_stop_reason: self.last_stop_reason.load(Ordering::Relaxed),
            warnings: self.warnings.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            insufficient_buffer: self.insufficient_buffer.load(Ordering::Relaxed),
            insufficient_buffer_len_classes: std::array::from_fn(|class| {
                self.insufficient_buffer_len_classes[class].load(Ordering::Relaxed)
            }),
            insufficient_buffer_max_len: self.insufficient_buffer_max_len.load(Ordering::Relaxed),
            other_reason_events: self.other_reason_events.load(Ordering::Relaxed),
            other_reason_last: self.other_reason_last.load(Ordering::Relaxed),
            reported_truncation_size: self.reported_truncation_size.load(Ordering::Relaxed),
            reported_buffer_bytes: self.reported_buffer_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Which of the four length classes a `PacketLength` falls in.
fn len_class(packet_length: u64) -> usize {
    LEN_CLASS_LIMITS
        .iter()
        .position(|limit| packet_length <= *limit)
        .unwrap_or(LEN_CLASS_LIMITS.len())
}

pub(crate) static EVENTS: StreamEventLog = StreamEventLog::new();

/// The events the real-time streams reported, summed over every capture since the process started.
/// The counts only grow, so a caller that wants one capture's share keeps the value from when that
/// capture began and subtracts it. The legacy backend does not report them.
pub fn stream_events() -> StreamEvents {
    EVENTS.snapshot()
}

/// The packets the real-time streams reported as dropped, summed over every capture since the
/// process started. The counts only grow, like those of [`stream_events()`]. The legacy backend
/// does not report them.
pub fn drop_events() -> DropEvents {
    EVENTS.snapshot_drops()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_reported_until_the_driver_says_something() {
        assert_eq!(StreamEventLog::new().snapshot(), StreamEvents::default());
    }

    #[test]
    fn insufficient_buffer_lands_in_the_class_of_its_packet_length() {
        let log = StreamEventLog::new();
        for length in [16_384, 16_385, 32_768, 32_769, 65_535, 65_536, u64::MAX] {
            log.record_process_info(false, REASON_INSUFFICIENT_BUFFER, length);
        }
        let events = log.snapshot();
        assert_eq!(events.insufficient_buffer, 7);
        assert_eq!(events.insufficient_buffer_len_classes, [1, 2, 2, 2]);
        assert_eq!(events.insufficient_buffer_max_len, u64::MAX);
    }

    #[test]
    fn the_largest_packet_length_is_kept_whatever_order_they_arrive_in() {
        let log = StreamEventLog::new();
        for length in [9_001, 60_000, 12_000] {
            log.record_process_info(true, REASON_INSUFFICIENT_BUFFER, length);
        }
        assert_eq!(log.snapshot().insufficient_buffer_max_len, 60_000);
    }

    #[test]
    fn insufficient_buffer_counts_whether_it_is_a_warning_or_an_error() {
        let log = StreamEventLog::new();
        log.record_process_info(true, REASON_INSUFFICIENT_BUFFER, 100);
        log.record_process_info(false, REASON_INSUFFICIENT_BUFFER, 100);
        let events = log.snapshot();
        assert_eq!((events.warnings, events.errors), (1, 1));
        assert_eq!(events.insufficient_buffer, 2);
        assert_eq!(events.other_reason_events, 0);
    }

    #[test]
    fn other_reasons_are_counted_apart_and_leave_the_insufficient_buffer_numbers_alone() {
        let log = StreamEventLog::new();
        log.record_process_info(true, 5, 70_000);
        log.record_process_info(false, 87, 70_000);
        let events = log.snapshot();
        assert_eq!(events.other_reason_events, 2);
        assert_eq!(events.other_reason_last, 87);
        assert_eq!(events.insufficient_buffer, 0);
        assert_eq!(events.insufficient_buffer_len_classes, [0; 4]);
        assert_eq!(events.insufficient_buffer_max_len, 0);
    }

    #[test]
    fn warnings_and_errors_are_split_by_the_flag() {
        let log = StreamEventLog::new();
        for _ in 0..3 {
            log.record_process_info(true, 1, 0);
        }
        log.record_process_info(false, 1, 0);
        let events = log.snapshot();
        assert_eq!((events.warnings, events.errors), (3, 1));
    }

    #[test]
    fn a_later_start_overwrites_the_reported_sizes_and_counts_itself() {
        let log = StreamEventLog::new();
        log.record_started(9000, 2_621_440);
        log.record_started(1500, 262_144);
        let events = log.snapshot();
        assert_eq!(events.started, 2);
        assert_eq!(events.reported_truncation_size, 1500);
        assert_eq!(events.reported_buffer_bytes, 262_144);
    }

    #[test]
    fn fatal_counts_fatal_events_and_fatal_stops_but_not_plain_stops() {
        let log = StreamEventLog::new();
        log.record_fatal();
        log.record_stopped(true, 31);
        log.record_stopped(false, 0);
        log.record_stopped(false, 995);
        let events = log.snapshot();
        assert_eq!(events.fatal, 2);
        assert_eq!(events.stopped, 2);
        assert_eq!(events.last_stop_reason, 995);
    }

    #[test]
    fn a_stop_with_reason_zero_clears_the_last_stop_reason() {
        let log = StreamEventLog::new();
        log.record_stopped(true, 31);
        log.record_stopped(false, 0);
        assert_eq!(log.snapshot().last_stop_reason, 0);
    }

    #[test]
    fn a_packet_that_was_not_dropped_is_not_counted() {
        let log = StreamEventLog::new();
        log.record_drop(0, 77);
        assert_eq!(log.snapshot_drops(), DropEvents::default());
    }

    #[test]
    fn drop_reasons_claim_slots_in_the_order_they_first_arrive() {
        let log = StreamEventLog::new();
        for (reason, location) in [(40, 1), (7, 2), (40, 3), (99, 4), (7, 5), (40, 6)] {
            log.record_drop(reason, location);
        }
        let drops = log.snapshot_drops();
        assert_eq!(drops.events, 6);
        assert_eq!(&drops.reasons[..4], [(40, 3), (7, 2), (99, 1), (0, 0)]);
        assert_eq!(drops.other, 0);
        assert_eq!(drops.last_location, 6);
    }

    #[test]
    fn a_reason_with_no_free_slot_is_counted_in_other() {
        let log = StreamEventLog::new();
        for reason in 1..=DROP_REASON_SLOTS as u32 + 2 {
            log.record_drop(reason, reason * 10);
        }
        log.record_drop(1, 5);
        let drops = log.snapshot_drops();
        assert_eq!(drops.events, DROP_REASON_SLOTS as u64 + 3);
        assert_eq!(drops.reasons[0], (1, 2));
        assert_eq!(
            drops.reasons[DROP_REASON_SLOTS - 1],
            (DROP_REASON_SLOTS as u32, 1)
        );
        assert_eq!(drops.other, 2);
        assert_eq!(drops.last_location, 5);
    }

    #[test]
    fn drops_and_stream_events_do_not_touch_each_other() {
        let log = StreamEventLog::new();
        log.record_drop(5, 1);
        assert_eq!(log.snapshot(), StreamEvents::default());
        log.record_process_info(true, 5, 0);
        assert_eq!(log.snapshot_drops().events, 1);
    }

    #[test]
    fn threads_that_race_for_the_same_new_reason_share_one_slot() {
        const THREADS: u64 = 8;
        const PER_THREAD: u64 = 1000;
        let log = StreamEventLog::new();
        let barrier = std::sync::Barrier::new(THREADS as usize);
        std::thread::scope(|scope| {
            for thread in 0..THREADS {
                let (log, barrier) = (&log, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..PER_THREAD {
                        log.record_drop(4242, thread as u32 + 1);
                    }
                });
            }
        });
        let drops = log.snapshot_drops();
        let claimed: Vec<_> = drops
            .reasons
            .iter()
            .filter(|(reason, _)| *reason == 4242)
            .collect();
        assert_eq!(claimed.len(), 1);
        assert_eq!(*claimed[0], (4242, THREADS * PER_THREAD));
        assert_eq!(drops.events, THREADS * PER_THREAD);
        assert_eq!(drops.other, 0);
        assert_eq!(
            drops
                .reasons
                .iter()
                .filter(|(reason, _)| *reason != 0)
                .count(),
            1
        );
    }

    #[test]
    fn threads_that_race_for_different_new_reasons_lose_no_event() {
        const THREADS: u32 = 8;
        const PER_THREAD: u64 = 1000;
        let log = StreamEventLog::new();
        let barrier = std::sync::Barrier::new(THREADS as usize);
        std::thread::scope(|scope| {
            for thread in 0..THREADS {
                let (log, barrier) = (&log, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    for round in 0..PER_THREAD {
                        // Each thread also drops with the reasons of the others, in a shifted order.
                        log.record_drop((thread + round as u32) % (THREADS + 2) + 1, 1);
                    }
                });
            }
        });
        let drops = log.snapshot_drops();
        let counted: u64 = drops.reasons.iter().map(|(_, count)| count).sum::<u64>() + drops.other;
        assert_eq!(drops.events, u64::from(THREADS) * PER_THREAD);
        assert_eq!(counted, drops.events);
        let mut claimed: Vec<_> = drops
            .reasons
            .iter()
            .map(|(reason, _)| *reason)
            .filter(|reason| *reason != 0)
            .collect();
        let claimed_len = claimed.len();
        claimed.sort_unstable();
        claimed.dedup();
        assert_eq!(claimed.len(), claimed_len);
        assert_eq!(claimed_len, DROP_REASON_SLOTS);
    }
}
