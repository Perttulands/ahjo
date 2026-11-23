//! A document put to the operator, read and marked like a paper he is
//! grading, against a desk of the test's own.

mod support;

use support::*;

/// A long document: its every line can be scrolled to, to the last.
#[gpui::test]
fn a_long_document_scrolls_to_its_end(cx: &mut gpui::TestAppContext) {
    let text: String = (1..=200)
        .map(|n| format!("Paragraph {n}: the board shows each gate where the work arrives.\n\n"))
        .collect();
    let desk = FakeDesk::start(vec![doc_ask("AH1", &text)]);
    let dir = scratch("long-doc");
    let (app, cx) = open(&desk, &dir, cx);
    let at = centre(cx, "doc");
    let scroll = app.read_with(cx, |a, _| a.doc_scroll.clone());
    assert!(
        scroll.max_offset().y > gpui::px(1000.),
        "the document runs well past the room: {:?}",
        scroll.max_offset()
    );
    for _ in 0..400 {
        wheel(cx, at, 10.);
        if scroll.offset().y <= -scroll.max_offset().y {
            break;
        }
    }
    assert_eq!(
        scroll.offset().y,
        -scroll.max_offset().y,
        "the wheel reaches the end"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Answered elsewhere stands away from the answer's buttons, and nothing
/// is posted until its undo has run out: taken back, the ask is as it was.
#[gpui::test]
fn answered_elsewhere_waits_out_an_undo(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![
        doc_ask("AH1", "The first document."),
        doc_ask("AH2", "The second document."),
    ]);
    let dir = scratch("elsewhere");
    let (app, cx) = open(&desk, &dir, cx);
    let current = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |a, _| a.current_ask().map(|a| a.id.clone()))
    };
    let first = current(cx).expect("an ask in front");
    click(cx, "elsewhere");
    assert_ne!(
        current(cx).as_ref(),
        Some(&first),
        "the next ask comes forward"
    );
    assert!(
        desk.posts().is_empty(),
        "nothing is posted while it can be undone"
    );
    click(cx, "undo");
    assert_eq!(
        current(cx),
        Some(first.clone()),
        "taken back, the ask is in front again"
    );
    let undo = std::time::Duration::from_millis(ahjo_app_desk::tokens::UNDO_MS);
    cx.executor().advance_clock(undo * 2);
    cx.run_until_parked();
    assert!(
        desk.posts().is_empty(),
        "an answer taken back is never posted"
    );
    click(cx, "elsewhere");
    cx.executor().advance_clock(undo);
    until(cx, "the answer to be posted", |_| !desk.posts().is_empty());
    let posts = desk.posts();
    assert_eq!(
        posts.len(),
        1,
        "the answer taken back was never posted: {posts:?}"
    );
    let (path, body) = &posts[0];
    assert_eq!(path, &format!("/api/asks/{first}/answer"));
    assert_eq!(
        body,
        &serde_json::json!({"round": 1, "verdict": "elsewhere", "options": [], "comment": ""}),
        "the shape the desk has always had"
    );
    let _ = std::fs::remove_dir_all(dir);
}
