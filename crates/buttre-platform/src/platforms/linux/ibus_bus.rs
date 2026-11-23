//! IBus private-bus connection, Factory protocol, and engine lifecycle.
//!
//! ibus-daemon runs its OWN message bus, separate from the session bus.
//! An engine component must (in this order — the daemon calls `CreateEngine`
//! the moment it sees the well-known name, so a name-first race would drop
//! that call):
//!
//! 1. connect to the private bus (address from `IBUS_ADDRESS` env or the
//!    address file under `~/.config/ibus/bus/`),
//! 2. serve `org.freedesktop.IBus.Factory` at `/org/freedesktop/IBus/Factory`,
//! 3. request the component name `org.freedesktop.IBus.buttre`.
//!
//! The daemon then calls `CreateEngine(<engine name>)` once per input context
//! and routes key events to the object path we return. The engine name picks
//! the method (`ibus_engines`, ADR-0005). Connecting to the session bus
//! instead (the pre-0.8 behavior) made the engine invisible to the daemon —
//! the original "typing dead on Linux" root cause.

use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use zbus::{dbus_interface, zvariant, ConnectionBuilder, ObjectServer};

use super::ibus::ButtreEngine;
use super::ibus_engines;
use super::learning_sync::{self, LearningWiring};
use super::macro_sync;
use buttre_core::state::macros::MacroStore;

// ============================================================================
// Private-bus address discovery
// ============================================================================

/// Resolve the IBus private bus address.
///
/// Order: `IBUS_ADDRESS` env (set by the daemon when it spawns us) → the
/// `IBUS_ADDRESS=` line of `~/.config/ibus/bus/<machine-id>-unix-<suffix>`.
/// Measured on IBus 1.5.29: the file lives under `~/.config`, the socket it
/// points to under `~/.cache/ibus/`.
pub fn resolve_ibus_address() -> Result<String> {
    if let Ok(addr) = std::env::var("IBUS_ADDRESS") {
        let addr = addr.trim().to_string();
        if !addr.is_empty() {
            return Ok(strip_guid(&addr));
        }
    }

    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("no XDG config directory"))?
        .join("ibus/bus");
    let machine_id = read_machine_id()?;

    // A Wayland session may also have DISPLAY set (Xwayland) — try the
    // Wayland-suffixed file first, it belongs to the live session.
    let mut candidates = Vec::new();
    if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
        candidates.push(dir.join(format!("{machine_id}-unix-{wayland}")));
    }
    if let Ok(display) = std::env::var("DISPLAY") {
        let num = display
            .trim_start_matches(':')
            .split('.')
            .next()
            .unwrap_or("0")
            .to_string();
        candidates.push(dir.join(format!("{machine_id}-unix-{num}")));
    }
    for path in &candidates {
        if let Some(addr) = parse_address_file(path) {
            return Ok(addr);
        }
    }

    // Single-session fallback: any address file in the bus directory.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(addr) = parse_address_file(&entry.path()) {
                return Ok(addr);
            }
        }
    }

    Err(anyhow!(
        "IBus bus address not found: IBUS_ADDRESS is unset and no address file \
         under {} — is ibus-daemon running?",
        dir.display()
    ))
}

fn read_machine_id() -> Result<String> {
    for path in ["/var/lib/dbus/machine-id", "/etc/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(path) {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return Ok(id);
            }
        }
    }
    Err(anyhow!("machine-id not found (dbus or systemd)"))
}

/// Extract the `IBUS_ADDRESS=` line from an ibus-daemon address file.
fn parse_address_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(addr) = line.strip_prefix("IBUS_ADDRESS=") {
            let addr = addr.trim();
            if !addr.is_empty() {
                return Some(strip_guid(addr));
            }
        }
    }
    None
}

/// Drop the `,guid=…` suffix — zbus's address parser may reject keys it
/// doesn't know, and the guid is only a connection-sharing optimization.
fn strip_guid(addr: &str) -> String {
    addr.split(",guid=").next().unwrap_or(addr).to_string()
}

// ============================================================================
// org.freedesktop.IBus.Factory
// ============================================================================

/// The daemon's entry point for engine instantiation: `CreateEngine(name)`
/// returns a fresh per-input-context engine object path. Without this
/// factory the daemon has no way to reach the engine at all.
struct ButtreFactory {
    engine_counter: u64,
    /// Shared with `macro_sync`'s watcher — one reload updates every live
    /// engine at once.
    macros: Arc<Mutex<MacroStore>>,
    /// `Settings::strict_spelling` mirror, kept current by the same
    /// `macro_sync` watcher — every engine's bridge consults it lazily per
    /// keystroke (`EngineBridge::set_strict_flag`).
    strict: Arc<AtomicBool>,
    /// `Settings::backspace_mode` (raw?) mirror on the same watcher
    /// (`EngineBridge::set_raw_backspace_flag`).
    raw_backspace: Arc<AtomicBool>,
    /// `Settings::use_preedit` mirror on the same watcher — each engine reads
    /// it per keystroke to choose the preedit vs commit-as-you-go model
    /// (gated on the client's surrounding-text capability).
    use_preedit: Arc<AtomicBool>,
    /// Personal-learning handles (`learning_sync`) — shared store, save
    /// channel, and the `learning_enabled` mirror each engine consults per
    /// keystroke (`sync_learning`).
    learning: LearningWiring,
}

#[dbus_interface(name = "org.freedesktop.IBus.Factory")]
impl ButtreFactory {
    async fn create_engine(
        &mut self,
        #[zbus(object_server)] server: &ObjectServer,
        engine_name: &str,
    ) -> zbus::fdo::Result<zvariant::OwnedObjectPath> {
        let Some(method) = ibus_engines::method_for_engine(engine_name) else {
            return Err(zbus::fdo::Error::Failed(format!(
                "unknown engine name: {engine_name}"
            )));
        };
        self.engine_counter += 1;
        let path = zvariant::OwnedObjectPath::try_from(format!(
            "/org/freedesktop/IBus/Engine/{}",
            self.engine_counter
        ))
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        let engine = ButtreEngine::new_for_factory(
            &method,
            self.macros.clone(),
            self.strict.clone(),
            self.raw_backspace.clone(),
            self.use_preedit.clone(),
            self.learning.clone(),
            path.clone(),
        );
        server
            .at(&path, engine)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        if let Err(e) = server.at(&path, EngineService { path: path.clone() }).await {
            // Don't leak the Engine interface if the Service fails to bind:
            // it has no Destroy handler of its own to reap it.
            let _ = server.remove::<ButtreEngine, _>(&path).await;
            return Err(zbus::fdo::Error::Failed(e.to_string()));
        }

        tracing::info!(
            "CreateEngine({engine_name}): serving {method} at {}",
            path.as_str()
        );
        Ok(path)
    }
}

/// `org.freedesktop.IBus.Service` — the daemon calls `Destroy` here when an
/// input context releases its engine; without it the object server would
/// leak one engine per context switch.
struct EngineService {
    path: zvariant::OwnedObjectPath,
}

#[dbus_interface(name = "org.freedesktop.IBus.Service")]
impl EngineService {
    async fn destroy(&self, #[zbus(object_server)] server: &ObjectServer) {
        if let Err(e) = server.remove::<ButtreEngine, _>(&self.path).await {
            tracing::warn!(
                "Destroy: engine removal at {} failed: {e}",
                self.path.as_str()
            );
        }
        // Removing the interface currently handling this call is safe —
        // zbus completes the in-flight invocation before dropping it.
        let _ = server.remove::<EngineService, _>(&self.path).await;
        tracing::debug!("Destroyed engine at {}", self.path.as_str());
    }
}

// ============================================================================
// Component entry point
// ============================================================================

/// Run the IBus engine component. Blocks forever; ibus-daemon owns this
/// process's lifecycle (it is spawned as `buttre --ibus` per the component
/// XML and killed by the daemon on shutdown/replace).
pub async fn run_engine() -> Result<()> {
    let addr = resolve_ibus_address()?;
    tracing::info!("Connecting to IBus private bus");

    // Shorthand/gõ tắt + setting mirrors: shared macro store and its own
    // watcher over macros.toml + settings.toml. The watcher also maintains a
    // `Settings::enabled` mirror for the backends that honor it; IBus does
    // not (off = another input source, ADR-0005), so that one is not kept.
    let macros = macro_sync::load_initial();
    let strict = macro_sync::load_initial_strict();
    let raw_backspace = macro_sync::load_initial_raw_backspace();
    let use_preedit = macro_sync::load_initial_use_preedit();
    macro_sync::spawn_watcher(
        macros.clone(),
        strict.clone(),
        use_preedit.clone(),
        Arc::new(AtomicBool::new(true)),
        raw_backspace.clone(),
    );

    // Personal learning: shared store + dedicated save-writer thread + its
    // own reload watcher. The daemon owns this process, so all three live
    // for the process lifetime (same contract as the watchers above).
    let (learning, learning_rx) = learning_sync::load_initial();
    learning_sync::spawn_writer(learning_rx);
    learning_sync::spawn_watcher(&learning);

    // ConnectionBuilder registers served objects before requesting names,
    // satisfying the factory-before-name sequence contract (module docs).
    // The connection must outlive the pending future below.
    let _connection = ConnectionBuilder::address(addr.as_str())?
        .serve_at(
            "/org/freedesktop/IBus/Factory",
            ButtreFactory {
                engine_counter: 0,
                macros,
                strict,
                raw_backspace,
                use_preedit,
                learning,
            },
        )?
        .name("org.freedesktop.IBus.buttre")?
        .build()
        .await?;

    tracing::info!("buttre registered with ibus-daemon (factory ready)");
    std::future::pending::<()>().await;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_guid_removes_suffix() {
        assert_eq!(
            strip_guid("unix:path=/home/u/.cache/ibus/dbus-abc,guid=7fe731"),
            "unix:path=/home/u/.cache/ibus/dbus-abc"
        );
        assert_eq!(strip_guid("unix:path=/tmp/x"), "unix:path=/tmp/x");
    }

    #[test]
    fn parse_address_file_extracts_ibus_address_line() {
        let dir = std::env::temp_dir().join("buttre-ibus-bus-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("addr");
        std::fs::write(
            &file,
            "# comment\nIBUS_ADDRESS=unix:path=/run/x,guid=deadbeef\nIBUS_DAEMON_PID=1\n",
        )
        .unwrap();
        assert_eq!(
            parse_address_file(&file).as_deref(),
            Some("unix:path=/run/x")
        );
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn parse_address_file_missing_returns_none() {
        assert!(parse_address_file(Path::new("/nonexistent/buttre-addr")).is_none());
    }
}
