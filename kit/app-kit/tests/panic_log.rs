//! A panic the app cannot survive is in its log. On Windows the window
//! procedure is an `extern "system"` callback: a panic in it cannot unwind
//! out and ends the process at once (0xc0000409). This test does the same in
//! a child process, a panic inside an `extern "C"` function, and reads the
//! log the child left.

use std::process::Command;

const CHILD: &str = "AHJO_PANIC_LOG_CHILD";

extern "C" fn window_procedure() {
    let tabs: Vec<&str> = Vec::new();
    let i = tabs.len() + 1;
    panic!("the tab {i} is not there");
}

#[test]
fn a_panic_that_ends_the_app_is_in_its_log() {
    if let Some(path) = std::env::var_os(CHILD) {
        ahjo_app_kit::app_log::install("ahjo-test", Some(path.into()));
        window_procedure();
        unreachable!("a panic in an extern \"C\" function ends the process");
    }
    let dir = std::env::temp_dir().join(format!("ahjo-panic-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The log's folder is made when it is missing, as on a first start.
    let path = dir.join("logs").join("app.log");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "a_panic_that_ends_the_app_is_in_its_log"])
        .env(CHILD, &path)
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();
    assert!(!out.status.success(), "the child carried on: {out:?}");
    let log = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "no log at {}: {e}; stderr: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    std::fs::remove_dir_all(&dir).unwrap();
    let first = log.lines().next().unwrap();
    assert!(
        first.contains("ahjo-test PANIC on thread '")
            && first.contains("panic_log.rs:")
            && first.ends_with(": the tab 1 is not there"),
        "{log}"
    );
    // The backtrace, though RUST_BACKTRACE is not set, names the function.
    assert!(log.contains("window_procedure"), "{log}");
}
