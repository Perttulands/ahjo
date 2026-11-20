//! A pane's screen: how big it may be, how much history it keeps, and what
//! `capture-pane` reads from it.

use std::num::NonZeroU16;

/// Scrollback lines kept per pane. With sizes capped at [`MAX_COLS`], a
/// pane's history costs at most 2,000 x 500 cells of about 36 bytes: 36 MB,
/// and about 8.6 MB at 120 columns. It grows only as lines scroll off, so an
/// idle session costs little.
const HISTORY_LINES: usize = 2000;
const MAX_COLS: u16 = 500;
const MAX_ROWS: u16 = 200;

/// A screen of this size, with the pane's history.
pub fn new(cols: NonZeroU16, rows: NonZeroU16) -> vt100::Parser {
    vt100::Parser::new(rows, cols, HISTORY_LINES)
}

/// Clamps a requested size to what the holder allows.
pub fn clamp(cols: u16, rows: u16) -> (NonZeroU16, NonZeroU16) {
    (
        NonZeroU16::new(cols.clamp(1, MAX_COLS)).unwrap(),
        NonZeroU16::new(rows.clamp(1, MAX_ROWS)).unwrap(),
    )
}

/// The pane's text, optionally with its history, its attributes as escape
/// sequences (`-e`) and wrapped lines joined (`-J`).
pub fn capture(screen: &mut vt100::Parser, history: bool, escapes: bool, join: bool) -> String {
    let (rows, cols) = screen.screen().size();
    let line = |screen: &vt100::Screen, n: u16| -> (String, bool) {
        let text = if escapes {
            let formatted = screen
                .rows_formatted(0, cols.get())
                .nth(n.into())
                .unwrap_or_default();
            format!("{formatted}\x1b[m")
        } else {
            screen.rows(0, cols.get()).nth(n.into()).unwrap_or_default()
        };
        (text, screen.row_wrapped(n))
    };
    let mut lines = Vec::new();
    if history {
        screen.screen_mut().set_scrollback(usize::MAX);
        let kept = screen.screen().scrollback();
        for offset in (1..=kept).rev() {
            screen.screen_mut().set_scrollback(offset);
            lines.push(line(screen.screen(), 0));
        }
        screen.screen_mut().set_scrollback(0);
    }
    lines.extend((0..rows.get()).map(|n| line(screen.screen(), n)));
    let mut out = String::new();
    let last = lines.len().saturating_sub(1);
    for (n, (text, wrapped)) in lines.into_iter().enumerate() {
        out.push_str(&text);
        if n < last && !(join && wrapped) {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_clamped() {
        let (cols, rows) = clamp(0, 9999);
        assert_eq!((cols.get(), rows.get()), (1, MAX_ROWS));
        let (cols, rows) = clamp(9999, 10);
        assert_eq!((cols.get(), rows.get()), (MAX_COLS, 10));
    }

    #[test]
    fn captures_history_and_joins_wrapped_lines() {
        let (cols, rows) = clamp(5, 2);
        let mut screen = new(cols, rows);
        screen.process(b"one\r\ntwo\r\nabcdefgh\r\nend");
        assert_eq!(capture(&mut screen, false, false, false), "fgh\nend");
        assert_eq!(
            capture(&mut screen, true, false, false),
            "one\ntwo\nabcde\nfgh\nend"
        );
        assert_eq!(
            capture(&mut screen, true, false, true),
            "one\ntwo\nabcdefgh\nend"
        );
        assert_eq!(screen.screen().scrollback(), 0);
    }

    #[test]
    fn history_is_bounded() {
        let (cols, rows) = clamp(10, 2);
        let mut screen = new(cols, rows);
        for n in 0..HISTORY_LINES + 100 {
            screen.process(format!("{n}\r\n").as_bytes());
        }
        let lines = capture(&mut screen, true, false, false).split('\n').count();
        assert_eq!(lines, HISTORY_LINES + 2);
    }
}
