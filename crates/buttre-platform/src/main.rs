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
use buttre_core::state::{Settings, StateObserver};
use buttre_platform::shared::{KeyboardManager, MethodRegistry, pipe_server};
use buttre_platform::shared::ui::{build_menu, create_tray_icon, show_help_dialog, MenuItems, helpers};
use buttre_platform::shared::observers::{UIObserver, MainUICallback, UIEvent, KeyboardObserver};
use log::{info, error, warn};
use std::sync::{Arc, Mutex, RwLock};
use buttre_core::AppState;
use buttre_core::Keyboard;
use buttre_core::keyboard::BackspaceMode;
use buttre_core::hotkey::{ButtreHotkeyManager, HotkeyAction};
use buttre_platform::{Backend, PlatformBackend, platform_name};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};
use std::sync::mpsc;

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
struct BackspaceModeObserver {
    keyboard: Arc<RwLock<Option<Keyboard>>>,
    mode: BackspaceMode,
}

impl StateObserver for BackspaceModeObserver {
    fn on_method_changed(&self, _method: &str, _enabled: bool) {
        apply_backspace_mode(&self.keyboard, self.mode);
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

fn main() -> Result<()> {
    // Initialize tracing (handles both log crate and tracing crate)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

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
    info!("Registered {} input methods", method_registry.get_all().len());
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
            MethodMetadata { id: "telex".to_string(), name: "Telex".to_string(), description: "Built-in Telex".to_string(), version: "1.0.0".to_string(), author: "buttre".to_string(), icon: None, is_builtin: true },
            MethodMetadata { id: "vni".to_string(), name: "VNI".to_string(), description: "Built-in VNI".to_string(), version: "1.0.0".to_string(), author: "buttre".to_string(), icon: None, is_builtin: true },
            MethodMetadata { id: "nom".to_string(), name: "Chữ Nôm".to_string(), description: "Built-in Nôm".to_string(), version: "1.0.0".to_string(), author: "buttre".to_string(), icon: None, is_builtin: true },
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
        huong_dan_item,
        thoat_item,
        ..
    } = menu_items;

    // --- Tray Setup ---
    // update_tray_icon in helpers handles custom_items with MethodMetadata now
    let (mut _tray_icon, telex_icon, vni_icon, english_icon, nom_icon, custom_icon) =
        create_tray_icon(&menu, &settings, &custom_items)?;

    // --- buttre Keyboard Setup ---
    let keyboard_manager = KeyboardManager::new()?;
    
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
    let mut hotkey_manager = ButtreHotkeyManager::new()
        .expect("Failed to create hotkey manager");
    
    // Register custom hotkeys (Ctrl+Shift+4..0) based on menu items count
    if let Err(e) = hotkey_manager.register_custom_methods(custom_items.len()) {
        tracing::error!("Failed to register custom hotkeys: {:?}", e);
    }
    
    info!("Hotkey manager initialized");

    // --- AppState Setup with Observers ---
    let app_state = Arc::new(Mutex::new(AppState::with_settings(settings.clone())));
    
    // Register observers
    let ui_rx = {
        let mut state = app_state.lock().unwrap();
        
        // Keyboard observer - updates keyboard when method changes
        let kb_manager = keyboard_manager;
        state.add_observer(Arc::new(KeyboardObserver::new(kb_manager)));

        // Backspace-mode observer (event-sourcing-completion Phase 4) - MUST
        // be registered AFTER KeyboardObserver so the Keyboard instance it
        // re-applies the mode to already reflects the new method.
        state.add_observer(Arc::new(BackspaceModeObserver {
            keyboard: keyboard.clone(),
            mode: backspace_mode,
        }));

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
                                if let Err(e) = app_state.lock().unwrap().set_method(&method_data.id) {
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
                    }
                    else if event.id == nom_item.id() {
                        let _ = app_state.lock().unwrap().set_method("nom");
                    }
                    else if event.id == english_item.id() {
                        let _ = app_state.lock().unwrap().set_method("english");
                    }
                    else if event.id == telex_item.id() {
                        let _ = app_state.lock().unwrap().set_method("telex");
                    }
                    else if event.id == vni_item.id() {
                        let _ = app_state.lock().unwrap().set_method("vni");
                    }
                    else {
                        let mut handled = false;
                        for (method_data, item) in &custom_items {
                            if event.id == item.id() {
                                // Direct .id access
                                let _ = app_state.lock().unwrap().set_method(&method_data.id);
                                handled = true;
                                break;
                            }
                        }
                        if !handled && event.id == huong_dan_item.id() {
                            show_help_dialog();
                        }
                    }
                }
            }
            _ => {}
        }
    })?;

    Ok(())
}
