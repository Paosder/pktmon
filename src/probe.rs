//! Memory probe knobs. Diagnostic branch only, never merged.

pub(crate) fn knob(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}
