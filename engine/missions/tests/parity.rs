//! Validation emits the finding codes Archon emits on the same files.
//!
//! `tests/parity` is a state directory: missions, a retired role card and a
//! Codex model cache. `archon-findings.json` holds what Archon's own
//! `mission validate --json` reported on each mission, recorded by
//! `archon-findings.sh`; this test holds Ahjo to every code and node.

use std::collections::BTreeMap;
use std::path::Path;

use ahjo_mission_engine::{Harnesses, Store, validate};
use ahjo_mission_model::{Finding, Mission};
use serde::Deserialize;

#[derive(Deserialize)]
struct Expected {
    errors: Vec<(String, String)>,
    warnings: Vec<(String, String)>,
}

fn located(findings: &[Finding]) -> Vec<(String, String)> {
    let mut pairs: Vec<_> = (findings.iter())
        .map(|finding| (finding.code.as_str().to_owned(), finding.node_id.clone()))
        .collect();
    pairs.sort();
    pairs
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn agents(parity: &Path) -> Self {
        let root = Self(ahjo_testkit::fresh_dir("agent-parity"));
        std::fs::create_dir_all(root.0.join("roles")).unwrap();
        std::fs::copy(
            parity.join("roles/critic.toml"),
            root.0.join("roles/critic.toml"),
        )
        .unwrap();
        root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn findings_match_archons_on_every_parity_mission() {
    let parity = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let expected: BTreeMap<String, Expected> = serde_json::from_str(
        &std::fs::read_to_string(parity.join("archon-findings.json")).unwrap(),
    )
    .unwrap();
    let store = Store::new(&parity);
    let scratch = Scratch::agents(&parity);
    let agents = Store::new(&scratch.0).agents();
    let harnesses = Harnesses::from_codex_home(&parity.join("codex"));
    let listed: Vec<_> = store
        .list_missions()
        .unwrap()
        .into_iter()
        .map(|listing| listing.slug)
        .collect();
    assert_eq!(
        listed,
        expected.keys().cloned().collect::<Vec<_>>(),
        "every parity mission has Archon's findings"
    );
    for (slug, archon) in expected {
        let source =
            std::fs::read_to_string(parity.join("missions").join(format!("{slug}.mission.toml")))
                .unwrap();
        let replacements: &[(&str, &str, &str)] = match slug.as_str() {
            "cards" => &[(
                "human_channel = \"pager\"",
                "human_channel = \"\"",
                "notify",
            )],
            "steps" => &[
                ("formation = \"swarm\"", "formation = \"\"", "solo"),
                ("harness = \"hermes\"", "harness = \"\"", "claude-code"),
            ],
            "inputs" => &[
                ("kind = \"url\"", "kind = \"text\"", "folder"),
                ("kind = \"website\"", "kind = \"text\"", "folder"),
                ("kind = \"image\"", "kind = \"text\"", "folder"),
            ],
            "limits" => &[
                ("outcome = \"maybe\"", "outcome = \"\"", "rejected"),
                ("outcome = \"  \"", "outcome = \"done\"", "rejected"),
                ("when_spent = \"maybe\"", "when_spent = \"\"", "best"),
            ],
            "wiring" => &[("\"vibes\", ", "", "human")],
            _ => &[],
        };
        let sanitized = replacements
            .iter()
            .fold(source.clone(), |text, (bad, good, _)| {
                assert!(text.contains(bad), "{slug}: missing {bad}");
                text.replacen(bad, good, 1)
            });
        // Each invalid value is rejected on its own, with valid values in the error.
        for (index, (_, _, allowed)) in replacements.iter().enumerate() {
            let candidate = replacements
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .fold(source.clone(), |text, (_, (bad, good, _))| {
                    text.replacen(bad, good, 1)
                });
            let error = Mission::parse(&candidate).unwrap_err();
            assert_eq!(error.code, "invalid_definition_source");
            assert!(error.message.contains(allowed), "{slug}: {error}");
        }
        let mut mission = Mission::parse(&sanitized).unwrap();
        // The recorded fixture uses Unix absolute paths. Preserve its existing,
        // missing and relative cases using paths native to the test host.
        if slug == "cards" {
            let existing = std::env::temp_dir();
            let missing = existing.join(format!("ahjo-135-absent-{}", std::process::id()));
            assert!(!missing.exists());
            for files in mission
                .input_cards
                .iter_mut()
                .map(|c| &mut c.files)
                .chain(
                    mission
                        .steps
                        .iter_mut()
                        .filter_map(|f| f.brief.as_mut().map(|b| &mut b.files)),
                )
                .chain(mission.gates.iter_mut().map(|g| &mut g.files))
            {
                for file in files {
                    if file == "/" {
                        *file = existing.to_string_lossy().into_owned();
                    } else if file.starts_with("/nonexistent/") {
                        *file = missing
                            .join(file.trim_start_matches('/'))
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            }
        }
        let report = validate(&mission, &agents, &harnesses);
        let mut sorted = archon.errors.clone();
        // Those findings now fail decoding, which is checked above.
        let decoded = match slug.as_str() {
            "cards" => vec![("invalid_human_channel", "inp_one")],
            "inputs" => vec![
                ("invalid_mission_input", "inp"),
                ("invalid_mission_output", "inp"),
                ("invalid_port_kind", "fmn_a"),
            ],
            "limits" => vec![("invalid_limit", "lim_warn_late")],
            _ => vec![],
        };
        for (code, node) in decoded {
            let index = sorted
                .iter()
                .position(|pair| pair.0 == code && pair.1 == node)
                .unwrap_or_else(|| panic!("{slug}: missing recorded finding {code} on {node}"));
            sorted.remove(index);
        }
        sorted.sort();
        // Ahjo deliberately refuses peer steps that Archon accepts.
        let (unsupported, comparable): (Vec<_>, Vec<_>) = report
            .errors
            .iter()
            .cloned()
            .partition(|f| f.code == ahjo_mission_model::Code::UnsupportedStep);
        if slug == "steps" {
            assert_eq!(
                located(&unsupported),
                [
                    ("unsupported_step".into(), "fmn_peer".into()),
                    ("unsupported_step".into(), "fmn_staff".into())
                ]
            );
        } else {
            assert!(unsupported.is_empty(), "{slug}");
        }
        assert_eq!(located(&comparable), sorted, "{slug} errors");
        let mut sorted = archon.warnings.clone();
        sorted.sort();
        assert_eq!(located(&report.warnings), sorted, "{slug} warnings");
    }
}

/// Archon's own examples load and validate clean, in the copies Archon ships.
#[test]
fn archons_examples_load_and_validate_clean() {
    let parity = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let store = Store::new(&parity);
    let scratch = Scratch::agents(&parity);
    let agents = Store::new(&scratch.0).agents();
    for slug in ["delivery", "operations-proof"] {
        let mission = store.read_mission(slug).unwrap();
        assert_eq!(mission.input_cards.len(), 1);
        let report = validate(&mission, &agents, &Harnesses::default());
        assert_eq!(report, Default::default(), "{slug}");
    }
}
