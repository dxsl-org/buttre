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
use super::ImeState;
use crate::shared::candidates::CandidateView;
use std::io::Write;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface,
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
    scales: OutputScales,
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
            "wl_output" => self.scales.bind(registry, name, version, qh),
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
        let Some(pixmap) = candidate_render::render(items, cursor, self.scales.max()) else {
            return self.hide();
        };
        let Some(surface) = self.surface(qh) else {
            return;
        };
        let wl = surface.wl.clone();
        let Some(buffer) = self
            .shm
            .as_ref()
            .and_then(|shm| one_shot_buffer(shm, &pixmap, qh))
        else {
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
}

/// A one-shot `wl_buffer` holding `pixmap`, backed by an unlinked temp file
/// (the pool is destroyed right away; the buffer keeps the memory). Shared
/// by both popup backends; their `wl_buffer` dispatch destroys it on
/// `release`.
pub(super) fn one_shot_buffer<D>(
    shm: &wl_shm::WlShm,
    pixmap: &Pixmap,
    qh: &QueueHandle<D>,
) -> Option<wl_buffer::WlBuffer>
where
    D: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
{
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

/// Attach `buffer` (rendered as `pixmap`) to `wl` at the pixmap's scale.
pub(super) fn present(wl: &wl_surface::WlSurface, buffer: &wl_buffer::WlBuffer, pixmap: &Pixmap) {
    wl.set_buffer_scale(pixmap.scale);
    wl.attach(Some(buffer), 0, 0);
    wl.damage(
        0,
        0,
        pixmap.width / pixmap.scale,
        pixmap.height / pixmap.scale,
    );
    wl.commit();
}

/// Integer scale of every `wl_output`, kept current by its `scale` event.
/// The popup renders at the LARGEST one: neither input-method protocol says
/// which output the popup lands on, and a sharp popup downscaled on a 1×
/// screen beats a blurry one upscaled on a 2× screen (Hyprland at scale 2
/// also placed the oversized 1× list off-screen). Unplugged outputs keep
/// their slot — harmless, the scale only errs high.
#[derive(Default)]
pub(super) struct OutputScales(Vec<Arc<AtomicI32>>);

impl OutputScales {
    pub(super) fn bind<D>(
        &mut self,
        registry: &wl_registry::WlRegistry,
        name: u32,
        version: u32,
        qh: &QueueHandle<D>,
    ) where
        D: Dispatch<wl_output::WlOutput, Arc<AtomicI32>> + 'static,
    {
        // `scale` arrived in wl_output v2.
        if version < 2 {
            return;
        }
        let slot = Arc::new(AtomicI32::new(1));
        let _output: wl_output::WlOutput = registry.bind(name, 2, qh, slot.clone());
        self.0.push(slot);
    }

    pub(super) fn max(&self) -> i32 {
        self.0
            .iter()
            .map(|s| s.load(Ordering::Relaxed))
            .max()
            .unwrap_or(1)
    }
}

fn on_output_event(slot: &AtomicI32, event: wl_output::Event) {
    if let wl_output::Event::Scale { factor } = event {
        slot.store(factor.max(1), Ordering::Relaxed);
    }
}

impl Dispatch<wl_output::WlOutput, Arc<AtomicI32>> for ImeV1State {
    fn event(
        _state: &mut Self,
        _output: &wl_output::WlOutput,
        event: wl_output::Event,
        slot: &Arc<AtomicI32>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        on_output_event(slot, event);
    }
}

impl Dispatch<wl_output::WlOutput, Arc<AtomicI32>> for ImeState {
    fn event(
        _state: &mut Self,
        _output: &wl_output::WlOutput,
        event: wl_output::Event,
        slot: &Arc<AtomicI32>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        on_output_event(slot, event);
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
