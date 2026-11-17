//! The Brain: the company brain as a night sky, a search that moves you
//! through it and a reader beside it, in its own window.
//!
//! The pages and links come straight from the brain's Markdown files (they
//! are the brain); ranked search comes from Context Citadel.

mod app;
mod arrange;
mod camera;
mod citadel;
mod corpus;
mod paint;
mod reader;
mod sky;
mod source;
mod tokens;

use app::{Bench, Start, Take};
use gpui::App;
use parvi_app_kit::theme::{FAMILY_FILES, Theme};
use std::borrow::Cow;

const USAGE: &str = "brain-app [--brain <folder>] [--citadel <url>] [--take sky|gather|orbit] [--search <words>] [--open <page>] [--bench <seconds>]";

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    let take = match value(&args, "--take") {
        Some(name) => match Take::parse(&name) {
            Some(t) => t,
            None => {
                eprintln!("no take called {name}: {USAGE}");
                std::process::exit(2);
            }
        },
        None => Take::Sky,
    };
    let bench = value(&args, "--bench").map(|s| match s.parse::<f32>() {
        Ok(s) => Bench::new(s),
        Err(_) => {
            eprintln!("--bench takes seconds: {USAGE}");
            std::process::exit(2);
        }
    });
    let start = Start {
        place: source::find(&args),
        take,
        query: value(&args, "--search"),
        open: value(&args, "--open"),
        bench,
    };
    gpui_platform::application().run(move |cx: &mut App| {
        let fonts = FAMILY_FILES.iter().map(|f| Cow::Borrowed(*f)).collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            panic!("the bundled fonts did not load: {e}");
        }
        cx.set_global(Theme::violet());
        parvi_app_kit::widgets::field::bind_keys(cx);
        app::open_window(start, cx);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
