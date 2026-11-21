//! Per-user keyboard setup run by the installer after registration
//! (`buttre --add-keyboard <id>`, `buttre --disable-layout-hotkey`).
//!
//! Registration (HKLM, elevated) makes every buttre keyboard AVAILABLE; this
//! adds the ones the user ticked in the installer to THEIR keyboard list —
//! the same thing Settings → Keyboards → Add does. It must run as the user
//! (the MSI's impersonated deferred actions), because the list lives in HKCU.

use anyhow::{Context, Result};
use windows::core::{s, w, BOOL, PCWSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use super::profiles::{guid_string, profiles_in};
use super::registration::CLSID_BUTTRE_TEXT_SERVICE;

/// Add the buttre keyboard for `method_id` to the current user's list.
pub fn add_keyboard(method_id: &str) -> Result<()> {
    let keyboards = std::env::current_exe()
        .context("cannot locate buttre.exe")?
        .parent()
        .context("buttre.exe has no parent directory")?
        .join("keyboards");
    let profile = profiles_in(&keyboards)
        .into_iter()
        .find(|p| p.method_id == method_id)
        .with_context(|| format!("no buttre keyboard '{method_id}'"))?;
    let langid = target_langid().context("buttre is not registered under any language")?;
    let entry = format!(
        "{langid:04x}:{CLSID_BUTTRE_TEXT_SERVICE}{}",
        guid_string(&profile.guid)
    );
    install_layout_or_tip(&entry)?;
    println!("added keyboard {} ({entry})", profile.description);
    Ok(())
}

/// The language to add keyboards under: the first language in the user's
/// OWN language list (Settings → Language, in order) that buttre is
/// registered under.
///
/// Never anything outside that list. `InstallLayoutOrTip` under a language
/// the user does not have ADDS that language to their list — measured: the
/// formats locale (`GetUserDefaultLangID`, en-GB on the field machine) put a
/// whole new "English (United Kingdom)" into a list that only had en-US.
fn target_langid() -> Option<u32> {
    let path =
        format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{CLSID_BUTTRE_TEXT_SERVICE}\\LanguageProfile");
    let languages = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(path).ok()?;
    let registered: Vec<u32> = languages
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter_map(|name| u32::from_str_radix(name.strip_prefix("0x")?, 16).ok())
        .collect();
    pick_langid(&user_language_ids(), &registered)
}

/// LANGIDs of the user's language list, in the user's order.
fn user_language_ids() -> Vec<u32> {
    use windows::Win32::Globalization::LocaleNameToLCID;
    let Ok(tags) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Control Panel\\International\\User Profile")
        .and_then(|profile| profile.get_value::<Vec<String>, _>("Languages"))
    else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(|tag| {
            let wide: Vec<u16> = tag.encode_utf16().chain(Some(0)).collect();
            // SAFETY: `wide` is a live NUL-terminated locale name.
            let lcid = unsafe { LocaleNameToLCID(PCWSTR(wide.as_ptr()), 0) };
            // LANGID is the low word of an LCID; 0 = unknown name.
            (lcid != 0).then_some(lcid & 0xFFFF)
        })
        .collect()
}

/// Pure choice behind [`target_langid`] (unit-tested): the first of the
/// user's languages buttre is registered under, or none.
fn pick_langid(user_languages: &[u32], registered: &[u32]) -> Option<u32> {
    user_languages
        .iter()
        .copied()
        .find(|langid| registered.contains(langid))
}

/// `InstallLayoutOrTip` from `input.dll` — the documented API behind
/// Settings' "Add a keyboard". Loaded at runtime: it is not in every SDK's
/// import libraries.
fn install_layout_or_tip(entry: &str) -> Result<()> {
    type InstallLayoutOrTip = unsafe extern "system" fn(PCWSTR, u32) -> BOOL;
    let wide: Vec<u16> = entry.encode_utf16().chain(Some(0)).collect();
    // SAFETY: input.dll is a system DLL; the symbol's signature is
    // `BOOL InstallLayoutOrTip(LPCWSTR psz, DWORD dwFlags)`. `wide` is a live
    // NUL-terminated buffer for the duration of the call.
    unsafe {
        let module = LoadLibraryW(w!("input.dll")).context("cannot load input.dll")?;
        let proc = GetProcAddress(module, s!("InstallLayoutOrTip"))
            .context("input.dll has no InstallLayoutOrTip")?;
        let install: InstallLayoutOrTip = std::mem::transmute(proc);
        anyhow::ensure!(
            install(PCWSTR(wide.as_ptr()), 0).as_bool(),
            "InstallLayoutOrTip({entry}) failed"
        );
    }
    Ok(())
}

/// Turn off Windows' "Switch keyboard layout" hotkey (Ctrl+Shift), which
/// fires before buttre's Ctrl+Shift+1…9 can (`switching.rs`). Win+Space
/// still switches keyboards. Same as Settings → Typing → Advanced keyboard
/// settings → Input language hot keys → Switch Keyboard Layout → Not Assigned.
pub fn disable_layout_hotkey() -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_SETLANGTOGGLE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let (toggle, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Keyboard Layout\\Toggle")
        .context("cannot open Keyboard Layout\\Toggle")?;
    // "3" = Not Assigned.
    toggle.set_value("Layout Hotkey", &"3")?;
    // SAFETY: SPI_SETLANGTOGGLE takes no parameter; it makes Windows re-read
    // the Toggle key now instead of at next sign-in.
    unsafe {
        SystemParametersInfoW(
            SPI_SETLANGTOGGLE,
            0,
            None,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .context("SPI_SETLANGTOGGLE failed")?;
    }
    println!("Windows' Ctrl+Shift keyboard-layout hotkey is now Not Assigned");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user's own order wins among languages buttre is registered for.
    #[test]
    fn picks_the_first_user_language_buttre_is_registered_under() {
        assert_eq!(
            pick_langid(&[0x042A, 0x0409], &[0x0409, 0x042A]),
            Some(0x042A)
        );
        assert_eq!(
            pick_langid(&[0x0411, 0x0409], &[0x0809, 0x0409]),
            Some(0x0409)
        );
    }

    /// The field bug: buttre registered under en-US and en-GB, user list only
    /// en-US. en-GB must never be picked — adding under it adds the language.
    #[test]
    fn never_picks_a_language_outside_the_users_list() {
        assert_eq!(pick_langid(&[0x0409], &[0x0809, 0x0409]), Some(0x0409));
        assert_eq!(pick_langid(&[0x0411], &[0x0809, 0x0409]), None);
        assert_eq!(pick_langid(&[], &[0x0409]), None);
    }
}
