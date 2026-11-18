//! Windows: gpui loads the executable's icon 1 for the window and taskbar.
//!
//! Linux: the X11 platform links `xcb`, `xkbcommon` and `xkbcommon-x11` by
//! their unversioned names, which only the `-dev` packages install. Hosts that
//! have the runtime libraries alone (WSL with WSLg) get links to the versioned
//! files in OUT_DIR, so the app builds without root. (The same step as
//! `bin/ahjo-swarm/build.rs`; a build script cannot share code with another.)

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => embed_windows_icon(),
        Ok("linux") => link_x11_libraries(),
        _ => {}
    }
}

fn embed_windows_icon() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let icon = std::path::Path::new(&manifest_dir).join("assets/icon/ahjo-brain.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let path = icon.display().to_string().replace('\\', "\\\\");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let script = out.join("ahjo-brain.rc");
    std::fs::write(
        &script,
        format!("#pragma code_page(65001)\n1 ICON \"{path}\"\n"),
    )
    .expect("write ahjo-brain.rc");
    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|error| panic!("embedding the Ahjo Brain icon failed: {error}"));
}

fn link_x11_libraries() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("linklibs");
    std::fs::create_dir_all(&out).expect("create linklibs");
    let dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
    ];
    for lib in ["xcb", "xkbcommon", "xkbcommon-x11"] {
        if dirs.iter().any(|d| {
            std::path::Path::new(d)
                .join(format!("lib{lib}.so"))
                .exists()
        }) {
            continue;
        }
        let versioned = format!("lib{lib}.so.");
        let found = dirs.iter().find_map(|d| {
            std::fs::read_dir(d)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.strip_prefix(&versioned)
                            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit()))
                    })
                })
        });
        if let Some(path) = found {
            let link = out.join(format!("lib{lib}.so"));
            let _ = std::fs::remove_file(&link);
            #[cfg(unix)]
            std::os::unix::fs::symlink(path, link).expect("link library");
            #[cfg(not(unix))]
            panic!(
                "linking {} for Linux needs a Unix build host",
                path.display()
            );
        }
    }
    println!("cargo:rustc-link-search=native={}", out.display());
}
