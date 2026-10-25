//! Size limits for data that arrives from outside the crate.

/// The largest payload one event may carry.
///
/// An IP datagram is at most 65,535 bytes. 256 KiB leaves room for link, VLAN and tunnel headers.
pub(crate) const MAX_LOGGED_PAYLOAD_BYTES: u32 = 256 * 1024;

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
}
