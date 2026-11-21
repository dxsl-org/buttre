//! One TSF language profile per input method (plan
//! `.agents/261121-1100-tsf-per-method-profiles`).
//!
//! Each method — Telex, VNI, Chữ Nôm, every keyboard TOML — is its own entry
//! in Windows' keyboard list, picked with Win+Space or Ctrl+Shift like any
//! other keyboard. The active profile IS the method: the text service reads it
//! from TSF, not from `Settings::input_method`.
//!
//! This table is the single source for both registration and the reverse
//! lookup, so the two cannot disagree about which GUID means which method.
//!
//! GUIDs must never change once shipped: Windows records the user's added
//! keyboards by profile GUID, and a profile that disappears takes the user's
//! choice with it (and leaves a dead `Preload` entry behind).

use std::path::Path;

use windows::core::GUID;

/// Telex keeps the GUID of the single profile buttre registered before this
/// table existed, so users who already added buttre keep their keyboard —
/// now as "buttre Telex", the method nearly all of them were typing with.
pub const TELEX_PROFILE: GUID = GUID::from_u128(0xB7447743_7652_4AB6_8D82_250D935EBCC0);

/// One keyboard entry Windows offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEntry {
    /// Engine method id: `telex`, `vni`, `nom`, or a keyboard TOML's stem.
    pub method_id: String,
    pub guid: GUID,
    /// Name shown in Settings and the Win+Space list.
    pub description: String,
}

/// Every profile to register: the built-in methods, then each keyboard TOML
/// in `keyboards_dir` (the `keyboards\` folder installed next to the DLL).
/// A TOML added later gets its profile the next time registration runs.
pub fn profiles_in(keyboards_dir: &Path) -> Vec<ProfileEntry> {
    crate::shared::method_menu::method_items_in(keyboards_dir)
        .into_iter()
        // "English" is the absence of buttre, i.e. the plain EN-US keyboard
        // the user switches to with Ctrl+Shift — never a buttre profile.
        .filter(|(id, _)| id != "english")
        .map(|(id, label)| ProfileEntry {
            guid: profile_guid(&id),
            description: format!("buttre {label}"),
            method_id: id,
        })
        .collect()
}

/// [`profiles_in`] for the keyboards installed next to this binary.
pub fn profiles() -> Vec<ProfileEntry> {
    profiles_in(&buttre_core::vietnamese::get_custom_dir())
}

/// The method a profile GUID stands for. Built-ins resolve without touching
/// disk; any other GUID is matched against the installed keyboard TOMLs.
pub fn method_for_profile(guid: &GUID) -> Option<String> {
    ["telex", "vni", "nom"]
        .into_iter()
        .find(|id| profile_guid(id) == *guid)
        .map(str::to_string)
        .or_else(|| {
            profiles()
                .into_iter()
                .find(|entry| entry.guid == *guid)
                .map(|entry| entry.method_id)
        })
}

/// The profile GUID of a method id: [`TELEX_PROFILE`] for Telex, otherwise a
/// name-based GUID derived from the id alone — stable across machines,
/// installs and releases without a table of hand-picked constants, which is
/// what lets a keyboard TOML added after install still get a permanent GUID.
///
/// FNV-1a-128 over a fixed namespace + the id, then MurmurHash3's `fmix64`
/// finalizer on each half (FNV alone leaves ids that differ only at the end
/// sharing most of their digits), stamped as an RFC 9562 version-8 (custom)
/// UUID. Hand-rolled rather than the RFC's SHA-1 scheme only to avoid a
/// dependency; every step is specified here, so it cannot drift with a
/// library upgrade — `derived_guids_are_frozen` pins the shipped values.
pub fn profile_guid(method_id: &str) -> GUID {
    if method_id == "telex" {
        return TELEX_PROFILE;
    }
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    let hash = b"buttre-tsf-profile:"
        .iter()
        .chain(method_id.as_bytes())
        .fold(OFFSET, |hash, &byte| {
            (hash ^ u128::from(byte)).wrapping_mul(PRIME)
        });
    let high = fmix64((hash >> 64) as u64 ^ hash as u64);
    let low = fmix64(hash as u64 ^ high);
    let mixed = (u128::from(high) << 64) | u128::from(low);
    // Version 8 in bits 76..80, variant 0b10 in bits 62..64.
    let stamped = (mixed & !(0xF << 76) & !(0b11 << 62)) | (0x8 << 76) | (0b10 << 62);
    GUID::from_u128(stamped)
}

/// MurmurHash3's 64-bit finalizer: every input bit affects every output bit.
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^ (k >> 33)
}

/// Registry/GUID text form `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, upper
/// case like the rest of buttre's registry keys.
pub fn guid_string(guid: &GUID) -> String {
    format!("{{{guid:?}}}").to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Users who added buttre before per-method profiles keep their keyboard.
    #[test]
    fn telex_keeps_the_original_profile_guid() {
        assert_eq!(
            guid_string(&profile_guid("telex")),
            "{B7447743-7652-4AB6-8D82-250D935EBCC0}"
        );
    }

    /// A changed GUID would silently drop every user's added keyboard. These
    /// values are shipped: if this test fails, the hash changed — revert it.
    #[test]
    fn derived_guids_are_frozen() {
        assert_eq!(
            guid_string(&profile_guid("vni")),
            "{3457D2F5-06C7-8AE2-B2A0-4E8FC0C11DB1}"
        );
        assert_eq!(
            guid_string(&profile_guid("nom")),
            "{209A2F9C-2DDA-8A6D-9E73-5D09D26AE3A9}"
        );
    }

    #[test]
    fn builtin_profiles_resolve_back_to_their_method() {
        for id in ["telex", "vni", "nom"] {
            assert_eq!(method_for_profile(&profile_guid(id)).as_deref(), Some(id));
        }
    }

    /// The bundled keyboards become profiles; English never does.
    #[test]
    fn english_is_never_a_profile_and_keyboards_are() {
        let keyboards = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../keyboards");
        let entries = profiles_in(&keyboards);

        let ids: Vec<&str> = entries.iter().map(|e| e.method_id.as_str()).collect();
        assert!(!ids.contains(&"english"));
        assert_eq!(&ids[..3], ["telex", "vni", "nom"]);
        let thai = entries
            .iter()
            .find(|e| e.method_id == "thai")
            .expect("bundled thai.toml must become a profile");
        assert_eq!(thai.guid, profile_guid("thai"));
        assert!(thai.description.starts_with("buttre "));
    }
}
