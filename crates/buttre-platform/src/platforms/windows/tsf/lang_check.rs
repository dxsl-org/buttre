//! Can the TSF text service actually receive keystrokes for this user?
//!
//! The tray asks this to choose between the TSF backend and the global-hook
//! backend, and the two must never both be live: the hook sees keys first
//! (`WH_KEYBOARD_LL`) and blocks the ones it handles, so a text service running
//! alongside it is starved — the user gets hook behaviour while believing they
//! are testing TSF.
//!
//! This used to ask a PROXY question — "is Vietnamese in the user's language
//! list?", via a PowerShell subprocess — and that proxy has been wrong in both
//! directions since registration started covering en-US as well as vi-VN:
//!
//! * buttre added as an input method under English, Vietnamese not in the
//!   language list → TSF works, but the tray chose Hook.
//! * Vietnamese in the language list, buttre never added → the tray chose TSF
//!   and nothing typed at all, because no text service was ever activated.
//!
//! So ask the real question instead, through the TSF API that owns the answer.

use crate::platforms::windows::tsf::CLSID_BUTTRE_TEXT_SERVICE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE, TF_IPP_FLAG_ENABLED,
    TF_PROFILETYPE_INPUTPROCESSOR,
};

/// CLSID of the TSF input-processor-profiles object. Not exported by
/// windows-rs under a stable name in this version, so it is spelled out here;
/// the value is fixed by Windows and cannot change.
const CLSID_TF_INPUT_PROCESSOR_PROFILES: windows::core::GUID =
    windows::core::GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643ecfed);

/// True when buttre's text service is ENABLED as one of this user's input
/// methods — the only state in which it can ever be handed a keystroke.
///
/// A registered-but-not-added service returns `false`: registration makes the
/// IME available in Windows' input-method picker, it does not select it. The
/// user still has to add it under Settings → Time & language → Language &
/// region → (a language) → Keyboards.
///
/// Errors are treated as `false` — the hook backend works everywhere, so
/// guessing "TSF" on a failed probe would risk leaving the user unable to type
/// at all.
pub fn is_buttre_text_service_enabled() -> bool {
    // SAFETY: standard COM lifecycle. `CoUninitialize` is called only when this
    // call is the one that initialised the apartment, so a host that already
    // set one up is left alone.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let we_inited = hr == windows::Win32::Foundation::S_OK;
        if hr.is_err() && !we_inited {
            tracing::warn!("CoInitializeEx failed while probing TSF profiles: {hr:?}");
            return false;
        }
        let enabled = probe_enabled_profiles().unwrap_or_else(|e| {
            tracing::warn!("could not enumerate TSF profiles ({e}); assuming buttre is not added");
            false
        });
        if we_inited {
            CoUninitialize();
        }
        enabled
    }
}

/// Walk every installed input-processor profile looking for ours with the
/// ENABLED flag set. `EnumProfiles(0)` means "all languages", which is what we
/// want: the profile is registered under en-US, vi-VN and the user's default,
/// and any one of them being enabled is enough.
///
/// # Safety
/// Caller must have initialised COM for this thread.
unsafe fn probe_enabled_profiles() -> windows::core::Result<bool> {
    let manager: ITfInputProcessorProfileMgr = unsafe {
        CoCreateInstance(
            &CLSID_TF_INPUT_PROCESSOR_PROFILES,
            None,
            CLSCTX_INPROC_SERVER,
        )?
    };
    let profiles = unsafe { manager.EnumProfiles(0)? };

    // Fetched in batches; Next reports how many it filled in and returns
    // S_FALSE (not an error) once the list is exhausted.
    let mut batch = [TF_INPUTPROCESSORPROFILE::default(); 16];
    loop {
        let mut fetched = 0u32;
        unsafe { profiles.Next(&mut batch, &mut fetched) }?;
        if fetched == 0 {
            return Ok(false);
        }
        for profile in &batch[..fetched as usize] {
            let is_ours = profile.clsid == CLSID_BUTTRE_TEXT_SERVICE
                && profile.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR;
            if is_ours && profile.dwFlags & TF_IPP_FLAG_ENABLED != 0 {
                tracing::info!(
                    "buttre text service is enabled for langid 0x{:04X}",
                    profile.langid
                );
                return Ok(true);
            }
        }
    }
}
