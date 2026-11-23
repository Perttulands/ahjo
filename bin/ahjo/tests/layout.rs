//! The workspace layout holds (ARCHITECTURE.md): crates are found by where
//! they sit, so a new area adds no line to the root manifest or CI, and
//! dependencies point one way.

use std::fs;
use std::path::{Path, PathBuf};

use toml::Table;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest(dir: &Path) -> Table {
    let text = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    text.parse().unwrap()
}

/// Each crate directory under `parent`, as (its name there, its path).
fn crates(parent: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .map(|path| (path.file_name().unwrap().to_string_lossy().into(), path))
        .collect();
    found.sort();
    found
}

fn package_name(dir: &Path) -> String {
    manifest(dir)["package"]["name"]
        .as_str()
        .unwrap()
        .to_string()
}

/// One dependency of a crate, as its manifest names it.
struct Dependency {
    /// The table that names it, such as `dependencies` or
    /// `target.'cfg(windows)'.build-dependencies`.
    table: String,
    /// The key it stands under there, which the crate's code uses.
    key: String,
    /// The package behind any rename (`gpui = { package = "gpui-pre" }`).
    package: String,
    /// The workspace path it points at, relative to the root
    /// (`areas/terminals/contract`), when it is a crate of the workspace.
    path: Option<String>,
}

/// A crate's normal and build dependencies, on every target.
fn dependencies(dir: &Path) -> Vec<Dependency> {
    dependencies_in(dir, &["dependencies", "build-dependencies"])
}

/// The dependencies a crate names in the `kinds` tables, at the top level
/// and under every `target`, with workspace inheritance resolved.
fn dependencies_in(dir: &Path, kinds: &[&str]) -> Vec<Dependency> {
    let root = fs::canonicalize(root()).unwrap();
    let workspace = manifest(&root);
    let manifest = manifest(dir);
    let inherited = workspace["workspace"]
        .get("dependencies")
        .and_then(|deps| deps.as_table());
    let mut scopes = vec![(String::new(), &manifest)];
    if let Some(targets) = manifest
        .get("target")
        .and_then(|targets| targets.as_table())
    {
        for (target, table) in targets {
            scopes.push((format!("target.'{target}'."), table.as_table().unwrap()));
        }
    }
    let mut found = Vec::new();
    for (scope, table) in scopes {
        for kind in kinds {
            let Some(deps) = table.get(*kind).and_then(|deps| deps.as_table()) else {
                continue;
            };
            for (alias, spec) in deps {
                // An inherited dependency takes its package and path from
                // the root manifest, where the path is relative to the root.
                let is_inherited = spec.get("workspace").and_then(|w| w.as_bool()) == Some(true);
                let (spec, base) = if is_inherited {
                    let spec = inherited.and_then(|deps| deps.get(alias));
                    let spec = spec.unwrap_or_else(|| {
                        panic!(
                            "{} inherits {alias}, which the workspace lacks",
                            dir.display()
                        )
                    });
                    (spec, root.as_path())
                } else {
                    (spec, dir)
                };
                let package = spec.get("package").and_then(|p| p.as_str());
                // A path outside the workspace, such as another checkout,
                // is no workspace crate: only its package counts.
                let path = spec.get("path").and_then(|p| p.as_str()).and_then(|path| {
                    let target = fs::canonicalize(base.join(path)).unwrap();
                    let relative = target.strip_prefix(&root).ok()?;
                    Some(relative.to_string_lossy().replace('\\', "/"))
                });
                found.push(Dependency {
                    table: format!("{scope}{kind}"),
                    key: alias.clone(),
                    package: package.unwrap_or(alias).to_string(),
                    path,
                });
            }
        }
    }
    found
}

/// The workspace paths a crate's normal and build dependencies point at.
fn uses(dir: &Path) -> Vec<String> {
    dependencies(dir)
        .into_iter()
        .filter_map(|dep| dep.path)
        .collect()
}

#[test]
fn members_are_found_by_where_they_sit() {
    let root = manifest(&root());
    let members: Vec<_> = root["workspace"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member.as_str().unwrap())
        .collect();
    assert_eq!(members, ["kit/*", "engine/*", "areas/*/*", "bin/*"]);
}

/// CI runs app crates in their own job by name (`ahjo-*-app`), so an
/// area's crates are named by their side.
#[test]
fn an_areas_crates_are_its_contract_node_and_app() {
    for (area, dir) in crates_of_areas() {
        for (side, crate_dir) in crates(&dir) {
            let expected = match side.as_str() {
                "contract" => format!("ahjo-{area}-contract"),
                "node" => format!("ahjo-{area}"),
                "app" => format!("ahjo-{area}-app"),
                other => panic!("areas/{area}/{other} is not contract, node or app"),
            };
            assert_eq!(package_name(&crate_dir), expected, "areas/{area}/{side}");
        }
    }
}

fn crates_of_areas() -> Vec<(String, PathBuf)> {
    let mut areas: Vec<_> = fs::read_dir(root().join("areas"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .map(|path| (path.file_name().unwrap().to_string_lossy().into(), path))
        .collect();
    areas.sort();
    areas
}

/// An engine crate that depends on nothing, or the mission model, which
/// depends only on `core`, serde's data formats and their schemas. A contract
/// using it still does no I/O.
fn is_plain_engine_crate(path: &str) -> bool {
    const MISSION_MODEL_USES: [&str; 5] = ["ahjo-core", "schemars", "serde", "serde_json", "toml"];
    let deps = dependencies(&root().join(path));
    match path {
        "engine/mission-model" => deps
            .iter()
            .all(|dep| MISSION_MODEL_USES.contains(&dep.package.as_str())),
        _ => path.starts_with("engine/") && deps.is_empty(),
    }
}

#[test]
fn dependencies_point_one_way() {
    for parent in ["kit", "engine"] {
        for (name, dir) in crates(&root().join(parent)) {
            for used in uses(&dir) {
                assert!(
                    used.starts_with("kit/") || used.starts_with("engine/"),
                    "{parent}/{name} uses {used}: the kit and the engine use no area"
                );
            }
        }
    }
    for (area, dir) in crates_of_areas() {
        for (side, crate_dir) in crates(&dir) {
            for used in uses(&crate_dir) {
                let allowed = match side.as_str() {
                    // Plain data shared below the areas, such as work
                    // references, is an engine crate with no dependencies.
                    "contract" => used == "kit/core" || is_plain_engine_crate(&used),
                    // The GUI toolkit stays out of the node, the web server
                    // out of the app.
                    "node" => used != "kit/app-kit",
                    _ => used != "kit/node-kit",
                } && (used.starts_with("kit/")
                    || used.starts_with("engine/")
                    || used.starts_with("areas/") && used.ends_with("/contract"));
                assert!(
                    allowed,
                    "areas/{area}/{side} uses {used}: an area reaches another \
                     only by its contract or a provided interface"
                );
            }
        }
    }
}

/// The GUI toolkit's packages, vendored forms included.
const GUI_TOOLKIT: [&str; 3] = ["gpui", "gpui-pre", "gpui-pre-platform"];
/// The web server's package.
const WEB_SERVER: &str = "axum";

/// Node crates never build the GUI toolkit and app crates never the web
/// server; a contract builds neither. Their tests may use both.
#[test]
fn the_gui_toolkit_and_the_web_server_stay_on_their_sides() {
    for (area, dir) in crates_of_areas() {
        for (side, crate_dir) in crates(&dir) {
            for dep in dependencies(&crate_dir) {
                let is_gui = GUI_TOOLKIT.contains(&dep.package.as_str());
                let is_web = dep.package == WEB_SERVER;
                let rule = match side.as_str() {
                    "node" if is_gui => "a node crate never uses the GUI toolkit",
                    "app" if is_web => "an app crate never uses the web server",
                    "contract" if is_gui || is_web => {
                        "a contract uses neither the GUI toolkit nor the web server"
                    }
                    _ => continue,
                };
                panic!(
                    "areas/{area}/{side} uses {} as `{}` in [{}]: {rule}",
                    dep.package, dep.key, dep.table
                );
            }
        }
    }
}

/// Standalone apps share the app kit and may use an area's wire types.
#[test]
fn a_standalone_app_uses_the_app_kit_and_contracts() {
    for app in ["bin/ahjo-brain", "bin/ahjo-desk"] {
        let used = uses(&root().join(app));
        assert!(
            used.iter().any(|dep| dep == "kit/app-kit"),
            "{app} uses {used:?}"
        );
        for dep in used {
            assert!(
                dep == "kit/app-kit" || dep.starts_with("areas/") && dep.ends_with("/contract"),
                "{app} uses {dep}"
            );
        }
    }
}

/// An area's tests drive the shell crate, not a binary, so they build no
/// other area's pages.
#[test]
fn an_areas_tests_build_no_binary() {
    for (area, dir) in crates_of_areas() {
        for (side, crate_dir) in crates(&dir) {
            let tests_use = dependencies_in(&crate_dir, &["dev-dependencies"]);
            for used in tests_use.into_iter().filter_map(|dep| dep.path) {
                assert!(
                    !used.starts_with("bin/"),
                    "areas/{area}/{side}'s tests use {used}"
                );
            }
        }
    }
}

/// A hand-rolled form of an interaction the kit owns, as one line of
/// source shows it.
struct HandRolled {
    /// What the line builds by hand.
    what: &'static str,
    finds: fn(&str) -> bool,
    /// The only files (or folders, ending in `/`) where it may stand: the
    /// kit element's own. Empty where it stands nowhere.
    home: &'static [&'static str],
    /// The kit's element to use instead.
    instead: &'static str,
}

/// `call(` with a number written straight into its `px(..)`: `text_size(px(12.))`.
fn literal_px(line: &str, call: &str) -> bool {
    let call = format!("{call}(px(");
    line.match_indices(&call).any(|(at, _)| {
        let rest = &line[at + call.len()..];
        rest.starts_with(|c: char| c.is_ascii_digit() || c == '-')
    })
}

const HAND_ROLLED: &[HandRolled] = &[
    HandRolled {
        what: "a scroll area built by hand",
        finds: |line| {
            [
                "overflow_y_scroll",
                "overflow_x_scroll",
                "overflow_scroll()",
            ]
            .iter()
            .any(|bare| line.contains(bare))
        },
        home: &["kit/app-kit/src/widgets/pane.rs"],
        instead: "pane::scroll, pane::scroll_x or pane::scroll_both",
    },
    HandRolled {
        what: "a virtualized list built by hand",
        finds: |line| line.contains("uniform_list(") || line.contains("uniform_list::<"),
        home: &["kit/app-kit/src/widgets/pane.rs"],
        instead: "pane::long_list",
    },
    HandRolled {
        what: "a scrollbar drawn by hand",
        finds: |line| {
            let lower = line.to_lowercase();
            lower.contains("scrollbar") || lower.contains("scroll_bar")
        },
        home: &["kit/app-kit/src/widgets/pane.rs"],
        instead: "pane::scroll, which draws its own thumb",
    },
    HandRolled {
        what: "a popup placed by hand (anchored)",
        finds: |line| line.contains("anchored("),
        home: &["kit/"],
        instead: "Host::popup (Popup::beside, a menu or popup::form), placed by popup::beside",
    },
    HandRolled {
        what: "something drawn over the page by hand (deferred)",
        finds: |line| line.contains("deferred("),
        home: &["kit/"],
        instead: "Host::popup or Host::layer on the frame's stack, kit::grip for an edge",
    },
    HandRolled {
        what: "a press outside caught by hand (on_mouse_down_out)",
        finds: |line| line.contains("on_mouse_down_out"),
        home: &["kit/"],
        instead: "a popup's Outside::Close or Outside::Keep",
    },
    HandRolled {
        what: "a close control drawn by hand (a × glyph or a Close word)",
        finds: |line| {
            let glyph = [
                "\"×\"",
                "\"\\u{00d7}\"",
                "\"\\u{d7}\"",
                "\"✕\"",
                "\"\\u{2715}\"",
            ]
            .iter()
            .any(|glyph| line.contains(glyph));
            let word =
                line.contains("\"Close\"") && (line.contains("button(") || line.contains("act("));
            glyph || word
        },
        home: &["kit/app-kit/src/widgets/kit.rs"],
        instead: "kit::close, which every surface shows (docs/adr/0002), or kit::cross for a remove",
    },
    HandRolled {
        what: "a type size written as a number",
        finds: |line| literal_px(line, "text_size") || literal_px(line, "line_height"),
        home: &[],
        instead: "theme::size (TEXT, NOTE, SMALL, TINY), theme::ty, or the page's tokens",
    },
    HandRolled {
        what: "a corner written as a number",
        finds: |line| literal_px(line, "rounded"),
        home: &[],
        instead: "theme::size::RADIUS and its kin, rounded_full, or the page's tokens",
    },
    HandRolled {
        what: "a colour written as a number",
        finds: |line| {
            ["rgb(0x", "rgba(0x", "hsla("]
                .iter()
                .any(|literal| line.contains(literal))
        },
        home: &[],
        instead: "the theme's colours, or a colour token in the page's tokens",
    },
];

/// Hand-rolled forms kept for now, by file and what was found: each a
/// debt, with the reason it stands. One that no longer finds anything is
/// refused too, so the list only shrinks.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "areas/files/app/src/view/tabs.rs",
        "a close control drawn by hand (a × glyph or a Close word)",
        "a file tab's × closes a tab, not a surface that opened, so it is \
         not the surfaces' close control (the lead, 2026-11-24, on the \
         ahjo-6yx review); it shows the unsaved dot where the × stands, \
         one control in two states, as an editor's tab does",
    ),
    (
        "areas/terminals/app/src/ui/tiles.rs",
        "something drawn over the page by hand (deferred)",
        "the divider between two tiled windows shares a split along either \
         axis; kit::grip sizes one edge's width across, so the kit has no \
         element for it yet",
    ),
    (
        "areas/terminals/app/src/ui/rail.rs",
        "a press outside caught by hand (on_mouse_down_out)",
        "Peek closes on any press outside it and lets that press act where \
         it lands (ARCHITECTURE.md, The panel); a popup's Outside::Close \
         swallows it",
    ),
    (
        "bin/ahjo-brain/src/editor.rs",
        "a popup placed by hand (anchored)",
        "the pages offered for a `[[` follow the caret and change with each \
         key while the keyboard stays in the text; a popup stands still \
         and takes the keyboard",
    ),
    (
        "bin/ahjo-brain/src/editor.rs",
        "something drawn over the page by hand (deferred)",
        "the same pages offered at the caret, drawn over the text",
    ),
];

/// Every interaction is the kit's element (CODING_STANDARDS.md, Interactions;
/// ARCHITECTURE.md, Kit): its hand-rolled forms stand only in the kit's
/// own files. A scroll area built by hand in a row measured nothing past
/// its height, as Desk's document once did; a popup placed by hand had
/// its own Escape, outside press and focus, each different.
#[test]
fn every_interaction_is_the_kits() {
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    sources(&path, out);
                }
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
    let root = fs::canonicalize(root()).unwrap();
    let mut files = vec![];
    for top in ["kit", "areas", "bin", "engine"] {
        sources(&root.join(top), &mut files);
    }
    let mut found = vec![];
    let mut used = vec![false; ALLOWED.len()];
    for file in files {
        let shown = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // This file names what it looks for.
        if shown == "bin/ahjo/tests/layout.rs" {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            // What a comment says is not code.
            let code = line.split_once("//").map_or(line, |(code, _)| code);
            for rolled in HAND_ROLLED.iter().filter(|rolled| (rolled.finds)(code)) {
                let at_home = rolled.home.iter().any(|home| match home.strip_suffix('/') {
                    Some(dir) => shown.starts_with(&format!("{dir}/")),
                    None => shown == *home,
                });
                if at_home {
                    continue;
                }
                let allowed = ALLOWED
                    .iter()
                    .position(|(path, what, _)| *path == shown && *what == rolled.what);
                match allowed {
                    Some(i) => used[i] = true,
                    None => found.push(format!(
                        "{shown}:{}: {}; use {}",
                        n + 1,
                        rolled.what,
                        rolled.instead
                    )),
                }
            }
        }
    }
    for ((path, what, _), used) in ALLOWED.iter().zip(used) {
        if !used {
            found.push(format!(
                "{path}: allowed {what}, but it is gone: take it off ALLOWED"
            ));
        }
    }
    assert!(
        found.is_empty(),
        "hand-rolled forms of what the kit owns; ARCHITECTURE.md (Kit) lists \
         each kit element and when to use it, and where the kit falls short, \
         extend it in kit/:\n  {}",
        found.join("\n  ")
    );
}
