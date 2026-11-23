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
use buttre_core::state::Settings;
use buttre_platform::shared::method_owner::{self, MethodOwner};
use log::info;
use std::sync::Mutex;

#[cfg(not(platform_macos))]
mod tray_app;

/// `buttre --register-tsf` / `--unregister-tsf`: install or remove the TSF
/// text-service registration for the DLL sitting next to this executable.
///
/// The installer calls this instead of writing the registry itself. Those keys
/// are not a flat list — a working text service also needs its TSF CATEGORIES
/// registered, and that goes through `ITfCategoryMgr`, a COM call an MSI
/// cannot express as registry rows. The MSI's hand-written approximation was
/// missing exactly that, plus the icon, and it shipped a service Windows
/// refused to name or drive. Keeping ONE implementation
/// (`registration::register_server`, the same one `regsvr32` reaches through
/// `DllRegisterServer`) is what stops the copies drifting again.
#[cfg(platform_windows)]
fn run_tsf_registration(unregister: bool) -> Result<()> {
    use anyhow::Context;
    use buttre_platform::platforms::windows::tsf::registration::{
        register_server, unregister_server,
    };

    if unregister {
        unregister_server()?;
        println!("buttre TSF unregistered");
        return Ok(());
    }

    let dll = std::env::current_exe()
        .context("cannot locate buttre.exe")?
        .parent()
        .context("buttre.exe has no parent directory")?
        .join("buttre_platform.dll");
    anyhow::ensure!(
        dll.exists(),
        "buttre_platform.dll not found next to buttre.exe ({})",
        dll.display()
    );

    register_server(&dll)?;
    println!("buttre TSF registered: {}", dll.display());
    Ok(())
}

#[cfg(not(platform_windows))]
fn run_tsf_registration(_unregister: bool) -> Result<()> {
    anyhow::bail!("--register-tsf is Windows-only")
}

/// `buttre --add-keyboard <id>` / `--disable-layout-hotkey`: the installer's
/// per-user steps (impersonated MSI actions), see `tsf::keyboard_setup`.
#[cfg(platform_windows)]
fn run_keyboard_setup(args: &[String]) -> Result<()> {
    use buttre_platform::platforms::windows::tsf::keyboard_setup;
    if args.iter().any(|a| a == "--disable-layout-hotkey") {
        return keyboard_setup::disable_layout_hotkey();
    }
    let id = args
        .iter()
        .skip_while(|a| *a != "--add-keyboard")
        .nth(1)
        .ok_or_else(|| {
            anyhow::anyhow!("--add-keyboard needs a keyboard id (telex, vni, nom, ...)")
        })?;
    keyboard_setup::add_keyboard(id)
}

#[cfg(not(platform_windows))]
fn run_keyboard_setup(_args: &[String]) -> Result<()> {
    anyhow::bail!("--add-keyboard / --disable-layout-hotkey are Windows-only")
}

/// Flags whose whole purpose is to print something.
///
/// Windows-only, like the single function that reads it: no other platform
/// builds a GUI-subsystem binary, so nowhere else needs to reattach a console.
#[cfg(platform_windows)]
const REPORTING_FLAGS: [&str; 10] = [
    "--version",
    "-V",
    "--help",
    "-h",
    "--doctor",
    "--tsf-status",
    "--register-tsf",
    "--unregister-tsf",
    "--add-keyboard",
    "--disable-layout-hotkey",
];

/// Bind stdout/stderr to the console that launched us, so the reporting flags
/// can actually be read.
///
/// buttre.exe is a GUI-subsystem binary — it must be, or launching the tray
/// would flash a console window. A GUI binary starts with NO console attached,
/// so `println!` writes to an invalid handle and vanishes. It appears to work
/// from a terminal only by luck: a GUI child sometimes inherits the shell's
/// handles, and sometimes does not. Captured by a script it reliably produced
/// nothing, which `check-tsf-status.ps1` then reported as "this exe is too old
/// for --tsf-status" — a diagnostic lying about a binary that was correct.
///
/// `SetStdHandle` has to run before the first print: Rust's `Stdout` caches the
/// handle it gets from `GetStdHandle` on first use.
///
/// CRITICAL: this must NOT touch a stdout the parent already gave us. A caller
/// that captures output (`$x = & buttre --tsf-status`, a pipe, a redirect to a
/// file) hands the child a perfectly good handle, and pointing stdout at
/// `CONOUT$` instead sends the report to the console where the caller cannot see
/// it — the first version of this function did exactly that and turned a
/// sometimes-empty capture into an always-empty one.
///
/// Silent no-op when there is no parent console either (launched from Explorer,
/// or by the MSI) — nothing to attach to, and those callers read the exit code.
#[cfg(platform_windows)]
fn attach_parent_console(args: &[String]) {
    use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };

    if !args.iter().any(|a| REPORTING_FLAGS.contains(&a.as_str())) {
        return;
    }

    // SAFETY: every call here takes only constants or a handle obtained from
    // the call above it. `CreateFileW` on the "CONOUT$" pseudo-file is the
    // documented way to reach the attached console; the handle it returns is
    // handed to `SetStdHandle`, which keeps it for the process's lifetime, so it
    // is deliberately not closed.
    unsafe {
        let existing = GetStdHandle(STD_OUTPUT_HANDLE);
        let usable = matches!(existing, Ok(h) if h != INVALID_HANDLE_VALUE && h != HANDLE(std::ptr::null_mut()));
        if usable {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return;
        }
        let Ok(console) = CreateFileW(
            windows::core::w!("CONOUT$"),
            (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
            FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) else {
            return;
        };
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, console);
        let _ = SetStdHandle(STD_ERROR_HANDLE, console);
    }
}

#[cfg(not(platform_windows))]
fn attach_parent_console(_args: &[String]) {}

/// `buttre --tsf-status`: report how buttre runs on this machine, and why.
///
/// Exists so diagnostics ask the AUTHORITY instead of guessing. A shell script
/// reading `HKCU\...\CTF\SortOrder` to infer "is buttre added as an input
/// method" answered NO while the text service was demonstrably running — the
/// same wrong-proxy mistake that made the backend gate test the language list.
/// Here the script calls the very functions `method_owner` decides with.
///
/// Exit code 0 = TSF keyboards (no tray), 1 = Hook (tray), so a caller can
/// branch without parsing text.
#[cfg(platform_windows)]
fn run_tsf_status() -> Result<()> {
    use buttre_platform::platforms::windows::tsf::{lang_check, registration};

    let registered = registration::is_tsf_registered();
    let langids = lang_check::enabled_langids();
    let enabled = !langids.is_empty();
    let under = if enabled {
        langids
            .iter()
            .map(|id| format!("0x{id:04X}"))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        "(none — add it in Windows keyboard settings)".to_string()
    };

    println!("registered (HKLM, available):   {registered}");
    println!("added by you (HKCU, languages): {under}");
    println!(
        "buttre runs as:                 {}",
        if registered && enabled {
            "TSF keyboards (no tray; Win+Space, Ctrl+Shift+1-9, Ctrl+Shift+` = off)"
        } else {
            "Hook (tray)"
        }
    );
    if registered && enabled {
        return Ok(());
    }

    // Flush before exiting. `process::exit` runs no destructors, and stdout is
    // BLOCK-buffered when redirected — so the whole report vanished whenever a
    // script captured it, which read as "this exe is too old for the flag".
    // Interactively it worked, because a terminal makes stdout line-buffered.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(1)
}

#[cfg(not(platform_windows))]
fn run_tsf_status() -> Result<()> {
    anyhow::bail!("--tsf-status is Windows-only")
}

/// Open the engine's log file (truncated per session so it can't grow
/// unbounded). ibus-daemon spawns `buttre --ibus` with stdout/stderr on
/// /dev/null, so without a file sink every engine log line is lost — which
/// is exactly how a dead file watcher went unnoticed (method-sync debug).
fn engine_log_file() -> Option<std::fs::File> {
    let dir = dirs::cache_dir()?.join("buttre");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::File::create(dir.join("engine.log")).ok()
}

/// Initialize tracing (handles both log crate and tracing crate). Defaults
/// to INFO when `RUST_LOG` is unset — the previous default (ERROR-only)
/// made watcher/sync diagnostics invisible in normal runs. The `--ibus`
/// engine logs to `~/.cache/buttre/engine.log` (its stderr is /dev/null,
/// see [`engine_log_file`]); everything else keeps stderr.
fn init_tracing(args: &[String]) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if args.iter().any(|a| a == "--ibus") {
        if let Some(file) = engine_log_file() {
            builder
                .with_writer(Mutex::new(file))
                .with_ansi(false)
                .init();
            return;
        }
    }
    builder.init();
}

/// `buttre --doctor`: one-screen diagnosis of which IME path this machine
/// uses — every signal the decision weighs (`backend_detect::Evidence`),
/// the verdict, and where the tri-surface sync state lives. Read-only;
/// safe to run anytime. Does not wait for a starting daemon: it reports
/// "đang chờ" instead.
#[cfg(platform_linux)]
fn run_doctor() {
    use buttre_platform::platforms::linux::{backend_detect, kwin_ime, method_sync};
    let evidence = backend_detect::gather();
    println!("buttre {} — doctor\n", env!("CARGO_PKG_VERSION"));
    println!(
        "session:  XDG_SESSION_TYPE={} XDG_CURRENT_DESKTOP={}",
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into()),
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "?".into()),
    );
    println!(
        "running:  ibus={} fcitx5={} (addon fcitx5-buttre: {}) wayland={} im-v2={}",
        evidence.ibus_running,
        evidence.fcitx5_running,
        evidence.fcitx5_addon,
        evidence.wayland,
        evidence.compositor_im_v2,
    );
    println!(
        "config:   env IM={:?} shell tự chạy IBus={} kwin={:?} → mong đợi {:?}",
        evidence.env_im,
        evidence.shell_spawns_ibus,
        evidence.kwin,
        evidence.expected(),
    );
    if let backend_detect::Judgement::Pending(f) = backend_detect::judge(&evidence, true) {
        println!("verdict:  đang chờ {f:?} khởi động (lúc đăng nhập buttre chờ tối đa 30 giây)");
    }
    println!(
        "kwinrc:   [Wayland] InputMethod = {}",
        kwin_ime::kwinrc_input_method().unwrap_or_else(|| "(không đặt)".into())
    );
    // Verdict without waiting (ADR-0003): who owns method choice here, and
    // the surface that explains it — Plasma with ibus-daemon also running
    // is still a tray machine when KWin spawns buttre.
    let surface = method_owner::current_surface();
    match method_owner::owner_of(surface) {
        MethodOwner::Os => {
            println!("owner:    OS sở hữu kiểu gõ (menu IBus/fcitx5) — không tray ({surface:?})")
        }
        MethodOwner::Buttre => {
            println!("owner:    buttre sở hữu kiểu gõ — tray ({surface:?})")
        }
    }
    match surface {
        method_owner::SurfacePath::LinuxKwinUnconfigured => println!(
            "LƯU Ý:    KWin chưa chạy buttre — System Settings → Keyboard → Virtual Keyboard → \
             chọn \"buttre (Wayland IME)\""
        ),
        method_owner::SurfacePath::LinuxFcitx5NoAddon => println!(
            "LƯU Ý:    fcitx5 đang phục vụ phiên này nhưng addon fcitx5-buttre CHƯA cài — \
             buttre không gõ được qua fcitx5"
        ),
        _ => {}
    }
    println!(
        "state:    method={:?} enabled={} (files trong ~/.config/buttre/)",
        method_sync::read_method(),
        method_sync::read_enabled(),
    );
    match Settings::get_path() {
        Ok(p) => println!("settings: {}", p.display()),
        Err(e) => println!("settings: không resolve được đường dẫn: {e}"),
    }
}

/// `buttre --detect`: the settled typing path as ONE stable token, for
/// installers (`scripts/install-linux-auto.sh`). Same verdict the tray acts
/// on, so no installer re-implements the decision in shell. Waits like a
/// manual launch for a configured daemon that is still starting.
#[cfg(platform_linux)]
fn run_detect() {
    use method_owner::SurfacePath as S;
    let token = match method_owner::settled_surface(std::time::Duration::from_secs(3)) {
        S::LinuxIbus => "ibus",
        S::LinuxFcitx5 => "fcitx5",
        S::LinuxFcitx5NoAddon => "fcitx5-no-addon",
        S::LinuxWaylandNative => "wayland-native",
        S::LinuxKwinUnconfigured => "kwin-unconfigured",
        S::LinuxUndetected | S::WindowsTsf | S::WindowsHook | S::MacosImkit => "none",
    };
    println!("{token}");
}

#[cfg(not(platform_linux))]
fn run_doctor() {
    println!("buttre --doctor hiện chỉ hỗ trợ Linux.");
}

fn main() -> Result<()> {
    // Informational flags are handled before ANY backend or UI setup: they
    // need no display, and letting them fall through to the tray path below
    // panics on Linux (the tray builds a GTK menu). Unknown flags still start
    // the tray, matching prior behaviour; `--ibus`/`--ime`/`--config` are
    // matched further down.
    let args: Vec<String> = std::env::args().collect();
    // BEFORE the first println. See `attach_parent_console`.
    attach_parent_console(&args);
    // ibus-daemon runs this from the component XML's `<engines exec=…>` and
    // parses stdout as XML — so it is answered BEFORE `init_tracing`, whose
    // default writer is stdout: one log line would corrupt the document and
    // drop every buttre engine from the registry.
    #[cfg(platform_linux)]
    if args.iter().any(|a| a == "--ibus-engines") {
        print!(
            "{}",
            buttre_platform::platforms::linux::ibus_engines::engines_xml()
        );
        return Ok(());
    }
    init_tracing(&args);
    // Self-heal a corrupt settings.toml ONCE, before anything loads it: the
    // strict-reading writers refuse to touch an unparseable file, so without
    // this quarantine every save would stay refused until a hand-fix.
    Settings::quarantine_if_corrupt();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("buttre {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "buttre {ver} — Vietnamese input method\n\n\
             Usage: buttre [FLAG]\n\n\
             Flags:\n  \
             (none)      Tray app (Windows, Wayland-native) — or the settings window\n  \
             \x20           on OS-owned platforms (IBus, fcitx5, macOS; ADR-0003)\n  \
             --autostart Login launch: tray where a tray exists, silent exit elsewhere\n  \
             --ibus      Run as the IBus engine (spawned by ibus-daemon, not by hand)\n  \
             --ibus-engines  Print the IBus engine list, one per input method (read by ibus-daemon)\n  \
             --ime       Run as a self-detecting IME (Wayland-native, IBus fallback)\n  \
             --config    Open the settings window\n  \
             --doctor    Print IME-backend diagnosis (fcitx/ibus/wayland) and exit\n  \
             --detect    Print the detected typing path as one token (Linux installers)\n  \
             --register-tsf    Register the Windows TSF text service (needs Administrator)\n  \
             --unregister-tsf  Remove that registration (needs Administrator)\n  \
             --tsf-status      Report whether buttre runs as TSF keyboards or the hook tray\n  \
             --add-keyboard <id>       Add a buttre keyboard to your Windows list (telex, vni, nom, ...)\n  \
             --disable-layout-hotkey   Turn off Windows' Ctrl+Shift keyboard switching\n  \
             --version   Print version and exit\n  \
             --help      Print this help and exit",
            ver = env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    // Installer hooks: run before any backend or UI setup — they must work in
    // the MSI's non-interactive, elevated context where there is no session to
    // put a tray icon in.
    if args.iter().any(|a| a == "--register-tsf") {
        return run_tsf_registration(false);
    }
    if args.iter().any(|a| a == "--unregister-tsf") {
        return run_tsf_registration(true);
    }
    if args
        .iter()
        .any(|a| a == "--add-keyboard" || a == "--disable-layout-hotkey")
    {
        return run_keyboard_setup(&args);
    }
    if args.iter().any(|a| a == "--tsf-status") {
        return run_tsf_status();
    }
    if args.iter().any(|a| a == "--doctor") {
        run_doctor();
        return Ok(());
    }
    #[cfg(platform_linux)]
    if args.iter().any(|a| a == "--detect") {
        run_detect();
        return Ok(());
    }

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
        // Checkbox autostart chỉ hiện nơi tray tồn tại — trên nền tảng
        // OS-sở-hữu nó không điều khiển được gì (xem nhánh Os bên dưới).
        return buttre_config::run(method_owner::decide() == MethodOwner::Buttre);
    }

    // Ai sở hữu lựa chọn kiểu gõ trên máy này (ADR-0003). Trên
    // nền tảng OS-sở-hữu (IBus/fcitx5/macOS) buttre KHÔNG dựng tray — menu
    // của OS đã là nơi chọn kiểu gõ, tray chỉ là nơi điều khiển thứ hai.
    // `buttre` không tham số ở đó mở cửa sổ Cấu hình: người dùng bấm vào app
    // phải được một thứ hữu ích.
    // Login launch waits for a configured IBus/fcitx5 that is still
    // starting — nobody types during login, and deciding before the daemon
    // is up puts a tray on a desktop whose OS owns method choice. A manual
    // launch waits briefly only.
    let wait = if args.iter().any(|a| a == "--autostart") {
        std::time::Duration::from_secs(30)
    } else {
        std::time::Duration::from_secs(3)
    };
    let surface = method_owner::settled_surface(wait);
    let owner = method_owner::owner_of(surface);
    info!("method owner: {owner:?} (surface: {surface:?})");
    if owner == MethodOwner::Os {
        // Đưa entry autostart về dạng mới (`--autostart`): entry cũ chạy
        // `buttre` trần, mà ở nhánh này nghĩa là bật cửa sổ Cấu hình mỗi lần
        // đăng nhập. Ghi lại theo đúng setting hiện tại là idempotent và tự
        // lành sau một lần chạy. Windows (bàn phím TSF): entry `--autostart`
        // thoát im lặng ở nhánh dưới. macOS không có autostart (IMKit do hệ
        // thống khởi chạy — buttre-autostart bail có chủ ý).
        #[cfg(any(platform_linux, platform_windows))]
        if let Err(e) = buttre_autostart::set_enabled(Settings::load().startup) {
            log::warn!("autostart reconciliation on OS-owned platform failed: {e:?}");
        }
        // Login launch: engine do daemon/OS spawn, không có gì để chạy — và
        // tuyệt đối không được bật cửa sổ lên giữa lúc đăng nhập.
        if args.iter().any(|a| a == "--autostart") {
            info!("OS-owned platform: nothing to autostart (engine is daemon-spawned)");
            return Ok(());
        }
        return buttre_config::run(false);
    }

    // The tray (Windows Hook/TSF, Wayland-native). Never on macOS: the OS
    // input menu owns method choice there, so the Os branch above always
    // returns and this binary is only the settings helper — which is also
    // why winit (tray-only) is not a macOS dependency.
    #[cfg(not(platform_macos))]
    return tray_app::run();
    #[cfg(platform_macos)]
    anyhow::bail!("macOS input methods are OS-owned (ADR-0003); there is no tray")
}
