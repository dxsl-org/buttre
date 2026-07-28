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
//! Availability and selection live in different hives, and that distinction is
//! the whole answer:
//!
//! * `HKLM\SOFTWARE\Microsoft\CTF\TIP\<clsid>` — written by the installer:
//!   "this text service EXISTS and may be offered".
//! * `HKCU\Software\Microsoft\CTF\TIP\<clsid>` — written by Windows when the
//!   user adds or removes the keyboard: "this user WANTS it".
//!
//! Only the second one answers the question, and we never write it.

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

/// Language ids the user has enabled buttre under, per `HKCU`.
///
/// Public so `buttre --tsf-status` can show them: "which languages" is the
/// first thing anyone asks when the answer is not what they expected.
pub fn enabled_langids() -> Vec<u32> {
    let Ok(profiles) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(
        "Software\\Microsoft\\CTF\\TIP\\{}\\LanguageProfile",
        CLSID_BUTTRE_TEXT_SERVICE
    )) else {
        // No per-user key at all: Windows has never been told to add it.
        return Vec::new();
    };

    profiles
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter(|langid| profile_enabled(&profiles, langid))
        .filter_map(|langid| parse_langid(&langid))
        .collect()
}

/// Is any profile under this language id switched on?
///
/// The `Enable` value sits one level deeper, on the profile GUID, and a
/// language can hold more than one profile — so this looks at every child
/// rather than assuming a single well-known GUID.
fn profile_enabled(profiles: &RegKey, langid: &str) -> bool {
    let Ok(language) = profiles.open_subkey(langid) else {
        return false;
    };
    language
        .enum_keys()
        .filter_map(|name| name.ok())
        .filter_map(|profile| language.open_subkey(profile).ok())
        .any(|profile| profile.get_value::<u32, _>("Enable").unwrap_or(0) != 0)
}

/// `"0x00000409"` → `0x0409`. Windows writes these as 8-digit hex with the
/// `0x` prefix; anything else is not ours to interpret.
fn parse_langid(key: &str) -> Option<u32> {
    let digits = key.strip_prefix("0x").or_else(|| key.strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langid_keys_parse_as_windows_writes_them() {
        assert_eq!(parse_langid("0x00000409"), Some(0x0409));
        assert_eq!(parse_langid("0x0000042A"), Some(0x042A));
    }

    #[test]
    fn anything_else_is_rejected_rather_than_guessed() {
        assert_eq!(parse_langid("0409"), None, "missing prefix");
        assert_eq!(parse_langid("0xnope"), None);
        assert_eq!(parse_langid(""), None);
    }

    #[test]
    fn probing_this_machine_does_not_panic() {
        // The answer depends on whether the developer added buttre, so only the
        // absence of a panic is asserted — a registry shape we did not expect
        // must degrade to "not added", never crash the tray at startup.
        let _ = is_buttre_text_service_enabled();
    }
}
