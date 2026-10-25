//! The one place the crate loads `PktMonApi.dll`.

use windows::{
    Win32::{
        Foundation::{HANDLE, HMODULE},
        System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExA},
    },
    core::{self as win, s},
};

/// Loads `PktMonApi.dll` from the Windows system folder only.
///
/// `LoadLibraryA` with a bare name searches the application folder before the system folder, so a
/// copy planted next to the executable would be loaded instead of the Windows one. A module of
/// this name that is already loaded is returned as it is, whatever folder it came from.
pub(crate) fn load_pktmon_api() -> win::Result<HMODULE> {
    // SAFETY: the name is a NUL-terminated literal and no file handle is passed.
    unsafe { LoadLibraryExA(s!("PktMonApi.dll"), HANDLE(0), LOAD_LIBRARY_SEARCH_SYSTEM32) }
}
