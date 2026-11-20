//! End, Limit and gate field edits.
use crate::Error;
use ahjo_mission_model::{
    EndOutcome, Gate, GateKind, LayoutNode, Limit, Mission, MissionChange, WhenSpent,
};
use toml_edit::{DocumentMut, Item, value};
fn missing() -> Error {
    Error::new("not_found", "node")
}
fn table<'a>(
    doc: &'a mut DocumentMut,
    key: &str,
    id: &str,
) -> Result<&'a mut toml_edit::Table, Error> {
    doc.get_mut(key)
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|arr| {
            arr.iter_mut()
                .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
        })
        .ok_or_else(missing)
}
fn check_limit(mission: &Mission, limit: &mut Limit) -> Result<(), Error> {
    limit.target = limit.target.trim().into();
    limit.title = limit.title.trim().into();
    if limit.title.is_empty() {
        limit.title = "Limit".into();
    }
    if limit.when_spent == WhenSpent::Stop {
        limit.when_spent = WhenSpent::Unset;
    }
    if !limit.target.is_empty()
        && !mission.has_input_card(&limit.target)
        && mission.step(&limit.target).is_none()
    {
        return Err(Error::new("invalid_limit", "invalid target"));
    }
    for knob in [
        &mut limit.rounds,
        &mut limit.seconds,
        &mut limit.warn_seconds,
        &mut limit.tokens,
    ] {
        if knob.is_some_and(|n| n < 0) {
            return Err(Error::new("invalid_limit", "negative limit"));
        }
        if *knob == Some(0) {
            *knob = None;
        }
    }
    Ok(())
}
pub(crate) fn create_limit(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<LayoutNode, Error> {
    let MissionChange::CreateLimit {
        title,
        target,
        rounds,
        seconds,
        warn_seconds,
        tokens,
        when_spent,
        x,
        y,
    } = patch
    else {
        unreachable!()
    };
    let mut limit = Limit {
        id: String::new(),
        title,
        target,
        rounds,
        seconds,
        warn_seconds,
        tokens,
        when_spent,
    };
    check_limit(mission, &mut limit)?;
    limit.id = crate::authoring::id("lim")?;
    crate::authoring_create::append(doc, "limit", &limit)?;
    Ok(LayoutNode { id: limit.id, x, y })
}
pub(crate) fn check_gate(gate: &mut Gate) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    gate.kinds.retain(|kind| seen.insert(*kind));
    if gate.kinds.is_empty() {
        return Err(Error::new("invalid_gate_kind", "gate needs a kind"));
    }
    crate::authoring_create::file_refs(&mut gate.files)?;
    let has_check_profile =
        !gate.check.is_empty() || !gate.check_version.is_empty() || !gate.check_value.is_empty();
    if has_check_profile && !gate.kinds.contains(&GateKind::Code) {
        return Err(Error::new(
            "invalid_code_gate_profile",
            "check needs code kind",
        ));
    }
    if (!gate.check.is_empty() || !gate.check_version.is_empty())
        && !crate::code_check::CODE_CHECKS.iter().any(|c| {
            (gate.check.is_empty() || c.id == gate.check)
                && (gate.check_version.is_empty() || c.version == gate.check_version)
        })
    {
        return Err(Error::new(
            "invalid_code_gate_profile",
            "unknown code check",
        ));
    }
    Ok(())
}
pub(crate) fn update(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<bool, Error> {
    match patch {
        MissionChange::UpdateEnd(u) => {
            let old = mission.end(&u.id).ok_or_else(missing)?;
            let outcome = match u.outcome.unwrap_or(old.outcome) {
                EndOutcome::Unset => EndOutcome::Done,
                outcome => outcome,
            };
            let title = u.title.as_deref().unwrap_or(&old.title);
            if title == old.title && outcome == old.outcome {
                return Ok(false);
            }
            let table = table(doc, "end", &u.id)?;
            if title != old.title {
                crate::write::set(table, "title", value(title));
            }
            if outcome != old.outcome {
                crate::write::set(table, "outcome", value(outcome.as_str()));
            }
        }
        MissionChange::UpdateLimit(u) => {
            let old = mission
                .limits
                .iter()
                .find(|n| n.id == u.id)
                .ok_or_else(missing)?;
            let mut next = old.clone();
            if let Some(v) = u.title {
                next.title = v;
            }
            if let Some(v) = u.target {
                next.target = v;
            }
            if let Some(v) = u.rounds {
                next.rounds = Some(v);
            }
            if let Some(v) = u.seconds {
                next.seconds = Some(v);
            }
            if let Some(v) = u.warn_seconds {
                next.warn_seconds = Some(v);
            }
            if let Some(v) = u.tokens {
                next.tokens = Some(v);
            }
            if let Some(v) = u.when_spent {
                next.when_spent = v;
            }
            check_limit(mission, &mut next)?;
            if *old == next {
                return Ok(false);
            }
            let table = table(doc, "limit", &u.id)?;
            for (key, before, after) in [
                ("title", old.title.as_str(), next.title.as_str()),
                ("target", old.target.as_str(), next.target.as_str()),
                (
                    "when_spent",
                    old.when_spent.as_str(),
                    next.when_spent.as_str(),
                ),
            ] {
                if before != after {
                    if key == "when_spent" && after.is_empty() {
                        table.remove(key);
                    } else {
                        crate::write::set(table, key, value(after));
                    }
                }
            }
            for (key, before, after) in [
                ("rounds", old.rounds, next.rounds),
                ("seconds", old.seconds, next.seconds),
                ("warn_seconds", old.warn_seconds, next.warn_seconds),
                ("tokens", old.tokens, next.tokens),
            ] {
                if before != after {
                    if let Some(v) = after {
                        crate::write::set(table, key, value(v));
                    } else {
                        table.remove(key);
                    }
                }
            }
        }
        MissionChange::UpdateGate(u) => {
            let old = mission.gate(&u.id).ok_or_else(missing)?;
            let sets_check =
                u.check.is_some() || u.check_version.is_some() || u.check_value.is_some();
            let sets_kinds = u.kinds.is_some();
            let sets_files = u.files.is_some();
            let mut next = old.clone();
            if let Some(v) = u.title {
                next.title = v;
            }
            if let Some(v) = u.kinds {
                next.kinds = v;
            }
            if let Some(v) = u.criterion {
                next.criterion = v;
            }
            if let Some(v) = u.check {
                next.check = v.trim().into();
            }
            if let Some(v) = u.check_version {
                next.check_version = v.trim().into();
            }
            if let Some(v) = u.check_value {
                next.check_value = v.trim().into();
            }
            if let Some(v) = u.files {
                next.files = v;
            }
            if !next.kinds.contains(&GateKind::Code) {
                if sets_check
                    && !(next.check.is_empty()
                        && next.check_version.is_empty()
                        && next.check_value.is_empty())
                {
                    return Err(Error::new(
                        "invalid_code_gate_profile",
                        "check needs code kind",
                    ));
                }
                next.check.clear();
                next.check_version.clear();
                next.check_value.clear();
            }
            if sets_kinds || sets_check || sets_files {
                let mut check = next.clone();
                if !sets_kinds {
                    check.kinds = GateKind::ALL.to_vec();
                }
                if !sets_check {
                    check.check.clear();
                    check.check_version.clear();
                    check.check_value.clear();
                }
                if !sets_files {
                    check.files.clear();
                }
                check_gate(&mut check)?;
                if sets_kinds {
                    next.kinds = check.kinds;
                }
                if sets_files {
                    next.files = check.files;
                }
            }
            if *old == next {
                return Ok(false);
            }
            let table = table(doc, "gate", &u.id)?;
            for (key, before, after) in [
                ("title", &old.title, &next.title),
                ("criterion", &old.criterion, &next.criterion),
                ("check", &old.check, &next.check),
                ("check_version", &old.check_version, &next.check_version),
                ("check_value", &old.check_value, &next.check_value),
            ] {
                if before != after {
                    if after.is_empty() && key.starts_with("check") {
                        table.remove(key);
                    } else {
                        crate::write::set(table, key, value(after.clone()));
                    }
                }
            }
            if old.kinds != next.kinds {
                let kinds: toml_edit::Array = next.kinds.iter().map(|kind| kind.as_str()).collect();
                crate::write::set(table, "kinds", value(kinds));
            }
            if old.files != next.files {
                if next.files.is_empty() {
                    table.remove("files");
                } else {
                    let files: toml_edit::Array = next.files.iter().map(String::as_str).collect();
                    crate::write::set(table, "files", value(files));
                }
            }
            if old.kinds.contains(&GateKind::Step) && !next.kinds.contains(&GateKind::Step) {
                let endpoint = format!("{}:judge", u.id);
                if let Some(wires) = doc
                    .get_mut("connection")
                    .and_then(Item::as_array_of_tables_mut)
                {
                    let indices: Vec<_> = wires
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            ["from", "to"]
                                .iter()
                                .any(|k| t.get(k).and_then(Item::as_str) == Some(endpoint.as_str()))
                        })
                        .map(|(i, _)| i)
                        .collect();
                    for i in indices.into_iter().rev() {
                        wires.remove(i);
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(true)
}
