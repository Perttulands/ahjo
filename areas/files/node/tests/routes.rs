//! Files' HTTP interface, as a browser or another node calls it.

use std::path::{Path, PathBuf};

use ahjo_files::{Files, MAX_BYTES};
use ahjo_node_kit::{NodeClient, NodeError};
use ahjo_testkit::node::Node;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const NODE: &str = "test-node";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path =
            std::env::temp_dir().join(format!("ahjo-files-http-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path.canonicalize().unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app(root: &Path) -> Router {
    ahjo_files::routes(
        std::sync::Arc::new(Files::new(NODE.to_owned(), &[root.to_path_buf()], None).unwrap()),
        std::sync::Arc::new(ahjo_files::drops::Drops::new(root.join("drops"), vec![])),
    )
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn at(route: &str, path: &Path) -> String {
    format!(
        "/api/files/{route}?node={NODE}&path={}",
        encode(path.to_str().unwrap())
    )
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: impl Into<Body>,
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(body.into())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body, headers)
}

/// Run a typed command on a node with this test's roots.
async fn command(
    root: &Path,
    name: &str,
    arguments: Value,
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let config = format!(
        "[files]\nroots = [{}]\n",
        serde_json::to_string(root.to_str().unwrap()).unwrap()
    );
    let node = Node::start(&[ahjo_files::LISTING], &config).await.unwrap();
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let result = client
        .request(
            axum::http::Method::POST,
            &format!("/api/call/{name}"),
            Some(arguments),
            None,
        )
        .await;
    let answer = match result {
        Ok((value, _)) => (StatusCode::OK, serde_json::to_vec(&value).unwrap()),
        Err(error) => {
            let error = error.downcast_ref::<NodeError>().unwrap();
            (
                error.status,
                serde_json::to_vec(&json!({"error":{"code":error.code,"message":error.message}}))
                    .unwrap(),
            )
        }
    };
    node.stop().await;
    (answer.0, answer.1, axum::http::HeaderMap::new())
}

/// The `base` an editor sends: the SHA-256 of what it read, in hex.
fn version(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn roots_name_the_node_and_its_roots() {
    let scratch = Scratch::new("roots");
    let (status, body, _) = command(&scratch.0, "file_list", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let path = scratch.0.to_str().unwrap();
    // The name drops Windows' verbatim prefix; the path keeps it.
    let name = path.strip_prefix(r"\\?\").unwrap_or(path);
    let answer: Value = serde_json::from_slice(&body).unwrap();
    assert!(!answer["node"].as_str().unwrap().is_empty());
    assert_eq!(answer["roots"], json!([{"name": name, "path": path}]));
}

#[tokio::test]
async fn a_request_for_another_node_is_not_answered_here() {
    let scratch = Scratch::new("other");
    let uri = format!(
        "/api/files/file?node=elsewhere&path={}",
        encode(scratch.0.to_str().unwrap())
    );
    let (status, body, _) = call(&app(&scratch.0), "GET", &uri, Body::empty()).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    assert!(text(&body).contains("elsewhere"), "{}", text(&body));
}

#[tokio::test]
async fn create_save_list_read_rename_delete() {
    let scratch = Scratch::new("cycle");
    let app = app(&scratch.0);
    let folder = scratch.0.join("models");
    let file = folder.join("notes.md");

    assert_eq!(
        command(&scratch.0, "file_mkdir", json!({"path":folder}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &at("file", &file), Body::empty())
            .await
            .0,
        StatusCode::CREATED
    );
    let (status, body, _) = call(&app, "POST", &at("file", &file), Body::empty()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text(&body).contains("error"));

    assert_eq!(
        call(&app, "PUT", &at("file", &file), "# kuvio").await.0,
        StatusCode::NO_CONTENT
    );
    let (status, body, _) = command(&scratch.0, "file_list", json!({"path":folder})).await;
    assert_eq!(status, StatusCode::OK);
    let listing = text(&body);
    assert!(
        listing.contains(r#""name":"notes.md","folder":false,"size":7"#),
        "{listing}"
    );

    let (status, body, headers) = call(&app, "GET", &at("file", &file), Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"# kuvio");
    assert_eq!(headers["content-type"], "application/octet-stream");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["content-security-policy"], "sandbox");

    let moved = folder.join("README.md");
    assert_eq!(
        command(&scratch.0, "file_move", json!({"path":file,"to":moved}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(std::fs::read(&moved).unwrap(), b"# kuvio");

    assert_eq!(
        command(&scratch.0, "file_delete", json!({"path":folder}))
            .await
            .0,
        StatusCode::OK
    );
    assert!(!folder.exists());
}

#[tokio::test]
async fn pictures_are_served_as_pictures() {
    let scratch = Scratch::new("picture");
    let picture = scratch.0.join("Shot.PNG");
    std::fs::write(&picture, b"\x89PNG").unwrap();
    let (status, _, headers) = call(
        &app(&scratch.0),
        "GET",
        &at("file", &picture),
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
}

#[tokio::test]
async fn viewer_reads_original_bytes_and_missing_files_report_why() {
    let scratch = Scratch::new("viewer");
    let app = app(&scratch.0);
    for (name, contents) in [
        ("notes.md", &b"# Notes\n[other](other.md)\n"[..]),
        ("data.json", &b"{\"a\":1}"[..]),
        ("binary.dat", &b"\0\x01\xff"[..]),
    ] {
        let file = scratch.0.join(name);
        std::fs::write(&file, contents).unwrap();
        let (status, body, _) = call(&app, "GET", &at("file", &file), Body::empty()).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(body, contents, "{name}");
    }
    let (status, body, _) = call(
        &app,
        "GET",
        &at("file", &scratch.0.join("missing.png")),
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.is_empty(), "the viewer has an error to show");
}

#[tokio::test]
async fn refusals_carry_their_status() {
    let scratch = Scratch::new("refusals");
    let root = scratch.0.join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(scratch.0.join("outside.txt"), "x").unwrap();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::write(root.join("b.txt"), "b").unwrap();
    let app = app(&root);

    let outside = scratch.0.join("outside.txt");
    assert_eq!(
        call(&app, "GET", &at("file", &outside), Body::empty())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        command(&root, "file_delete", json!({"path":root})).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", &at("file", &root.join("gone")), Body::empty())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        command(&root, "file_list", json!({"path":root.join("a.txt")}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        command(
            &root,
            "file_move",
            json!({"path":root.join("a.txt"), "to":root.join("b.txt")})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"b");
}

#[tokio::test]
async fn a_save_from_a_stale_base_and_an_upload_over_a_file_are_refused() {
    let scratch = Scratch::new("stale");
    let app = app(&scratch.0);
    let file = scratch.0.join("plan.txt");

    let (status, _, _) = call(&app, "POST", &at("file", &file), "first").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(std::fs::read(&file).unwrap(), b"first");
    let (status, _, _) = call(&app, "POST", &at("file", &file), "upload").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(&file).unwrap(), b"first");

    let read = version(b"first");
    std::fs::write(&file, "an agent's edit").unwrap();
    let stale = format!("{}&base={read}", at("file", &file));
    let (status, body, _) = call(&app, "PUT", &stale, "operator's draft").await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert!(text(&body).contains("Changed"), "{}", text(&body));
    assert_eq!(std::fs::read(&file).unwrap(), b"an agent's edit");

    let current = version(b"an agent's edit");
    let fresh = format!("{}&base={current}", at("file", &file));
    assert_eq!(
        call(&app, "PUT", &fresh, "merged").await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"merged");
}

#[tokio::test]
async fn an_oversized_save_is_refused_and_writes_nothing() {
    let scratch = Scratch::new("oversized");
    let file = scratch.0.join("big.bin");
    let body = vec![0u8; MAX_BYTES as usize + 1];
    let (status, _, _) = call(&app(&scratch.0), "PUT", &at("file", &file), body).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!file.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn find_and_content_search_report_an_unreadable_chosen_folder() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("find-permission");
    let locked = scratch.0.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("note.txt"), "needle").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let mut responses = Vec::new();
    for (name, arguments) in [
        ("file_locate", json!({"under":locked,"query":"needle"})),
        ("file_search", json!({"path":locked,"query":"needle"})),
    ] {
        responses.push(command(&scratch.0, name, arguments).await);
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (status, body, _) in responses {
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(text(&body).contains("Permission denied"), "{}", text(&body));
    }
}

#[tokio::test]
async fn diff_reports_head_changes_no_repository_and_git_errors() {
    use std::process::Command;
    let scratch = Scratch::new("diff");
    let repo = scratch.0.join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    let file = repo.join("[notes].txt");
    std::fs::write(&file, "old\n").unwrap();
    let (_, body, _) = command(&scratch.0, "file_diff", json!({"path":file})).await;
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(result["error"].as_str().unwrap().contains("HEAD"));
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-qm",
        "initial",
    ]);
    std::fs::write(&file, "new\n").unwrap();
    let (status, body, _) = command(&scratch.0, "file_diff", json!({"path":file})).await;
    assert_eq!(status, StatusCode::OK);
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["repository"], repo.to_str().unwrap());
    assert!(result["diff"].as_str().unwrap().contains("-old\n+new"));
    assert_eq!(result["truncated"], false);
    std::fs::write(&file, format!("{}\n", "x".repeat(1_100_000))).unwrap();
    let (_, body, _) = command(&scratch.0, "file_diff", json!({"path":file})).await;
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["truncated"], true);
    assert!(result["diff"].as_str().unwrap().len() <= 1 << 20);
    let plain = scratch.0.join("plain");
    std::fs::write(&plain, "plain").unwrap();
    let (_, body, _) = command(&scratch.0, "file_diff", json!({"path":plain})).await;
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["repository"], "");
    assert_eq!(result["diff"], "");
    assert_eq!(
        command(&scratch.0, "file_diff", json!({"path":scratch.0}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        command(
            &scratch.0,
            "file_diff",
            json!({"path":scratch.0.join("missing")})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn status_names_every_file_that_differs_from_head_and_a_whole_diff_has_every_line() {
    use std::process::Command;
    let scratch = Scratch::new("status");
    let repo = scratch.0.join("repo");
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .current_dir(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    std::fs::write(repo.join("kept.txt"), &lines).unwrap();
    std::fs::write(repo.join("docs/gone.md"), "gone\n").unwrap();
    std::fs::write(repo.join("docs/old name.md"), "rename me\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-qm",
        "initial",
    ]);
    std::fs::write(
        repo.join("kept.txt"),
        lines.replace("line 10\n", "line ten\n"),
    )
    .unwrap();
    std::fs::remove_file(repo.join("docs/gone.md")).unwrap();
    std::fs::write(repo.join("docs/new.md"), "new\n").unwrap();
    git(&["config", "status.renames", "false"]);
    git(&["mv", "docs/old name.md", "renamed.md"]);
    let (status, body, _) =
        command(&scratch.0, "file_status", json!({"path":repo.join("docs")})).await;
    assert_eq!(status, StatusCode::OK);
    let result: ahjo_files_contract::RepoStatus = serde_json::from_slice(&body).unwrap();
    assert_eq!(result.repository, repo.to_str().unwrap());
    let mut states: Vec<_> = result
        .files
        .iter()
        .map(|f| {
            (
                f.path
                    .strip_prefix(repo.to_str().unwrap())
                    .unwrap()
                    .to_owned(),
                f.state,
            )
        })
        .collect();
    states.sort();
    assert_eq!(
        states,
        [
            ("/docs/gone.md".to_owned(), 'D'),
            ("/docs/new.md".to_owned(), '?'),
            ("/kept.txt".to_owned(), 'M'),
            ("/renamed.md".to_owned(), 'R'),
        ]
    );
    let rename = result.files.iter().find(|file| file.state == 'R').unwrap();
    assert_eq!(
        rename.renamed_from.as_deref(),
        repo.join("docs/old name.md").to_str()
    );
    assert!(
        result
            .files
            .iter()
            .filter(|file| file.state != 'R')
            .all(|file| file.renamed_from.is_none())
    );
    let (status, body, _) = command(
        &scratch.0,
        "file_status",
        json!({"path":repo.join("missing/parent")}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<ahjo_files_contract::RepoStatus>(&body).unwrap(),
        result
    );
    let file = repo.join("kept.txt");
    let (_, body, _) = command(&scratch.0, "file_diff", json!({"path":file})).await;
    let some: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!some["diff"].as_str().unwrap().contains("line 1\n"));
    let (_, body, _) = command(&scratch.0, "file_diff", json!({"path":file,"whole":true})).await;
    let all: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let diff = all["diff"].as_str().unwrap();
    assert!(diff.contains(" line 1\n") && diff.contains(" line 20\n"));
    assert!(diff.contains("-line 10\n+line ten\n"));
    // A file deleted since the commit still shows what went, also when its
    // folder went with it; a path that never was is not found, and one
    // that climbs out of the roots is refused.
    let gone = repo.join("docs/gone.md");
    let (status, body, _) = command(&scratch.0, "file_diff", json!({"path":gone})).await;
    assert_eq!(status, StatusCode::OK);
    let deleted: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(deleted["diff"].as_str().unwrap().contains("\n-gone\n"));
    std::fs::remove_file(repo.join("docs/new.md")).unwrap();
    std::fs::remove_dir(repo.join("docs")).unwrap();
    let (status, body, _) = command(&scratch.0, "file_diff", json!({"path":gone})).await;
    assert_eq!(status, StatusCode::OK);
    let deleted: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(deleted["diff"].as_str().unwrap().contains("\n-gone\n"));
    let plain = scratch.0.join("plain");
    std::fs::create_dir(&plain).unwrap();
    let never = plain.join("never/was.txt");
    let (status, _, _) = command(&scratch.0, "file_diff", json!({"path":never})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let climbing = scratch.0.join("gone/../../../etc/passwd");
    let (status, _, _) = command(&scratch.0, "file_diff", json!({"path":climbing})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, body, _) = command(&scratch.0, "file_status", json!({"path":plain})).await;
    let none: ahjo_files_contract::RepoStatus = serde_json::from_slice(&body).unwrap();
    assert_eq!(none, Default::default());
}
