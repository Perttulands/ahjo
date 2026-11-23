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
