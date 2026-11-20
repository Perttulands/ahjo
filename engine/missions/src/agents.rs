//! Agent files and their revision-checked edits.
use std::path::{Path, PathBuf};

use ahjo_mission_model::{AGENT_SCHEMA, Agent, AgentUsage, AgentUse, UnreadableAgent};

use crate::{Error, links};

#[derive(Clone, Debug)]
pub struct AgentStore {
    dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct AgentEdit {
    pub agent: Agent,
    pub etag: String,
}

pub fn check_agent_id(id: &str) -> Result<(), Error> {
    if !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(Error::new(
            "invalid_name",
            format!("agent {id:?} must use lowercase letters, digits and dashes"),
        ))
    }
}

impl AgentStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn upgrade(&self) -> Result<(), Error> {
        crate::agents_migration::upgrade(&self.dir)
    }

    fn root(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    fn read_raw(&self, id: &str) -> Result<(Agent, String), Error> {
        check_agent_id(id)?;
        let raw = links::read_definition(&self.dir.join(format!("{id}.toml")))?;
        let agent = parse_agent(id, &raw)?;
        Ok((agent, raw))
    }

    pub fn source(&self, id: &str) -> Result<String, Error> {
        self.upgrade()?;
        self.read_raw(id).map(|(_, raw)| raw)
    }

    pub fn read(&self, id: &str) -> Result<Agent, Error> {
        self.upgrade()?;
        self.read_raw(id).map(|(agent, _)| agent)
    }

    pub fn list(&self) -> Result<(Vec<Agent>, Vec<UnreadableAgent>), Error> {
        self.upgrade()?;
        let mut agents = Vec::new();
        let mut unreadable = Vec::new();
        for name in links::list_names(&self.dir, ".toml")? {
            match self.read_raw(&name) {
                Ok((agent, _)) => agents.push(agent),
                Err(error) => unreadable.push(UnreadableAgent {
                    name,
                    reason: error.to_string(),
                }),
            }
        }
        Ok((agents, unreadable))
    }

    pub fn agent_edit(&self, id: &str) -> Result<AgentEdit, Error> {
        self.upgrade()?;
        let locks = crate::write::locks(self.root(), &format!("agent:{id}"));
        let _guard = locks
            .document
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (agent, raw) = self.read_raw(id)?;
        Ok(AgentEdit {
            agent,
            etag: crate::authoring::etag(&raw),
        })
    }

    pub fn create(&self, agent: Agent) -> Result<AgentEdit, Error> {
        self.upgrade()?;
        check_agent_id(&agent.id)?;
        self.validate(&agent)?;
        let locks = crate::write::locks(self.root(), &format!("agent:{}", agent.id));
        let _guard = locks
            .document
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let path = self.dir.join(format!("{}.toml", agent.id));
        let archive = self.dir.join("archive").join(format!("{}.toml", agent.id));
        if std::fs::symlink_metadata(&path).is_ok() || std::fs::symlink_metadata(&archive).is_ok() {
            return Err(Error::new(
                "already_exists",
                "agent already exists or is archived",
            ));
        }
        let raw = encode_agent(&agent)?;
        crate::write::atomic(&path, raw.as_bytes()).map_err(crate::error::io)?;
        Ok(AgentEdit {
            agent,
            etag: crate::authoring::etag(&raw),
        })
    }

    pub fn edit(&self, id: &str, etag: &str, agent: Agent) -> Result<AgentEdit, Error> {
        self.upgrade()?;
        self.validate(&agent)?;
        if agent.id != id {
            return Err(Error::new("invalid_agent", "agent id cannot change"));
        }
        let locks = crate::write::locks(self.root(), &format!("agent:{id}"));
        let _guard = locks
            .document
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (current, raw) = self.read_raw(id)?;
        check_etag(etag, &raw)?;
        if current == agent {
            return Ok(AgentEdit {
                agent,
                etag: crate::authoring::etag(&raw),
            });
        }
        let raw = encode_agent(&agent)?;
        crate::write::atomic(&self.dir.join(format!("{id}.toml")), raw.as_bytes())
            .map_err(crate::error::io)?;
        Ok(AgentEdit {
            agent,
            etag: crate::authoring::etag(&raw),
        })
    }

    pub fn delete(&self, id: &str, etag: &str) -> Result<(), Error> {
        self.upgrade()?;
        let locks = crate::write::locks(self.root(), &format!("agent:{id}"));
        let _references = locks
            .references
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let _guard = locks
            .document
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (_, raw) = self.read_raw(id)?;
        check_etag(etag, &raw)?;
        let archive = self.dir.join("archive");
        std::fs::create_dir_all(&archive).map_err(crate::error::io)?;
        let target = archive.join(format!("{id}.toml"));
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(Error::new("already_exists", "agent archive already exists"));
        }
        std::fs::rename(self.dir.join(format!("{id}.toml")), target).map_err(crate::error::io)
    }

    fn validate(&self, agent: &Agent) -> Result<(), Error> {
        check_agent_id(&agent.id)?;
        if agent.schema != AGENT_SCHEMA || agent.name.trim().is_empty() {
            return Err(Error::new(
                "invalid_agent",
                "agent needs schema 1 and a name",
            ));
        }
        crate::Harnesses::from_host()
            .check(
                "agent",
                agent.harness.as_str(),
                &agent.model,
                agent.effort.as_str(),
            )
            .map_err(|error| Error::new("invalid_agent", error.message))
    }

    pub fn usage(&self, id: &str) -> Result<AgentUsage, Error> {
        self.upgrade()?;
        check_agent_id(id)?;
        let locks = crate::write::locks(self.root(), "agent-references");
        let _guard = locks
            .references
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let store = crate::Store::new(self.root());
        let mut result = AgentUsage {
            complete: true,
            ..Default::default()
        };
        for listing in store.list_missions()? {
            let mission = match store.read_mission(&listing.slug) {
                Ok(mission) => mission,
                Err(error) => {
                    result.unreadable.push(UnreadableAgent {
                        name: listing.slug,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            for step in &mission.steps {
                for seat in &step.seats {
                    if seat.agent == id {
                        result.usage.push(AgentUse {
                            mission_id: mission.id.clone(),
                            mission_slug: listing.slug.clone(),
                            mission_title: mission.title.clone(),
                            step_id: step.id.clone(),
                            step_title: step.title.clone(),
                            seat_id: seat.id.clone(),
                            seat_label: seat.label.clone(),
                        });
                    }
                }
            }
        }
        result.complete = result.unreadable.is_empty();
        Ok(result)
    }
}

pub(crate) fn encode_agent(agent: &Agent) -> Result<String, Error> {
    toml_edit::ser::to_string_pretty(agent)
        .map_err(|error| Error::new("invalid_agent", error.to_string()))
}

fn check_etag(etag: &str, raw: &str) -> Result<(), Error> {
    if etag.is_empty() {
        return Err(Error::new(
            "precondition_required",
            "agent ETag is required",
        ));
    }
    if etag != crate::authoring::etag(raw) {
        return Err(Error::new("conflict", "agent changed"));
    }
    Ok(())
}

pub(crate) fn parse_agent(id: &str, raw: &str) -> Result<Agent, Error> {
    let agent: Agent = toml::from_str(raw).map_err(|error| {
        Error::new(
            "invalid_definition_source",
            format!("agent {id:?}: {error}"),
        )
    })?;
    if agent.schema != AGENT_SCHEMA {
        return Err(Error::new(
            "unsupported_schema",
            format!("agent {id:?} has schema {}", agent.schema),
        ));
    }
    if agent.id != id || agent.name.trim().is_empty() {
        return Err(Error::new(
            "invalid_definition_source",
            format!("agent {id:?} needs its matching id and a name"),
        ));
    }
    Ok(agent)
}
