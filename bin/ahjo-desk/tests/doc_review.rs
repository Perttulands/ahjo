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
    // The status line says it waits, with its Undo.
    click(cx, "status-action");
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

/// The character offset of `words` in `text`, as the desk counts.
fn at(text: &str, words: &str) -> usize {
    text[..text.find(words).expect("the words are there")]
        .chars()
        .count()
}

/// A tap on a sentence takes the sentence; a note written on it and sent
/// as another round reaches the desk in the web desk's shape: the marks
/// first (a note on characters `start..end`), then `send back`.
#[gpui::test]
fn a_note_on_a_passage_goes_back_as_the_web_sends_it(cx: &mut gpui::TestAppContext) {
    let text = "# Gates\n\nThe gate is fine. It **opens** the next step.\n";
    let desk = FakeDesk::start(vec![doc_ask("AH1", text)]);
    let dir = scratch("note");
    let (app, cx) = open(&desk, &dir, cx);
    let tap = at(text, "opens");
    app.update(cx, |a, cx| {
        a.press_text(tap, cx);
        a.release_text(cx);
    });
    let chosen = app.read_with(cx, |a, _| a.grading.chosen.clone());
    let sentence = at(text, "It **")..at(text, "step.") + 5;
    assert_eq!(chosen, Some(sentence.clone()), "a tap takes its sentence");
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    cx.simulate_input("Say which step");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click(cx, "again");
    until(cx, "the answer to be posted", |_| desk.posts().len() >= 2);
    let posts = desk.posts();
    let (path, body) = &posts[0];
    assert_eq!(path, "/api/asks/AH1/marks");
    let mark = &body["marks"][0];
    assert_eq!(
        (&mark["kind"], &mark["start"], &mark["end"], &mark["text"]),
        (
            &serde_json::json!("note"),
            &serde_json::json!(sentence.start),
            &serde_json::json!(sentence.end),
            &serde_json::json!("Say which step")
        ),
    );
    let (path, body) = &posts[1];
    assert_eq!(path, "/api/asks/AH1/answer");
    assert_eq!(
        body,
        &serde_json::json!({"round": 1, "verdict": "send back", "options": [], "comment": ""})
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A rewrite goes to the desk as the words instead of a passage, and the
/// mark the desk keeps writes them in after that passage on the page.
#[gpui::test]
fn a_rewrite_round_trips(cx: &mut gpui::TestAppContext) {
    let text = "Keep this. The gate is fine as it is. End.\n";
    let desk = FakeDesk::start(vec![doc_ask("AH1", text)]);
    let dir = scratch("rewrite");
    let (app, cx) = open(&desk, &dir, cx);
    let (from, to) = (at(text, "fine"), at(text, " as it is"));
    app.update(cx, |a, cx| {
        a.press_text(from, cx);
        a.drag_text(to - 1, cx);
        a.release_text(cx);
    });
    cx.simulate_keystrokes("r");
    cx.run_until_parked();
    let started = app.read_with(cx, |a, cx| a.pen.read(cx).text.clone());
    assert_eq!(
        started, "fine",
        "a rewrite starts from the words it replaces"
    );
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("one element");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click(cx, "send");
    until(cx, "the marks to be posted", |_| !desk.posts().is_empty());
    let kept = desk.state.lock().unwrap().asks[0]["marks"].clone();
    let marks: Vec<ahjo_app_desk::model::Mark> = serde_json::from_value(kept).unwrap();
    assert_eq!(
        (
            marks[0].kind.as_str(),
            marks[0].start,
            marks[0].end,
            marks[0].text.as_str()
        ),
        ("rewrite", Some(from), Some(to), "one element")
    );
    let paper = ahjo_app_desk::paper::Paper::new(text, None);
    let (shown, _) = paper.shown(&marks, false);
    assert_eq!(
        shown[0].pieces[0].text,
        "Keep this. The gate is fine\u{2038}one element as it is. End."
    );
    let _ = std::fs::remove_dir_all(dir);
}
