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

/// Swarm had a Brain tab; Ahjo Brain is its own app now. A settings file
/// the tab wrote to still opens, keeps what it holds, and shows no Brain.
#[gpui::test]
fn a_settings_file_from_the_brain_tab_still_opens(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("ahjo-swarm-brain-tab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.json");
    std::fs::write(&path, r#"{"theme": "nord", "brain": {"read": 18}}"#).unwrap();
    cx.update(ahjo_app_shell::prepare);
    let cx = cx.add_empty_window();
    let (tx, _rx) = async_channel::unbounded();
    let settings = path.clone();
    let (ids, theme, brain) = cx.update(|window, cx| {
        let hub = cx.new(|_| Hub::new(Net::start(), tx, Some(settings)));
        let pages = mount(ahjo_app_swarm::PAGES, &hub, window, cx).unwrap();
        let settings = &hub.read(cx).settings;
        (
            pages.iter().map(|p| p.meta.id).collect::<Vec<_>>(),
            settings.theme.clone(),
            settings.sections.get("brain").cloned(),
        )
    });
    assert_eq!(theme, "nord", "the file was read");
    assert_eq!(brain, Some(serde_json::json!({"read": 18})), "and kept");
    assert!(!path.with_extension("json.broken").exists());
    assert!(!ids.contains(&"brain"), "there is no Brain page: {ids:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}
