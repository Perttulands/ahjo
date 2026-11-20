//! Translate CLI authoring commands into one node edit.
use super::{
    Args, Side, endpoint, only_slot, select, select_formation, selected_input, set_node_flag,
    slot_id, value_flags,
};
use ahjo_missions_contract::{Mission, MissionInput, MissionOutput, Slot};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub(super) fn operation<'a>(
    noun: &str,
    verb: &str,
    a: &Args,
    m: &Mission,
) -> Result<(&'a str, Value, Option<&'a str>)> {
    let node = |kind: &str| -> Result<String> {
        let selector = a.arg(1)?;
        let id = match kind {
            "formation" => &select_formation(m, selector)?.id,
            "gate" => &select(&m.gates, selector, |n| (&n.id, &n.title))?.id,
            "end" => &select(&m.ends, selector, |n| (&n.id, &n.title))?.id,
            "limit" => &select(&m.limits, selector, |n| (&n.id, &n.title))?.id,
            _ => unreachable!(),
        };
        Ok(id.clone())
    };
    let mut v = value_flags(
        a,
        &[
            ("title", "title"),
            ("goal", "goal"),
            ("human-channel", "humanChannel"),
            ("input-hint", "inputHint"),
            ("criterion", "criterion"),
            ("check", "check"),
            ("check-version", "checkVersion"),
            ("check-value", "checkValue"),
            ("outcome", "outcome"),
            ("target", "target"),
            ("when-spent", "whenSpent"),
        ],
    );
    set_node_flag(&mut v, a, "files");
    for coord in ["x", "y"] {
        if let Some(n) = a.flag(coord) {
            v[coord] = json!(n.parse::<i64>()?);
        }
    }
    let result = match (noun, verb) {
        ("mission", "create") => ("createInputCard", Some("inputCard")),
        ("mission", "update") => {
            v["id"] = json!(selected_input(m, a.pos.get(1).map(String::as_str))?.id);
            ("updateInputCard", None)
        }
        ("mission", "input" | "output") => {
            let card = selected_input(m, None)?;
            if a.has("required") && a.has("optional") {
                bail!("give --required or --optional, not both")
            }
            if a.has("delete")
                && ["kind", "description", "from", "required", "optional"]
                    .iter()
                    .any(|k| a.has(k))
            {
                bail!("--delete takes no other input or output flags")
            }
            if verb == "input" {
                let items = edit_declarations(
                    &card.inputs,
                    a,
                    |item| &item.name,
                    |old, name| {
                        let mut item = old.unwrap_or_else(|| MissionInput {
                            name: name.into(),
                            kind: "text".into(),
                            ..Default::default()
                        });
                        declaration_flags(
                            a,
                            &mut item.kind,
                            &mut item.description,
                            &mut item.required,
                        );
                        item
                    },
                )?;
                v = json!({"id":card.id,"inputs":items});
            } else {
                let items = edit_declarations(
                    &card.outputs,
                    a,
                    |item| &item.name,
                    |old, name| {
                        let mut item = old.unwrap_or_else(|| MissionOutput {
                            name: name.into(),
                            kind: "text".into(),
                            ..Default::default()
                        });
                        declaration_flags(
                            a,
                            &mut item.kind,
                            &mut item.description,
                            &mut item.required,
                        );
                        if let Some(from) = a.flag("from") {
                            item.from = from.into();
                        }
                        item
                    },
                )?;
                v = json!({"id":card.id,"outputs":items});
            }
            ("updateInputCard", None)
        }
        ("formation", "create") => {
            v["type"] = json!(a.pos.get(1).map(String::as_str).unwrap_or(""));
            ("createFormation", Some("formation"))
        }
        ("formation", "delete") => {
            v = json!({"id":node("formation")?});
            ("deleteFormation", None)
        }
        ("formation", "rename") => {
            v = json!({"id":node("formation")?,"title":a.arg(2)?});
            ("updateFormation", None)
        }
        ("formation", "place") => {
            // device/environment as the step editor offers it; home or empty
            // for wherever the run starts.
            let place = match a.arg(2)? {
                "home" => "",
                place => place,
            };
            if !place.is_empty() && !place.contains('/') {
                bail!("place is device/environment, or home");
            }
            v = json!({"id":node("formation")?,"device":place});
            ("updateFormation", None)
        }
        ("formation", "set-type") => {
            let id = node("formation")?;
            let keep = a
                .flag("keep-slot")
                .map(|s| slot_id(m, &id, s))
                .transpose()?
                .unwrap_or_default();
            v = json!({"id":id,"type":a.arg(2)?,"keepSlotId":keep});
            ("setFormationType", None)
        }
        ("formation", "set-slots") => {
            let formation = select_formation(m, a.arg(1)?)?;
            let controller = a.flag("controller").unwrap_or("");
            let mut slots = Vec::new();
            for entry in a.flags.get("slot").into_iter().flatten() {
                let (slot_id, label) = entry.split_once('=').context("--slot needs id=label")?;
                if slot_id.is_empty() || label.is_empty() {
                    bail!("--slot needs id=label");
                }
                let mut slot = formation
                    .slots
                    .iter()
                    .find(|s| s.id == slot_id)
                    .cloned()
                    .unwrap_or_else(|| Slot {
                        id: slot_id.into(),
                        ..Default::default()
                    });
                slot.label = label.into();
                slot.controller = slot_id == controller;
                slots.push(slot);
            }
            if slots.is_empty() {
                bail!("at least one --slot is required");
            }
            if !controller.is_empty() && !slots.iter().any(|s| s.id == controller) {
                bail!("--controller must name a slot");
            }
            v = json!({"id":formation.id,"type":formation.kind,"slots":slots});
            ("setFormationType", None)
        }
        ("formation", "assign" | "unassign") => {
            let id = node("formation")?;
            let slot = match a.flag("slot") {
                Some(selector) => slot_id(m, &id, selector)?,
                None => only_slot(m, &id)?,
            };
            v = json!({"formationId":id,"slotId":slot,"agentId":a.flag("role").unwrap_or(""),"harness":a.flag("harness").unwrap_or(""),"model":a.flag("model").unwrap_or(""),"effort":a.flag("effort").unwrap_or("")});
            ("assignSlot", None)
        }
        ("formation", "agent") => {
            let id = node("formation")?;
            let slot = match a.flag("slot") {
                Some(selector) => slot_id(m, &id, selector)?,
                None => only_slot(m, &id)?,
            };
            let agent = a.pos.get(2).map(String::as_str).unwrap_or("");
            v = json!({"formationId":id,"slotId":slot,"agent":agent});
            ("nameAgent", None)
        }
        ("formation", "fresh") => {
            let fresh = match a.arg(2)? {
                "on" | "true" => true,
                "off" | "false" => false,
                _ => bail!("fresh is on or off"),
            };
            v = json!({"id":node("formation")?,"fresh":fresh});
            ("updateFormation", None)
        }
        ("formation", "set-brief") => {
            if let Some(repository) = a.flag("repository") {
                v["repository"] = json!(repository);
            }
            v["formationId"] = json!(node("formation")?);
            if let Some(bead) = a.flag("bead") {
                v["work"] = json!(bead);
            }
            if let Some(links) = a.flags.get("link") {
                v["links"] = json!(links);
            }
            ("setBrief", None)
        }
        ("formation", "add-input" | "add-output") => {
            v = json!({"formationId":node("formation")?,"direction":if verb=="add-input"{"input"}else{"output"},"label":a.flag("label").context("--label is required")?,"kind":a.flag("kind").unwrap_or("")});
            ("addPort", None)
        }
        ("formation", "set-port") => {
            let formation = node("formation")?;
            let full = endpoint(m, &format!("{formation}:{}", a.arg(2)?), Side::From)?;
            v["formationId"] = json!(formation);
            v["portId"] = json!(full.split_once(':').unwrap().1);
            for (flag, key) in [("label", "label"), ("kind", "kind")] {
                if let Some(x) = a.flag(flag) {
                    v[key] = json!(x)
                }
            }
            ("updatePort", None)
        }
        ("formation", "wire") => {
            v = json!({"from":endpoint(m,a.arg(1)?,Side::From)?,"to":endpoint(m,a.arg(2)?,Side::To)?,"joinIfOccupied":a.has("join")});
            ("wireConnection", None)
        }
        ("mission", "wire") => {
            let (input, to) = if a.pos.len() == 2 {
                (&selected_input(m, None)?.id, a.arg(1)?)
            } else {
                (&selected_input(m, Some(a.arg(1)?))?.id, a.arg(2)?)
            };
            v = json!({"from":format!("{input}:out"),"to":endpoint(m,to,Side::To)?});
            ("wireConnection", None)
        }
        ("formation", "unwire") => {
            v = json!({"from":endpoint(m,a.arg(1)?,Side::From)?,"to":endpoint(m,a.arg(2)?,Side::To)?});
            ("unwireConnection", None)
        }
        ("gate", "create") => {
            if let Some(kinds) = a.flag("kinds") {
                v["kinds"] = json!(
                    kinds
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                )
            }
            ("createGate", Some("gate"))
        }
        ("gate", "update") => {
            v["id"] = json!(node("gate")?);
            if a.has("clear-check") {
                if ["check", "check-version", "check-value"]
                    .iter()
                    .any(|flag| a.has(flag))
                {
                    bail!("--clear-check cannot be combined with check fields");
                }
                for key in ["check", "checkVersion", "checkValue"] {
                    v[key] = json!("");
                }
            }
            if let Some(kinds) = a.flag("kinds") {
                v["kinds"] = json!(
                    kinds
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                )
            }
            ("updateGate", None)
        }
        ("gate", "delete") => {
            v = json!({"id":node("gate")?});
            ("deleteGate", None)
        }
        ("gate", "judge") => {
            if a.has("detach") {
                v = json!({"gateId":node("gate")?});
                ("detachGateJudge", None)
            } else {
                let chain = a
                    .flag("chain")
                    .context("--chain is required")?
                    .split(',')
                    .map(|s| select_formation(m, s).map(|step| &step.id))
                    .collect::<Result<Vec<_>>>()?;
                v = json!({"gateId":node("gate")?,"chain":chain});
                ("setGateJudge", None)
            }
        }
        ("end", "create") => ("createEnd", Some("end")),
        ("end", "update") => {
            v["id"] = json!(node("end")?);
            ("updateEnd", None)
        }
        ("end", "delete") => {
            v = json!({"id":node("end")?});
            ("deleteEnd", None)
        }
        ("limit", "create" | "update") => {
            if verb == "update" {
                v["id"] = json!(node("limit")?)
            }
            if let Some(target) = a.flag("target") {
                v["target"] = json!(if target.is_empty() {
                    String::new()
                } else if target == "input" {
                    selected_input(m, None)?.id.clone()
                } else if let Ok(step) = select_formation(m, target) {
                    step.id.clone()
                } else {
                    selected_input(m, Some(target))?.id.clone()
                });
            }
            for (flag, key) in [
                ("rounds", "rounds"),
                ("tokens", "tokens"),
                ("time", "seconds"),
                ("warn", "warnSeconds"),
            ] {
                if let Some(s) = a.flag(flag) {
                    v[key] = json!(duration(s)?);
                }
            }
            (
                if verb == "create" {
                    "createLimit"
                } else {
                    "updateLimit"
                },
                if verb == "create" {
                    Some("limit")
                } else {
                    None
                },
            )
        }
        ("limit", "delete") => {
            v = json!({"id":node("limit")?});
            ("deleteLimit", None)
        }
        _ => bail!("{noun} {verb} is not implemented"),
    };
    Ok((result.0, v, result.1))
}
fn edit_declarations<T: Clone>(
    existing: &[T],
    args: &Args,
    name_of: impl Fn(&T) -> &str,
    update: impl FnOnce(Option<T>, &str) -> T,
) -> Result<Vec<T>> {
    let name = args.arg(1)?;
    let mut items = existing.to_vec();
    let index = items.iter().position(|item| name_of(item) == name);
    if args.has("delete") {
        items.remove(index.context("input or output not found")?);
    } else if let Some(index) = index {
        let item = items.remove(index);
        items.insert(index, update(Some(item), name));
    } else {
        items.push(update(None, name));
    }
    Ok(items)
}

fn declaration_flags(
    args: &Args,
    kind: &mut String,
    description: &mut String,
    required: &mut bool,
) {
    if let Some(value) = args.flag("kind") {
        *kind = value.into();
    }
    if let Some(value) = args.flag("description") {
        *description = value.into();
    }
    if args.has("required") {
        *required = true;
    } else if args.has("optional") {
        *required = false;
    }
}

fn duration(s: &str) -> Result<i64> {
    if s.is_empty() {
        return Ok(0);
    }
    let mut sum: i64 = 0;
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c)
        } else {
            let n: i64 = digits.parse()?;
            digits.clear();
            let multiplier = match c {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                _ => bail!("invalid duration"),
            };
            sum = sum
                .checked_add(n.checked_mul(multiplier).context("duration too large")?)
                .context("duration too large")?;
        }
    }
    if !digits.is_empty() {
        sum = sum
            .checked_add(digits.parse::<i64>()?)
            .context("duration too large")?
    }
    Ok(sum)
}
