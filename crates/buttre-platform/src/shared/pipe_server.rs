//! Named Pipe Server
//!
//! Handles keystroke processing requests from the buttre TSF DLL.
//!
//! ## Security
//!
//! The pipe is created with an explicit DACL that allows ONLY the pipe-creating user
//! (`OW` = owner) access, and a mandatory-integrity-label SACL that blocks any process
//! at or below Low integrity (`LW` = low-integrity-level mandatory policy: no write).
//! This is important because:
//!
//! - The IPC handlers mutate the shared `Arc<RwLock<Option<Keyboard>>>` that the
//!   keyboard hook reads on every keystroke. Without a DACL any same-desktop process
//!   could observe or corrupt the user's in-progress composition.
//! - Browser renderer sandboxes (Edge, Chrome) and AppContainer processes typically
//!   run at Low integrity. The SACL keeps them out even if they share the user SID.
//!
//! Concurrent pipe instances are capped (`PIPE_INSTANCE_CAP`) and the number of
//! in-flight client-handler threads is capped (`MAX_CONCURRENT_THREADS`) — without
//! these limits a misbehaving local process can exhaust threads or kernel pipe
//! buffers.

use anyhow::Result;
use buttre_core::Keyboard;
#[cfg(windows)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

#[cfg(windows)]
use crate::platforms::windows::tsf::ipc::{
    Action, IpcRequest, IpcResponse, BUFFER_SIZE, PIPE_NAME,
};
#[cfg(windows)]
use anyhow::Context;
#[cfg(windows)]
use buttre_core::Action as EngineAction;
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use tracing::{debug, info, warn};

/// Owner-only DACL with low-integrity-level mandatory write block.
/// See module docs for rationale.
#[cfg(windows)]
const PIPE_SDDL: &str = "D:(A;;GA;;;OW)S:(ML;;NW;;;LW)";

/// Cap on simultaneous pipe instances. Previously `PIPE_UNLIMITED_INSTANCES` —
/// any client could open thousands and exhaust kernel resources.
#[cfg(windows)]
const PIPE_INSTANCE_CAP: u32 = 4;

/// Cap on simultaneous client-handler threads. Limits the blast radius of a
/// misbehaving client that opens many connections.
#[cfg(windows)]
const MAX_CONCURRENT_THREADS: usize = 4;

/// Tracks live handler threads so spawning logic can refuse new connections
/// when at capacity. Decremented by `ThreadCountGuard` on thread exit.
#[cfg(windows)]
static THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Drop guard that decrements `THREAD_COUNT` when a handler thread exits
/// (including via panic unwind).
#[cfg(windows)]
struct ThreadCountGuard;
#[cfg(windows)]
impl Drop for ThreadCountGuard {
    fn drop(&mut self) {
        THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Run the Named Pipe server
pub fn run_pipe_server(keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
    #[cfg(windows)]
    {
        info!("Starting Named Pipe server: {}", PIPE_NAME);
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{FromRawHandle, RawHandle};
        use windows::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
        };

        let pipe_name_wide: Vec<u16> = std::ffi::OsStr::new(PIPE_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Build the SECURITY_ATTRIBUTES once. The backing security-descriptor
        // pointer must outlive every CreateNamedPipeW call — we leak it for the
        // process lifetime, which is what we want here (the server runs forever).
        let security_attrs = build_pipe_security_attrs();
        let sa_ptr: Option<*const _> = security_attrs.as_ref().map(|sa| sa as *const _);
        if security_attrs.is_none() {
            warn!("Failed to build pipe SECURITY_ATTRIBUTES — falling back to default DACL");
        }

        loop {
            // Refuse new pipe instances once the handler-thread cap is hit.
            // Otherwise we'd happily CreateNamedPipeW and then drop the connection.
            if THREAD_COUNT.load(Ordering::SeqCst) >= MAX_CONCURRENT_THREADS {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }

            // SAFETY:
            // 1. pipe_name_wide is valid UTF-16 null-terminated string from OsStr::encode_wide
            // 2. PCWSTR(pipe_name_wide.as_ptr()) creates valid pointer to the string
            // 3. PIPE_ACCESS_DUPLEX and other flags are valid constants from Windows SDK
            // 4. BUFFER_SIZE is positive and reasonable (defined in ipc module)
            // 5. CreateNamedPipeW is properly declared in windows crate
            // 6. sa_ptr (if Some) points to a SECURITY_ATTRIBUTES we built and held above
            // 7. Returns INVALID_HANDLE_VALUE on error, which we check
            let h_pipe = unsafe {
                CreateNamedPipeW(
                    windows::core::PCWSTR(pipe_name_wide.as_ptr()),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                    PIPE_INSTANCE_CAP,
                    BUFFER_SIZE as u32,
                    BUFFER_SIZE as u32,
                    0,
                    sa_ptr,
                )
            };

            if h_pipe == INVALID_HANDLE_VALUE {
                // SAFETY: GetLastError is properly declared and safe to call
                let err = unsafe { GetLastError() };
                warn!("Failed to create named pipe: 0x{:X}", err.0);
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            debug!("Waiting for client connection...");
            // SAFETY:
            // 1. h_pipe is a valid HANDLE from CreateNamedPipeW (checked non-INVALID above)
            // 2. ConnectNamedPipe is properly declared in windows crate
            // 3. None for overlapped means synchronous operation
            let connected = unsafe { ConnectNamedPipe(h_pipe, None).is_ok() };

            // SAFETY: GetLastError is safe to call
            if connected || unsafe { GetLastError().0 } == 535 {
                // ERROR_PIPE_CONNECTED
                debug!("Client connected");

                // Re-check the thread cap; another connection may have come in
                // since the top-of-loop check.
                if THREAD_COUNT.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_THREADS {
                    THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
                    warn!("Refusing pipe connection — thread cap reached");
                    // SAFETY: h_pipe is valid; we own it and are not using it after this point
                    unsafe {
                        let _ = CloseHandle(h_pipe);
                    };
                    continue;
                }

                let keyboard_clone = keyboard.clone();
                // Create File before moving to thread (File is Send, HANDLE is not)
                // SAFETY:
                // 1. h_pipe.0 is a valid Windows HANDLE from CreateNamedPipeW
                // 2. from_raw_handle takes ownership of the handle
                // 3. File will close the handle when dropped (no double-free)
                // 4. Handle is not used after this point (ownership transferred)
                let mut file = unsafe { std::fs::File::from_raw_handle(h_pipe.0 as RawHandle) };

                std::thread::spawn(move || {
                    let _guard = ThreadCountGuard;
                    if let Err(e) = handle_client(&mut file, keyboard_clone) {
                        warn!("Error handling client: {:?}", e);
                    }
                });
            } else {
                // SAFETY:
                // 1. h_pipe is a valid HANDLE that we own
                // 2. CloseHandle is properly declared in windows crate
                // 3. We don't use h_pipe after this point
                unsafe {
                    let _ = CloseHandle(h_pipe);
                };
            }
        }
    }

    #[cfg(not(windows))]
    {
        let _ = keyboard;
        anyhow::bail!("Pipe server is only supported on Windows");
    }
}

/// Build the SECURITY_ATTRIBUTES used for the named pipe.
///
/// Returns `None` if Windows can't parse our SDDL string — in that case the
/// caller falls back to the default DACL (still scoped to the current user,
/// but no integrity-level filter and inherits whatever default Windows applies).
#[cfg(windows)]
fn build_pipe_security_attrs() -> Option<windows::Win32::Security::SECURITY_ATTRIBUTES> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    let sddl_wide: Vec<u16> = std::ffi::OsStr::new(PIPE_SDDL)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut psd = PSECURITY_DESCRIPTOR::default();
    // SAFETY:
    // 1. sddl_wide is a valid null-terminated UTF-16 string.
    // 2. PCWSTR(sddl_wide.as_ptr()) is valid for the duration of this call (vec lives here).
    // 3. SDDL_REVISION_1 = 1.
    // 4. psd is a fresh out-parameter we just allocated on the stack.
    // 5. The function allocates the descriptor with LocalAlloc on success; we intentionally
    //    leak it for the lifetime of the process (the server never shuts down).
    let result = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            1, // SDDL_REVISION_1
            &mut psd,
            None,
        )
    };

    if result.is_err() {
        return None;
    }

    Some(SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: psd.0,
        bInheritHandle: BOOL(0),
    })
}

/// Handle client connection
#[cfg(windows)]
fn handle_client(pipe: &mut std::fs::File, keyboard: Arc<RwLock<Option<Keyboard>>>) -> Result<()> {
    let mut buffer = vec![0u8; BUFFER_SIZE];
    // Disconnect any client that sends more than 3 unparseable messages in a row —
    // prevents a misbehaving (or malicious) client from holding a thread forever.
    let mut bad_frames: u32 = 0;
    const MAX_BAD_FRAMES: u32 = 3;

    loop {
        let bytes_read = match pipe.read(&mut buffer) {
            Ok(0) => {
                debug!("Client disconnected (0 bytes)");
                break;
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                debug!("Client disconnected (broken pipe)");
                break;
            }
            Err(e) => return Err(e).context("Read failed"),
        };

        let request: IpcRequest = match bincode::deserialize(&buffer[..bytes_read]) {
            Ok(req) => {
                bad_frames = 0;
                req
            }
            Err(e) => {
                bad_frames += 1;
                warn!(
                    "Failed to deserialize request ({}/{}): {}",
                    bad_frames, MAX_BAD_FRAMES, e
                );
                if bad_frames >= MAX_BAD_FRAMES {
                    warn!("Disconnecting client after {} bad frames", bad_frames);
                    break;
                }
                continue;
            }
        };

        let response = process_request(request, keyboard.clone());

        let response_bytes =
            bincode::serialize(&response).context("Failed to serialize response")?;
        pipe.write_all(&response_bytes).context("Write failed")?;
        pipe.flush().context("Flush failed")?;
    }

    Ok(())
}

/// Process IPC request
#[cfg(windows)]
fn process_request(request: IpcRequest, keyboard: Arc<RwLock<Option<Keyboard>>>) -> IpcResponse {
    // Poison-tolerant lock acquisition: if a previous handler panicked while
    // holding the write lock, recover the data rather than re-panicking here
    // (which would just kill another handler thread).
    let mut kb_opt = match keyboard.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("Keyboard RwLock was poisoned — recovering");
            poisoned.into_inner()
        }
    };

    if let Some(ref mut kb) = *kb_opt {
        match request {
            IpcRequest::ProcessKey(ch) => {
                match kb.process(ch) {
                    Ok(actions) => {
                        // Take first action (main typing action)
                        if let Some(action) = actions.into_iter().next() {
                            match action {
                                EngineAction::Replace {
                                    backspace_count,
                                    text,
                                } => IpcResponse::Action(Action::Replace {
                                    delete: backspace_count,
                                    insert: text,
                                }),
                                EngineAction::DoNothing => IpcResponse::Action(Action::DoNothing),
                                EngineAction::Commit(text) => {
                                    IpcResponse::Action(Action::Replace {
                                        delete: 0,
                                        insert: text,
                                    })
                                }
                                EngineAction::UpdateComposition { .. }
                                | EngineAction::ConfirmComposition(_) => {
                                    // TODO: Update IPC protocol to support Composition actions
                                    warn!("Ignoring Composition action in IPC server (not yet supported)");
                                    IpcResponse::Action(Action::DoNothing)
                                }
                                EngineAction::ShowCandidates { .. }
                                | EngineAction::HideCandidates => {
                                    // Ignore candidate actions in pipe server
                                    IpcResponse::Action(Action::DoNothing)
                                }
                            }
                        } else {
                            IpcResponse::Action(Action::DoNothing)
                        }
                    }
                    Err(e) => {
                        warn!("Keyboard process error: {}", e);
                        IpcResponse::Action(Action::DoNothing)
                    }
                }
            }
            IpcRequest::ProcessBackspace => {
                match kb.backspace() {
                    Ok(EngineAction::Replace {
                        backspace_count,
                        text,
                    }) => IpcResponse::Action(Action::Replace {
                        delete: backspace_count,
                        insert: text,
                    }),
                    Ok(EngineAction::DoNothing) => IpcResponse::Action(Action::DoNothing),
                    Ok(EngineAction::Commit(text)) => IpcResponse::Action(Action::Replace {
                        delete: 0,
                        insert: text,
                    }),
                    Ok(EngineAction::UpdateComposition { .. })
                    | Ok(EngineAction::ConfirmComposition(_)) => {
                        // TODO: Update IPC protocol to support Composition actions
                        warn!("Ignoring Composition action in IPC server (not yet supported)");
                        IpcResponse::Action(Action::DoNothing)
                    }
                    Ok(EngineAction::ShowCandidates { .. }) | Ok(EngineAction::HideCandidates) => {
                        // Ignore candidate actions
                        IpcResponse::Action(Action::DoNothing)
                    }
                    Err(e) => {
                        warn!("Keyboard backspace error: {}", e);
                        IpcResponse::Action(Action::DoNothing)
                    }
                }
            }
            IpcRequest::Reset => {
                kb.reset();
                IpcResponse::Action(Action::DoNothing)
            }
            IpcRequest::Ping => IpcResponse::Pong,
        }
    } else {
        // No keyboard loaded (English mode)
        IpcResponse::Action(Action::DoNothing)
    }
}
