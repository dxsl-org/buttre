//! Nôm candidate popup for the KWin backend ([`super::v1`]).
//!
//! `zwp_input_method_v1` has no candidate requests, so the list is a surface
//! of our own: a `zwp_input_panel_surface_v1` in OVERLAY mode — the role
//! fcitx5 uses on Plasma. KWin places an overlay panel directly under the
//! text cursor of the focused field (flipping above it near the screen
//! bottom) and never gives it focus, so keys keep flowing to the grab.
//!
//! Lifecycle: one surface, created on the first list; showing = attaching a
//! freshly rendered buffer, hiding = attaching none (unmaps; KWin hides the
//! window). Each buffer is used once and destroyed when KWin releases it.

use super::candidate_render::{self, Pixmap};
use super::v1::ImeV1State;
use crate::shared::candidates::CandidateView;
use std::io::Write;
use std::os::fd::AsFd;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::input_method::zv1::client::{
    zwp_input_panel_surface_v1, zwp_input_panel_v1,
};

#[derive(Default)]
pub(super) struct CandidatePanel {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    input_panel: Option<zwp_input_panel_v1::ZwpInputPanelV1>,
    surface: Option<Surface>,
    /// Whether a buffer is attached — lets `hide` skip a pointless commit.
    shown: bool,
}

struct Surface {
    wl: wl_surface::WlSurface,
    /// Kept alive for the role; dropping the proxy does not destroy it.
    _panel: zwp_input_panel_surface_v1::ZwpInputPanelSurfaceV1,
}

impl CandidatePanel {
    /// Bind the globals the popup needs. Returns `true` when `interface` was
    /// one of them. Without all three the popup is skipped silently and Nôm
    /// still types (Space/digits pick candidates blind).
    pub(super) fn bind_global(
        &mut self,
        registry: &wl_registry::WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<ImeV1State>,
    ) -> bool {
        match interface {
            "wl_compositor" => {
                self.compositor = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "wl_shm" => self.shm = Some(registry.bind(name, 1, qh, ())),
            "zwp_input_panel_v1" => self.input_panel = Some(registry.bind(name, 1, qh, ())),
            _ => return false,
        }
        true
    }

    /// Show (or repaint) the page holding `cursor`.
    pub(super) fn show(
        &mut self,
        items: &[CandidateView],
        cursor: usize,
        qh: &QueueHandle<ImeV1State>,
    ) {
        let Some(pixmap) = candidate_render::render(items, cursor) else {
            return self.hide();
        };
        let Some(surface) = self.surface(qh) else {
            return;
        };
        let wl = surface.wl.clone();
        let Some(buffer) = self.buffer(&pixmap, qh) else {
            return;
        };
        wl.attach(Some(&buffer), 0, 0);
        wl.damage(0, 0, pixmap.width, pixmap.height);
        wl.commit();
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

    /// The panel surface, created on first use.
    fn surface(&mut self, qh: &QueueHandle<ImeV1State>) -> Option<&Surface> {
        if self.surface.is_none() {
            let (Some(compositor), Some(input_panel)) = (&self.compositor, &self.input_panel)
            else {
                tracing::debug!("candidate popup unavailable (no wl_compositor / input panel)");
                return None;
            };
            let wl = compositor.create_surface(qh, ());
            let panel = input_panel.get_input_panel_surface(&wl, qh, ());
            panel.set_overlay_panel();
            self.surface = Some(Surface { wl, _panel: panel });
        }
        self.surface.as_ref()
    }

    /// A one-shot `wl_buffer` holding `pixmap`, backed by an unlinked temp
    /// file (the pool is destroyed right away; the buffer keeps the memory).
    fn buffer(&self, pixmap: &Pixmap, qh: &QueueHandle<ImeV1State>) -> Option<wl_buffer::WlBuffer> {
        let shm = self.shm.as_ref()?;
        let size = pixmap.data.len();
        let file = std::env::var_os("XDG_RUNTIME_DIR")
            .and_then(|dir| tempfile::tempfile_in(dir).ok())
            .or_else(|| tempfile::tempfile().ok());
        let Some(mut file) = file else {
            tracing::warn!("candidate popup: cannot create a shm file");
            return None;
        };
        if let Err(e) = file.write_all(&pixmap.data) {
            tracing::warn!("candidate popup: shm write failed: {e}");
            return None;
        }
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            pixmap.width,
            pixmap.height,
            pixmap.stride,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        pool.destroy();
        Some(buffer)
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for ImeV1State {
    fn event(
        _state: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // One-shot buffers: once KWin is done reading, nothing reuses it.
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

wayland_client::delegate_noop!(ImeV1State: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(ImeV1State: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(ImeV1State: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(ImeV1State: ignore wl_surface::WlSurface);
wayland_client::delegate_noop!(ImeV1State: ignore zwp_input_panel_v1::ZwpInputPanelV1);
wayland_client::delegate_noop!(ImeV1State: ignore zwp_input_panel_surface_v1::ZwpInputPanelSurfaceV1);
