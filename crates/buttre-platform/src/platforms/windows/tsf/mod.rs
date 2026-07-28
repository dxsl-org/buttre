//! buttre Windows TSF Text Service
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/platform_windows_tsf_tests.rs`.
//!
//! # TSF DLL Architecture
//!
//! This module implements Windows Text Services Framework (TSF) for Vietnamese input.
//! The DLL is built from `buttre-platform` crate with `crate-type = ["cdylib", "rlib"]`.
//!
//! ## COM Exports (Required by Windows)
//!
//! The following COM functions are exported and used by Windows:
//! - `DllGetClassObject` - Returns class factory for creating TSF instances
//! - `DllCanUnloadNow` - Indicates if DLL can be safely unloaded
//! - `DllRegisterServer` - Registers TSF service with Windows (called by regsvr32.exe)
//! - `DllUnregisterServer` - Unregisters TSF service
//! - `DllMain` - DLL entry point (process attach/detach)
//!
//! `DllMain` is defined in `com.rs`; the others are defined at the bottom of this file and are ESSENTIAL for TSF to work.
//!
//! ## Build & Installation
//!
//! ```bash
//! # Build only (no admin):
//! .\scripts\build-tsf.ps1
//!
//! # Install and register exactly the way the release MSI does
//! # (Administrator):
//! .\scripts\build-tsf.ps1 -Install
//! ```
//!
//! ## Important Notes
//!
//! 1. **Modules `factory` and `registration` MUST be `pub`** - They are used by COM exports
//! 2. **Do NOT add duplicate COM exports** - They already exist in this file
//! 3. **TSF requires Vietnamese language** - Must be installed in Windows Settings
//! 4. **DLL path detection** - Uses `get_dll_path()` from registration module

#![cfg(windows)]
#![allow(non_snake_case)]
#![allow(unused)] // Suppress warnings for development code

use anyhow::Result;
use windows::core::{ComObjectInner, IUnknown, Interface, BOOL, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, S_FALSE, S_OK};
// use windows::Win32::System::Com::*;

pub mod com;
pub mod factory; // Public for COM exports
pub mod ipc;
// Note: key_event_sink is integrated into text_service module
mod lang_check;
pub mod logging;
pub mod registration; // Public for COM exports
mod text_ops;
pub mod text_service;

use factory::ClassFactory;
use logging::log_debug;
pub use registration::{get_dll_path, register_server, unregister_server};

use buttre_core::Keyboard;
use std::sync::{Arc, Mutex, RwLock};

/// Windows TSF Backend
pub struct TsfBackend {
    _keyboard: Option<Arc<RwLock<Option<Keyboard>>>>,
}

impl TsfBackend {
    pub fn new() -> Result<Self> {
        // Check 1: TSF DLL must be registered
        if !registration::is_tsf_registered() {
            anyhow::bail!("TSF service not registered");
        }

        // Check 2: Ensure we can get DLL path (sanity check)
        get_dll_path()?;

        // Check 3: the user must have ADDED buttre as an input method.
        //
        // Registration only makes it available in Windows' picker. Until it is
        // selected there, no application ever activates the text service, so
        // choosing this backend would leave the user unable to type at all —
        // and choosing Hook while it IS selected means both layers run, with
        // the hook winning because it sees keys first.
        if !lang_check::is_buttre_text_service_enabled() {
            anyhow::bail!(
                "buttre is registered but not added as a Windows input method. \
                 Add it under Settings > Time & language > Language & region > \
                 (language) > Keyboards, then restart buttre."
            );
        }

        Ok(Self { _keyboard: None })
    }

    pub fn init(&mut self, keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
        logging::init_logging();
        self._keyboard = Some(keyboard);
        // TSF DLL runs in its own process, so "init" here might handle IPC setup
        // which is already started in main.rs (pipe_server).
        Ok(())
    }

    pub fn set_enabled(&mut self, _enabled: bool) {
        // TSF status is usually controlled by the OS or our IPC broker
    }

    pub fn cleanup(&mut self) {
        // Global cleanup
    }
}

// ============================================================================
// COM EXPORTS - Required by Windows for TSF to work
// ============================================================================
// DO NOT REMOVE OR DUPLICATE these exports!
// They are called by Windows (regsvr32.exe, COM runtime, etc.)
// ============================================================================

// CLSID for buttre Text Service
// NOTE: This GUID must match the one in registration.rs
pub const CLSID_BUTTRE_TEXT_SERVICE: GUID = GUID::from_u128(0xE6B8A6C0_1234_5678_9ABC_DEF012345678);

/// DllGetClassObject - Returns class factory for creating TSF instances
/// Called by Windows COM runtime when an application needs to create our TSF service
/// DO NOT REMOVE - Required for COM registration
///
/// # Safety
///
/// Called only by the Windows COM runtime via `GetProcAddress`/`LoadLibrary`
/// (never from Rust code — see `buttre_platform.def`). `riid` and `ppv` must
/// be valid pointers for the duration of the call, which the COM runtime
/// guarantees; both are checked and dereferenced only inside the `unsafe`
/// block below.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    _rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut core::ffi::c_void,
) -> HRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let factory: IUnknown = ClassFactory::new().into_object().into_interface();
        // SAFETY:
        // 1. factory is a valid IUnknown COM interface created above
        // 2. riid points to valid GUID provided by COM runtime
        // 3. ppv is a valid output pointer provided by COM runtime
        // 4. query() is a COM method that safely handles the interface query
        // 5. If successful, writes interface pointer to ppv and increments refcount
        unsafe { factory.query(riid, ppv) }
    }));
    result.unwrap_or(E_FAIL)
}

/// DllCanUnloadNow - Check if DLL can be safely unloaded from memory
/// Called by Windows COM runtime to determine if the DLL is still in use
/// DO NOT REMOVE - Required for COM lifecycle management
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    // Trivial atomic load — panic is implausible, but keep the FFI rule consistent.
    let result = std::panic::catch_unwind(|| if com::dll_can_unload() { S_OK } else { S_FALSE });
    result.unwrap_or(S_FALSE) // On panic: claim "in use" to avoid use-after-free
}

/// DllRegisterServer - Register the TSF service with Windows
/// Called by regsvr32.exe during installation
/// DO NOT REMOVE - Required for TSF registration
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        #[cfg(debug_assertions)]
        log_debug("DllRegisterServer called");

        match get_dll_path() {
            Ok(dll_path) => match register_server(&dll_path) {
                Ok(()) => {
                    #[cfg(debug_assertions)]
                    log_debug(&format!("Registered: {:?}", dll_path));
                    S_OK
                }
                Err(e) => {
                    #[cfg(debug_assertions)]
                    log_debug(&format!("Registration failed: {:?}", e));
                    E_FAIL
                }
            },
            Err(e) => {
                #[cfg(debug_assertions)]
                log_debug(&format!("Failed to get DLL path: {:?}", e));
                E_FAIL
            }
        }
    }));
    result.unwrap_or(E_FAIL)
}

/// DllUnregisterServer - Unregister the TSF service from Windows
/// Called by regsvr32.exe /u during uninstallation
/// Removes all registry entries created by DllRegisterServer
/// DO NOT REMOVE - Required for TSF unregistration
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        #[cfg(debug_assertions)]
        log_debug("DllUnregisterServer called");

        match unregister_server() {
            Ok(()) => S_OK,
            Err(e) => {
                #[cfg(debug_assertions)]
                log_debug(&format!("Unregistration failed: {:?}", e));
                E_FAIL
            }
        }
    }));
    result.unwrap_or(E_FAIL)
}
