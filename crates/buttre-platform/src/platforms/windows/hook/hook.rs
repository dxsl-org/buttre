//! # Windows Keyboard Hook Backend
//!
//! ## ⚠️ ARCHITECTURE - SINGLE SOURCE OF TRUTH (READ THIS!)
//!
//! ### The KEYBOARD Global is the ONLY Source of Truth
//!
//! This hook uses `KEYBOARD` global (Arc<Mutex<Option<Keyboard>>>) to determine
//! if Vietnamese/Nôm/Custom input is active:
//!
//! - `KEYBOARD = Some(keyboard)` → Input method active (Telex/VNI/Nôm/Custom)
//! - `KEYBOARD = None` → English mode (passthrough)
//!
//! ### ❌ DO NOT USE SEPARATE FLAGS! (e.g., old VIETNAMESE_ENABLED)
//!
//! **Why?** Separate flags cause state synchronization bugs!
//!
//! **Previous Bug:**
//! - Had `VIETNAMESE_ENABLED` flag checked at line 369
//! - When user selected VNI from menu, keyboard was loaded but flag wasn't updated
//! - Result: VNI didn't work because flag was still false
//!
//! **Current Solution:**
//! - Only check `KEYBOARD.is_some()` directly
//! - No separate flag → No state mismatch possible!
//!
//! ## Flow:
//! ```text
//! OS Keystroke → Hook Callback
//!     ↓
//! Check KEYBOARD.is_some() ← SINGLE CHECK, NO FLAGS!
//!     ↓ (if Some)
//! KEYBOARD.lock().process(char) → Action
//!     ↓
//! send_backspaces/send_string
//! ```
//!
//! ## Method Change Flow:
//! ```text
//! User selects method (menu/hotkey)
//!     ↓
//! AppState.set_method("vni")
//!     ↓
//! KeyboardObserver.on_method_changed()
//!     ↓
//! KeyboardManager.set_method("vni")
//!     ↓
//! *KEYBOARD.lock() = Some(KeyboardBuilder::vni())
//!     ↓
//! Hook automatically sees new keyboard (shared Arc!)
//! NO NEED TO SET ANY FLAG! ✅
//! ```
//!
//! ## Key Points:
//! - Uses SetWindowsHookEx with WH_KEYBOARD_LL to capture ALL keystrokes
//! - Checks `KEYBOARD.is_some()` to determine if method is active (NOT a flag!)
//! - Shares KEYBOARD Arc with KeyboardManager (automatic sync!)
//! - Executes Action by injecting backspaces/text via SendInput


use std::sync::{Arc, Mutex, RwLock, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicIsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};
use buttre_core::Action;
use buttre_core::Keyboard;

use super::profiling::{ProfileTimer, HOOK_PROFILER};
use super::queue::{QueueProcessor, KeyEvent, timestamp_us};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, 
    KBDLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WH_MOUSE_LL, 
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN,
};

use crate::platforms::windows::common::{
    is_buffer_reset_key, is_special_key, send_backspaces, send_string, send_replacement,
    VK_BACK,
    show_candidates, hide_candidates,
};
use crate::platforms::windows::common::input::BUTTRE_INJECTED;

/// Global state - necessary for hook callback
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static MOUSE_HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

// ============================================================================
// KEYBOARD: SINGLE SOURCE OF TRUTH
// ============================================================================
// This is the ONLY state we check to determine if input method is active!
//
// - Some(keyboard) = Vietnamese/VNI/Nôm/Custom mode
// - None = English mode
//
// DO NOT add separate flags here! They cause state sync bugs!
// See file header comment for full explanation.
//
// OPTIMIZATION (Phase 4, Task 3): Using RwLock instead of Mutex
// - Read-heavy workload: Most operations just check state or call process()
// - RwLock allows multiple concurrent readers → Lower contention
// - Only method switching needs write lock (rare operation)
// ============================================================================
static KEYBOARD: OnceLock<Arc<RwLock<Option<Keyboard>>>> = OnceLock::new();

// ============================================================================
// QUEUE PROCESSOR: Async processing to optimize hook callback
// ============================================================================
// The queue processor decouples hook callback from processing:
// - Hook enqueues events (~10-50μs) and returns immediately
// - Background thread dequeues and processes asynchronously
// - Total latency still < 2ms (imperceptible to user)
//
// This dramatically reduces hook callback time and prevents system freezes.
// ============================================================================
static QUEUE_PROCESSOR: OnceLock<Mutex<Option<QueueProcessor>>> = OnceLock::new();

// Optimization: Track if the keyboard engine is "dirty" (has active state)
// This allows us to skip acquiring the Mutex for reset_engine() if we know
// the engine is already clean.
//
// Rules:
// - Set to TRUE when kb.process() is called
// - Set to FALSE when kb.reset() is called
// - Spurious TRUE is safe (just causes unnecessary lock)
// - Spurious FALSE is unsafe (missed reset) - causing bug
// - Initial state: FALSE (new keyboard is clean)
static KEYBOARD_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// PROCESSING: lock-free re-entrancy guard.
//
// Was `OnceLock<Mutex<bool>>` — but `.lock().unwrap()` inside an `extern "system"`
// hook callback can panic on poison, and a panic across the FFI boundary into Windows
// is UB and can hang every keystroke on the desktop. AtomicBool cannot poison.
//
// Set via `compare_exchange` in keyboard_proc; cleared by a local Drop guard on
// every return path (including any future panic the Rust unwinder traverses).
static PROCESSING: AtomicBool = AtomicBool::new(false);

/// Fast typing detection: last key and timestamp (for English escape)
/// If same key pressed twice within 150ms, treat as escape to English
static LAST_KEY: AtomicU8 = AtomicU8::new(0);
static LAST_KEY_TIME: AtomicU64 = AtomicU64::new(0);
const FAST_TYPING_THRESHOLD_MS: u64 = 150; // ms between same keys to trigger escape

// [rest of struct ModifierState and get_modifier_state]

// ... (Skip to helper functions)

/// Helper to reset keyboard buffer
#[inline]
fn reset_engine() {
    // Optimization: If engine is clean, don't acquire lock!
    if !KEYBOARD_DIRTY.load(Ordering::Acquire) {
        return;
    }

    if let Some(keyboard) = KEYBOARD.get() {
        // Blocking, poison-tolerant write (was try_write).  A skipped reset on
        // a word-boundary key (Enter/Tab/arrows) leaves the previous word's
        // state in the engine, so the next line diffs against it and "jumps
        // back" to the prior line.  No caller holds the lock when reset_engine
        // runs, so a brief block is safe.
        let mut kb_opt = match keyboard.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(ref mut kb) = *kb_opt {
            kb.reset();
            KEYBOARD_DIRTY.store(false, Ordering::Release);
        }
    }
}

// Update mouse_proc to use optimized reset_engine
#[cfg(windows)]
// SAFETY:
// 1. This is a Windows hook callback - must use extern "system" calling convention
// 2. Called by Windows OS with valid parameters per SetWindowsHookExW contract
// 3. code, wparam, lparam are provided by Windows and guaranteed valid for hook context
// 4. CallNextHookEx is properly declared in windows_sys
// 5. Must return LRESULT per Windows hook chain protocol
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Get mouse hook handle for CallNextHookEx
    let hook_handle = MOUSE_HOOK_HANDLE.load(Ordering::Relaxed);
    
    // Always call next hook if code < 0
    if code < 0 {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }
    
    // Check for mouse button down events
    let is_click = matches!(wparam as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN);
    
    if is_click {
        // Reset keyboard buffer on mouse click using optimized helper
        reset_engine();
        hide_candidates(); // Hide candidate window on mouse click
        if KEYBOARD_DIRTY.load(Ordering::Relaxed) {
             debug!("Buffer reset on mouse click");
        }
        
        // Reset fast typing state on mouse click
        LAST_KEY.store(0, Ordering::Relaxed);
        LAST_KEY_TIME.store(0, Ordering::Relaxed);
    }
    
    // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
    unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) }
}

/// Modifier key state snapshot
/// Consolidates multiple GetKeyState calls into a single struct
#[derive(Debug, Clone, Copy)]
struct ModifierState {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    caps: bool,
}

/// Get all modifier states in one pass
/// Reduces syscall overhead from 8 calls to 5 calls per keystroke
#[cfg(windows)]
// SAFETY:
// 1. GetKeyState is properly declared in windows_sys
// 2. VK_* constants are valid Windows virtual key codes from Windows SDK
// 3. GetKeyState returns i16 where high bit indicates key pressed
// 4. Bit 0 of GetKeyState indicates toggle state (for Caps Lock)
// 5. Called from hook context where GetKeyState is valid
unsafe fn get_modifier_state() -> ModifierState {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    // SAFETY: GetKeyState is safe to call from hook context with valid VK_ constants
    unsafe {
        ModifierState {
            ctrl: (GetKeyState(VK_CONTROL as i32) as i16) < 0,
            alt: (GetKeyState(VK_MENU as i32) as i16) < 0,
            shift: (GetKeyState(VK_SHIFT as i32) as i16) < 0,
            win: (GetKeyState(VK_LWIN as i32) as i16) < 0 || (GetKeyState(VK_RWIN as i32) as i16) < 0,
            caps: (GetKeyState(VK_CAPITAL as i32) & 1) != 0,
        }
    }
}

/// Install the keyboard hook
#[cfg(windows)]
pub fn install_hook(
    keyboard: Arc<RwLock<Option<Keyboard>>>,
    _callback: Option<Box<dyn Fn(bool) + Send + Sync>>,
) -> anyhow::Result<()> {
    // Initialize global state
    // Note: Hooks are atomics now, no need to init mutex
    let _ = KEYBOARD.set(keyboard.clone());
    // PROCESSING is now a plain AtomicBool — no initialization needed.
    
    // Initialize and start queue processor (if async-queue feature enabled)
    #[cfg(feature = "async-queue")]
    {
        let mut processor = QueueProcessor::new(keyboard.clone(), 1000); // Max 1000 events
        processor.start()?;
        let _ = QUEUE_PROCESSOR.set(Mutex::new(Some(processor)));
        info!("Queue processor enabled (async-queue feature)");
    }
    #[cfg(not(feature = "async-queue"))]
    {
        info!("Queue processor disabled (sync mode)");
    }
    
    // Initialize candidate window
    use crate::platforms::windows::common::candidate_window;
    if let Err(e) = candidate_window::init() {
        warn!("Failed to initialize candidate window: {}", e);
    }

    // Install keyboard hook
    // SAFETY:
    // 1. SetWindowsHookExW is properly declared in windows_sys - function pointer is valid
    // 2. WH_KEYBOARD_LL is a valid hook type constant from Windows SDK
    // 3. keyboard_proc is a valid extern "system" function with correct signature
    // 4. GetModuleHandleW(null) returns current module handle - valid for DLL hooks
    // 5. Thread ID 0 means hook is global (all threads in desktop)
    let kb_handle = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };

    if kb_handle == 0 {
        error!("SetWindowsHookEx (keyboard) failed");
        anyhow::bail!("Failed to install keyboard hook");
    }

    HOOK_HANDLE.store(kb_handle as isize, Ordering::SeqCst);

    // Install mouse hook
    // SAFETY:
    // 1. Same invariants as keyboard hook above
    // 2. WH_MOUSE_LL is a valid hook type for low-level mouse events
    // 3. mouse_proc has correct extern "system" signature
    let mouse_handle = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };

    if mouse_handle == 0 {
        warn!("SetWindowsHookEx (mouse) failed - continuing without mouse support");
    } else {
        MOUSE_HOOK_HANDLE.store(mouse_handle as isize, Ordering::SeqCst);
        info!("Mouse hook installed (handle: {:?})", mouse_handle);
    }

    // Set thread priority for better responsiveness
    // SAFETY:
    // 1. GetCurrentThread returns a pseudo-handle for current thread - always valid
    // 2. THREAD_PRIORITY_ABOVE_NORMAL is a valid constant from Windows SDK
    // 3. SetThreadPriority is properly declared in windows_sys
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }

    info!(
        "Keyboard hook installed (handle: {:?}, priority: ABOVE_NORMAL)",
        kb_handle
    );
    
    // Start profiling stats printer thread (prints stats every 30 seconds)
    std::thread::spawn(|| {
        use std::time::Duration;
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let stats = HOOK_PROFILER.get_stats();
            if stats.total_calls > 0 {
                info!("\n{}", stats);
            }
        }
    });
    
    Ok(())
}

#[cfg(not(windows))]
pub fn install_hook(
    _keyboard: Arc<RwLock<Option<Keyboard>>>,
    _callback: Option<Box<dyn Fn(bool) + Send + Sync>>,
) -> anyhow::Result<()> {
    anyhow::bail!("Keyboard hook is only supported on Windows")
}

/// Uninstall the keyboard and mouse hooks
#[cfg(windows)]
pub fn uninstall_hook() -> anyhow::Result<()> {
    // Stop queue processor first
    if let Some(processor_mutex) = QUEUE_PROCESSOR.get() {
        if let Ok(mut processor_opt) = processor_mutex.lock() {
            if let Some(processor) = processor_opt.take() {
                drop(processor); // Calls stop() in Drop impl
                info!("Queue processor stopped");
            }
        }
    }
    
    // Uninstall keyboard hook
    let current_kb_handle = HOOK_HANDLE.load(Ordering::SeqCst);
    if current_kb_handle != 0 {
        // SAFETY:
        // 1. current_kb_handle was obtained from SetWindowsHookExW and stored in HOOK_HANDLE
        // 2. We check it's non-zero before using it
        // 3. UnhookWindowsHookEx is properly declared in windows_sys
        // 4. Even if handle is invalid, UnhookWindowsHookEx just returns failure (safe)
        unsafe { UnhookWindowsHookEx(current_kb_handle as _) };
        HOOK_HANDLE.store(0, Ordering::SeqCst);
        info!("Keyboard hook uninstalled");
    }
    
    // Uninstall mouse hook
    let current_mouse_handle = MOUSE_HOOK_HANDLE.load(Ordering::SeqCst);
    if current_mouse_handle != 0 {
        // SAFETY:
        // 1. Same invariants as keyboard hook uninstall above
        // 2. current_mouse_handle is from SetWindowsHookExW, validated non-zero
        unsafe { UnhookWindowsHookEx(current_mouse_handle as _) };
        MOUSE_HOOK_HANDLE.store(0, Ordering::SeqCst);
        info!("Mouse hook uninstalled");
    }
    
    Ok(())
}

#[cfg(not(windows))]
pub fn uninstall_hook() -> anyhow::Result<()> {
    Ok(())
}

// ============================================================================
// REMOVED: set_vietnamese_enabled() function
// ============================================================================
// This function was removed because we no longer use a separate flag!
// The hook now checks KEYBOARD.is_some() directly (single source of truth).
//
// If you're looking for how to enable/disable input method:
//   - It's automatic! Just update KEYBOARD via KeyboardManager
//   - KeyboardManager.set_method("vni") → KEYBOARD = Some(keyboard)
//   - KeyboardManager.set_method("english") → KEYBOARD = None
//   - Hook sees changes immediately (shared Arc)
// ============================================================================

/// Run Windows message loop (required for hook to work)
#[cfg(windows)]
pub fn run_message_loop() {
    info!("Starting message loop for keyboard hook");
    // SAFETY:
    // 1. MSG is a POD struct - zeroed() creates valid zero-initialized instance
    // 2. GetMessageW is properly declared in windows_sys
    // 3. HWND 0 means retrieve messages for current thread
    // 4. Filter min/max 0,0 means retrieve all messages
    // 5. GetMessageW returns > 0 for normal messages, 0 for WM_QUIT, < 0 for error
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            // Process messages
        }
    }
}

#[cfg(not(windows))]
pub fn run_message_loop() {}

/// Keyboard hook callback
#[cfg(windows)]
// SAFETY:
// 1. This is a Windows hook callback - must use extern "system" calling convention
// 2. Called by Windows OS with valid parameters per SetWindowsHookExW contract
// 3. code, wparam, lparam are provided by Windows and guaranteed valid for hook context
// 4. lparam points to KBDLLHOOKSTRUCT allocated by Windows - valid during callback
// 5. CallNextHookEx is properly declared in windows_sys
// 6. Must return LRESULT per Windows hook chain protocol
// 7. CRITICAL: Never block or take long locks in hook callbacks (can freeze system)
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Start profiling timer - will automatically record on drop
    // Track as passthrough initially, will update if we process Vietnamese
    let mut _timer = ProfileTimer::start(false);
    
    // Get hook handle for CallNextHookEx
    let hook_handle = HOOK_HANDLE.load(Ordering::Relaxed);

    // Always call next hook if code < 0
    if code < 0 {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // Get keyboard info
    // SAFETY:
    // 1. lparam is a valid pointer to KBDLLHOOKSTRUCT provided by Windows
    // 2. KBDLLHOOKSTRUCT is valid for the duration of this callback
    // 3. We're making a copy of the struct (not holding the pointer)
    let kb = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };

    // CRITICAL: Skip our own injected keys to prevent infinite loop
    if kb.dwExtraInfo == BUTTRE_INJECTED {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // Only process key down events for Vietnamese conversion
    // But we need to handle BOTH key down and key up for toggle detection
    let is_key_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
    let is_key_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

    if !is_key_down && !is_key_up {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // Get all modifier states in one pass
    // SAFETY: get_modifier_state calls GetKeyState which is safe from hook context
    let mods = unsafe { get_modifier_state() };
    
    // HOTKEY REMOVED: Now using global-hotkey crate

    // Only process key DOWN events for Vietnamese conversion from here
    if !is_key_down {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    let vk = kb.vkCode as u16;

    // 1. HIGHEST PRIORITY: Modifiers (Ctrl/Alt/Win)
    // If modifiers are pressed, we must RESET the engine to prevent state pollution
    // e.g. Ctrl+Z (Undo), Ctrl+Backspace (Delete Word), Alt+Tab...
    // Allows shortcuts to pass through cleanly
    if mods.ctrl || mods.alt || mods.win {
        reset_engine();
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 2. BUFFER RESET KEYS: Movement, terminators, function keys, etc.
    // Consolidated check using is_buffer_reset_key() based on UniKey behavior.
    // This includes: arrows, Home, End, PgUp/Down, Enter, Tab, Escape, Insert, Delete, F1-F24
    if is_buffer_reset_key(vk) {
        // Reset the composition on a word-boundary key so the next word starts
        // fresh.  CRITICAL: do NOT hold any lock here — reset_engine() acquires
        // the write lock itself, and a read lock held across that call makes the
        // write fail, silently skipping the reset (the Enter "jump-back" bug).
        // reset_engine()/hide_candidates() are no-ops in English mode or when
        // nothing is composing, so calling them unconditionally is safe.
        //
        // Force the dirty flag so reset_engine() ALWAYS performs the reset on a
        // word-boundary key — never gated by KEYBOARD_DIRTY tracking.  This is
        // the same pattern the Nôm candidate paths use and removes any chance of
        // a stale composition surviving Enter/Tab/arrow (the jump-back bug).
        KEYBOARD_DIRTY.store(true, Ordering::Release);
        reset_engine();
        hide_candidates(); // Hide candidate window on buffer reset
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 4. BACKSPACE: Special handling
    if vk == VK_BACK {
        // SPECIAL CASE: Backspace when candidates are showing
        // Need to delete input text + candidates, then re-display updated state with new candidates
        if is_candidates_showing() {
            use crate::platforms::windows::common::{get_input_text_len, get_candidates_text_len};
            
            let input_len = get_input_text_len();
            let candidates_len = get_candidates_text_len();
            let total_len = input_len + candidates_len;
            
            // Hide candidates first (will be re-shown if buffer not empty)
            hide_candidates();
            
            if total_len > 0 {
                // Delete all displayed text (input + candidates)
                send_backspaces(total_len);
                
                // Update internal buffer, sync executor state, and get new candidates
                if let Some(keyboard) = KEYBOARD.get() {
                    if let Ok(mut kb_opt) = keyboard.try_write() {
                        if let Some(ref mut kb) = *kb_opt {
                            // Use backspace_with_candidates to properly sync executor state
                            if let Some((buffer_str, candidates)) = kb.backspace_with_candidates() {
                                // Display remaining buffer (if any)
                                if !buffer_str.is_empty() {
                                    send_string(&buffer_str);
                                    
                                    // If we have candidates, show them
                                    if !candidates.is_empty() {
                                        // Convert to buttre_engine::pipeline::Candidate format
                                        use buttre_engine::pipeline::Candidate as EngineCandidate;
                                        
                                        let engine_candidates: Vec<EngineCandidate> = candidates.into_iter()
                                            .take(10) // Limit to 10
                                            .collect();
                                        
                                        if let Some(formatted) = show_candidates(engine_candidates.clone(), buffer_str.clone()) {
                                            send_string(&formatted);
                                        }
                                        
                                    }
                                }
                            }
                        }
                    }
                }
                
                return 1; // Block original backspace
            }
        }
        
        // ASYNC MODE: Enqueue backspace event
        #[cfg(feature = "async-queue")]
        let use_async = true;
        #[cfg(not(feature = "async-queue"))]
        let use_async = false;
        
        if use_async {
            if let Some(processor_mutex) = QUEUE_PROCESSOR.get() {
                if let Ok(processor_opt) = processor_mutex.lock() {
                    if let Some(processor) = processor_opt.as_ref() {
                        let event = KeyEvent::Backspace { timestamp_us: timestamp_us() };
                        if !processor.enqueue(event) {
                            warn!("Failed to enqueue backspace - queue full");
                        }
                        return 1; // Block key - will be processed async
                    }
                }
            }
            // Fallback to sync if queue not available
        }
        
        // SYNC MODE: Process in callback.
        // Blocking, poison-tolerant write() — see the char path for the full
        // rationale.  A dropped backspace here would let the SYSTEM delete a
        // character while the engine buffer stayed unchanged (the exact desync
        // the comment below warns about), so the lock must not be skipped.
        if let Some(keyboard) = KEYBOARD.get() {
            let result = {
                let mut kb_opt = match keyboard.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("Keyboard RwLock poisoned — recovering");
                        poisoned.into_inner()
                    }
                };
                if let Some(ref mut kb) = *kb_opt {
                    // Keyboard loaded - handle backspace through engine
                    match kb.backspace() {
                        Ok(action) => {
                            action
                        },
                        Err(e) => {
                            warn!("Keyboard backspace error: {}", e);
                            Action::DoNothing
                        }
                    }
                } else {
                    // English mode - no keyboard, let system handle backspace
                    Action::DoNothing
                }
            };
            
            match result {
                Action::Replace { backspace_count, text } => {
                    // IMPORTANT: Always emit backspaces manually, NEVER let system handle!
                    // Letting system handle causes buffer desync because:
                    // 1. kb.backspace() already modified internal buffer
                    // 2. CallNextHookEx passes to system, which deletes on screen
                    // 3. But next backspace enters hook with NEW context, finds empty buffer
                    // This is why Unikey always emits fake backspaces via keybd_event.
                    
                    if backspace_count > 0 || !text.is_empty() {
                        if backspace_count > 0 {
                            send_backspaces(backspace_count);
                        }
                        
                        // NOTE: Removed thread::sleep() - NEVER sleep in hook callback!
                        // Sleep causes keystroke backlog and system freeze.
                        // SendInput batching should handle timing.
                        
                        if !text.is_empty() {
                            send_string(&text);
                        }
                        return 1; // Block original Backspace
                    }
                }
                _ => {}
            }
        }
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 5. SKIP SPECIAL KEYS (Function keys, etc.)
    if is_special_key(vk) {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 6. RECURSION CHECK (atomic, panic-safe)
    // CAS-claim the flag; bail out if another invocation is already in flight.
    // No panic-on-poison failure mode (unlike the previous Mutex<bool>).
    if PROCESSING.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }
    // The guard clears PROCESSING on every return path (including unwinding).
    struct ProcessingGuard;
    impl Drop for ProcessingGuard {
        fn drop(&mut self) {
            PROCESSING.store(false, Ordering::Release);
        }
    }
    let _processing_guard = ProcessingGuard;

    // 7. CONVERT TO CHAR
    let ch = match vk_to_char(vk, mods.shift, mods.caps) {
        Some(c) => c,
        None => {
            // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
            return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
        }
    };

    // ========================================================================
    // 8. KEYBOARD CHECK - SINGLE SOURCE OF TRUTH ⭐ CRITICAL!
    // ========================================================================
    // IMPORTANT: We check KEYBOARD.is_some() to determine if input method is active.
    // DO NOT use a separate flag (like old VIETNAMESE_ENABLED) because it creates
    // state synchronization issues!
    //
    // Architecture Flow:
    //   User selects method → KeyboardManager updates KEYBOARD global
    //   Hook checks → if KEYBOARD.is_some() → process input
    //                 if KEYBOARD.is_none() → English mode, pass through
    //
    // This ensures:
    //   - Single source of truth (KEYBOARD global)
    //   - No state mismatch between flag and actual keyboard
    //   - Works for all methods: Telex, VNI, Nôm, Custom
    // ========================================================================
    if let Some(keyboard) = KEYBOARD.get() {
        if let Ok(kb_opt) = keyboard.try_read() {
            if kb_opt.is_none() {
                // No keyboard loaded = English mode
                                // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
                return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
            }
            // Drop lock immediately - we'll lock again for processing below
        } else {
            // Lock failed - keyboard busy, skip this keystroke
                        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
            return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
        }
    } else {
        // KEYBOARD not initialized yet
                // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 9. CANDIDATE SELECTION (1-5)
    // Check if candidates are showing and user pressed 1-5
    use crate::platforms::windows::common::is_candidates_showing;
    if is_candidates_showing() && ch >= '1' && ch <= '5' {
        let number = (ch as u8) - b'0';
        if let Some((selected, backspace_count)) = select_candidate(number) {
            info!("Candidate selected: {} -> {} (backspace: {})", number, selected, backspace_count);
            // Xóa các ký tự đã gõ trước
            if backspace_count > 0 {
                send_backspaces(backspace_count);
            }
            // Chèn chữ Nôm đã chọn
            send_string(&selected);
            // Mark engine as dirty so reset_engine() will actually reset
            KEYBOARD_DIRTY.store(true, Ordering::Release);
            // Reset engine state
            reset_engine();
            // Block the number key
                        return 1; // Block key
        }
    }

    // 9b. SPACE KEY - AUTO-SELECT SINGLE CANDIDATE (for Nôm input)
    // If space is pressed while candidates are showing:
    // - 1 candidate → auto-select it
    // - Multiple candidates → block space, let engine process internally without outputting space
    use crate::platforms::windows::common::{get_candidates_count, select_candidate};
    let candidates_showing = is_candidates_showing();
    let candidate_count = if candidates_showing { get_candidates_count() } else { 0 };
    
    if ch == ' ' && candidates_showing {
        
        if candidate_count == 1 {
            // Exactly 1 candidate - auto-select it
            if let Some((selected, backspace_count)) = select_candidate(1) {
                info!("Space auto-select: {} (backspace: {})", selected, backspace_count);
                // Delete input + candidates display
                if backspace_count > 0 {
                    send_backspaces(backspace_count);
                }
                // Insert selected Nôm character
                send_string(&selected);
                // Insert space after selection
                send_string(" ");
                // Hide candidates UI state
                hide_candidates();
                // Mark engine as dirty so reset_engine() will actually reset
                KEYBOARD_DIRTY.store(true, Ordering::Release);
                // Reset engine state
                reset_engine();
                // Block the space key
                                return 1; // Block key
            }
        }
        // Multiple candidates - BLOCK space to prevent buffer commit
        // This allows continuing to type for multi-keyword search
        // But we need to let the space go through to engine for the buffer
        // So we'll just continue to normal processing (don't return)
    }

    // 10. FAST TYPING ESCAPE
    if check_fast_typing_escape(ch) {
        debug!("Fast typing escape detected for '{}'", ch);
                reset_engine();
        // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
        return unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) };
    }

    // 11. KEYBOARD PROCESS
    // Two modes: async-queue (fast, enqueue only) or sync (process in callback)
    #[cfg(feature = "async-queue")]
    let use_async = true;
    #[cfg(not(feature = "async-queue"))]
    let use_async = false;
    
    if use_async {
        // ASYNC MODE: Enqueue event and return immediately (~10-50μs)
        if let Some(processor_mutex) = QUEUE_PROCESSOR.get() {
            if let Ok(processor_opt) = processor_mutex.lock() {
                if let Some(processor) = processor_opt.as_ref() {
                    _timer.mark_vietnamese();
                    let event = KeyEvent::Character { ch, timestamp_us: timestamp_us() };
                    if !processor.enqueue(event) {
                        warn!("Failed to enqueue character '{}' - queue full", ch);
                    }
                    // Clear processing flag and return
                                        return 1; // Block key - will be processed async
                }
            }
        }
        // Fallback to sync if queue not available
    }
    
    // SYNC MODE: Process in callback (original behavior).
    //
    // Lock acquisition is a BLOCKING, poison-tolerant write() — NOT try_write().
    // try_write() silently dropped the keystroke on contention, but because the
    // raw key is not blocked in that case (block_key stays false on DoNothing),
    // the unprocessed character leaked to the screen while the engine buffer
    // stayed behind.  That desynced last_output and corrupted the rest of the
    // word under fast typing.  Every lock holder is µs-scale (kb.process / a
    // config write on the pipe thread), so a brief block here is safe — this is
    // the synchronous model mature IMEs (OpenKey, GoNhanh) rely on, and the OS
    // LowLevelHooksTimeout remains the ultimate backstop.
    let result = if let Some(keyboard) = KEYBOARD.get() {
        let mut kb_opt = match keyboard.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Keyboard RwLock poisoned — recovering");
                poisoned.into_inner()
            }
        };
        {
            if let Some(ref mut kb) = *kb_opt {
                // Mark as Vietnamese input for profiling
                _timer.mark_vietnamese();

                match kb.process(ch) {
                    Ok(actions) => {
                        // Mark dirty - engine state changed (or at least processed input)
                        KEYBOARD_DIRTY.store(true, Ordering::Release);
                        
                        // Process all actions (main action + candidate UI)
                        let mut main_action = Action::DoNothing;
                        
                        for action in actions {
                            match action {
                                Action::ShowCandidates { candidates, input } => {
                                    // Xóa candidates cũ trước nếu đang hiển thị
                                    use crate::platforms::windows::common::get_candidates_text_len;
                                    let old_candidates_len = get_candidates_text_len();
                                    
                                    // Show candidates inline: "1天 2𡗶 3霄 4𡗶 5天"
                                    if let Some(candidates_text) = show_candidates(candidates, input) {
                                        // Merge main_action with candidates display
                                        main_action = match main_action {
                                            Action::Replace { backspace_count, text } => {
                                                // Merge: backspace old candidates + main backspaces, then show main text + candidates
                                                Action::Replace {
                                                    backspace_count: backspace_count + old_candidates_len,
                                                    text: text + &candidates_text,
                                                }
                                            }
                                            Action::Commit(text) => {
                                                // Merge: delete old candidates, show committed text + candidates
                                                Action::Replace {
                                                    backspace_count: old_candidates_len,
                                                    text: text + &candidates_text,
                                                }
                                            }
                                            Action::DoNothing => {
                                                // Just show candidates (delete old + show new)
                                                Action::Replace {
                                                    backspace_count: old_candidates_len,
                                                    text: candidates_text,
                                                }
                                            }
                                            other => other, // Keep other actions as-is
                                        };
                                    }
                                }
                                Action::HideCandidates => {
                                    // Hide candidates: need to delete the displayed text
                                    use crate::platforms::windows::common::get_candidates_text_len;
                                    let candidates_len = get_candidates_text_len();
                                    if candidates_len > 0 {
                                        send_backspaces(candidates_len);
                                    }
                                    hide_candidates();
                                }
                                // Other actions are main actions (take the first one)
                                other => {
                                    if matches!(main_action, Action::DoNothing) {
                                        main_action = other;
                                    }
                                }
                            }
                        }
                        
                        main_action
                    },
                    Err(e) => {
                        warn!("Keyboard process error: {}", e);
                        Action::DoNothing
                    }
                }
            } else {
                // No keyboard loaded (English mode)
                Action::DoNothing
            }
        }
    } else {
        Action::DoNothing
    };

    // 11. HANDLE RESULT
    let block_key = match result {
        Action::Replace { backspace_count, text } => {
            if backspace_count > 0 || !text.is_empty() {
                // IMPORTANT: Always emit backspaces manually, NEVER let system handle!
                // See comment in backspace handler (step 5) for detailed explanation.
                
                // Batch send backspaces and text
                send_replacement(backspace_count, &text);
                true
            } else {
                false
            }
        }
        Action::Commit(text) => {
            if !text.is_empty() {
                info!("Commit: '{}'", text);
                send_string(&text);
                true
            } else {
                false
            }
        }
        Action::DoNothing => {
            // FIX: If method returns DoNothing for separator keys (Space, Enter, Tab),
            // we should reset the engine to prevent buffer pollution.
            // This matches Unikey behavior where separators always reset the buffer.
            if ch == ' ' || ch == '\n' || ch == '\t' {
                reset_engine();
            }
            false
        }
        // For Hook backend, we treat Composition events as DoNothing for now.
        // In the future, we could simulate composition via Replace if needed,
        // but currently the Engine outputs Replace for non-TSF contexts.
        Action::UpdateComposition { .. } | Action::ConfirmComposition(_) => {
            if ch == ' ' || ch == '\n' || ch == '\t' {
                reset_engine();
            }
            false
        }
        // Candidate UI actions are ignored in Hook mode (only for TSF)
        // TODO: Implement fake candidate window for Hook mode
        Action::ShowCandidates { .. } | Action::HideCandidates => {
            false
        }
    };

    // PROCESSING flag is cleared automatically by ProcessingGuard's Drop impl.

    // SAFETY: CallNextHookEx is safe because hook_handle is valid from SetWindowsHookExW
    if block_key { 1 } else { unsafe { CallNextHookEx(hook_handle as _, code, wparam, lparam) } }
}



/// Convert virtual key code to character
#[cfg(windows)]
fn vk_to_char(vk: u16, shift: bool, caps: bool) -> Option<char> {
    match vk {
        // Letters A-Z (VK_A = 0x41 to VK_Z = 0x5A)
        0x41..=0x5A => {
            // XOR: shift ^ caps for uppercase
            let uppercase = shift ^ caps;
            if uppercase {
                Some((vk as u8) as char) // A-Z
            } else {
                Some((vk as u8 + 32) as char) // a-z
            }
        }

        // Numbers 0-9 (VK_0 = 0x30 to VK_9 = 0x39)
        0x30..=0x39 => {
            if shift {
                // Shifted number keys
                match vk {
                    0x30 => Some(')'),
                    0x31 => Some('!'),
                    0x32 => Some('@'),
                    0x33 => Some('#'),
                    0x34 => Some('$'),
                    0x35 => Some('%'),
                    0x36 => Some('^'),
                    0x37 => Some('&'),
                    0x38 => Some('*'),
                    0x39 => Some('('),
                    _ => None,
                }
            } else {
                Some((vk as u8) as char)
            }
        }

        // Space (VK_SPACE = 0x20)
        0x20 => Some(' '),

        // Common punctuation (unshifted)
        0xBE => Some(if shift { '>' } else { '.' }), // VK_OEM_PERIOD
        0xBC => Some(if shift { '<' } else { ',' }), // VK_OEM_COMMA
        0xBD => Some(if shift { '_' } else { '-' }), // VK_OEM_MINUS
        0xBB => Some(if shift { '+' } else { '=' }), // VK_OEM_PLUS
        0xBA => Some(if shift { ':' } else { ';' }), // VK_OEM_1
        0xBF => Some(if shift { '?' } else { '/' }), // VK_OEM_2
        0xC0 => Some(if shift { '~' } else { '`' }), // VK_OEM_3
        0xDB => Some(if shift { '{' } else { '[' }), // VK_OEM_4
        0xDC => Some(if shift { '|' } else { '\\' }), // VK_OEM_5
        0xDD => Some(if shift { '}' } else { ']' }), // VK_OEM_6
        0xDE => Some(if shift { '"' } else { '\'' }), // VK_OEM_7

        _ => None,
    }
}

#[cfg(not(windows))]
fn vk_to_char(_vk: u16) -> Option<char> {
    None
}

/// Get current timestamp in milliseconds
#[inline]
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Check if this is a fast repeated key (same key pressed within threshold)
/// Returns true if this should escape to English mode
#[inline]
fn check_fast_typing_escape(ch: char) -> bool {
    let now = current_time_ms();
    let last_key = LAST_KEY.load(Ordering::Relaxed);
    let last_time = LAST_KEY_TIME.load(Ordering::Relaxed);
    
    // Update last key info
    let ch_byte = (ch as u32).min(255) as u8;
    LAST_KEY.store(ch_byte, Ordering::Relaxed);
    LAST_KEY_TIME.store(now, Ordering::Relaxed);
    
    // Check if same key pressed within threshold
    if last_key == ch_byte && now - last_time < FAST_TYPING_THRESHOLD_MS {
        // Fast repeated key detected - this could be intentional English
        // But we only escape if the key is a tone/modifier key (s, f, r, x, j, z, w)
        // Note: 'd' and 'e' removed to allow dd -> đ, ee -> ê
        matches!(ch.to_ascii_lowercase(), 's' | 'f' | 'r' | 'x' | 'j' | 'z' | 'w')
    } else {
        false
    }
}

