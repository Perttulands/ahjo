//! Local ownership of agent seats through the terminal engine.
use crate::{
    Claude, Codex, Dispatch, HarnessAdapter, Pi, SeatBinding, SeatId, SeatRecords, Shell,
    TranscriptRoots,
};
use ahjo_term::{SessionSpec, Sessions, waiting::harness::Harness};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LocalSeats {
    engine: Arc<dyn Sessions>,
    records: SeatRecords,
    node: String,
    roots: TranscriptRoots,
    creation: Arc<Mutex<()>>,
    /// Each kept session in use, by name, and the dispatch using it.
    kept: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    pub(super) turns: crate::TurnWorktrees,
    pub(super) running: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(super) runtime: tokio::runtime::Handle,
}
impl LocalSeats {
    pub fn new(
        engine: Arc<dyn Sessions>,
        root: impl Into<PathBuf>,
        node: String,
        roots: TranscriptRoots,
    ) -> Self {
        let root = root.into();
        Self {
            engine,
            turns: crate::TurnWorktrees::new(root.join("turns"), Default::default()),
            records: SeatRecords::new(root),
            node,
            roots,
            creation: Arc::new(Mutex::new(())),
            kept: Default::default(),
            running: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            runtime: tokio::runtime::Handle::current(),
        }
    }
    /// The checkouts this node makes turn worktrees from, for steps other
    /// coordinators place here.
    pub fn with_repositories(mut self, repositories: BTreeMap<String, PathBuf>) -> Self {
        self.turns = self.turns.with_repositories(repositories);
        self
    }
    pub fn records(&self) -> &SeatRecords {
        &self.records
    }
    pub fn engine(&self) -> &Arc<dyn Sessions> {
        &self.engine
    }
    pub fn roots(&self) -> &TranscriptRoots {
        &self.roots
    }
    /// Create once, or verify the identity of the existing owned seat. A
    /// kept seat is the agent's own session, one a run started: a live one
    /// is used as it is, unless another dispatch holds it, when this one
    /// gets its own.
    pub async fn dispatch(&self, request: Dispatch) -> Result<String, crate::Error> {
        let id = request.id.clone();
        let answer = self.dispatch_held(request).await;
        if answer.is_err() {
            self.release(&id);
        }
        answer
    }
    async fn dispatch_held(&self, request: Dispatch) -> Result<String, crate::Error> {
        (async {
            let _creation = self.creation.lock().await;
            let hash = hex::encode(Sha256::digest(request.id.as_bytes()));
            let request = match self.records.dispatch(&request.id) {
                // A retry is the dispatch as first recorded.
                Ok(recorded) => recorded,
                Err(_) => match self.own_name(request, &hash[..4]).await? {
                    request if request.keep => self.hold(request, &hash[..4]),
                    request => request,
                },
            };
            self.records.create(&request).map_err(|e| e.to_string())?;
            let _guard = self.creation_guard(&request.id).await?;
            if let Some(seat) = self.records.seat(&request.id).map_err(|e| e.to_string())? {
                let ended = self
                    .records
                    .events(&request.id, 0)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|event| event.key == "seat-cleanup");
                let completed = self
                    .records
                    .result(&request.id)
                    .map_err(|e| e.to_string())?
                    .is_some();
                if !ended && !completed {
                    self.engine
                        .verify(&seat.session)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                self.record_created(&request.id, &seat)?;
                return Ok(request.id);
            }
            let (_, adapter) = adapter(&request.harness)?;
            let shell = if cfg!(windows) {
                Shell::PowerShell
            } else {
                Shell::Posix
            };
            let binding = SeatBinding {
                model: request.model.clone(),
                effort: request.effort.clone(),
            };
            let launch = adapter
                .launch(&binding, &request.cwd, shell, &self.roots)
                .map_err(|e| e.to_string())?;
            // Only a seat is used: a session a run created under this name,
            // the agent's own or one an earlier attempt created and crashed
            // before binding. A session that merely has the name is not.
            let name = if request.name.is_empty() {
                format!(
                    "ahjo-{}-{}-{}",
                    component(&request.run_id),
                    component(&request.seat_id),
                    &hash[..8]
                )
            } else {
                request.name.clone()
            };
            if let Some(session) = self.engine.seat(&name).await.map_err(|e| e.to_string())? {
                let cwd = if request.keep {
                    self.folder(&session).await
                } else {
                    None
                };
                return self.bind(&request.id, name, session, cwd, false).await;
            }
            let mut env = BTreeMap::new();
            if request.harness == "claude-code" {
                // Claude's unstyled ghost text is indistinguishable from an
                // operator's unsent input on a terminal screen.
                env.insert(
                    "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION".into(),
                    "false".into(),
                );
            }
            let session = self
                .engine
                .create(SessionSpec {
                    name: name.clone(),
                    cwd: request.cwd.clone(),
                    env,
                    command: command(launch),
                    cols: 160,
                    rows: 48,
                })
                .await
                .map_err(|e| e.to_string())?;
            self.bind(&request.id, name, session, None, true).await
        })
        .await
        .map_err(|message: String| crate::Error::new("seat_error", message))
    }
    /// A session a run did not create is never a step's to take over, even
    /// under the agent's name: the dispatch then works in a session of its
    /// own beside it, ended with it.
    async fn own_name(&self, mut request: Dispatch, tag: &str) -> Result<Dispatch, String> {
        if request.name.is_empty() {
            return Ok(request);
        }
        let seat = self.engine.seat(&request.name).await;
        if seat.map_err(|e| e.to_string())?.is_none() {
            let other = self.engine.find(&request.name).await;
            if other.map_err(|e| e.to_string())?.is_some() {
                request.name = format!("{}-{tag}", request.name);
                request.keep = false;
            }
        }
        Ok(request)
    }
    /// Claims a kept seat's session for this dispatch; one another open
    /// dispatch holds leaves this one a session of its own, ended with it.
    fn hold(&self, mut request: Dispatch, tag: &str) -> Dispatch {
        let Ok(mut held) = self.kept.lock() else {
            return request;
        };
        match held.get(&request.name) {
            Some(holder) if *holder != request.id => {
                request.name = format!("{}-{tag}", request.name);
                request.keep = false;
            }
            _ => {
                held.insert(request.name.clone(), request.id.clone());
            }
        }
        request
    }
    /// Lets go of the kept session a dispatch held.
    fn release(&self, id: &str) {
        if let Ok(mut held) = self.kept.lock() {
            held.retain(|_, holder| holder != id);
        }
    }
    /// The folder a live session's harness runs in, as tmux last reported it.
    async fn folder(&self, session: &ahjo_term::SessionRef) -> Option<PathBuf> {
        let listed = self.engine.listed().await.ok()?;
        let info = listed.iter().find(|s| s.id == session.session_id)?;
        let path = &info.facts.as_ref()?.path;
        (!path.is_empty()).then(|| standing(path))
    }
    async fn bind(
        &self,
        id: &str,
        name: String,
        session: ahjo_term::SessionRef,
        cwd: Option<PathBuf>,
        started: bool,
    ) -> Result<String, String> {
        let seat = SeatId {
            node: self.node.clone(),
            session_name: name,
            session,
            cwd,
        };
        if let Err(error) = self.records.bind_seat(id, seat.clone()) {
            // Only a session this dispatch started is its to end.
            let cleanup = if started {
                format!("{:?}", self.engine.kill(&seat.session).await)
            } else {
                "the session was already there and stays".into()
            };
            return Err(format!(
                "cannot persist owned seat: {error}; cleanup: {cleanup}"
            ));
        }
        self.record_created(id, &seat)?;
        Ok(id.to_owned())
    }
    fn record_created(&self, id: &str, seat: &SeatId) -> Result<(), String> {
        let dispatch = self.records.dispatch(id).map_err(|e| e.to_string())?;
        let data = json!({"dispatch_id":id,"node":seat.node,"session_name":seat.session_name,"session_id":seat.session.session_id,"pane_id":seat.session.pane_id,"generation":seat.session.generation,"harness":dispatch.harness,"model":dispatch.model,"effort":dispatch.effort});
        self.records
            .append(
                id,
                "seat-created",
                "seat_created",
                data.as_object().cloned().unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Bind a correction dispatch to the same live seat.
    pub async fn reuse(&self, request: &Dispatch, original: &str) -> Result<(), crate::Error> {
        (async {
            let _creation = self.creation.lock().await;
            self.records.create(request).map_err(|e| e.to_string())?;
            let _guard = self.creation_guard(&request.id).await?;
            let seat = self
                .records
                .seat(original)
                .map_err(|e| e.to_string())?
                .ok_or("original dispatch has no seat")?;
            self.engine
                .verify(&seat.session)
                .await
                .map_err(|e| e.to_string())?;
            self.records
                .bind_seat(&request.id, seat)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|message: String| crate::Error::new("seat_error", message))
    }
    /// End only the recorded server generation; retries return the first outcome.
    pub async fn end_seat(&self, dispatch_id: &str, cause: &str) -> Result<String, crate::Error> {
        (async {
        let _creation = self.creation.lock().await;
        let _guard = self.creation_guard(dispatch_id).await?;
        if let Some(event) = self
            .records
            .events(dispatch_id, 0)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|event| event.key == "seat-cleanup")
        {
            return Ok(event
                .data
                .get("outcome")
                .and_then(|value| value.as_str())
                .unwrap_or("left_cleanup_failed")
                .into());
        }
        let seat = self
            .records
            .seat(dispatch_id)
            .map_err(|e| e.to_string())?
            .ok_or("dispatch has no seat")?;
        let keep = self.records.dispatch(dispatch_id).is_ok_and(|d| d.keep);
        let found = self.engine.find(&seat.session_name).await;
        let (outcome, detail) = match found {
            // The agent's own session stays for the next run.
            Ok(Some(_)) if keep => ("kept", String::new()),
            Ok(None) => ("gone", String::new()),
            Ok(Some(current)) if current != seat.session => ("left_socket_changed", "seat identity changed".into()),
            Err(error) => ("left_cleanup_failed", error.to_string()),
            Ok(Some(_)) => match self.engine.kill(&seat.session).await {
                Ok(()) => ("ended", String::new()),
                Err(error) if error.to_string().contains("generation") => ("left_socket_changed", error.to_string()),
                Err(error) => ("left_cleanup_failed", error.to_string()),
            },
        };
        let data = json!({"dispatch_id":dispatch_id,"outcome":outcome,"cause":cause,"detail":detail,"session_name":seat.session_name});
        self.records
            .append(
                dispatch_id,
                "seat-cleanup",
                "seat_cleanup",
                data.as_object().cloned().unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
        self.release(dispatch_id);
        Ok(outcome.into())
     }).await.map_err(|message: String| crate::Error::new("seat_error", message))
    }
    pub(super) fn detach(&self, dispatch_id: &str) -> Result<String, String> {
        let seat = self
            .records
            .seat(dispatch_id)
            .map_err(|e| e.to_string())?
            .ok_or("dispatch has no seat")?;
        let data = json!({"dispatch_id":dispatch_id,"outcome":"left_shutdown","cause":"shutdown","session_name":seat.session_name});
        self.records
            .append(
                dispatch_id,
                "seat-detached",
                "seat_cleanup",
                data.as_object().cloned().unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
        Ok("left_shutdown".into())
    }
    async fn creation_guard(&self, id: &str) -> Result<std::fs::File, String> {
        let records = self.records.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || records.creation_guard(&id))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }
}
/// The folder a process stands in, from what the system says of it. Linux
/// names a removed one with ` (deleted)` after it, as an agent's is once
/// the checkout its first attempt worked in is gone; its harness still
/// keeps its record under the folder's own name.
fn standing(path: &str) -> PathBuf {
    match path.strip_suffix(" (deleted)") {
        Some(gone) if !std::path::Path::new(path).exists() => PathBuf::from(gone),
        _ => PathBuf::from(path),
    }
}
/// What the seat's session runs. On Windows the harness runs in PowerShell,
/// which finds npm's `.cmd` and `.ps1` shims as a person's prompt would;
/// the command is passed encoded, so no quoting layer can split it.
/// On Unix it runs under `env -u` for the variables by which a harness marks
/// its own child processes: a node started from inside an agent's session
/// would otherwise hand them to every seat, and a Claude seat that believes
/// it is a child keeps no transcript, so its turn could never be seen to end.
fn command(launch: crate::Launch) -> Vec<String> {
    if !cfg!(windows) {
        let mut argv = vec!["env".to_owned()];
        for name in INHERITED_MARKERS {
            argv.extend(["-u".to_owned(), (*name).to_owned()]);
        }
        argv.push("--".into());
        argv.extend(launch.argv);
        return argv;
    }
    let utf16: Vec<u8> = launch
        .command
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    vec![
        "powershell.exe".into(),
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-EncodedCommand".into(),
        base64(&utf16),
    ]
}
const INHERITED_MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SSE_PORT",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CODEX_THREAD_ID",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    "AI_AGENT",
];
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
pub(crate) fn adapter(name: &str) -> Result<(Harness, Arc<dyn HarnessAdapter>), String> {
    match name {
        "claude-code" => Ok((Harness::Claude, Arc::new(Claude))),
        "openai-codex" => Ok((Harness::Codex, Arc::new(Codex))),
        "pi" => Ok((Harness::Pi, Arc::new(Pi))),
        _ => Err(format!("unknown harness {name:?}")),
    }
}
fn component(text: &str) -> String {
    let value: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if value.is_empty() {
        "seat".into()
    } else {
        value
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn encodes_base64_with_padding() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
        assert_eq!(super::base64(b""), "");
    }
    #[test]
    fn a_removed_folder_keeps_its_own_name() {
        let dir = ahjo_testkit::fresh_dir("standing");
        let named = dir.join("odd (deleted)");
        std::fs::create_dir_all(&named).unwrap();
        // A folder that is really called that stays as it is.
        assert_eq!(super::standing(named.to_str().unwrap()), named);
        let gone = format!("{} (deleted)", dir.join("turn-1").display());
        assert_eq!(super::standing(&gone), dir.join("turn-1"));
        assert_eq!(super::standing("/w"), std::path::PathBuf::from("/w"));
    }
    #[cfg(unix)]
    #[test]
    fn a_seat_does_not_inherit_the_markers_of_the_agent_that_started_its_node() {
        let argv = super::command(crate::Launch {
            command: String::new(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "echo ${CLAUDECODE:-none} ${CLAUDE_CODE_CHILD_SESSION:-none} $KEPT".into(),
            ],
        });
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env("CLAUDECODE", "1")
            .env("CLAUDE_CODE_CHILD_SESSION", "1")
            .env("KEPT", "kept")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "none none kept"
        );
    }
}
