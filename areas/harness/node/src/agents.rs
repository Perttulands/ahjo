//! Agent commands use the mission engine's shared agent store.

use std::{path::PathBuf, sync::Arc};

use ahjo_core::Error;
use ahjo_harness_contract::agents::{
    CreateAgent, DeleteAgent, DeletedAgent, EditAgent, EditedAgent, ListAgents, ListedAgents,
    ShowAgent, ShownAgent,
};
use ahjo_mission_engine::{AgentStore, Harnesses};
use ahjo_mission_model::Agent;
use ahjo_node_kit::{Command, Effect};

pub struct Service {
    store: AgentStore,
    codex_home: Option<PathBuf>,
}

impl Service {
    pub fn new(root: PathBuf, codex_home: Option<PathBuf>) -> Result<Self, Error> {
        let store = AgentStore::new(root.join("agents"));
        store.upgrade()?;
        Ok(Self { store, codex_home })
    }

    fn list(&self, node: String) -> Result<ListedAgents, Error> {
        let (agents, unreadable) = self.store.list()?;
        let harnesses = self
            .codex_home
            .as_deref()
            .map_or_else(Harnesses::from_host, Harnesses::from_codex_home);
        Ok(ListedAgents {
            node,
            agents,
            unreadable,
            harnesses: harnesses.catalog(),
            effort_policy: Harnesses::effort_policy(),
        })
    }

    fn show(&self, input: ShowAgent, node: String) -> Result<ShownAgent, Error> {
        let edit = self.store.agent_edit(&input.agent)?;
        let usage = self.store.usage(&input.agent)?;
        Ok(ShownAgent {
            node,
            agent: edit.agent,
            etag: edit.etag,
            usage,
        })
    }

    fn create(&self, input: CreateAgent, node: String) -> Result<EditedAgent, Error> {
        let edit = self.store.create(Agent {
            schema: 1,
            id: input.id,
            name: input.name,
            instructions: input.instructions,
            harness: input.harness,
            model: input.model,
            effort: input.effort,
            notes: Vec::new(),
        })?;
        Ok(EditedAgent {
            node,
            agent: edit.agent,
            etag: edit.etag,
        })
    }

    fn edit(&self, input: EditAgent, node: String) -> Result<EditedAgent, Error> {
        let current = self.store.agent_edit(&input.agent)?;
        let etag = input.etag.as_deref().unwrap_or(&current.etag);
        let mut agent = current.agent;
        if let Some(name) = input.name {
            agent.name = name;
        }
        if let Some(instructions) = input.instructions {
            agent.instructions = instructions;
        }
        if let Some(harness) = input.harness {
            agent.harness = harness;
        }
        if let Some(model) = input.model {
            agent.model = model;
        }
        if let Some(effort) = input.effort {
            agent.effort = effort;
        }
        if let Some(notes) = input.notes {
            agent.notes = notes;
        }
        let edit = self.store.edit(&input.agent, etag, agent)?;
        Ok(EditedAgent {
            node,
            agent: edit.agent,
            etag: edit.etag,
        })
    }

    fn delete(&self, input: DeleteAgent, node: String) -> Result<DeletedAgent, Error> {
        let current = self.store.agent_edit(&input.agent)?;
        let etag = input.etag.as_deref().unwrap_or(&current.etag);
        self.store.delete(&input.agent, etag)?;
        Ok(DeletedAgent {
            node,
            agent: input.agent,
        })
    }
}

pub fn commands(service: Arc<Service>, node: String) -> Vec<Command> {
    let list_service = service.clone();
    let list_node = node.clone();
    let list = Command::for_request(Effect::Reads, move |_: ListAgents| {
        let service = list_service.clone();
        let node = list_node.clone();
        async move { service.list(node) }
    });
    let show_service = service.clone();
    let show_node = node.clone();
    let show = Command::for_request(Effect::Reads, move |input: ShowAgent| {
        let service = show_service.clone();
        let node = show_node.clone();
        async move { service.show(input, node) }
    });
    let create_service = service.clone();
    let create_node = node.clone();
    let create = Command::for_request(Effect::Writes, move |input: CreateAgent| {
        let service = create_service.clone();
        let node = create_node.clone();
        async move { service.create(input, node) }
    });
    let edit_service = service.clone();
    let edit_node = node.clone();
    let edit = Command::for_request(Effect::Sets, move |input: EditAgent| {
        let service = edit_service.clone();
        let node = edit_node.clone();
        async move { service.edit(input, node) }
    });
    let delete = Command::for_request(Effect::Destroys, move |input: DeleteAgent| {
        let service = service.clone();
        let node = node.clone();
        async move { service.delete(input, node) }
    });
    vec![list, show, create, edit, delete]
}
