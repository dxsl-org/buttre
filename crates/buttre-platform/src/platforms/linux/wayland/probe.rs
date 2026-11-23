//! Does the compositor offer `zwp_input_method_manager_v2` to ordinary
//! clients? wlroots compositors (sway, Hyprland, …) do; Mutter does not
//! implement it and KWin only offers v1 on the socket it hands the IME it
//! spawns — so this is the evidence that `buttre --ime` can serve the
//! session on its own (`backend_detect`).

use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

#[derive(Default)]
struct Globals {
    input_method_v2: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            if interface == "zwp_input_method_manager_v2" {
                state.input_method_v2 = true;
            }
        }
    }
}

/// One registry round trip on `WAYLAND_DISPLAY`. `false` on any failure
/// (no display, connection refused) — absence of evidence.
pub fn compositor_offers_input_method_v2() -> bool {
    // `WAYLAND_SOCKET` is a one-shot fd a compositor hands to a process it
    // spawned; `connect_to_env` would consume it. Never probe on it.
    if std::env::var_os("WAYLAND_SOCKET").is_some() {
        return false;
    }
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let mut queue = conn.new_event_queue::<Globals>();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut globals = Globals::default();
    queue.roundtrip(&mut globals).is_ok() && globals.input_method_v2
}
