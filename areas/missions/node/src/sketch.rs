//! A mission sketched in seconds: what it is for and a few rough steps,
//! laid out as Input, the steps one after another, and Done. Nobody does
//! the steps yet; an agent reading the notes staffs and briefs them.
use crate::Missions;
use ahjo_core::Error;
use ahjo_missions_contract::{self as wire, NotePatch};
use serde_json::json;

/// The rough steps written one per line (or separated by `;`), trimmed,
/// with the empty ones left out.
pub(crate) fn steps(text: &str) -> Vec<String> {
    text.split(['\n', ';'])
        .map(str::trim)
        .filter(|step| !step.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Fill the new, empty mission `slug` with `steps` chained from its Input
/// to a Done end, and keep `note` as what it is for. A failure stops
/// here: what was made so far stays, as any edit does.
pub(crate) fn fill(
    m: &Missions,
    slug: &str,
    steps: &[String],
    note: &str,
    actor: &str,
) -> Result<(), Error> {
    let call = |name: &str, args: serde_json::Value| {
        let mut args = args;
        args["mission"] = json!(slug);
        args["updated_by"] = json!(actor);
        crate::authoring::call(m, name, &args)
    };
    if !steps.is_empty() {
        call("mission_create", json!({}))?;
        let mut previous: Option<String> = None;
        for title in steps {
            let made = call("formation_create", json!({"title": title}))?;
            let id = made["formation"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            match &previous {
                None => call("mission_wire", json!({"to": id}))?,
                Some(from) => call("formation_wire", json!({"from": from, "to": id}))?,
            };
            previous = Some(id);
        }
        let end = call("end_create", json!({"title": "Done"}))?;
        let end = end["end"]["id"].as_str().unwrap_or_default();
        if let Some(last) = previous {
            call("formation_wire", json!({"from": last, "to": end}))?;
        }
    }
    if !note.trim().is_empty() {
        let current = m.store.notes_edit(slug)?;
        m.store.patch_notes(
            slug,
            &current.etag,
            NotePatch::Append {
                target: wire::MISSION_THREAD.into(),
                author: if actor.starts_with("human:") {
                    "human:ui".into()
                } else {
                    actor.into()
                },
                text: note.trim_end().into(),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rough_steps_are_lines_or_semicolons() {
        assert_eq!(
            super::steps("Gather PRs\n\n  Draft ; Review\n"),
            ["Gather PRs", "Draft", "Review"]
        );
    }
}
