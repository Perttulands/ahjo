//! What app tests share: a node served from a thread of its own while the
//! test drives the app on gpui's test platform ([`serve`]), waiting for what
//! the app shows ([`until`]) and clicking it ([`click`]).
//!
//! Behind the `app` feature, so node crates do not build the GUI toolkit.

use std::future::Future;
use std::time::{Duration, Instant};

use gpui::{Modifiers, VisualTestContext};

use crate::node::Node;
use crate::timeout;

/// How long [`until`] waits before it fails, before
/// `AHJO_TEST_TIMEOUT_SCALE`.
pub const UNTIL: Duration = Duration::from_secs(30);

/// What a test's node thread started: the node, what the test reads from
/// it (`value`) and what to do once the node has stopped (`then`).
pub struct Started<T> {
    node: Node,
    value: T,
    after: Vec<Box<dyn FnOnce() + Send>>,
}

impl<T> Started<T> {
    pub fn new(node: Node, value: T) -> Started<T> {
        Started {
            node,
            value,
            after: Vec::new(),
        }
    }

    /// Runs `after` in the node's thread once the node has stopped, such as
    /// removing a folder the node was serving.
    pub fn then(mut self, after: impl FnOnce() + Send + 'static) -> Started<T> {
        self.after.push(Box::new(after));
        self
    }
}

/// A node serving from a thread of its own, with its own tokio runtime, so a
/// gpui test can drive an app against it. Reads as its `value` too, so a
/// test's fixture fields are `served.home`. Dropping it stops the node and
/// runs what [`Started::then`] named.
pub struct Served<T> {
    /// `http://<address>`.
    pub url: String,
    /// The node's id.
    pub id: String,
    /// What the setup gave the test.
    pub value: T,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Runs `setup` in a thread of its own, on a runtime of its own, and returns
/// once it has started its node. A setup that panics fails the test with its
/// own message.
pub fn serve<T, Setup, Fut>(setup: Setup) -> Served<T>
where
    T: Send + 'static,
    Setup: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Started<T>>,
{
    let (ready, wait) = std::sync::mpsc::channel();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("a tokio runtime");
        runtime.block_on(async move {
            let started = setup().await;
            let Started { node, value, after } = started;
            let _ = ready.send((node.url(), node.id(), value));
            // The test drops its `Served`, or ends without it.
            let _ = stopped.await;
            node.stop().await;
            for after in after {
                after();
            }
        });
    });
    match wait.recv() {
        Ok((url, id, value)) => Served {
            url,
            id,
            value,
            stop: Some(stop),
            thread: Some(thread),
        },
        Err(_) => match thread.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => panic!("the node's thread ended before its node started"),
        },
    }
}

impl<T> std::ops::Deref for Served<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> Drop for Served<T> {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let joined = thread.join();
            // A test already failing keeps its own message.
            if !std::thread::panicking() {
                joined.expect("the node's thread stopped cleanly");
            }
        }
    }
}

/// Waits until `ok` holds, letting the app run between looks, and fails
/// naming `what` after [`UNTIL`] (scaled). Each look delivers the window a
/// frame, as a platform does, so what animates (a panel sliding) moves on
/// in real time.
pub fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut ok: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + timeout(UNTIL);
    loop {
        cx.update(|window, cx| {
            if window.simulate_next_frame(cx) > 0 {
                window.refresh();
            }
        });
        cx.run_until_parked();
        if ok(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Clicks the middle of the element drawn with `selector` once it is on
/// screen and holds still (a panel may be sliding in).
pub fn click(cx: &mut VisualTestContext, selector: &str) {
    let at = held_still(cx, selector);
    cx.simulate_click(at.center(), Modifiers::none());
}

/// Where the element drawn with `selector` is, once it is on screen and
/// stands in the same place over two frames.
pub fn held_still(cx: &mut VisualTestContext, selector: &str) -> gpui::Bounds<gpui::Pixels> {
    let mut at = None;
    let mut still = 0;
    until(cx, &format!("{selector} on screen, holding still"), |cx| {
        let now = bounds(cx, selector);
        still = match now.is_some() && std::mem::replace(&mut at, now) == now {
            true => still + 1,
            false => 0,
        };
        still >= 2
    });
    at.expect("on screen")
}

/// Where the element drawn with `selector` is, if it is on screen.
pub fn bounds(cx: &mut VisualTestContext, selector: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    // gpui keys its debug selectors by `&'static str`; a test's few leaked
    // names are freed when its process ends.
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}
