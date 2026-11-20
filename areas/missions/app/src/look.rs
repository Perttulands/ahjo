//! The gate's look while it is being decided (proto/board-v3), chosen when
//! the app starts: `AHJO_BOARD_GATE` = `fork` (the line itself splits),
//! `exit` (the question sits on the exit of the step it checks) or `tag`
//! (a small card pointing the way on).
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Fork,
    Exit,
    Tag,
}

pub fn gate() -> Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    *GATE.get_or_init(|| match std::env::var("AHJO_BOARD_GATE").as_deref() {
        Ok("exit") => Gate::Exit,
        Ok("tag") => Gate::Tag,
        _ => Gate::Fork,
    })
}
