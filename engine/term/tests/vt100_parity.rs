//! The sparse-row fork must retain vt100 0.18.1's rendering semantics.

use std::num::NonZeroU16;

fn dimension(value: u16) -> NonZeroU16 {
    value.try_into().unwrap()
}

#[test]
fn deleting_a_full_colored_row_uses_default_background() {
    let mut parser = vt100::Parser::new(dimension(2), dimension(8), 0);
    parser.process(b"\x1b[41m\x1b[2J\x1b[HX\x1b[H\x1b[8P");
    for col in 0..8 {
        assert_eq!(
            parser.screen().cell(0, col).unwrap().bgcolor(),
            vt100::Color::Default
        );
    }
}

#[test]
fn sparse_rows_match_upstream() {
    let mut actual = vt100::Parser::new(dimension(4), dimension(8), 0);
    let mut upstream = vt100_upstream::Parser::new(dimension(4), dimension(8), 0);
    let sequences: &[&[u8]] = &[
        b"\x1b[41m\x1b[2J",
        b"\x1b[HX\x1b[H\x1b[8P",
        b"\x1b[0mabc\x1b[2@Z",
        b"\x1b[1;2H\x1b[3P",
        b"\x1b[42m\x1b[2X",
        b"\x1b[3;2H\x1b[1K",
        b"\x1b[4;1Hone\r\ntwo\r\nthree",
        b"\x1b[2;4H\x1b[1L\x1b[1M",
        b"\x1b[?1049h\x1b[44m\x1b[2Jalt\x1b[?1049l",
        b"\x1b[1;1H\x1b[2J",
    ];
    for (index, sequence) in sequences.iter().enumerate() {
        actual.process(sequence);
        upstream.process(sequence);
        assert_screens(&actual, &upstream, index);
        if index == 4 {
            actual.set_size(dimension(5), dimension(11));
            upstream.set_size(dimension(5), dimension(11));
            assert_screens(&actual, &upstream, index);
        }
        if index == 7 {
            actual.set_size(dimension(3), dimension(6));
            upstream.set_size(dimension(3), dimension(6));
            assert_screens(&actual, &upstream, index);
        }
    }
}

#[test]
fn compact_rows_match_upstream_through_scrollback_and_resize() {
    let (rows, cols) = (dimension(4), dimension(8));
    let mut actual = vt100::Parser::new(rows, cols, 6);
    let mut upstream = vt100_upstream::Parser::new(rows, cols, 6);
    // Push a default blank row, a colored blank row and a row with a
    // wide character into history. The visible grid also has blank rows.
    let output = "\x1b[2;1H\x1b[41m\x1b[2K\x1b[0m\r\n界\r\n\r\n\r\n\r\n";
    actual.process(output.as_bytes());
    upstream.process(output.as_bytes());
    assert_eq!(actual.screen().scrollback_len(), 3);
    for offset in [3, 0] {
        actual.screen_mut().set_scrollback(offset);
        upstream.screen_mut().set_scrollback(offset);
        assert_screens(&actual, &upstream, offset);
    }

    actual.set_size(dimension(7), dimension(10));
    upstream.set_size(dimension(7), dimension(10));
    assert_screens(&actual, &upstream, 4);

    let actual_before = actual.screen().clone();
    let upstream_before = upstream.screen().clone();
    let output = b"\x1b[1;1Hblank\x1b[2;2H\x1b[2P\x1b[3;1H\x1b[2@";
    actual.process(output);
    upstream.process(output);
    assert_screens(&actual, &upstream, 5);
    assert_eq!(
        actual.screen().contents_diff(&actual_before),
        upstream.screen().contents_diff(&upstream_before)
    );
}

fn assert_screens(actual: &vt100::Parser, upstream: &vt100_upstream::Parser, step: usize) {
    let a = actual.screen();
    let b = upstream.screen();
    assert_eq!(
        a.contents_formatted(),
        b.contents_formatted(),
        "step {step}"
    );
    assert_eq!(a.cursor_position(), b.cursor_position(), "step {step}");
    assert_eq!(a.size(), b.size());
    let (rows, cols) = a.size();
    for row in 0..rows.get() {
        for col in 0..cols.get() {
            let x = a.cell(row, col).unwrap();
            let y = b.cell(row, col).unwrap();
            assert_eq!(x.contents(), y.contents(), "step {step}, {row},{col}");
            assert_eq!(
                format!("{:?}", x.fgcolor()),
                format!("{:?}", y.fgcolor()),
                "step {step}, {row},{col}"
            );
            assert_eq!(
                format!("{:?}", x.bgcolor()),
                format!("{:?}", y.bgcolor()),
                "step {step}, {row},{col}"
            );
            assert_eq!(
                (
                    x.bold(),
                    x.dim(),
                    x.italic(),
                    x.underline(),
                    x.inverse(),
                    x.is_wide(),
                    x.is_wide_continuation()
                ),
                (
                    y.bold(),
                    y.dim(),
                    y.italic(),
                    y.underline(),
                    y.inverse(),
                    y.is_wide(),
                    y.is_wide_continuation()
                ),
                "step {step}, {row},{col}"
            );
        }
    }
}
