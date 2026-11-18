//! Windows: the exe carries an icon and file properties as a resource (gpui
//! loads icon 1 for the window and taskbar).
//!
//! Linux: the X11 platform links `xcb`, `xkbcommon` and `xkbcommon-x11` by
//! their unversioned names, which only the `-dev` packages install; hosts
//! with the runtime libraries alone get links to the versioned files in
//! OUT_DIR, as `bin/ahjo-swarm` does.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => embed_windows_resource(),
        Ok("linux") => link_x11_libraries(),
        _ => {}
    }
}

fn embed_windows_resource() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let icon = std::path::Path::new(&manifest_dir).join("assets/icon/ahjo-desk.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = |part: &str| std::env::var(format!("CARGO_PKG_VERSION_{part}")).expect(part);
    let (major, minor, patch) = (version("MAJOR"), version("MINOR"), version("PATCH"));
    let text = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Ahjo Desk"
      VALUE "FileVersion", "{text}"
      VALUE "InternalName", "ahjo-desk"
      VALUE "OriginalFilename", "ahjo-desk.exe"
      VALUE "ProductName", "Ahjo Desk"
      VALUE "ProductVersion", "{text}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let script = out.join("ahjo-desk.rc");
    std::fs::write(&script, rc).expect("write ahjo-desk.rc");
    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|e| panic!("embedding the icon and file properties failed: {e}"));
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
