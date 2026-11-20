//! A mission sketched in seconds: what it is for and a few rough steps,
//! laid out as Input, the steps one after another, and Done. Nobody does
//! the steps yet; an agent reading the notes staffs and briefs them.
use crate::Missions;
use ahjo_core::Error;
use ahjo_missions_contract::{self as wire, LayoutPatch, MissionChange, NotePatch, Precondition};

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
/// to a Done end, and keep `note` as what it is for. Creating a sketch is
/// all or nothing: on a failure the half-made mission is removed with its
/// archive, and the error says so.
pub(crate) fn fill(
    m: &Missions,
    id: &str,
    slug: &str,
    steps: &[String],
    note: &str,
    actor: &str,
) -> Result<(), Error> {
    let Err(error) = sketch(m, id, steps, note, actor) else {
        return Ok(());
    };
    let message = match discard(m, id) {
        Ok(()) => format!(
            "{}; the mission was not created and nothing of it is left, so create it again once that is put right",
            error.message
        ),
        Err(left) => format!(
            "{}; the half-made mission {slug} could not be removed ({}), so delete it with `ahjo mission delete {slug}` before creating it again",
            error.message, left.message
        ),
    };
    m.changed();
    Err(Error::new(error.code, message))
}

/// Remove the mission `slug` this sketch made, archive and all.
fn discard(m: &Missions, id: &str) -> Result<(), Error> {
    let current = m.store.mission_edit(id)?;
    let deleted = m.store.delete_mission(
        id,
        Precondition {
            rev: current.mission.rev,
            etag: current.etag,
        },
    )?;
    m.store
        .remove_mission_archive(&deleted.slug, &deleted.archive_id)?;
    Ok(())
}

fn sketch(m: &Missions, id: &str, steps: &[String], note: &str, actor: &str) -> Result<(), Error> {
    if !steps.is_empty() {
        let current = m.store.mission_edit(id)?;
        let mut cards = vec![MissionChange::CreateInputCard {
            title: "Input".into(),
            goal: String::new(),
            files: Vec::new(),
            human_channel: wire::HumanChannel::Unset,
            x: 0,
            y: 0,
        }];
        cards.extend(steps.iter().map(|title| MissionChange::CreateStep {
            formation: wire::Formation::Solo,
            title: title.clone(),
            x: 0,
            y: 0,
        }));
        cards.push(MissionChange::CreateEnd {
            title: "Done".into(),
            outcome: wire::EndOutcome::Done,
            x: 0,
            y: 0,
        });
        let edited = m.store.patch_batch(
            id,
            Precondition {
                rev: current.mission.rev,
                etag: current.etag,
            },
            cards,
            actor,
        )?;
        let input = &edited.mission.input_cards[current.mission.input_cards.len()];
        let end = &edited.mission.ends[current.mission.ends.len()];
        let mut from = format!("{}:{}", input.id, wire::INPUT_CARD_OUT);
        let mut wires = Vec::with_capacity(steps.len() + 1);
        for step in &edited.mission.steps[current.mission.steps.len()..] {
            wires.push(connection(
                from,
                format!("{}:{}", step.id, step.inputs[0].id),
            ));
            from = format!("{}:{}", step.id, step.outputs[0].id);
        }
        wires.push(connection(from, format!("{}:{}", end.id, wire::END_IN)));
        let edited = m.store.patch_batch(
            id,
            Precondition {
                rev: edited.mission.rev,
                etag: edited.etag,
            },
            wires,
            actor,
        )?;
        let layout = m.store.layout_edit(id)?;
        let arranged = ahjo_mission_engine::arrange(&edited.mission, &layout.layout);
        m.store
            .patch_layout(id, &layout.etag, LayoutPatch::Nodes(arranged.nodes))?;
    }
    if !note.trim().is_empty() {
        let current = m.store.notes_edit(id)?;
        m.store.patch_notes(
            id,
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

fn connection(from: String, to: String) -> MissionChange {
    MissionChange::WireConnection {
        from,
        to,
        id: String::new(),
        join_if_occupied: false,
    }
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
