//! Input Method Registry
//!
//! **Tests**: Integration tests for this module are located in `crates/buttre-platform/tests/shared_method_registry_tests.rs`.
//!
//! Manages all available input methods (built-in and custom)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Input method information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodInfo {
    /// Unique identifier (e.g., "telex", "vni", "my_custom")
    pub id: String,

    /// Display name (e.g., "Telex", "VNI", "My Custom Method")
    pub name: String,

    /// Method source (built-in or custom)
    pub source: MethodSource,

    /// Config file path (None for built-in methods)
    pub config_path: Option<PathBuf>,

    /// Description
    pub description: Option<String>,
}

/// Method source type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodSource {
    /// Built-in method (embedded in binary)
    BuiltIn,

    /// Custom method (loaded from file)
    Custom,
}

/// Method Registry - manages all available input methods
pub struct MethodRegistry {
    methods: Vec<MethodInfo>,
}

impl MethodRegistry {
    /// Create a new registry with built-in methods
    pub fn new() -> Self {
        let mut registry = Self {
            methods: Vec::new(),
        };

        // Register built-in methods
        registry.register_builtin("telex", "Telex", "Vietnamese Telex input method");
        registry.register_builtin("vni", "VNI", "Vietnamese Number Input method");
        registry.register_builtin("nom", "Nôm", "Chữ Nôm input method (experimental)");

        // Scan and register custom methods
        if let Err(e) = registry.scan_custom_methods() {
            tracing::warn!("Failed to scan custom methods: {}", e);
        }

        registry
    }

    /// Register a built-in method
    fn register_builtin(&mut self, id: &str, name: &str, description: &str) {
        self.methods.push(MethodInfo {
            id: id.to_string(),
            name: name.to_string(),
            source: MethodSource::BuiltIn,
            config_path: None,
            description: Some(description.to_string()),
        });
    }

    /// Register every keyboard TOML across the keyboard dirs (user copies
    /// shadow shipped ones — `keyboard_files`).
    fn scan_custom_methods(&mut self) -> anyhow::Result<()> {
        for path in buttre_core::vietnamese::keyboard_files() {
            // Skip built-in methods (they're already registered)
            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(file_stem, "telex" | "vni" | "nom") {
                continue;
            }

            // Try to load config to get metadata
            match self.load_custom_method_info(&path) {
                Ok(info) => {
                    tracing::info!("Registered custom method: {} from {:?}", info.name, path);
                    self.methods.push(info);
                }
                Err(e) => {
                    tracing::warn!("Failed to load custom method from {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    /// Load custom method info from config file
    fn load_custom_method_info(&self, path: &Path) -> anyhow::Result<MethodInfo> {
        // Load config to get metadata
        let config = buttre_core::Config::load(path.to_str().unwrap())?;

        Ok(MethodInfo {
            id: config.metadata.id.clone(),
            name: config.metadata.name.clone(),
            source: MethodSource::Custom,
            config_path: Some(path.to_path_buf()),
            description: Some(config.metadata.description.clone()),
        })
    }

    /// Get all registered methods
    pub fn get_all(&self) -> &[MethodInfo] {
        &self.methods
    }

    /// Get method by ID
    pub fn get(&self, id: &str) -> Option<&MethodInfo> {
        self.methods.iter().find(|m| m.id == id)
    }

    /// Get all built-in methods
    pub fn get_builtin(&self) -> Vec<&MethodInfo> {
        self.methods
            .iter()
            .filter(|m| m.source == MethodSource::BuiltIn)
            .collect()
    }

    /// Get all custom methods
    pub fn get_custom(&self) -> Vec<&MethodInfo> {
        self.methods
            .iter()
            .filter(|m| m.source == MethodSource::Custom)
            .collect()
    }

    /// Rescan custom methods (for hot reload)
    pub fn rescan(&mut self) -> anyhow::Result<()> {
        // Remove all custom methods
        self.methods.retain(|m| m.source == MethodSource::BuiltIn);

        // Rescan
        self.scan_custom_methods()
    }
}

impl Default for MethodRegistry {
    fn default() -> Self {
        Self::new()
    }
}
