//! A gate on the board, in the look chosen (`look::gate`): never a step's
//! card, its decider and its rule readable at a glance.
use super::canvas::{CARD_SMALL, CARD_TEXT, harness_mark};
use super::*;
use crate::look;
use ahjo_missions_contract::{Gate, Mission};
use gpui::{AnyElement, Hsla, SharedString};

const PAD: f32 = 12.;
const GAP: f32 = 8.;
const MARK: f32 = 38.;
const ICON: f32 = 20.;
const TITLE: f32 = 15.;
const ROW_ALPHA: f32 = 0.10;
const CHIP_ALPHA: f32 = 0.16;

/// Who decides a gate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Decider {
    You,
    Judge,
    Checks,
}

pub(super) fn decider(g: &Gate) -> Decider {
    if g.kinds.iter().any(|k| k == "human") {
        Decider::You
    } else if g.kinds.iter().any(|k| k == "formation") {
        Decider::Judge
    } else {
        Decider::Checks
    }
}

fn decider_words(d: Decider) -> &'static str {
    match d {
        Decider::You => "YOU DECIDE",
        Decider::Judge => "A JUDGE DECIDES",
        Decider::Checks => "CHECKS DECIDE",
    }
}

/// The gate's rule in a few words, strongest first: how many tries it
/// allows and what then, from the limit on the step it sends back.
pub(super) fn rule(m: &Mission, gate: &str) -> Option<(String, String)> {
    let l = board::limits_on(m, gate).into_iter().next()?;
    let r = l.rounds?;
    Some(match l.when_spent.as_str() {
        "best" => (format!("BEST OF {r}"), format!("×{r}")),
        "fail" => (format!("{r} TRIES, THEN FAIL"), format!("{r}↺")),
        _ => (format!("{r} TRIES, THEN ASK"), format!("{r}↺")),
    })
}

/// Where an outcome goes: its target's name and whether it goes back.
fn outcome(m: &Mission, gate: &str, port: &str) -> Option<(String, bool)> {
    let from = format!("{gate}:{port}");
    let flow = ahjo_missions_contract::flow::build(m);
    let here = flow.numbers.get(gate).copied();
    m.connections.iter().find(|c| c.from == from).map(|c| {
        let to = c.to.split(':').next().unwrap_or(&c.to);
        let back = matches!((here, flow.numbers.get(to)), (Some(a), Some(b)) if *b <= a);
        (m.node_name(to).to_owned(), back)
    })
}

fn hex(c: Hsla) -> String {
    let c = gpui::Rgba::from(c);
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.).round() as u8,
        (c.g * 255.).round() as u8,
        (c.b * 255.).round() as u8
    )
}

thread_local! {
    static ICONS: std::cell::RefCell<std::collections::HashMap<String, std::sync::Arc<gpui::Image>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn svg(key: String, body: impl FnOnce() -> String) -> std::sync::Arc<gpui::Image> {
    ICONS.with(|icons| {
        icons
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                std::sync::Arc::new(gpui::Image::from_bytes(
                    gpui::ImageFormat::Svg,
                    body().into_bytes(),
                ))
            })
            .clone()
    })
}

/// The decider's mark: a person, a judge's scales, or code.
pub(super) fn icon(d: Decider, colour: Hsla, size: f32) -> AnyElement {
    let path = match d {
        Decider::You => {
            r#"<circle cx="12" cy="7.5" r="3.6"/><path d="M4.5 20.5c0-4.2 3.4-7.4 7.5-7.4s7.5 3.2 7.5 7.4"/>"#
        }
        Decider::Judge => {
            r#"<path d="M12 3.5v17M7 20.5h10M4.5 7h15M6.5 7l-3 6.5c0 1.7 1.3 2.8 3 2.8s3-1.1 3-2.8zM17.5 7l-3 6.5c0 1.7 1.3 2.8 3 2.8s3-1.1 3-2.8z"/>"#
        }
        Decider::Checks => r#"<path d="M8.5 6.5L3 12l5.5 5.5M15.5 6.5L21 12l-5.5 5.5"/>"#,
    };
    let c = hex(colour);
    let image = svg(format!("icon:{path}:{c}"), || {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 24 24" fill="none" stroke="{c}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">{path}</svg>"#
        )
    });
    gpui::img(image)
        .size(px(size))
        .flex_none()
        .into_any_element()
}

/// A diamond, the flowchart's decision.
fn diamond(fill: Hsla, edge: Hsla, size: f32) -> AnyElement {
    let (f, e) = (hex(fill), hex(edge));
    let image = svg(format!("diamond:{f}:{e}"), || {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 60 60"><path d="M30 2.5L57.5 30L30 57.5L2.5 30Z" fill="{f}" stroke="{e}" stroke-width="2.5" stroke-linejoin="round"/></svg>"#
        )
    });
    gpui::img(image).size(px(size)).into_any_element()
}

fn small(text: impl Into<SharedString>, colour: Hsla, z: f32) -> Div {
    div()
        .text_size(px(CARD_SMALL * z))
        .text_color(colour)
        .truncate()
        .child(text.into())
}

fn chip(text: impl Into<SharedString>, colour: Hsla, z: f32) -> Div {
    div()
        .flex_none()
        .px(px(6. * z))
        .py(px(1. * z))
        .rounded(px(4. * z))
        .bg(ahjo_app_kit::theme::alpha(colour, CHIP_ALPHA))
        .border_1()
        .border_color(ahjo_app_kit::theme::alpha(colour, 0.45))
        .text_size(px(CARD_SMALL * z))
        .font_weight(FontWeight::BOLD)
        .text_color(colour)
        .child(text.into())
}

/// The mark's colour: the gate's own, or "needs you" when a person
/// decides, or the live state while a run is shown.
fn mark_colour(
    ink: &look::Ink,
    d: Decider,
    state: Option<crate::store::run_state::NodeState>,
) -> Hsla {
    use crate::store::run_state::NodeState;
    match state {
        Some(NodeState::Done) => ink.done,
        Some(NodeState::Running) => ink.running,
        Some(NodeState::Waiting) => ink.human,
        Some(NodeState::Failed | NodeState::Blocked) => ink.failed,
        _ if d == Decider::You => ink.human,
        _ => ink.gate_mark,
    }
}

pub(super) fn gate_face(
    t: &ahjo_app_kit::theme::Theme,
    m: &Mission,
    card: &board::Card,
    z: f32,
    state: Option<crate::store::run_state::NodeState>,
) -> Div {
    let Some(g) = m.gates.iter().find(|g| g.id == card.id) else {
        return div();
    };
    let ink = look::ink(t);
    let d = decider(g);
    let mark = mark_colour(&ink, d, state);
    let number = ahjo_missions_contract::flow::build(m)
        .numbers
        .get(&g.id)
        .copied();
    let eyebrow = match number {
        Some(n) => format!("{n} · GATE"),
        None => "GATE".to_owned(),
    };
    let rule = rule(m, &g.id);
    let pass = outcome(m, &g.id, ahjo_missions_contract::GATE_PASS);
    let fail = outcome(m, &g.id, ahjo_missions_contract::GATE_FAIL);
    match look::gate() {
        look::Gate::Card => {
            let title = div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::BOLD)
                .text_size(px(TITLE * z))
                .text_color(t.text)
                .child(g.title.clone());
            let head = div()
                .h(px(board::GATE_CARD_HEAD * z))
                .flex_none()
                .px(px(PAD * z))
                .flex()
                .items_center()
                .gap(px(PAD * z))
                .child(
                    div()
                        .size(px(MARK * z))
                        .flex_none()
                        .rounded(px(8. * z))
                        .bg(ahjo_app_kit::theme::alpha(mark, 0.16))
                        .border_1()
                        .border_color(ahjo_app_kit::theme::alpha(mark, 0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(d, mark, ICON * z)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2. * z))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(GAP * z))
                                .child(small(eyebrow, t.dim, z))
                                .child(small(decider_words(d), mark, z)),
                        )
                        .child(title)
                        .child(match &rule {
                            Some((words, _)) => div().flex().child(chip(words.clone(), mark, z)),
                            None => small(g.criterion.clone(), t.text2, z),
                        }),
                );
            let judge = board::folded_judge(m, &g.id).and_then(|id| {
                let f = m.formations.iter().find(|f| f.id == id)?;
                let harness = f
                    .slots
                    .first()
                    .map(|s| s.harness.clone())
                    .unwrap_or_default();
                Some(
                    div()
                        .debug_selector(|| "canvas-gate-judge".into())
                        .h(px(board::GATE_CARD_JUDGE * z))
                        .flex_none()
                        .mx(px(PAD * z))
                        .px(px(GAP * z))
                        .rounded(px(6. * z))
                        .bg(t.surface)
                        .border_1()
                        .border_color(t.divider)
                        .flex()
                        .items_center()
                        .gap(px(GAP * z))
                        .child(small("JUDGE", t.dim, z))
                        .children(harness_mark(&harness, t.text, t.text2, z))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(CARD_TEXT * z))
                                .text_color(t.text)
                                .child(f.title.clone()),
                        ),
                )
            });
            let row = |sign: &str,
                       word: &str,
                       to: Option<(String, bool)>,
                       colour: Hsla,
                       selector: &'static str| {
                let target = match to {
                    Some((name, true)) => format!("↺ back to {name}"),
                    Some((name, false)) => format!("→ {name}"),
                    None => "not wired".to_owned(),
                };
                div()
                    .debug_selector(move || selector.into())
                    .h(px(board::GATE_CARD_ROW * z))
                    .flex_none()
                    .px(px(PAD * z))
                    .flex()
                    .items_center()
                    .gap(px(GAP * z))
                    .bg(ahjo_app_kit::theme::alpha(colour, ROW_ALPHA))
                    .border_t_1()
                    .border_color(ahjo_app_kit::theme::alpha(colour, 0.25))
                    .child(
                        div()
                            .text_size(px(CARD_SMALL * z))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colour)
                            .child(format!("{sign} {word}")),
                    )
                    .child(small(target, t.text2, z).flex_1())
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(head)
                .child(row("✓", "PASS", pass, ink.pass, "canvas-gate-pass-label"))
                .child(row("✕", "FAIL", fail, ink.fail, "canvas-gate-fail-label"))
                .children(judge.map(|j| j.mt(px(6. * z))))
        }
        look::Gate::Diamond => {
            let size = board::DIAMOND * z;
            let top = (board::DIAMOND_H - board::DIAMOND) / 2.;
            let port_mark = |sign: &'static str, colour: Hsla, dy: f32, selector: &'static str| {
                div()
                    .debug_selector(move || selector.into())
                    .absolute()
                    .right(px(10. * z))
                    .top(px((board::DIAMOND_H / 2. + dy - 8.) * z))
                    .text_size(px(CARD_SMALL * z))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colour)
                    .child(sign)
            };
            div()
                .size_full()
                .relative()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(top * z))
                        .size(px(size))
                        .child(diamond(ink.gate_fill_solid(t), mark, size))
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(d, mark, ICON * z)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px((board::DIAMOND + PAD) * z))
                        .right(px(28. * z))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(2. * z))
                        .child(small(format!("{eyebrow} · {}", decider_words(d)), mark, z))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(TITLE * z))
                                .text_color(t.text)
                                .child(g.title.clone()),
                        )
                        .child(match &rule {
                            Some((words, _)) => div().flex().child(chip(words.clone(), mark, z)),
                            None => small(g.criterion.clone(), t.text2, z),
                        }),
                )
                .child(port_mark(
                    "✓",
                    ink.pass,
                    -board::GATE_PORTS,
                    "canvas-gate-pass-label",
                ))
                .child(port_mark(
                    "✕",
                    ink.fail,
                    board::GATE_PORTS,
                    "canvas-gate-fail-label",
                ))
        }
        look::Gate::Badge => {
            let size = board::BADGE * z;
            let cx = board::BADGE_W / 2.;
            let r = board::BADGE / 2.;
            let mid = board::BADGE_MID;
            let stroke = 1.5 * ink.outcome_width;
            let line = |x0: f32, x1: f32, y: f32, colour: Hsla| {
                div()
                    .absolute()
                    .left(px(x0 * z))
                    .w(px((x1 - x0) * z))
                    .top(px((y - stroke / 2.) * z))
                    .h(px(stroke * z))
                    .bg(colour)
            };
            let at = |dy: f32| cx + (r * r - dy * dy).max(0.).sqrt() - 1.;
            let inner: AnyElement = match &rule {
                Some((_, short)) => div()
                    .text_size(px(18. * z))
                    .font_weight(FontWeight::BOLD)
                    .text_color(mark)
                    .child(short.clone())
                    .into_any_element(),
                None => icon(d, mark, 24. * z),
            };
            div()
                .size_full()
                .relative()
                .child(line(0., cx - r, mid, ink.wire))
                .child(line(
                    at(board::GATE_PORTS),
                    board::BADGE_W,
                    mid - board::GATE_PORTS,
                    ink.pass,
                ))
                .child(line(
                    at(board::GATE_PORTS),
                    board::BADGE_W,
                    mid + board::GATE_PORTS,
                    ink.fail,
                ))
                .child(
                    div()
                        .absolute()
                        .left(px((cx - r) * z))
                        .top(px((mid - r) * z))
                        .size(px(size))
                        .rounded_full()
                        .bg(ink.gate_fill_solid(t))
                        .border(px(2.5 * z))
                        .border_color(mark)
                        .shadow_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(inner),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px((mid + r + 6.) * z))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2. * z))
                        .child(
                            div()
                                .w_full()
                                .text_center()
                                .line_clamp(2)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(CARD_TEXT * z))
                                .text_color(t.text)
                                .child(g.title.clone()),
                        )
                        .child(small(
                            match &rule {
                                Some((words, _)) => words.clone(),
                                None => decider_words(d).to_owned(),
                            },
                            mark,
                            z,
                        )),
                )
                .child(
                    div()
                        .debug_selector(|| "canvas-gate-pass-label".into())
                        .absolute()
                        .size_0(),
                )
                .child(
                    div()
                        .debug_selector(|| "canvas-gate-fail-label".into())
                        .absolute()
                        .size_0(),
                )
        }
    }
}
