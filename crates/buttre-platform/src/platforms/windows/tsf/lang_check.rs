//! Has the user ADDED buttre as one of their input methods?
//!
//! The tray asks this to choose between the TSF backend and the global-hook
//! backend, and the two must never both be live: the hook sees keys first
//! (`WH_KEYBOARD_LL`) and blocks the ones it handles, so a text service running
//! alongside it is starved. Answer it wrong in the other direction and the tray
//! picks TSF while no text service can ever activate — the user cannot type at
//! all.
//!
//! Two earlier versions of this check were both wrong, in instructive ways:
//!
//! 1. "Is Vietnamese in the user's language list?" — a PROXY, and wrong in both
//!    directions once the profile started being registered under en-US too.
//! 2. `ITfInputProcessorProfileMgr::EnumProfiles` + `TF_IPP_FLAG_ENABLED` —
//!    SELF-CONFIRMING. That flag reflects the machine-wide `Enable=1` that our
//!    own installer writes under `HKLM\...\CTF\TIP`, so it answered "yes" on a
//!    machine where the user had never added buttre and Win+Space did not offer
//!    it. A check that reads back what we ourselves wrote validates nothing.
//!
//! Availability and selection live in different places, and that distinction is
//! the whole answer:
//!
//! * `HKLM\SOFTWARE\Microsoft\CTF\TIP\<clsid>` — written by the installer:
//!   "this text service EXISTS and may be offered". We write it, so reading it
//!   back proves nothing.
//! * `HKCU\Control Panel\International\User Profile\<lang>` — written by
//!   Windows when the keyboard is added, as a value NAMED
//!   `<langid>:{clsid}{profile}`. This is the list `Get-WinUserLanguageList`
//!   reports and Settings edits, and it is the one that answers the question.
//! * `HKCU\Software\Microsoft\CTF\TIP\<clsid>\...\Enable` — a per-user OVERRIDE,
//!   present only when the user has explicitly disabled a profile. Absent means
//!   "no opinion", NOT "not added" — a third wrong guess this module made.
//!
//! Attempt 3 read only that last key, found it empty on a machine where the
//! keyboard was working, and answered "Hook" while TSF was live — putting both
//! layers on the keyboard at once, which is the exact thing this check exists
//! to prevent.

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

// The registry-string form of the CLSID, not the `GUID` constant of the same
// name in the parent module — registry paths are built from the braced text.
use crate::platforms::windows::tsf::registration::CLSID_BUTTRE_TEXT_SERVICE;

/// True when buttre's text service is one of this user's input methods — the
/// only state in which it can ever be handed a keystroke.
///
/// A registered-but-not-added service returns `false`: registration makes the
/// IME available in Windows' picker, it does not select it. The user still has
/// to add it under Settings → Time & language → Language & region →
/// (a language) → Keyboards.
///
/// A missing key, an unreadable one, or `Enable=0` all mean `false` — the hook
/// backend works everywhere, so guessing "TSF" here risks leaving the user
/// unable to type.
pub fn is_buttre_text_service_enabled() -> bool {
    match enabled_langids() {
        ids if ids.is_empty() => {
            tracing::info!("buttre is registered but not added as an input method");
            false
        }
        ids => {
            let list: Vec<String> = ids.iter().map(|id| format!("0x{id:04X}")).collect();
            tracing::info!("buttre is an input method for: {}", list.join(", "));
            true
        }
    }
}

/// Language ids the user has buttre added under.
///
/// Public so `buttre --tsf-status` can show them: "which languages" is the
/// first thing anyone asks when the answer is not what they expected.
pub fn enabled_langids() -> Vec<u32> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(languages) = hkcu.open_subkey("Control Panel\\International\\User Profile") else {
        return Vec::new();
    };

    let mut langids: Vec<u32> = languages
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter_map(|language| languages.open_subkey(language).ok())
        .flat_map(|language| added_langids_in(&language))
        .collect();

    langids.retain(|langid| !explicitly_disabled(&hkcu, *langid));
    langids.sort_unstable();
    langids.dedup();
    langids
}

/// Langids from one language subkey's value NAMES.
///
/// Windows records a keyboard as a value named `<langid>:<layout-or-tip>`, where
/// a text service is spelled `{clsid}{profile}`. The value's DATA is only an
/// ordering index, so the name is the whole signal.
fn added_langids_in(language: &RegKey) -> Vec<u32> {
    language
        .enum_values()
        .filter_map(|entry| entry.ok())
        .filter_map(|(name, _)| parse_tip_entry(&name))
        .collect()
}

/// `"0409:{clsid}{profile}"` → `Some(0x0409)` when the CLSID is ours.
///
/// Case-insensitive on the GUID: Windows has written both cases over the years,
/// and a case-sensitive compare here would silently answer "not added".
fn parse_tip_entry(name: &str) -> Option<u32> {
    let (langid, service) = name.split_once(':')?;
    if !service
        .to_ascii_uppercase()
        .starts_with(&CLSID_BUTTRE_TEXT_SERVICE.to_ascii_uppercase())
    {
        return None;
    }
    u32::from_str_radix(langid, 16).ok()
}

/// Has the user explicitly switched this language's profile OFF?
///
/// `HKCU\...\CTF\TIP\<clsid>\LanguageProfile\0x0000<langid>\<profile>` carries
/// `Enable` only as an override. A MISSING key means "no opinion" — treating it
/// as "not added" is what made the previous version of this module wrong.
fn explicitly_disabled(hkcu: &RegKey, langid: u32) -> bool {
    let path = format!(
        "Software\\Microsoft\\CTF\\TIP\\{}\\LanguageProfile\\0x{:08X}",
        CLSID_BUTTRE_TEXT_SERVICE, langid
    );
    let Ok(language) = hkcu.open_subkey(path) else {
        return false;
    };
    // Any profile under it explicitly zeroed counts as off; a language with no
    // profiles listed is simply unopinionated.
    language
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter_map(|profile| language.open_subkey(profile).ok())
        .filter_map(|profile| profile.get_value::<u32, _>("Enable").ok())
        .any(|enable| enable == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "{B7447743-7652-4AB6-8D82-250D935EBCC0}";

    #[test]
    fn tip_entries_parse_as_windows_writes_them() {
        let name = format!("0409:{CLSID_BUTTRE_TEXT_SERVICE}{PROFILE}");
        assert_eq!(parse_tip_entry(&name), Some(0x0409));
        let vietnamese = format!("042a:{CLSID_BUTTRE_TEXT_SERVICE}{PROFILE}");
        assert_eq!(parse_tip_entry(&vietnamese), Some(0x042A));
    }

    #[test]
    fn guid_case_does_not_change_the_answer() {
        // Windows has written both cases; a case-sensitive compare here would
        // silently report the keyboard as not added.
        let lower = format!(
            "0409:{}{}",
            CLSID_BUTTRE_TEXT_SERVICE.to_ascii_lowercase(),
            PROFILE.to_ascii_lowercase()
        );
        assert_eq!(parse_tip_entry(&lower), Some(0x0409));
    }

    #[test]
    fn other_keyboards_are_not_mistaken_for_ours() {
        assert_eq!(parse_tip_entry("0409:00000409"), None, "plain layout");
        assert_eq!(
            parse_tip_entry(
                "042a:{C2CB2CF0-AF47-413E-9780-8BC3A3C16068}{5FB02EC5-0A77-4684-B4FA-DEF8A2195628}"
            ),
            None,
            "Microsoft's Vietnamese IME"
        );
        assert_eq!(parse_tip_entry("nonsense"), None);
        assert_eq!(parse_tip_entry(""), None);
    }

    #[test]
    fn probing_this_machine_does_not_panic() {
        // The answer depends on whether the developer added buttre, so only the
        // absence of a panic is asserted — a registry shape we did not expect
        // must degrade to "not added", never crash the tray at startup.
        let _ = is_buttre_text_service_enabled();
    }
}
