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

/// The smallest real-time stream truncation size, in bytes.
///
/// Anything shorter would cut a frame inside its Ethernet and IP headers.
pub(crate) const MIN_TRUNCATION_SIZE: u16 = 64;

/// The largest truncation size Microsoft documents for `PacketMonitorCreateRealtimeStream`.
///
/// The driver's behavior above it is unverified. It may refuse to create the stream, accept the
/// value, or cut frames at this size anyway.
pub(crate) const DOCUMENTED_MAX_TRUNCATION_SIZE: u16 = 9000;

/// The truncation size the stream is requested with: the requested one, raised to the minimum.
/// There is no upper cap, so that a caller can ask for a size large enough to receive a
/// receive-coalesced frame of up to 64 KB whole. A size above
/// [`DOCUMENTED_MAX_TRUNCATION_SIZE`] is subject to [`open_with_truncation_fallback`].
pub(crate) fn truncation_size_in_range(requested: u16) -> u16 {
    requested.clamp(MIN_TRUNCATION_SIZE, u16::MAX)
}

/// Opens something with the requested truncation size, and once with the documented maximum when
/// a request above it fails, so that asking for more never loses what asking for less would have
/// opened. Returns what `open` produced together with the truncation size it was opened with.
///
/// The error of the last attempt is returned when every attempt fails.
pub(crate) fn open_with_truncation_fallback<T, E: std::fmt::Debug>(
    requested: u16,
    mut open: impl FnMut(u16) -> Result<T, E>,
) -> Result<(T, u16), E> {
    let requested = truncation_size_in_range(requested);
    match open(requested) {
        Ok(opened) => Ok((opened, requested)),
        Err(error) if requested > DOCUMENTED_MAX_TRUNCATION_SIZE => {
            log::warn!(
                "Opening the stream with truncation size {requested} failed ({error:?}), \
                 retrying with {DOCUMENTED_MAX_TRUNCATION_SIZE}"
            );
            open(DOCUMENTED_MAX_TRUNCATION_SIZE)
                .map(|opened| (opened, DOCUMENTED_MAX_TRUNCATION_SIZE))
        }
        Err(error) => Err(error),
    }
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

    #[test]
    fn truncation_sizes_are_raised_to_the_minimum_and_never_capped() {
        assert_eq!(truncation_size_in_range(0), 64);
        assert_eq!(truncation_size_in_range(63), 64);
        assert_eq!(truncation_size_in_range(MIN_TRUNCATION_SIZE), 64);
        assert_eq!(truncation_size_in_range(1500), 1500);
        assert_eq!(truncation_size_in_range(9000), 9000);
        assert_eq!(truncation_size_in_range(9001), 9001);
        assert_eq!(truncation_size_in_range(65535), 65535);
    }

    #[test]
    fn a_request_the_driver_accepts_is_opened_once() {
        let mut asked = Vec::new();
        let opened = open_with_truncation_fallback(65535, |size| {
            asked.push(size);
            Ok::<_, ()>("stream")
        });
        assert_eq!(opened, Ok(("stream", 65535)));
        assert_eq!(asked, [65535]);
    }

    #[test]
    fn a_refused_request_above_the_documented_maximum_is_retried_at_it() {
        let mut asked = Vec::new();
        let opened = open_with_truncation_fallback(65535, |size| {
            asked.push(size);
            if size > 9000 {
                Err("refused")
            } else {
                Ok("stream")
            }
        });
        assert_eq!(opened, Ok(("stream", 9000)));
        assert_eq!(asked, [65535, 9000]);
    }

    #[test]
    fn when_the_retry_fails_too_the_error_is_returned_for_the_legacy_fallback() {
        let mut asked = Vec::new();
        let opened: Result<((), u16), _> = open_with_truncation_fallback(20_000, |size| {
            asked.push(size);
            Err(size)
        });
        assert_eq!(opened, Err(9000));
        assert_eq!(asked, [20_000, 9000]);
    }

    #[test]
    fn a_request_at_the_documented_maximum_or_below_is_never_retried() {
        for requested in [0, 64, 1500, 9000] {
            let mut asked = 0;
            let opened: Result<((), u16), _> = open_with_truncation_fallback(requested, |_| {
                asked += 1;
                Err("refused")
            });
            assert_eq!(opened, Err("refused"));
            assert_eq!(asked, 1);
        }
    }
}
