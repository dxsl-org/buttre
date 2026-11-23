//! Vietnamese compatibility module
//!
//! This module provides `MethodMetadata` for backward compatibility with UI code.
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

/// Every directory that may hold keyboard TOMLs, highest priority first.
/// All of them count: a keyboard id resolves to the FIRST dir holding
/// `<id>.toml` (so a user copy shadows a shipped one), and the method list
/// is the union. A single winning dir used to hide the rest — on Linux
/// packages the user dir hid the 7 keyboards shipped in
/// `/usr/share/buttre/keyboards`, and vice versa.
///
/// Order: host-supplied resource dir ([`set_resource_dir`]), next to the
/// executable (portable/Windows install), the working dir (dev), the user
/// data dir (`%LOCALAPPDATA%` / `~/.local/share` …`/buttre/keyboards`), and
/// on Linux the package dir `/usr/share/buttre/keyboards`. Only dirs that
/// exist are returned.
pub fn keyboard_dirs() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = RESOURCE_DIR.get() {
        candidates.push(dir.join("keyboards"));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("keyboards")))
    {
        candidates.push(exe_dir);
    }
    candidates.push(std::path::PathBuf::from("keyboards"));
    if let Some(data_dir) = dirs::data_local_dir() {
        candidates.push(data_dir.join("buttre").join("keyboards"));
    }
    #[cfg(target_os = "linux")]
    candidates.push(std::path::PathBuf::from("/usr/share/buttre/keyboards"));

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for dir in candidates {
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Path of keyboard `id` (`<id>.toml`) in the first of `dirs` holding it.
pub fn find_keyboard_in<P: AsRef<std::path::Path>>(
    dirs: &[P],
    id: &str,
) -> Option<std::path::PathBuf> {
    let file = format!("{id}.toml");
    dirs.iter()
        .map(|dir| dir.as_ref().join(&file))
        .find(|path| path.is_file())
}

/// [`find_keyboard_in`] over [`keyboard_dirs`].
pub fn find_keyboard(id: &str) -> Option<std::path::PathBuf> {
    find_keyboard_in(&keyboard_dirs(), id)
}

/// Every `*.toml` across `dirs`, one per file stem — the first dir holding a
/// stem wins, matching [`find_keyboard_in`]. Ordered by dir priority, then
/// by name within a dir (stable menus whatever `read_dir` returns).
pub fn keyboard_files_in<P: AsRef<std::path::Path>>(dirs: &[P]) -> Vec<std::path::PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut in_dir: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        in_dir.sort();
        for path in in_dir {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if seen.insert(stem.to_string()) {
                files.push(path);
            }
        }
    }
    files
}

/// [`keyboard_files_in`] over [`keyboard_dirs`].
pub fn keyboard_files() -> Vec<std::path::PathBuf> {
    keyboard_files_in(&keyboard_dirs())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A user dir and a package dir: the method list is the union, and an id
    /// present in both resolves to the higher-priority (user) copy.
    #[test]
    fn keyboards_union_across_dirs_with_first_dir_shadowing() {
        let root = std::env::temp_dir().join(format!("buttre-kb-dirs-{}", std::process::id()));
        let user = root.join("user");
        let system = root.join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(user.join("cham.toml"), "").unwrap();
        std::fs::write(system.join("cham.toml"), "").unwrap();
        std::fs::write(system.join("thai.toml"), "").unwrap();
        std::fs::write(system.join("notes.txt"), "").unwrap();
        let dirs = [&user, &system];

        assert_eq!(
            keyboard_files_in(&dirs),
            vec![user.join("cham.toml"), system.join("thai.toml")]
        );
        assert_eq!(
            find_keyboard_in(&dirs, "cham"),
            Some(user.join("cham.toml"))
        );
        assert_eq!(
            find_keyboard_in(&dirs, "thai"),
            Some(system.join("thai.toml"))
        );
        assert_eq!(find_keyboard_in(&dirs, "khmer"), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
