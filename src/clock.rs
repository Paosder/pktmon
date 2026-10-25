//! The performance counter, which stamps ETW events when the session asks for QPC time.

use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::delay::{record_delivery_delay, ticks_to_us};

static FREQUENCY: AtomicU64 = AtomicU64::new(0);

fn frequency() -> u64 {
    let cached = FREQUENCY.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    let mut ticks_per_second = 0i64;
    // SAFETY: the pointer is to a live local.
    let queried = unsafe { QueryPerformanceFrequency(&mut ticks_per_second) }.as_bool();
    let frequency = if queried { u64::try_from(ticks_per_second).unwrap_or(0) } else { 0 };
    FREQUENCY.store(frequency, Ordering::Relaxed);
    frequency
}

/// Counts how long ago an event stamped with `event_timestamp` was written. Runs inside the ETW
/// callback, so it only reads two clocks and bumps atomics.
pub(crate) fn record_event_age(event_timestamp: i64) {
    let frequency = frequency();
    if frequency == 0 {
        return;
    }
    let mut now = 0i64;
    // SAFETY: the pointer is to a live local.
    if !unsafe { QueryPerformanceCounter(&mut now) }.as_bool() {
        return;
    }
    record_delivery_delay(ticks_to_us(now.saturating_sub(event_timestamp), frequency));
}
