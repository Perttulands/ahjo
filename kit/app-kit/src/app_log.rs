//! An app's log: gpui's warnings and errors, and every panic with its
//! message, place and backtrace, on stderr and in a file (a Windows program
//! has no console). A panic is written when it happens, before anything
//! unwinds, so one the process cannot survive is in the log too: on Windows
//! a panic in the window procedure cannot unwind out of it and ends the app
//! at once (exception 0xc0000409), and the log is all that says why.

use std::io::Write;
use std::path::{Path, PathBuf};

struct Log {
    app: &'static str,
    path: Option<PathBuf>,
}

impl Log {
    fn write(&self, text: &str) {
        eprintln!("{text}");
        if let Some(path) = &self.path {
            append(path, text);
        }
    }
}

fn append(path: &Path, text: &str) {
    if let Ok(mut file) = std::fs::File::options()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{text}");
    }
}

impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        self.write(&format!(
            "{} {} {} {}: {}",
            now(),
            self.app,
            r.level(),
            r.target(),
            r.args()
        ));
    }

    fn flush(&self) {}
}

/// Installs the log of app `app` (`ahjo-swarm`), kept in `path` (none:
/// stderr only), and its panic hook.
pub fn install(app: &'static str, path: Option<PathBuf>) {
    if let Some(dir) = path.as_deref().and_then(Path::parent) {
        let _ = std::fs::create_dir_all(dir);
    }
    if log::set_boxed_logger(Box::new(Log {
        app,
        path: path.clone(),
    }))
    .is_ok()
    {
        log::set_max_level(log::LevelFilter::Warn);
    }
    // The report replaces the default hook's lines on stderr.
    std::panic::set_hook(Box::new(move |info| {
        let text = report(app, info, &std::backtrace::Backtrace::force_capture());
        eprintln!("{text}");
        if let Some(path) = &path {
            append(path, &text);
        }
    }));
}

/// What a panic leaves in the log: when, which thread, the message, where,
/// and the backtrace.
fn report(
    app: &str,
    info: &std::panic::PanicHookInfo,
    backtrace: &std::backtrace::Backtrace,
) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(a panic without a message)".into());
    let at = info
        .location()
        .map_or_else(|| "an unknown place".into(), |l| l.to_string());
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("unnamed");
    format!(
        "{} {app} PANIC on thread '{thread}' at {at}: {message}\n{backtrace}",
        now()
    )
}

/// The local time to the second, else UTC marked `Z`.
fn now() -> String {
    let utc = time::OffsetDateTime::now_utc();
    let (t, zone) = match time::UtcOffset::current_local_offset() {
        Ok(o) => (utc.to_offset(o), ""),
        Err(_) => (utc, "Z"),
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{zone}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warning_is_written_to_the_log_with_its_time() {
        use log::Log as _;
        let path = std::env::temp_dir().join(format!("ahjo-app-log-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let logger = Log {
            app: "ahjo-swarm",
            path: Some(path.clone()),
        };
        logger.log(
            &log::Record::builder()
                .level(log::Level::Warn)
                .target("gpui")
                .args(format_args!(
                    "failed to show system notification: unavailable"
                ))
                .build(),
        );
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        // `2026-11-21 09:59:22 ahjo-swarm WARN ...`: the date, the clock, the line.
        let mut parts = text.splitn(3, ' ');
        let (date, clock, line) = (parts.next(), parts.next(), parts.next().unwrap());
        assert_eq!(date.unwrap().len(), 10, "{text}");
        assert!(
            clock.unwrap().starts_with(|c: char| c.is_ascii_digit()),
            "{text}"
        );
        assert_eq!(
            line,
            "ahjo-swarm WARN gpui: failed to show system notification: unavailable\n"
        );
    }
}
