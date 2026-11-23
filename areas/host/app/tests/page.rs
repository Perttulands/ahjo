//! Both host pages render data from a testkit node.
use ahjo_app_kit::link::client::LinkState;
use ahjo_testkit::app::{Served, Started, serve, until};
use ahjo_testkit::node::Node;
use gpui::TestAppContext;

fn start() -> Served<()> {
    serve(move || async move {
        let node = Node::start(&[ahjo_host::LISTING], "").await.unwrap();
        // A read-only fixture with an invalid user is never sent to a real manager.
        std::fs::write(
            node.dir().join("services.json"),
            serde_json::json!({"services": [
                {"id":"fixture", "name":"Fixture service", "readOnly":true,
                 "components":[{"kind":"systemd-user", "user":"\u{0}", "unit":"fixture.service"}]}
            ]})
            .to_string(),
        )
        .unwrap();
        Started::new(node, ())
    })
}
#[gpui::test]
fn readings_and_services_render_node_data(cx: &mut TestAppContext) {
    let running = start();
    let dir = ahjo_testkit::fresh_dir("ahjo-host-page");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, settings) = (running.url.clone(), dir.join("app.json"));
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_host_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    until(cx, "the node link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    until(cx, "a host reading", |cx| {
        cx.debug_bounds("host-chart").is_some()
    });
    let row = Box::leak(format!("svc-row-{}-fixture", running.id).into_boxed_str());
    until(cx, "the fixture service", |cx| {
        cx.debug_bounds(row).is_some()
    });
    assert!(cx.debug_bounds("services-refresh").is_some());
    let at = cx.debug_bounds(row).unwrap().center();
    cx.simulate_click(at, gpui::Modifiers::none());
    until(cx, "the service, chosen", |cx| {
        cx.debug_bounds("pane-title-Fixture service").is_some()
    });
    for verb in ["start", "stop", "restart"] {
        for control in [
            format!("svc-{verb}-{}-fixture", running.id),
            format!("svc-row-{verb}-{}-fixture", running.id),
        ] {
            let control = Box::leak(control.into_boxed_str());
            assert!(cx.debug_bounds(control).is_none(), "read-only: no {verb}");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
