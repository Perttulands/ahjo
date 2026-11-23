//! What tests start: a node holding only the areas a test names
//! ([`node`]), a tmux server of the test's own ([`tmux`]), a whole
//! `ahjo serve` process ([`process`]), a stand-in for the sessions
//! ([`sessions`]) and, with the `app` feature, what app tests share
//! ([`app`]).

#[cfg(feature = "app")]
pub mod app;
pub mod node;
pub mod process;
pub mod sessions;
#[cfg(unix)]
pub mod tmux;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// How long a test waits for something that takes up to `base` on an
/// ordinary machine: `base` times `AHJO_TEST_TIMEOUT_SCALE` (default 1), so
/// a slow machine waits longer without any test changing. A scale that is
/// not a positive number fails the test.
pub fn timeout(base: Duration) -> Duration {
    match std::env::var("AHJO_TEST_TIMEOUT_SCALE") {
        Err(_) => base,
        Ok(scale) => match scale.parse::<f64>() {
            Ok(scale) if scale > 0. && scale.is_finite() => base.mul_f64(scale),
            _ => panic!("AHJO_TEST_TIMEOUT_SCALE={scale:?} is not a positive number"),
        },
    }
}

/// A directory only this test uses: freshly created, so nothing else was
/// there before, and private to the user running the tests.
pub fn fresh_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    loop {
        let dir = std::env::temp_dir().join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return dir,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("creating {}: {error}", dir.display()),
        }
    }
}
