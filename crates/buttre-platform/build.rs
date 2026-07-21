use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Detect platform and set cfg
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // Tell cargo to recognize our custom cfg attributes
    println!("cargo:rustc-check-cfg=cfg(platform_windows)");
    println!("cargo:rustc-check-cfg=cfg(platform_macos)");
    println!("cargo:rustc-check-cfg=cfg(platform_linux)");

    match target_os.as_str() {
        "windows" => println!("cargo:rustc-cfg=platform_windows"),
        "macos" => println!("cargo:rustc-cfg=platform_macos"),
        "linux" => println!("cargo:rustc-cfg=platform_linux"),
        _ => println!("cargo:warning=Unknown target OS: {}", target_os),
    }

    // Asset staging: copy keyboards + Nôm DB into target dir.
    // Runs on Windows AND Linux so that cargo-deb/cargo-generate-rpm
    // can pick up `target/release/buttre_nom.db` via the metadata.deb/rpm asset lists.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    // The workspace root is two levels up from crates/buttre-platform
    let workspace_root = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();
    let src_keyboards = workspace_root.join("keyboards");

    // Honor CARGO_TARGET_DIR instead of assuming `<workspace>/target/<profile>`:
    // derive the profile output dir from OUT_DIR, which cargo always sets to
    // `<target>/<profile>/build/<pkg>-<hash>/out` under the REAL target dir. A
    // redirected target (common on a VirtualBox shared folder, where build
    // outputs must live on a real filesystem) would otherwise stage assets
    // where cargo-deb / cargo-generate-rpm never look, and packaging fails on a
    // missing buttre_nom.db.
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("OUT_DIR is always <target>/<profile>/build/<pkg>/out")
        .to_path_buf();
    let dest_keyboards = target_dir.join("keyboards");

    println!("cargo:rerun-if-changed={}", src_keyboards.display());

    if src_keyboards.exists() {
        if !dest_keyboards.exists() {
            fs::create_dir_all(&dest_keyboards).unwrap();
        }
        for entry in fs::read_dir(src_keyboards).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                let file_name = path.file_name().unwrap();
                let dest_path = dest_keyboards.join(file_name);
                fs::copy(&path, &dest_path).unwrap();
                println!("Copied {} to {}", path.display(), dest_path.display());
            }
        }
    }

    // Copy buttre_nom.db (if available — optional asset; packaging will fail if required and missing)
    let nom_sources = [
        Path::new(&manifest_dir).join("buttre_nom.db"),
        workspace_root
            .join("crates")
            .join("buttre-core")
            .join("resources")
            .join("nom")
            .join("buttre_nom.db"),
        workspace_root.join("buttre_nom.db"),
    ];

    for src in nom_sources.iter() {
        if src.exists() {
            if !target_dir.exists() {
                fs::create_dir_all(&target_dir).unwrap();
            }
            let dest = target_dir.join("buttre_nom.db");
            match fs::copy(src, &dest) {
                Ok(_) => {
                    println!("Copied Nôm DB from {} to {}", src.display(), dest.display());
                    break;
                }
                Err(e) => println!("cargo:warning=Failed to copy Nom DB: {}", e),
            }
        }
    }

    // Windows-only: icon embedding and DLL DEF file
    if target_os == "windows" {
        let icon_path = Path::new(&manifest_dir).join("icons").join("buttre.ico");
        if icon_path.exists() {
            println!("cargo:rerun-if-changed={}", icon_path.display());
            embed_resource::compile("buttre-platform-icon.rc", embed_resource::NONE);
            println!("Embedded icon: {}", icon_path.display());
        } else {
            println!(
                "cargo:warning=Icon file not found at {}",
                icon_path.display()
            );
        }

        let def_path = Path::new(&manifest_dir).join("buttre_platform.def");
        if def_path.exists() {
            println!("cargo:rerun-if-changed={}", def_path.display());
            println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path.display());
            println!("Using DEF file for DLL exports: {}", def_path.display());
        }
    }
}
