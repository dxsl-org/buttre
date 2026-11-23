//! Registration Module
//!
//! Handles COM server and TSF service registration

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

use super::profiles::{guid_string, profiles_in, ProfileEntry};

// GUIDs for buttre TSF
pub const CLSID_BUTTRE_TEXT_SERVICE: &str = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}";
// Profile GUIDs live in `profiles.rs` — one per input method.

// Language IDs
const LANGID_VIETNAMESE: u32 = 0x042A; // Vietnamese (0x042A)
const LANGID_ENGLISH_US: u32 = 0x0409; // English (US) (0x0409)

/// Check if TSF service is registered
pub fn is_tsf_registered() -> bool {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let clsid_path = format!("SOFTWARE\\Classes\\CLSID\\{}", CLSID_BUTTRE_TEXT_SERVICE);
    hklm.open_subkey(&clsid_path).is_ok()
}

/// File name of the 32-bit text service, installed next to the 64-bit one.
///
/// TSF loads the text service INTO each application, so a 32-bit application
/// (Zalo is one) can only load a 32-bit DLL — and it finds it through the
/// 32-bit registry view's CLSID. Without it, Windows would not even let the
/// user pick buttre in such an application. The `CTF\TIP` keys (profiles,
/// categories) are shared by both views and need no second copy.
pub const X86_DLL_NAME: &str = "buttre_platform_x86.dll";

const CLSID_ROOT: &str = "SOFTWARE\\Classes\\CLSID";

/// Register the COM server in one registry view: `0` for this process's own,
/// `KEY_WOW64_32KEY` for the 32-bit one.
fn register_com_server(dll_path: &Path, view: u32) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let clsid_path = format!("{CLSID_ROOT}\\{CLSID_BUTTRE_TEXT_SERVICE}");

    let (clsid_key, _) = hklm
        .create_subkey_with_flags(&clsid_path, KEY_ALL_ACCESS | view)
        .context("Failed to create CLSID key")?;

    clsid_key
        .set_value("", &"buttre Vietnamese Input")
        .context("Failed to set CLSID description")?;

    // InprocServer32
    let (inproc_key, _) = clsid_key
        .create_subkey_with_flags("InprocServer32", KEY_ALL_ACCESS | view)
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

/// Remove the COM server from one registry view (see [`register_com_server`]).
/// A key that does not exist is not an error.
fn unregister_com_server(view: u32) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(root) = hklm.open_subkey_with_flags(CLSID_ROOT, KEY_ALL_ACCESS | view) {
        let _ = root.delete_subkey_all(CLSID_BUTTRE_TEXT_SERVICE);
    }
}

/// Register the 64-bit server and, beside it, the 32-bit one for 32-bit
/// applications — or remove a 32-bit registration whose DLL is gone, so no
/// 32-bit application is pointed at a missing file.
///
/// Only a 64-bit build does this: in a 32-bit process the native view IS the
/// 32-bit one, already written by [`register_com_server`].
fn register_com_servers(dll_path: &Path) -> Result<()> {
    register_com_server(dll_path, 0)?;
    if cfg!(target_pointer_width = "64") {
        let x86 = dll_path.with_file_name(X86_DLL_NAME);
        if x86.exists() {
            register_com_server(&x86, KEY_WOW64_32KEY)
                .context("Failed to register the 32-bit text service")?;
        } else {
            unregister_com_server(KEY_WOW64_32KEY);
        }
    }
    Ok(())
}

/// Register every input-method profile under one language, and drop profiles
/// of ours that are no longer offered (a keyboard TOML that was removed).
///
/// Written over the existing keys rather than deleted and recreated: a profile
/// key that disappears, even briefly, makes Windows prune the user's own
/// keyboard choice for it.
fn register_tsf_language_profiles(
    tip_key: &RegKey,
    dll_path: &Path,
    langid: u32,
    profiles: &[ProfileEntry],
) -> Result<()> {
    let language_path = format!("LanguageProfile\\0x{:08X}", langid);
    let (language_key, _) = tip_key
        .create_subkey(&language_path)
        .context("Failed to create language key")?;

    let wanted: Vec<String> = profiles.iter().map(|p| guid_string(&p.guid)).collect();
    prune_stale_profiles(&language_key, &wanted);

    let dll_path_str = dll_path.to_string_lossy().to_string();
    for (profile, guid) in profiles.iter().zip(&wanted) {
        let (profile_key, _) = language_key
            .create_subkey(guid)
            .with_context(|| format!("Failed to create profile key {guid}"))?;
        profile_key
            .set_value("Description", &profile.description)
            .context("Failed to set description")?;
        profile_key
            .set_value("IconFile", &dll_path_str)
            .context("Failed to set icon file")?;
        profile_key
            .set_value("IconIndex", &0u32)
            .context("Failed to set icon index")?;
        // Machine-wide availability; the user still adds the keyboard in
        // Settings (see `lang_check` for why this is not "added").
        profile_key
            .set_value("Enable", &1u32)
            .context("Failed to set Enable flag")?;
    }
    Ok(())
}

/// Delete profile keys under one language that are not in `wanted`. Only
/// GUID-shaped names are touched: anything else is not a profile we wrote.
fn prune_stale_profiles(language_key: &RegKey, wanted: &[String]) {
    let stale: Vec<String> = language_key
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter(|name| is_guid_key(name) && !wanted.iter().any(|w| w.eq_ignore_ascii_case(name)))
        .collect();
    for name in stale {
        match language_key.delete_subkey_all(&name) {
            Ok(()) => tracing::info!("removed stale TSF profile {name}"),
            Err(e) => tracing::warn!("could not remove stale TSF profile {name}: {e}"),
        }
    }
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` — the only key names `LanguageProfile`
/// language keys hold for profiles.
fn is_guid_key(name: &str) -> bool {
    name.len() == 38
        && name.starts_with('{')
        && name.ends_with('}')
        && name[1..37].chars().enumerate().all(|(i, c)| {
            matches!(i, 8 | 13 | 18 | 23) == (c == '-') && (c == '-' || c.is_ascii_hexdigit())
        })
}

/// Language IDs to register the text service under.
///
/// Only languages the machine ACTUALLY HAS. Registering under a language that
/// is not installed produces a keyboard entry the user can see and select but
/// which can never work: Windows resolves it through `Substitutes` to the plain
/// layout, so picking "Vietnamese - buttre" silently types with
/// "Vietnamese - US". That entry cost days of debugging, on both sides of this
/// conversation, and it exists for no one — a Windows user does not care whether
/// the Vietnamese language pack is present, they just want buttre in the list.
///
/// So: Vietnamese when Vietnamese is installed, and the machine's default
/// language always (it is installed by definition). `en-US` is no longer added
/// unconditionally — on an English machine it IS the default, and on a Japanese
/// one an en-US entry was never wanted.
///
/// Both the user and system defaults go in. Under the MSI these are the same
/// thing, and that matters: the deferred custom action runs as SYSTEM
/// (`Impersonate="no"`, required to write HKLM), so "the user's" default there
/// is SYSTEM's. Including the system default keeps the answer sane in that
/// context instead of depending on which account the installer happened to use.
fn language_ids_for(user_default: u32, system_default: u32, vietnamese: bool) -> Vec<u32> {
    let mut languages = vec![user_default, system_default];
    if vietnamese {
        languages.push(LANGID_VIETNAMESE);
    }
    languages.retain(|&langid| langid != 0);
    languages.sort_unstable();
    languages.dedup();
    languages
}

/// Is a Vietnamese language installed for this user?
///
/// Read from the language list Windows itself maintains, matching on the BCP-47
/// prefix so `vi`, `vi-VN` and any future regional variant all count.
///
/// `false` when the list cannot be read — the conservative direction. A missing
/// Vietnamese profile means the user must add the language before buttre appears
/// under it; a spurious one means an entry that looks available and is not.
fn is_vietnamese_installed() -> bool {
    let Ok(profile) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey("Control Panel\\International\\User Profile")
    else {
        return false;
    };
    let Ok(languages) = profile.get_value::<Vec<String>, _>("Languages") else {
        return false;
    };
    languages
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("vi") || tag.to_ascii_lowercase().starts_with("vi-"))
}

/// [`language_ids_for`] against this machine's real language configuration.
fn get_installed_languages() -> Vec<u32> {
    use windows::Win32::Globalization::{GetSystemDefaultLangID, GetUserDefaultLangID};

    // SAFETY: both take no arguments, cannot fail, and have no side effects —
    // they return the current user's and the system's default UI language ids.
    let (user_lang, system_lang) = unsafe {
        (
            GetUserDefaultLangID() as u32,
            GetSystemDefaultLangID() as u32,
        )
    };
    language_ids_for(user_lang, system_lang, is_vietnamese_installed())
}

/// Delete language profiles that are no longer wanted.
///
/// Needed because registration no longer unregisters first — deleting the TIP
/// key made Windows prune the user's own keyboard choice, so the install writes
/// over the existing keys instead. Without this, a profile registered by an
/// older build under a language the machine does not have would survive every
/// upgrade forever, which is precisely the phantom "Vietnamese - buttre" entry
/// that silently typed with the plain layout.
///
/// Only languages absent from `wanted` are removed, and `wanted` contains
/// exactly the installed ones — so this can never delete a profile the user is
/// actually able to use. Failures are logged, not fatal: a leftover profile is a
/// cosmetic wart, while aborting the install over one would be worse.
fn prune_stale_language_profiles(tip_key: &RegKey, wanted: &[u32]) {
    let Ok(profiles) = tip_key.open_subkey_with_flags("LanguageProfile", KEY_ALL_ACCESS) else {
        return;
    };
    let stale: Vec<String> = profiles
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter(|name| match parse_profile_langid(name) {
            Some(langid) => !wanted.contains(&langid),
            // An unparseable name is not ours to interpret; leave it alone.
            None => false,
        })
        .collect();

    for name in stale {
        match profiles.delete_subkey_all(&name) {
            Ok(()) => tracing::info!("removed stale TSF language profile {name}"),
            Err(e) => tracing::warn!("could not remove stale language profile {name}: {e}"),
        }
    }
}

/// `"0x0000042A"` → `0x042A`, matching how [`register_tsf_language_profile`]
/// writes the key.
fn parse_profile_langid(key: &str) -> Option<u32> {
    let digits = key.strip_prefix("0x").or_else(|| key.strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
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
    // A text service with no language profile is invisible everywhere, with no
    // error to show for it. Refuse rather than "succeed" into that state.
    anyhow::ensure!(
        !languages.is_empty(),
        "no language to register the text service under (Windows reported no default language)"
    );

    prune_stale_language_profiles(&tip_key, &languages);

    // The keyboards installed next to THIS DLL — the same folder the text
    // service resolves profiles against at runtime (`set_resource_dir`).
    // Never `current_exe()`: under regsvr32 that is System32.
    let keyboards = dll_path
        .parent()
        .map(|dir| dir.join("keyboards"))
        .unwrap_or_default();
    let profiles = profiles_in(&[&keyboards]);
    for profile in &profiles {
        tracing::info!(
            "TSF profile {} = {} ({})",
            guid_string(&profile.guid),
            profile.method_id,
            profile.description
        );
    }

    for &langid in &languages {
        register_tsf_language_profiles(&tip_key, dll_path, langid, &profiles).with_context(
            || {
                format!(
                    "Failed to register language profiles for LANGID 0x{:08X}",
                    langid
                )
            },
        )?;
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

// Define GUID manually to match CLSID_BUTTRE_TEXT_SERVICE string
const CLSID_BUTTRE: windows::core::GUID = windows::core::GUID {
    data1: 0xE6B8A6C0,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34, 0x56, 0x78],
};

/// TSF categories the text service declares, as `(category, label)`.
///
/// `IMMERSIVESUPPORT` is how an IME declares itself compatible with Windows
/// (packaged) apps and `SYSTRAYSUPPORT` with the taskbar input indicator.
/// Every built-in keyboard IME declares both; buttre without them was never
/// loaded into Windows Terminal. Deliberately NOT declared: `COMLESS`
/// (buttre creates COM objects while active) and `UIELEMENTENABLED`
/// (promises a UI-less candidate API buttre does not implement).
fn categories() -> [(windows::core::GUID, &'static str); 4] {
    use windows::Win32::UI::TextServices::{
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, GUID_TFCAT_TIP_KEYBOARD,
    };
    [
        (GUID_TFCAT_TIP_KEYBOARD, "Keyboard"),
        (
            GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
            "DisplayAttributeProvider",
        ),
        (GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, "ImmersiveSupport"),
        (GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, "SystraySupport"),
    ]
}

// Register Categories using ITfCategoryMgr
fn register_categories() -> Result<()> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::TextServices::{CLSID_TF_CategoryMgr, ITfCategoryMgr};

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

        for (category, label) in categories() {
            cat_mgr
                .RegisterCategory(&CLSID_BUTTRE, &category, &CLSID_BUTTRE)
                .with_context(|| format!("Failed to register {label} category"))?;
        }
    }
    Ok(())
}

fn unregister_categories() -> Result<()> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::TextServices::{CLSID_TF_CategoryMgr, ITfCategoryMgr};

    // SAFETY:
    // 1. CoCreateInstance is properly declared in windows crate
    // 2. Same invariants as register_categories above
    // 3. UnregisterCategory is safe even if category doesn't exist
    // 4. Errors are ignored (best-effort cleanup during uninstall)
    unsafe {
        if let Ok(cat_mgr) =
            CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
        {
            for (category, _) in categories() {
                let _ = cat_mgr.UnregisterCategory(&CLSID_BUTTRE, &category, &CLSID_BUTTRE);
            }
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
        register_com_servers(dll_path)?;
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
        unregister_com_server(0);
        unregister_com_server(KEY_WOW64_32KEY);
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

    const JA_JP: u32 = 0x0411;

    /// The bug this pins, and it took days to find: registering under a language
    /// the machine does not have produces an entry the user can SEE and SELECT
    /// and which silently types with the plain layout instead.
    #[test]
    fn no_vietnamese_profile_when_vietnamese_is_not_installed() {
        for default in [LANGID_ENGLISH_US, JA_JP] {
            let ids = language_ids_for(default, default, false);
            assert!(
                !ids.contains(&LANGID_VIETNAMESE),
                "phantom vi-VN entry for default {default:#06X}"
            );
            assert_eq!(ids, vec![default]);
        }
    }

    /// The reason the IME exists: once Vietnamese IS installed, buttre has to be
    /// offerable under it, whatever the system language is.
    #[test]
    fn vietnamese_is_registered_once_installed() {
        for default in [LANGID_ENGLISH_US, JA_JP, LANGID_VIETNAMESE] {
            assert!(
                language_ids_for(default, default, true).contains(&LANGID_VIETNAMESE),
                "vi-VN missing for default {default:#06X}"
            );
        }
    }

    /// The MSI's custom action runs as SYSTEM, so the two defaults can differ.
    /// Both must be covered, and neither may be dropped in favour of the other.
    #[test]
    fn user_and_system_defaults_are_both_registered() {
        let ids = language_ids_for(JA_JP, LANGID_ENGLISH_US, false);
        assert!(ids.contains(&JA_JP));
        assert!(ids.contains(&LANGID_ENGLISH_US));
    }

    /// A duplicate would create the same profile key twice — harmless but it
    /// makes the registry read as if two profiles exist.
    #[test]
    fn each_language_is_included_exactly_once() {
        let ids = language_ids_for(LANGID_VIETNAMESE, LANGID_VIETNAMESE, true);
        assert_eq!(ids, vec![LANGID_VIETNAMESE]);
    }

    /// A zero langid is not a language. Windows can report 0 in odd service
    /// contexts, and a `LanguageProfile\0x00000000` key would be pure noise.
    #[test]
    fn zero_is_never_registered() {
        assert_eq!(
            language_ids_for(0, LANGID_ENGLISH_US, false),
            vec![LANGID_ENGLISH_US]
        );
        assert!(language_ids_for(0, 0, false).is_empty());
    }

    #[test]
    fn profile_key_names_round_trip() {
        // Must match `register_tsf_language_profile`'s `0x{:08X}`, or pruning
        // would silently skip every profile it is supposed to clean up.
        for langid in [LANGID_ENGLISH_US, LANGID_VIETNAMESE, JA_JP] {
            let key = format!("0x{langid:08X}");
            assert_eq!(parse_profile_langid(&key), Some(langid));
        }
    }

    #[test]
    fn unparseable_profile_names_are_left_alone() {
        // Anything we cannot read is something we did not write — pruning it
        // would mean deleting a stranger's registry key.
        assert_eq!(parse_profile_langid("Category"), None);
        assert_eq!(parse_profile_langid("0409"), None, "missing 0x prefix");
        assert_eq!(parse_profile_langid(""), None);
    }
}
