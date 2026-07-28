//! Vietnamese compatibility module
//!
//! This module provides ConfigLoader for backward compatibility with UI code.
//! Real Vietnamese implementation is in buttre-keyboard.

// TODO: Remove this module when buttre-core/src/keyboard is ready.

pub mod config_loader {
    use serde::{Deserialize, Serialize};

    /// Metadata for an input method
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MethodMetadata {
        pub id: String,
        pub name: String,
        pub description: String,
        pub version: String,
        pub author: String,
        pub icon: Option<String>,
        pub is_builtin: bool,
    }

    impl MethodMetadata {
        pub fn new(id: &str, name: &str) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                description: format!("Built-in {} input method", name),
                version: "1.0.0".to_string(),
                author: "buttre".to_string(),
                icon: None,
                is_builtin: true,
            }
        }
    }

    pub struct ConfigLoader;

    impl ConfigLoader {
        pub fn list_methods_with_metadata() -> anyhow::Result<Vec<MethodMetadata>> {
            // Return built-in methods
            Ok(vec![
                MethodMetadata::new("telex", "Telex"),
                MethodMetadata::new("vni", "VNI"),
            ])
        }
    }
}

/// Directory holding buttre's shipped data files, when the caller had to say
/// so explicitly. See [`set_resource_dir`].
static RESOURCE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Pin the directory that ships buttre's data files (`buttre_nom.db`,
/// `keyboards/*.toml`), overriding the `current_exe()` search below.
///
/// Required by any host where buttre is NOT the executable. The Windows TSF
/// text service is a DLL loaded into Word, Notepad, Chrome — there
/// `current_exe()` is the HOST application, so the search looked for
/// `buttre_nom.db` inside the Office install directory and of course never
/// found it. The symptom was silent: Nôm loaded with no dictionary and typed
/// plain Quốc ngữ, and custom keyboards vanished, with nothing but a `warn!`
/// in a log nobody was reading.
///
/// First call wins; later calls are ignored, so a host can call it
/// unconditionally on every activation.
pub fn set_resource_dir(dir: std::path::PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

/// Get custom keyboards directory
pub fn get_custom_dir() -> std::path::PathBuf {
    // 0. Host-supplied resource directory (see `set_resource_dir`)
    if let Some(dir) = RESOURCE_DIR.get() {
        let keyboards = dir.join("keyboards");
        if keyboards.exists() {
            return keyboards;
        }
    }

    // 1. Check folder "keyboards" next to executable (Priority 1 - Release/Portable)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let local_keyboards = exe_dir.join("keyboards");
            if local_keyboards.exists() {
                return local_keyboards;
            }
        }
    }

    // 2. Check current working directory (Priority 2 - Dev)
    let cwd_keyboards = std::path::PathBuf::from("keyboards");
    if cwd_keyboards.exists() {
        return cwd_keyboards;
    }

    // 3. Fallback to AppData/Local/buttre/keyboards (Priority 3 - Installed)
    if let Some(data_dir) = dirs::data_local_dir() {
        data_dir.join("buttre").join("keyboards")
    } else {
        std::path::PathBuf::from("keyboards")
    }
}

/// Get Nôm dictionary database path
pub fn get_nom_db_path() -> Option<std::path::PathBuf> {
    let filename = "buttre_nom.db";

    tracing::info!("Searching for Nôm dictionary: {}", filename);

    // 0. Host-supplied resource directory (see `set_resource_dir`) — the only
    //    location that works when buttre is a DLL inside someone else's process.
    if let Some(dir) = RESOURCE_DIR.get() {
        for path in [
            dir.join(filename),
            dir.join("resources").join("nom").join(filename),
        ] {
            tracing::debug!("Checking resource dir: {:?}", path);
            if path.exists() {
                tracing::info!("✓ Found Nôm dictionary at: {:?}", path);
                return Some(path);
            }
        }
    }

    // 1. Check next to executable
    if let Ok(exe_path) = std::env::current_exe() {
        tracing::debug!("Executable path: {:?}", exe_path);
        if let Some(exe_dir) = exe_path.parent() {
            let path = exe_dir.join(filename);
            tracing::debug!("Checking: {:?}", path);
            if path.exists() {
                tracing::info!("✓ Found Nôm dictionary at: {:?}", path);
                return Some(path);
            }

            // Check in resources/nom
            let res_path = exe_dir.join("resources").join("nom").join(filename);
            tracing::debug!("Checking: {:?}", res_path);
            if res_path.exists() {
                tracing::info!("✓ Found Nôm dictionary at: {:?}", res_path);
                return Some(res_path);
            }
        }
    }

    // 2. Check current working directory
    let cwd_path = std::path::PathBuf::from(filename);
    tracing::debug!(
        "Checking CWD: {:?}",
        cwd_path.canonicalize().unwrap_or(cwd_path.clone())
    );
    if cwd_path.exists() {
        tracing::info!(
            "✓ Found Nôm dictionary at: {:?}",
            cwd_path.canonicalize().unwrap_or(cwd_path.clone())
        );
        return Some(cwd_path);
    }

    // 3. Check AppData (Windows) / XDG_DATA_HOME (Linux/macOS user install)
    if let Some(data_dir) = dirs::data_local_dir() {
        let path = data_dir.join("buttre").join(filename);
        tracing::debug!("Checking user data dir: {:?}", path);
        if path.exists() {
            tracing::info!("✓ Found Nôm dictionary at: {:?}", path);
            return Some(path);
        }
    }

    // 4. Linux system install (deb/rpm asset destination)
    #[cfg(target_os = "linux")]
    {
        let system_path = std::path::PathBuf::from("/usr/share/buttre").join(filename);
        tracing::debug!("Checking system path: {:?}", system_path);
        if system_path.exists() {
            tracing::info!("✓ Found Nôm dictionary at: {:?}", system_path);
            return Some(system_path);
        }
    }

    tracing::warn!("✗ Nôm dictionary not found in any location!");
    None
}
