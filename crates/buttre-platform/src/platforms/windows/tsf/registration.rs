//! Registration Module
//!
//! Handles COM server and TSF service registration

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

// GUIDs for buttre TSF
pub const CLSID_BUTTRE_TEXT_SERVICE: &str = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}";
// Must match the LanguageProfile GUID in installers/windows/product.wxs —
// MSI and runtime registration write the same profile or uninstall orphans one.
pub const GUID_PROFILE: &str = "{B7447743-7652-4AB6-8D82-250D935EBCC0}";

// Language IDs
const LANGID_VIETNAMESE: u32 = 0x042A; // Vietnamese (0x042A)
const LANGID_ENGLISH_US: u32 = 0x0409; // English (US) (0x0409)

/// Check if TSF service is registered
pub fn is_tsf_registered() -> bool {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let clsid_path = format!("SOFTWARE\\Classes\\CLSID\\{}", CLSID_BUTTRE_TEXT_SERVICE);
    hklm.open_subkey(&clsid_path).is_ok()
}

/// Register COM server
pub fn register_com_server(dll_path: &Path) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let clsid_path = format!("SOFTWARE\\Classes\\CLSID\\{}", CLSID_BUTTRE_TEXT_SERVICE);

    let (clsid_key, _) = hklm
        .create_subkey(&clsid_path)
        .context("Failed to create CLSID key")?;

    clsid_key
        .set_value("", &"buttre Vietnamese Input")
        .context("Failed to set CLSID description")?;

    // InprocServer32
    let (inproc_key, _) = clsid_key
        .create_subkey("InprocServer32")
        .context("Failed to create InprocServer32 key")?;

    let dll_path_str = dll_path.to_string_lossy().to_string();
    inproc_key
        .set_value("", &dll_path_str)
        .context("Failed to set DLL path")?;

    inproc_key
        .set_value("ThreadingModel", &"Apartment")
        .context("Failed to set threading model")?;

    Ok(())
}

/// Unregister COM server
pub fn unregister_com_server() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let clsid_path = format!("SOFTWARE\\Classes\\CLSID\\{}", CLSID_BUTTRE_TEXT_SERVICE);

    match hklm.delete_subkey_all(&clsid_path) {
        Ok(_) => Ok(()),
        Err(_) => Ok(()), // Key might not exist, that's ok
    }
}

/// Register TSF service for a specific language
fn register_tsf_language_profile(tip_key: &RegKey, dll_path: &Path, langid: u32) -> Result<()> {
    let profile_path = format!("LanguageProfile\\0x{:08X}\\{}", langid, GUID_PROFILE);
    let (profile_key, _) = tip_key
        .create_subkey(&profile_path)
        .context("Failed to create language profile key")?;

    profile_key
        .set_value("Description", &"buttre - Vietnamese Input")
        .context("Failed to set description")?;

    let dll_path_str = dll_path.to_string_lossy().to_string();
    profile_key
        .set_value("IconFile", &dll_path_str)
        .context("Failed to set icon file")?;

    profile_key
        .set_value("IconIndex", &0u32)
        .context("Failed to set icon index")?;

    // CRITICAL: Enable the profile so Windows will load the DLL
    #[cfg(debug_assertions)]
    eprintln!("Setting Enable flag for language 0x{:08X}", langid);

    profile_key
        .set_value("Enable", &1u32)
        .context("Failed to set Enable flag")?;

    #[cfg(debug_assertions)]
    eprintln!("Enable flag set successfully!");

    Ok(())
}

/// Language IDs to register the text service under, given the machine's
/// default UI language.
///
/// Vietnamese is unconditional. This is a Vietnamese IME: a user who adds a
/// keyboard does it under the Vietnamese language, and a profile that is not
/// registered there simply cannot be picked — no error, the entry is just
/// absent. Deriving the list from `GetUserDefaultLangID` alone (the previous
/// behavior) meant that on an English Windows — the common case for a
/// developer machine, and plenty of user machines — the IME registered under
/// en-US only and was unreachable from the Vietnamese language it exists to
/// serve. `LANGID_VIETNAMESE` was declared for this and never used.
///
/// English (US) stays in the list so the service is also reachable on a
/// machine with no Vietnamese language installed yet, and the user's own
/// default is kept for the same reason.
fn language_ids_for(user_default: u32) -> Vec<u32> {
    let mut languages = vec![LANGID_VIETNAMESE, LANGID_ENGLISH_US, user_default];
    languages.sort_unstable();
    languages.dedup();
    languages
}

/// [`language_ids_for`] against this machine's default UI language.
fn get_installed_languages() -> Vec<u32> {
    use windows::Win32::Globalization::GetUserDefaultLangID;

    // SAFETY: GetUserDefaultLangID takes no arguments, cannot fail, and has no
    // side effects — it returns the current user's default UI language id.
    let user_lang = unsafe { GetUserDefaultLangID() } as u32;
    language_ids_for(user_lang)
}

/// Register TSF service for installed languages only
pub fn register_tsf_service(dll_path: &Path) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let tip_path = format!(
        "SOFTWARE\\Microsoft\\CTF\\TIP\\{}",
        CLSID_BUTTRE_TEXT_SERVICE
    );

    let (tip_key, _) = hklm
        .create_subkey(&tip_path)
        .context("Failed to create TIP key")?;

    // Get installed languages (only those active on system)
    let languages = get_installed_languages();

    // Register for each supported language
    for &langid in &languages {
        register_tsf_language_profile(&tip_key, dll_path, langid).with_context(|| {
            format!(
                "Failed to register language profile for LANGID 0x{:08X}",
                langid
            )
        })?;
    }

    Ok(())
}

/// Unregister TSF service
pub fn unregister_tsf_service() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let tip_path = format!(
        "SOFTWARE\\Microsoft\\CTF\\TIP\\{}",
        CLSID_BUTTRE_TEXT_SERVICE
    );

    match hklm.delete_subkey_all(&tip_path) {
        Ok(_) => Ok(()),
        Err(_) => Ok(()), // Key might not exist, that's ok
    }
}

// Register Categories using ITfCategoryMgr
fn register_categories() -> Result<()> {
    use windows::core::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::TextServices::{
        CLSID_TF_CategoryMgr, ITfCategoryMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_TIP_KEYBOARD,
    };

    // Define GUID manually to match CLSID_BUTTRE_TEXT_SERVICE string
    const CLSID_BUTTRE: GUID = GUID {
        data1: 0xE6B8A6C0,
        data2: 0x1234,
        data3: 0x5678,
        data4: [0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34, 0x56, 0x78],
    };

    // SAFETY:
    // 1. CoCreateInstance is properly declared in windows crate
    // 2. CLSID_TF_CategoryMgr is a valid Windows CLSID constant
    // 3. CLSCTX_INPROC_SERVER is a valid COM context flag
    // 4. RegisterCategory is a COM method - safe to call on valid interface
    // 5. CLSID_BUTTRE and GUID_TFCAT_* are valid GUID constants
    // 6. All COM methods use proper error handling with ?
    unsafe {
        let cat_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                .context("Failed to create CategoryMgr")?;

        cat_mgr
            .RegisterCategory(&CLSID_BUTTRE, &GUID_TFCAT_TIP_KEYBOARD, &CLSID_BUTTRE)
            .context("Failed to register Keyboard Category")?;

        cat_mgr
            .RegisterCategory(
                &CLSID_BUTTRE,
                &GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
                &CLSID_BUTTRE,
            )
            .context("Failed to register DisplayAttributeProvider Category")?;
    }
    Ok(())
}

fn unregister_categories() -> Result<()> {
    use windows::core::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::TextServices::{
        CLSID_TF_CategoryMgr, ITfCategoryMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_TIP_KEYBOARD,
    };

    const CLSID_BUTTRE: GUID = GUID {
        data1: 0xE6B8A6C0,
        data2: 0x1234,
        data3: 0x5678,
        data4: [0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34, 0x56, 0x78],
    };

    // SAFETY:
    // 1. CoCreateInstance is properly declared in windows crate
    // 2. Same invariants as register_categories above
    // 3. UnregisterCategory is safe even if category doesn't exist
    // 4. Errors are ignored (best-effort cleanup during uninstall)
    unsafe {
        if let Ok(cat_mgr) =
            CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
        {
            let _ =
                cat_mgr.UnregisterCategory(&CLSID_BUTTRE, &GUID_TFCAT_TIP_KEYBOARD, &CLSID_BUTTRE);
            let _ = cat_mgr.UnregisterCategory(
                &CLSID_BUTTRE,
                &GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
                &CLSID_BUTTRE,
            );
        }
    }
    Ok(())
}

/// Register server (called by DllRegisterServer)
pub fn register_server(dll_path: &Path) -> Result<()> {
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    // SAFETY: CoInitializeEx is safe to call; returns S_OK if we initialised COM,
    // S_FALSE if it was already initialised by the caller (no increment), or an
    // error HRESULT if initialisation is impossible.
    let co_hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // Propagate hard failures; S_FALSE (already init) is fine.
    co_hr.ok().context("Failed to initialize COM")?;
    // Only call CoUninitialize if WE incremented the refcount (S_OK, not S_FALSE).
    let we_inited = co_hr == S_OK;

    let result = (|| {
        register_com_server(dll_path)?;
        register_tsf_service(dll_path)?;
        register_categories()?;
        Ok(())
    })();

    // SAFETY: only uninitialize COM if we were the ones who initialized it.
    if we_inited {
        unsafe {
            CoUninitialize();
        }
    }

    result
}

/// Unregister server (called by DllUnregisterServer)
pub fn unregister_server() -> Result<()> {
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    // SAFETY: same reasoning as register_server.
    let co_hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    co_hr.ok().context("Failed to initialize COM")?;
    let we_inited = co_hr == S_OK;

    let result = (|| {
        unregister_categories()?;
        unregister_tsf_service()?;
        unregister_com_server()?;
        Ok(())
    })();

    if we_inited {
        unsafe {
            CoUninitialize();
        }
    }

    result
}

pub fn get_dll_path() -> Result<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };

    // SAFETY:
    // 1. GetModuleHandleExW is properly declared in windows crate
    // 2. func_ptr is a valid function pointer (get_dll_path function address)
    // 3. GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS tells Windows to find module containing func_ptr
    // 4. GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT means don't increment refcount
    // 5. GetModuleFileNameW retrieves the DLL path for hmodule
    // 6. buffer is a valid Vec<u16> with capacity 260 (MAX_PATH)
    // 7. from_utf16_lossy safely converts to String (handles invalid UTF-16)
    unsafe {
        let mut hmodule = HMODULE::default();
        let func_ptr = get_dll_path as *const ();

        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(func_ptr as *const u16),
            &mut hmodule,
        )
        .context("Failed to get module handle")?;

        let mut buffer = vec![0u16; 260];
        let len = GetModuleFileNameW(Some(hmodule), &mut buffer);

        if len == 0 {
            anyhow::bail!("Failed to get module file name");
        }

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Ok(PathBuf::from(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this pins: the IME shipped registered under en-US only on an
    /// English Windows, so it never appeared under the Vietnamese language —
    /// the one place a user goes to add it.
    #[test]
    fn vietnamese_is_registered_whatever_the_system_language_is() {
        for user_default in [
            LANGID_ENGLISH_US,
            0x0411, /* ja-JP */
            0x0409,
            LANGID_VIETNAMESE,
        ] {
            assert!(
                language_ids_for(user_default).contains(&LANGID_VIETNAMESE),
                "vi-VN missing for user default {user_default:#06X}"
            );
        }
    }

    #[test]
    fn english_stays_available_as_a_fallback() {
        assert!(language_ids_for(LANGID_VIETNAMESE).contains(&LANGID_ENGLISH_US));
    }

    /// A duplicate would create the same profile key twice — harmless but it
    /// makes the registry read as if two profiles exist.
    #[test]
    fn the_users_own_language_is_included_exactly_once() {
        let ids = language_ids_for(LANGID_VIETNAMESE);
        assert_eq!(ids.iter().filter(|&&id| id == LANGID_VIETNAMESE).count(), 1);
        assert_eq!(ids, vec![LANGID_ENGLISH_US, LANGID_VIETNAMESE]);
    }
}
