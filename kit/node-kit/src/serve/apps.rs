//! The apps this node hands out, so another machine on the tailnet installs
//! and updates them from here (ARCHITECTURE.md, Install and upgrade): the
//! builds of Ahjo Swarm, Ahjo Brain and Ahjo Desk made from this node's own
//! commit, kept in `<apps>/<platform>/<version>/` beside the manifest
//! `ahjo upgrade` wrote, and the install script.
//!
//! - `GET /apps/<platform>`: the manifest, `{version, files: [{name, size,
//!   sha256}]}`, or `no_apps` (404) when no build for this version is kept.
//! - `GET /apps/<platform>/<file>`: a file the manifest names.
//! - `GET /install.ps1`: the Windows install script, pointed at this node
//!   as the request reached it, and at `[node] desk`.
//!
//! `<apps>` is `[node] apps`, or `apps` beside the folder holding this
//! binary (`~/.local/share/ahjo/apps` for `~/.local/share/ahjo/bin/ahjo`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ahjo_core::Error;
use axum::Router;
use axum::extract::{Path as Route, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};

use super::Failure;

/// What `<apps>/<platform>/<version>/manifest.json` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<File>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
}

/// The manifest's name in a build's folder.
pub const MANIFEST: &str = "manifest.json";

/// The platforms a node keeps apps for.
pub const PLATFORMS: &[&str] = &["windows", "linux"];

/// Where `version`'s apps for `platform` are kept under `apps`.
pub fn folder(apps: &Path, platform: &str, version: &str) -> PathBuf {
    apps.join(platform).join(version)
}

/// `apps` beside the folder holding `binary`: `<root>/apps` for
/// `<root>/bin/ahjo`.
pub fn beside(binary: &Path) -> Option<PathBuf> {
    Some(binary.parent()?.parent()?.join("apps"))
}

struct Apps {
    dir: Option<PathBuf>,
    desk: Option<String>,
}

pub(super) fn routes(dir: Option<PathBuf>, desk: Option<String>) -> Router {
    let apps = Arc::new(Apps { dir, desk });
    Router::new()
        .route("/apps/{platform}", get(manifest))
        .route("/apps/{platform}/{file}", get(file))
        .route("/install.ps1", get(script))
        .with_state(apps)
}

/// Why no apps are handed out: `why`, else that none were put in place.
fn missing(platform: &str, why: Option<String>) -> Failure {
    let why = why.map(|w| format!(" ({w})")).unwrap_or_default();
    Failure(
        StatusCode::NOT_FOUND,
        Error::new(
            "no_apps",
            format!(
                "the Ahjo node here (build {}) keeps no {platform} apps of its build{why}; \
                 on it, ahjo upgrade --windows-apps DIR puts them in place",
                crate::VERSION
            ),
        ),
    )
}

/// The manifest of this version's apps for `platform`, and their folder.
fn read(apps: &Apps, platform: &str) -> Result<(Manifest, PathBuf), Failure> {
    if !PLATFORMS.contains(&platform) {
        return Err(missing(
            platform,
            Some(format!("the platforms are {}", PLATFORMS.join(", "))),
        ));
    }
    let Some(dir) = &apps.dir else {
        return Err(missing(platform, Some("it has no apps folder".into())));
    };
    let here = folder(dir, platform, crate::VERSION);
    let path = here.join(MANIFEST);
    let text = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => missing(platform, None),
        _ => missing(platform, Some(format!("{}: {e}", path.display()))),
    })?;
    let manifest: Manifest = serde_json::from_slice(&text).map_err(|e| {
        missing(
            platform,
            Some(format!("{} does not read: {e}", path.display())),
        )
    })?;
    if manifest.version != crate::VERSION {
        return Err(missing(
            platform,
            Some(format!("{} is for {}", path.display(), manifest.version)),
        ));
    }
    Ok((manifest, here))
}

async fn manifest(State(apps): State<Arc<Apps>>, Route(platform): Route<String>) -> Response {
    match read(&apps, &platform) {
        Ok((manifest, _)) => axum::Json(manifest).into_response(),
        Err(failure) => failure.into_response(),
    }
}

async fn file(
    State(apps): State<Arc<Apps>>,
    Route((platform, name)): Route<(String, String)>,
) -> Response {
    let (manifest, dir) = match read(&apps, &platform) {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    // Only what the manifest names: no other path is reachable.
    if !manifest.files.iter().any(|f| f.name == name) {
        return Failure(
            StatusCode::NOT_FOUND,
            Error::new(
                "no_file",
                format!("{name} is not among this node's {platform} apps"),
            ),
        )
        .into_response();
    }
    let path = dir.join(&name);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{name}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => Failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            Error::new("io", format!("reading {}: {e}", path.display())),
        )
        .into_response(),
    }
}

/// The script, for whoever asked: the node as the request named it (its
/// `Host`, which the trust check has admitted), so a laptop gets the
/// tailnet name it used.
async fn script(State(apps): State<Arc<Apps>>, headers: HeaderMap) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1");
    let text = install_script(&format!("http://{host}"), apps.desk.as_deref());
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

const SCRIPT: &str = include_str!("install.ps1");

/// The install script pointed at `node` and `desk`. Each is written into a
/// single-quoted PowerShell string, where only `'` needs doubling.
pub fn install_script(node: &str, desk: Option<&str>) -> String {
    let quoted = |s: &str| s.replace('\'', "''");
    SCRIPT
        .replace("{{NODE}}", &quoted(node.trim_end_matches('/')))
        .replace(
            "{{DESK}}",
            &quoted(desk.unwrap_or_default().trim_end_matches('/')),
        )
        .replace("{{VERSION}}", crate::VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_names_the_node_and_desk_it_was_asked_for() {
        let text = install_script("http://rig.tail1.ts.net:7300/", Some("http://rig:18782"));
        assert!(text.contains("$node = 'http://rig.tail1.ts.net:7300'"));
        assert!(text.contains("$desk = 'http://rig:18782'"));
        assert!(!text.contains("{{"));
        let odd = install_script("http://it's", None);
        assert!(odd.contains("$node = 'http://it''s'"));
        assert!(odd.contains("$desk = ''"));
    }

    #[test]
    fn apps_live_beside_the_binarys_folder() {
        assert_eq!(
            beside(Path::new("/home/u/.local/share/ahjo/bin/ahjo")),
            Some(PathBuf::from("/home/u/.local/share/ahjo/apps"))
        );
    }
}
