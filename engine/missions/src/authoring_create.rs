//! Construct graph nodes with Archon's default shape.
use crate::Error;
use ahjo_mission_model::{
    End, EndOutcome, Formation, Gate, GateKind, HumanChannel, InputCard, Mission, MissionChange,
    Port, Slot, Step,
};
use serde::Serialize;
use toml_edit::{ArrayOfTables, DocumentMut, Item};

pub(crate) fn append<T: Serialize>(
    doc: &mut DocumentMut,
    key: &str,
    node: &T,
) -> Result<(), Error> {
    let table = toml_edit::ser::to_document(node)
        .map_err(|e| Error::new("invalid_definition", e.to_string()))?
        .into_table();
    crate::write::table_arrays(doc, &[key]);
    if !doc.contains_key(key) {
        doc[key] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let array = doc[key].as_array_of_tables_mut().ok_or_else(|| {
        Error::new(
            "invalid_definition",
            format!("{key} is not an array of tables"),
        )
    })?;
    array.push(table);
    Ok(())
}

pub(crate) fn file_refs(files: &mut Vec<String>) -> Result<(), Error> {
    for file in files.iter_mut() {
        *file = file.trim().into();
    }
    files.retain(|file| !file.is_empty());
    for file in files.iter() {
        let windows_absolute = file.starts_with("\\\\")
            || (file.len() >= 3
                && file.as_bytes()[0].is_ascii_alphabetic()
                && file.as_bytes()[1] == b':'
                && matches!(file.as_bytes()[2], b'/' | b'\\'));
        if !std::path::Path::new(file).is_absolute() && !windows_absolute {
            return Err(Error::new(
                "relative_file_reference",
                format!("file {file:?} is relative: use an absolute path"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply(
    doc: &mut DocumentMut,
    _mission: &Mission,
    patch: MissionChange,
) -> Result<ahjo_mission_model::LayoutNode, Error> {
    match patch {
        MissionChange::CreateInputCard {
            title,
            goal,
            mut files,
            human_channel,
            x,
            y,
        } => {
            file_refs(&mut files)?;
            let node = InputCard {
                id: crate::authoring::id("inp")?,
                title: if title.is_empty() {
                    "Input".into()
                } else {
                    title
                },
                goal,
                files,
                human_channel: if human_channel == HumanChannel::Notify {
                    HumanChannel::Unset
                } else {
                    human_channel
                },
                ..InputCard::default()
            };
            append(doc, "input_card", &node)?;
            Ok(ahjo_mission_model::LayoutNode { id: node.id, x, y })
        }
        MissionChange::CreateStep {
            mut formation,
            mut title,
            x,
            y,
        } => {
            let (default_title, slots): (&str, &[(&str, bool)]) = match formation {
                Formation::Unset | Formation::Solo => {
                    formation = Formation::Solo;
                    ("Solo task", &[("Agent", false)])
                }
                Formation::Peer => ("Peer huddle", &[("Peer", false), ("Peer", false)]),
                Formation::Orchestrated => (
                    "Orchestration",
                    &[("Orchestrator", true), ("Agent", false), ("Agent", false)],
                ),
            };
            if title.is_empty() {
                title = default_title.into();
            }
            let mut node = Step {
                id: crate::authoring::id("fmn")?,
                formation,
                title,
                ..Step::default()
            };
            node.inputs.push(Port {
                id: crate::authoring::id("port")?,
                label: "Input".into(),
                ..Port::default()
            });
            node.outputs.push(Port {
                id: crate::authoring::id("port")?,
                label: "Output".into(),
                ..Port::default()
            });
            for (label, controller) in slots {
                node.slots.push(Slot {
                    id: crate::authoring::id("slot")?,
                    label: (*label).into(),
                    controller: *controller,
                    ..Slot::default()
                });
            }
            append(doc, "step", &node)?;
            Ok(ahjo_mission_model::LayoutNode { id: node.id, x, y })
        }
        MissionChange::CreateGate {
            title,
            mut kinds,
            criterion,
            check,
            check_version,
            check_value,
            files,
            x,
            y,
        } => {
            if kinds.is_empty() {
                kinds.push(GateKind::Step);
            }
            let mut node = Gate {
                id: crate::authoring::id("gate")?,
                title: if title.is_empty() {
                    "Review gate".into()
                } else {
                    title
                },
                kinds,
                criterion,
                check,
                check_version,
                check_value,
                files,
            };
            crate::authoring_controls::check_gate(&mut node)?;
            append(doc, "gate", &node)?;
            Ok(ahjo_mission_model::LayoutNode { id: node.id, x, y })
        }
        MissionChange::CreateEnd {
            mut title,
            mut outcome,
            x,
            y,
        } => {
            title = title.trim().into();
            if outcome == EndOutcome::Unset {
                outcome = EndOutcome::Done;
            }
            if title.is_empty() {
                title = if outcome == EndOutcome::Done {
                    "Done"
                } else {
                    "Rejected"
                }
                .into();
            }
            let node = End {
                id: crate::authoring::id("end")?,
                title,
                outcome,
            };
            append(doc, "end", &node)?;
            Ok(ahjo_mission_model::LayoutNode { id: node.id, x, y })
        }
        _ => unreachable!(),
    }
}
