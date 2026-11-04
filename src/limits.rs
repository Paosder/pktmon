//! Size limits for data that arrives from outside the crate.

/// The largest payload one event may carry.
///
/// An IP datagram is at most 65,535 bytes. 256 KiB leaves room for link, VLAN and tunnel headers.
pub(crate) const MAX_LOGGED_PAYLOAD_BYTES: u32 = 256 * 1024;

/// The smallest real-time stream buffer multiplier the driver accepts.
pub(crate) const MIN_BUFFER_SIZE_MULTIPLIER: u16 = 1;

/// The largest real-time stream buffer multiplier the driver accepts.
///
/// Microsoft documents the stream buffer as 256 KiB times a multiplier from 1 to 10, so the most
/// the driver can hold for the reader is 2.5 MiB.
pub(crate) const MAX_BUFFER_SIZE_MULTIPLIER: u16 = 10;

/// The multiplier the stream is created with: the requested one, brought into the range the driver
/// accepts. A value outside it makes the stream fail to open, and the capture would then fall back
/// to the legacy backend without saying why.
pub(crate) fn buffer_size_multiplier_in_range(requested: u16) -> u16 {
    requested.clamp(MIN_BUFFER_SIZE_MULTIPLIER, MAX_BUFFER_SIZE_MULTIPLIER)
}

/// The payload length an event reports, or `None` when it claims more than a packet can hold.
///
/// The consumer allocates a buffer of this size inside the ETW callback. An absurd length from a
/// damaged or forged event must be skipped, because a failed allocation there ends the process.
pub(crate) fn logged_payload_len(reported: u32) -> Option<usize> {
    if reported <= MAX_LOGGED_PAYLOAD_BYTES {
        usize::try_from(reported).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_up_to_the_limit_pass_through() {
        assert_eq!(logged_payload_len(0), Some(0));
        assert_eq!(logged_payload_len(128), Some(128));
        assert_eq!(logged_payload_len(9000), Some(9000));
        assert_eq!(logged_payload_len(65_535 + 18), Some(65_553));
        assert_eq!(logged_payload_len(MAX_LOGGED_PAYLOAD_BYTES), Some(262_144));
    }

    #[test]
    fn lengths_above_the_limit_are_refused() {
        assert_eq!(logged_payload_len(MAX_LOGGED_PAYLOAD_BYTES + 1), None);
        assert_eq!(logged_payload_len(1 << 30), None);
        assert_eq!(logged_payload_len(u32::MAX), None);
    }

    #[test]
    fn multipliers_the_driver_accepts_pass_through() {
        for multiplier in MIN_BUFFER_SIZE_MULTIPLIER..=MAX_BUFFER_SIZE_MULTIPLIER {
            assert_eq!(buffer_size_multiplier_in_range(multiplier), multiplier);
        }
    }

    #[test]
    fn multipliers_outside_the_driver_range_are_brought_into_it() {
        assert_eq!(buffer_size_multiplier_in_range(0), 1);
        assert_eq!(buffer_size_multiplier_in_range(11), 10);
        assert_eq!(buffer_size_multiplier_in_range(u16::MAX), 10);
    }
}
