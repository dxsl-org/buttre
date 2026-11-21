// SPDX-License-Identifier: GPL-3.0-only
//! Icons for the input-mode button next to the taskbar's language indicator
//! (`langbar.rs`): the active method's icon while buttre is on, the "E" icon
//! while it is off — the same artwork the hook app's tray shows.

use image::imageops::FilterType;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

// `static`, not `const`: one copy each, at one address (the test compares
// which artwork was picked by address).
static TELEX: &[u8] = include_bytes!("../../../../../icons/telex.png");
static VNI: &[u8] = include_bytes!("../../../../../icons/vni.png");
static NOM: &[u8] = include_bytes!("../../../../../icons/nom.png");
static CUSTOM: &[u8] = include_bytes!("../../../../../icons/custom.png");
static OFF: &[u8] = include_bytes!("../../../../../icons/english.png");

/// The PNG shown for `method_id`, or for buttre being off.
fn artwork(method_id: &str, on: bool) -> &'static [u8] {
    match (on, method_id) {
        (false, _) => OFF,
        (true, "telex") => TELEX,
        (true, "vni") => VNI,
        (true, "nom") => NOM,
        (true, _) => CUSTOM,
    }
}

/// A new icon at the system small-icon size. The caller owns it — TSF
/// destroys what `ITfLangBarItemButton::GetIcon` returns. `None` only if
/// decoding or GDI fails, which leaves the button iconless rather than broken.
pub fn icon_for(method_id: &str, on: bool) -> Option<HICON> {
    // SAFETY: GetSystemMetrics takes a constant index and cannot fail.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let image = image::load_from_memory(artwork(method_id, on))
        .ok()?
        .resize_exact(size, size, FilterType::Lanczos3)
        .into_rgba8();
    // GDI wants BGRA; straight (non-premultiplied) alpha is what icons use.
    let bgra: Vec<u8> = image
        .pixels()
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    create_icon(&bgra, size as i32)
}

/// Build an alpha icon from top-down BGRA pixels.
fn create_icon(bgra: &[u8], size: i32) -> Option<HICON> {
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size,
        biHeight: -size, // negative: top-down rows
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    // SAFETY: `info` describes a size×size 32bpp DIB; `bits` receives a
    // pointer to exactly size*size*4 bytes, which is what we copy in. Both
    // bitmaps are deleted after CreateIconIndirect, which copies them.
    unsafe {
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(color.into());
            return None;
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
        // All-zero AND mask: transparency comes from the alpha channel.
        let mask = CreateBitmap(size, size, 1, 1, None);
        let icon_info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&icon_info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off always shows the "E" artwork, whatever the method.
    #[test]
    fn off_uses_the_english_artwork() {
        for method in ["telex", "vni", "nom", "thai"] {
            assert_eq!(artwork(method, false).as_ptr(), OFF.as_ptr());
        }
        assert_eq!(artwork("vni", true).as_ptr(), VNI.as_ptr());
        assert_eq!(artwork("thai", true).as_ptr(), CUSTOM.as_ptr());
    }

    #[test]
    fn every_artwork_builds_an_icon() {
        for (method, on) in [
            ("telex", true),
            ("vni", true),
            ("nom", true),
            ("thai", true),
            ("telex", false),
        ] {
            let icon = icon_for(method, on);
            assert!(icon.is_some(), "{method} on={on}");
            if let Some(icon) = icon {
                // SAFETY: the icon was just created by this test.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
                }
            }
        }
    }
}
