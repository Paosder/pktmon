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

/// The counters behind [`StreamEvents`].
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
}
