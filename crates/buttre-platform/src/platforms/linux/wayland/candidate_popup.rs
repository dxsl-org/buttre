//! Nôm candidate popup for the `zwp_input_method_v2` backend (sway,
//! Hyprland, niri, labwc, COSMIC).
//!
//! The protocol's own `zwp_input_popup_surface_v2` — the role fcitx5 uses on
//! these compositors. The compositor places it beside the focused field's
//! text cursor, shows it only while the input method is active, and never
//! gives it keyboard focus, so keys keep flowing to the grab. A compositor
//! that does not draw input popups simply shows nothing; Space and 1–9 still
//! pick candidates.
//!
//! Same lifecycle as the KWin panel ([`super::candidate_panel`]): one
//! surface, created on the first list; showing = attaching a freshly
//! rendered buffer, hiding = attaching none.

use super::candidate_panel::{one_shot_buffer, present, OutputScales};
use super::candidate_render;
use super::ImeState;
use crate::shared::candidates::CandidateView;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_v2, zwp_input_popup_surface_v2,
};

#[derive(Default)]
pub(super) struct CandidatePopup {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    scales: OutputScales,
    surface: Option<Surface>,
    /// Whether a buffer is attached — lets `hide` skip a pointless commit.
    shown: bool,
}

struct Surface {
    wl: wl_surface::WlSurface,
    /// Kept alive for the role; dropping the proxy does not destroy it.
    _popup: zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2,
}

impl CandidatePopup {
    /// Bind the globals the popup needs. Returns `true` when `interface` was
    /// one of them.
    pub(super) fn bind_global(
        &mut self,
        registry: &wl_registry::WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<ImeState>,
    ) -> bool {
        match interface {
            "wl_compositor" => {
                self.compositor = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "wl_shm" => self.shm = Some(registry.bind(name, 1, qh, ())),
            "wl_output" => self.scales.bind(registry, name, version, qh),
            _ => return false,
        }
        true
    }

    /// Show (or repaint) the page holding `cursor`.
    pub(super) fn show(
        &mut self,
        im: &zwp_input_method_v2::ZwpInputMethodV2,
        items: &[CandidateView],
        cursor: usize,
        qh: &QueueHandle<ImeState>,
    ) {
        let Some(pixmap) = candidate_render::render(items, cursor, self.scales.max()) else {
            return self.hide();
        };
        let Some(shm) = self.shm.clone() else {
            return;
        };
        let Some(surface) = self.surface(im, qh) else {
            return;
        };
        let wl = surface.wl.clone();
        let Some(buffer) = one_shot_buffer(&shm, &pixmap, qh) else {
            return;
        };
        present(&wl, &buffer, &pixmap);
        self.shown = true;
    }

    pub(super) fn hide(&mut self) {
        if !self.shown {
            return;
        }
        if let Some(surface) = &self.surface {
            surface.wl.attach(None, 0, 0);
            surface.wl.commit();
        }
        self.shown = false;
    }

    /// The popup surface, created on first use.
    fn surface(
        &mut self,
        im: &zwp_input_method_v2::ZwpInputMethodV2,
        qh: &QueueHandle<ImeState>,
    ) -> Option<&Surface> {
        if self.surface.is_none() {
            let Some(compositor) = &self.compositor else {
                tracing::debug!("candidate popup unavailable (no wl_compositor)");
                return None;
            };
            let wl = compositor.create_surface(qh, ());
            let popup = im.get_input_popup_surface(&wl, qh, ());
            self.surface = Some(Surface { wl, _popup: popup });
        }
        self.surface.as_ref()
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for ImeState {
    fn event(
        _state: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // One-shot buffers: once the compositor is done reading, nothing
        // reuses it.
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

wayland_client::delegate_noop!(ImeState: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(ImeState: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(ImeState: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(ImeState: ignore wl_surface::WlSurface);
// `text_input_rectangle` only matters to a client positioning its own
// popup; the compositor already places this one at the cursor.
wayland_client::delegate_noop!(ImeState: ignore zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2);
