//! The editor's model, CHROTE's (`scheduledSchedule.ts`): the API speaks
//! interval and cron, the editor Every, Daily, Weekly and Cron; and the plain
//! words a task is described in.

use ahjo_scheduled_contract::{
    CreateRequest, PatchRequest, Schedule, Target, TargetRequest, Task, schedule,
};
use jiff::Timestamp;

use crate::ACTOR;

/// An interval's length in minutes, from `every_minutes` or `duration`, as
/// the node reads it.
pub fn interval_minutes(s: &Schedule) -> Option<f64> {
    schedule::interval(s).ok().map(|d| d.as_secs_f64() / 60.)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Every,
    Daily,
    Weekly,
    Cron,
}

pub const MODES: [(Mode, &str); 4] = [
    (Mode::Every, "Every"),
    (Mode::Daily, "Daily"),
    (Mode::Weekly, "Weekly"),
    (Mode::Cron, "Cron"),
];

/// Monday first, Sunday last: (cron value, short, long).
pub const WEEKDAYS: [(u8, &str, &str); 7] = [
    (1, "Mon", "Monday"),
    (2, "Tue", "Tuesday"),
    (3, "Wed", "Wednesday"),
    (4, "Thu", "Thursday"),
    (5, "Fri", "Friday"),
    (6, "Sat", "Saturday"),
    (0, "Sun", "Sunday"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleForm {
    pub mode: Mode,
    pub every_value: String,
    /// Hours rather than minutes.
    pub every_hours: bool,
    pub time: String,
    pub weekdays: Vec<u8>,
    pub cron: String,
    pub timezone: String,
}

/// This computer's IANA zone, so nobody types one.
pub fn local_zone() -> String {
    #[cfg(windows)]
    if let Some(zone) = windows_zone() {
        return zone;
    }
    jiff::tz::TimeZone::system()
        .iana_name()
        .map(str::to_string)
        .unwrap_or_else(|| "UTC".into())
}

/// The IANA zone for Windows' zone key in the user's home region (#222).
/// One Windows zone covers several places ("FLE Standard Time" is Helsinki,
/// Kyiv, Riga, Sofia, Tallinn and Vilnius); jiff picks the world default,
/// so a Finnish machine would read Europe/Kiev. Windows' own ICU resolves
/// the key with the region the user set as home. `None` when any step
/// fails or the answer is a zone jiff does not know, and jiff's choice
/// stands.
#[cfg(windows)]
fn windows_zone() -> Option<String> {
    use windows_sys::Win32::Globalization::{
        GetUserDefaultGeoName, ucal_getTimeZoneIDForWindowsID,
    };
    use windows_sys::Win32::System::Time::{
        DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation,
    };
    // SAFETY: each call writes at most the capacity it is given into a
    // buffer owned here, and the strings passed in are NUL-terminated.
    unsafe {
        let mut info: DYNAMIC_TIME_ZONE_INFORMATION = std::mem::zeroed();
        if GetDynamicTimeZoneInformation(&mut info) == u32::MAX {
            return None;
        }
        let key = &info.TimeZoneKeyName;
        let key_len = key.iter().position(|&c| c == 0).unwrap_or(key.len());
        let mut geo = [0u16; 16];
        let geo_len = GetUserDefaultGeoName(geo.as_mut_ptr(), geo.len() as i32);
        let mut region: Vec<u8> = match geo_len {
            n if n > 1 => String::from_utf16(&geo[..n as usize - 1])
                .ok()?
                .into_bytes(),
            _ => b"001".to_vec(),
        };
        region.push(0);
        let mut id = [0u16; 64];
        let mut status = 0;
        let n = ucal_getTimeZoneIDForWindowsID(
            key.as_ptr(),
            key_len as i32,
            region.as_ptr(),
            id.as_mut_ptr(),
            id.len() as i32,
            &mut status,
        );
        if status > 0 || n <= 0 || n as usize > id.len() {
            return None;
        }
        let zone = String::from_utf16(&id[..n as usize]).ok()?;
        jiff::tz::TimeZone::get(&zone).ok().map(|_| zone)
    }
}

impl Default for ScheduleForm {
    fn default() -> Self {
        ScheduleForm {
            mode: Mode::Daily,
            every_value: "30".into(),
            every_hours: false,
            time: "09:00".into(),
            weekdays: vec![1, 2, 3, 4, 5],
            cron: "0 9 * * *".into(),
            timezone: local_zone(),
        }
    }
}

fn parse_time(time: &str) -> Option<(u32, u32)> {
    let (h, m) = time.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h <= 23 && m <= 59).then_some((h, m))
}

fn sort_days(days: &[u8]) -> Vec<u8> {
    let order = |d: &u8| WEEKDAYS.iter().position(|w| w.0 == *d).unwrap_or(9);
    let mut out: Vec<u8> = days.iter().map(|d| if *d == 7 { 0 } else { *d }).collect();
    out.sort_by_key(order);
    out.dedup();
    out
}

/// `M H * * *` and `M H * * days`, CHROTE's daily and weekly shapes.
fn daily_or_weekly(expression: &str) -> Option<(u32, u32, Option<Vec<u8>>)> {
    let f: Vec<&str> = expression.split(' ').collect();
    if f.len() != 5 || f[2] != "*" || f[3] != "*" {
        return None;
    }
    let num = |s: &str| (1..=2).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !num(f[0]) || !num(f[1]) {
        return None;
    }
    let (m, h) = (f[0].parse().ok()?, f[1].parse().ok()?);
    if f[4] == "*" {
        return Some((h, m, None));
    }
    // Days as a list (`1,3`) or ranges (`1-5`), or both.
    let mut days = vec![];
    for d in f[4].split(',') {
        match d.as_bytes() {
            [c @ b'0'..=b'7'] => days.push(c - b'0'),
            [a @ b'0'..=b'7', b'-', b @ b'0'..=b'7'] if a <= b => {
                days.extend((a - b'0')..=(b - b'0'))
            }
            _ => return None,
        }
    }
    Some((h, m, Some(days)))
}

impl ScheduleForm {
    pub fn error(&self) -> Option<&'static str> {
        if self.timezone.trim().is_empty() {
            return Some("Timezone is required.");
        }
        match self.mode {
            Mode::Every => match self.every_value.trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v > 0. => {
                    (self.every_hours && v > 24. * 30.).then_some("Interval is too long.")
                }
                _ => Some("Interval must be a positive number."),
            },
            Mode::Daily => parse_time(&self.time)
                .is_none()
                .then_some("Time must be HH:MM."),
            Mode::Weekly if parse_time(&self.time).is_none() => Some("Time must be HH:MM."),
            Mode::Weekly => self.weekdays.is_empty().then_some("Pick at least one day."),
            Mode::Cron => (self.cron.split_whitespace().count() != 5)
                .then_some("Cron needs five fields: minute hour day month weekday."),
        }
    }

    pub fn schedule(&self) -> Schedule {
        let timezone = match self.timezone.trim() {
            "" => "UTC".to_string(),
            z => z.to_string(),
        };
        match self.mode {
            Mode::Every => {
                let v: f64 = self.every_value.trim().parse().unwrap_or(0.);
                let minutes = if self.every_hours { v * 60. } else { v };
                Schedule {
                    kind: "interval".into(),
                    every_minutes: minutes.round() as i64,
                    timezone,
                    ..Schedule::default()
                }
            }
            Mode::Cron => Schedule {
                kind: "cron".into(),
                expression: self.cron.trim().into(),
                timezone,
                ..Schedule::default()
            },
            Mode::Daily | Mode::Weekly => {
                let (h, m) = parse_time(&self.time).unwrap_or((9, 0));
                let days = if self.mode == Mode::Weekly {
                    sort_days(&self.weekdays)
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                } else {
                    "*".into()
                };
                Schedule {
                    kind: "cron".into(),
                    expression: format!("{m} {h} * * {days}"),
                    timezone,
                    ..Schedule::default()
                }
            }
        }
    }

    pub fn of(s: &Schedule) -> ScheduleForm {
        let base = ScheduleForm::default();
        let timezone = match s.timezone.trim() {
            "" => base.timezone.clone(),
            z => z.to_string(),
        };
        if s.kind == "interval" {
            // A duration that is not whole minutes shows rounded; it is
            // kept as it was unless When is changed (`TaskForm::opened`).
            let minutes = interval_minutes(s).map_or(30, |m| (m.round() as i64).max(1));
            let hours = minutes >= 60 && minutes % 60 == 0;
            return ScheduleForm {
                mode: Mode::Every,
                every_value: (if hours { minutes / 60 } else { minutes }).to_string(),
                every_hours: hours,
                timezone,
                ..base
            };
        }
        let expression = s.expression.trim();
        match daily_or_weekly(expression) {
            Some((h, m, None)) => ScheduleForm {
                mode: Mode::Daily,
                time: format!("{h:02}:{m:02}"),
                cron: expression.into(),
                timezone,
                ..base
            },
            Some((h, m, Some(days))) => ScheduleForm {
                mode: Mode::Weekly,
                time: format!("{h:02}:{m:02}"),
                weekdays: sort_days(&days),
                cron: expression.into(),
                timezone,
                ..base
            },
            None => ScheduleForm {
                mode: Mode::Cron,
                cron: if expression.is_empty() {
                    base.cron.clone()
                } else {
                    expression.into()
                },
                timezone,
                ..base
            },
        }
    }
}

/// The plain words a task card shows, CHROTE's `describeSchedule`.
pub fn describe(s: &Schedule) -> String {
    if s.kind == "interval" {
        let Some(exact) = interval_minutes(s) else {
            return "No schedule".into();
        };
        if exact.fract() != 0. {
            return format!("Every {}", s.duration.trim());
        }
        let m = exact as i64;
        if m % 60 == 0 && m >= 60 {
            let h = m / 60;
            return if h == 1 {
                "Every hour".into()
            } else {
                format!("Every {h} hours")
            };
        }
        return if m == 1 {
            "Every minute".into()
        } else {
            format!("Every {m} minutes")
        };
    }
    let expression = s.expression.trim();
    match daily_or_weekly(expression) {
        Some((h, m, None)) => format!("Daily at {h:02}:{m:02}"),
        Some((h, m, Some(days))) => {
            let days = sort_days(&days);
            let weekdays = days.len() == 5 && (1..=5).all(|d| days.contains(&d));
            let when = if weekdays {
                "Weekdays".to_string()
            } else {
                days.iter()
                    .map(|d| WEEKDAYS.iter().find(|w| w.0 == *d).map_or("?", |w| w.1))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!("{when} at {h:02}:{m:02}")
        }
        None if expression.is_empty() => "No schedule".into(),
        None => cron_words(expression).unwrap_or_else(|| format!("Cron {expression}")),
    }
}

/// The cron shapes people write that read as a sentence: every N minutes
/// or hours, at a minute past each hour, a day of each month. `None` for
/// the rest, which keep their cron text.
fn cron_words(expression: &str) -> Option<String> {
    let f: Vec<&str> = expression.split_whitespace().collect();
    let [minute, hour, day, month, weekday] = f[..] else {
        return None;
    };
    if month != "*" {
        return None;
    }
    let num = |s: &str, most: u32| s.parse::<u32>().ok().filter(|n| *n <= most);
    let step = |s: &str| match s {
        "*" => Some(1),
        s => s.strip_prefix("*/")?.parse::<u32>().ok().filter(|n| *n > 0),
    };
    let every = |n: u32, one: &str, many: &str| match n {
        1 => format!("Every {one}"),
        n => format!("Every {n} {many}"),
    };
    let when = match (step(minute), num(minute, 59), step(hour), num(hour, 23)) {
        // `*/15 * …`: every so many minutes.
        (Some(n), _, Some(1), _) => every(n, "minute", "minutes"),
        // `0 */3 …`, `30 * …`: every so many hours, at a minute past.
        (_, Some(m), Some(n), _) => match m {
            0 => every(n, "hour", "hours"),
            m => format!("{} at :{m:02}", every(n, "hour", "hours")),
        },
        // `30 9 1 * *`: a day of each month.
        (_, Some(m), _, Some(h)) if weekday == "*" => {
            let d = num(day, 31).filter(|d| *d > 0)?;
            return Some(format!("Day {d} of each month at {h:02}:{m:02}"));
        }
        _ => return None,
    };
    if day != "*" {
        return None;
    }
    match weekday {
        "*" => Some(when),
        "1-5" => Some(format!("{when} on weekdays")),
        "0,6" | "6,0" => Some(format!("{when} at weekends")),
        _ => None,
    }
}

/// `in 2h 5m`, `due now`: liveness as words.
pub fn countdown(at: Option<Timestamp>, now: Timestamp) -> String {
    let Some(at) = at else {
        return String::new();
    };
    let seconds = (at.as_millisecond() - now.as_millisecond() + 500).div_euclid(1000);
    if seconds <= 0 {
        return "due now".into();
    }
    let (d, h, m) = (
        seconds / 86_400,
        seconds % 86_400 / 3_600,
        seconds % 3_600 / 60,
    );
    if d > 0 {
        format!("in {d}d {h}h")
    } else if h > 0 {
        format!("in {h}h {m}m")
    } else if m > 0 {
        format!("in {m}m")
    } else {
        "in <1m".into()
    }
}

/// `2h ago`, `just now`: how long since a run.
pub fn ago(at: Timestamp, now: Timestamp) -> String {
    let seconds = (now.as_millisecond() - at.as_millisecond()).div_euclid(1000);
    let (d, h, m) = (seconds / 86_400, seconds / 3_600, seconds / 60);
    if d > 0 {
        format!("{d}d ago")
    } else if h > 0 {
        format!("{h}h ago")
    } else if m > 0 {
        format!("{m}m ago")
    } else {
        "just now".into()
    }
}

/// `09:00` today, `Nov 9, 09:00` any other day, in this computer's zone.
pub fn when(at: Option<Timestamp>) -> String {
    let zone = jiff::tz::TimeZone::system();
    when_on(at, Timestamp::now().to_zoned(zone.clone()).date(), zone)
}

/// [`when`] with today and the zone given.
pub fn when_on(
    at: Option<Timestamp>,
    today: jiff::civil::Date,
    zone: jiff::tz::TimeZone,
) -> String {
    let Some(at) = at else {
        return "—".into();
    };
    let at = at.to_zoned(zone);
    let format = if at.date() == today {
        "%H:%M"
    } else {
        "%b %-d, %H:%M"
    };
    at.strftime(format).to_string()
}

/// How a run went, in words: `ok`, or `failed` and why. The node's reason
/// leads with what kind of failure it was, which the page has just said.
pub fn result(status: &str, message: &str) -> String {
    if status == ahjo_scheduled_contract::SUCCESS {
        return "ok".into();
    }
    if status.is_empty() {
        return "unknown".into();
    }
    let why = message.replace("scheduled task target not found: ", "");
    let said = if status == "error" { "failed" } else { status };
    if why.is_empty() {
        said.to_string()
    } else {
        format!("{said}: {why}")
    }
}

/// The editor: a new task or one being edited, on the node it lives on.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskForm {
    pub node: String,
    /// `None` for a new task.
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub targets: Vec<Target>,
    pub schedule: ScheduleForm,
    /// Kept as the task had it: the form has no switch for it, and saving
    /// an edit never turns a disabled task back on.
    pub enabled: bool,
    pub paused: bool,
    /// The schedule the task had and the form it opened as: saved as it
    /// was unless the form's When was changed, so a cadence the form cannot
    /// say (a `duration`, a zone left to the node) is never rewritten.
    pub opened: Option<(ScheduleForm, Schedule)>,
}

impl TaskForm {
    pub fn new(node: &str, targets: Vec<Target>) -> TaskForm {
        TaskForm {
            node: node.into(),
            id: None,
            name: String::new(),
            prompt: String::new(),
            targets,
            schedule: ScheduleForm::default(),
            enabled: true,
            paused: false,
            opened: None,
        }
    }

    pub fn edit(node: &str, t: &Task) -> TaskForm {
        TaskForm {
            node: node.into(),
            id: Some(t.id.clone()),
            name: t.name.clone(),
            prompt: t.prompt.clone(),
            targets: t.targets.clone(),
            schedule: ScheduleForm::of(&t.schedule),
            enabled: t.enabled,
            paused: t.paused,
            opened: Some((ScheduleForm::of(&t.schedule), t.schedule.clone())),
        }
    }

    pub fn duplicate(node: &str, t: &Task) -> TaskForm {
        TaskForm {
            id: None,
            name: format!("{} copy", t.name),
            ..TaskForm::edit(node, t)
        }
    }

    pub fn toggle(&mut self, target: Target) {
        match self.targets.iter().position(|t| *t == target) {
            Some(i) => {
                self.targets.remove(i);
            }
            None => self.targets.push(target),
        }
    }

    /// What is wrong before saving, CHROTE's order of asking.
    pub fn error(&self) -> Option<&'static str> {
        if self.prompt.trim().is_empty() {
            return Some("Prompt is required.");
        }
        if self.targets.is_empty() {
            return Some("Add at least one target session.");
        }
        self.schedule.error()
    }

    /// The schedule as opened if When was not changed, else the form's.
    pub fn schedule_to_save(&self) -> Schedule {
        match &self.opened {
            Some((form, schedule)) if *form == self.schedule => schedule.clone(),
            _ => self.schedule.schedule(),
        }
    }

    /// The create or patch body; the name defaults to the prompt's first
    /// line.
    pub fn body(&self) -> serde_json::Value {
        let name = match self.name.trim() {
            "" => self
                .prompt
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect(),
            n => n.to_string(),
        };
        let targets = self
            .targets
            .iter()
            .map(|target| TargetRequest {
                session_name: target.session_name.clone(),
                unix_user: target.unix_user.clone(),
                ..Default::default()
            })
            .collect();
        let body = if self.id.is_some() {
            serde_json::to_value(PatchRequest {
                name: Some(name),
                prompt: Some(self.prompt.clone()),
                targets: Some(targets),
                schedule: Some(self.schedule_to_save()),
                enabled: Some(self.enabled),
                paused: Some(self.paused),
                updated_by: ACTOR.into(),
                ..Default::default()
            })
        } else {
            serde_json::to_value(CreateRequest {
                name,
                prompt: self.prompt.clone(),
                targets,
                schedule: self.schedule_to_save(),
                enabled: Some(self.enabled),
                paused: self.paused,
                created_by: ACTOR.into(),
                updated_by: ACTOR.into(),
                ..Default::default()
            })
        };
        body.expect("scheduled form fields are JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_today_goes_without_its_date() {
        let utc = jiff::tz::TimeZone::UTC;
        let at: Timestamp = "2026-11-18T07:27:00Z".parse().unwrap();
        let day = |d| jiff::civil::date(2026, 11, d);
        assert_eq!(when_on(Some(at), day(18), utc.clone()), "07:27");
        assert_eq!(when_on(Some(at), day(19), utc.clone()), "Nov 18, 07:27");
        assert_eq!(when_on(None, day(18), utc), "—");
    }

    #[test]
    fn a_failed_run_says_why_once() {
        assert_eq!(result("success", ""), "ok");
        assert_eq!(
            result(
                "error",
                "ops: scheduled task target not found: no session ops on this node"
            ),
            "failed: ops: no session ops on this node"
        );
        assert_eq!(result("partial", ""), "partial");
        assert_eq!(result("", ""), "unknown");
    }

    /// The zone read from Windows' ICU is one jiff knows and keeps this
    /// machine's clock; on a Finnish home region it reads Helsinki (#222).
    #[cfg(windows)]
    #[test]
    fn windows_names_the_zone_of_the_home_region() {
        let zone = windows_zone().expect("Windows answers a zone");
        eprintln!("windows zone: {zone}");
        let now = Timestamp::now();
        let named = jiff::tz::TimeZone::get(&zone).unwrap();
        let system = jiff::tz::TimeZone::system();
        assert_eq!(named.to_offset(now), system.to_offset(now));
        assert_eq!(local_zone(), zone);
    }

    fn cron(expression: &str) -> Schedule {
        Schedule {
            kind: "cron".into(),
            expression: expression.into(),
            timezone: "Europe/Helsinki".into(),
            ..Schedule::default()
        }
    }

    /// CHROTE's scheduledSchedule.test.ts.
    #[test]
    fn the_editor_reads_and_writes_chrotes_shapes() {
        let mut f = ScheduleForm {
            timezone: "Europe/Helsinki".into(),
            ..ScheduleForm::default()
        };
        assert_eq!(f.schedule().expression, "0 9 * * *");
        f.mode = Mode::Weekly;
        f.weekdays = vec![0, 3, 1];
        f.time = "07:05".into();
        assert_eq!(f.schedule().expression, "5 7 * * 1,3,0");
        f.mode = Mode::Every;
        f.every_value = "2".into();
        f.every_hours = true;
        assert_eq!(f.schedule().every_minutes, 120);
        assert_eq!(f.schedule().kind, "interval");

        let weekly = ScheduleForm::of(&cron("30 8 * * 1,2,3,4,5"));
        assert_eq!((weekly.mode, weekly.time.as_str()), (Mode::Weekly, "08:30"));
        assert_eq!(weekly.weekdays, [1, 2, 3, 4, 5]);
        assert_eq!(ScheduleForm::of(&cron("0 16 * * *")).mode, Mode::Daily);
        assert_eq!(ScheduleForm::of(&cron("*/5 * * * *")).mode, Mode::Cron);
        let hours = ScheduleForm::of(&Schedule {
            kind: "interval".into(),
            every_minutes: 180,
            ..Schedule::default()
        });
        assert_eq!((hours.every_value.as_str(), hours.every_hours), ("3", true));
    }

    /// Review 13: an imported `duration` interval read as every 30 minutes
    /// and, saved with only its name changed, became every 30 minutes.
    #[test]
    fn an_unedited_cadence_is_saved_as_it_was() {
        let task: Task = serde_json::from_str(
            r#"{"id":"tsk_1","name":"daily","prompt":"p","targets":[{"session_name":"ops"}],
            "schedule":{"type":"interval","duration":"24h","timezone":"Local"},
            "enabled":true,"paused":false,"created_at":"2026-06-27T14:00:00Z","updated_at":"2026-06-27T14:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(describe(&task.schedule), "Every 24 hours");
        let mut form = TaskForm::edit("n", &task);
        assert_eq!(
            (
                form.schedule.every_value.as_str(),
                form.schedule.every_hours
            ),
            ("24", true)
        );
        form.name = "renamed".into();
        let body = form.body();
        assert_eq!(
            body["schedule"],
            serde_json::json!({"type":"interval","duration":"24h","timezone":"Local"})
        );
        form.schedule.every_value = "12".into();
        assert_eq!(
            form.body()["schedule"],
            serde_json::json!({"type":"interval","every_minutes":720,"timezone":form.schedule.timezone})
        );
        let odd = Schedule {
            kind: "interval".into(),
            duration: "90s".into(),
            ..Schedule::default()
        };
        assert_eq!(describe(&odd), "Every 90s");
    }

    /// Review 17: saving an edit sent `enabled: true` and so turned a
    /// disabled task back on.
    #[test]
    fn an_edit_keeps_a_disabled_task_disabled() {
        let task: Task = serde_json::from_str(
            r#"{"id":"tsk_1","name":"off","prompt":"p","targets":[{"session_name":"ops"}],
            "schedule":{"type":"interval","every_minutes":5,"timezone":"UTC"},
            "enabled":false,"paused":false,"created_at":"2026-06-27T14:00:00Z","updated_at":"2026-06-27T14:00:00Z"}"#,
        )
        .unwrap();
        let mut form = TaskForm::edit("n", &task);
        form.name = "renamed".into();
        assert_eq!(form.body()["enabled"], false);
        assert_eq!(TaskForm::new("n", vec![]).body()["enabled"], true);
    }

    #[test]
    fn schedules_read_as_words() {
        assert_eq!(describe(&cron("0 9 * * 1")), "Mon at 09:00");
        assert_eq!(describe(&cron("0 9 * * 1-5")), "Weekdays at 09:00");
        assert_eq!(
            describe(&cron("0 9 * * 1-3,5")),
            "Mon, Tue, Wed, Fri at 09:00"
        );
        assert_eq!(describe(&cron("0 9 * * 5-1")), "Cron 0 9 * * 5-1");
        assert_eq!(describe(&cron("0 9 * * 1,2,3,4,5")), "Weekdays at 09:00");
        assert_eq!(describe(&cron("0 9 * * 0,6")), "Sat, Sun at 09:00");
        assert_eq!(describe(&cron("15 16 * * *")), "Daily at 16:15");
        assert_eq!(describe(&cron("0 */3 * * *")), "Every 3 hours");
        assert_eq!(describe(&cron("0 * * * *")), "Every hour");
        assert_eq!(describe(&cron("30 */2 * * *")), "Every 2 hours at :30");
        assert_eq!(describe(&cron("*/15 * * * *")), "Every 15 minutes");
        assert_eq!(describe(&cron("* * * * *")), "Every minute");
        assert_eq!(
            describe(&cron("*/10 * * * 1-5")),
            "Every 10 minutes on weekdays"
        );
        assert_eq!(
            describe(&cron("30 9 1 * *")),
            "Day 1 of each month at 09:30"
        );
        // What does not read as a sentence keeps its cron text.
        assert_eq!(describe(&cron("0 9-17 * * *")), "Cron 0 9-17 * * *");
        assert_eq!(describe(&cron("0 0 1 1 *")), "Cron 0 0 1 1 *");
        assert_eq!(describe(&cron("0 */3 1 * *")), "Cron 0 */3 1 * *");
        let every = |m| Schedule {
            kind: "interval".into(),
            every_minutes: m,
            ..Schedule::default()
        };
        assert_eq!(describe(&every(60)), "Every hour");
        assert_eq!(describe(&every(90)), "Every 90 minutes");
        assert_eq!(describe(&every(1)), "Every minute");
    }

    #[test]
    fn a_form_says_what_is_missing_in_chrotes_order() {
        let mut f = TaskForm::new("n", vec![]);
        assert_eq!(f.error(), Some("Prompt is required."));
        f.prompt = "Continue if work is clear\nmore".into();
        assert_eq!(f.error(), Some("Add at least one target session."));
        f.toggle(Target {
            session_name: "ops".into(),
            unix_user: String::new(),
        });
        f.schedule.time = "25:00".into();
        assert_eq!(f.error(), Some("Time must be HH:MM."));
        f.schedule.time = "09:00".into();
        assert_eq!(f.error(), None);
        assert_eq!(f.body()["name"], "Continue if work is clear");
    }

    #[test]
    fn countdowns_read_as_words() {
        let now: Timestamp = "2026-06-27T14:00:00Z".parse().unwrap();
        let at = |s: &str| Some(s.parse::<Timestamp>().unwrap());
        assert_eq!(countdown(at("2026-06-27T15:05:00Z"), now), "in 1h 5m");
        assert_eq!(countdown(at("2026-06-29T15:00:00Z"), now), "in 2d 1h");
        assert_eq!(countdown(at("2026-06-27T13:00:00Z"), now), "due now");
        assert_eq!(countdown(at("2026-06-27T14:00:20Z"), now), "in <1m");
        let ran = |s: &str| ago(s.parse().unwrap(), now);
        assert_eq!(ran("2026-06-27T13:59:40Z"), "just now");
        assert_eq!(ran("2026-06-27T13:15:00Z"), "45m ago");
        assert_eq!(ran("2026-06-27T11:00:00Z"), "3h ago");
        assert_eq!(ran("2026-06-25T13:00:00Z"), "2d ago");
    }
}
