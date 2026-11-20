use crate::brain::{Brain, Refusal, refusal_code};
use ahjo_brain_contract::{
    BAD_REQUEST, BRAIN_ERROR, NOT_FOUND, UNAVAILABLE,
    commands::{
        BrainChange, BrainChanges, BrainChangesResult, BrainCollection, BrainCollectionResult,
        BrainCollections, BrainCollectionsResult, BrainFiles, BrainFilesResult, BrainPage,
        BrainRead, BrainSave, BrainSaved, BrainSearch, BrainSearchResults, BrainSend, BrainSent,
        BrainTexts, BrainTextsResult,
    },
};
use ahjo_core::{Error, TypedCommand};
use ahjo_node_kit::{Command, Effect};
use ahjo_term::Sessions;
use schemars::JsonSchema;
use std::sync::Arc;

struct Ctx {
    brain: Arc<Brain>,
    node: String,
    sessions: Option<Arc<dyn Sessions>>,
}

fn command<Input, F, Fut>(ctx: &Arc<Ctx>, effect: Effect, handler: F) -> Command
where
    Input: TypedCommand + JsonSchema,
    Input::Output: JsonSchema,
    F: Fn(Arc<Ctx>, Input) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Input::Output, Error>> + Send + 'static,
{
    let ctx = ctx.clone();
    Command::for_request(effect, move |args| handler(ctx.clone(), args))
}

fn refused(error: Refusal) -> Error {
    Error::new(refusal_code(error.status), error.message)
}

pub fn commands(
    brain: Arc<Brain>,
    node: String,
    sessions: Option<Arc<dyn Sessions>>,
) -> Vec<Command> {
    let ctx = Arc::new(Ctx {
        brain,
        node,
        sessions,
    });
    vec![
        command(&ctx, Effect::Reads, |ctx, args: BrainSearch| async move {
            let results = ctx.brain.search(&args.query).await.map_err(refused)?;
            Ok(BrainSearchResults {
                node: ctx.node.clone(),
                results,
            })
        })
        .with_error_status(|error| {
            (error.code == BRAIN_ERROR).then_some(axum::http::StatusCode::BAD_GATEWAY)
        }),
        command(&ctx, Effect::Reads, |ctx, args: BrainRead| async move {
            let page = ctx.brain.page(&args.path).await.map_err(refused)?;
            Ok(BrainPage {
                node: ctx.node.clone(),
                page,
            })
        }),
        command(&ctx, Effect::Writes, save),
        command(&ctx, Effect::Reads, |ctx, _: BrainCollections| async move {
            let collections = ctx.brain.collections().await.map_err(refused)?;
            Ok(BrainCollectionsResult {
                node: ctx.node.clone(),
                collections,
            })
        }),
        command(&ctx, Effect::Reads, changes),
        command(&ctx, Effect::Reads, |ctx, _: BrainFiles| async move {
            Ok(BrainFilesResult {
                node: ctx.node.clone(),
                files: ctx.brain.files().await.map_err(refused)?,
            })
        }),
        command(&ctx, Effect::Reads, |ctx, args: BrainTexts| async move {
            Ok(BrainTextsResult {
                node: ctx.node.clone(),
                texts: ctx.brain.texts(args.paths).await.map_err(refused)?,
            })
        }),
        command(
            &ctx,
            Effect::Reads,
            |ctx, args: BrainCollection| async move {
                let read = ctx.brain.collection(&args.name).await.map_err(refused)?;
                Ok(BrainCollectionResult {
                    node: ctx.node.clone(),
                    pages: read.found,
                    error: read.error,
                })
            },
        ),
        command(&ctx, Effect::Writes, send),
    ]
}

async fn save(ctx: Arc<Ctx>, args: BrainSave) -> Result<BrainSaved, Error> {
    // Finish the page save and commit even if the caller stops waiting.
    let brain = ctx.brain.clone();
    let task = tokio::spawn(async move {
        brain
            .save(
                &args.path,
                &args.text,
                args.summary.as_deref().unwrap_or(""),
                args.base.as_deref(),
            )
            .await
    });
    let commit = task
        .await
        .map_err(|error| Error::new(BRAIN_ERROR, format!("Save stopped: {error}")))?
        .map_err(refused)?;
    Ok(BrainSaved {
        node: ctx.node.clone(),
        commit,
    })
}

async fn changes(ctx: Arc<Ctx>, args: BrainChanges) -> Result<BrainChangesResult, Error> {
    if args.limit == Some(0) {
        return Err(Error::new(
            BAD_REQUEST,
            "limit must be a positive whole number",
        ));
    }
    let read = ctx.brain.changes(args.limit).await.map_err(refused)?;
    let changes = read
        .found
        .into_iter()
        .map(|change| BrainChange {
            commit: change.commit,
            paths: change
                .pages
                .iter()
                .map(|page| page.path.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            pages: change.pages,
        })
        .collect();
    Ok(BrainChangesResult {
        node: ctx.node.clone(),
        changes,
        error: read.error,
    })
}

async fn send(ctx: Arc<Ctx>, args: BrainSend) -> Result<BrainSent, Error> {
    let page = ctx.brain.page(&args.path).await.map_err(refused)?;
    let sessions = ctx.sessions.as_ref().ok_or_else(|| {
        Error::new(
            UNAVAILABLE,
            "No terminal sessions are available on this node",
        )
    })?;
    let target = args.session;
    let found: Vec<_> = sessions
        .listed()
        .await
        .map_err(|error| Error::new(UNAVAILABLE, format!("{error:#}")))?
        .into_iter()
        .filter(|session| {
            session.id == target
                || (session.user.is_none() && session.name == target)
                || session
                    .user
                    .as_ref()
                    .is_some_and(|user| format!("{user}/{}", session.name) == target)
        })
        .collect();
    let session = match found.as_slice() {
        [session] => session,
        [] => return Err(Error::new(NOT_FOUND, format!("No session {target}"))),
        _ => {
            return Err(Error::new(
                BAD_REQUEST,
                format!("Session {target} is ambiguous"),
            ));
        }
    };
    let error = |error: anyhow::Error| Error::new(BRAIN_ERROR, error.to_string());
    let pane = sessions.active_pane(&session.id).await.map_err(error)?;
    let reference = format!("brain page {}", page.file);
    sessions
        .deliver(pane, reference.clone(), true)
        .await
        .map_err(error)?
        .await
        .map_err(error)?;
    Ok(BrainSent {
        node: ctx.node.clone(),
        session: session.id.clone(),
        reference,
        sent: true,
    })
}
