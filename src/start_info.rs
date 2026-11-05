//! What the Packet Monitor driver reports when a real-time stream starts.

use std::sync::atomic::{AtomicU64, Ordering};

/// The stream settings the driver reported when it started the real-time stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamStartInfo {
    /// The size in bytes of the packet buffer the driver reserved for the stream.
    pub packet_buffer_size_bytes: u64,
    /// The size in bytes at which the driver cuts a frame.
    pub truncation_size: u16,
}

/// Set once the driver has reported, so that a report of all zeros is not read as "no report".
const REPORTED: u64 = 1 << 63;

/// The latest start report of one stream, packed into a single word so a reader never sees the
/// buffer size of one report next to the truncation size of another.
#[derive(Debug)]
pub(crate) struct StreamStartSlot(AtomicU64);

impl StreamStartSlot {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Keeps what the driver reported with a stream-started event.
    pub(crate) fn record(&self, packet_buffer_size_in_bytes: u32, truncation_size: u16) {
        self.0.store(
            encode(packet_buffer_size_in_bytes, truncation_size),
            Ordering::Release,
        );
    }

    /// The report the driver made, or `None` before it made one.
    pub(crate) fn get(&self) -> Option<StreamStartInfo> {
        decode(self.0.load(Ordering::Acquire))
    }
}

fn encode(packet_buffer_size_in_bytes: u32, truncation_size: u16) -> u64 {
    REPORTED | u64::from(truncation_size) << 32 | u64::from(packet_buffer_size_in_bytes)
}

fn decode(word: u64) -> Option<StreamStartInfo> {
    if word & REPORTED == 0 {
        return None;
    }
    Some(StreamStartInfo {
        packet_buffer_size_bytes: word & u64::from(u32::MAX),
        truncation_size: (word >> 32) as u16,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn before_the_driver_reports_there_is_no_start_info() {
        assert_eq!(StreamStartSlot::new().get(), None);
    }

    #[test]
    fn the_start_report_is_read_back_as_the_driver_made_it() {
        let slot = StreamStartSlot::new();
        slot.record(2_621_440, 9000);
        assert_eq!(
            slot.get(),
            Some(StreamStartInfo {
                packet_buffer_size_bytes: 2_621_440,
                truncation_size: 9000,
            })
        );
    }

    #[test]
    fn the_extreme_values_survive_the_packing() {
        let slot = StreamStartSlot::new();
        slot.record(u32::MAX, u16::MAX);
        assert_eq!(
            slot.get(),
            Some(StreamStartInfo {
                packet_buffer_size_bytes: u64::from(u32::MAX),
                truncation_size: u16::MAX,
            })
        );
    }

    #[test]
    fn a_report_of_zeros_is_still_a_report() {
        let slot = StreamStartSlot::new();
        slot.record(0, 0);
        assert_eq!(
            slot.get(),
            Some(StreamStartInfo {
                packet_buffer_size_bytes: 0,
                truncation_size: 0,
            })
        );
    }

    #[test]
    fn a_later_report_replaces_the_earlier_one() {
        let slot = StreamStartSlot::new();
        slot.record(262_144, 1500);
        slot.record(524_288, 9000);
        assert_eq!(
            slot.get(),
            Some(StreamStartInfo {
                packet_buffer_size_bytes: 524_288,
                truncation_size: 9000,
            })
        );
    }
}
