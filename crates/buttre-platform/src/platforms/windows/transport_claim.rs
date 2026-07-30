//! The TSF transport claim — how the hook learns that the text service owns
//! the foreground window, so exactly one of the two ever processes a key.
//!
//! Windows runs two delivery paths with different coverage: TSF reaches
//! cooperating apps (including elevated windows, where UIPI blocks the hook's
//! `SendInput`), the hook reaches nearly everything else. Running both per
//! session is the only way to cover the union — but both acting on the SAME
//! keystroke writes it twice and corrupts the user's text. This module is the
//! arbitration: the DLL PUBLISHES "thread N's window is mine" when Windows
//! activates it, and the hook STANDS DOWN whenever the foreground window
//! belongs to that thread.
//!
//! Failure asymmetry drives every default here (ADR-0003 invariant 4): "no
//! layer runs" is a visible nuisance the user retypes; "both run" is garbled
//! text they cannot explain. So the claim means STAND DOWN, never "go" — a
//! corrupt or hostile value can silence the hook (annoying, diagnosable), but
//! nothing written into this region can make two layers type at once.
//!
//! ## Shape
//!
//! One named file mapping (`Local\buttre_tsf_claim`) holding a single `u32`:
//! the thread id whose window currently hosts an ACTIVE buttre text service,
//! or 0 for none. `Local\` scopes it to the user's session — input state must
//! not cross sessions, and `Global\` would need privileges anyway.
//!
//! Writers: only the DLL (inside the host app). Reader: the hook (tray
//! process), once per keystroke — a mapped atomic load, ~nanoseconds, and
//! deliberately NEVER value-cached: a cache is where stale state would live,
//! and stale state here is the double-typing bug.
//!
//! ## Self-healing
//!
//! A host app that crashes never clears its claim. The reader therefore
//! verifies the claimed thread still exists before honouring it —
//! `OpenThread` + `GetExitCodeThread`, on the READER side, because a dead
//! writer by definition cannot clean up after itself. The check runs only
//! when the claim matches the foreground thread (the stand-down path), so it
//! adds nothing to the hook's composing path.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    GetExitCodeThread, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
};

/// The one shared `u32` — see the module doc. `None` when the mapping could
/// not be created/mapped: publishing degrades to a no-op and reading to "no
/// claim", i.e. the pre-phase behaviour on each side.
fn claim_cell() -> Option<&'static AtomicU32> {
    static CELL: OnceLock<Option<&'static AtomicU32>> = OnceLock::new();
    *CELL.get_or_init(|| {
        // SAFETY: CreateFileMappingW with INVALID_HANDLE_VALUE creates (or
        // opens, if another process got there first — ERROR_ALREADY_EXISTS is
        // success with the existing object) a pagefile-backed mapping of 4
        // bytes, zero-initialised by the kernel on first creation. The view is
        // never unmapped: it must live as long as any code might read it, and
        // the process teardown reclaims it. The pointer is 4-byte-aligned
        // (views are page-aligned) and points at read-write memory, so casting
        // to &'static AtomicU32 is sound; cross-process atomicity of aligned
        // u32 loads/stores is architectural on x86_64/aarch64.
        unsafe {
            let mapping = CreateFileMappingW(
                HANDLE(usize::MAX as *mut _), // INVALID_HANDLE_VALUE: pagefile-backed
                None,
                PAGE_READWRITE,
                0,
                4,
                w!("Local\\buttre_tsf_claim"),
            )
            .ok()?;
            let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4);
            if view.Value.is_null() {
                // Mapping handle leaks here; acceptable for a once-per-process
                // failure path, and closing it would not help the null view.
                return None;
            }
            Some(&*(view.Value as *const AtomicU32))
        }
    })
}

/// DLL side: claim (or with 0, release) the foreground for the text service.
///
/// Called from `Activate`/`OnSetFocus` with the host thread's id and from
/// `Deactivate` with 0. Idempotent and racy-by-design: activations happen on
/// profile/app switches, not keystrokes, and the reader tolerates a stale
/// value for the instant between switch and republish.
pub fn publish(thread_id: u32) {
    if let Some(cell) = claim_cell() {
        cell.store(thread_id, Ordering::Release);
    }
}

/// Hook side: must the hook stand down for this foreground thread?
///
/// True iff a live text service has claimed exactly `foreground_thread`. Reads
/// the shared value fresh every call (never cached — module doc) and treats a
/// claim from a dead thread as no claim, so a crashed host app costs at most
/// the keystrokes typed before its window left the foreground.
pub fn is_claimed_by(foreground_thread: u32) -> bool {
    if foreground_thread == 0 {
        return false;
    }
    let Some(cell) = claim_cell() else {
        return false;
    };
    if cell.load(Ordering::Acquire) != foreground_thread {
        return false;
    }
    // Only reached when the claim matches — i.e. we are about to stand down —
    // so this syscall never taxes the hook's composing path.
    thread_is_alive(foreground_thread)
}

/// Is the thread still running? `false` on any failure: an unverifiable claim
/// must not silence the hook (the "no layer" failure is the recoverable one,
/// but an eternally-silenced hook after a host crash is not).
fn thread_is_alive(thread_id: u32) -> bool {
    // SAFETY: OpenThread with a limited-information access right returns a
    // handle we own and close; GetExitCodeThread writes to a valid local.
    unsafe {
        let Ok(handle) = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, thread_id) else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeThread(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(handle);
        alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentThreadId;

    // These run in ONE process, but the mapping is the real named kernel
    // object — the same code path both processes use in production. It is
    // SHARED mutable state, and the test harness runs tests in parallel, so
    // every test must hold this lock or they race each other's publishes.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn no_claim_means_the_hook_runs() {
        let _guard = SERIAL.lock().unwrap();
        publish(0);
        // SAFETY: GetCurrentThreadId has no preconditions.
        let me = unsafe { GetCurrentThreadId() };
        assert!(!is_claimed_by(me));
        assert!(!is_claimed_by(0), "thread 0 is never a valid claim");
    }

    #[test]
    fn a_live_claim_stands_the_hook_down_and_release_lifts_it() {
        let _guard = SERIAL.lock().unwrap();
        // SAFETY: GetCurrentThreadId has no preconditions.
        let me = unsafe { GetCurrentThreadId() };
        publish(me);
        assert!(
            is_claimed_by(me),
            "own (live) thread must satisfy the check"
        );
        assert!(
            !is_claimed_by(me.wrapping_add(1)),
            "a different foreground thread must not"
        );
        publish(0);
        assert!(!is_claimed_by(me), "release must lift the stand-down");
    }

    #[test]
    fn a_dead_threads_claim_is_ignored() {
        let _guard = SERIAL.lock().unwrap();
        // A thread that has already exited: spawn one, join it, use its id.
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            // SAFETY: GetCurrentThreadId has no preconditions.
            let id = unsafe { GetCurrentThreadId() };
            tx.send(id).unwrap();
        });
        let dead_id = rx.recv().unwrap();
        handle.join().unwrap();

        publish(dead_id);
        assert!(
            !is_claimed_by(dead_id),
            "a claim left by a dead thread (crashed host) must not silence the hook"
        );
        publish(0);
    }
}
