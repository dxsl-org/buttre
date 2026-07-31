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
//! (atomic) on EVERY control change (instant-apply — the "Đóng" button only
//! closes) — the tray's own directory watcher (mirroring the one already
//! wired for `learning.toml`/`macros.toml`) picks up each change and
//! re-applies it live.

use buttre_core::state::learning::LearningStore;
use buttre_core::state::macros::MacroStore;
use buttre_core::state::Settings;

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

/// Open `path` in the platform's default editor. Small enough to keep
/// self-contained rather than pull in a shared helper crate — sharing it
/// would mean depending on `buttre-platform` (or a new crate) for three
/// lines, and this crate deliberately stays clear of `buttre-platform`'s
/// winit-0.29 dependency chain.
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

/// Push persisted settings onto the window's value widgets. Shared by the
/// initial open and the live-refresh poll so the two paths can never drift.
/// No method picker: kiểu gõ thuộc menu của OS hoặc tray, không thuộc cửa
/// sổ này (ADR-0003 — "một chỗ chọn là đủ").
fn apply_settings_to_window(window: &ConfigWindow, settings: &Settings) {
    window.set_autostart(settings.startup);
    window.set_raw_backspace(settings.backspace_mode == "raw");
    window.set_strict_spelling(settings.strict_spelling);
    window.set_learning_enabled(settings.learning_enabled);
    window.set_shorthand_enabled(settings.shorthand);
    // Checkbox is inverted: ON = no-preedit = use_preedit false.
    window.set_use_preedit_off(!settings.use_preedit);
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
///
/// # Arguments
/// * `show_autostart` - `false` trên nền tảng OS-sở-hữu (ADR-0003): ở đó
///   entry autostart chỉ thoát im lặng, nên checkbox bị ẩn thay vì nói dối.
///   Chỉ `main.rs` (nơi biết `MethodOwner`) quyết định được giá trị này —
///   crate này cố ý không phụ thuộc `buttre-platform`.
pub fn run(show_autostart: bool) -> anyhow::Result<()> {
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

    let window = ConfigWindow::new()?;
    window.set_show_autostart(show_autostart);
    apply_settings_to_window(&window, &settings);
    // Single-sourced from Cargo.toml — the old help_dialog.rs MessageBox
    // had this hardcoded ("0.7.7-beta") and silently went stale after every
    // release bump; `CARGO_PKG_VERSION` can never drift.
    window.set_version(env!("CARGO_PKG_VERSION").into());

    window.on_open_url(|url| {
        // Windows: `explorer.exe <url>` (NOT `cmd /C start`) — explorer
        // passes the URL as a single CreateProcess argv with no cmd.exe
        // shell re-parse in between, so it can't be confused by shell
        // metacharacters (`&`, `|`, `^`, …) the way `cmd /C start "" <url>`
        // can (cmd re-splits its `/C` command line on those even though
        // `Command::args` already quoted the argv boundary correctly —
        // e.g. any query-string URL containing `&` would silently break,
        // or worse, execute a second command if this callback is ever fed
        // a less-trusted string than today's two hardcoded literals).
        let result = if cfg!(target_os = "windows") {
            std::process::Command::new("explorer.exe")
                .arg(url.as_str())
                .spawn()
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(url.as_str()).spawn()
        } else {
            std::process::Command::new("xdg-open")
                .arg(url.as_str())
                .spawn()
        };
        if let Err(e) = result {
            eprintln!("cannot open {url}: {e:?}");
        }
    });

    // Snapshot for the live-refresh poll timer below.
    let mut poll_last = settings.clone();

    let weak = window.as_weak();
    window.on_save_settings(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        // Fields this window does NOT own — kiểu gõ, on/off (tray/menu OS,
        // ADR-0003), the transport kill switch, auto_correct — must carry the
        // CURRENT store value, not the open-time snapshot: the tray or the
        // IBus panel may have changed them while this window was open, and
        // re-saving a stale copy would silently revert that change.
        let base = Settings::load();
        let new_settings = Settings {
            shorthand: window.get_shorthand_enabled(),
            startup: window.get_autostart(),
            backspace_mode: if window.get_raw_backspace() {
                "raw".to_string()
            } else {
                "grapheme".to_string()
            },
            learning_enabled: window.get_learning_enabled(),
            strict_spelling: window.get_strict_spelling(),
            use_preedit: !window.get_use_preedit_off(),
            ..base
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

    window.on_open_macros_file(|| {
        if let Ok(path) = MacroStore::get_path() {
            open_in_editor(&path);
        }
    });

    // "Đóng" — every setting already saved itself on change, so closing is
    // the button's only job.
    window.on_close_window(|| {
        if let Err(e) = slint::quit_event_loop() {
            eprintln!("quit_event_loop failed: {e:?}");
        }
    });

    window.show()?;
    center_on_screen(&window);

    // Re-center after the first rendered frame: only then does the window
    // report its real outer size instead of the pre-frame estimate.
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(80), move || {
        let Some(window) = weak.upgrade() else { return };
        center_on_screen(&window);
    });

    // Live-refresh: while this window is open, follow external settings.toml
    // changes (the tray menu, or an IBus-panel switch the tray relays into the
    // file) so the widgets never go stale against the resident tray. Polling on
    // the Slint UI thread avoids a notify dependency and cross-thread
    // marshalling. Widget edits auto-save (each control's `selected`/`toggled`
    // is wired to `save-settings()`), but a PROGRAMMATIC property set from Rust
    // does not emit those interaction callbacks — so this poll only writes
    // widgets, never triggering save-settings, and there is no write-back loop.
    // The PartialEq guard also leaves an unsaved in-progress edit untouched:
    // until it auto-saves, disk still equals the last applied snapshot.
    let poll_timer = slint::Timer::default();
    let weak = window.as_weak();
    poll_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(1000),
        move || {
            let Some(window) = weak.upgrade() else { return };
            let disk = Settings::load();
            if disk != poll_last {
                apply_settings_to_window(&window, &disk);
                poll_last = disk;
            }
        },
    );

    slint::run_event_loop()?;
    // `poll_timer` lives until here — dropping it earlier would stop the poll.
    drop(poll_timer);
    Ok(())
}

/// Best-effort center on the monitor the window opened on, called right
/// after `show()` and again after the first frame (the real outer size is
/// only known then). Everything stays inside winit's own coordinate space —
/// mixing in Win32 screen metrics breaks under DPI virtualization (a 150%
/// display reports a different pixel grid than the one winit positions in).
/// No-op where the platform forbids client-side positioning (Wayland) or
/// the monitor/size is not known yet.
fn center_on_screen(window: &ConfigWindow) {
    use i_slint_backend_winit::winit::dpi::PhysicalPosition;
    use i_slint_backend_winit::WinitWindowAccessor;

    window.window().with_winit_window(|w| {
        let Some(monitor) = w.current_monitor() else {
            return;
        };
        let screen = monitor.size();
        let origin = monitor.position();
        let size = w.outer_size();
        if screen.width == 0 || size.width == 0 {
            return;
        }
        let x = origin.x + ((screen.width as i32 - size.width as i32) / 2).max(0);
        let y = origin.y + ((screen.height as i32 - size.height as i32) / 2).max(0);
        w.set_outer_position(PhysicalPosition::new(x, y));
    });
}
