# vt100 in Ahjo

Based on [vt100 0.18.1](https://github.com/doy/vt100-rust/tree/v0.18.1), MIT
licensed.

- A uniform row in `src/row.rs` holds one blank cell, including its erase
  attributes, until an edit makes its cells differ. This saves allocations for
  blank visible rows. Upstream trims trailing default cells from scrollback;
  the uniform representation also lets those rows be trimmed without
  materializing them.
- `Parser::render_replacement` opts into drawing U+FFFD. Vte and tmux 3.6+
  emit it for malformed UTF-8, and it can also appear literally in output.
  Upstream reports it through the unhandled-character callback.

The engine's `vt100_parity` test compares the row behavior with the unmodified
release.
