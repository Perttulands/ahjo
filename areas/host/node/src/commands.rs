//! The host and service commands shared by the app, CLI and MCP.

use std::{sync::Arc, time::Duration};

use ahjo_core::{Error, TypedCommand};
use ahjo_host_contract::{
    Outcome, SERVICE_FAILED, SERVICE_UNCONFIRMED,
    commands::{
        HostShow, ServiceActed, ServiceList, ServiceLog, ServiceLogged, ServiceRestart,
        ServiceShow, ServiceShown, ServiceStart, ServiceStop, ServicesListed,
    },
};
use ahjo_node_kit::{Command, Effect, events::Log};
use axum::http::StatusCode;
use schemars::JsonSchema;

use crate::host::{
    self,
    services::{LOG_LINES, Services},
};

struct Ctx {
    services: Arc<Services>,
    events: Arc<Log>,
    node: String,
}

fn command<C, F, Fut>(ctx: &Arc<Ctx>, effect: Effect, handler: F) -> Command
where
    C: TypedCommand + JsonSchema,
    C::Output: JsonSchema,
    F: Fn(Arc<Ctx>, C) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<C::Output, Error>> + Send + 'static,
{
    let ctx = ctx.clone();
    Command::for_request(effect, move |args| handler(ctx.clone(), args))
}

pub(crate) fn commands(services: Arc<Services>, events: Arc<Log>, node: String) -> Vec<Command> {
    let ctx = Arc::new(Ctx {
        services,
        events,
        node,
    });
    vec![
        command(&ctx, Effect::Reads, |ctx, args: HostShow| async move {
            Ok(host::now(
                Duration::from_millis(500),
                &ctx.events,
                ctx.node.clone(),
                args.full.unwrap_or_default(),
            )
            .await)
        }),
        command(&ctx, Effect::Reads, |ctx, _: ServiceList| async move {
            let services = ctx.services.list().await?;
            Ok(ServicesListed {
                node: ctx.node.clone(),
                services,
            })
        }),
        command(&ctx, Effect::Reads, |ctx, args: ServiceShow| async move {
            let service = ctx.services.one(&args.id).await?;
            Ok(ServiceShown {
                node: ctx.node.clone(),
                service,
            })
        }),
        command(&ctx, Effect::Sets, |ctx, args: ServiceStart| {
            act(ctx, args.id, "start")
        })
        .with_error_status(action_status),
        command(&ctx, Effect::Sets, |ctx, args: ServiceStop| {
            act(ctx, args.id, "stop")
        })
        .with_error_status(action_status),
        command(&ctx, Effect::Writes, |ctx, args: ServiceRestart| {
            act(ctx, args.id, "restart")
        })
        .with_error_status(action_status),
        command(&ctx, Effect::Reads, |ctx, args: ServiceLog| async move {
            let log = ctx
                .services
                .log(&args.id, args.lines.unwrap_or(LOG_LINES))
                .await?;
            Ok(ServiceLogged {
                node: ctx.node.clone(),
                log,
            })
        }),
    ]
}

async fn act(ctx: Arc<Ctx>, id: String, action: &'static str) -> Result<ServiceActed, Error> {
    let (service, errors, outcome) = ctx.services.act(&id, action).await?;
    let code = match outcome {
        Outcome::Failed => SERVICE_FAILED,
        Outcome::Unconfirmed => SERVICE_UNCONFIRMED,
        Outcome::Done => {
            return Ok(ServiceActed {
                node: ctx.node.clone(),
                service,
                errors,
                outcome,
            });
        }
    };
    Err(Error::new(code, errors.join("; ")))
}

fn action_status(error: &Error) -> Option<StatusCode> {
    match error.code.as_str() {
        SERVICE_FAILED => Some(StatusCode::BAD_GATEWAY),
        SERVICE_UNCONFIRMED => Some(StatusCode::GATEWAY_TIMEOUT),
        _ => None,
    }
}
