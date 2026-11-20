use ahjo_mission_engine::AgentStore;
use ahjo_mission_model::{AGENT_SCHEMA, Agent, AgentNote, Effort, Harness};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ahjo-agent-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn agents(&self) -> AgentStore {
        AgentStore::new(self.0.join("agents"))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn agent(id: &str) -> Agent {
    Agent {
        schema: AGENT_SCHEMA,
        id: id.into(),
        name: id.into(),
        harness: Harness::ClaudeCode,
        effort: Effort::Medium,
        ..Default::default()
    }
}

fn role(dir: &Path, id: &str, status: &str) -> String {
    let source = format!(
        "schema = 1\n[card]\nid = {id:?}\ndisplay_name = {id:?}\nkind = \"specialist\"\nsummary = \"Keep the evidence\"\ntags = [\"review\"]\nstatus = {status:?}\nreviewer_notes = \"Extra instructions\"\n\n[[note]]\nts = \"2026-01-01\"\nactor = \"agent:test\"\ntext = \"A note\"\n"
    );
    std::fs::create_dir_all(dir.join("roles")).unwrap();
    std::fs::write(dir.join("roles").join(format!("{id}.toml")), &source).unwrap();
    source
}

#[test]
fn create_replace_delete_and_validate_agent() {
    let scratch = Scratch::new();
    let store = scratch.agents();
    let original = store.create(agent("writer")).unwrap();
    assert_eq!(
        store.create(original.agent.clone()).unwrap_err().code,
        "already_exists"
    );
    let mut invalid = original.agent.clone();
    invalid.name.clear();
    assert_eq!(
        store
            .edit("writer", &original.etag, invalid)
            .unwrap_err()
            .code,
        "invalid_agent"
    );
    let mut changed = original.agent.clone();
    changed.instructions = "Write".into();
    let saved = store
        .edit("writer", &original.etag, changed.clone())
        .unwrap();
    assert_eq!(saved.agent, changed);
    assert_eq!(store.read("writer").unwrap(), changed);
    store.delete("writer", &saved.etag).unwrap();
    assert_eq!(store.read("writer").unwrap_err().code, "not_found");
    assert!(scratch.0.join("agents/archive/writer.toml").exists());
}

#[cfg(unix)]
#[test]
fn edits_follow_a_symlinked_agent_file() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let store = scratch.agents();
    store.upgrade().unwrap();
    let target = scratch.0.join("outside.toml");
    let raw = toml_edit::ser::to_string_pretty(&agent("writer")).unwrap();
    std::fs::write(&target, raw).unwrap();
    let link = scratch.0.join("agents/writer.toml");
    symlink(&target, &link).unwrap();
    let edit = store.agent_edit("writer").unwrap();
    let mut changed = edit.agent;
    changed.instructions = "Keep this".into();
    store.edit("writer", &edit.etag, changed).unwrap();
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::read_to_string(target)
            .unwrap()
            .contains("Keep this")
    );
}

#[test]
fn concurrent_writers_do_not_lose_agents() {
    let scratch = Scratch::new();
    let store = scratch.agents();
    std::thread::scope(|scope| {
        for index in 0..16 {
            let store = store.clone();
            scope.spawn(move || {
                store.create(agent(&format!("agent-{index}"))).unwrap();
            });
        }
    });
    assert_eq!(store.list().unwrap().0.len(), 23);
}

#[test]
fn usage_reports_known_seats_and_unreadable_missions() {
    let scratch = Scratch::new();
    let missions = scratch.0.join("missions");
    std::fs::create_dir_all(&missions).unwrap();
    std::fs::write(
        missions.join("work.mission.toml"),
        r#"
schema = 6
id = "msn_work"
slug = "work"
title = "Work"
rev = 1
[[step]]
id = "fmn_work"
formation = "solo"
title = "Step"
[[step.seat]]
id = "seat_work"
label = "Worker"
agent = "builder"
"#,
    )
    .unwrap();
    std::fs::write(missions.join("broken.mission.toml"), "broken = [").unwrap();
    let report = scratch.agents().usage("builder").unwrap();
    assert!(!report.complete);
    assert_eq!(report.unreadable[0].name, "broken");
    assert_eq!(report.usage.len(), 1);
    assert_eq!(report.usage[0].seat_id, "seat_work");
}

#[test]
fn retired_roles_archive_and_can_be_restored() {
    let scratch = Scratch::new();
    let source = role(&scratch.0, "critic", "retired");
    let store = scratch.agents();
    store.upgrade().unwrap();
    assert_eq!(store.read("critic").unwrap_err().code, "not_found");
    assert!(
        store
            .list()
            .unwrap()
            .0
            .iter()
            .all(|agent| agent.id != "critic")
    );
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/critic.toml")).unwrap(),
        source
    );
    std::fs::rename(
        scratch.0.join("agents/archive/critic.toml"),
        scratch.0.join("agents/critic.toml"),
    )
    .unwrap();
    let restored = store.read("critic").unwrap();
    assert!(restored.instructions.contains("Keep the evidence"));
    assert!(restored.instructions.contains("Extra instructions"));
    assert_eq!(restored.notes[0].text, "A note");
}

#[test]
fn retired_starter_is_not_recreated_and_its_notes_survive() {
    let scratch = Scratch::new();
    role(&scratch.0, "builder", "retired");
    let store = scratch.agents();
    store.upgrade().unwrap();
    assert_eq!(store.read("builder").unwrap_err().code, "not_found");
    assert_eq!(store.list().unwrap().0.len(), 6);
    store.upgrade().unwrap();
    let archived = std::fs::read_to_string(scratch.0.join("agents/archive/builder.toml")).unwrap();
    assert!(archived.contains("A note"));
    assert!(!scratch.0.join("agents/builder.toml").exists());
}

#[test]
fn migration_preserves_extra_card_text() {
    let scratch = Scratch::new();
    let source = role(&scratch.0, "critic", "active");
    let store = scratch.agents();
    let migrated = store.read("critic").unwrap();
    assert!(migrated.instructions.contains("Extra instructions"));
    assert_eq!(
        migrated.notes[0],
        AgentNote {
            at: "2026-01-01".into(),
            by: "agent:test".into(),
            text: "A note".into()
        }
    );
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/critic.toml")).unwrap(),
        source
    );
}

#[test]
fn edits_require_fresh_etag_and_preserve_notes() {
    let scratch = Scratch::new();
    let store = scratch.agents();
    let original = store.create(agent("writer")).unwrap();
    let mut changed = original.agent.clone();
    changed.notes.push(AgentNote {
        at: "today".into(),
        by: "agent:test".into(),
        text: "Did well".into(),
    });
    let saved = store
        .edit("writer", &original.etag, changed.clone())
        .unwrap();
    assert_eq!(
        store
            .edit("writer", &original.etag, original.agent)
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(store.read("writer").unwrap(), changed);
    assert_eq!(
        store.delete("writer", "").unwrap_err().code,
        "precondition_required"
    );
    assert_eq!(
        store.delete("writer", "stale").unwrap_err().code,
        "conflict"
    );
    store.delete("writer", &saved.etag).unwrap();
}

#[test]
fn characters_are_converted_and_the_original_is_backed_up() {
    let scratch = Scratch::new();
    role(&scratch.0, "critic", "active");
    let source = r#"{"schema":2,"characters":[{"id":"reviewer","name":"Reviewer setup","agent_id":"critic","harness":"openai-codex","model":"gpt-6","effort":"ultra","description":"Check carefully","picture":"data:image/png;base64,AA=="}]}"#;
    let path = scratch.0.join("roles/characters.json");
    std::fs::write(&path, source).unwrap();
    let store = scratch.agents();
    store.upgrade().unwrap();
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/characters.json.bak")).unwrap(),
        source
    );
    let setup = store.read("reviewer-character").unwrap();
    assert_eq!(setup.name, "Reviewer setup");
    assert_eq!(setup.harness, Harness::OpenAiCodex);
    assert_eq!(setup.effort, Effort::Ultra);
    assert_eq!(setup.model, "gpt-6");
    assert!(setup.instructions.contains("Keep the evidence"));
    assert!(setup.instructions.contains("Check carefully"));
    store.upgrade().unwrap();
    assert_eq!(store.list().unwrap().0.len(), 9);
}

#[test]
fn deleting_a_starter_archives_it_without_recreating_it() {
    let scratch = Scratch::new();
    let store = scratch.agents();
    let original = store.agent_edit("judge").unwrap();
    store.delete("judge", &original.etag).unwrap();
    assert_eq!(store.read("judge").unwrap_err().code, "not_found");
    assert!(scratch.0.join("agents/archive/judge.toml").exists());
    assert_eq!(
        store.create(original.agent).unwrap_err().code,
        "already_exists"
    );
    std::fs::rename(
        scratch.0.join("agents/archive/judge.toml"),
        scratch.0.join("agents/judge.toml"),
    )
    .unwrap();
    assert!(store.read("judge").is_ok());
}

#[test]
fn conflict_error_keeps_its_code_and_message() {
    let scratch = Scratch::new();
    let store = scratch.agents();
    let original = store.create(agent("writer")).unwrap();
    let error = store.delete("writer", "stale").unwrap_err();
    assert_eq!(error.code, "conflict");
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::json!({"code":"conflict", "message":"agent changed"})
    );
    assert_eq!(store.agent_edit("writer").unwrap().etag, original.etag);
}

#[test]
fn agent_usage_reports_every_seat_in_every_mission() {
    let dir = Scratch::new();
    let store = ahjo_mission_engine::Store::new(&dir.0);
    for (slug, title) in [("one", "One"), ("two", "Two")] {
        let mut edit = store.create_mission(slug, title, "").unwrap();
        edit = store
            .patch(
                slug,
                ahjo_mission_model::Precondition {
                    rev: edit.mission.rev,
                    etag: edit.etag.clone(),
                },
                ahjo_mission_model::MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Orchestrated,
                    title: "Pair".into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
        let step = edit.mission.steps[0].clone();
        for seat in step.seats {
            edit = store
                .patch(
                    slug,
                    ahjo_mission_model::Precondition {
                        rev: edit.mission.rev,
                        etag: edit.etag.clone(),
                    },
                    ahjo_mission_model::MissionChange::StaffSeat {
                        step_id: step.id.clone(),
                        seat_id: seat.id,
                        agent: "builder".into(),
                        harness: ahjo_mission_model::Harness::ClaudeCode,
                        model: "".into(),
                        effort: ahjo_mission_model::Effort::Medium,
                    },
                    "",
                )
                .unwrap();
        }
    }
    let report = store.agents().usage("builder").unwrap();
    assert!(report.complete);
    assert_eq!(report.usage.len(), 6);
    assert_eq!(
        report
            .usage
            .iter()
            .filter(|u| u.mission_slug == "one")
            .count(),
        3
    );
    assert_eq!(
        report
            .usage
            .iter()
            .filter(|u| u.mission_slug == "two")
            .count(),
        3
    );
    assert!(report.usage.iter().all(|u| u.step_title == "Pair"));
    assert!(store.agents().usage("judge").unwrap().usage.is_empty());
}

#[test]
fn an_unreadable_legacy_role_is_reported_without_hiding_others() {
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("roles")).unwrap();
    let source = "schema = [";
    std::fs::write(scratch.0.join("roles/broken.toml"), source).unwrap();
    let store = scratch.agents();
    let (agents, unreadable) = store.list().unwrap();
    assert_eq!(agents.len(), 7);
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].name, "broken");
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/broken.toml")).unwrap(),
        source
    );
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("agents/broken.toml")).unwrap(),
        source
    );
}

#[test]
fn migration_refuses_to_activate_a_retired_role_through_an_existing_agent() {
    let scratch = Scratch::new();
    role(&scratch.0, "builder", "retired");
    std::fs::create_dir_all(scratch.0.join("agents")).unwrap();
    let active = toml_edit::ser::to_string_pretty(&agent("builder")).unwrap();
    std::fs::write(scratch.0.join("agents/builder.toml"), &active).unwrap();
    assert_eq!(scratch.agents().upgrade().unwrap_err().code, "conflict");
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("agents/builder.toml")).unwrap(),
        active
    );
    assert!(!scratch.0.join("agents/.migrated").exists());
}

#[test]
fn legacy_camel_case_character_role_is_resolved() {
    let scratch = Scratch::new();
    role(&scratch.0, "critic", "active");
    let source = r#"{"schema":1,"characters":[{"id":"special","name":"Special","agentId":"critic","harness":"pi","model":"","effort":"high"}]}"#;
    std::fs::write(scratch.0.join("roles/characters.json"), source).unwrap();
    let migrated = scratch.agents().read("special").unwrap();
    assert_eq!(migrated.harness, Harness::Pi);
    assert_eq!(migrated.effort, Effort::High);
    assert!(migrated.instructions.contains("Keep the evidence"));
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/characters.json.bak")).unwrap(),
        source
    );
}

#[test]
fn setups_with_unavailable_roles_are_archived_and_the_backup_survives() {
    let scratch = Scratch::new();
    role(&scratch.0, "retired-one", "retired");
    std::fs::write(scratch.0.join("roles/unreadable.toml"), "schema = [").unwrap();
    let source = r#"{"schema":2,"characters":[{"id":"missing-setup","name":"Missing","agent_id":"gone","harness":"pi","effort":"high"},{"id":"retired-setup","name":"Retired","agent_id":"retired-one","harness":"pi","effort":"high"},{"id":"broken-setup","name":"Broken","agent_id":"unreadable","harness":"pi","effort":"high"}]}"#;
    std::fs::write(scratch.0.join("roles/characters.json"), source).unwrap();
    let store = scratch.agents();
    let (agents, unreadable) = store.list().unwrap();
    assert_eq!(agents.len(), 7);
    assert_eq!(unreadable[0].name, "unreadable");
    for id in ["missing-setup", "retired-setup", "broken-setup"] {
        assert_eq!(store.read(id).unwrap_err().code, "not_found");
        let archived =
            std::fs::read_to_string(scratch.0.join("agents/archive").join(format!("{id}.toml")))
                .unwrap();
        assert!(archived.contains(id));
    }
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/characters.json.bak")).unwrap(),
        source
    );
}

#[test]
fn unreadable_saved_setups_are_reported_without_blocking_the_agent_list() {
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("roles")).unwrap();
    let source = r#"{"schema":2,"characters":["#;
    std::fs::write(scratch.0.join("roles/characters.json"), source).unwrap();
    let (agents, unreadable) = scratch.agents().list().unwrap();
    assert_eq!(agents.len(), 7);
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].name, "characters-unreadable");
    assert!(unreadable[0].reason.contains("characters.json"));
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("roles/characters.json")).unwrap(),
        source
    );
}

#[cfg(unix)]
#[test]
fn a_broken_legacy_role_link_is_reported_without_hiding_other_agents() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("roles")).unwrap();
    let link = scratch.0.join("roles/broken.toml");
    symlink("missing.toml", &link).unwrap();
    let (agents, unreadable) = scratch.agents().list().unwrap();
    assert_eq!(agents.len(), 7);
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].name, "broken");
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
