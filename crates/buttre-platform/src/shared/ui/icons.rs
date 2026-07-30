//! Icon loading utilities for tray and menu icons

use anyhow::Result;
use tray_icon::Icon as TrayIcon;

/// Embedded icon bytes
pub const VIETNAMESE_ICON_BYTES: &[u8] = include_bytes!("../../../icons/vietnamese.png");
pub const ENGLISH_ICON_BYTES: &[u8] = include_bytes!("../../../icons/english.png");
pub const CHECK_ICON_BYTES: &[u8] = include_bytes!("../../../icons/check.png");
pub const CUSTOM_ICON_BYTES: &[u8] = include_bytes!("../../../icons/custom.png");

// Input method specific icons
pub const TELEX_ICON_BYTES: &[u8] = include_bytes!("../../../icons/telex.png");
pub const VNI_ICON_BYTES: &[u8] = include_bytes!("../../../icons/vni.png");
pub const NOM_ICON_BYTES: &[u8] = include_bytes!("../../../icons/nom.png");

/// Load a tray icon from embedded bytes
pub fn load_icon_from_bytes(bytes: &[u8]) -> Result<TrayIcon> {
    let (icon_rgba, icon_width, icon_height) = decode_rgba(bytes)?;
    Ok(TrayIcon::from_rgba(icon_rgba, icon_width, icon_height)?)
}

/// One tray icon in both of its states: full colour for ON, greyscale for OFF.
///
/// Both variants are built once at startup from the same source bytes — the
/// OFF variant must always derive from the ORIGINAL pixels, never from an
/// already-processed icon, or repeated toggles would compound the transform.
#[derive(Clone)]
pub struct IconSet {
    pub color: TrayIcon,
    pub grey: TrayIcon,
}

impl IconSet {
    /// Build both variants from embedded PNG bytes, with a 1×1 transparent
    /// fallback on decode failure (same degradation `create_tray_icon` used
    /// for single icons — a blank tray icon beats a startup crash).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        match decode_rgba(bytes) {
            Ok((rgba, w, h)) => {
                let grey = greyscale_rgba(&rgba);
                Self {
                    color: TrayIcon::from_rgba(rgba, w, h).unwrap_or_else(|_| blank_icon()),
                    grey: TrayIcon::from_rgba(grey, w, h).unwrap_or_else(|_| blank_icon()),
                }
            }
            Err(_) => Self {
                color: blank_icon(),
                grey: blank_icon(),
            },
        }
    }

    /// The variant for the given enabled state.
    pub fn variant(&self, enabled: bool) -> &TrayIcon {
        if enabled {
            &self.color
        } else {
            &self.grey
        }
    }
}

/// Greyscale via BT.601 luminance (`0.299r + 0.587g + 0.114b`), alpha kept.
///
/// Chosen over alpha-fading for the OFF state: at 16px on a light taskbar a
/// faded icon all but disappears — indistinguishable from a dead tray — while
/// greyscale keeps full contrast and still reads instantly as "not active".
/// This is the only place that knows how the OFF variant is produced.
fn greyscale_rgba(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        let y = (299 * px[0] as u32 + 587 * px[1] as u32 + 114 * px[2] as u32) / 1000;
        let y = y as u8;
        px[0] = y;
        px[1] = y;
        px[2] = y;
        // px[3] (alpha) untouched — the icon's shape must not change.
    }
    out
}

/// 1×1 transparent icon — the never-fails fallback.
fn blank_icon() -> TrayIcon {
    TrayIcon::from_rgba(vec![0, 0, 0, 0], 1, 1).expect("a 1x1 RGBA buffer is always valid")
}

/// Decode embedded PNG bytes to raw RGBA.
fn decode_rgba(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let image = image::load_from_memory(bytes)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok((image.into_raw(), width, height))
}

/// Load a menu icon from embedded bytes
pub fn load_menu_icon(bytes: &[u8]) -> Option<muda::Icon> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    muda::Icon::from_rgba(rgba, width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greyscale_flattens_colour_and_keeps_alpha() {
        // One saturated red pixel, half-transparent.
        let rgba = vec![200u8, 40, 40, 128];
        let grey = greyscale_rgba(&rgba);
        assert_eq!(grey.len(), 4, "pixel count must not change");
        assert_eq!(grey[0], grey[1], "r == g after greyscale");
        assert_eq!(grey[1], grey[2], "g == b after greyscale");
        assert_eq!(
            grey[3], 128,
            "alpha must be untouched — the shape is the icon"
        );
        // BT.601: (299*200 + 587*40 + 114*40)/1000 = 87
        assert_eq!(grey[0], 87);
    }

    #[test]
    fn greyscale_keeps_contrast_between_light_and_dark() {
        // The reason greyscale was chosen over alpha-fade: a dark and a light
        // pixel must still differ afterwards, or the icon melts into the
        // taskbar and OFF reads as "tray died".
        let rgba = vec![
            240, 240, 240, 255, // near-white
            30, 30, 30, 255, // near-black
        ];
        let grey = greyscale_rgba(&rgba);
        assert!(grey[0] as i16 - grey[4] as i16 > 150, "contrast preserved");
    }

    #[test]
    fn icon_set_builds_from_real_embedded_bytes() {
        // The actual shipped icons must decode — a bad PNG here would silently
        // give every user a blank tray via the fallback.
        for bytes in [
            TELEX_ICON_BYTES,
            VNI_ICON_BYTES,
            NOM_ICON_BYTES,
            ENGLISH_ICON_BYTES,
            CUSTOM_ICON_BYTES,
            VIETNAMESE_ICON_BYTES,
        ] {
            let (rgba, w, h) = decode_rgba(bytes).expect("shipped icon must decode");
            assert!(
                w > 1 && h > 1,
                "shipped icon must not be the 1x1 fallback size"
            );
            assert_eq!(rgba.len(), (w * h * 4) as usize);
        }
    }
}
