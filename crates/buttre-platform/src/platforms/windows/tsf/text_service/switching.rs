// SPDX-License-Identifier: GPL-3.0-only
//! Turning buttre on/off and switching input methods from inside the text
//! service (plan `.agents/261121-1100-tsf-per-method-profiles`).
//!
//! - `Ctrl+Shift+1…9` activates the Nth buttre profile (1 Telex, 2 VNI,
//!   3 Chữ Nôm, then the keyboard TOMLs in `profiles.rs` order) and turns
//!   buttre on.
//! - `` Ctrl+Shift+` `` (the key left of `1`) or `Ctrl+Shift+0` turns buttre
//!   OFF while keeping its profile active: keys go to the application
//!   untouched. `` ` `` exists because Windows intercepts `Ctrl+Shift+0`
//!   (the long-standing "Excel cannot unhide columns" issue): on the field
//!   machine it reached the text service as a bare `0`, modifiers gone.
//!   Neither switches to a Windows keyboard — once buttre's profile is not
//!   active, nothing of buttre receives keys, so `Ctrl+Shift+1` could never
//!   bring it back.
//!
//! On/off lives in a TSF GLOBAL compartment: one value for the whole session,
//! seen and change-notified in every application where buttre is loaded, so
//! turning off in Word is off in Notepad too. Never written to disk — a new
//! session starts on.

use windows::core::{Result, GUID};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;

use crate::platforms::windows::tsf::profiles::{self, ProfileEntry};
use crate::platforms::windows::tsf::CLSID_BUTTRE_TEXT_SERVICE;

/// Global compartment holding buttre's on/off state (`VT_I4`: 1 on, 0 off;
/// empty = never set = on).
pub const GUID_COMPARTMENT_BUTTRE_ENABLED: GUID =
    GUID::from_u128(0x6f1d2c57_8a3e_4b0c_9d41_2e7b5a90c3f8);

/// What a `Ctrl+Shift+<key>` chord asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchCommand {
    /// `` Ctrl+Shift+` `` or `Ctrl+Shift+0`.
    TurnOff,
    /// `Ctrl+Shift+N`, N in 1..=9: the Nth profile (1-based).
    Activate(usize),
}

/// The switch command for a key with its modifiers, if it is one. Alt must
/// be up: `Ctrl+Alt+Shift+digit` stays the application's (and AltGr is
/// Ctrl+Alt on many layouts).
pub fn switch_command(vkey: u16, ctrl: bool, shift: bool, alt: bool) -> Option<SwitchCommand> {
    if !ctrl || !shift || alt {
        return None;
    }
    match vkey {
        // VK_OEM_3 is the `` ` `` key on US layouts — left of `1`.
        0x30 | 0xC0 => Some(SwitchCommand::TurnOff),
        0x31..=0x39 => Some(SwitchCommand::Activate(usize::from(vkey - 0x30))),
        _ => None,
    }
}

/// The profile `Ctrl+Shift+n` selects, among `entries` in table order.
pub fn profile_for_slot(entries: &[ProfileEntry], n: usize) -> Option<&ProfileEntry> {
    n.checked_sub(1).and_then(|index| entries.get(index))
}

/// The profile `Ctrl+Shift+n` selects on this machine.
pub fn installed_profile_for_slot(n: usize) -> Option<ProfileEntry> {
    profile_for_slot(&profiles::profiles(), n).cloned()
}

/// Is buttre on? An unset compartment means on (fresh session).
pub fn read_enabled(thread_mgr: &ITfThreadMgr) -> bool {
    // SAFETY: plain COM calls on the thread manager TSF gave us; the VARIANT
    // union is only read after its type tag is checked.
    unsafe {
        let Ok(value) = enabled_compartment(thread_mgr).and_then(|c| c.GetValue()) else {
            return true;
        };
        let inner = &value.Anonymous.Anonymous;
        !(inner.vt == VT_I4 && inner.Anonymous.lVal == 0)
    }
}

/// Turn buttre on or off for the whole session.
pub fn write_enabled(thread_mgr: &ITfThreadMgr, client_id: u32, on: bool) -> Result<()> {
    let value = VARIANT::from(i32::from(on));
    // SAFETY: plain COM calls; `client_id` is the id TSF handed us at Activate.
    unsafe { enabled_compartment(thread_mgr)?.SetValue(client_id, &value) }
}

/// The global compartment itself, for reading, writing and advising.
pub fn enabled_compartment(thread_mgr: &ITfThreadMgr) -> Result<ITfCompartment> {
    // SAFETY: plain COM calls on a live thread manager.
    unsafe {
        thread_mgr
            .GetGlobalCompartment()?
            .GetCompartment(&GUID_COMPARTMENT_BUTTRE_ENABLED)
    }
}

/// Make `profile` the active keyboard for the whole session — what picking it
/// in the Win+Space list does. Fails when the user has not added that keyboard.
pub fn activate_profile(profile: &GUID) -> Result<()> {
    // SAFETY: CoCreateInstance on a system CLSID in a thread TSF has
    // initialised COM for; the GUID pointers are to live locals.
    unsafe {
        let manager: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        let mut active = TF_INPUTPROCESSORPROFILE::default();
        manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut active)?;
        manager.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            active.langid,
            &CLSID_BUTTRE_TEXT_SERVICE,
            profile,
            HKL::default(),
            TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_with_ctrl_shift_are_commands() {
        assert_eq!(
            switch_command(0x30, true, true, false),
            Some(SwitchCommand::TurnOff)
        );
        assert_eq!(
            switch_command(0x31, true, true, false),
            Some(SwitchCommand::Activate(1))
        );
        assert_eq!(
            switch_command(0x39, true, true, false),
            Some(SwitchCommand::Activate(9))
        );
    }

    /// Windows intercepts Ctrl+Shift+0, so the key left of `1` must turn
    /// buttre off too.
    #[test]
    fn backtick_turns_off_like_zero() {
        assert_eq!(
            switch_command(0xC0, true, true, false),
            Some(SwitchCommand::TurnOff)
        );
        assert_eq!(switch_command(0xC0, false, false, false), None, "plain `");
    }

    /// Plain digits are text, Ctrl+digit and Shift+digit belong to the app,
    /// and Alt excludes AltGr layouts and Ctrl+Alt+Shift shortcuts.
    #[test]
    fn other_chords_are_left_alone() {
        assert_eq!(switch_command(0x31, false, false, false), None);
        assert_eq!(switch_command(0x31, true, false, false), None);
        assert_eq!(switch_command(0x31, false, true, false), None);
        assert_eq!(switch_command(0x31, true, true, true), None);
        assert_eq!(
            switch_command(0x5A, true, true, false),
            None,
            "Ctrl+Shift+Z"
        );
    }

    #[test]
    fn slots_follow_the_profile_table() {
        let entries = profiles::profiles_in(std::path::Path::new("does-not-exist"));
        assert_eq!(
            profile_for_slot(&entries, 1).map(|e| e.method_id.as_str()),
            Some("telex")
        );
        assert_eq!(
            profile_for_slot(&entries, 3).map(|e| e.method_id.as_str()),
            Some("nom")
        );
        assert!(profile_for_slot(&entries, 0).is_none());
        assert!(
            profile_for_slot(&entries, 4).is_none(),
            "no keyboard TOMLs here"
        );
    }
}
