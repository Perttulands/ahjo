//! A grilling ask on a later round, against a desk of the test's own: where
//! the conversation starts, and that it follows what he writes.

mod support;

use support::*;

/// An ask on its fourth round opens at that round, not at the first: the
/// earlier rounds stand above it, out of sight.
#[gpui::test]
fn a_later_round_opens_at_its_own_top(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![grill_ask("AH1", 4)]);
    let dir = scratch("later-round");
    let (app, cx) = open(&desk, &dir, cx);
    cx.run_until_parked();
    let turns = app.read_with(cx, |a, _| a.turns.clone());
    let round = turns.bounds_for_item(3).expect("the fourth round is drawn");
    assert!(
        round.top() > turns.bounds().bottom(),
        "three rounds fill more than the column: {round:?}"
    );
    // As far as the column scrolls: a round shorter than the column leaves
    // the end of the round before it in sight above.
    let to_top = (round.top() - turns.bounds().top()).min(turns.max_offset().y);
    assert_eq!(
        -turns.offset().y,
        to_top,
        "the round in front stands at the column's top"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// His words on the last question grow line by line at the column's foot:
/// the column follows, so the line he is writing stays in it.
#[gpui::test]
fn the_column_follows_his_words_on_the_last_question(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![grill_ask("AH1", 4)]);
    let dir = scratch("follow-words");
    let (app, cx) = open(&desk, &dir, cx);
    let turns = app.read_with(cx, |a, _| a.turns.clone());
    let at = centre(cx, "turns");
    for _ in 0..400 {
        wheel(cx, at, 10.);
        if turns.offset().y <= -turns.max_offset().y {
            break;
        }
    }
    click(cx, "words-2");
    let tall = |cx: &mut gpui::VisualTestContext| {
        cx.run_until_parked();
        app.read_with(cx, |a, cx| a.words.read(cx).bounds())
            .expect("his words are drawn")
            .size
            .height
    };
    let one_line = tall(cx);
    for _ in 0..12 {
        cx.simulate_input("the box must stay where he can see what he writes ");
    }
    assert!(tall(cx) > one_line * 3., "his words ran onto more lines");
    let last = app
        .read_with(cx, |a, cx| a.words.read(cx).caret())
        .expect("a caret");
    assert!(
        last.top() >= turns.bounds().top() && last.bottom() <= turns.bounds().bottom(),
        "the line he is writing is in the column: {last:?} in {:?}",
        turns.bounds()
    );
    let _ = std::fs::remove_dir_all(dir);
}
