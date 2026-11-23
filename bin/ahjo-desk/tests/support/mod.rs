//! A desk of the test's own: the decision desk's HTTP API as the app uses
//! it (`/srv/desk/server.py`), holding the asks a test gives it and keeping
//! every write the app makes, so a test reads what the agents would.

#![allow(dead_code)]

use ahjo_app_desk::app::DeskApp;
use ahjo_app_desk::desk::{Config, Loaded, Place};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct State {
    pub asks: Vec<Value>,
    /// Every POST: its path and body.
    pub posts: Vec<(String, Value)>,
}

pub struct FakeDesk {
    pub url: String,
    pub state: Arc<Mutex<State>>,
}

impl FakeDesk {
    pub fn start(asks: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}", listener.local_addr().expect("its address"));
        let state = Arc::new(Mutex::new(State {
            asks,
            posts: vec![],
        }));
        let shared = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let state = shared.clone();
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        Self { url, state }
    }

    pub fn posts(&self) -> Vec<(String, Value)> {
        self.state.lock().unwrap().posts.clone()
    }
}

fn serve(mut stream: std::net::TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().expect("a stream"));
    let mut head = String::new();
    if reader.read_line(&mut head).is_err() {
        return;
    }
    let mut parts = head.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    let (status, reply) = route(&method, &path, &body, state);
    let bytes = reply.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{bytes}",
        bytes.len()
    );
}

fn route(method: &str, path: &str, body: &[u8], state: &Mutex<State>) -> (&'static str, Value) {
    let mut s = state.lock().unwrap();
    match (method, path) {
        ("GET", "/api/asks") => ("200 OK", Value::Array(s.asks.clone())),
        ("GET", "/api/desks") => ("200 OK", json!([])),
        ("POST", p) if p.starts_with("/api/asks/") => {
            let payload: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            s.posts.push((p.to_string(), payload.clone()));
            let mut bits = p.trim_start_matches("/api/asks/").split('/');
            let (id, action) = (bits.next().unwrap_or(""), bits.next().unwrap_or(""));
            let Some(ask) = s.asks.iter_mut().find(|a| a["id"] == id) else {
                return ("404 Not Found", json!({"error": "Unknown ask"}));
            };
            match action {
                "marks" => ask["marks"] = payload["marks"].clone(),
                "answer" => {
                    ask["answer"] = json!({
                        "verdict": payload["verdict"],
                        "options": payload["options"],
                        "comment": payload["comment"],
                        "at": "2026-11-23T10:00:00+00:00",
                    });
                    ask["status"] = json!("answered");
                }
                _ => return ("400 Bad Request", json!({"error": "Unknown action"})),
            }
            let revision = ask["revision"].as_u64().unwrap_or(1) + 1;
            ask["revision"] = json!(revision);
            ("200 OK", ask.clone())
        }
        _ => ("404 Not Found", json!({"error": "not found"})),
    }
}

/// A document ask as the desk keeps one.
pub fn doc_ask(id: &str, text: &str) -> Value {
    json!({
        "id": id, "bead": "ahjo-test", "kind": "look", "title": format!("{id} to read"),
        "question": "Mark what is wrong.", "status": "open", "round": 1, "revision": 1,
        "media": [], "options": [], "comments": [], "marks": [], "answer": null,
        "doc": {"name": "doc.md", "text": text},
        "created_at": "2026-11-23T09:00:00+00:00", "updated_at": "2026-11-23T09:00:00+00:00",
    })
}

/// A grilling ask on its `round`th round, each round three questions (the
/// last answered in words only) and every earlier one answered.
pub fn grill_ask(id: &str, round: u32) -> Value {
    let one = |n: u32, answered: bool| {
        json!({
            "id": id, "bead": "ahjo-test", "kind": "grill", "title": format!("Round {n} of the grilling"),
            "question": "Three questions.", "status": if answered { "answered" } else { "open" },
            "round": n, "revision": n, "media": [], "options": [], "comments": [], "marks": [],
            "questions": [
                {"id": "q1", "title": format!("Round {n}: which way?"), "options": [
                    {"name": "One", "why": "The first way"}, {"name": "Other", "why": "The second way"}],
                 "recommend": "One"},
                {"id": "q2", "title": format!("Round {n}: how many?"), "options": [
                    {"name": "Few", "why": "Enough to tell"}, {"name": "Many", "why": "A long sitting"}],
                 "recommend": "Few"},
                {"id": "q3", "title": format!("Round {n}: anything else?")},
            ],
            "answer": if answered {
                json!({"verdict": "grill", "options": [], "comment": "", "at": "2026-11-23T09:30:00+00:00",
                       "answers": {"q1": {"pick": "One", "comment": ""}, "q3": {"pick": null, "comment": "Nothing more."}}})
            } else {
                Value::Null
            },
            "created_at": "2026-11-23T09:00:00+00:00", "updated_at": "2026-11-23T09:00:00+00:00",
        })
    };
    let mut ask = one(round, false);
    ask["history"] = (1..round).map(|n| one(n, true)).collect();
    ask
}

/// A scratch folder for what the app keeps (its settings and drafts), so
/// nothing of the operator's is read or written.
pub fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ahjo-desk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// The Desk in a test window on `desk`, its settings in `dir`.
pub fn open<'a>(
    desk: &FakeDesk,
    dir: &std::path::Path,
    cx: &'a mut TestAppContext,
) -> (Entity<DeskApp>, &'a mut VisualTestContext) {
    // SAFETY: set before the app starts, and every test of this binary sets
    // its own folder the same way before reading it.
    unsafe { std::env::set_var("AHJO_DESK_CONFIG", dir.join("desk.json")) };
    cx.executor().allow_parking();
    cx.update(|cx| {
        cx.set_global(ahjo_app_kit::theme::Theme::default());
        ahjo_app_kit::widgets::field::bind_keys(cx);
    });
    let loaded = Loaded {
        config: Config {
            tell_session: false,
            ..Default::default()
        },
        places: vec![Place::parse(&format!("Test={}", desk.url))],
        notice: None,
        kept: false,
    };
    let (app, cx) = cx.add_window_view(move |window, cx| DeskApp::new(loaded, None, window, cx));
    until(cx, "the asks", |cx| {
        app.read_with(cx, |a, _| a.current_ask().is_some())
    });
    (app, cx)
}

pub fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut ok: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if ok(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The centre of what `selector` draws, in the platform's pixels.
pub fn centre(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
    cx.run_until_parked();
    let ui = cx.update(|window, _| window.ui_scale());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
        .center()
        .map(|v| v * ui)
}

pub fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = centre(cx, selector);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

/// A wheel notch down (`lines` of them) at `at`.
pub fn wheel(cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, lines: f32) {
    cx.simulate_event(gpui::ScrollEvent {
        position: at,
        delta: gpui::ScrollDelta::Lines(point(0., -lines)),
        ..Default::default()
    });
    cx.run_until_parked();
}
