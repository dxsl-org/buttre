//! Draw the Nôm candidate list into an ARGB8888 pixmap for a Wayland
//! `wl_shm` buffer. Pure: no Wayland objects, so the panel module only
//! handles surfaces and buffers.
//!
//! Pango does the text: its fontconfig fallback picks a font that has the
//! Han/Nôm glyph even when the UI font does not — the same lookup GTK and Qt
//! apps use, so the popup shows exactly the glyphs the app can show.

use crate::shared::candidates::CandidateView;
use pangocairo::{cairo, pango};

/// Candidates per page — number keys 1..=9 pick a slot on the current page.
pub(super) const PAGE_SIZE: usize = 9;

const FONT: &str = "Sans 13";
const PAD: f64 = 6.0;
const ROW_GAP: f64 = 2.0;
/// Breeze palette: neutral light colors that read fine on any compositor.
const BG: (f64, f64, f64) = (0.988, 0.988, 0.988);
const BORDER: (f64, f64, f64) = (0.737, 0.745, 0.753);
const TEXT: (f64, f64, f64) = (0.137, 0.149, 0.161);
const HIGHLIGHT: (f64, f64, f64) = (0.239, 0.682, 0.914);
const HIGHLIGHT_TEXT: (f64, f64, f64) = (1.0, 1.0, 1.0);

/// A rendered popup: premultiplied ARGB32, native endian — byte-identical to
/// `wl_shm::Format::Argb8888` on every little-endian host. `width`/`height`
/// are buffer pixels; the logical size is that divided by `scale`.
pub(super) struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub scale: i32,
    pub stride: i32,
    pub data: Vec<u8>,
}

/// Render the page holding `cursor` (a GLOBAL index into `items`) as a
/// vertical list `1. 𡗶 (trời)`, the cursor row highlighted, plus a
/// `‹ 2/5 ›` footer when there is more than one page, at `scale`× the
/// logical size. `None` when `items` is empty or cairo cannot allocate.
pub(super) fn render(items: &[CandidateView], cursor: usize, scale: i32) -> Option<Pixmap> {
    if items.is_empty() {
        return None;
    }
    let cursor = cursor.min(items.len() - 1);
    let start = (cursor / PAGE_SIZE) * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(items.len());
    let pages = items.len().div_ceil(PAGE_SIZE);

    let mut lines: Vec<String> = items[start..end]
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, c.display))
        .collect();
    if pages > 1 {
        lines.push(format!("‹ {}/{} ›", start / PAGE_SIZE + 1, pages));
    }

    // Measure on a scratch surface: the layout's size decides the real one.
    let scratch = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).ok()?;
    let font = pango::FontDescription::from_string(FONT);
    let layouts: Vec<pango::Layout> = {
        let cr = cairo::Context::new(&scratch).ok()?;
        lines
            .iter()
            .map(|text| {
                let layout = pangocairo::create_layout(&cr);
                layout.set_font_description(Some(&font));
                layout.set_text(text);
                layout
            })
            .collect()
    };
    let sizes: Vec<(i32, i32)> = layouts.iter().map(|l| l.pixel_size()).collect();
    let text_w = sizes.iter().map(|s| s.0).max().unwrap_or(0) as f64;
    let row_h: Vec<f64> = sizes.iter().map(|s| s.1 as f64 + ROW_GAP * 2.0).collect();
    let width = (text_w + PAD * 4.0).ceil() as i32;
    let height = (row_h.iter().sum::<f64>() + PAD * 2.0).ceil() as i32;

    // Laid out in logical pixels, drawn at `scale`× so a HiDPI output gets
    // sharp glyphs (the surface declares the same buffer scale).
    let scale = scale.max(1);
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width * scale, height * scale).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        cr.scale(f64::from(scale), f64::from(scale));
        set_rgb(&cr, BG);
        cr.paint().ok()?;
        set_rgb(&cr, BORDER);
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, width as f64 - 1.0, height as f64 - 1.0);
        cr.stroke().ok()?;

        let highlighted = cursor - start;
        let mut y = PAD;
        for (i, layout) in layouts.iter().enumerate() {
            if i == highlighted {
                set_rgb(&cr, HIGHLIGHT);
                cr.rectangle(PAD, y, width as f64 - PAD * 2.0, row_h[i]);
                cr.fill().ok()?;
                set_rgb(&cr, HIGHLIGHT_TEXT);
            } else {
                set_rgb(&cr, TEXT);
            }
            cr.move_to(PAD * 2.0, y + ROW_GAP);
            // Re-bind the measured layout to this context's font options.
            pangocairo::update_layout(&cr, layout);
            pangocairo::show_layout(&cr, layout);
            y += row_h[i];
        }
    }
    surface.flush();
    let stride = surface.stride();
    let data = surface.take_data().ok()?.to_vec();
    Some(Pixmap {
        width: width * scale,
        height: height * scale,
        scale,
        stride,
        data,
    })
}

fn set_rgb(cr: &cairo::Context, (r, g, b): (f64, f64, f64)) {
    cr.set_source_rgb(r, g, b);
}
