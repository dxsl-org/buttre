use buttre_platform::shared::input::{MethodRegistry, MethodSource};

#[test]
fn test_registry_creation() {
    let registry = MethodRegistry::new();

    // Should have at least 3 built-in methods
    assert!(registry.get_all().len() >= 3);

    // Check built-in methods
    assert!(registry.get("telex").is_some());
    assert!(registry.get("vni").is_some());
    assert!(registry.get("nom").is_some());
}

#[test]
fn test_get_builtin() {
    let registry = MethodRegistry::new();
    let builtin = registry.get_builtin();

    assert_eq!(builtin.len(), 3);
    assert!(builtin.iter().all(|m| m.source == MethodSource::BuiltIn));
}

/// Every keyboard TOML the repo ships must load, and its `metadata.id` must
/// equal its lowercase filename stem. Both custom-method surfaces depend on
/// that convention agreeing: the tray switches by `metadata.id` (registry)
/// while the IBus panel and the engine load by FILENAME STEM
/// (`keyboards/{id}.toml`), so a mismatched fixture would render in both
/// menus yet silently fail to round-trip through `method_sync`.
#[test]
fn repo_keyboards_load_and_ids_match_filename_stems() {
    let keyboards_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../keyboards")
        .canonicalize()
        .expect("repo keyboards/ dir must exist");

    let mut checked = 0;
    for entry in std::fs::read_dir(&keyboards_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let config = buttre_core::Config::load(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("{path:?} must load as a keyboard config: {e}"));
        assert_eq!(
            config.metadata.id, stem,
            "{path:?}: metadata.id must equal the filename stem"
        );
        assert_eq!(
            stem.to_lowercase(),
            stem,
            "{path:?}: filename stem must be lowercase (method_sync reads lowercase)"
        );
        checked += 1;
    }
    assert!(checked > 0, "repo keyboards/ must contain at least one TOML");
}
