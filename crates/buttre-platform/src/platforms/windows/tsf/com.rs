//! COM helper utilities

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::BOOL;
use windows::Win32::Foundation::HINSTANCE;

use super::logging::{init_logging, log_debug};

/// Global DLL reference count
static DLL_REF_COUNT: AtomicU32 = AtomicU32::new(0);

/// Increment DLL reference count
pub fn dll_add_ref() {
    DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
}

/// Decrement DLL reference count
pub fn dll_release() {
    DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
}

/// Get current DLL reference count
pub fn dll_get_ref_count() -> u32 {
    DLL_REF_COUNT.load(Ordering::SeqCst)
}

/// Check if DLL can be unloaded
pub fn dll_can_unload() -> bool {
    dll_get_ref_count() == 0
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    _hinst_dll: HINSTANCE,
    fdw_reason: u32,
    _lpv_reserved: *const core::ffi::c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    // Deliberately does almost nothing. `DllMain` runs under the loader lock,
    // where creating directories, opening files or installing a global
    // subscriber can deadlock the host application — and this DLL loads into
    // EVERY application that uses TSF. Logging is initialized from
    // `TextService::Activate` instead (see `logging::init_logging`), which
    // runs as an ordinary COM call with no such constraint.
    //
    // A panic crossing an FFI boundary is undefined behaviour, so the little
    // that happens here is still caught.
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match fdw_reason {
        DLL_PROCESS_ATTACH | DLL_PROCESS_DETACH => {}
        _ => {}
    }));

    BOOL(ok.is_ok() as i32)
}
