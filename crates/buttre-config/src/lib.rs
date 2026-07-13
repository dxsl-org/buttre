//! Native config window for buttre — a separate PROCESS from the tray, but
//! the SAME binary (launched via `buttre --config`, mirroring the existing
//! `--ibus`/`--ime` arg-dispatch in `buttre-platform/src/main.rs`).
//!
//! Isolated in its own crate so Slint's winit-0.30 dependency never links
//! into `buttre-platform` proper — only the thin `--config` arg-dispatch
//! arm in `main.rs` calls [`run`], and that arm never coexists in the same
//! process with the tray's own winit-0.29 event loop (see
//! `.agents/260713-1308-config-window-and-shorthand/phase-02-slint-config-scaffold.md`
//! for the full packaging rationale).
//!
//! Live-sync with the resident tray process is file-watch only (no IPC): this
//! window reads `Settings::load()` on open and calls `Settings::save()`
//! (atomic) on "Lưu" — the tray's own directory watcher (mirroring the one
//! already wired for `learning.toml`/`macros.toml`) picks up the change and
//! re-applies it live.

use buttre_core::state::learning::LearningStore;
use buttre_core::state::macros::MacroStore;
use buttre_core::state::Settings;
use buttre_core::vietnamese::get_custom_dir;
use buttre_core::Config as KeyboardConfig;

mod learned_adapter;
mod macro_adapter;

// `slint::include_modules!()` splices in `build.rs`/`slint-build`'s
// generated Rust — code this crate does not author or control. Slint emits
// `todo!()` stubs for a codegen path we never exercise (embedding a
// Rust-defined component), which trips the workspace's `clippy::todo` deny.
// Scoped to this crate only; `buttre-config`'s own hand-written code below
// contains no todos.
#[allow(clippy::todo)]
mod generated {
    slint::include_modules!();
}
use generated::*;

/// One selectable entry in the General tab's method dropdown.
struct MethodChoice {
    id: String,
    name: String,
}

/// Built-ins plus a scan of the custom-keyboards directory — mirrors
/// `buttre-platform`'s `MethodRegistry` discovery logic using only
/// `buttre_core` APIs (this crate deliberately does not depend on
/// `buttre-platform`, to keep its winit-0.29 tray stack out of this
/// binary's `--config` code path).
fn discover_methods() -> Vec<MethodChoice> {
    let mut methods = vec![
        MethodChoice {
            id: "english".to_string(),
            name: "English".to_string(),
        },
        MethodChoice {
            id: "telex".to_string(),
            name: "Telex".to_string(),
        },
        MethodChoice {
            id: "vni".to_string(),
            name: "VNI".to_string(),
        },
        MethodChoice {
            id: "nom".to_string(),
            name: "Chữ Nôm".to_string(),
        },
    ];

    let custom_dir = get_custom_dir();
    if let Ok(entries) = std::fs::read_dir(&custom_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(stem, "telex" | "vni" | "nom") {
                continue; // already listed as built-in
            }
            if let Ok(cfg) = KeyboardConfig::load(&path.to_string_lossy()) {
                methods.push(MethodChoice {
                    id: cfg.metadata.id.clone(),
                    name: cfg.metadata.name.clone(),
                });
            }
        }
    }
    methods
}

/// Open `path` in the platform's default editor — mirrors
/// `buttre-platform/src/main.rs`'s `open_learning_file`/`open_macros_file`
/// (duplicated rather than shared: it's three lines, and sharing it would
/// mean depending on `buttre-platform`, which this crate deliberately does
/// not, to keep winit-0.29 out of the `--config` code path).
fn open_in_editor(path: &std::path::Path) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("notepad.exe").arg(path).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    if let Err(e) = result {
        eprintln!("cannot open {}: {e:?}", path.display());
    }
}

fn learned_word_row_to_slint(r: &learned_adapter::LearnedWordRow) -> LearnedWordRow {
    LearnedWordRow {
        word: r.word.as_str().into(),
        count: r.count as i32,
    }
}

fn macro_row_to_slint(r: &macro_adapter::MacroRow) -> MacroRow {
    MacroRow {
        trigger: r.trigger.as_str().into(),
        expand: r.expand.as_str().into(),
        enabled: r.enabled,
    }
}

fn refresh_learned_words(window: &ConfigWindow) {
    let rows: Vec<LearnedWordRow> = learned_adapter::load_rows()
        .iter()
        .map(learned_word_row_to_slint)
        .collect();
    window.set_learned_words(slint::ModelRc::new(slint::VecModel::from(rows)));
}

fn refresh_macro_rows(window: &ConfigWindow) {
    let rows: Vec<MacroRow> = macro_adapter::load_rows()
        .iter()
        .map(macro_row_to_slint)
        .collect();
    window.set_macro_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
}

/// Entry point called by `buttre-platform`'s `--config` arg-dispatch arm.
/// Blocks until the window is closed (Slint owns this process's event loop
/// for its lifetime) — the caller must invoke this BEFORE any tray/hook
/// setup, never after, since the two event loops can never coexist.
pub fn run() -> anyhow::Result<()> {
    // Single-instance: a second `buttre --config` invocation (e.g. the user
    // clicks "Cấu hình…" twice) should not open a second window. There is no
    // cross-process "focus the existing window" primitive without extra
    // IPC, so the simpler, honest behavior is: exit immediately, leaving the
    // first window as-is.
    let instance = single_instance::SingleInstance::new("buttre-config")
        .map_err(|e| anyhow::anyhow!("single-instance lock failed: {e}"))?;
    if !instance.is_single() {
        return Ok(());
    }

    let settings = Settings::load();
    let methods = discover_methods();
    let method_index = methods
        .iter()
        .position(|m| m.id == settings.input_method)
        .unwrap_or(0) as i32;
    let method_names: Vec<slint::SharedString> =
        methods.iter().map(|m| m.name.as_str().into()).collect();

    let window = ConfigWindow::new()?;
    window.set_method_names(slint::ModelRc::new(slint::VecModel::from(method_names)));
    window.set_method_index(method_index);
    window.set_autostart(settings.startup);
    window.set_raw_backspace(settings.backspace_mode == "raw");
    window.set_learning_enabled(settings.learning_enabled);
    window.set_shorthand_enabled(settings.shorthand);

    let weak = window.as_weak();
    window.on_save_settings(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let index = window.get_method_index().max(0) as usize;
        let input_method = methods
            .get(index)
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "english".to_string());

        let new_settings = Settings {
            input_method,
            auto_correct: settings.auto_correct,
            shorthand: window.get_shorthand_enabled(),
            startup: window.get_autostart(),
            backspace_mode: if window.get_raw_backspace() {
                "raw".to_string()
            } else {
                "grapheme".to_string()
            },
            learning_enabled: window.get_learning_enabled(),
        };

        // Autostart registration is a per-OS side effect, not just a
        // settings field — apply it the same way the tray's own toggle
        // does (`buttre-autostart`, shared by both), so the window and the
        // tray never disagree about whether the OS actually has the entry
        // registered.
        if let Err(e) = buttre_autostart::set_enabled(new_settings.startup) {
            eprintln!("autostart set_enabled failed: {e:?}");
        }

        if let Err(e) = new_settings.save() {
            eprintln!("failed to save settings.toml: {e:?}");
        }
    });

    // ── Từ đã học ─────────────────────────────────────────────────────────
    refresh_learned_words(&window);

    let weak = window.as_weak();
    window.on_delete_learned_word(move |word| {
        let Some(window) = weak.upgrade() else { return };
        if let Err(e) = learned_adapter::delete_word(&word) {
            eprintln!("failed to delete learned word: {e:?}");
        }
        refresh_learned_words(&window);
    });

    let weak = window.as_weak();
    window.on_clear_learned_words(move || {
        let Some(window) = weak.upgrade() else { return };
        if let Err(e) = learned_adapter::clear_all() {
            eprintln!("failed to clear learned words: {e:?}");
        }
        refresh_learned_words(&window);
    });

    window.on_open_learning_file(|| {
        if let Ok(path) = LearningStore::get_path() {
            open_in_editor(&path);
        }
    });

    // ── Gõ tắt ────────────────────────────────────────────────────────────
    refresh_macro_rows(&window);

    let weak = window.as_weak();
    window.on_save_macro(move |old_trigger, new_trigger, expand, enabled| {
        let Some(window) = weak.upgrade() else {
            return false;
        };
        let result = if old_trigger.is_empty() {
            macro_adapter::add(&new_trigger, &expand, enabled)
        } else {
            macro_adapter::edit(&old_trigger, &new_trigger, &expand, enabled)
        };
        match result {
            Ok(warning) => {
                window.set_macro_form_is_error(false);
                window.set_macro_form_message(warning.map(|w| w.0).unwrap_or_default().into());
                refresh_macro_rows(&window);
                true
            }
            Err(e) => {
                window.set_macro_form_is_error(true);
                window.set_macro_form_message(e.0.into());
                false
            }
        }
    });

    let weak = window.as_weak();
    window.on_delete_macro(move |trigger| {
        let Some(window) = weak.upgrade() else { return };
        if let Err(e) = macro_adapter::delete(&trigger) {
            eprintln!("failed to delete macro: {e:?}");
        }
        refresh_macro_rows(&window);
    });

    let weak = window.as_weak();
    window.on_toggle_macro_enabled(move |trigger, enabled| {
        let Some(window) = weak.upgrade() else { return };
        if let Err(e) = macro_adapter::set_enabled(&trigger, enabled) {
            eprintln!("failed to toggle macro: {e:?}");
        }
        refresh_macro_rows(&window);
    });

    window.on_open_macros_file(|| {
        if let Ok(path) = MacroStore::get_path() {
            open_in_editor(&path);
        }
    });

    window.run()?;
    Ok(())
}
