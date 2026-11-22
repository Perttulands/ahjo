//! The scheduled commands, each an MCP tool and an `ahjo scheduled <verb>`:
//! list, show, create, edit, copy, pause, resume, run and delete, as the
//! Scheduled view lists, makes, edits, pauses, resumes, runs and deletes
//! them.

use std::sync::Arc;

use ahjo_core::{Error, NodeId};
use ahjo_node_kit::{Command, Effect};
use ahjo_scheduled_contract::commands::{
    ScheduledCopy, ScheduledCreate, ScheduledDelete, ScheduledDeleted, ScheduledEdit,
    ScheduledList, ScheduledListed, ScheduledPause, ScheduledRan, ScheduledResume, ScheduledRun,
    ScheduledShow, ScheduledSummary, ScheduledTask,
};
use ahjo_scheduled_contract::{BAD_REQUEST, NOT_FOUND, Schedule, Target};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

use crate::service::{Create, Patch, Scheduled};

/// Who the task's audit says made a change.
const ACTOR: &str = "ahjo";

/// What the commands reach.
pub(crate) struct Ctx {
    pub node: NodeId,
    pub scheduled: Arc<Scheduled>,
}

fn command<Input, Output, F, Fut>(
    ctx: &Arc<Ctx>,
    name: &'static str,
    effect: Effect,
    call: F,
) -> Command
where
    Input: DeserializeOwned + JsonSchema,
    Output: Serialize + JsonSchema,
    F: Fn(Arc<Ctx>, Input) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Output, Error>> + Send + 'static,
{
    let ctx = ctx.clone();
    Command::typed(name, effect, move |args| call(ctx.clone(), args))
        .with_error_status(crate::routes::failure_status)
}

/// Every scheduled command.
pub(crate) fn commands(ctx: &Arc<Ctx>) -> Vec<Command> {
    vec![
        command(
            ctx,
            "scheduled_list",
            Effect::Reads,
            |ctx, _: ScheduledList| async move { list(&ctx) },
        ),
        command(
            ctx,
            "scheduled_show",
            Effect::Reads,
            |ctx, args: ScheduledShow| async move {
                let mut said = [ctx.scheduled.get(&task_id(&ctx, &args.id)?)?];
                ctx.scheduled.with_replaced(&mut said).await;
                let [task] = said;
                Ok(ScheduledTask {
                    node: ctx.node.clone(),
                    task,
                })
            },
        ),
        command(ctx, "scheduled_create", Effect::Writes, create),
        command(ctx, "scheduled_edit", Effect::Sets, edit),
        command(ctx, "scheduled_copy", Effect::Writes, copy),
        command(
            ctx,
            "scheduled_pause",
            Effect::Sets,
            |ctx, args: ScheduledPause| async move {
                let task = ctx.scheduled.pause(&task_id(&ctx, &args.id)?, ACTOR)?;
                Ok(ScheduledTask {
                    node: ctx.node.clone(),
                    task,
                })
            },
        ),
        command(
            ctx,
            "scheduled_resume",
            Effect::Sets,
            |ctx, args: ScheduledResume| async move {
                let task = ctx
                    .scheduled
                    .resume(&task_id(&ctx, &args.id)?, ACTOR)
                    .await?;
                Ok(ScheduledTask {
                    node: ctx.node.clone(),
                    task,
                })
            },
        ),
        command(
            ctx,
            "scheduled_run",
            Effect::Writes,
            |ctx, args: ScheduledRun| async move {
                let (task, run) = ctx
                    .scheduled
                    .run_now(&task_id(&ctx, &args.id)?, ACTOR)
                    .await?;
                Ok(ScheduledRan {
                    task,
                    run,
                    node: ctx.node.clone(),
                })
            },
        ),
        command(
            ctx,
            "scheduled_delete",
            Effect::Destroys,
            |ctx, args: ScheduledDelete| async move {
                let deleted = task_id(&ctx, &args.id)?;
                ctx.scheduled.delete(&deleted)?;
                Ok(ScheduledDeleted {
                    deleted,
                    node: ctx.node.clone(),
                })
            },
        ),
    ]
}

fn list(ctx: &Ctx) -> Result<ScheduledListed, Error> {
    let tasks = ctx
        .scheduled
        .list()?
        .into_iter()
        .map(|task| ScheduledSummary {
            id: task.id,
            name: task.name,
            sessions: task.targets.iter().map(Target::label).collect(),
            schedule: task.schedule,
            paused: task.paused,
            held: task.held,
            next_run: task.next_run,
            last_status: task.last_status,
        })
        .collect();
    Ok(ScheduledListed {
        node: ctx.node.clone(),
        tasks,
    })
}

/// The id of the task an argument names: its id, or the name only one
/// task has.
fn task_id(ctx: &Ctx, given: &str) -> Result<String, Error> {
    let tasks = ctx.scheduled.list()?;
    if tasks.iter().any(|t| t.id == given) {
        return Ok(given.to_owned());
    }
    let named: Vec<_> = tasks.iter().filter(|t| t.name == given).collect();
    match named[..] {
        [one] => Ok(one.id.clone()),
        [] => Err(Error::new(
            NOT_FOUND,
            format!(
                "scheduled task not found: none has the id or name {given:?}; `ahjo scheduled list` shows them"
            ),
        )),
        _ => Err(Error::new(
            BAD_REQUEST,
            format!(
                "{} tasks are named {given:?}: give one's id ({})",
                named.len(),
                named
                    .iter()
                    .map(|t| t.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

async fn create(ctx: Arc<Ctx>, args: ScheduledCreate) -> Result<ScheduledTask, Error> {
    let create = Create {
        name: args.name,
        prompt: args.prompt,
        targets: targets(args.sessions),
        schedule: schedule(args.every, args.cron, args.timezone)?
            .ok_or_else(|| Error::new(BAD_REQUEST, "every or cron is required"))?,
        enabled: Some(true),
        paused: args.paused.unwrap_or_default(),
        created_by: ACTOR.into(),
        updated_by: ACTOR.into(),
    };
    let task = ctx.scheduled.create(create).await?;
    Ok(ScheduledTask {
        node: ctx.node.clone(),
        task,
    })
}

async fn edit(ctx: Arc<Ctx>, args: ScheduledEdit) -> Result<ScheduledTask, Error> {
    let id = task_id(&ctx, &args.id)?;
    let patch = Patch {
        name: args.name,
        prompt: args.prompt,
        targets: args.sessions.map(targets),
        schedule: schedule(args.every, args.cron, args.timezone)?,
        updated_by: ACTOR.into(),
        ..Default::default()
    };
    let task = ctx.scheduled.patch(&id, patch).await?;
    Ok(ScheduledTask {
        node: ctx.node.clone(),
        task,
    })
}

async fn copy(ctx: Arc<Ctx>, args: ScheduledCopy) -> Result<ScheduledTask, Error> {
    let from = ctx.scheduled.get(&task_id(&ctx, &args.id)?)?;
    let create = Create {
        name: args.name.unwrap_or_else(|| format!("{} (copy)", from.name)),
        prompt: from.prompt,
        targets: from.targets,
        schedule: from.schedule,
        enabled: Some(true),
        paused: true,
        created_by: ACTOR.into(),
        updated_by: ACTOR.into(),
    };
    let task = ctx.scheduled.create(create).await?;
    Ok(ScheduledTask {
        node: ctx.node.clone(),
        task,
    })
}

fn targets(sessions: Vec<String>) -> Vec<Target> {
    sessions
        .into_iter()
        .map(|s| {
            let (unix_user, session_name) = s.split_once('/').unwrap_or(("", &s));
            Target {
                session_name: session_name.into(),
                unix_user: unix_user.into(),
            }
        })
        .collect()
}

fn schedule(
    every: Option<i64>,
    cron: Option<String>,
    timezone: Option<String>,
) -> Result<Option<Schedule>, Error> {
    let timezone = timezone.unwrap_or_default();
    Ok(match (every, cron) {
        (Some(_), Some(_)) => return Err(Error::new(BAD_REQUEST, "give every or cron, not both")),
        (Some(every_minutes), None) => Some(Schedule {
            kind: "interval".into(),
            every_minutes,
            timezone,
            ..Default::default()
        }),
        (None, Some(expression)) => Some(Schedule {
            kind: "cron".into(),
            expression,
            timezone,
            ..Default::default()
        }),
        (None, None) => None,
    })
}
