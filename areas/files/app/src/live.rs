//! A web page on a node shown running (ADR 0006): the app's own scheme
//! serves the folder the page was opened in, and nothing else, each request
//! read from the node through the app's link.
//!
//! Each folder is a site of its own (an origin), named by a hash of its node
//! and path, so a page keeps its storage and reaches no other folder's. The
//! node confines each read to the folder again, links followed
//! ([`FOLDER_FILE_PATH`]).
//!
//! A sound or video file plays in a page of the app's own, the folder's
//! root ([`PLAYER`]), which names the file in its query and tells the app
//! when the web view cannot play it ([`CannotShow`]). A 3D model turns in
//! another ([`model_page`]), with three.js, which the app serves itself
//! from a site no folder has ([`APP_SITE`]), so a model shows with no
//! network.
//!
//! The system web view takes each answer whole. A file asked for whole (a
//! script, a stylesheet, an image) comes whole, however long; a range, as a
//! player asks for, holds at most [`MOST`] bytes from where it was asked,
//! said so, and the player asks for the rest as it plays.

use ahjo_app_kit::host::{Reach, Request};
use ahjo_app_kit::web_view::{Respond, Scheme, SchemeRequest, SchemeResponse, scheme_url};
use ahjo_core::NodeId;
use ahjo_files_contract::FOLDER_FILE_PATH;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

/// The scheme's name.
pub const SCHEME: &str = "ahjo-file";

/// The most bytes of a file one answer to a range holds.
pub const MOST: u64 = 8 << 20;

/// The site the app's own scripts are served from: never a folder's,
/// whose sites are hashes ([`site`]).
pub const APP_SITE: &str = "app";

/// three.js and the parts of it the model viewer uses, bundled by
/// `web/bundle-three` (three.js is MIT-licensed; its notice is in the file).
pub const THREE: &[u8] = include_bytes!("../web/three.js");

/// What the headers of a node's answer may tell the page.
const PASSED: &[&str] = &["content-type", "content-range", "accept-ranges"];

/// The folder a page serves from, now.
#[derive(Clone)]
struct Served {
    node: String,
    folder: String,
    site: String,
    reach: Reach,
}

/// The folder one view serves pages from. Clones share it.
#[derive(Clone, Default)]
pub struct Live {
    served: Arc<Mutex<Option<Served>>>,
}

impl Live {
    /// Serves `name` in `folder` on `node` from now on, instead of any
    /// folder before: the address the view loads it from.
    pub fn serve(&self, node: &str, folder: &str, name: &str, reach: Reach) -> String {
        let site = site(node, folder);
        let url = scheme_url(SCHEME, &site, name);
        let mut served = self.served.lock().unwrap_or_else(|e| e.into_inner());
        if !served
            .as_ref()
            .is_some_and(|s| s.node == node && s.folder == folder)
        {
            *served = Some(Served {
                node: node.into(),
                folder: folder.into(),
                site,
                reach,
            });
        }
        url
    }

    /// Plays `name` in `folder` on `node` from now on, as [`Self::serve`]
    /// serves a page: the address of the player page for it, `sound` for a
    /// sound file, on a page the colour `background` (`#rrggbb`). The
    /// player names `open_id` (the open's number) in what it says.
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &self,
        node: &str,
        folder: &str,
        name: &str,
        open_id: u64,
        sound: bool,
        background: &str,
        reach: Reach,
    ) -> String {
        let root = self.serve(node, folder, "", reach);
        format!(
            "{root}?play={}&open={open_id}&as={}&bg={}",
            enc(name),
            if sound { "audio" } else { "video" },
            enc(background)
        )
    }

    /// Shows the 3D model `name` in `folder` on `node` from now on, as
    /// [`Self::play`] plays a sound: the address of the model page for it,
    /// on the colour `background` (`#rrggbb`), naming `open_id` (the
    /// open's number) in what it says.
    pub fn show_model(
        &self,
        node: &str,
        folder: &str,
        name: &str,
        open_id: u64,
        background: &str,
        reach: Reach,
    ) -> String {
        let root = self.serve(node, folder, "", reach);
        format!(
            "{root}?model={}&open={open_id}&bg={}",
            enc(name),
            enc(background)
        )
    }

    /// Whether the file at `path` on `node` is in the folder served now, so
    /// a page shown from it may load it.
    pub fn serves(&self, node: &str, path: &str) -> bool {
        let served = self.served.lock().unwrap_or_else(|e| e.into_inner());
        served.as_ref().is_some_and(|s| {
            s.node == node
                && path
                    .strip_prefix(s.folder.as_str())
                    .is_some_and(|rest| rest.starts_with('/') || s.folder.ends_with('/'))
        })
    }

    /// The scheme a view loads its pages from.
    pub fn scheme(&self) -> Scheme {
        let live = self.clone();
        Scheme {
            name: SCHEME,
            serve: Arc::new(move |asked, respond| live.answer(asked, respond)),
        }
    }

    /// Answers one request of the page.
    pub fn answer(&self, asked: SchemeRequest, respond: Respond) {
        if asked.site == APP_SITE {
            return respond(app_file(&asked));
        }
        let served = self
            .served
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(served) = served.filter(|s| s.site == asked.site) else {
            return respond(SchemeResponse::refused(
                403,
                "That address is not the open page's folder.",
            ));
        };
        if asked.method != "GET" {
            return respond(SchemeResponse::refused(405, "Pages here only read."));
        }
        let path = asked.path.trim_start_matches('/');
        if path
            .split('/')
            .any(|part| part == ".." || part == "." || part.contains('\\'))
        {
            return respond(SchemeResponse::refused(
                403,
                "That is outside the page's folder.",
            ));
        }
        if path.is_empty() {
            let model = asked.query.split('&').any(|p| p.starts_with("model="));
            return respond(SchemeResponse {
                status: 200,
                headers: vec![
                    ("Content-Type".into(), "text/html; charset=utf-8".into()),
                    ("Cache-Control".into(), "no-store".into()),
                ],
                body: if model {
                    model_page().into_bytes()
                } else {
                    PLAYER.as_bytes().to_vec()
                },
            });
        }
        let uri = format!(
            "{FOLDER_FILE_PATH}?folder={}&path={}",
            enc(&served.folder),
            enc(path)
        );
        let mut request = Request::get(NodeId(served.node.clone()), uri);
        let range = asked.header("range").and_then(bounded);
        let ranged = range.is_some();
        if let Some(range) = range {
            request = request.header("Range", range);
        }
        let opened = served.reach.open(request);
        served.reach.spawn(async move {
            respond(match opened.await {
                Ok(answer) => read(answer, ranged).await,
                Err(e) => SchemeResponse::refused(502, e.message),
            })
        });
    }
}

/// One of the app's own files, from [`APP_SITE`]: three.js, read by any
/// folder's model page (a classic script, so no other site needs leave).
fn app_file(asked: &SchemeRequest) -> SchemeResponse {
    if asked.method != "GET" {
        return SchemeResponse::refused(405, "Pages here only read.");
    }
    if asked.path != "three.js" {
        return SchemeResponse::refused(404, "The app has no such file.");
    }
    SchemeResponse {
        status: 200,
        headers: vec![
            (
                "Content-Type".into(),
                "text/javascript; charset=utf-8".into(),
            ),
            ("Content-Length".into(), THREE.len().to_string()),
        ],
        body: THREE.to_vec(),
    }
}

/// The node's answer as the page gets it: the whole file when it was asked
/// whole, else the range it was asked, at most [`MOST`] bytes of it.
async fn read(mut answer: ahjo_app_kit::link::http::Streamed, ranged: bool) -> SchemeResponse {
    let mut headers: Vec<(String, String)> = answer
        .headers
        .iter()
        .filter(|(n, _)| PASSED.contains(&n.as_str()))
        .cloned()
        .collect();
    headers.push(("Cache-Control".into(), "no-store".into()));
    if !(200..300).contains(&answer.status) {
        let body = read_all(&mut answer).await.unwrap_or_default();
        let why = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        let mut refused = SchemeResponse::refused(answer.status, why);
        refused.headers.extend(
            headers
                .into_iter()
                .filter(|(n, _)| n == "content-range" || n == "Cache-Control"),
        );
        return refused;
    }
    // The node bounds a range already; this only guards against more.
    let most = if ranged { MOST as usize } else { usize::MAX };
    let mut body = Vec::new();
    loop {
        match answer.piece().await {
            Ok(Some(piece)) => {
                let room = most - body.len();
                body.extend_from_slice(&piece[..piece.len().min(room)]);
                if body.len() >= most {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => return SchemeResponse::refused(502, e),
        }
    }
    headers.push(("Content-Length".into(), body.len().to_string()));
    SchemeResponse {
        status: answer.status,
        headers,
        body,
    }
}

async fn read_all(answer: &mut ahjo_app_kit::link::http::Streamed) -> Result<Vec<u8>, String> {
    answer.up_to(64 << 10).await
}

/// One range a page asked for, made to hold at most [`MOST`] bytes; `None`
/// for anything but one byte range, which asks for the whole.
fn bounded(asked: &str) -> Option<String> {
    let spec = asked.trim().strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (from, to) = spec.split_once('-')?;
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() {
        let last: u64 = to.parse().ok()?;
        return Some(format!("bytes=-{}", last.min(MOST)));
    }
    let from: u64 = from.parse().ok()?;
    let most = from.saturating_add(MOST - 1);
    let to = if to.is_empty() {
        most
    } else {
        to.parse::<u64>().ok()?.min(most)
    };
    Some(format!("bytes={from}-{to}"))
}

/// The page a sound or video file plays in, at its folder's root: the file
/// named in its query (`play`), for the open numbered `open`, as sound or
/// video (`as`), on its background (`bg`). It plays from the folder like any page's file, so the player
/// asks for ranges as it plays and seeks. When the web view cannot play it
/// (an error, or a video whose picture it cannot decode), it tells the app
/// ([`CannotShow`], [`Failure::Media`]).
pub const PLAYER: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Player</title>
<style>html,body{margin:0;height:100%}body{display:flex;align-items:center;justify-content:center}
video{max-width:100%;max-height:100%}audio{width:min(90%,640px)}</style></head><body><script>
(() => {
  const asked = new URLSearchParams(location.search);
  const name = asked.get("play");
  if (!name) return;
  document.title = name;
  if (/^#[0-9a-fA-F]{6}$/.test(asked.get("bg") || "")) document.body.style.background = asked.get("bg");
  const sound = asked.get("as") === "audio";
  const media = document.createElement(sound ? "audio" : "video");
  media.id = "media";
  media.controls = true;
  media.preload = "metadata";
  let told = false;
  const tell = (code, message) => {
    if (told || !window.ipc) return;
    told = true;
    window.ipc.postMessage("ahjo-files:" + JSON.stringify({ site: location.hostname, name, open_id: Number(asked.get("open")) || 0, failed: "media", code, message }));
  };
  media.addEventListener("error", () => tell(media.error ? media.error.code : 0, media.error ? media.error.message : ""));
  if (!sound) media.addEventListener("loadedmetadata", () => { if (!media.videoWidth) tell(0, "no picture"); });
  media.src = encodeURIComponent(name);
  document.body.append(media);
})();
</script></body></html>"#;

/// The page a 3D model turns in, at its folder's root: the file named in
/// its query (`model`: glb, gltf, obj, fbx or stl), for the open numbered
/// `open`, on its background (`bg`). It loads the model and what the model
/// names beside it (a glTF's buffers and textures, an OBJ's materials and
/// theirs) from the folder like any page's file, and three.js from the app
/// ([`APP_SITE`], its address the kit's [`scheme_url`]). A drag turns it,
/// the wheel zooms, Reset view goes back; it draws when one of those
/// changes the view, and when a file the model names arrives after it first
/// showed (a texture). A file it names that is not there is named on the
/// page, whenever that turns out; when the model does not load at all it
/// tells the app why ([`CannotShow`], [`Failure::Model`]). `window.view`
/// holds its camera and controls, for a check to read.
pub fn model_page() -> String {
    MODEL_PAGE.replace("{three}", &scheme_url(SCHEME, APP_SITE, "three.js"))
}

/// [`model_page`], before it is given three.js's address.
const MODEL_PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8"><title>Model</title>
<style>html,body{margin:0;height:100%;overflow:hidden;font:13px system-ui,sans-serif}
canvas{display:block;touch-action:none;cursor:grab}canvas:active{cursor:grabbing}
button,#note{position:fixed;padding:4px 10px;border-radius:4px;background:rgba(128,128,128,.18);border:1px solid rgba(128,128,128,.4);color:inherit;font:inherit}
button{top:8px;right:8px;cursor:pointer}#note{left:8px;bottom:8px;max-width:calc(100% - 40px)}</style></head>
<body><button id="reset" title="The view it opened with">Reset view</button><div id="note" hidden></div>
<script src="{three}"></script><script>
(() => {
  const asked = new URLSearchParams(location.search);
  const name = asked.get("model");
  if (!name) return;
  document.title = name;
  const bg = /^#[0-9a-fA-F]{6}$/.test(asked.get("bg") || "") ? asked.get("bg") : "#202020";
  const lum = [1, 3, 5].map((i) => parseInt(bg.slice(i, i + 2), 16)).reduce((a, c, i) => a + c * [0.3, 0.59, 0.11][i], 0);
  document.body.style.background = bg;
  document.body.style.color = lum < 128 ? "#d8d8d8" : "#202020";
  const note = (text) => { const n = document.getElementById("note"); n.textContent = text; n.hidden = false; };
  let told = false;
  const tell = (why) => {
    note(name + ": " + why);
    if (told || !window.ipc) return;
    told = true;
    window.ipc.postMessage("ahjo-files:" + JSON.stringify({ site: location.hostname, name, open_id: Number(asked.get("open")) || 0, failed: "model", why }));
  };
  if (!window.THREE) return tell("the app's 3D viewer did not load");
  const T = window.THREE;
  let renderer;
  try {
    renderer = new T.WebGLRenderer({ antialias: true, alpha: true });
  } catch (e) {
    return tell("this view cannot draw 3D (" + (e && e.message || e) + ")");
  }
  renderer.setPixelRatio(window.devicePixelRatio || 1);
  document.body.prepend(renderer.domElement);
  const scene = new T.Scene();
  scene.environment = new T.PMREMGenerator(renderer).fromScene(new T.RoomEnvironment(), 0.04).texture;
  scene.add(new T.HemisphereLight(0xffffff, 0x404040, 1.2));
  const camera = new T.PerspectiveCamera(45, 1, 0.01, 1000);
  const sun = new T.DirectionalLight(0xffffff, 1.6);
  sun.position.set(1, 2, 3);
  camera.add(sun);
  scene.add(camera);
  const controls = new T.OrbitControls(camera, renderer.domElement);
  const draw = () => renderer.render(scene, camera);
  controls.addEventListener("change", draw);
  const size = () => {
    renderer.setSize(window.innerWidth, window.innerHeight);
    camera.aspect = window.innerWidth / Math.max(window.innerHeight, 1);
    camera.updateProjectionMatrix();
    draw();
  };
  window.addEventListener("resize", size);
  document.getElementById("reset").addEventListener("click", () => controls.reset());
  window.view = { camera, controls };
  // Files of the folder that could not be read, by their names in it. A
  // texture may only arrive, or fail, after the model first showed (an
  // OBJ's material starts its images and returns): each one that settles
  // then is drawn, or named.
  let shown = false;
  const missing = [];
  const fileOf = (url) => {
    try {
      const u = new URL(url, location.href);
      return u.host === location.host ? decodeURIComponent(u.pathname.slice(1)) : url;
    } catch { return url; }
  };
  const sayMissing = () => { if (shown && missing.length) note("Not found beside it: " + missing.join(", ")); };
  const manager = new T.LoadingManager();
  // The model itself failing is said as its loader says it, not as missing.
  manager.onError = (url) => {
    const f = fileOf(url);
    if (f !== name && !missing.includes(f)) missing.push(f);
    sayMissing();
  };
  manager.onProgress = () => { if (shown) draw(); };
  const at = (path) => new URL(path.replace(/\\/g, "/").split("/").map(encodeURIComponent).join("/"), location.href).href;
  const ext = name.split(".").pop().toLowerCase();
  const load = async () => {
    const url = at(name);
    if (ext === "glb" || ext === "gltf") return (await new T.GLTFLoader(manager).loadAsync(url)).scene;
    if (ext === "fbx") return await new T.FBXLoader(manager).loadAsync(url);
    if (ext === "stl") {
      const shape = await new T.STLLoader(manager).loadAsync(url);
      return new T.Mesh(shape, new T.MeshStandardMaterial({ color: shape.hasColors ? 0xffffff : 0xb4b8bc, vertexColors: !!shape.hasColors, roughness: 0.6, metalness: 0.1 }));
    }
    if (ext === "obj") {
      const text = await new T.FileLoader(manager).loadAsync(url);
      const obj = new T.OBJLoader(manager);
      for (const [, lib] of text.matchAll(/^[ \t]*mtllib[ \t]+(.+?)[ \t]*$/gm)) {
        try {
          const materials = await new T.MTLLoader(manager).loadAsync(at(lib));
          materials.preload();
          obj.setMaterials(materials);
        } catch {}
      }
      return obj.parse(text);
    }
    throw new Error("its kind is not one this view shows");
  };
  load().then((object) => {
    const box = new T.Box3().setFromObject(object);
    if (box.isEmpty()) throw new Error("it has nothing to draw");
    scene.add(object);
    const ball = box.getBoundingSphere(new T.Sphere());
    const radius = ball.radius || 1;
    const away = radius / Math.sin(T.MathUtils.degToRad(camera.fov / 2));
    camera.near = away / 100;
    camera.far = away * 100;
    camera.position.copy(ball.center).add(new T.Vector3(0.6, 0.45, 1).normalize().multiplyScalar(away));
    controls.target.copy(ball.center);
    controls.update();
    controls.saveState();
    shown = true;
    size();
    sayMissing();
    document.body.dataset.shown = "yes";
  }).catch((e) => {
    tell(missing.length
      ? missing.join(", ") + (missing.length > 1 ? " are" : " is") + " not there or could not be read"
      : (e && e.message) || String(e));
  });
})();
</script></body></html>"##;

/// What a player or model page tells the app when the web view cannot play
/// or show its file (posted as `ahjo-files:` and this as JSON): the file
/// `name` of the folder served as `site`, as it was opened as `open_id`,
/// and how it failed.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct CannotShow {
    pub site: String,
    pub name: String,
    pub open_id: u64,
    #[serde(flatten)]
    pub failure: Failure,
}

/// How a page of the app's own failed to show its file (`failed` in what
/// it posts).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(tag = "failed", rename_all = "snake_case")]
pub enum Failure {
    /// The player's media error: `code` is the media error's (2 it stopped
    /// loading, 3 it could not decode it, 4 it does not play its format),
    /// or 0 for a video whose picture did not show.
    Media { code: u16, message: String },
    /// The model page could not load the model, and why, in words.
    Model { why: String },
}

impl CannotShow {
    /// The message a player or model page posted, when it is one.
    pub fn of(message: &str) -> Option<Self> {
        let said: Self = serde_json::from_str(message.strip_prefix("ahjo-files:")?).ok()?;
        // The site is the host's first name: `ahjo-file.<site>.localhost`.
        let site = said
            .site
            .strip_prefix(&format!("{SCHEME}."))
            .and_then(|s| s.strip_suffix(".localhost"))
            .unwrap_or(&said.site)
            .to_owned();
        Some(Self { site, ..said })
    }

    /// Whether it is about the file `name` in `folder` on `node`, as it
    /// was opened as `open_id`.
    pub fn is_about(&self, node: &str, folder: &str, name: &str, open_id: u64) -> bool {
        self.site == site(node, folder) && self.name == name && self.open_id == open_id
    }

    /// Why, in a sentence, for the operator.
    pub fn why(&self) -> String {
        let name = &self.name;
        match &self.failure {
            Failure::Model { why } => format!("{name} does not show here: {why}"),
            Failure::Media { code: 0, .. } => format!(
                "{name} shows no picture here: its video is in a format this view does not play"
            ),
            Failure::Media { code: 2, message } => format!("{name} stopped loading: {message}"),
            Failure::Media { code: 3, .. } => {
                format!("{name} does not play here: its data did not decode")
            }
            Failure::Media { .. } => {
                format!("{name} does not play here: this view does not play its format")
            }
        }
    }
}

/// The site a folder serves as: a hash of its node and path, so no page
/// learns either from its address.
fn site(node: &str, folder: &str) -> String {
    let hash = Sha256::digest(format!("{node}\n{folder}").as_bytes());
    hex::encode(&hash[..10])
}

pub(crate) fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_holds_at_most_one_answer() {
        assert_eq!(bounded("bytes=0-99").as_deref(), Some("bytes=0-99"));
        assert_eq!(
            bounded("bytes=10-").as_deref(),
            Some(format!("bytes=10-{}", 10 + MOST - 1).as_str())
        );
        assert_eq!(
            bounded(&format!("bytes=0-{}", 4 * MOST)).as_deref(),
            Some(format!("bytes=0-{}", MOST - 1).as_str())
        );
        assert_eq!(
            bounded(&format!("bytes=-{}", 2 * MOST)).as_deref(),
            Some(format!("bytes=-{MOST}").as_str())
        );
        assert_eq!(bounded("bytes=0-1,5-6"), None);
        assert_eq!(bounded("items=0-1"), None);
    }

    #[test]
    fn a_player_says_which_file_it_cannot_play() {
        let host = format!("{SCHEME}.{}.localhost", site("box", "/work"));
        let said = CannotShow::of(&format!(
            r#"ahjo-files:{{"site":"{host}","name":"a.avi","open_id":7,"failed":"media","code":4,"message":"no demuxer"}}"#
        ))
        .expect("a player's message");
        assert!(said.is_about("box", "/work", "a.avi", 7));
        assert!(!said.is_about("box", "/work", "a.avi", 8));
        assert!(!said.is_about("box", "/work", "b.avi", 7));
        assert!(!said.is_about("box", "/other", "a.avi", 7));
        assert_eq!(
            said.why(),
            "a.avi does not play here: this view does not play its format"
        );
        assert_eq!(CannotShow::of("ahjo-kit:escape"), None);
        assert_eq!(CannotShow::of(r#"ahjo-files:{"site":"x"}"#), None);
    }

    #[test]
    fn a_model_page_loads_three_from_the_app_and_says_why_it_failed() {
        let three = ahjo_app_kit::web_view::scheme_url(SCHEME, APP_SITE, "three.js");
        let page = model_page();
        assert!(page.contains(&format!(r#"<script src="{three}">"#)));
        assert!(!page.contains("{three}"));
        // No folder's site (hex) is the app's.
        assert!(!APP_SITE.bytes().all(|b| b.is_ascii_hexdigit()));
        let host = format!("{SCHEME}.{}.localhost", site("box", "/work"));
        let said = CannotShow::of(&format!(
            r#"ahjo-files:{{"site":"{host}","name":"scene.gltf","open_id":2,"failed":"model","why":"scene.bin is not there or could not be read"}}"#
        ))
        .expect("a model page's message");
        assert!(said.is_about("box", "/work", "scene.gltf", 2));
        assert_eq!(
            said.failure,
            Failure::Model {
                why: "scene.bin is not there or could not be read".into()
            }
        );
        assert_eq!(
            said.why(),
            "scene.gltf does not show here: scene.bin is not there or could not be read"
        );
    }

    #[test]
    fn each_folder_is_a_site_of_its_own() {
        let a = site("box", "/work/a");
        assert_eq!(a.len(), 20);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, site("box", "/work/b"));
        assert_ne!(a, site("other", "/work/a"));
    }
}
