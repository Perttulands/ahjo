//! Reference files on Input cards, briefs and gates that name no file this
//! card can open. A file may still appear before a run reads it, so these
//! warn rather than block.

use std::path::Path;

use super::possessive;
use ahjo_mission_model::{Code, Finding, Mission};

pub fn findings(mission: &Mission) -> Vec<Finding> {
    let cards = mission
        .input_cards
        .iter()
        .map(|card| (&card.id, &card.files));
    let briefs = (mission.steps.iter())
        .filter_map(|step| step.brief.as_ref().map(|brief| (&step.id, &brief.files)));
    let gates = mission.gates.iter().map(|gate| (&gate.id, &gate.files));
    let mut findings = Vec::new();
    for (card, files) in cards.chain(briefs).chain(gates) {
        for file in files {
            let (code, problem) = if !Path::new(file).is_absolute() {
                (Code::RelativeFile, "is relative: use an absolute path")
            } else if std::fs::metadata(file)
                .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
            {
                (Code::MissingFile, "does not exist")
            } else {
                continue;
            };
            findings.push(Finding {
                path: Some(file.clone()),
                ..Finding::new(
                    code,
                    card,
                    format!(
                        "{} file {file} {problem}",
                        possessive(mission.card_name(card))
                    ),
                )
            });
        }
    }
    findings
}
