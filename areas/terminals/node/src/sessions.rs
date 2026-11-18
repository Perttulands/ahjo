//! The terminals area as other areas reach it ([`ahjo_term::Sessions`]).

use std::sync::Arc;

use ahjo_term::backend::Reply;
use ahjo_term::control::PaneId;
use ahjo_term::engine::{Delivering, PaneStream, SessionInfo};
use ahjo_term::{Generation, Renames, Screen, SessionRef, SessionSpec, Sessions, Start};
use anyhow::{Context, bail};
use tokio::sync::watch;

use crate::hub::Hub;
use crate::launch::{Config, Harness, Launch};
use crate::launcher::Launcher;

pub(crate) struct Provided {
    pub hub: Arc<Hub>,
    pub launcher: Arc<Launcher>,
}

impl Sessions for Provided {
    fn list(&self) -> Vec<SessionInfo> {
        self.hub.sessions()
    }

    fn listed(&self) -> Reply<'_, Vec<SessionInfo>> {
        Box::pin(async move {
            self.hub.ready().await?;
            Ok(self.hub.sessions())
        })
    }

    fn users(&self) -> Vec<String> {
        self.hub.users()
    }

    fn watch(&self) -> watch::Receiver<u64> {
        self.hub.watch()
    }

    fn renames(&self) -> Vec<(Option<String>, Renames)> {
        self.hub.renames()
    }

    /// CHROTE's pane for an unattended send.
    fn active_pane<'a>(&'a self, session: &'a str) -> Reply<'a, PaneId> {
        Box::pin(self.hub.active_pane(session))
    }

    fn local_pane(&self, pane: PaneId) -> PaneId {
        self.hub.pane(pane).map_or(pane, |(_, id)| id)
    }

    fn deliver(&self, pane: PaneId, text: String, enter: bool) -> Reply<'_, Delivering> {
        Box::pin(self.hub.deliver(pane, text, enter))
    }

    fn start(&self, start: Start) -> Reply<'_, String> {
        Box::pin(async move {
            let launcher = self.launcher.get();
            let config = Config {
                harnesses: vec![Harness {
                    id: "start".into(),
                    command: start.command,
                    flags: String::new(),
                }],
                folders: Vec::new(),
                roots: Vec::new(),
                cols: launcher.cols,
                rows: launcher.rows,
            };
            let launch = Launch {
                harness: "start".into(),
                folder: start.folder,
                name: start.name,
                flags: String::new(),
                user: start.user,
                notify: false,
                size: None,
            };
            let (session, _) = self.hub.launch(&config, &launch).await?;
            Ok(session)
        })
    }

    fn create(&self, spec: SessionSpec) -> Reply<'_, SessionRef> {
        Box::pin(async move { self.hub.own_engine().create_session(spec).await })
    }

    fn find<'a>(&'a self, name: &'a str) -> Reply<'a, Option<SessionRef>> {
        Box::pin(async move { self.hub.own_engine().find_session(name).await })
    }

    fn seat<'a>(&'a self, name: &'a str) -> Reply<'a, Option<SessionRef>> {
        Box::pin(async move { self.hub.own_engine().find_seat(name).await })
    }

    fn verify<'a>(&'a self, seat: &'a SessionRef) -> Reply<'a, ()> {
        Box::pin(async move { self.hub.own_engine().verify_session(seat).await })
    }

    fn kill<'a>(&'a self, seat: &'a SessionRef) -> Reply<'a, ()> {
        Box::pin(async move { self.hub.own_engine().kill_session(seat).await })
    }

    fn follow(&self, pane: PaneId) -> Option<PaneStream> {
        self.hub.subscribe(pane)
    }

    fn screen(&self, pane: PaneId) -> Option<Screen> {
        let (server, local) = self.hub.pane(pane)?;
        server.engine.read_screen(local)
    }

    fn input(&self, pane: PaneId, bytes: Vec<u8>) -> Reply<'_, ()> {
        Box::pin(self.hub.send_input(pane, bytes))
    }

    fn pid(&self, pane: PaneId) -> Reply<'_, u32> {
        Box::pin(async move {
            let (server, local) = self.hub.pane(pane).context("unknown pane")?;
            let target = format!("%{local}");
            let pid = server
                .backend()
                .command(&["display-message", "-p", "-t", &target, "#{pane_pid}"])
                .await?;
            pid.trim()
                .parse()
                .with_context(|| format!("pane {pane} has no process id: {pid:?}"))
        })
    }

    fn server<'a>(&'a self, session: &'a str) -> Reply<'a, Generation> {
        Box::pin(async move {
            let (server, local) = self.hub.session(session)?;
            let said = server
                .backend()
                .command(&["display-message", "-p", "-t", local, "#{pid} #{start_time}"])
                .await?;
            let mut words = said.split_whitespace().map(str::parse::<u64>);
            match (words.next(), words.next(), words.next()) {
                (Some(Ok(pid)), Some(Ok(start_time)), None) => Ok(Generation {
                    pid: u32::try_from(pid)?,
                    start_time,
                }),
                _ => {
                    bail!("the server of session {session} did not say its pid and start: {said:?}")
                }
            }
        })
    }
}
