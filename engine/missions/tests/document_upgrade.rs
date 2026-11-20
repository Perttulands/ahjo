//! Old mission files are upgraded in place, with the original kept until saved.
use ahjo_mission_engine::Store;
use ahjo_mission_model::{LayoutNode, LayoutPatch, MissionChange, NotePatch, Precondition};
use std::path::Path;

const MISSION: &str = r#"# A comment the author wrote
schema = 1
id = "msn_old"
slug = "old"
title = "Old"
rev = 3
updatedBy = "agent:alice" # keep this

[[inputCard]]
id = "inp_start"
title = "Start"
humanChannel = "notify"
inputHint = "The inputCard text is a value"

[[formation]]
id = "fmn_work"
type = "solo"
title = "Work"
[[formation.slot]]
id = "seat_work"
label = "Worker"
agentId = "writer"

[[gate]]
id = "gate_review"
title = "Review"
kinds = ["human", "formation"]
checkVersion = "1"
checkValue = "hello"

[[end]]
id = "end_done"
title = "Done"
outcome = "done"

[[limit]]
id = "lim_work"
title = "Limit"
target = "fmn_work"
rounds = 2
warnSeconds = 10
whenSpent = "fail"

[[connection]]
id = "wire_first"
from = "inp_start:out"
to = "fmn_work:in"
"#;
const LAYOUT: &str = r#"schema = 1
missionId = "msn_old"
missionRev = 3
updatedAt = "2026-01-01T00:00:00Z"
[[node]]
id = "fmn_work"
x = 42
y = 90
"#;
const NOTES: &str = r#"schema = 2
missionId = "msn_old"
rev = 1
updatedAt = "2026-01-01T00:00:00Z"
updatedBy = "human:alice"
[[entry]]
id = "note_one"
target = "fmn_work"
author = "human:alice"
createdAt = "2026-01-01T00:00:00Z"
text = "Keep inputCard unchanged in this text"
"#;

fn fixture() -> (std::path::PathBuf, Store) {
    let root = ahjo_testkit::fresh_dir("document-upgrade");
    for (dir, name, text) in [
        ("missions", "old.mission.toml", MISSION),
        ("layout", "old.layout.toml", LAYOUT),
        ("notes", "old.notes.toml", NOTES),
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join(name), text).unwrap();
    }
    (root.clone(), Store::new(root))
}

fn backup(path: &Path) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    name.into()
}

#[test]
fn old_documents_keep_data_comments_and_backups_until_their_next_save() {
    let (root, store) = fixture();
    let mission_path = root.join("missions/old.mission.toml");
    let layout_path = root.join("layout/old.layout.toml");
    let notes_path = root.join("notes/old.notes.toml");
    let mission = store.mission_edit("old").unwrap();
    let layout = store.layout_edit("old").unwrap();
    let notes = store.notes_edit("old").unwrap();
    assert_eq!(mission.mission.schema, 4);
    assert_eq!(
        mission.mission.input_cards[0].input_hint,
        "The inputCard text is a value"
    );
    assert_eq!(mission.mission.steps[0].seats[0].agent, "writer");
    assert_eq!(mission.mission.gates[0].check_value, "hello");
    assert_eq!(
        mission.mission.gates[0].kinds,
        [
            ahjo_mission_model::GateKind::Human,
            ahjo_mission_model::GateKind::Step
        ]
    );
    assert_eq!(mission.mission.limits[0].warn_seconds, Some(10));
    assert_eq!(layout.layout.schema, 2);
    assert_eq!(layout.layout.nodes[0].x, 42);
    assert_eq!(notes.notes.schema, 3);
    assert_eq!(
        notes.notes.entries[0].text,
        "Keep inputCard unchanged in this text"
    );
    for (path, original) in [
        (&mission_path, MISSION),
        (&layout_path, LAYOUT),
        (&notes_path, NOTES),
    ] {
        assert_eq!(std::fs::read_to_string(backup(path)).unwrap(), original);
    }
    let migrated = std::fs::read_to_string(&mission_path).unwrap();
    assert!(
        migrated.contains("# A comment the author wrote"),
        "{migrated}"
    );
    assert!(migrated.contains("# keep this"), "{migrated}");

    let stale = store
        .patch(
            "old",
            Precondition {
                rev: 2,
                etag: mission.etag.clone(),
            },
            MissionChange::Title("stale".into()),
            "agent:test",
        )
        .unwrap_err();
    assert_eq!(stale.code, "conflict");
    assert!(backup(&mission_path).exists());

    store
        .patch(
            "old",
            Precondition {
                rev: mission.mission.rev,
                etag: mission.etag,
            },
            MissionChange::Title("Changed".into()),
            "agent:test",
        )
        .unwrap();
    assert!(!backup(&mission_path).exists());
    assert!(backup(&layout_path).exists());
    assert!(backup(&notes_path).exists());
    store
        .patch_layout(
            "old",
            &layout.etag,
            LayoutPatch::Nodes(vec![LayoutNode {
                id: "fmn_work".into(),
                x: 100,
                y: 200,
            }]),
        )
        .unwrap();
    assert!(!backup(&layout_path).exists());
    store
        .patch_notes(
            "old",
            &notes.etag,
            NotePatch::Append {
                target: "mission".into(),
                author: "agent:test".into(),
                text: "Answer".into(),
            },
        )
        .unwrap();
    assert!(!backup(&notes_path).exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_upgrade_keeps_the_original_schema_one_backup() {
    use sha2::{Digest, Sha256};
    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    let schema_two = r#"# An original comment
schema = 2
id = "msn_old"
slug = "old"
title = "formation is the topic"
rev = 3
input_card = [{ id = "inp_start", title = "formation" }]
formation = [{ id = "fmn_work", type = "solo", title = "formation", brief = { goal = "formation" } }]
gate = [{ id = "gate_review", kinds = ["human", "formation"], criterion = "formation" }]
end = [{ id = "end_done", outcome = "done" }]
connection = [{ id = "edge", from = "fmn_work:out", to = "end_done:in" }]
"#;
    std::fs::write(&path, schema_two).unwrap();
    std::fs::write(backup(&path), MISSION).unwrap();
    let mut marker = backup(&path).into_os_string();
    marker.push(".ahjo-migration");
    std::fs::write(&marker, hex::encode(Sha256::digest(MISSION.as_bytes()))).unwrap();

    let mission = store.mission_edit("old").unwrap();
    assert_eq!(mission.mission.schema, 4);
    assert_eq!(mission.mission.steps[0].id, "fmn_work");
    assert_eq!(
        mission.mission.steps[0].formation,
        ahjo_mission_model::Formation::Solo
    );
    assert_eq!(mission.mission.steps[0].title, "formation");
    assert_eq!(
        mission.mission.steps[0].brief.as_ref().unwrap().goal,
        "formation"
    );
    assert_eq!(
        mission.mission.gates[0].kinds,
        [
            ahjo_mission_model::GateKind::Human,
            ahjo_mission_model::GateKind::Step
        ]
    );
    assert_eq!(mission.mission.gates[0].criterion, "formation");
    assert_eq!(std::fs::read_to_string(backup(&path)).unwrap(), MISSION);
    let migrated = std::fs::read_to_string(&path).unwrap();
    assert!(migrated.contains("# An original comment"), "{migrated}");
    assert!(migrated.contains("step ="), "{migrated}");

    store
        .patch(
            "old",
            Precondition {
                rev: mission.mission.rev,
                etag: mission.etag,
            },
            MissionChange::Title("Saved".into()),
            "agent:test",
        )
        .unwrap();
    assert!(!backup(&path).exists());
    assert!(!std::path::Path::new(&marker).exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_readers_upgrade_once_and_keep_the_original_backup() {
    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..8 {
            let store = store.clone();
            let barrier = barrier.clone();
            workers.push(scope.spawn(move || {
                barrier.wait();
                store.read_mission("old").unwrap()
            }));
        }
        for worker in workers {
            let mission = worker.join().unwrap();
            assert_eq!(mission.schema, 4);
            assert_eq!(mission.id, "msn_old");
        }
    });
    assert_eq!(std::fs::read_to_string(backup(&path)).unwrap(), MISSION);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("schema = 4")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_or_ambiguous_old_data_never_overwrites_the_original() {
    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    let original = MISSION.replace(
        "humanChannel = \"notify\"",
        "humanChannel = \"notify\"\nhuman_channel = \"session\"",
    );
    std::fs::write(&path, &original).unwrap();
    let error = store.read_mission("old").unwrap_err();
    assert_eq!(error.code, "invalid_definition_source");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(!backup(&path).exists());

    let original = MISSION.replace("humanChannel = \"notify\"", "unknownField = \"lost\"");
    std::fs::write(&path, &original).unwrap();
    assert_eq!(
        store.read_mission("old").unwrap_err().code,
        "invalid_definition_source"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(!backup(&path).exists());
    let original = MISSION.replace(
        r#"kinds = ["human", "formation"]"#,
        r#"kinds = ["human", "formation", "step"]"#,
    );
    std::fs::write(&path, &original).unwrap();
    assert_eq!(
        store.read_mission("old").unwrap_err().code,
        "invalid_definition_source"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(!backup(&path).exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_existing_different_backup_is_never_overwritten() {
    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    std::fs::write(backup(&path), "someone else's backup").unwrap();
    let error = store.read_mission("old").unwrap_err();
    assert_eq!(error.code, "conflict");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), MISSION);
    assert_eq!(
        std::fs::read_to_string(backup(&path)).unwrap(),
        "someone else's backup"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_preexisting_unrelated_backup_survives_normal_saves() {
    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    let mission = store.mission_edit("old").unwrap();
    store
        .patch(
            "old",
            Precondition {
                rev: mission.mission.rev,
                etag: mission.etag,
            },
            MissionChange::Title("Saved".into()),
            "agent:test",
        )
        .unwrap();
    std::fs::write(backup(&path), "personal backup").unwrap();
    let current = store.mission_edit("old").unwrap();
    store
        .patch(
            "old",
            Precondition {
                rev: current.mission.rev,
                etag: current.etag,
            },
            MissionChange::Title("Saved again".into()),
            "agent:test",
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(backup(&path)).unwrap(),
        "personal backup"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_documents_keep_the_link_and_backup_beside_the_target() {
    use std::os::unix::fs::symlink;
    let (root, store) = fixture();
    let link = root.join("missions/old.mission.toml");
    let target = root.join("outside.mission.toml");
    std::fs::rename(&link, &target).unwrap();
    symlink(&target, &link).unwrap();
    let mission = store.mission_edit("old").unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(backup(&target)).unwrap(), MISSION);
    store
        .patch(
            "old",
            Precondition {
                rev: mission.mission.rev,
                etag: mission.etag,
            },
            MissionChange::Title("Linked".into()),
            "agent:test",
        )
        .unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!backup(&target).exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_staffing_keeps_only_settings_that_differ_from_the_agent() {
    use ahjo_mission_model::{AGENT_SCHEMA, Agent, Effort, HARNESS_DEFAULT_MODEL, Harness};

    let (root, store) = fixture();
    let path = root.join("missions/old.mission.toml");
    store
        .agents()
        .create(Agent {
            schema: AGENT_SCHEMA,
            id: "writer".into(),
            name: "Writer".into(),
            instructions: "Keep the evidence.".into(),
            harness: Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: Effort::Ultra,
            ..Agent::default()
        })
        .unwrap();
    let old = r#"schema = 3
id = "msn_old"
slug = "old"
title = "Old"
rev = 3

[[step]]
id = "fmn_work"
title = "Write"
formation = "peer"

[[step.slot]]
id = "original"
label = "Writer"
agent = "ignored-name"
agent_id = "writer"
harness = "openai-codex"
model = "gpt-6"
effort = "ultra"

[[step.slot]]
id = "override"
label = "Reviewer"
agent = "ignored-reviewer"
agent_id = "writer"
harness = "openai-codex"
model = ""
effort = "high"
"#;
    std::fs::write(&path, old).unwrap();

    let mission = store.read_mission("old").unwrap();
    let step = &mission.steps[0];
    let inherited = &step.seats[0];
    assert_eq!(inherited.id, "original");
    assert_eq!(inherited.agent, "writer");
    assert!(inherited.harness.is_unset());
    assert!(inherited.model.is_empty());
    assert!(inherited.effort.is_unset());
    let changed = &step.seats[1];
    assert_eq!(changed.id, "override");
    assert_eq!(changed.agent, "writer");
    assert!(changed.harness.is_unset());
    assert_eq!(changed.model, HARNESS_DEFAULT_MODEL);
    assert_eq!(changed.effort, Effort::High);

    let updated = std::fs::read_to_string(&path).unwrap();
    assert!(updated.contains("[[step.seat]]"));
    assert!(!updated.contains("ignored"));
    assert!(!updated.contains("harness ="));
    assert!(!updated.contains("ultra"));
    assert_eq!(std::fs::read_to_string(backup(&path)).unwrap(), old);
    assert_eq!(store.read_mission("old").unwrap(), mission);
    std::fs::remove_dir_all(root).unwrap();
}
