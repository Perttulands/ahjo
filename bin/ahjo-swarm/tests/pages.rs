//! The app's real list of pages mounts together: no two share an id, a
//! frame kind, an `everywhere` chord or a setting key, and none takes a kind
//! the link or the shell owns. The hub never dials a node.

use ahjo_app_kit::Hub;
use ahjo_app_kit::link::socket::Net;
use ahjo_app_shell::shell::mount;
use gpui::{AppContext, TestAppContext};

#[gpui::test]
fn the_listed_pages_mount_together(cx: &mut TestAppContext) {
    cx.update(ahjo_app_shell::prepare);
    let cx = cx.add_empty_window();
    let (tx, _rx) = async_channel::unbounded();
    let mounted = cx.update(|window, cx| {
        let hub = cx.new(|_| Hub::new(Net::start(), tx, None));
        mount(ahjo_app_swarm::PAGES, &hub, window, cx).map(|pages| (pages.len(), pages[0].meta.id))
    });
    // Each listed page, and the shell's Settings page.
    assert_eq!(
        mounted.unwrap(),
        (ahjo_app_swarm::PAGES.len() + 1, "sessions")
    );
}
