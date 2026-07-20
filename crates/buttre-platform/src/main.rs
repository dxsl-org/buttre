#![windows_subsystem = "windows"]

//! # buttre Platform - Main Entry Point
//!
//! ## Data Flow:
//! ```text
//! User Input → Hook/TSF Backend → buttre-keyboard → buttre-engine → Action → Output
//! ```
//!
//! ## This file orchestrates:
//! 1. Load settings & build UI (tray menu)
//! 2. Initialize AppState (manages current method, enabled state)
//! 3. Start Platform Backend (Hook on Windows, IBus on Linux)
//! 4. Run event loop (handle menu clicks, hotkeys)
//!
//! ## Backend calls buttre-keyboard DIRECTLY (NOT via buttre-core::Engine)

use anyhow::Result;
use buttre_core::hotkey::{ButtreHotkeyManager, HotkeyAction};
use buttre_core::keyboard::BackspaceMode;
use buttre_core::state::learning::{LearningFile, LearningStore};
use buttre_core::state::macros::MacroStore;
use buttre_core::state::{Settings, StateObserver};
use buttre_core::AppState;
use buttre_core::Keyboard;
use buttre_platform::shared::observers::{KeyboardObserver, MainUICallback, UIEvent, UIObserver};
use buttre_platform::shared::ui::{build_menu, create_tray_icon, helpers, MenuItems};
use buttre_platform::shared::{pipe_server, KeyboardManager, MethodRegistry};
use buttre_platform::{platform_name, Backend, PlatformBackend};
use log::{error, info, warn};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

/// Apply the backspace-deletion mode to whatever `Keyboard` is currently
/// loaded (event-sourcing-completion Phase 4). A no-op in English mode
/// (`keyboard` is `None`) — nothing to set it on.
fn apply_backspace_mode(keyboard: &Arc<RwLock<Option<Keyboard>>>, mode: BackspaceMode) {
    if let Ok(mut guard) = keyboard.write() {
        if let Some(kb) = guard.as_mut() {
            kb.set_backspace_mode(mode);
        }
    } else {
        error!("apply_backspace_mode: keyboard lock poisoned, skipping");
    }
}

/// Re-applies `backspace_mode` after every input-method switch.
/// `KeyboardObserver` REPLACES the `Keyboard` instance behind the shared
/// handle on method change (`Keyboard::new` always starts at the engine
/// default, `BackspaceMode::Grapheme`), which would otherwise silently drop
/// the user's persisted raw-backspace preference. Must be registered AFTER
/// `KeyboardObserver` so the new instance already exists when this fires.
///
/// `mode` is a `Mutex`, not a plain field: before the config window (P2),
/// backspace mode could only ever be set once at startup (no tray toggle
/// existed), so a fixed value was sufficient. Now the settings-file watcher
/// in the event loop below can update it live via `set_mode`, and this
/// observer must read the CURRENT value on every subsequent method switch,
/// not whatever was current at construction time.
struct BackspaceModeObserver {
    keyboard: Arc<RwLock<Option<Keyboard>>>,
    mode: Mutex<BackspaceMode>,
}

impl BackspaceModeObserver {
    fn set_mode(&self, mode: BackspaceMode) {
        *self.mode.lock().unwrap() = mode;
    }
}

impl StateObserver for BackspaceModeObserver {
    fn on_method_changed(&self, _method: &str, _enabled: bool) {
        apply_backspace_mode(&self.keyboard, *self.mode.lock().unwrap());
    }

    fn on_settings_changed(&self, _settings: &Settings) {}
}

/// Route the `ToggleLastWord` hotkey to the Hook backend's delivery path
/// (event-sourcing-completion Phase 4). Hook multiword backend only — see
/// `hook.rs` for the focus guard and chord-exemption CRITICALs this depends
/// on. TSF is deferred (scope note, phase-04-user-controls.md): TSF's own
/// `Keyboard` instances live inside `vietnamese_engine.rs` and never touch
/// this `keyboard` handle, so its window is always empty here — this no-ops
/// safely for that backend too, with no extra branching needed. Also a safe
/// no-op on non-Windows platforms (not yet implemented there).
#[cfg(platform_windows)]
fn dispatch_toggle_last_word(keyboard: &Arc<RwLock<Option<Keyboard>>>) {
    buttre_platform::platforms::windows::hook::dispatch_toggle_last_word(keyboard);
}

#[cfg(not(platform_windows))]
fn dispatch_toggle_last_word(_keyboard: &Arc<RwLock<Option<Keyboard>>>) {}

/// Debounce successive personal-learning save requests down to the LATEST
/// snapshot only (event-sourcing-completion Phase 5, red-team C3): a
/// snapshot is the full current store state, not a delta, so replaying every
/// intermediate one queued since the last poll is wasted disk I/O — keeping
/// only the last item is a lossless debounce. Non-blocking: returns `None`
/// immediately once the channel is empty. A disconnected sender (every
/// `Keyboard` dropped, e.g. mid-shutdown) is treated the same as "nothing
/// new" — never an error worth logging on this poll path.
fn drain_latest_learning_save(rx: &mpsc::Receiver<LearningFile>) -> Option<LearningFile> {
    let mut latest = None;
    while let Ok(file) = rx.try_recv() {
        latest = Some(file);
    }
    latest
}

/// Watch learning.toml's directory for on-disk changes to the file (tray →
/// "Từ đã học" → user edits and saves). Returns the watcher handle — dropping
/// it stops the watch, so the caller must keep it alive for the app's
/// lifetime. `None` (with a log) when the path can't be resolved or the
/// watch can't be established: hand-edits then simply require a restart,
/// never an error.
fn watch_learning_file(tx: mpsc::Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let path = match LearningStore::get_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("learning.toml path unresolved, hand-edit reload disabled: {e:?}");
            return None;
        }
    };
    let dir = path.parent()?.to_path_buf();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("cannot create {dir:?}, hand-edit reload disabled: {e:?}");
        return None;
    }
    let file_name = path.file_name()?.to_os_string();
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event
                    .paths
                    .iter()
                    .any(|p| p.file_name() == Some(file_name.as_os_str()))
                {
                    let _ = tx.send(());
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("learning.toml watcher failed, hand-edit reload disabled: {e:?}");
                return None;
            }
        };
    // Watch the DIRECTORY, not the file: `write_atomic`'s rename replaces
    // the file node, which breaks per-file watches on some backends.
    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        warn!("learning.toml watch failed, hand-edit reload disabled: {e:?}");
        return None;
    }
    Some(watcher)
}

/// Watch macros.toml's directory for on-disk changes (config window / hand
/// edit) — mirrors `watch_learning_file` exactly, same directory, different
/// filename. Simpler than the learning watcher: nothing at the TYPING layer
/// ever writes `macros.toml` (see `buttre_core::state::macros`'s module
/// doc), so there is no own-write suppression concern for THAT writer.
/// The config window's "Mở tệp gốc" (`buttre_config::open_in_editor`,
/// seed-if-missing) is the sole
/// exception — it fires this same watcher, but reloading an unchanged
/// (still-empty) file is an idempotent no-op, so no suppression is needed
/// there either.
fn watch_macros_file(tx: mpsc::Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let path = match MacroStore::get_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("macros.toml path unresolved, hand-edit reload disabled: {e:?}");
            return None;
        }
    };
    let dir = path.parent()?.to_path_buf();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("cannot create {dir:?}, hand-edit reload disabled: {e:?}");
        return None;
    }
    let file_name = path.file_name()?.to_os_string();
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event
                    .paths
                    .iter()
                    .any(|p| p.file_name() == Some(file_name.as_os_str()))
                {
                    let _ = tx.send(());
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("macros.toml watcher failed, hand-edit reload disabled: {e:?}");
                return None;
            }
        };
    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        warn!("macros.toml watch failed, hand-edit reload disabled: {e:?}");
        return None;
    }
    Some(watcher)
}

/// Watch settings.toml's directory for on-disk changes (the config window's
/// "Lưu", or a hand-edit) — same shape as `watch_learning_file`/
/// `watch_macros_file`. See the call site's comment for how the reader
/// tells its own writes apart from a genuine external change.
fn watch_settings_file(tx: mpsc::Sender<()>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let path = match Settings::get_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("settings.toml path unresolved, hand-edit reload disabled: {e:?}");
            return None;
        }
    };
    let dir = path.parent()?.to_path_buf();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("cannot create {dir:?}, hand-edit reload disabled: {e:?}");
        return None;
    }
    let file_name = path.file_name()?.to_os_string();
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event
                    .paths
                    .iter()
                    .any(|p| p.file_name() == Some(file_name.as_os_str()))
                {
                    let _ = tx.send(());
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                warn!("settings.toml watcher failed, hand-edit reload disabled: {e:?}");
                return None;
            }
        };
    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        warn!("settings.toml watch failed, hand-edit reload disabled: {e:?}");
        return None;
    }
    Some(watcher)
}

fn main() -> Result<()> {
    // Initialize tracing (handles both log crate and tracing crate)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Engine modes (Linux) — must branch BEFORE the single-instance lock
    // (engine processes coexist with the user's tray instance) and before
    // any UI/winit setup (engines are headless).
    //
    // `--ibus`: the IBus component, spawned by ibus-daemon per the XML.
    // `--ime`:  auto-detect — Wayland-native zwp_input_method_v2 on
    //           compositors that support it (sway/Hyprland/KDE, launched
    //           from the compositor config), IBus fallback otherwise.
    #[cfg(platform_linux)]
    {
        if std::env::args().any(|arg| arg == "--ibus") {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            return rt.block_on(buttre_platform::platforms::linux::ibus_bus::run_engine());
        }
        if std::env::args().any(|arg| arg == "--ime") {
            return buttre_platform::platforms::linux::run_engine_auto();
        }
    }

    // `--config`: launch the native Slint config window as a separate
    // process sharing this same binary — mirrors the `--ibus`/`--ime`
    // dispatch above. Checked BEFORE the tray's single-instance lock below:
    // the config window has its OWN single-instance lock (`buttre-config`'s
    // `run()`), independent of whether the tray is running, so a user can
    // open "Cấu hình…" while the tray keeps typing live. Must never run
    // alongside the tray's own winit-0.29 event loop in the same process —
    // see `buttre-config`'s crate doc for why the two winit versions can
    // never coexist.
    if std::env::args().any(|arg| arg == "--config") {
        return buttre_config::run();
    }

    // Single instance check
    let instance = single_instance::SingleInstance::new("buttre")
        .map_err(|e| anyhow::anyhow!("Failed to create single instance lock: {}", e))?;

    if !instance.is_single() {
        error!("Another instance of buttre is already running. Exiting.");
        std::process::exit(0);
    }

    // Initialize Method Registry
    info!("Initializing method registry...");
    let method_registry = MethodRegistry::new();
    info!(
        "Registered {} input methods",
        method_registry.get_all().len()
    );
    for method in method_registry.get_all() {
        info!("  - {} ({})", method.name, method.id);
    }

    // Load settings
    let mut settings = Settings::load();
    info!("Loaded settings: {:?}", settings);

    // Load available input methods (built-in + custom)
    use buttre_core::vietnamese::config_loader::{ConfigLoader, MethodMetadata};

    // ConfigLoader manual fallback to built-ins if failure
    let all_methods = ConfigLoader::list_methods_with_metadata().unwrap_or_else(|e| {
        error!("Failed to list methods: {:?}", e);
        vec![
            MethodMetadata {
                id: "telex".to_string(),
                name: "Telex".to_string(),
                description: "Built-in Telex".to_string(),
                version: "1.0.0".to_string(),
                author: "buttre".to_string(),
                icon: None,
                is_builtin: true,
            },
            MethodMetadata {
                id: "vni".to_string(),
                name: "VNI".to_string(),
                description: "Built-in VNI".to_string(),
                version: "1.0.0".to_string(),
                author: "buttre".to_string(),
                icon: None,
                is_builtin: true,
            },
            MethodMetadata {
                id: "nom".to_string(),
                name: "Chữ Nôm".to_string(),
                description: "Built-in Nôm".to_string(),
                version: "1.0.0".to_string(),
                author: "buttre".to_string(),
                icon: None,
                is_builtin: true,
            },
        ]
    });

    // Validate input method (fallback to English if method not found)
    let is_valid_method = match settings.input_method.as_str() {
        "english" => true,
        method_id => all_methods.iter().any(|m| m.id == method_id),
    };

    if !is_valid_method {
        warn!(
            "Input method '{}' not found, falling back to English",
            settings.input_method
        );
        settings.input_method = "english".to_string();
        if let Err(e) = settings.save() {
            error!("Failed to save settings: {:?}", e);
        }
    }

    let event_loop = EventLoop::new()?;

    // We need a hidden window for the event loop to work properly on some platforms/configs
    use winit::window::WindowBuilder;
    let _window = WindowBuilder::new()
        .with_visible(false)
        .build(&event_loop)?;

    // --- Menu Setup ---
    // Build menu from registry
    let (menu, menu_items) = build_menu(&settings, &method_registry);

    // Extract menu items for event handling
    let MenuItems {
        english_item,
        chu_viet_menu,
        telex_item,
        vni_item,
        nom_item,
        custom_items,
        cau_hinh_item,
        thoat_item,
        ..
    } = menu_items;

    // Re-apply autostart registration while the setting is on: the exe path
    // may have changed since it was registered (update/move), and re-writing
    // the same entry is idempotent. Failure is a warning, never fatal.
    if settings.startup {
        if let Err(e) = buttre_autostart::set_enabled(true) {
            warn!("autostart re-registration failed: {e:?}");
        }
    }

    // --- Tray Setup ---
    // update_tray_icon in helpers handles custom_items with MethodMetadata now
    let (mut _tray_icon, telex_icon, vni_icon, english_icon, nom_icon, custom_icon) =
        create_tray_icon(&menu, &settings, &custom_items)?;

    // --- buttre Keyboard Setup ---
    // Arc-shared: the `KeyboardObserver` drives it on method switches, and
    // the event loop below drives it directly for the live "Học thông minh"
    // toggle and the learning.toml external-edit reload.
    let keyboard_manager = Arc::new(KeyboardManager::new()?);

    // Personal learning (event-sourcing-completion Phase 5): store + save
    // channel always exist (the tray can enable learning at runtime), but
    // WIRING them into keyboards is gated on `Settings::learning_enabled` —
    // unwired, no saves are ever sent and no snapshot is ever consulted, so
    // TYPING is byte-identical to pre-Phase-5 behavior. (The notify watcher
    // below does run regardless — a deliberate, typing-invisible delta so a
    // later tray enable needs no lazy watcher setup.) Disk is only read when
    // learning is actually on; the enable-toggle path does its own fresh
    // `load()`. Wired BEFORE `set_method` below so the FIRST keyboard
    // instance already has it — see `Keyboard::set_learning`'s doc on why
    // the initial hand-off matters.
    let mut learning_enabled = settings.learning_enabled;
    let learning_store = Arc::new(Mutex::new(if learning_enabled {
        LearningStore::load()
    } else {
        LearningStore::default()
    }));
    let (learning_tx, learning_save_rx) = mpsc::channel::<LearningFile>();
    if learning_enabled {
        keyboard_manager.set_learning(learning_store.clone(), learning_tx.clone());
    }

    // Watch learning.toml so hand-edits (tray → "Từ đã học") apply live.
    // The receiver is drained in the event loop; `last_own_save` suppresses
    // the events triggered by our own `write_atomic` calls below, and
    // `learning_reload_pending` carries a suppressed-but-real external edit
    // over to a later iteration instead of dropping it.
    let (learning_file_tx, learning_file_rx) = mpsc::channel::<()>();
    let _learning_watcher = watch_learning_file(learning_file_tx);
    let mut last_own_save = std::time::Instant::now();
    let mut learning_reload_pending = false;

    // Shorthand/gõ tắt (ADR-0001 — a SEPARATE mechanism from personal
    // learning, never merged into learning.toml). Simpler wiring than
    // learning: read-only at the keyboard layer, so no save channel and no
    // own-write suppression on the watcher — every fire is a real external
    // edit (config window in a later phase, or a hand-edit today).
    let mut shorthand_enabled = settings.shorthand;
    let macros_store = Arc::new(Mutex::new(if shorthand_enabled {
        MacroStore::load()
    } else {
        MacroStore::default()
    }));
    if shorthand_enabled {
        keyboard_manager.set_macros(macros_store.clone());
    }
    let (macros_file_tx, macros_file_rx) = mpsc::channel::<()>();
    let _macros_watcher = watch_macros_file(macros_file_tx);

    // Strict-spelling control ("Kiểm soát gắt gao chính tả tiếng Việt") —
    // remembered by the manager and re-applied on every method switch, so
    // wiring it BEFORE `set_method` below mirrors the learning/macros order.
    keyboard_manager.set_strict_spelling(settings.strict_spelling);

    // Apply initial settings to keyboard
    if let Err(e) = keyboard_manager.set_method(&settings.input_method) {
        error!("Failed to set initial input method: {:?}", e);
    }

    let keyboard = keyboard_manager.get_keyboard();

    // Apply the persisted backspace-deletion mode (event-sourcing-completion
    // Phase 4). `Keyboard::new` always starts at the engine default
    // (`BackspaceMode::Grapheme`) — the platform layer is the only place
    // that knows `Settings::backspace_mode`, so it must apply it explicitly,
    // both now and after every future method switch (see
    // `BackspaceModeObserver`, registered below).
    let backspace_mode = BackspaceMode::from_settings_str(&settings.backspace_mode);
    apply_backspace_mode(&keyboard, backspace_mode);

    // --- Platform Backend Setup ---
    let mut backend = Backend::new()?;
    backend.init(keyboard.clone())?;

    // ============================================================================
    // ARCHITECTURE NOTE: backend.set_enabled() is NOT needed anymore!
    // ============================================================================
    // Old design (WRONG):
    //   backend.set_enabled(settings.input_method != "english");
    //   → This set VIETNAMESE_ENABLED flag, which got out of sync
    //   → When user selected VNI from menu, keyboard loaded but flag stayed false
    //   → Result: VNI didn't work!
    //
    // New design (CORRECT):
    //   - Backend shares KEYBOARD Arc with KeyboardManager
    //   - KeyboardManager.set_method() updates KEYBOARD directly
    //   - Hook checks KEYBOARD.is_some() (not a separate flag!)
    //   - Everything syncs automatically via shared Arc
    //   - No need to call set_enabled() at all!
    //
    // The line below is commented out for documentation:
    // backend.set_enabled(settings.input_method != "english");  // ← NOT NEEDED!
    // ============================================================================

    let backend = Arc::new(backend);
    info!("Platform backend initialized: {}", platform_name());

    // --- Start Pipe Server for TSF ---
    let pipe_keyboard = keyboard.clone();
    std::thread::spawn(move || {
        if let Err(e) = pipe_server::run_pipe_server(pipe_keyboard) {
            error!("Pipe server error: {:?}", e);
        }
    });

    // --- Hotkey Setup ---
    let mut hotkey_manager = ButtreHotkeyManager::new().expect("Failed to create hotkey manager");

    // Register custom hotkeys (Ctrl+Shift+4..0) based on menu items count
    if let Err(e) = hotkey_manager.register_custom_methods(custom_items.len()) {
        tracing::error!("Failed to register custom hotkeys: {:?}", e);
    }

    info!("Hotkey manager initialized");

    // --- AppState Setup with Observers ---
    let app_state = Arc::new(Mutex::new(AppState::with_settings(settings.clone())));

    // Held outside the observer list too (Arc is shared, not moved) so the
    // settings-file watcher below can call `set_mode` directly — the config
    // window can change backspace mode without a method switch happening.
    let backspace_mode_observer = Arc::new(BackspaceModeObserver {
        keyboard: keyboard.clone(),
        mode: Mutex::new(backspace_mode),
    });

    // Register observers
    let ui_rx = {
        let mut state = app_state.lock().unwrap();

        // Keyboard observer - updates keyboard when method changes
        state.add_observer(Arc::new(KeyboardObserver::new(keyboard_manager.clone())));

        // Backspace-mode observer (event-sourcing-completion Phase 4) - MUST
        // be registered AFTER KeyboardObserver so the Keyboard instance it
        // re-applies the mode to already reflects the new method.
        state.add_observer(backspace_mode_observer.clone());

        // Backend observer - updates Platform backend mode
        state.add_observer(backend.clone());

        // Create UI event channel
        let (ui_tx, ui_rx) = mpsc::channel();

        // UI observer - updates tray icon and menu via proxy
        let ui_callback = Arc::new(MainUICallback::new(ui_tx));
        state.add_observer(Arc::new(UIObserver::new(ui_callback)));

        info!("Registered 3 observers");
        ui_rx // Pass receiver to outer scope
    };

    // Watch settings.toml so the config window's "Lưu" applies live without
    // a tray restart (P2 requirement F5). Own-write suppression here is
    // COMPARISON-based, not timer-based (unlike the learning.toml watcher):
    // the tray itself writes this file on every method switch / autostart /
    // learning / shorthand toggle via `AppState`, so on every watcher fire
    // the event loop diffs the reloaded file against `AppState`'s own
    // in-memory settings (always "what the tray itself last intended") —
    // equal means it was our own write (or a no-op edit), no reload needed;
    // different means a genuine external change (config window), apply it.
    // This avoids the "external edit racing our own autosave gets silently
    // dropped" gap a fixed suppression window would have.
    let (settings_file_tx, settings_file_rx) = mpsc::channel::<()>();
    let _settings_watcher = watch_settings_file(settings_file_tx);

    // --- Event Loop ---
    let menu_channel = muda::MenuEvent::receiver();

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(50),
        ));

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),

            Event::AboutToWait => {
                // Process UI events from observers
                while let Ok(ui_event) = ui_rx.try_recv() {
                    match ui_event {
                        UIEvent::UpdateMenuCheckmarks(method) => {
                            helpers::update_menu_checkmarks(
                                &method,
                                &english_item,
                                &chu_viet_menu,
                                &telex_item,
                                &vni_item,
                                &nom_item,
                                custom_items.as_slice(),
                            );
                        }
                        UIEvent::UpdateTrayIcon(method, enabled) => {
                            helpers::update_tray_icon(
                                &method,
                                enabled,
                                &mut _tray_icon,
                                &telex_icon,
                                &vni_icon,
                                &english_icon,
                                &nom_icon,
                                &custom_icon,
                                custom_items.as_slice(),
                            );
                        }
                    }
                }

                // Personal-learning off-thread save (event-sourcing-
                // completion Phase 5, red-team C3): the ONLY place
                // `LearningStore::write_atomic` is ever called — never from
                // the hook callback or under the KEYBOARD lock. Nothing is
                // ever queued while learning is unwired.
                if let Some(file) = drain_latest_learning_save(&learning_save_rx) {
                    if let Err(e) = LearningStore::write_atomic(&file) {
                        error!("Failed to save learning.toml: {:?}", e);
                    }
                    last_own_save = std::time::Instant::now();
                }

                // learning.toml edited on disk (tray → "Từ đã học" → user
                // saved in their editor): reload the shared store and
                // re-inject so the edit applies live.
                //
                // ORDERING (load-bearing): this block must stay BELOW the
                // save drain above — the drain flushes every dirty in-memory
                // signal to disk first, so replacing the store with
                // `LearningStore::load()` can never clobber unsaved state.
                //
                // Events within 2s of our own `write_atomic` are (usually)
                // our own rename landing; those are deferred via
                // `learning_reload_pending`, not dropped, so a real external
                // edit that races an autosave still applies once the window
                // clears — reloading our own write is an idempotent no-op.
                learning_reload_pending |= learning_file_rx.try_iter().count() > 0;
                if learning_reload_pending
                    && learning_enabled
                    && last_own_save.elapsed() > std::time::Duration::from_secs(2)
                {
                    learning_reload_pending = false;
                    info!("learning.toml changed on disk — reloading");
                    *learning_store.lock().unwrap() = LearningStore::load();
                    keyboard_manager.set_learning(learning_store.clone(), learning_tx.clone());
                }

                // macros.toml edited on disk (tray → "Gõ tắt" → hand-edit or
                // a future config window): reload and re-inject live. No
                // own-write suppression needed — nothing at the typing layer
                // ever writes this file (see `state::macros`'s module doc),
                // so every watcher fire is a genuine external edit.
                if macros_file_rx.try_iter().count() > 0 && shorthand_enabled {
                    info!("macros.toml changed on disk — reloading");
                    *macros_store.lock().unwrap() = MacroStore::load();
                    keyboard_manager.set_macros(macros_store.clone());
                }

                // settings.toml edited externally (config window's "Lưu", or
                // a hand-edit): diff against what the tray itself believes
                // (`AppState`'s in-memory settings — always up to date with
                // the tray's OWN writes) and apply only the fields that
                // actually changed, through the exact same code paths the
                // tray's own menu handlers use below.
                if settings_file_rx.try_iter().count() > 0 {
                    let known = app_state.lock().unwrap().settings().clone();
                    let new_settings = Settings::load();
                    if new_settings != known {
                        info!("settings.toml changed externally — applying");

                        // `auto_correct` has no runtime side effect to apply
                        // (unused field — engine leniency intentionally does
                        // no spell-check) but MUST still be synced into
                        // `AppState`'s in-memory copy first: every setter
                        // below re-saves the WHOLE settings struct, and
                        // without this sync a lone external edit to
                        // `auto_correct` would be silently reverted by the
                        // next field's setter call (stale in-memory value
                        // winning over the just-applied external one).
                        if new_settings.auto_correct != known.auto_correct {
                            app_state.lock().unwrap().settings_mut().auto_correct =
                                new_settings.auto_correct;
                        }

                        if new_settings.input_method != known.input_method {
                            if let Err(e) = app_state
                                .lock()
                                .unwrap()
                                .set_method(&new_settings.input_method)
                            {
                                error!("Failed to apply external method change: {:?}", e);
                            }
                        }
                        if new_settings.backspace_mode != known.backspace_mode {
                            let mode =
                                BackspaceMode::from_settings_str(&new_settings.backspace_mode);
                            backspace_mode_observer.set_mode(mode);
                            apply_backspace_mode(&keyboard, mode);
                            if let Err(e) = app_state
                                .lock()
                                .unwrap()
                                .set_backspace_mode(&new_settings.backspace_mode)
                            {
                                error!("Failed to persist external backspace_mode change: {:?}", e);
                            }
                        }
                        if new_settings.learning_enabled != known.learning_enabled {
                            learning_enabled = new_settings.learning_enabled;
                            if learning_enabled {
                                *learning_store.lock().unwrap() = LearningStore::load();
                                keyboard_manager
                                    .set_learning(learning_store.clone(), learning_tx.clone());
                            } else {
                                keyboard_manager.clear_learning();
                            }
                            if let Err(e) = app_state
                                .lock()
                                .unwrap()
                                .set_learning_enabled(learning_enabled)
                            {
                                error!(
                                    "Failed to persist external learning_enabled change: {:?}",
                                    e
                                );
                            }
                        }
                        if new_settings.shorthand != known.shorthand {
                            shorthand_enabled = new_settings.shorthand;
                            if shorthand_enabled {
                                *macros_store.lock().unwrap() = MacroStore::load();
                                keyboard_manager.set_macros(macros_store.clone());
                            } else {
                                keyboard_manager.clear_macros();
                            }
                            if let Err(e) =
                                app_state.lock().unwrap().set_shorthand(shorthand_enabled)
                            {
                                error!("Failed to persist external shorthand change: {:?}", e);
                            }
                        }
                        if new_settings.strict_spelling != known.strict_spelling {
                            keyboard_manager.set_strict_spelling(new_settings.strict_spelling);
                            if let Err(e) = app_state
                                .lock()
                                .unwrap()
                                .set_strict_spelling(new_settings.strict_spelling)
                            {
                                error!(
                                    "Failed to persist external strict_spelling change: {:?}",
                                    e
                                );
                            }
                        }
                        if new_settings.startup != known.startup {
                            match buttre_autostart::set_enabled(new_settings.startup) {
                                Ok(()) => {
                                    if let Err(e) =
                                        app_state.lock().unwrap().set_startup(new_settings.startup)
                                    {
                                        error!(
                                            "Failed to persist external startup change: {:?}",
                                            e
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "autostart set_enabled({}) failed: {e:?}",
                                        new_settings.startup
                                    );
                                }
                            }
                        }
                    }
                }

                if let Some(action) = hotkey_manager.check_hotkey() {
                    match action {
                        HotkeyAction::Toggle => {
                            info!("Hotkey: Toggle Vietnamese/English");
                            if let Err(e) = app_state.lock().unwrap().toggle() {
                                error!("Failed to toggle: {:?}", e);
                            }
                        }
                        HotkeyAction::Telex => {
                            if let Err(e) = app_state.lock().unwrap().set_method("telex") {
                                error!("Failed to set method: {:?}", e);
                            }
                        }
                        HotkeyAction::Vni => {
                            if let Err(e) = app_state.lock().unwrap().set_method("vni") {
                                error!("Failed to set method: {:?}", e);
                            }
                        }
                        HotkeyAction::Nom => {
                            if let Err(e) = app_state.lock().unwrap().set_method("nom") {
                                error!("Failed to set method: {:?}", e);
                            }
                        }
                        HotkeyAction::Custom(index) => {
                            if let Some((method_data, _)) = custom_items.get(index) {
                                // Direct .id access
                                if let Err(e) =
                                    app_state.lock().unwrap().set_method(&method_data.id)
                                {
                                    error!("Failed to set method: {:?}", e);
                                }
                            }
                        }
                        HotkeyAction::ToggleLastWord => {
                            info!("Hotkey: ToggleLastWord");
                            dispatch_toggle_last_word(&keyboard);
                        }
                    }
                }

                // Menu events
                if let Ok(event) = menu_channel.try_recv() {
                    if event.id == thoat_item.id() {
                        elwt.exit();
                    } else if event.id == nom_item.id() {
                        let _ = app_state.lock().unwrap().set_method("nom");
                    } else if event.id == english_item.id() {
                        let _ = app_state.lock().unwrap().set_method("english");
                    } else if event.id == telex_item.id() {
                        let _ = app_state.lock().unwrap().set_method("telex");
                    } else if event.id == vni_item.id() {
                        let _ = app_state.lock().unwrap().set_method("vni");
                    } else if event.id == cau_hinh_item.id() {
                        // Spawn the config window as a separate PROCESS
                        // (same exe, `--config` arg-dispatch in `main`) —
                        // never call `buttre_config::run()` in-process here,
                        // it owns a competing winit event loop (see
                        // `buttre-config`'s crate doc). Non-blocking: the
                        // tray keeps typing live while it's open.
                        match std::env::current_exe() {
                            Ok(exe) => {
                                if let Err(e) =
                                    std::process::Command::new(exe).arg("--config").spawn()
                                {
                                    error!("Failed to spawn config window: {:?}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to resolve current_exe for config window: {:?}", e)
                            }
                        }
                    } else {
                        for (method_data, item) in &custom_items {
                            if event.id == item.id() {
                                // Direct .id access
                                let _ = app_state.lock().unwrap().set_method(&method_data.id);
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file(marker: u32) -> LearningFile {
        let mut file = LearningFile::default();
        file.user_attested.insert(format!("marker{marker}"), 1);
        file
    }

    #[test]
    fn drain_latest_learning_save_returns_none_when_empty() {
        let (_tx, rx) = mpsc::channel::<LearningFile>();
        assert!(drain_latest_learning_save(&rx).is_none());
    }

    #[test]
    fn drain_latest_learning_save_debounces_to_the_last_queued_snapshot() {
        // Red-team C3: a burst of saves queued between two polls (e.g. rapid
        // word commits) must collapse to a single disk write of the LATEST
        // state — replaying every intermediate snapshot would be wasted I/O
        // for no additional correctness (each snapshot is the full state,
        // not a delta).
        let (tx, rx) = mpsc::channel::<LearningFile>();
        tx.send(sample_file(1)).unwrap();
        tx.send(sample_file(2)).unwrap();
        tx.send(sample_file(3)).unwrap();

        let latest =
            drain_latest_learning_save(&rx).expect("must return the latest queued snapshot");
        assert!(latest.user_attested.contains_key("marker3"));
        assert!(
            !latest.user_attested.contains_key("marker1"),
            "intermediate snapshots must not linger"
        );

        // The channel must be fully drained — a second poll finds nothing.
        assert!(drain_latest_learning_save(&rx).is_none());
    }

    #[test]
    fn drain_latest_learning_save_ignores_a_disconnected_sender() {
        let (tx, rx) = mpsc::channel::<LearningFile>();
        tx.send(sample_file(1)).unwrap();
        drop(tx);
        assert!(
            drain_latest_learning_save(&rx).is_some(),
            "a queued item must still be returned even after the sender disconnects"
        );
        assert!(
            drain_latest_learning_save(&rx).is_none(),
            "a disconnected, empty channel must be treated as \"nothing new\", not an error"
        );
    }
}
