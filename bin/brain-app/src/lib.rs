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
use parvi_app_kit::theme::FONT_FILES;
use std::borrow::Cow;

const USAGE: &str = "brain-app [--brain <folder>] [--citadel <url>] [--take sky|gather|orbit] [--search <words>] [--open <page>] [--bench <seconds>] [--where]";

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
    if args.iter().any(|a| a == "--where") {
        std::process::exit(tell_where(&source::find(&args)));
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
        let fonts = FONT_FILES.iter().map(|f| Cow::Borrowed(*f)).collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            panic!("the bundled fonts did not load: {e}");
        }
        let (theme, notice) = parvi_app_kit::theme::chosen();
        if let Some(notice) = notice {
            eprintln!("brain-app: {notice}");
        }
        cx.set_global(theme);
        parvi_app_kit::widgets::field::bind_keys(cx);
        app::open_window(start, cx);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}

/// `--where`: where this launch finds the brain, its search and its cache,
/// and how long reading the brain takes from here, without a window.
fn tell_where(place: &source::Where) -> i32 {
    println!("search: {}", place.citadel);
    match &place.cache {
        Some(c) => println!("cache:  {}", c.display()),
        None => println!("cache:  none"),
    }
    let Some(root) = &place.brain else {
        println!("brain:  not found; give --brain <folder> or BRAIN_ROOT");
        return 1;
    };
    println!("brain:  {}", root.display());
    let kept = place
        .cache
        .as_ref()
        .map(|c| source::load(c, root))
        .unwrap_or_default();
    let started = std::time::Instant::now();
    match corpus::scan(root, &kept.entries) {
        Ok(entries) => {
            println!(
                "read:   {} pages in {:.1} s ({} cached before)",
                entries.len(),
                started.elapsed().as_secs_f32(),
                kept.entries.len()
            );
            0
        }
        Err(e) => {
            println!("read:   failed: {e}");
            1
        }
    }
}
